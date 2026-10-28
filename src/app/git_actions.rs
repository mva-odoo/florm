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
    /// Amending, only the staged changes join the last commit (none: its message changes).
    pub(super) fn commit(&mut self, n_staged: usize) {
        let msg = self.git.commit_msg.trim().to_string();
        let mut args = vec!["commit"];
        if self.git.amend {
            args.push("--amend");
        } else if n_staged == 0 {
            args.push("-a");
        }
        args.extend(["-m", &msg]);
        self.git_job(if self.git.amend { "commit --amend" } else { "commit" }, &args);
        self.git.commit_msg.clear();
        self.git.amend = false;
    }

    /// "Modifier le dernier commit": its message in the box, to change; the commit button then
    /// replaces it.
    pub(super) fn start_amend(&mut self) {
        let Some(repo) = self.repo.clone() else { return };
        self.git.amend = true;
        self.git.focus_commit = true;
        self.jobs.spawn(move || JobResult::LastCommitMessage(git::stdout(&repo, &["log", "-1", "--format=%B"]).map(|m| m.trim().to_string())));
    }

    /// Several git commands one after the other (stops at the first that fails), as one operation.
    pub(super) fn git_seq_at(&mut self, repo: PathBuf, label: &str, cmds: Vec<Vec<String>>) {
        for c in &cmds {
            self.log(format!("$ git {}", c.join(" ")));
        }
        self.set_status(format!("git {label}…"));
        self.git.busy += 1;
        let label = label.to_string();
        self.jobs.spawn(move || {
            let mut out = Vec::new();
            let mut result = Ok(String::new());
            for c in &cmds {
                let a: Vec<&str> = c.iter().map(String::as_str).collect();
                match git::run(&repo, &a) {
                    Ok(o) => out.push(o),
                    Err(e) => {
                        out.push(e);
                        result = Err(out.join("\n").trim().to_string());
                        break;
                    }
                }
            }
            let result = result.map(|_| out.join("\n").trim().to_string());
            JobResult::GitOp { label, result, status: Some((repo.clone(), git::status(&repo))) }
        });
    }

    /// Asks before git commands that lose something (or rewrite what was pushed).
    pub(super) fn confirm_git(&mut self, title: &str, msg: String, yes: &str, label: &str, cmds: Vec<Vec<String>>, alt: Option<(String, Vec<Vec<String>>)>) {
        self.confirm = Some(Confirm::Git { title: title.into(), msg, yes: yes.into(), label: label.into(), cmds, alt });
    }

    /// The remote to push to: the one the current branch tracks, else "origin", else the first.
    pub(super) fn default_remote(&self) -> Option<String> {
        let names: Vec<String> = self.git.remotes.iter().map(|(n, _)| n.clone()).collect();
        let up = self.git.status.as_ref().and_then(|s| s.upstream()).and_then(|(u, ..)| git::split_remote(&names, &u)).map(|(r, _)| r);
        up.or_else(|| names.iter().find(|n| *n == "origin").cloned()).or_else(|| names.first().cloned())
    }

    /// `git push --force-with-lease`: after a rebase or an amend, the remote branch is replaced,
    /// unless someone pushed to it meanwhile.
    pub(super) fn force_push(&mut self) {
        if self.repo.is_none() {
            return;
        }
        let branch = self.git.status.as_ref().map(|s| s.branch.clone()).unwrap_or_default();
        let remote = self.default_remote().unwrap_or_else(|| "origin".into());
        let cmd = vec!["push".into(), "--force-with-lease".into(), "-u".into(), remote.clone(), "HEAD".into()];
        self.confirm_git(
            t!("Push forcé", "Force push"),
            tf!(
                "Remplacer la branche « {branch} » de {remote} par la tienne ?\nLes commits qui n'y sont que là-bas (réécrits par un rebase ou un amend) disparaissent de {remote}.\n--force-with-lease refuse si quelqu'un d'autre y a poussé entre-temps.",
                "Replace the branch “{branch}” of {remote} with yours?\nThe commits only there (rewritten by a rebase or an amend) disappear from {remote}.\n--force-with-lease refuses if someone else pushed to it meanwhile."
            ),
            t!("Forcer le push", "Force the push"),
            "push --force-with-lease",
            vec![cmd],
            None,
        );
    }

    /// The branches read, then what was asked done with `target` (or the list shown to choose).
    pub(super) fn pick_branch(&mut self, pick: BranchPick, target: Option<String>) {
        let Some(repo) = self.repo.clone() else {
            self.set_status(t!("Pas de dépôt git", "No git repository"));
            return;
        };
        self.jobs.spawn(move || JobResult::GitBranches(git::branch_list(&repo), pick, target));
    }

    pub(super) fn branches_ready(&mut self, list: Vec<git::Branch>, pick: BranchPick, target: Option<String>) {
        if let Some(t) = target {
            match list.into_iter().find(|b| b.name == t) {
                Some(b) => self.branch_act(b, pick),
                None => self.set_error(tf!("La branche « {t} » n'existe plus", "The branch “{t}” no longer exists")),
            }
            return;
        }
        let current = self.git.status.as_ref().map(|s| s.branch.clone()).unwrap_or_default();
        let items: Vec<Item> = list
            .into_iter()
            // what makes no sense on the current branch, and renaming a remote one
            .filter(|b| pick == BranchPick::Checkout || b.name != current)
            .filter(|b| pick != BranchPick::Rename || !b.remote)
            .map(|b| {
                let detail = if b.remote { t!("distante", "remote").to_string() } else if b.upstream.is_empty() { String::new() } else { tf!("suit {}", "tracks {}", b.upstream) };
                Item { label: b.name.clone(), detail, action: Action::Branch(b, pick) }
            })
            .collect();
        self.open_overlay_with(Mode::Branches(pick), items, String::new());
    }

    /// A branch chosen for `pick`.
    pub(super) fn branch_act(&mut self, b: git::Branch, pick: BranchPick) {
        let Some(repo) = self.repo.clone() else { return };
        let names: Vec<String> = self.git.remotes.iter().map(|(n, _)| n.clone()).collect();
        let name = b.name.clone();
        match pick {
            BranchPick::Checkout => {
                // a remote branch: the local one of the same name (git creates it, tracking it)
                let target = if b.remote { git::split_remote(&names, &name).map(|(_, n)| n).unwrap_or(name) } else { name };
                self.git_job_at(repo, "checkout", &["checkout", &target]);
            }
            BranchPick::Merge => self.git_job_at(repo, "merge", &["-c", "core.editor=true", "merge", "--no-edit", &name]),
            BranchPick::Rebase => self.git_job_at(repo, "rebase", &["-c", "core.editor=true", "rebase", &name]),
            BranchPick::Rename => self.open_overlay_with(Mode::Prompt(Prompt::RenameBranch { old: name.clone(), upstream: b.upstream }), Vec::new(), name),
            BranchPick::Delete if b.remote => self.ask_delete_remote_branch(&name),
            BranchPick::Delete => {
                let local = vec!["branch".to_string(), "-D".into(), name.clone()];
                let alt = git::split_remote(&names, &b.upstream).map(|(r, rb)| {
                    (tf!("Supprimer aussi {}", "Delete {} too", b.upstream), vec![local.clone(), vec!["push".into(), r, "--delete".into(), rb]])
                });
                self.confirm_git(
                    t!("Supprimer la branche", "Delete the branch"),
                    tf!(
                        "Supprimer la branche locale « {name} » ?\nSes commits qui ne sont dans aucune autre branche restent récupérables quelque temps par le reflog.",
                        "Delete the local branch “{name}”?\nIts commits that are in no other branch can still be brought back for a while from the reflog."
                    ),
                    t!("Supprimer en local", "Delete locally"),
                    "branch -D",
                    vec![local],
                    alt,
                );
            }
        }
    }

    fn ask_delete_remote_branch(&mut self, full: &str) {
        let names: Vec<String> = self.git.remotes.iter().map(|(n, _)| n.clone()).collect();
        let Some((remote, rb)) = git::split_remote(&names, full) else {
            self.set_error(tf!("Remote inconnu pour « {full} »", "Unknown remote for “{full}”"));
            return;
        };
        self.confirm_git(
            t!("Supprimer la branche distante", "Delete the remote branch"),
            tf!("Supprimer « {rb} » sur {remote}, pour tout le monde ?", "Delete “{rb}” on {remote}, for everyone?"),
            t!("Supprimer sur le remote", "Delete on the remote"),
            "push --delete",
            vec![vec!["push".into(), remote, "--delete".into(), rb]],
            None,
        );
    }

    /// The actions of the commit menu that are not about showing something.
    pub(super) fn hist_git_act(&mut self, a: HistAct, repo: PathBuf) {
        let cur = self.git.status.as_ref().map(|s| s.branch.clone()).unwrap_or_default();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        match a {
            HistAct::Merge(target) => self.git_job_at(repo, "merge", &["-c", "core.editor=true", "merge", "--no-edit", &target]),
            HistAct::Reset(hash, "--hard") => {
                let short = &hash[..hash.len().min(8)];
                self.confirm_git(
                    t!("Reset --hard", "Reset --hard"),
                    tf!(
                        "Ramener « {cur} » sur {short} et jeter toutes les modifications non commitées ?\nLes commits d'après restent récupérables par le reflog ; les modifications non commitées, non.",
                        "Move “{cur}” to {short} and throw away every uncommitted change?\nThe commits after it can be brought back from the reflog; the uncommitted changes cannot."
                    ),
                    t!("Tout jeter et ramener la branche", "Throw away and move the branch"),
                    "reset --hard",
                    vec![s(&["reset", "--hard", &hash])],
                    None,
                );
            }
            HistAct::Reset(hash, mode) => self.git_job_at(repo, "reset", &["reset", mode, &hash]),
            HistAct::RenameBranch(name) => self.pick_branch(BranchPick::Rename, Some(name)),
            HistAct::DeleteBranch(name) => self.pick_branch(BranchPick::Delete, Some(name)),
            HistAct::DeleteRemoteBranch(full) => self.ask_delete_remote_branch(&full),
            HistAct::TagAt(hash) => self.open_overlay_with(Mode::Prompt(Prompt::TagAt(hash)), Vec::new(), String::new()),
            HistAct::PushTag(tag) => match self.default_remote() {
                Some(r) => self.git_job_at(repo, "push tag", &["push", &r, &format!("refs/tags/{tag}")]),
                None => self.set_error(t!("Aucun remote où pousser", "No remote to push to")),
            },
            HistAct::DeleteTag(tag) => {
                let local = s(&["tag", "-d", &tag]);
                let alt = self.default_remote().map(|r| (tf!("Supprimer aussi sur {r}", "Delete on {r} too"), vec![local.clone(), s(&["push", &r, "--delete", &format!("refs/tags/{tag}")])]));
                self.confirm_git(t!("Supprimer le tag", "Delete the tag"), tf!("Supprimer le tag « {tag} » ?", "Delete the tag “{tag}”?"), t!("Supprimer en local", "Delete locally"), "tag -d", vec![local], alt);
            }
            HistAct::Bisect(hash, verb) => {
                let mark = s(&["bisect", verb, &hash]);
                if self.git.status.as_ref().is_some_and(|s| s.bisecting) {
                    self.git_seq_at(repo, "bisect", vec![mark]);
                } else {
                    self.git_seq_at(repo, "bisect", vec![s(&["bisect", "start"]), mark]);
                }
            }
            _ => {}
        }
    }

    /// The rules Claude follows for the commit messages of this project (`.florm/commit.md`),
    /// created with an example the first time.
    pub(super) fn open_commit_rules(&mut self) {
        let Some(repo) = self.repo.clone() else {
            self.set_status(t!("Pas de dépôt git", "No git repository"));
            return;
        };
        let path = crate::claude::commit_rules_path(&repo);
        if !path.exists() {
            // what the CLAUDE.md already says about commits, as a start
            let current = crate::claude::commit_rules(&repo);
            let body = if current.is_empty() {
                t!(
                    "- En français.\n- Sujet : `[TAG] module : ce qui change`, tags [ADD] (nouveauté), [IMP] (amélioration), [FIX] (correction).\n- Sujet de 72 caractères au plus, puis une ligne vide et le pourquoi.\n",
                    "- In English.\n- Subject: `[TAG] module: what changes`, tags [ADD] (new), [IMP] (improvement), [FIX] (fix).\n- Subject of at most 72 characters, then an empty line and the why.\n"
                )
                .to_string()
            } else {
                format!("{current}\n")
            };
            let head = t!(
                "<!-- Consignes que Claude suit pour écrire les messages de commit de ce projet (✨ et ✂ de la vue Git). -->\n<!-- Les lignes <!-- … --> sont ignorées. Fichier à commiter pour le partager avec l'équipe. -->\n\n",
                "<!-- Rules Claude follows when writing the commit messages of this project (✨ and ✂ of the Git view). -->\n<!-- The <!-- … --> lines are ignored. Commit this file to share it with the team. -->\n\n"
            );
            let write = std::fs::create_dir_all(path.parent().unwrap_or(&repo)).and_then(|()| std::fs::write(&path, format!("{head}{body}")));
            if let Err(e) = write {
                self.set_error(tf!("Création de {} impossible : {e}", "Cannot create {}: {e}", path.display()));
                return;
            }
        }
        self.open_path(path);
        self.set_status(t!("Consignes des messages de commit : Claude les suit pour ✨ et ✂", "Commit message rules: Claude follows them for ✨ and ✂"));
    }

    /// The untracked files listed, then deleted once confirmed (`git clean`).
    pub(super) fn ask_clean(&mut self) {
        let Some(repo) = self.repo.clone() else { return };
        self.jobs.spawn(move || JobResult::CleanPreview(git::run(&repo, &["clean", "-n", "-d"])));
    }

    pub(super) fn clean_preview_ready(&mut self, r: Result<String, String>) {
        let list = match r {
            Ok(l) => l,
            Err(e) => {
                self.set_error(e);
                return;
            }
        };
        let files: Vec<&str> = list.lines().filter_map(|l| l.strip_prefix("Would remove ")).collect();
        if files.is_empty() {
            self.set_status(t!("Aucun fichier non suivi à supprimer", "No untracked file to delete"));
            return;
        }
        let mut shown = files.iter().take(25).map(|f| format!("  {f}")).collect::<Vec<_>>().join("\n");
        if files.len() > 25 {
            shown.push_str(&tf!("\n  … et {} autre(s)", "\n  … and {} more", files.len() - 25));
        }
        let n = files.len();
        self.confirm_git(
            t!("Supprimer les fichiers non suivis", "Delete the untracked files"),
            tf!(
                "Supprimer définitivement {n} fichier(s) ou dossier(s) que git ne suit pas ?\n(les fichiers ignorés par .gitignore restent)\n\n{shown}",
                "Delete for good {n} file(s) or folder(s) git does not track?\n(the files ignored by .gitignore stay)\n\n{shown}"
            ),
            t!("Supprimer", "Delete"),
            "clean",
            vec![vec!["clean".into(), "-f".into(), "-d".into()]],
            None,
        );
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
