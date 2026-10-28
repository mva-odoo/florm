//! Git view: commit graph, diffs, and what is done from them.

use super::*;
use history::DiffAct;

impl App {
    pub(super) fn load_history(&mut self) {
        let Some(repo) = self.repo.clone() else {
            self.history.error = Some(t!("Aucun dépôt git dans ce dossier.", "No git repository in this folder.").into());
            return;
        };
        if self.history.repo.as_ref() != Some(&repo) {
            self.history.graph = None;
            self.history.limit = 0;
        }
        self.history.repo = Some(repo.clone());
        self.history.loading = true;
        // a reload (refresh, change made in a terminal…) keeps the selected commit
        if self.history.select_hash.is_none() && self.history.wip_sel.is_none() && self.history.compare.is_none() {
            self.history.select_hash = self.history.selected.and_then(|i| self.history.graph.as_ref().and_then(|g| g.commits.get(i)).map(|c| c.hash.clone()));
        }
        self.history.selected = None;
        self.history.details = None;
        let all = self.history.all;
        self.history.gen += 1;
        let (gen, limit) = (self.history.gen, if self.history.limit == 0 { HISTORY_LIMIT } else { self.history.limit });
        self.jobs.spawn(move || {
            let r = history::load(&repo, all, limit);
            JobResult::History(repo, gen, r)
        });
    }

    pub(super) fn select_commit(&mut self, i: usize) {
        let (Some(g), Some(repo)) = (&self.history.graph, self.history.repo.clone()) else { return };
        let Some(c) = g.commits.get(i) else { return };
        let hash = c.hash.clone();
        self.history.selected = Some(i);
        self.history.details = None;
        let h2 = hash.clone();
        self.jobs.spawn(move || {
            let r = history::details(&repo, &h2);
            JobResult::HistoryDetails(h2, r)
        });
        // its changes under the list
        self.load_commit_diff(hash, None);
    }

    /// Loads a commit diff (whole commit or one file) into the pane under the commit list.
    fn load_commit_diff(&mut self, hash: String, path: Option<String>) {
        let Some(repo) = self.history.repo.clone() else { return };
        self.history.diff_loading = true;
        self.jobs.spawn(move || {
            let result = match &path {
                // -m --first-parent: merges are shown against their first parent (what the merge brought)
                Some(p) => git::run(&repo, &["show", "--format=", "-m", "--first-parent", &hash, "--", p]),
                None => git::run(&repo, &["show", "--format=", "--patch", "-m", "--first-parent", &hash]),
            };
            JobResult::HistoryDiff { hash, path, result }
        });
    }

    /// Requests the blame of the cursor line when it changed (blame mode).
    pub(super) fn update_blame(&mut self) {
        if !self.blame_mode {
            return;
        }
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(path) = d.path.clone() else { return };
        let line = d.rope.char_to_line(d.primary_sel().head);
        let key = (d.uid, line, d.version);
        if self.blame_req == Some(key) || d.last_change.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.blame_req = Some(key);
        let (uid, version, contents) = (d.uid, d.version, d.rope.to_string());
        self.jobs.spawn(move || JobResult::Blame { uid, line, version, result: git::blame_line(&path, line + 1, &contents) });
    }

    pub(super) fn open_line_history(&mut self) {
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(path) = d.path.clone() else {
            self.set_status(t!("Le fichier n'est pas enregistré", "The file is not saved"));
            return;
        };
        // the selected lines, or the cursor line
        let sel = d.primary_sel();
        let (a, b) = (d.rope.char_to_line(sel.start()), d.rope.char_to_line(sel.end()));
        // a selection ending at the start of a line does not take that line
        let b = if b > a && d.rope.line_to_char(b) == sel.end() { b - 1 } else { b };
        let name = d.title();
        let Some(repo) = self.repo_for(&path) else {
            self.set_status(t!("Ce fichier n'est pas dans un dépôt git", "This file is not in a git repository"));
            return;
        };
        // the lines as they are in HEAD (the buffer may have changed since)
        let lines: Vec<usize> = (a..=b).filter_map(|l| d.head_line(l)).collect();
        let (Some(&first), Some(&last)) = (lines.iter().min(), lines.iter().max()) else {
            self.set_status(t!("Ligne ajoutée depuis le dernier commit : pas encore d'historique", "Line added since the last commit: no history yet"));
            return;
        };
        let title = if a == b { tf!("Historique de la ligne {} — {name}", "History of line {} — {name}", a + 1) } else { tf!("Historique des lignes {}–{} — {name}", "History of lines {}–{} — {name}", a + 1, b + 1) };
        self.open_history_window(title, repo, false);
        self.jobs.spawn(move || JobResult::LineHistory(git::line_history(&path, first + 1, last + 1)));
    }

    fn open_history_window(&mut self, title: String, repo: PathBuf, reflog: bool) {
        self.line_history = Some(LineHistoryUi { title, repo, loading: true, error: None, commits: Vec::new(), diffs: Vec::new(), selected: 0, reflog });
    }

    /// Every commit of the current file, through its renames.
    pub(super) fn open_file_history(&mut self) {
        let Some(path) = self.docs.get(self.active).and_then(|d| d.path.clone()) else {
            self.set_status(t!("Le fichier n'est pas enregistré", "The file is not saved"));
            return;
        };
        let Some(repo) = self.repo_for(&path) else {
            self.set_status(t!("Ce fichier n'est pas dans un dépôt git", "This file is not in a git repository"));
            return;
        };
        let rel = path.strip_prefix(&repo).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        self.file_history(repo, rel);
    }

    /// Every commit of `rel` (path in `repo`), through its renames.
    pub(super) fn file_history(&mut self, repo: PathBuf, rel: String) {
        self.history_search(repo, git::SearchKind::File, rel);
    }

    /// The commits of the history matching `query`, in the history window.
    pub(super) fn history_search(&mut self, repo: PathBuf, kind: git::SearchKind, query: String) {
        let title = match kind {
            git::SearchKind::File => tf!("Historique du fichier — {query}", "History of the file — {query}"),
            _ => format!("{} : « {query} »", kind.label()),
        };
        self.open_history_window(title, repo.clone(), false);
        self.jobs.spawn(move || JobResult::LineHistory(git::search(&repo, kind, &query, 300)));
    }

    /// Where HEAD has been (commits, checkouts, resets…): a lost commit can be brought back.
    pub(super) fn open_reflog(&mut self) {
        let Some(repo) = self.repo.clone() else {
            self.set_status(t!("Pas de dépôt git", "No git repository"));
            return;
        };
        self.open_history_window(t!("Reflog : où HEAD est passée", "Reflog: where HEAD has been").into(), repo.clone(), true);
        self.jobs.spawn(move || JobResult::LineHistory(git::reflog(&repo, 500)));
    }

    /// The diff of the commit chosen in the history window, when not read yet.
    pub(super) fn line_history_load_diff(&mut self) {
        let Some(lh) = self.line_history.as_ref() else { return };
        let (Some(c), Some(None)) = (lh.commits.get(lh.selected), lh.diffs.get(lh.selected)) else { return };
        let (repo, hash) = (lh.repo.clone(), c.hash.clone());
        self.jobs.spawn(move || {
            let r = git::run(&repo, &["show", "--format=", "--patch", "-m", "--first-parent", &hash]).map(|t| t.chars().take(400_000).collect());
            JobResult::LineHistoryDiff(hash, r)
        });
    }

    pub(super) fn line_history_ui(&mut self, ctx: &egui::Context) {
        let Some(lh) = self.line_history.as_mut() else { return };
        let mut open = true;
        let (mut show_graph, mut copy, mut chose) = (None, None, false);
        let mut act: Option<HistAct> = None;
        egui::Window::new(lh.title.clone())
            .id(egui::Id::new("line_history"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([960.0, 540.0])
            .min_size([480.0, 260.0])
            .show(ctx, |ui| {
                if lh.loading {
                    ui.horizontal(|ui| {
                        spinner(ui);
                        ui.label(t!("Recherche des commits…", "Looking for the commits…"));
                    });
                    return;
                }
                if let Some(e) = &lh.error {
                    ui.label(RichText::new(e).color(err()));
                    return;
                }
                if lh.commits.is_empty() {
                    ui.label(RichText::new(t!("Aucun commit trouvé.", "No commit found.")).weak());
                    return;
                }
                ui.set_min_size(ui.available_size());
                let height = ui.available_height();
                ui.horizontal_top(|ui| {
                    // commit list
                    ui.vertical(|ui| {
                        ui.set_width(320.0);
                        let head = if lh.reflog { tf!("{} ENTRÉE(S), LA PLUS RÉCENTE EN HAUT", "{} ENTRIES, THE NEWEST AT THE TOP", lh.commits.len()) } else { tf!("{} COMMIT(S)", "{} COMMIT(S)", lh.commits.len()) };
                        ui.label(RichText::new(head).small().strong());
                        egui::ScrollArea::vertical().id_salt("lh_list").max_height(height - 24.0).auto_shrink([false, false]).show(ui, |ui| {
                            for (i, c) in lh.commits.iter().enumerate() {
                                let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), Sense::click());
                                let v = ui.visuals();
                                if i == lh.selected {
                                    ui.painter().rect_filled(rect, 5.0, v.selection.bg_fill);
                                } else if r.hovered() {
                                    ui.painter().rect_filled(rect, 5.0, v.widgets.hovered.weak_bg_fill);
                                }
                                let p = ui.painter().with_clip_rect(rect.shrink(4.0));
                                p.text(rect.left_top() + egui::vec2(8.0, 6.0), Align2::LEFT_TOP, &c.subject, egui::FontId::proportional(14.0), v.text_color());
                                p.text(
                                    rect.left_top() + egui::vec2(8.0, 25.0),
                                    Align2::LEFT_TOP,
                                    format!("{} · {} · {}", c.author, c.date, &c.hash[..c.hash.len().min(7)]),
                                    egui::FontId::proportional(12.0),
                                    v.weak_text_color(),
                                );
                                if r.clicked() && lh.selected != i {
                                    lh.selected = i;
                                    chose = true;
                                }
                            }
                        });
                    });
                    ui.separator();
                    // selected commit: header + diff
                    ui.vertical(|ui| {
                        let c = &lh.commits[lh.selected];
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(&c.subject).strong()).truncate());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if lh.reflog {
                                    ui.menu_button(t!("Récupérer ▾", "Bring back ▾"), |ui| {
                                        let items = [
                                            (t!("Créer une branche ici…", "Create a branch here…"), HistAct::BranchAt(c.hash.clone())),
                                            (t!("Checkout de ce commit (HEAD détachée)", "Checkout of this commit (detached HEAD)"), HistAct::Checkout(c.hash.clone())),
                                            (t!("Cherry-pick sur la branche courante", "Cherry-pick on the current branch"), HistAct::CherryPick(c.hash.clone())),
                                            (t!("Ramener la branche courante ici (reset --hard)…", "Move the current branch here (reset --hard)…"), HistAct::Reset(c.hash.clone(), "--hard")),
                                        ];
                                        for (label, a) in items {
                                            if ui.button(label).clicked() {
                                                act = Some(a);
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                } else if ui.button(t!("Voir dans le graphe", "Show in the graph")).clicked() {
                                    show_graph = Some(c.hash.clone());
                                }
                                if ui.button(t!("Copier le hash", "Copy the hash")).clicked() {
                                    copy = Some(c.hash.clone());
                                }
                            });
                        });
                        ui.label(RichText::new(format!("{} · {} · {}", c.author, c.date, &c.hash[..c.hash.len().min(10)])).weak());
                        ui.separator();
                        match &lh.diffs[lh.selected] {
                            Some(d) => {
                                let _ = history::diff_view(ui, d, "line_history_diff", &[]);
                            }
                            None => {
                                spinner(ui);
                            }
                        }
                    });
                });
            });
        if !open {
            self.line_history = None;
        }
        if chose {
            self.line_history_load_diff();
        }
        if let Some(h) = copy {
            ctx.output_mut(|o| o.copied_text = h);
            self.set_status(t!("Hash copié", "Hash copied"));
        }
        if let Some(a) = act {
            let Some(repo) = self.line_history.as_ref().map(|l| l.repo.clone()) else { return };
            self.select_repo(repo.clone());
            match a {
                HistAct::BranchAt(hash) => self.open_overlay_with(Mode::Prompt(Prompt::BranchAt(hash)), Vec::new(), String::new()),
                HistAct::Checkout(hash) => self.git_job_at(repo, "checkout", &["checkout", &hash]),
                HistAct::CherryPick(hash) => self.git_job_at(repo, "cherry-pick", &["-c", "core.editor=true", "cherry-pick", &hash]),
                a => self.hist_git_act(a, repo),
            }
        }
        if let Some(h) = show_graph {
            let Some(r) = self.line_history.take().map(|l| l.repo) else { return };
            self.select_repo(r);
            self.show_tab(document::View::Git);
            self.history.all = true;
            // the filter could hide it
            self.history.filter.clear();
            self.history.select_hash = Some(h);
            self.history.seek = true;
            self.load_history();
        }
    }

    /// Key of what is selected in the graph: a commit hash, or "INDEX" / "WORKTREE" for the
    /// staged / unstaged changes (rows at the top, like tig).
    pub(super) fn history_key(&self) -> Option<String> {
        if let Some((a, b, _)) = &self.history.compare {
            return Some(format!("{a}..{b}"));
        }
        match self.history.wip_sel {
            Some(true) => Some("INDEX".into()),
            Some(false) => Some("WORKTREE".into()),
            None => self.history.selected.and_then(|i| self.history.graph.as_ref().map(|g| g.commits[i].hash.clone())),
        }
    }

    pub(super) fn start_compare(&mut self, a: String, b: String, title: String) {
        let Some(repo) = self.history.repo.clone() else { return };
        let h = &mut self.history;
        h.compare = Some((a.clone(), b.clone(), title));
        h.wip_sel = None;
        h.rebase = None;
        h.diff = None;
        h.diff_loading = false;
        h.details = None;
        let key = format!("{a}..{b}");
        self.jobs.spawn(move || JobResult::HistoryDetails(key, history::compare(&repo, &a, &b)));
    }

    /// Diff between two commits (all files, or one).
    fn load_range_diff(&mut self, a: String, b: String, path: Option<String>) {
        let Some(repo) = self.history.repo.clone() else { return };
        self.history.diff_loading = true;
        self.jobs.spawn(move || {
            let result = match &path {
                Some(p) => git::run(&repo, &["diff", &a, &b, "--", p]),
                None => git::run(&repo, &["diff", &a, &b]),
            };
            JobResult::HistoryDiff { hash: format!("{a}..{b}"), path, result }
        });
    }

    /// The changes not committed (indexed or not) in the detail, below the commits.
    pub(super) fn show_wip(&mut self, staged: bool) {
        let h = &mut self.history;
        h.compare = None;
        h.rebase = None;
        h.wip_sel = Some(staged);
        h.diff = None;
        h.diff_loading = false;
        self.load_wip_all(staged);
    }

    /// Diff of all the staged (`staged`) or unstaged changes, new files included.
    pub(super) fn load_wip_all(&mut self, staged: bool) {
        let Some(repo) = self.history.repo.clone() else { return };
        self.history.diff_loading = true;
        self.jobs.spawn(move || {
            let result = if staged { git::run(&repo, &["diff", "--cached"]) } else { worktree_diff(&repo, &["diff"]) };
            let hash = if staged { "INDEX" } else { "WORKTREE" }.to_string();
            JobResult::HistoryDiff { hash, path: None, result }
        });
    }

    /// Button or double click in the diff of the graph view.
    fn diff_action(&mut self, source: DiffSource, click: history::DiffClick) {
        let Some(d) = &self.history.diff else { return };
        let Some(repo) = self.history.repo.clone() else { return };
        let (text, lines) = (d.text.clone(), &d.lines);
        let idx = match click {
            history::DiffClick::Line(i) | history::DiffClick::Hunk(i, _) | history::DiffClick::File(i, _) => i,
        };
        // a new file not yet known to git (only in the unstaged changes)
        let untracked = source == DiffSource::Worktree && history::file_patch(&text, idx).is_some_and(|f| f.contains("\n--- /dev/null"));
        let Some(path) = history::file_at(&text, idx).or_else(|| d.path.clone()) else { return };
        let line = history::target_line(lines, idx);
        let lose = |what: &str| tf!("{what}\n\nCe changement n'est pas encore dans un commit : il sera perdu.", "{what}\n\nThis change is not in a commit yet: it will be lost.");
        let stage_file = DiffOp::Git(vec!["add".into(), "--".into(), path.clone()]);
        use history::DiffClick;
        let (label, op, confirm) = match click {
            DiffClick::Line(_) | DiffClick::File(_, DiffAct::Open) | DiffClick::Hunk(_, DiffAct::Open) => {
                self.jump_to_file(&repo, &path, line);
                return;
            }
            // a whole file, or a block of a new file (git has no blocks for it yet)
            DiffClick::File(_, DiffAct::Stage) => ("add", stage_file, None),
            DiffClick::Hunk(_, DiffAct::Stage | DiffAct::Discard) if untracked && source == DiffSource::Worktree => match click {
                DiffClick::Hunk(_, DiffAct::Stage) => ("add", stage_file, None),
                _ => ("rm", DiffOp::Delete(repo.join(&path)), Some(lose(&tf!("Supprimer le nouveau fichier « {path} » ?", "Delete the new file “{path}”?")))),
            },
            DiffClick::File(_, DiffAct::Unstage) => ("restore", DiffOp::Git(vec!["restore".into(), "--staged".into(), "--".into(), path.clone()]), None),
            DiffClick::File(_, DiffAct::Discard) if untracked => {
                ("rm", DiffOp::Delete(repo.join(&path)), Some(lose(&tf!("Supprimer le nouveau fichier « {path} » ?", "Delete the new file “{path}”?"))))
            }
            DiffClick::File(_, DiffAct::Discard) => (
                "restore",
                DiffOp::Git(vec!["restore".into(), "--".into(), path.clone()]),
                Some(lose(&tf!("Annuler toutes les modifications non indexées de « {path} » ?", "Discard all the unstaged changes of “{path}”?"))),
            ),
            DiffClick::File(_, DiffAct::Revert) => {
                let Some(patch) = history::file_patch(&text, idx) else { return };
                ("apply", DiffOp::Apply { patch, flags: vec!["-R"] }, None)
            }
            DiffClick::Hunk(_, act) => {
                let Some(patch) = history::hunk_patch(&text, idx) else { return };
                match act {
                    DiffAct::Stage => ("apply", DiffOp::Apply { patch, flags: vec!["--cached"] }, None),
                    DiffAct::Unstage => ("apply", DiffOp::Apply { patch, flags: vec!["--cached", "-R"] }, None),
                    DiffAct::Discard => (
                        "apply",
                        DiffOp::Apply { patch, flags: vec!["-R"] },
                        Some(lose(&tf!("Annuler ce bloc de modifications dans « {path} » (vers la ligne {line}) ?", "Discard this block of changes in “{path}” (around line {line})?"))),
                    ),
                    DiffAct::Revert => ("apply", DiffOp::Apply { patch, flags: vec!["-R"] }, None),
                    DiffAct::Open => return,
                }
            }
        };
        match confirm {
            Some(msg) => self.confirm = Some(Confirm::DiffOp { msg, label: label.into(), op }),
            None => {
                // the changes leave the diff shown at once; git's new diff follows
                if source != DiffSource::Commit && source != DiffSource::Compare {
                    let whole = matches!(click, DiffClick::File(..));
                    if let Some(d) = self.history.diff.as_mut() {
                        d.text = history::without(&d.text, idx, whole);
                        d.lines = history::parse_diff(&d.text);
                        d.files = history::diff_files(&d.lines);
                    }
                }
                self.run_diff_op(label.into(), op);
            }
        }
    }

    pub(super) fn run_diff_op(&mut self, label: String, op: DiffOp) {
        let Some(repo) = self.history.repo.clone() else { return };
        self.git.busy += 1;
        self.jobs.spawn(move || {
            let result = match op {
                DiffOp::Apply { patch, flags } => {
                    let mut a = vec!["apply"];
                    a.extend(flags);
                    a.extend(["--recount", "-"]);
                    git::run_with(&repo, &a, &[], Some(&patch))
                }
                DiffOp::Git(args) => {
                    let a: Vec<&str> = args.iter().map(String::as_str).collect();
                    git::run(&repo, &a)
                }
                DiffOp::Delete(p) => std::fs::remove_file(&p).map(|()| tf!("{} supprimé", "{} deleted", p.display())).map_err(|e| e.to_string()),
            };
            JobResult::GitOp { label, result, status: Some((repo.clone(), git::status(&repo))) }
        });
    }

    /// Opens a file of the repository at a line (from a diff).
    fn jump_to_file(&mut self, repo: &Path, rel: &str, line: usize) {
        let path = repo.join(rel);
        if !path.is_file() {
            self.history.notice = Some((tf!("« {rel} » n'existe plus dans tes fichiers.", "“{rel}” no longer exists in your files."), err()));
            return;
        }
        if self.open_path(path) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.goto_line(line.saturating_sub(1), 0);
            }
            self.pending_focus = true;
        }
    }

    /// The list takes the whole view again: nothing selected below it.
    pub(super) fn close_history_detail(&mut self) {
        let h = &mut self.history;
        h.selected = None;
        h.details = None;
        h.wip_sel = None;
        h.compare = None;
        h.diff = None;
        h.diff_loading = false;
    }

    /// Something is shown under the commit list (Escape closes it before the view).
    pub(super) fn history_detail_open(&self) -> bool {
        let h = &self.history;
        h.rebase.is_none() && (h.selected.is_some() || h.wip_sel.is_some() || h.compare.is_some())
    }

    pub(super) fn history_ui(&mut self, ui: &mut egui::Ui) {
        if self.install_guide(ui, crate::tools::Tool::Git, false) {
            return;
        }
        // commits proposed by Claude: read over before committing, in place of the list
        if self.git.plan.is_some() {
            self.commit_plan_ui(ui);
            return;
        }
        let (mut select, mut diff_to_tab, mut close_detail) = (None, false, false);
        let mut diff_click: Option<history::DiffClick> = None;
        let source = match (self.history.wip_sel, &self.history.compare) {
            (_, Some(_)) => DiffSource::Compare,
            (Some(false), _) => DiffSource::Worktree,
            (Some(true), _) => DiffSource::Index,
            (None, _) => DiffSource::Commit,
        };
        let (mut rb_run, mut rb_cancel) = (false, false);
        let op_banner = self
            .git
            .status
            .as_ref()
            .filter(|_| self.repo == self.history.repo)
            .and_then(|st| st.operation.map(|o| (o, st.entries.iter().filter(|e| e.conflicted()).count())));
        let mut op_cmd: Option<Vec<String>> = None;
        let (mut menu_act, mut clear_notice, mut resume, mut abort_rebase) = (None, false, false, false);
        let (mut wip_pick, mut wip_ops): (Option<bool>, Vec<Vec<String>>) = (None, Vec::new());
        let split_hidden = self.split.as_ref().map(|s| s.info.short.clone());
        let accent = self.hl.colors.accent;
        // working tree status (same repository as the graph)
        let wip = self.git.status.clone().filter(|_| self.repo == self.history.repo);
        let current = wip.as_ref().map(|s| s.branch.clone()).unwrap_or_default();
        let bisecting = wip.as_ref().is_some_and(|s| s.bisecting);
        let mut bisect_cmd: Option<&'static str> = None;
        let (n_staged, n_changed) = wip.as_ref().map_or((0, 0), |st| {
            (st.entries.iter().filter(|e| e.staged()).count(), st.entries.iter().filter(|e| e.unstaged() && e.x != '?').count())
        });
        // pull requests containing the selected commit
        let commit_links = match (self.history.selected.zip(self.history.graph.as_ref()), self.history.repo.clone()) {
            (Some((i, g)), Some(repo)) if self.history.wip_sel.is_none() => {
                let sha = g.commits[i].hash.clone();
                let msg = self.history.details.as_ref().map(|d| d.message.clone()).unwrap_or_default();
                self.commit_prs(&repo, &sha, &msg)
            }
            _ => Vec::new(),
        };
        let mut open_pr = None;
        let (mut do_commit, mut suggest_msg, suggesting) = (false, false, self.git.suggesting);
        let (mut ask_plan, planning) = (false, self.git.planning);
        let commit_msg = &mut self.git.commit_msg;
        let amend = &mut self.git.amend;
        let mut start_amend = false;
        // the last commit is already on the remote: amending it will need a forced push
        let pushed = wip.as_ref().and_then(|s| s.upstream()).is_some_and(|(_, ahead, _)| ahead == 0);
        let focus_commit = std::mem::take(&mut self.git.focus_commit);
        // AI review of the changes not committed: asked, kept remarks given to Claude, a remark's action
        let (mut ai_start, mut ai_fix, mut ai_act) = (false, false, None);
        let ai = &mut self.git.ai;
        let cache = &mut self.md_cache;
        let h = &mut self.history;
        // messages and operations in progress, above the list (nothing when there are none)
        let banner = h.notice.is_some() || split_hidden.is_some() || op_banner.is_some() || bisecting || h.graph.as_ref().is_some_and(|g| g.rebasing);
        egui::TopBottomPanel::top("history_top").show_animated_inside(ui, banner, |ui| {
            if let Some((n, color)) = &h.notice {
                ui.horizontal(|ui| {
                    let icon = if *color == err() { "⚠" } else { "✔" };
                    ui.label(RichText::new(format!("{icon} {n}")).color(*color).strong());
                    if ui.small_button("×").clicked() {
                        clear_notice = true;
                    }
                });
            }
            if bisecting {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(t!("🔎 Bisect en cours : teste le commit actuel, puis dis s'il est bon ou mauvais", "🔎 Bisect in progress: test the current commit, then say whether it is good or bad")).color(warn()).strong());
                    for (label, verb, tip) in [
                        (t!("✔ Bon", "✔ Good"), "good", t!("Le bug n'est pas là : il est apparu après", "The bug is not there: it came later")),
                        (t!("× Mauvais", "× Bad"), "bad", t!("Le bug est là : il est apparu avant ou ici", "The bug is there: it came before or here")),
                        (t!("Passer", "Skip"), "skip", t!("Impossible à tester : en essayer un autre", "Cannot be tested: try another one")),
                        (t!("Terminer", "Finish"), "reset", t!("Arrêter le bisect et revenir sur la branche", "Stop the bisect and go back to the branch")),
                    ] {
                        if ui.button(label).on_hover_text(tip).clicked() {
                            bisect_cmd = Some(verb);
                        }
                    }
                });
            }
            if let Some(short) = &split_hidden {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tf!("✂ Le découpage de {short} est en cours.", "✂ The splitting of {short} is in progress.")).color(warn()).strong());
                    if ui.button(t!("Revenir au découpage", "Back to the splitting")).clicked() {
                        resume = true;
                    }
                });
            } else if let Some((op, n)) = op_banner {
                ui.horizontal_wrapped(|ui| {
                    let what = if n > 0 { tf!(" — {n} fichier(s) en conflit à résoudre", " — {n} conflicting file(s) to resolve") } else { String::new() };
                    ui.label(RichText::new(tf!("⚠ {} en cours{what}", "⚠ {} in progress{what}", op.label())).color(warn()).strong());
                    if ui.add_enabled(n == 0, egui::Button::new(t!("Continuer", "Continue"))).clicked() {
                        let mut a = vec!["-c".to_string(), "core.editor=true".to_string()];
                        a.extend(op.continue_args().iter().map(|s| s.to_string()));
                        op_cmd = Some(a);
                    }
                    if ui.button(t!("Abandonner", "Abort")).on_hover_text(t!("Remet la branche comme avant l'opération", "Puts the branch back as before the operation")).clicked() {
                        op_cmd = Some(op.abort_args().iter().map(|s| s.to_string()).collect());
                    }
                    if op == git::Operation::Rebase && ui.button(t!("Reprendre le découpage", "Resume the splitting")).clicked() {
                        resume = true;
                    }
                });
            } else if h.graph.as_ref().is_some_and(|g| g.rebasing) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t!("⚠ Un rebase est en cours dans ce dépôt (découpage pas terminé ?)", "⚠ A rebase is in progress in this repository (splitting not finished?)")).color(warn()));
                    if ui.button(t!("Reprendre le découpage", "Resume the splitting")).clicked() {
                        resume = true;
                    }
                    if ui.button(t!("Annuler le rebase", "Abort the rebase")).on_hover_text(t!("git rebase --abort : remet la branche comme avant", "git rebase --abort: puts the branch back as before")).clicked() {
                        abort_rebase = true;
                    }
                });
            }
            ui.add_space(2.0);
        });

        // keyboard (when no text field has it): ↑ ↓ commits, ← → files of the diff, Page ↑ ↓ scroll the diff
        let free = ui.ctx().memory(|m| m.focused().is_none());
        let keys = if free {
            ui.input(|i| {
                (
                    i.key_pressed(Key::ArrowDown) as i32 - i.key_pressed(Key::ArrowUp) as i32,
                    i.key_pressed(Key::ArrowRight) as i32 - i.key_pressed(Key::ArrowLeft) as i32,
                    i.key_pressed(Key::PageDown) as i32 - i.key_pressed(Key::PageUp) as i32,
                )
            })
        } else {
            (0, 0, 0)
        };
        match keys {
            (_, 1, _) => h.jump = Some(history::FileJump::Next),
            (_, -1, _) => h.jump = Some(history::FileJump::Prev),
            (_, _, 1) => h.jump = Some(history::FileJump::PageDown),
            (_, _, -1) => h.jump = Some(history::FileJump::PageUp),
            _ => {}
        }
        let selected_commit = h.selected.zip(h.graph.as_ref()).map(|(i, g)| g.commits[i].clone());
        let has_detail = selected_commit.is_some() || h.wip_sel.is_some() || h.compare.is_some() || h.rebase.is_some();

        // ------------------------------------------------ commit list, with the working tree on top
        let mut list = |ui: &mut egui::Ui| {
            if n_staged + n_changed > 0 || !commit_msg.is_empty() || focus_commit || *amend {
                let was = *amend;
                (do_commit, suggest_msg, ask_plan) = commit_box(ui, commit_msg, amend, pushed, focus_commit, n_staged, n_changed, suggesting, planning, accent);
                start_amend = *amend && !was;
            }
            if let Some(st) = &wip {
                let staged: Vec<&git::Entry> = st.entries.iter().filter(|e| e.staged()).collect();
                let unstaged: Vec<&git::Entry> = st.entries.iter().filter(|e| e.unstaged()).collect();
                for (is_staged, list, color, label, stats) in [
                    (false, &unstaged, warn(), t!("Modifications non indexées", "Unstaged changes"), &st.unstaged_stats),
                    (true, &staged, ok(), t!("Modifications indexées", "Staged changes"), &st.staged_stats),
                ] {
                    if list.is_empty() {
                        continue;
                    }
                    let (a, d) = stats.values().fold((0, 0), |(a, d), s| (a + s.0.unwrap_or(0), d + s.1.unwrap_or(0)));
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
                    let sel = h.wip_sel == Some(is_staged);
                    if sel {
                        ui.painter().rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
                    }
                    let p = ui.painter();
                    let cx = rect.left() + history::LANE_W / 2.0 + 2.0;
                    if is_staged {
                        p.circle_filled(egui::pos2(cx, rect.center().y), 4.5, color);
                    } else {
                        p.circle_stroke(egui::pos2(cx, rect.center().y), 4.5, egui::Stroke::new(1.5_f32, color));
                    }
                    let r = p.text(egui::pos2(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, label, egui::FontId::proportional(13.5), color);
                    let info = tf!("{} fichier(s)", "{} file(s)", list.len());
                    let r = p.text(egui::pos2(r.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, info, egui::FontId::proportional(12.0), ui.visuals().weak_text_color());
                    let r = p.text(egui::pos2(r.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, format!("+{a}"), egui::FontId::monospace(11.5), ok());
                    p.text(egui::pos2(r.right() + 6.0, rect.center().y), Align2::LEFT_CENTER, format!("−{d}"), egui::FontId::monospace(11.5), err());
                    if resp.on_hover_text(t!("Voir les changements, indexer / désindexer", "See the changes, stage / unstage")).clicked() {
                        wip_pick = Some(is_staged);
                    }
                }
                if !st.entries.is_empty() {
                    ui.separator();
                }
            }
            let Some(g) = &h.graph else {
                match &h.error {
                    Some(e) => ui.label(RichText::new(e).color(err())),
                    None => ui.label(RichText::new(t!("Chargement…", "Loading…")).weak()),
                };
                return;
            };
            let f = h.filter.to_lowercase();
            let idx: Vec<usize> = if f.is_empty() {
                (0..g.commits.len()).collect()
            } else {
                (0..g.commits.len())
                    .filter(|&i| {
                        let c = &g.commits[i];
                        c.subject.to_lowercase().contains(&f)
                            || c.author.to_lowercase().contains(&f)
                            || c.hash.starts_with(&f)
                            || c.refs.iter().any(|r| r.to_lowercase().contains(&f))
                    })
                    .collect()
            };
            if idx.is_empty() {
                ui.label(RichText::new(t!("Aucun commit", "No commit")).weak());
                return;
            }
            // ↑ / ↓ move the selection (and keep it in view)
            let pos = h.selected.filter(|_| h.wip_sel.is_none()).and_then(|s| idx.iter().position(|&i| i == s));
            let moved = match keys.0 {
                1 => Some(pos.map_or(0, |p| (p + 1).min(idx.len() - 1))),
                -1 => Some(pos.map_or(0, |p| p.saturating_sub(1))),
                _ => None,
            };
            if let Some(k) = moved {
                select = Some(idx[k]);
            }
            let graph_w = g.lanes.clamp(1, 24) as f32 * history::LANE_W + 6.0;
            let row_h = 22.0;
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut area = egui::ScrollArea::vertical().id_salt("history_list").auto_shrink([false, false]);
            // a commit chosen from elsewhere ("voir dans le graphe"): in the middle of the list
            if std::mem::take(&mut h.scroll_to_sel) {
                if let Some(k) = pos {
                    area = area.vertical_scroll_offset((k as f32 * row_h - ui.available_height() / 2.0).max(0.0));
                }
            }
            if let Some(k) = moved {
                let id = ui.make_persistent_id("history_list");
                let top = egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |st| st.offset.y);
                let (y, view_h) = (k as f32 * row_h, ui.available_height());
                if y < top {
                    area = area.vertical_scroll_offset(y);
                } else if y + row_h > top + view_h {
                    area = area.vertical_scroll_offset(y + row_h - view_h);
                }
            }
            area.show_rows(ui, row_h, idx.len(), |ui, range| {
                for k in range {
                    let i = idx[k];
                    let c = &g.commits[i];
                    let r = history::row_ui(ui, c, &g.rows[i], graph_w, h.selected == Some(i) && h.wip_sel.is_none() && h.compare.is_none(), f.is_empty());
                    if r.clicked() || r.secondary_clicked() {
                        select = Some(i);
                    }
                    if r.double_clicked() {
                        diff_to_tab = true;
                    }
                    let sel = h.selected.filter(|&s| s != i).map(|s| g.commits[s].hash.as_str());
                    r.context_menu(|ui| {
                        commit_menu(ui, c, &MenuCtx { local: &g.local, current: &current, bisecting }, sel, &mut menu_act);
                    });
                }
            });
        };
        // one of the two can take all the room
        let view = egui::Id::new("git_view");
        let maxed = maximize::maxed(ui.ctx(), view);
        if maxed == Some("detail") && !has_detail {
            maximize::restore(ui.ctx(), view);
        }
        if !has_detail || maxed == Some("list") {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                list(ui);
                if has_detail {
                    maximize::corner(ui, view, "list");
                }
            });
        } else {
            if maxed != Some("detail") {
                let avail = ui.available_height();
                egui::TopBottomPanel::top("history_list_panel")
                    .resizable(true)
                    .default_height((avail * 0.36).max(140.0))
                    .height_range(100.0..=(avail - 160.0).max(120.0))
                    .show_inside(ui, |ui| {
                        list(ui);
                        maximize::corner(ui, view, "list");
                    });
            }

            // ------------------------------------------------ below: what is selected, then its changes (like a pull request)
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.add_space(4.0);
                let r = ui.max_rect();
                if h.rebase.is_none() {
                    let at = egui::Rect::from_min_size(egui::pos2(r.right() - 26.0, r.top() + 2.0), egui::vec2(24.0, 24.0));
                    if ui.put(at, egui::Button::new(RichText::new("×").size(16.0)).frame(false)).on_hover_text(t!("Fermer le détail (Échap)", "Close the details (Escape)")).clicked() {
                        close_detail = true;
                    }
                }
                let at = egui::Rect::from_min_size(egui::pos2(r.right() - if h.rebase.is_none() { 50.0 } else { 24.0 }, r.top() + 4.0), egui::vec2(20.0, 20.0));
                maximize::button_at(ui, at, view, "detail");
                if let Some(rb) = h.rebase.as_mut() {
                    ui.label(RichText::new(t!("Rebase interactif", "Interactive rebase")).size(16.0).strong());
                    ui.label(
                        RichText::new(t!(
                            "Du plus ancien (en haut) au plus récent. Change l'ordre avec ↑ ↓ et choisis quoi faire de chaque commit. \
                             « Découper » s'arrête sur le commit et ouvre l'assistant de découpage. \
                             Les commits sont réécrits : si la branche est déjà poussée, il faudra ensuite « ⬆ Push --force-with-lease » (menu ↕ Synchroniser).",
                            "From the oldest (at the top) to the newest. Change the order with ↑ ↓ and choose what to do with each commit. \
                             “Split” stops on the commit and opens the splitting assistant. \
                             The commits are rewritten: if the branch is already pushed, “⬆ Push --force-with-lease” (↕ Sync menu) will be needed afterwards."
                        ))
                        .weak()
                        .small(),
                    );
                    ui.add_space(4.0);
                    let n = rb.steps.len();
                    let mut swap = None;
                    egui::ScrollArea::vertical().id_salt("rebase_steps").max_height(ui.available_height() - 70.0).show(ui, |ui| {
                        for (k, st) in rb.steps.iter_mut().enumerate() {
                            egui::Frame::none()
                                .fill(ui.visuals().faint_bg_color)
                                .rounding(6.0)
                                .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                                .outer_margin(egui::Margin { bottom: 3.0, ..Default::default() })
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        if ui.add_enabled(k > 0, egui::Button::new("↑").small()).on_hover_text(t!("Plus tôt", "Earlier")).clicked() {
                                            swap = Some((k, k - 1));
                                        }
                                        if ui.add_enabled(k + 1 < n, egui::Button::new("↓").small()).on_hover_text(t!("Plus tard", "Later")).clicked() {
                                            swap = Some((k, k + 1));
                                        }
                                        egui::ComboBox::from_id_salt(("rebase_action", k)).selected_text(st.action.label()).width(220.0).show_ui(ui, |ui| {
                                            for a in crate::rebase::Action::ALL {
                                                ui.selectable_value(&mut st.action, a, a.label());
                                            }
                                        });
                                        ui.label(RichText::new(&st.short).monospace().color(accent));
                                        let mut subj = RichText::new(&st.subject);
                                        if st.action == crate::rebase::Action::Drop {
                                            subj = subj.strikethrough().weak();
                                        }
                                        ui.add(egui::Label::new(subj).truncate());
                                    });
                                    match st.action {
                                        crate::rebase::Action::Reword => {
                                            ui.add(egui::TextEdit::multiline(&mut st.message).desired_rows(2).desired_width(f32::INFINITY));
                                        }
                                        crate::rebase::Action::Squash => {
                                            ui.label(RichText::new(t!("↳ fusionné dans le commit gardé au-dessus (messages mis bout à bout)", "↳ merged into the kept commit above (messages joined)")).small().weak());
                                        }
                                        crate::rebase::Action::Fixup => {
                                            ui.label(RichText::new(t!("↳ fusionné dans le commit gardé au-dessus (son message est oublié)", "↳ merged into the kept commit above (its message is dropped)")).small().weak());
                                        }
                                        crate::rebase::Action::Split => {
                                            ui.label(RichText::new(t!("↳ le rebase s'arrête ici : tu répartis ses changements (blocs ou lignes) en plusieurs commits", "↳ the rebase stops here: you share its changes (blocks or lines) out into several commits")).small().weak());
                                        }
                                        _ => {}
                                    }
                                });
                        }
                    });
                    if let Some((a, b)) = swap {
                        rb.steps.swap(a, b);
                    }
                    ui.separator();
                    let check = crate::rebase::check(&rb.steps);
                    if let Err(e) = &check {
                        ui.label(RichText::new(e).color(err()));
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(check.is_ok(), filled_button(t!("Lancer le rebase", "Run the rebase"), accent)).clicked() {
                            rb_run = true;
                        }
                        if ui.button(t!("Annuler", "Cancel")).clicked() {
                            rb_cancel = true;
                        }
                    });
                    return;
                }
                // --- header of what is selected
                let (mut chips, mut file_buttons): (Vec<(char, String, Option<(Option<u32>, Option<u32>)>)>, Vec<history::DiffButton>) = (Vec::new(), Vec::new());
                if let Some((_, _, title)) = &h.compare {
                    ui.label(RichText::new(title).size(16.0).strong());
                    if let Some(d) = &h.details {
                        if !d.message.is_empty() {
                            ui.label(RichText::new(&d.message).weak().small());
                        }
                        chips = d.files.clone();
                    }
                    file_buttons = vec![(t!("Ouvrir", "Open"), t!("Ouvrir le fichier (tel qu'il est dans tes fichiers)", "Open the file (as it is in your files)"), DiffAct::Open)];
                } else if let Some(staged) = h.wip_sel {
                    let (title, color) = if staged { (t!("Modifications indexées", "Staged changes"), ok()) } else { (t!("Modifications non indexées", "Unstaged changes"), warn()) };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).size(16.0).strong().color(color));
                        ui.label(
                            RichText::new(if staged {
                                t!("elles iront dans le prochain commit", "they will go in the next commit")
                            } else {
                                t!("pas encore indexées : « + Indexer » pour les ajouter au prochain commit", "not staged yet: “+ Stage” to add them to the next commit")
                            })
                            .weak(),
                        );
                    });
                    if let Some(st) = &wip {
                        ui.horizontal(|ui| {
                            if staged {
                                if ui.button(t!("Tout désindexer", "Unstage all")).clicked() {
                                    wip_ops.push(vec!["restore".into(), "--staged".into(), ".".into()]);
                                }
                            } else if ui.button(t!("Tout indexer", "Stage all")).clicked() {
                                wip_ops.push(vec!["add".into(), "-A".into()]);
                            }
                            let tip = t!("Claude relit tout ce qui n'est pas commité (indexé ou non, nouveaux fichiers compris) et propose des remarques sous les lignes : tu gardes celles qui comptent", "Claude reads over everything not committed (staged or not, new files included) and suggests remarks under the lines: you keep the ones that matter");
                            if crate::features::on(crate::features::Feature::Claude) && ui.add_enabled(!ai.loading, egui::Button::new(t!("✳ Revue IA", "✳ AI review"))).on_hover_text(tip).clicked() {
                                ai_start = true;
                            }
                        });
                        chips = st
                            .entries
                            .iter()
                            .filter(|e| if staged { e.staged() } else { e.unstaged() })
                            .map(|e| {
                                let stats = if staged { st.staged_stats.get(&e.path) } else { st.unstaged_stats.get(&e.path) };
                                (if staged { e.x } else { e.label() }, e.path.clone(), stats.copied())
                            })
                            .collect();
                    }
                    file_buttons = if staged {
                        vec![
                            (t!("Ouvrir", "Open"), t!("Ouvrir le fichier", "Open the file"), DiffAct::Open),
                            (t!("− Désindexer", "− Unstage"), t!("Le retirer du prochain commit (les modifications restent dans le fichier)", "Take it out of the next commit (the changes stay in the file)"), DiffAct::Unstage),
                        ]
                    } else {
                        vec![
                            (t!("Ouvrir", "Open"), t!("Ouvrir le fichier", "Open the file"), DiffAct::Open),
                            (t!("+ Indexer", "+ Stage"), t!("Ajouter tout le fichier au prochain commit", "Add the whole file to the next commit"), DiffAct::Stage),
                            (t!("↺ Annuler", "↺ Discard"), t!("Remettre le fichier comme au dernier commit (demande confirmation)", "Put the file back as at the last commit (asks for confirmation)"), DiffAct::Discard),
                        ]
                    };
                } else if let Some(c) = &selected_commit {
                    match &h.details {
                        None => {
                            spinner(ui);
                        }
                        Some(d) => {
                            let (subject, body) = d.message.split_once('\n').unwrap_or((&d.message, ""));
                            ui.horizontal(|ui| {
                                ui.add(egui::Label::new(RichText::new(subject).size(16.0).strong()).truncate());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add_space(52.0); // the × closing the detail, the button taking all the room
                                    ui.menu_button("Actions ▾", |ui| {
                                        let local = h.graph.as_ref().map_or(&[][..], |g| &g.local[..]);
                                        commit_menu(ui, c, &MenuCtx { local, current: &current, bisecting }, None, &mut menu_act);
                                    });
                                    if ui.button(t!("Diff dans un onglet", "Diff in a tab")).on_hover_text(t!("Tout le diff du commit dans un onglet de l'éditeur (double-clic sur le commit)", "The whole diff of the commit in an editor tab (double-click on the commit)")).clicked() {
                                        diff_to_tab = true;
                                    }
                                });
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label(RichText::new(&c.hash[..c.hash.len().min(8)]).monospace().color(accent));
                                ui.label(RichText::new(format!("· {} · {}", d.author.split(" <").next().unwrap_or(&d.author), d.date)).weak());
                                for r in &c.refs {
                                    ui.label(RichText::new(r.strip_prefix("HEAD -> ").unwrap_or(r)).small().color(accent).background_color(icons::mix(ui.visuals().panel_fill, accent, 0.15)));
                                }
                                for pr in &commit_links {
                                    let (state, color) = super::pulls::state_badge(&pr.state, false);
                                    let tip = if pr.title.is_empty() { tf!("Pull request {state} — clic : l'ouvrir", "Pull request {state} — click: open it") } else { tf!("{}\n{state} — clic : l'ouvrir", "{}\n{state} — click: open it", pr.title) };
                                    if ui.add(egui::Button::new(RichText::new(format!("⇄ PR #{}", pr.number)).small().color(color)).small()).on_hover_text(tip).clicked() {
                                        open_pr = Some(pr.number);
                                    }
                                }
                            });
                            let body = body.trim();
                            if !body.is_empty() {
                                egui::CollapsingHeader::new(RichText::new(t!("Message complet", "Full message")).small().weak())
                                    .default_open(body.lines().count() <= 6)
                                    .show(ui, |ui| {
                                        egui::ScrollArea::vertical().id_salt("commit_msg").max_height(120.0).auto_shrink([false, true]).show(ui, |ui| {
                                            ui.add(egui::Label::new(RichText::new(history::reflow(body)).weak()).wrap());
                                        });
                                    });
                            }
                            chips = d.files.clone();
                        }
                    }
                    file_buttons = vec![
                        (t!("Ouvrir", "Open"), t!("Ouvrir le fichier (tel qu'il est dans tes fichiers)", "Open the file (as it is in your files)"), DiffAct::Open),
                        (
                            t!("↺ Annuler dans mes fichiers", "↺ Undo in my files"),
                            t!("Défaire ce que ce commit a changé dans ce fichier, dans tes fichiers actuels (à valider ensuite)", "Undo what this commit changed in this file, in your current files (to commit afterwards)"),
                            DiffAct::Revert,
                        ),
                    ];
                }
                ui.add_space(2.0);
                ui.separator();
                // --- files on the left (click: go to its diff), every diff one under the other on the right
                let (adds, dels) = chips.iter().fold((0u32, 0u32), |(a, r), (_, _, s)| match s {
                    Some((x, y)) => (a + x.unwrap_or(0), r + y.unwrap_or(0)),
                    _ => (a, r),
                });
                egui::SidePanel::left("history_files")
                    .resizable(true)
                    .default_width(300.0)
                    .width_range(180.0..=600.0)
                    .frame(egui::Frame::none().inner_margin(egui::Margin { left: 0.0, right: 6.0, top: 2.0, bottom: 0.0 }))
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tf!("{} FICHIER(S)", "{} FILE(S)", chips.len())).small().strong().color(accent));
                            ui.label(RichText::new(format!("+{adds}")).small().monospace().color(ok()));
                            ui.label(RichText::new(format!("−{dels}")).small().monospace().color(err()));
                            if h.diff_loading {
                                spinner(ui);
                            }
                        });
                        ui.add_space(2.0);
                        egui::ScrollArea::vertical().id_salt("diff_files").auto_shrink([false, false]).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (letter, path, stats) in &chips {
                                if file_row(ui, *letter, path, *stats).clicked() {
                                    if let Some(k) = h.diff.as_ref().and_then(|d| d.files.iter().position(|f| f.path == *path)) {
                                        if let Some(start) = h.diff.as_ref().map(|d| d.files[k].start) {
                                            h.collapsed.remove(&start);
                                        }
                                        h.jump = Some(history::FileJump::To(k));
                                    }
                                }
                            }
                        });
                    });
                egui::CentralPanel::default().frame(egui::Frame::none().inner_margin(egui::Margin { left: 6.0, ..Default::default() })).show_inside(ui, |ui| {
                    if let Some(d) = h.diff.as_ref().filter(|d| !d.files.is_empty()) {
                        // in a row: alone, the right-to-left layout would take the whole height
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button(t!("Tout déplier", "Unfold all")).clicked() {
                                    h.collapsed.clear();
                                }
                                if ui.small_button(t!("Tout replier", "Fold all")).clicked() {
                                    h.collapsed = d.files.iter().map(|f| f.start).collect();
                                }
                            });
                        });
                    }
                    // the AI review of the changes not committed: its state, then its remarks under their lines
                    let wip_view = matches!(source, DiffSource::Worktree | DiffSource::Index);
                    let ai_rows = if wip_view { ai.rows(h.diff.as_ref()) } else { Vec::new() };
                    if wip_view {
                        ai_review::review_bar(ui, cache, ai, &ai_rows, accent, &mut ai_act, |ui, kept| {
                            let tip = t!("Écrit les remarques gardées dans l'onglet Claude : tu relis, puis tu envoies", "Writes the kept remarks in the Claude tab: you read them over, then send");
                            ai_fix = ui.add_enabled(kept > 0, filled_button(tf!("Faire corriger les {kept} gardée(s) par Claude", "Have Claude fix the {kept} kept"), accent)).on_hover_text(tip).clicked();
                        });
                    }
                    let mut ai_at: HashMap<usize, Vec<usize>> = HashMap::new();
                    for (k, row) in ai_rows.iter().enumerate() {
                        if let Some(i) = row {
                            ai_at.entry(*i).or_default().push(k);
                        }
                    }
                    let ai_lines: HashSet<usize> = ai_at.keys().copied().collect();
                    let ai_notes = &mut ai.notes;
                    let mut show_ai = |ui: &mut egui::Ui, i: usize| {
                        for &k in ai_at.get(&i).map_or(&[][..], |v| &v[..]) {
                            ai_review::note_card(ui, cache, k, &mut ai_notes[k], true, accent, &mut ai_act);
                        }
                    };
                    match &h.diff {
                        Some(d) => {
                            let hunk_buttons: Vec<history::DiffButton> = match source {
                                DiffSource::Worktree => vec![
                                    (t!("+ Indexer ce bloc", "+ Stage this block"), t!("Ajouter ce bloc au prochain commit", "Add this block to the next commit"), DiffAct::Stage),
                                    (t!("↺ Annuler ce bloc", "↺ Discard this block"), t!("Supprimer ce changement du fichier (demande confirmation)", "Remove this change from the file (asks for confirmation)"), DiffAct::Discard),
                                ],
                                DiffSource::Index => {
                                    vec![(t!("− Désindexer ce bloc", "− Unstage this block"), t!("Retirer ce bloc du prochain commit (le changement reste dans le fichier)", "Take this block out of the next commit (the change stays in the file)"), DiffAct::Unstage)]
                                }
                                DiffSource::Commit => vec![(t!("↺ Annuler ce changement", "↺ Undo this change"), t!("Défaire ce bloc dans tes fichiers actuels (à valider ensuite)", "Undo this block in your current files (to commit afterwards)"), DiffAct::Revert)],
                                DiffSource::Compare => Vec::new(),
                            };
                            let jump = h.jump.take();
                            let notes = if ai_lines.is_empty() { None } else { Some(history::Notes { at: &ai_lines, show: &mut show_ai }) };
                            if let Some(c) = history::stacked_diff_view(ui, &d.lines, &d.files, &mut h.collapsed, "history_stacked", &hunk_buttons, &file_buttons, jump, notes) {
                                diff_click = Some(c);
                            }
                        }
                        None if h.diff_loading => {
                            ui.horizontal(|ui| {
                                spinner(ui);
                                ui.label(RichText::new(t!("Chargement du diff…", "Loading the diff…")).weak());
                            });
                        }
                        None => {}
                    }
                });
            });
        }

        if close_detail {
            self.close_history_detail();
        }
        if start_amend {
            self.start_amend();
        }
        if do_commit {
            self.commit(n_staged);
        }
        if let (Some(verb), Some(repo)) = (bisect_cmd, self.history.repo.clone()) {
            self.git_job_at(repo, "bisect", &["bisect", verb]);
        }
        if suggest_msg {
            self.suggest_commit_message();
        }
        if ask_plan {
            self.start_commit_plan();
        }
        if let (Some(n), Some(repo)) = (open_pr, self.history.repo.clone()) {
            self.open_pr(repo, n);
        }
        if clear_notice {
            self.history.notice = None;
        }
        if let Some(c) = diff_click {
            self.diff_action(source, c);
        }
        if diff_to_tab {
            match &self.history.diff {
                Some(d) => {
                    let short = &d.hash[..d.hash.len().min(8)];
                    let title = match &d.path {
                        Some(p) => format!("{short} : {p}"),
                        None => format!("diff {short}"),
                    };
                    let text = d.text.clone();
                    self.new_scratch(title, text, "Diff", true);
                }
                None => self.set_status(t!("Le diff est en cours de chargement", "The diff is loading")),
            }
        }
        if resume {
            if self.split.is_some() {
                self.show_tab(document::View::Split);
            } else {
                self.resume_split();
            }
        }
        if abort_rebase {
            if let Some(repo) = self.history.repo.clone() {
                self.git_job_at(repo, "rebase --abort", &["rebase", "--abort"]);
            }
        }
        if let Some(a) = menu_act {
            let repo = self.history.repo.clone();
            match (a, repo) {
                (HistAct::Checkout(target), Some(repo)) => self.git_job_at(repo, "checkout", &["checkout", &target]),
                (HistAct::BranchAt(hash), _) => {
                    self.open_overlay_with(Mode::Prompt(Prompt::BranchAt(hash)), Vec::new(), String::new())
                }
                (HistAct::Copy(text), _) => {
                    ui.ctx().output_mut(|o| o.copied_text = text.clone());
                    self.set_status(tf!("Copié : {text}", "Copied: {text}"));
                }
                (HistAct::Diff(hash), _) => {
                    if let Some(i) = self.history.graph.as_ref().and_then(|g| g.commits.iter().position(|c| c.hash == hash)) {
                        select = Some(i);
                    }
                }
                (HistAct::Split(hash), _) => self.start_split(hash),
                (HistAct::Claude(hash), Some(repo)) => self.claude_attach_commit(&repo, &hash),
                (HistAct::CherryPick(hash), Some(repo)) => self.git_job_at(repo, "cherry-pick", &["-c", "core.editor=true", "cherry-pick", &hash]),
                (HistAct::Revert(hash), Some(repo)) => self.git_job_at(repo, "revert", &["-c", "core.editor=true", "revert", "--no-edit", &hash]),
                (HistAct::Compare(a, b, title), _) => {
                    self.start_compare(a.clone(), b.clone(), title);
                    self.load_range_diff(a, b, None);
                }
                (HistAct::Rebase(hash), Some(repo)) => match crate::rebase::load(&repo, &hash) {
                    Ok(steps) => {
                        let h = &mut self.history;
                        h.rebase = Some(RebaseUi { from: hash, steps });
                        h.compare = None;
                        h.wip_sel = None;
                        h.diff = None;
                    }
                    Err(e) => self.history.notice = Some((e, err())),
                },
                (a, Some(repo)) => self.hist_git_act(a, repo),
                _ => {}
            }
        }
        if let Some(args) = op_cmd {
            if let Some(repo) = self.history.repo.clone() {
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                self.git_job_at(repo, "rebase/merge", &a);
            }
        }
        if rb_cancel {
            self.history.rebase = None;
        }
        if rb_run {
            if let (Some(rb), Some(repo)) = (self.history.rebase.take(), self.history.repo.clone()) {
                self.git.busy += 1;
                self.set_status(t!("Rebase en cours…", "Rebase in progress…"));
                self.jobs.spawn(move || JobResult::GitOp {
                    label: if crate::rebase::splits(&rb.steps) { t!("rebase (découpage)", "rebase (splitting)").into() } else { "rebase".into() },
                    result: crate::rebase::run(&repo, &rb.from, &rb.steps, crate::rebase::self_editor),
                    status: None,
                });
            }
        }
        if let Some(staged) = wip_pick {
            self.show_wip(staged);
        }
        if ai_start {
            self.ai_review_wip();
        }
        if ai_fix {
            self.ai_fix_with_claude();
        }
        if let Some(a) = ai_act {
            self.ai_wip_act(a);
        }
        if !wip_ops.is_empty() {
            // everything leaves the side shown: empty at once, git's new diff follows
            if let Some(d) = self.history.diff.as_mut() {
                d.text = t!("(aucune différence)", "(no difference)").into();
                d.lines = history::parse_diff(&d.text);
                d.files = Vec::new();
            }
            if let Some(repo) = self.history.repo.clone() {
                for args in wip_ops {
                    let a: Vec<&str> = args.iter().map(String::as_str).collect();
                    let label = if args[0] == "add" { "add" } else { "restore" };
                    self.git_job_at(repo.clone(), label, &a);
                }
            }
        }
        if let Some(i) = select {
            if Some(i) != self.history.selected || self.history.wip_sel.is_some() || self.history.compare.is_some() || self.history.rebase.is_some() {
                let h = &mut self.history;
                h.wip_sel = None;
                h.compare = None;
                h.rebase = None;
                h.diff = None;
                h.diff_loading = false;
                self.select_commit(i);
            }
        }
    }
}

/// `git <args>` (a diff of the working tree), followed by the new files compared with nothing.
pub(super) fn worktree_diff(repo: &Path, args: &[&str]) -> Result<String, String> {
    git::run(repo, args).map(|mut text| {
        // new files are not in `git diff`: compared with nothing, one by one
        let others = git::stdout(repo, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
        for p in others.lines().filter(|p| !p.is_empty()).take(200) {
            if std::fs::metadata(repo.join(p)).is_ok_and(|m| m.len() > 2_000_000) {
                continue;
            }
            let d = match git::run(repo, &["diff", "--no-index", "--", "/dev/null", p]) {
                Err(e) if e.starts_with("diff --git") => e,
                Ok(o) => o,
                Err(_) => continue,
            };
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&d);
        }
        text
    })
}

/// Message and button of the next commit, ✨ to have it written by Claude, ✂ to have Claude split
/// the changes into several commits; returns (commit now, ask the message, ask the split).
#[allow(clippy::too_many_arguments)]
fn commit_box(ui: &mut egui::Ui, msg: &mut String, amend: &mut bool, pushed: bool, focus: bool, n_staged: usize, n_changed: usize, suggesting: bool, planning: bool, accent: Color32) -> (bool, bool, bool) {
    let claude_on = crate::features::on(crate::features::Feature::Claude);
    let (label, tip) = if *amend && n_staged > 0 {
        (tf!("✔ Modifier le dernier commit (+{n_staged} indexé(s))", "✔ Amend the last commit (+{n_staged} staged)"), t!("Le dernier commit est remplacé : ce message, et les fichiers indexés en plus", "The last commit is replaced: this message, and the staged files added"))
    } else if *amend {
        (t!("✔ Modifier le dernier commit", "✔ Amend the last commit").to_string(), t!("Rien d'indexé : seul le message du dernier commit change", "Nothing staged: only the message of the last commit changes"))
    } else if n_staged > 0 {
        (tf!("✔ Commit ({n_staged} indexé(s))", "✔ Commit ({n_staged} staged)"), t!("Valide les fichiers indexés", "Commits the staged files"))
    } else {
        (tf!("✔ Commit tout ({n_changed})", "✔ Commit all ({n_changed})"), t!("Rien d'indexé : valide toutes les modifications des fichiers suivis", "Nothing staged: commits all the changes of the tracked files"))
    };
    let can = !msg.trim().is_empty() && (*amend || n_staged > 0 || n_changed > 0);
    let (mut go, mut suggest, mut plan) = (false, false, false);
    egui::Frame::none().inner_margin(egui::Margin { left: 4.0, right: 4.0, top: 4.0, bottom: 6.0 }).show(ui, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            let clicked = ui.add_enabled(can, filled_button(label, accent)).on_hover_text(tip).clicked();
            if !claude_on {
            } else if planning {
                spinner(ui);
            } else if ui.button("✂").on_hover_text(t!("Claude découpe les changements (tous, nouveaux fichiers compris) en plusieurs commits par thème, avec leurs messages : tu relis et modifies le plan avant de commiter", "Claude splits the changes (all of them, new files included) into several commits by theme, with their messages: you read and change the plan before committing")).clicked() {
                plan = true;
            }
            if !claude_on {
            } else if suggesting {
                spinner(ui);
            } else if ui.button("✨").on_hover_text(t!("Claude propose le message d'après les changements (indexés, sinon tous) et ta demande dans l'onglet Claude", "Claude suggests the message from the changes (staged, else all) and your request in the Claude tab")).clicked() {
                suggest = true;
            }
            let resp = ui.add(egui::TextEdit::multiline(msg).hint_text(t!("Message du commit (Ctrl+Entrée)", "Commit message (Ctrl+Enter)")).desired_rows(1).desired_width(f32::INFINITY));
            if focus {
                resp.request_focus();
            }
            let ctrl_enter = resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
            go = can && (clicked || ctrl_enter);
        });
        ui.horizontal(|ui| {
            ui.checkbox(amend, t!("Modifier le dernier commit", "Amend the last commit"))
                .on_hover_text(t!("git commit --amend : remplace le dernier commit (message, et fichiers indexés en plus) au lieu d'en créer un", "git commit --amend: replaces the last commit (message, and staged files added) instead of making a new one"));
            if *amend && pushed {
                ui.label(RichText::new(t!("⚠ déjà poussé : il faudra ensuite un push --force-with-lease", "⚠ already pushed: a push --force-with-lease will be needed afterwards")).small().color(warn()));
            }
        });
    });
    (go, suggest, plan)
}

/// A changed file in the list next to the diff: status letter, name, folder, lines added / removed.
pub(super) fn file_row(ui: &mut egui::Ui, letter: char, path: &str, stats: Option<(Option<u32>, Option<u32>)>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
    let v = ui.visuals();
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let (letter, color) = match letter {
        'A' | '?' => ('A', ok()),
        'D' => ('D', err()),
        'R' | 'C' => (letter, Color32::from_rgb(0x8a, 0x7c, 0xd8)),
        'U' => ('U', err()),
        _ => ('M', warn()),
    };
    let p = ui.painter().with_clip_rect(rect);
    let cy = rect.center().y;
    // the letter in a small colored square
    let badge = egui::Rect::from_center_size(egui::pos2(rect.left() + 12.0, cy), egui::vec2(16.0, 16.0));
    p.rect_filled(badge, 3.0, icons::mix(v.panel_fill, color, 0.18));
    p.text(badge.center(), Align2::CENTER_CENTER, letter, egui::FontId::monospace(11.0), color);
    // lines added / removed on the right, the name and its folder in the rest
    let mut right = rect.right() - 4.0;
    if let Some((a, d)) = stats {
        let d = d.map_or("bin".to_string(), |d| format!("−{d}"));
        let r = p.text(egui::pos2(right, cy), Align2::RIGHT_CENTER, d, egui::FontId::monospace(11.0), err());
        let r = p.text(egui::pos2(r.left() - 5.0, cy), Align2::RIGHT_CENTER, a.map_or(String::new(), |a| format!("+{a}")), egui::FontId::monospace(11.0), ok());
        right = r.left() - 8.0;
    }
    let p = p.with_clip_rect(egui::Rect::from_min_max(rect.min, egui::pos2(right, rect.bottom())));
    let (dir, name) = path.rsplit_once('/').map_or(("", path), |(d, n)| (d, n));
    let strike = letter == 'D';
    let r = p.text(egui::pos2(rect.left() + 26.0, cy), Align2::LEFT_CENTER, name, egui::FontId::proportional(13.0), if strike { v.weak_text_color() } else { v.strong_text_color() });
    if strike {
        p.line_segment([egui::pos2(r.left(), cy), egui::pos2(r.right(), cy)], egui::Stroke::new(1.0_f32, v.weak_text_color()));
    }
    if !dir.is_empty() {
        p.text(egui::pos2(r.right() + 6.0, cy), Align2::LEFT_CENTER, dir, egui::FontId::proportional(11.5), v.weak_text_color());
    }
    resp.on_hover_text(path)
}
