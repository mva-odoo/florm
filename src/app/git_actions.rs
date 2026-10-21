//! Git commands run from the app.

use super::*;

impl App {
    pub(super) fn git_refresh(&mut self) {
        let Some(selected) = self.repo.clone() else { return };
        let repos = self.repos.clone();
        self.jobs.spawn(move || {
            let statuses = repos.iter().map(|r| (r.clone(), git::status(r))).collect();
            let remotes = repos.iter().map(|r| (r.clone(), git::stdout(r, &["remote", "-v"]).unwrap_or_default())).collect();
            JobResult::GitStatus { selected, statuses, remotes }
        });
    }

    pub(super) fn git_job(&mut self, label: &str, args: &[&str]) {
        let Some(repo) = self.repo.clone() else {
            self.set_status("Pas de dépôt git");
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
            JobResult::GitOp { label, result: git::run(&repo, &a) }
        });
    }

    /// The graph view on the staged / unstaged changes, scrolled to the diff of `rel`.
    /// The git view (tab): branch, remote actions and commit on top, then the graph and diffs.
    pub(super) fn open_git_view(&mut self) {
        if let Some(sp) = self.split.as_mut() {
            sp.shown = false;
        }
        self.history.open = true;
        self.prefs.open = false;
        if self.history.graph.is_none() || self.history.repo != self.repo {
            self.load_history();
        }
        self.git_refresh();
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
            let text = if text.is_empty() { "(aucune différence)".into() } else { text };
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
        if self.history.open && self.history.repo.as_ref().is_some_and(|r| changed.contains(r)) && self.history.rebase.is_none() {
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
