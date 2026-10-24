//! Git commands run from the app.

use super::*;

impl App {
    pub(super) fn git_refresh(&mut self) {
        let Some(selected) = self.repo.clone() else { return };
        let repos = self.repos.clone();
        self.jobs.spawn(move || {
            let statuses = repos.iter().map(|r| (r.clone(), git::status(r))).collect();
            let remotes = repos.iter().map(|r| (r.clone(), git::stdout(r, &["remote", "-v"]).unwrap_or_default())).collect();
            let stashes = git::stashes(&selected);
            JobResult::GitStatus { selected, statuses, remotes, stashes }
        });
    }

    /// Commits with the message typed in the Git view (everything tracked when nothing is staged).
    pub(super) fn commit(&mut self, n_staged: usize) {
        let msg = self.git.commit_msg.trim().to_string();
        let mut args = vec!["commit"];
        if n_staged == 0 {
            args.push("-a");
        }
        args.extend(["-m", &msg]);
        self.git_job("commit", &args);
        self.git.commit_msg.clear();
    }

    pub(super) fn git_job(&mut self, label: &str, args: &[&str]) {
        let Some(repo) = self.repo.clone() else {
            self.set_status(t!("Pas de dépôt git", "No git repository"));
            return;
        };
        self.git_job_at(repo, label, args);
    }

    pub(super) fn git_job_at(&mut self, repo: PathBuf, label: &str, args: &[&str]) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.log(format!("$ git {}", args.join(" ")));
        self.set_status(format!("git {label}…"));
        self.git.busy += 1;
        let label = label.to_string();
        self.jobs.spawn(move || {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            let result = git::run(&repo, &a);
            JobResult::GitOp { label, result, status: Some((repo.clone(), git::status(&repo))) }
        });
    }

    /// The graph view on the staged / unstaged changes, scrolled to the diff of `rel`.
    /// The git view (tab): branch, remote actions and commit on top, then the graph and diffs.
    pub(super) fn open_git_view(&mut self) {
        if self.bottom == Some(BottomTab::Git) {
            self.bottom = None;
        }
        self.show_tab(document::View::Git);
        self.git_view_load();
    }

    /// The graph read (again if the repository changed) and the status, wherever the view is shown.
    pub(super) fn git_view_load(&mut self) {
        if self.history.graph.is_none() || self.history.repo != self.repo {
            self.load_history();
        }
        self.git_refresh();
    }

    /// The git view is shown, in the editor area or in the bottom panel.
    pub(super) fn git_shown(&self) -> bool {
        self.has_tab(document::View::Git) || self.bottom == Some(BottomTab::Git)
    }

    pub(super) fn show_diff(&mut self, rel: String, untracked: bool) {
        let Some(repo) = self.repo.clone() else { return };
        if untracked {
            self.open_path(repo.join(&rel));
            return;
        }
        self.jobs.spawn(move || {
            let text = git::run(&repo, &["diff", "HEAD", "--", &rel])
                .or_else(|_| git::run(&repo, &["diff", "--cached", "--", &rel]))
                .unwrap_or_else(|e| e);
            let text = if text.is_empty() { t!("(aucune différence)", "(no difference)").into() } else { text };
            JobResult::NewTab { title: format!("diff : {rel}"), text, syntax: "Diff".into() }
        });
    }

    /// Repositories changed outside Florm (commit, checkout, add… in a terminal): refresh the git
    /// panel, the graph and the gutter marks of their open files.
    pub(super) fn check_git_stamps(&mut self) {
        let mut changed = Vec::new();
        for r in &self.repos {
            let st = git::stamp(r);
            match self.git_stamps.insert(r.clone(), st.clone()) {
                Some(old) if old != st => changed.push(r.clone()),
                _ => {}
            }
        }
        if changed.is_empty() {
            return;
        }
        self.git_refresh();
        if self.git_shown() && self.history.repo.as_ref().is_some_and(|r| changed.contains(r)) && self.history.rebase.is_none() {
            self.load_history();
            if let Some(staged) = self.history.wip_sel {
                self.load_wip_all(staged);
            }
        }
        for i in 0..self.docs.len() {
            let in_repo = self.docs[i].path.as_deref().and_then(|p| self.repo_for(p)).is_some_and(|r| changed.contains(&r));
            if in_repo {
                self.refresh_git_base(i);
            }
        }
    }

    /// Re-reads the HEAD version of document `i` (gutter marks), without linting it.
    fn refresh_git_base(&mut self, i: usize) {
        let Some(d) = self.docs.get(i) else { return };
        let Some(path) = d.path.clone() else { return };
        let (uid, version) = (d.uid, d.version);
        self.jobs.spawn(move || JobResult::Analyzed { uid, version, reload: false, diags: None, base: git::head_text(&path), log: Vec::new() });
    }
}

/// What the Stash menu of the Git view asked for (a stash is named by its commit).
pub(super) enum StashAct {
    Push,
    Show(String, String),
    Apply(String),
    Pop(String),
    /// Asks first.
    AskDrop(String, String),
    Drop(String),
}

impl App {
    /// The stashes of the repository, read again.
    pub(super) fn stash_reload(&mut self) {
        self.git.stashes = self.repo.as_deref().map(git::stashes).unwrap_or_default();
    }

    pub(super) fn stash_act(&mut self, act: StashAct) {
        let Some(repo) = self.repo.clone() else { return };
        let (verb, hash) = match act {
            StashAct::Push => {
                let msg = std::mem::take(&mut self.git.stash_msg);
                let mut args = vec!["stash", "push"];
                if !self.git.stash_tracked_only {
                    args.push("--include-untracked");
                }
                if !msg.trim().is_empty() {
                    args.extend(["-m", msg.trim()]);
                }
                self.git_job_at(repo, "stash", &args);
                return;
            }
            StashAct::Show(hash, what) => {
                self.start_compare(format!("{hash}^1"), hash, format!("Stash : {what}"));
                return;
            }
            StashAct::AskDrop(hash, what) => {
                self.confirm = Some(Confirm::DropStash(hash, what));
                return;
            }
            StashAct::Apply(hash) => ("apply", hash),
            StashAct::Pop(hash) => ("pop", hash),
            StashAct::Drop(hash) => ("drop", hash),
        };
        self.log(format!("$ git stash {verb} {hash}"));
        self.set_status(format!("git stash {verb}…"));
        self.git.busy += 1;
        self.jobs.spawn(move || {
            let result = git::stash_op(&repo, verb, &hash);
            JobResult::GitOp { label: format!("stash {verb}"), result, status: Some((repo.clone(), git::status(&repo))) }
        });
    }
}
