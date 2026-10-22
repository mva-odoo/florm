//! Git view: commit graph, diffs, and what is done from them.

use super::*;
use history::DiffAct;

impl App {
    pub(super) fn load_history(&mut self) {
        let Some(repo) = self.repo.clone() else {
            self.history.error = Some("Aucun dépôt git dans ce dossier.".into());
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
            self.set_status("Le fichier n'est pas enregistré");
            return;
        };
        let line = d.rope.char_to_line(d.primary_sel().head);
        let name = d.title();
        let Some(head_line) = d.head_line(line) else {
            self.set_status("Ligne ajoutée depuis le dernier commit : pas encore d'historique");
            return;
        };
        self.line_history = Some(LineHistoryUi {
            title: format!("Historique de la ligne {} — {name}", line + 1),
            loading: true,
            error: None,
            commits: Vec::new(),
            diffs: Vec::new(),
            selected: 0,
        });
        self.jobs.spawn(move || JobResult::LineHistory(git::line_history(&path, head_line + 1)));
    }

    pub(super) fn line_history_ui(&mut self, ctx: &egui::Context) {
        let Some(lh) = self.line_history.as_mut() else { return };
        let mut open = true;
        let (mut show_graph, mut copy) = (None, None);
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
                        ui.spinner();
                        ui.label("Recherche des commits qui ont modifié cette ligne…");
                    });
                    return;
                }
                if let Some(e) = &lh.error {
                    ui.label(RichText::new(e).color(err()));
                    return;
                }
                if lh.commits.is_empty() {
                    ui.label(RichText::new("Aucun commit trouvé pour cette ligne.").weak());
                    return;
                }
                ui.set_min_size(ui.available_size());
                let height = ui.available_height();
                ui.horizontal_top(|ui| {
                    // commit list
                    ui.vertical(|ui| {
                        ui.set_width(320.0);
                        ui.label(RichText::new(format!("{} COMMIT(S) ONT MODIFIÉ CETTE LIGNE", lh.commits.len())).small().strong());
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
                                if r.clicked() {
                                    lh.selected = i;
                                }
                            }
                        });
                    });
                    ui.separator();
                    // selected commit: header + diff
                    ui.vertical(|ui| {
                        let c = &lh.commits[lh.selected];
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&c.subject).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Voir dans le graphe").clicked() {
                                    show_graph = Some(c.hash.clone());
                                }
                                if ui.button("Copier le hash").clicked() {
                                    copy = Some(c.hash.clone());
                                }
                            });
                        });
                        ui.label(RichText::new(format!("{} · {} · {}", c.author, c.date, &c.hash[..c.hash.len().min(10)])).weak());
                        ui.separator();
                        let _ = history::diff_view(ui, &lh.diffs[lh.selected], "line_history_diff", &[]);
                    });
                });
            });
        if !open {
            self.line_history = None;
        }
        if let Some(h) = copy {
            ctx.output_mut(|o| o.copied_text = h);
            self.set_status("Hash copié");
        }
        if let Some(h) = show_graph {
            self.line_history = None;
            if let Some(r) = self.docs.get(self.active).and_then(|d| d.path.clone()).and_then(|p| self.repo_for(&p)) {
                self.select_repo(r);
            }
            self.history.open = true;
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

    fn start_compare(&mut self, a: String, b: String, title: String) {
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
        let lose = |what: &str| format!("{what}\n\nCe changement n'est pas encore dans un commit : il sera perdu.");
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
                _ => ("rm", DiffOp::Delete(repo.join(&path)), Some(lose(&format!("Supprimer le nouveau fichier « {path} » ?")))),
            },
            DiffClick::File(_, DiffAct::Unstage) => ("restore", DiffOp::Git(vec!["restore".into(), "--staged".into(), "--".into(), path.clone()]), None),
            DiffClick::File(_, DiffAct::Discard) if untracked => {
                ("rm", DiffOp::Delete(repo.join(&path)), Some(lose(&format!("Supprimer le nouveau fichier « {path} » ?"))))
            }
            DiffClick::File(_, DiffAct::Discard) => (
                "restore",
                DiffOp::Git(vec!["restore".into(), "--".into(), path.clone()]),
                Some(lose(&format!("Annuler toutes les modifications non indexées de « {path} » ?"))),
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
                        Some(lose(&format!("Annuler ce bloc de modifications dans « {path} » (vers la ligne {line}) ?"))),
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
                DiffOp::Delete(p) => std::fs::remove_file(&p).map(|()| format!("{} supprimé", p.display())).map_err(|e| e.to_string()),
            };
            JobResult::GitOp { label, result, status: Some((repo.clone(), git::status(&repo))) }
        });
    }

    /// Opens a file of the repository at a line (from a diff).
    fn jump_to_file(&mut self, repo: &Path, rel: &str, line: usize) {
        let path = repo.join(rel);
        if !path.is_file() {
            self.history.notice = Some((format!("« {rel} » n'existe plus dans tes fichiers."), err()));
            return;
        }
        if self.open_path(path) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.goto_line(line.saturating_sub(1), 0);
            }
            self.history.open = false;
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
        let commit_msg = &mut self.git.commit_msg;
        let focus_commit = std::mem::take(&mut self.git.focus_commit);
        // AI review of the changes not committed: asked, kept remarks given to Claude, a remark's action
        let (mut ai_start, mut ai_fix, mut ai_act) = (false, false, None);
        let ai = &mut self.git.ai;
        let cache = &mut self.md_cache;
        let h = &mut self.history;
        // messages and operations in progress, above the list (nothing when there are none)
        let banner = h.notice.is_some() || split_hidden.is_some() || op_banner.is_some() || h.graph.as_ref().is_some_and(|g| g.rebasing);
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
            if let Some(short) = &split_hidden {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("✂ Le découpage de {short} est en cours.")).color(warn()).strong());
                    if ui.button("Revenir au découpage").clicked() {
                        resume = true;
                    }
                });
            } else if let Some((op, n)) = op_banner {
                ui.horizontal_wrapped(|ui| {
                    let what = if n > 0 { format!(" — {n} fichier(s) en conflit à résoudre") } else { String::new() };
                    ui.label(RichText::new(format!("⚠ {} en cours{what}", op.label())).color(warn()).strong());
                    if ui.add_enabled(n == 0, egui::Button::new("Continuer")).clicked() {
                        let mut a = vec!["-c".to_string(), "core.editor=true".to_string()];
                        a.extend(op.continue_args().iter().map(|s| s.to_string()));
                        op_cmd = Some(a);
                    }
                    if ui.button("Abandonner").on_hover_text("Remet la branche comme avant l'opération").clicked() {
                        op_cmd = Some(op.abort_args().iter().map(|s| s.to_string()).collect());
                    }
                    if op == git::Operation::Rebase && ui.button("Reprendre le découpage").clicked() {
                        resume = true;
                    }
                });
            } else if h.graph.as_ref().is_some_and(|g| g.rebasing) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠ Un rebase est en cours dans ce dépôt (découpage pas terminé ?)").color(warn()));
                    if ui.button("Reprendre le découpage").clicked() {
                        resume = true;
                    }
                    if ui.button("Annuler le rebase").on_hover_text("git rebase --abort : remet la branche comme avant").clicked() {
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
        let list = |ui: &mut egui::Ui| {
            if n_staged + n_changed > 0 || !commit_msg.is_empty() || focus_commit {
                (do_commit, suggest_msg) = commit_box(ui, commit_msg, focus_commit, n_staged, n_changed, suggesting, accent);
            }
            if let Some(st) = &wip {
                let staged: Vec<&git::Entry> = st.entries.iter().filter(|e| e.staged()).collect();
                let unstaged: Vec<&git::Entry> = st.entries.iter().filter(|e| e.unstaged()).collect();
                for (is_staged, list, color, label, stats) in [
                    (false, &unstaged, warn(), "Modifications non indexées", &st.unstaged_stats),
                    (true, &staged, ok(), "Modifications indexées", &st.staged_stats),
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
                    let info = format!("{} fichier(s)", list.len());
                    let r = p.text(egui::pos2(r.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, info, egui::FontId::proportional(12.0), ui.visuals().weak_text_color());
                    let r = p.text(egui::pos2(r.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, format!("+{a}"), egui::FontId::monospace(11.5), ok());
                    p.text(egui::pos2(r.right() + 6.0, rect.center().y), Align2::LEFT_CENTER, format!("−{d}"), egui::FontId::monospace(11.5), err());
                    if resp.on_hover_text("Voir les changements, indexer / désindexer").clicked() {
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
                    None => ui.label(RichText::new("Chargement…").weak()),
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
                ui.label(RichText::new("Aucun commit").weak());
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
                        commit_menu(ui, c, &g.local, sel, &mut menu_act);
                    });
                }
            });
        };
        if !has_detail {
            egui::CentralPanel::default().show_inside(ui, list);
        } else {
            let avail = ui.available_height();
            egui::TopBottomPanel::top("history_list_panel")
                .resizable(true)
                .default_height((avail * 0.36).max(140.0))
                .height_range(100.0..=(avail - 160.0).max(120.0))
                .show_inside(ui, list);

            // ------------------------------------------------ below: what is selected, then its changes (like a pull request)
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.add_space(4.0);
                if h.rebase.is_none() {
                    let r = ui.max_rect();
                    let at = egui::Rect::from_min_size(egui::pos2(r.right() - 26.0, r.top() + 2.0), egui::vec2(24.0, 24.0));
                    if ui.put(at, egui::Button::new(RichText::new("×").size(16.0)).frame(false)).on_hover_text("Fermer le détail (Échap)").clicked() {
                        close_detail = true;
                    }
                }
                if let Some(rb) = h.rebase.as_mut() {
                    ui.label(RichText::new("Rebase interactif").size(16.0).strong());
                    ui.label(
                        RichText::new(
                            "Du plus ancien (en haut) au plus récent. Change l'ordre avec ↑ ↓ et choisis quoi faire de chaque commit. \
                             « Découper » s'arrête sur le commit et ouvre l'assistant de découpage. \
                             Les commits sont réécrits : si la branche est déjà poussée, il faudra un push --force.",
                        )
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
                                        if ui.add_enabled(k > 0, egui::Button::new("↑").small()).on_hover_text("Plus tôt").clicked() {
                                            swap = Some((k, k - 1));
                                        }
                                        if ui.add_enabled(k + 1 < n, egui::Button::new("↓").small()).on_hover_text("Plus tard").clicked() {
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
                                            ui.label(RichText::new("↳ fusionné dans le commit gardé au-dessus (messages mis bout à bout)").small().weak());
                                        }
                                        crate::rebase::Action::Fixup => {
                                            ui.label(RichText::new("↳ fusionné dans le commit gardé au-dessus (son message est oublié)").small().weak());
                                        }
                                        crate::rebase::Action::Split => {
                                            ui.label(RichText::new("↳ le rebase s'arrête ici : tu répartis ses changements (blocs ou lignes) en plusieurs commits").small().weak());
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
                        if ui.add_enabled(check.is_ok(), filled_button("Lancer le rebase", accent)).clicked() {
                            rb_run = true;
                        }
                        if ui.button("Annuler").clicked() {
                            rb_cancel = true;
                        }
                    });
                    return;
                }
                // --- header of what is selected
                let (mut chips, mut file_buttons): (Vec<(char, String, Option<(Option<u32>, Option<u32>)>)>, &[history::DiffButton]) = (Vec::new(), &[]);
                if let Some((_, _, title)) = &h.compare {
                    ui.label(RichText::new(title).size(16.0).strong());
                    if let Some(d) = &h.details {
                        if !d.message.is_empty() {
                            ui.label(RichText::new(&d.message).weak().small());
                        }
                        chips = d.files.clone();
                    }
                    file_buttons = &[("Ouvrir", "Ouvrir le fichier (tel qu'il est dans tes fichiers)", DiffAct::Open)];
                } else if let Some(staged) = h.wip_sel {
                    let (title, color) = if staged { ("Modifications indexées", ok()) } else { ("Modifications non indexées", warn()) };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).size(16.0).strong().color(color));
                        ui.label(
                            RichText::new(if staged {
                                "elles iront dans le prochain commit"
                            } else {
                                "pas encore indexées : « + Indexer » pour les ajouter au prochain commit"
                            })
                            .weak(),
                        );
                    });
                    if let Some(st) = &wip {
                        ui.horizontal(|ui| {
                            if staged {
                                if ui.button("Tout désindexer").clicked() {
                                    wip_ops.push(vec!["restore".into(), "--staged".into(), ".".into()]);
                                }
                            } else if ui.button("Tout indexer").clicked() {
                                wip_ops.push(vec!["add".into(), "-A".into()]);
                            }
                            let tip = "Claude relit tout ce qui n'est pas commité (indexé ou non, nouveaux fichiers compris) et propose des remarques sous les lignes : tu gardes celles qui comptent";
                            if ui.add_enabled(!ai.loading, egui::Button::new("✳ Revue IA")).on_hover_text(tip).clicked() {
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
                        &[
                            ("Ouvrir", "Ouvrir le fichier", DiffAct::Open),
                            ("− Désindexer", "Le retirer du prochain commit (les modifications restent dans le fichier)", DiffAct::Unstage),
                        ]
                    } else {
                        &[
                            ("Ouvrir", "Ouvrir le fichier", DiffAct::Open),
                            ("+ Indexer", "Ajouter tout le fichier au prochain commit", DiffAct::Stage),
                            ("↺ Annuler", "Remettre le fichier comme au dernier commit (demande confirmation)", DiffAct::Discard),
                        ]
                    };
                } else if let Some(c) = &selected_commit {
                    match &h.details {
                        None => {
                            ui.spinner();
                        }
                        Some(d) => {
                            let (subject, body) = d.message.split_once('\n').unwrap_or((&d.message, ""));
                            ui.horizontal(|ui| {
                                ui.add(egui::Label::new(RichText::new(subject).size(16.0).strong()).truncate());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add_space(28.0); // the × closing the detail
                                    ui.menu_button("Actions ▾", |ui| {
                                        commit_menu(ui, c, h.graph.as_ref().map_or(&[][..], |g| &g.local[..]), None, &mut menu_act);
                                    });
                                    if ui.button("Diff dans un onglet").on_hover_text("Tout le diff du commit dans un onglet de l'éditeur (double-clic sur le commit)").clicked() {
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
                                    let tip = if pr.title.is_empty() { format!("Pull request {state} — clic : l'ouvrir") } else { format!("{}\n{state} — clic : l'ouvrir", pr.title) };
                                    if ui.add(egui::Button::new(RichText::new(format!("⇄ PR #{}", pr.number)).small().color(color)).small()).on_hover_text(tip).clicked() {
                                        open_pr = Some(pr.number);
                                    }
                                }
                            });
                            let body = body.trim();
                            if !body.is_empty() {
                                egui::CollapsingHeader::new(RichText::new("Message complet").small().weak())
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
                    file_buttons = &[
                        ("Ouvrir", "Ouvrir le fichier (tel qu'il est dans tes fichiers)", DiffAct::Open),
                        (
                            "↺ Annuler dans mes fichiers",
                            "Défaire ce que ce commit a changé dans ce fichier, dans tes fichiers actuels (à valider ensuite)",
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
                            ui.label(RichText::new(format!("{} FICHIER(S)", chips.len())).small().strong().color(accent));
                            ui.label(RichText::new(format!("+{adds}")).small().monospace().color(ok()));
                            ui.label(RichText::new(format!("−{dels}")).small().monospace().color(err()));
                            if h.diff_loading {
                                ui.spinner();
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
                                if ui.small_button("Tout déplier").clicked() {
                                    h.collapsed.clear();
                                }
                                if ui.small_button("Tout replier").clicked() {
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
                            let tip = "Écrit les remarques gardées dans l'onglet Claude : tu relis, puis tu envoies";
                            ai_fix = ui.add_enabled(kept > 0, filled_button(format!("Faire corriger les {kept} gardée(s) par Claude"), accent)).on_hover_text(tip).clicked();
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
                            let hunk_buttons: &[history::DiffButton] = match source {
                                DiffSource::Worktree => &[
                                    ("+ Indexer ce bloc", "Ajouter ce bloc au prochain commit", DiffAct::Stage),
                                    ("↺ Annuler ce bloc", "Supprimer ce changement du fichier (demande confirmation)", DiffAct::Discard),
                                ],
                                DiffSource::Index => {
                                    &[("− Désindexer ce bloc", "Retirer ce bloc du prochain commit (le changement reste dans le fichier)", DiffAct::Unstage)]
                                }
                                DiffSource::Commit => &[("↺ Annuler ce changement", "Défaire ce bloc dans tes fichiers actuels (à valider ensuite)", DiffAct::Revert)],
                                DiffSource::Compare => &[],
                            };
                            let jump = h.jump.take();
                            let notes = if ai_lines.is_empty() { None } else { Some(history::Notes { at: &ai_lines, show: &mut show_ai }) };
                            if let Some(c) = history::stacked_diff_view(ui, &d.lines, &d.files, &mut h.collapsed, "history_stacked", hunk_buttons, file_buttons, jump, notes) {
                                diff_click = Some(c);
                            }
                        }
                        None if h.diff_loading => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("Chargement du diff…").weak());
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
        if do_commit {
            self.commit(n_staged);
        }
        if suggest_msg {
            self.suggest_commit_message();
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
                    self.history.open = false;
                    self.new_scratch(title, text, "Diff", true);
                }
                None => self.set_status("Le diff est en cours de chargement"),
            }
        }
        if resume {
            match self.split.as_mut() {
                Some(s) => s.shown = true,
                None => self.resume_split(),
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
                    self.set_status(format!("Copié : {text}"));
                }
                (HistAct::Diff(hash), _) => {
                    if let Some(i) = self.history.graph.as_ref().and_then(|g| g.commits.iter().position(|c| c.hash == hash)) {
                        select = Some(i);
                    }
                }
                (HistAct::Split(hash), _) => self.start_split(hash),
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
                self.set_status("Rebase en cours…");
                self.jobs.spawn(move || JobResult::GitOp {
                    label: if crate::rebase::splits(&rb.steps) { "rebase (découpage)".into() } else { "rebase".into() },
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
                d.text = "(aucune différence)".into();
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

/// Message and button of the next commit, and ✨ to have it written by Claude; returns
/// (commit now, ask Claude).
fn commit_box(ui: &mut egui::Ui, msg: &mut String, focus: bool, n_staged: usize, n_changed: usize, suggesting: bool, accent: Color32) -> (bool, bool) {
    let (label, tip) = if n_staged > 0 {
        (format!("✔ Commit ({n_staged} indexé(s))"), "Valide les fichiers indexés")
    } else {
        (format!("✔ Commit tout ({n_changed})"), "Rien d'indexé : valide toutes les modifications des fichiers suivis")
    };
    let can = !msg.trim().is_empty() && (n_staged > 0 || n_changed > 0);
    let (mut go, mut suggest) = (false, false);
    egui::Frame::none().inner_margin(egui::Margin { left: 4.0, right: 4.0, top: 4.0, bottom: 6.0 }).show(ui, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            let clicked = ui.add_enabled(can, filled_button(label, accent)).on_hover_text(tip).clicked();
            if suggesting {
                ui.spinner();
            } else if ui.button("✨").on_hover_text("Claude propose le message d'après les changements (indexés, sinon tous) et ta demande dans l'onglet Claude").clicked() {
                suggest = true;
            }
            let resp = ui.add(egui::TextEdit::multiline(msg).hint_text("Message du commit (Ctrl+Entrée)").desired_rows(1).desired_width(f32::INFINITY));
            if focus {
                resp.request_focus();
            }
            let ctrl_enter = resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
            go = can && (clicked || ctrl_enter);
        });
    });
    (go, suggest)
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
