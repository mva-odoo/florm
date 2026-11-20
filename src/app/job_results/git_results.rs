//! Results of the git jobs: status, history, operations, split of a commit.

use super::*;

impl App {
    /// The status of the repositories: branches, changes, remotes, stashes.
    pub(super) fn on_git_status(&mut self, selected: PathBuf, statuses: Vec<(PathBuf, Result<git::Status, String>)>, remotes: Vec<(PathBuf, String)>, stashes: Vec<git::Stash>) {
        if self.repo.as_ref() != Some(&selected) {
            return;
        }
        self.git.stashes = stashes;
        let raw = remotes.iter().find(|(r, _)| *r == selected).map(|(_, t)| t.as_str()).unwrap_or("");
        self.git.remotes = git::parse_remotes(raw);
        // branch / remote of every repository, for the tree
        self.repo_info.clear();
        for (repo, st) in &statuses {
            let Ok(st) = st else { continue };
            if self.pr.repo.as_ref() == Some(repo) {
                self.pr.branch = Some(st.branch.clone());
            }
            let mut label = st.branch.clone();
            if let Some((up, ahead, behind)) = st.upstream() {
                // the usual origin/<same branch> is left to the tooltip
                if !up.split_once('/').is_some_and(|(_, b)| b == st.branch) {
                    label.push_str(&format!(" → {up}"));
                }
                if ahead > 0 {
                    label.push_str(&format!(" ↑{ahead}"));
                }
                if behind > 0 {
                    label.push_str(&format!(" ↓{behind}"));
                }
            }
            let raw = remotes.iter().find(|(r, _)| r == repo).map(|(_, t)| t.as_str()).unwrap_or("");
            let mut urls: Vec<String> = raw
                .lines()
                .filter(|l| l.ends_with("(fetch)"))
                .map(|l| l.trim_end_matches("(fetch)").trim().replacen('\t', " : ", 1))
                .collect();
            urls.dedup();
            let mut tip = tf!("Branche : {}\n", "Branch: {}\n", st.branch);
            match st.upstream() {
                Some((up, a, b)) => tip.push_str(&tf!("Suit : {up} ({a} à pousser, {b} à récupérer)\n", "Tracks: {up} ({a} to push, {b} to pull)\n")),
                None => tip.push_str(t!("Pas de branche distante suivie\n", "No tracked remote branch\n")),
            }
            if urls.is_empty() {
                tip.push_str(t!("Aucun remote", "No remote"));
            } else {
                tip.push_str(&tf!("Remote(s) :\n{}", "Remote(s):\n{}", urls.join("\n")));
            }
            let n = st.entries.len();
            if n > 0 {
                label.push_str(&tf!(" · {n} modif.", " · {n} changed"));
            }
            self.repo_info.insert(repo.clone(), (label, tip));
        }
        self.git.map.clear();
        self.git.status = None;
        self.git.error = None;
        let root = self.root.clone();
        for (repo, st) in statuses {
            match st {
                Ok(st) => {
                    for e in &st.entries {
                        let abs = repo.join(&e.path);
                        // Color parent folders up to the opened folder (repos may be nested in it).
                        for a in abs.ancestors().skip(1) {
                            if root.as_ref().is_some_and(|r| !a.starts_with(r) || a == r.as_path()) {
                                break;
                            }
                            self.git.map.entry(a.to_path_buf()).or_insert('M');
                        }
                        self.git.map.insert(abs, e.label());
                    }
                    if repo == selected {
                        // found stopped on conflicts (a rebase started in a terminal…): the
                        // assistant opens, once for that operation
                        let stop = st.operation.filter(|_| st.entries.iter().any(|e| e.conflicted())).map(|op| (repo.clone(), op));
                        if stop.is_some() && stop != self.conflicts.seen {
                            self.conflicts.seen = stop.clone();
                            self.conflicts.stale = true;
                            if !self.has_tab(document::View::Conflicts) {
                                self.show_tab(document::View::Conflicts);
                            }
                        }
                        if st.operation.is_none() {
                            self.conflicts.seen = None;
                        }
                        self.git.status = Some(st);
                    }
                }
                Err(e) if repo == selected => self.git.error = Some(e),
                Err(_) => {}
            }
        }
        // the branch may have changed: comments of the pull request in the editor or not
        self.pr_update_notes();
    }

    /// The graph of commits of the Git view.
    pub(super) fn on_history(&mut self, repo: PathBuf, gen: u64, r: Result<history::Graph, String>) {
        if self.history.repo.as_ref() == Some(&repo) && gen == self.history.gen {
            self.history.loading = false;
            match r {
                Ok(g) => {
                    let wanted = self.history.select_hash.take();
                    let pick = wanted.as_ref().and_then(|h| g.commits.iter().position(|c| &c.hash == h));
                    // older than the commits loaded: load more of them
                    let more = self.history.seek && pick.is_none() && g.commits.len() >= self.history.limit.max(HISTORY_LIMIT) && self.history.limit < HISTORY_MAX;
                    self.history.graph = Some(g);
                    self.history.error = None;
                    if more {
                        self.history.limit = HISTORY_MAX;
                        self.history.select_hash = wanted;
                        self.load_history();
                    } else if let Some(i) = pick {
                        self.history.seek = false;
                        self.history.wip_sel = None;
                        self.history.diff = None;
                        self.history.scroll_to_sel = true;
                        self.select_commit(i);
                    } else if let Some(h) = wanted.filter(|_| std::mem::take(&mut self.history.seek)) {
                        self.history.notice = Some((tf!("Commit {} introuvable dans le graphe", "Commit {} not found in the graph", &h[..h.len().min(8)]), err()));
                    }
                }
                Err(e) => self.history.error = Some(e),
            }
        }
    }

    /// The blame of a line, shown at its end.
    pub(super) fn on_blame(&mut self, uid: u64, line: usize, version: u64, result: Result<git::Blame, String>) {
        if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
            let text = match result {
                Ok(b) if b.uncommitted() => t!("Vous · pas encore commité", "You · not committed yet").to_string(),
                Ok(b) => format!(
                    "{} · {} · {} ({})",
                    b.author,
                    git::ago(b.time),
                    b.summary,
                    &b.hash[..b.hash.len().min(7)]
                ),
                Err(_) => t!("pas de blame (fichier hors git ou non suivi)", "no blame (file outside git or not tracked)").to_string(),
            };
            d.blame = Some((line, version, text));
        }
    }

    /// The commits that touched a line.
    pub(super) fn on_line_history(&mut self, result: Result<Vec<git::LineCommit>, String>) {
        if let Some(lh) = self.line_history.as_mut() {
            lh.loading = false;
            match result {
                Ok(commits) => {
                    lh.diffs = commits.iter().map(|c| (!c.diff.is_empty()).then(|| history::parse_diff(&c.diff))).collect();
                    lh.commits = commits;
                }
                Err(e) => lh.error = Some(e),
            }
        }
        self.line_history_load_diff();
    }

    /// The diff of a commit of the history of a line.
    pub(super) fn on_line_history_diff(&mut self, hash: String, result: Result<String, String>) {
        if let Some(lh) = self.line_history.as_mut() {
            let text = result.unwrap_or_else(|e| e);
            let text = if text.trim().is_empty() { t!("(aucune différence)", "(no difference)").to_string() } else { text };
            for (k, c) in lh.commits.iter().enumerate() {
                if c.hash == hash && lh.diffs[k].is_none() {
                    lh.diffs[k] = Some(history::parse_diff(&text));
                }
            }
        }
    }

    /// The diff of a commit of the graph (of one of its files with `path`).
    pub(super) fn on_history_diff(&mut self, hash: String, path: Option<String>, result: Result<String, String>) {
        if self.history_key().as_deref() == Some(hash.as_str()) {
            self.history.diff_loading = false;
            let text = result.unwrap_or_else(|e| e);
            let text = if text.trim().is_empty() { t!("(aucune différence)", "(no difference)").to_string() } else { text };
            let lines = history::parse_diff(&text);
            let files = history::diff_files(&lines);
            // a reload of the same diff (after staging…) keeps the folded files
            let same = self.history.diff.as_ref().is_some_and(|d| d.hash == hash);
            if !same {
                self.history.collapsed.clear();
            }
            if let Some(p) = self.history.jump_path.take() {
                if let Some(k) = files.iter().position(|f| f.path == p) {
                    self.history.jump = Some(history::FileJump::To(k));
                }
            }
            self.history.diff = Some(HistoryDiff { hash, path, text, lines, files });
        }
    }

    /// The details of a commit of the graph.
    pub(super) fn on_history_details(&mut self, hash: String, r: Result<history::Details, String>) {
        if self.history_key().as_deref() == Some(hash.as_str()) {
            match r {
                Ok(d) => self.history.details = Some(d),
                Err(e) => self.log(e),
            }
        }
    }

    /// A git command done: its output, then everything that depends on the repository.
    pub(super) fn on_git_op(&mut self, label: String, result: Result<String, String>, status: Option<(PathBuf, Result<git::Status, String>)>) {
        self.git.busy = self.git.busy.saturating_sub(1);
        if label.starts_with("stash") {
            self.stash_reload();
        }
        if let Some((repo, Ok(st))) = status {
            if self.repo.as_ref() == Some(&repo) {
                self.git.status = Some(st);
            }
        }
        match result {
            Ok(out) => {
                // bisect: where it is, or the commit found
                if label == "bisect" {
                    let line = out.lines().find(|l| l.contains("is the first bad commit")).or_else(|| out.lines().find(|l| l.starts_with("Bisecting:")));
                    if let Some(l) = line {
                        self.history.notice = Some((l.trim().to_string(), ok()));
                    }
                }
                self.log(out);
                self.git.op_error = None;
                self.set_status(tf!("git {label} : OK", "git {label}: OK"));
            }
            Err(e) => {
                self.log(e.clone());
                if self.git_shown() {
                    let first = e.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
                    self.history.notice = Some((tf!("git {label} a échoué : {first}", "git {label} failed: {first}"), err()));
                }
                self.git.op_error = Some((label.clone(), e));
                self.set_error(tf!("git {label} : échec (détail dans le panneau Git)", "git {label}: failed (details in the Git panel)"));
            }
        }
        self.git_refresh();
        // stopped on conflicts: the assistant opens on them
        self.conflicts.stale = true;
        let stopped = self.git.status.as_ref().is_some_and(|st| st.operation.is_some() && st.entries.iter().any(|e| e.conflicted()));
        if stopped && !self.has_tab(document::View::Conflicts) {
            self.show_tab(document::View::Conflicts);
        }
        // staging, unstaging or discarding does not move branches: keep the selection
        let refs = !matches!(label.as_str(), "add" | "restore" | "apply" | "rm");
        if self.git_shown() && refs {
            self.load_history();
        }
        if let (true, Some(staged)) = (self.git_shown(), self.history.wip_sel) {
            self.load_wip_all(staged);
        }
        if let Some(t) = self.tree.as_mut() {
            t.refresh();
        }
        for i in 0..self.docs.len() {
            if self.docs[i].path.is_some() {
                self.analyze(i, false);
            }
        }
    }

    /// A git repository created.
    pub(super) fn on_repo_init(&mut self, root: PathBuf, dir: PathBuf, result: Result<String, String>, repos: Vec<PathBuf>) {
        match result {
            Ok(out) => {
                self.log(out);
                self.repo_init = None;
                self.set_status(tf!("Dépôt git créé dans {}", "Git repository created in {}", dir.display()));
            }
            // the window stays, with what went wrong (the steps before stay done)
            Err(e) => {
                self.log(e.clone());
                match self.repo_init.as_mut() {
                    Some(st) => st.error = Some(e),
                    None => self.set_error(tf!("Dépôt git : {}", "Git repository: {}", e.lines().last().unwrap_or(""))),
                }
            }
        }
        if let Some(st) = self.repo_init.as_mut() {
            st.busy = false;
        }
        self.apply_repos(root, repos);
        self.exec_refresh_tree();
    }
}
