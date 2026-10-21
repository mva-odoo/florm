//! Editor panels: split view, view tabs, document bodies.

use super::*;

impl App {
    pub(super) fn is_split(&self) -> bool {
        self.docs.iter().any(|d| d.pane == 1)
    }

    /// Places new documents in the active panel, closes a panel left empty, and makes the
    /// active panel follow the active document (however it was activated).
    pub(super) fn normalize_panes(&mut self) {
        let focused = self.focused_pane;
        for d in self.docs.iter_mut().filter(|d| d.pane == document::NO_PANE) {
            d.pane = focused;
        }
        if !self.docs.iter().any(|d| d.pane == 0) {
            self.docs.iter_mut().for_each(|d| d.pane = 0);
        }
        self.focused_pane = self.docs.get(self.active).map_or(0, |d| d.pane);
        if let Some(d) = self.docs.get(self.active) {
            self.pane_active[self.focused_pane as usize] = Some(d.uid);
        }
        let other = 1 - self.focused_pane as usize;
        let ok = self.pane_active[other].is_some_and(|u| self.docs.iter().any(|d| d.uid == u && d.pane as usize == other));
        if !ok {
            self.pane_active[other] = self.docs.iter().rev().find(|d| d.pane as usize == other).map(|d| d.uid);
        }
    }

    /// Document shown in `pane`.
    pub(super) fn pane_doc(&self, pane: u8) -> Option<usize> {
        if pane == self.focused_pane {
            return (self.active < self.docs.len()).then_some(self.active);
        }
        let uid = self.pane_active[pane as usize]?;
        self.docs.iter().position(|d| d.uid == uid)
    }

    /// Sends tab `i` to the other panel (creating the split in `dir` if needed) and focuses it.
    pub(super) fn split_tab(&mut self, i: usize, dir: Option<SplitDir>) {
        let Some(d) = self.docs.get(i) else { return };
        let pane = d.pane;
        let alone = self.docs.iter().filter(|x| x.pane == pane).count() == 1;
        if alone && !self.is_split() {
            self.set_status("Ouvre un deuxième fichier pour diviser l'éditeur (un fichier ne peut être que dans un panneau).");
            return;
        }
        if let Some(dir) = dir {
            self.split_dir = dir;
        }
        self.docs[i].pane = 1 - pane;
        self.activate_tab(i);
    }

    pub(super) fn unsplit(&mut self) {
        self.docs.iter_mut().for_each(|d| d.pane = 0);
        self.focused_pane = 0;
    }

    pub(super) fn activate_tab(&mut self, i: usize) {
        self.active = i;
        self.history.open = false;
        self.prefs.open = false;
        if let Some(sp) = self.split.as_mut() {
            sp.shown = false;
        }
        self.pending_focus = true;
    }

    /// One panel of a split editor: its tabs and its editor.
    pub(super) fn pane_ui(&mut self, ui: &mut egui::Ui, pane: u8) -> Option<egui::Response> {
        self.tabs_strip(ui, Some(pane));
        let rect = ui.available_rect_before_wrap();
        self.drop_zones.panes.push((pane, rect));
        let i = self.pane_doc(pane)?;
        self.breadcrumbs_ui(ui, i, pane);
        self.conflict_bar(ui, i, pane);
        let focused = pane == self.focused_pane;
        let id = if focused { editor_id() } else { editor_view::other_editor_id() };
        let r = self.doc_body(ui, i, id, focused)?;
        focused.then_some(r)
    }

    /// Body of tab `i`: the editor, an image, or a Markdown preview. Returns the editor's response.
    pub(super) fn doc_body(&mut self, ui: &mut egui::Ui, i: usize, id: egui::Id, focused: bool) -> Option<egui::Response> {
        match self.docs.get(i)?.view {
            document::View::Text => {
                let matches: &[(usize, usize)] = if focused && self.find.open { &self.find.matches } else { &[] };
                let doc = self.docs.get_mut(i)?;
                let s = &self.settings;
                let opts = editor_view::ViewOpts {
                    font_size: s.font_size,
                    minimap: s.show_minimap,
                    inline_diff: s.inline_diff,
                    sticky: s.sticky_scroll,
                    rulers: &s.rulers,
                };
                Some(editor_view::show(ui, doc, &self.hl, &opts, matches, id))
            }
            document::View::Image => {
                self.image_view(ui, i);
                None
            }
            document::View::Preview(src) => {
                self.markdown_view(ui, src);
                None
            }
            document::View::Welcome => {
                egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.welcome_ui(ui));
                None
            }
            document::View::Help => {
                self.render_markdown(ui, "guide", include_str!("../../assets/guide.md"), "file://".into());
                None
            }
            document::View::Search => {
                let bg = ui.visuals().panel_fill;
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin { left: 12.0, right: 12.0, top: 10.0, bottom: 4.0 }))
                    .show_inside(ui, |ui| self.search_ui(ui, true));
                None
            }
            document::View::Terminal(id) => {
                if self.term.cwd.is_none() {
                    self.term.cwd = self.root.clone();
                }
                let shell = self.term_shell();
                self.term.tab_ui(ui, id, shell.as_deref(), self.hl.colors, self.hl.is_light());
                None
            }
        }
    }

    /// Adds a read-only tab showing `view` (search, terminal) in the focused panel and shows it.
    pub(super) fn open_view_tab(&mut self, view: document::View, title: String) {
        if let Some(i) = self.docs.iter().position(|d| d.view == view) {
            self.activate_tab(i);
            return;
        }
        let uid = self.next_uid();
        let mut d = Document::new(uid, self.settings.tab_size, self.settings.translate_tabs_to_spaces);
        d.view = view;
        d.read_only = true;
        d.title_override = Some(title);
        self.docs.push(d);
        self.activate_tab(self.docs.len() - 1);
    }

    /// The "find in files" panel in an editor tab (instead of the bottom panel).
    pub(super) fn open_search_tab(&mut self) {
        if self.bottom == Some(BottomTab::Search) {
            self.bottom = None;
        }
        self.open_view_tab(document::View::Search, "🔍 Recherche".into());
        self.search.focus = true;
    }

    /// A new terminal in an editor tab (it can be split beside a file like any tab).
    pub(super) fn open_terminal_tab(&mut self) {
        if self.term.cwd.is_none() {
            self.term.cwd = self.root.clone();
        }
        let shell = self.term_shell();
        let ctx = self.egui_ctx.clone();
        match self.term.open_tab(&ctx, shell.as_deref(), None) {
            Ok((id, title)) => self.open_view_tab(document::View::Terminal(id), format!(">_ {title}")),
            Err(e) => self.set_status(e),
        }
    }

    /// Terminals moved into the editor ("⇱ Onglet"), or started from a terminal tab, get their tab;
    /// terminal tabs follow the title of their shell.
    pub(super) fn sync_terminal_tabs(&mut self) {
        if let Some(id) = self.term.dock_request.take() {
            self.term.set_docked(id, true);
            if self.bottom == Some(BottomTab::Terminal) && !self.term.has_panel_sessions() {
                // the panel would open a new terminal at once: close it instead
                self.bottom = None;
            }
        }
        // "⇲ Panneau du bas": its tab goes, the panel shows it
        let docked = self.term.docked_ids();
        let back = self.docs.iter().position(|d| matches!(d.view, document::View::Terminal(id) if !docked.contains(&id) && self.term.title_of(id).is_some()));
        if let Some(i) = back {
            self.close_tab(i, true);
            self.bottom = Some(BottomTab::Terminal);
        }
        for id in docked {
            if !self.docs.iter().any(|d| d.view == document::View::Terminal(id)) {
                let title = self.term.title_of(id).unwrap_or_default();
                self.open_view_tab(document::View::Terminal(id), format!(">_ {title}"));
            }
        }
        for d in self.docs.iter_mut() {
            if let document::View::Terminal(id) = d.view {
                if let Some(t) = self.term.title_of(id) {
                    let t = format!(">_ {t}");
                    if d.title_override.as_ref() != Some(&t) {
                        d.title_override = Some(t);
                    }
                }
            }
        }
    }

    fn image_view(&mut self, ui: &mut egui::Ui, i: usize) {
        let Some(path) = self.docs[i].path.clone() else { return };
        let uri = format!("file://{}", path.display());
        let bg = self.hl.colors.bg;
        let mut zoom = self.docs[i].zoom;
        egui::TopBottomPanel::top(egui::Id::new("image_bar").with(i)).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let size = egui::Image::new(uri.as_str()).load_for_size(ui.ctx(), egui::Vec2::splat(4096.0)).ok().and_then(|t| t.size());
                if let Some(s) = size {
                    ui.label(RichText::new(format!("{} × {} px", s.x as u32, s.y as u32)).strong());
                }
                if let Ok(m) = std::fs::metadata(&path) {
                    let kb = m.len() as f64 / 1024.0;
                    ui.label(RichText::new(if kb < 1024.0 { format!("{kb:.1} Ko") } else { format!("{:.1} Mo", kb / 1024.0) }).weak());
                }
                ui.separator();
                if ui.selectable_label(zoom == 0.0, "Ajuster").clicked() {
                    zoom = 0.0;
                }
                for (label, z) in [("50 %", 0.5), ("100 %", 1.0), ("200 %", 2.0)] {
                    if ui.selectable_label(zoom == z, label).clicked() {
                        zoom = z;
                    }
                }
                if ui.button("Afficher dans le gestionnaire de fichiers").clicked() {
                    let _ = crate::os::reveal(&path);
                }
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::none().fill(bg)).show_inside(ui, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                let avail = ui.available_size();
                let img = egui::Image::new(uri.as_str()).show_loading_spinner(true);
                let img = match zoom {
                    0.0 => img.max_size(avail - egui::vec2(24.0, 24.0)).fit_to_original_size(1.0).maintain_aspect_ratio(true),
                    z => img.fit_to_original_size(z),
                };
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    ui.add(img);
                });
            });
        });
        self.docs[i].zoom = zoom;
    }

    fn markdown_view(&mut self, ui: &mut egui::Ui, src: u64) {
        let Some(d) = self.docs.iter().find(|d| d.uid == src) else {
            ui.label(RichText::new("Le fichier Markdown de cet aperçu a été fermé.").weak());
            return;
        };
        let text = d.rope.to_string();
        let base = d.path.as_deref().and_then(Path::parent).map(|p| format!("file://{}/", p.display())).unwrap_or_else(|| "file://".into());
        self.render_markdown(ui, ("md_preview", src), &text, base);
    }

    fn render_markdown(&mut self, ui: &mut egui::Ui, id: impl std::hash::Hash, text: &str, base: String) {
        let bg = self.hl.colors.bg;
        let cache = &mut self.md_cache;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(bg)).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
                let w = ui.available_width().min(880.0);
                let pad = ((ui.available_width() - w) / 2.0).max(16.0);
                egui::Frame::none().inner_margin(egui::Margin { left: pad, right: pad, top: 16.0, bottom: 40.0 }).show(ui, |ui| {
                    ui.set_max_width(w);
                    egui_commonmark::CommonMarkViewer::new()
                        .default_implicit_uri_scheme(base)
                        .max_image_width(Some(w as usize))
                        .show(ui, cache, text);
                });
            });
        });
    }

    /// Opens (or shows) the rendered preview of the active Markdown file, in its panel or beside it.
    pub(super) fn markdown_preview(&mut self, side: bool) {
        let Some(d) = self.docs.get(self.active) else { return };
        let is_md = d.syntax.contains("Markdown") || d.path.as_deref().is_some_and(|p| p.extension().is_some_and(|e| e == "md" || e == "markdown"));
        if !is_md {
            self.set_status("L'aperçu est pour les fichiers Markdown (.md)");
            return;
        }
        let (src, title, src_pane) = (d.uid, d.title(), d.pane);
        let src_idx = self.active;
        if let Some(i) = self.docs.iter().position(|x| x.view == document::View::Preview(src)) {
            if side {
                self.docs[i].pane = 1 - src_pane;
                self.active = src_idx;
            } else {
                self.activate_tab(i);
            }
            return;
        }
        let uid = self.next_uid();
        let mut p = Document::new(uid, self.settings.tab_size, self.settings.translate_tabs_to_spaces);
        p.view = document::View::Preview(src);
        p.read_only = true;
        p.title_override = Some(format!("Aperçu : {title}"));
        p.pane = if side { 1 - src_pane } else { src_pane };
        self.docs.push(p);
        if side {
            // keep typing in the source, the preview follows beside it
            self.split_dir = SplitDir::Right;
            self.active = src_idx;
        } else {
            self.active = self.docs.len() - 1;
        }
    }

    /// Bar above a file with merge conflicts: navigation and "keep mine / theirs / both".
    pub(super) fn conflict_bar(&mut self, ui: &mut egui::Ui, i: usize, pane: u8) {
        let path = self.docs.get(i).and_then(|d| d.path.clone());
        let in_repo = path.as_deref().and_then(|p| self.repo_for(p)).is_some();
        let Some(d) = self.docs.get_mut(i) else { return };
        let conflicts = d.conflicts();
        if conflicts.is_empty() && !d.had_conflicts {
            return;
        }
        let line = d.rope.char_to_line(d.primary_sel().head);
        let k = conflicts.iter().position(|c| line <= c.end).unwrap_or(0);
        let (mut act, mut goto, mut resolved) = (None, None, false);
        let bg = icons::mix(ui.visuals().panel_fill, if conflicts.is_empty() { ok() } else { warn() }, 0.15);
        egui::TopBottomPanel::top(egui::Id::new("conflicts").with(pane))
            .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(10.0, 4.0)))
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if conflicts.is_empty() {
                        ui.label(RichText::new("✔ Tous les conflits de ce fichier sont résolus").color(ok()).strong());
                        if in_repo && ui.button("Enregistrer et marquer comme résolu (git add)").clicked() {
                            resolved = true;
                        }
                        return;
                    }
                    let c = &conflicts[k];
                    ui.label(RichText::new(format!("⚠ Conflit de merge {}/{}", k + 1, conflicts.len())).color(warn()).strong());
                    if ui.small_button("◀").on_hover_text("Conflit précédent").clicked() {
                        goto = Some(conflicts[(k + conflicts.len() - 1) % conflicts.len()].start);
                    }
                    if ui.small_button("▶").on_hover_text("Conflit suivant").clicked() {
                        goto = Some(conflicts[(k + 1) % conflicts.len()].start);
                    }
                    ui.add_space(8.0);
                    let mine = if c.ours.is_empty() { "Garder le mien".to_string() } else { format!("Garder le mien ({})", c.ours) };
                    let theirs = if c.theirs.is_empty() { "Garder le leur".to_string() } else { format!("Garder le leur ({})", c.theirs) };
                    if ui.button(RichText::new(mine).color(highlight::green())).on_hover_text("Ta version (en vert)").clicked() {
                        act = Some((c.clone(), document::Resolution::Ours));
                    }
                    if ui.button(RichText::new(theirs).color(highlight::blue())).on_hover_text("La version qui arrive (en bleu)").clicked() {
                        act = Some((c.clone(), document::Resolution::Theirs));
                    }
                    if ui.button("Garder les deux").on_hover_text("La tienne puis l'autre").clicked() {
                        act = Some((c.clone(), document::Resolution::Both));
                    }
                    ui.label(RichText::new("(Ctrl+Z pour revenir)").weak().small());
                });
            });
        let d = &mut self.docs[i];
        if let Some((c, how)) = act {
            d.resolve_conflict(&c, how);
            self.active = i;
            self.pending_focus = true;
        }
        if let Some(l) = goto {
            d.goto_line(l, 0);
            self.active = i;
        }
        if resolved {
            d.had_conflicts = false;
            if self.save(i) {
                if let Some(p) = self.docs[i].path.clone() {
                    if let Some(repo) = self.repo_for(&p) {
                        let rel = p.strip_prefix(&repo).unwrap_or(&p).to_string_lossy().into_owned();
                        self.git_job_at(repo, "add", &["add", "--", &rel]);
                    }
                }
            }
        }
    }

    /// "dossier › fichier › Classe › méthode" above the editor; a click goes to the symbol.
    pub(super) fn breadcrumbs_ui(&mut self, ui: &mut egui::Ui, i: usize, pane: u8) {
        if !self.settings.breadcrumbs {
            return;
        }
        let root = self.root.clone();
        let accent = self.hl.colors.accent;
        let Some(d) = self.docs.get_mut(i) else { return };
        let line = d.rope.char_to_line(d.primary_sel().head);
        // folders of the path (relative to the project), with their absolute path for the menus
        let mut dirs: Vec<(String, Option<PathBuf>)> = Vec::new();
        let file = match &d.path {
            Some(p) => {
                let base = root.as_ref().filter(|r| p.starts_with(r)).cloned().unwrap_or_default();
                let rel = p.strip_prefix(&base).unwrap_or(p);
                let mut acc = base.clone();
                let mut comps: Vec<(String, Option<PathBuf>)> = rel
                    .components()
                    .map(|c| {
                        acc.push(c.as_os_str());
                        (c.as_os_str().to_string_lossy().into_owned(), Some(acc.clone()))
                    })
                    .collect();
                let f = comps.pop().map(|c| c.0).unwrap_or_default();
                if comps.len() > 3 {
                    comps.drain(..comps.len() - 3);
                    comps.insert(0, ("…".into(), None));
                }
                dirs = comps;
                f
            }
            None => d.title(),
        };
        let chain: Vec<(String, symbols::Kind, usize)> =
            symbols::path_at(d.symbols(), line).iter().map(|s| (s.name.clone(), s.kind, s.line)).collect();
        let is_md = d.view == document::View::Text && d.syntax.contains("Markdown");
        let (mut goto, mut list, mut preview, mut open, mut reveal) = (None, false, false, None, None);
        let bg = self.hl.colors.bg;
        egui::TopBottomPanel::top(egui::Id::new("crumbs").with(pane))
            .exact_height(20.0)
            .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(10.0, 0.0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    // pale and small: the pinned lines of the editor already show where you are
                    let weak = ui.visuals().weak_text_color().gamma_multiply(0.75);
                    let sep = |ui: &mut egui::Ui| {
                        ui.label(RichText::new("›").size(11.5).color(weak));
                    };
                    for (dname, dir) in &dirs {
                        match dir {
                            // a folder: its files and folders in a menu
                            Some(dir) => {
                                let r = ui.menu_button(RichText::new(dname).size(11.5).color(weak), |ui| {
                                    ui.set_max_height(420.0);
                                    if ui.button("Afficher dans l'arborescence").clicked() {
                                        reveal = Some(dir.clone());
                                        ui.close_menu();
                                    }
                                    ui.separator();
                                    let mut entries: Vec<(String, PathBuf, bool)> = std::fs::read_dir(dir)
                                        .map(|rd| {
                                            rd.flatten()
                                                .filter(|e| e.file_name() != ".git")
                                                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path(), e.path().is_dir()))
                                                .collect()
                                        })
                                        .unwrap_or_default();
                                    entries.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
                                    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                                        for (name, path, is_dir) in entries {
                                            let label = if is_dir { format!("📁 {name}") } else { name };
                                            if ui.button(label).clicked() {
                                                if is_dir {
                                                    reveal = Some(path);
                                                } else {
                                                    open = Some(path);
                                                }
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                });
                                r.response.on_hover_text(dir.display().to_string());
                            }
                            None => {
                                ui.label(RichText::new(dname).size(11.5).color(weak));
                            }
                        }
                        sep(ui);
                    }
                    let r = ui.add(egui::Label::new(RichText::new(&file).size(11.5).color(weak)).sense(Sense::click()));
                    if r.on_hover_text("Symboles du fichier (Ctrl+R)").clicked() {
                        list = true;
                    }
                    for (name, kind, l) in &chain {
                        sep(ui);
                        let text = RichText::new(format!("{} {name}", kind.icon())).size(11.5).color(accent.gamma_multiply(0.8));
                        if ui.add(egui::Label::new(text).sense(Sense::click())).on_hover_text(format!("Aller à la ligne {}", l + 1)).clicked() {
                            goto = Some(*l);
                        }
                    }
                    if is_md {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Aperçu").on_hover_text("Aperçu Markdown à côté (Ctrl+K V)").clicked() {
                                preview = true;
                            }
                        });
                    }
                });
            });
        if let Some(l) = goto {
            if let Some(d) = self.docs.get_mut(i) {
                let ind = d.indent_len(l);
                d.goto_line(l, ind);
            }
            self.active = i;
            self.pending_focus = true;
        }
        if list {
            self.active = i;
            self.pending.push(Cmd::GotoSymbol);
        }
        if preview {
            self.active = i;
            self.markdown_preview(true);
        }
        if let Some(p) = open {
            self.open_path(p);
        }
        if let Some(p) = reveal {
            self.reveal_in_tree(&p);
            self.tree_nav.focus = true;
        }
    }

    /// Ends a tab drag: reorder in a tab strip, move to the other panel, or split.
    pub(super) fn resolve_tab_drag(&mut self, ctx: &egui::Context) {
        let Some(from) = self.tab_drag else { return };
        let Some(p) = ctx.pointer_latest_pos() else { return };
        if from >= self.docs.len() {
            self.tab_drag = None;
            return;
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        let accent = self.hl.colors.accent;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tab_drop")));
        let src = self.docs[from].pane;
        let split = self.is_split();
        let alone = self.docs.iter().filter(|d| d.pane == src).count() == 1;
        let zones = std::mem::take(&mut self.drop_zones);
        let mut target = None;
        let hint = |r: egui::Rect, text: &str| {
            painter.rect_filled(r.shrink(4.0), 8.0, accent.gamma_multiply(0.18));
            painter.rect_stroke(r.shrink(4.0), 8.0, egui::Stroke::new(2.0_f32, accent));
            painter.text(r.center(), Align2::CENTER_CENTER, text, egui::FontId::proportional(16.0), accent);
        };
        if let Some((pane, _, tabs)) = zones.strips.iter().find(|(_, r, _)| r.contains(p)) {
            let k = tabs.iter().filter(|(r, _)| r.center().x < p.x).count();
            let before = tabs.get(k).map(|(_, g)| *g);
            let x = match tabs.get(k) {
                Some((r, _)) => r.left() - 1.0,
                None => tabs.last().map_or(p.x, |(r, _)| r.right() + 1.0),
            };
            let top = tabs.first().map_or(p.y - 10.0, |(r, _)| r.top() + 6.0);
            painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x - 1.5, top), egui::pos2(x + 1.5, top + TAB_STRIP_H - 8.0)), 1.5, accent);
            // insertion index in the whole list: before a tab, or after the last one of the panel
            let to = before.unwrap_or_else(|| tabs.last().map_or(self.docs.len(), |(_, g)| g + 1));
            target = Some(TabDrop::Strip(*pane, to));
        } else if let Some((pane, r)) = zones.panes.iter().find(|(_, r)| r.contains(p)) {
            if *pane != src {
                hint(*r, "Déplacer dans ce panneau");
                target = Some(TabDrop::Pane(*pane));
            } else if !split && !alone {
                if p.x > r.left() + r.width() * 0.6 {
                    hint(egui::Rect::from_min_max(egui::pos2(r.center().x, r.top()), r.max), "Diviser à droite");
                    target = Some(TabDrop::Split(SplitDir::Right));
                } else if p.y > r.top() + r.height() * 0.6 {
                    hint(egui::Rect::from_min_max(egui::pos2(r.left(), r.center().y), r.max), "Diviser en bas");
                    target = Some(TabDrop::Split(SplitDir::Down));
                }
            }
        }
        if !ctx.input(|i| i.pointer.primary_down()) {
            self.tab_drag = None;
            match target {
                Some(TabDrop::Strip(pane, to)) => {
                    self.docs[from].pane = pane;
                    self.move_tab(from, to);
                }
                Some(TabDrop::Pane(pane)) => {
                    self.docs[from].pane = pane;
                    self.activate_tab(from);
                }
                Some(TabDrop::Split(dir)) => self.split_tab(from, Some(dir)),
                None => {}
            }
        }
    }

    /// Moves tab `from` before tab `to` (`to == len`: at the end); the moved tab becomes active.
    pub(super) fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.docs.len() {
            return;
        }
        let d = self.docs.remove(from);
        let to = if to > from { to - 1 } else { to }.min(self.docs.len());
        self.docs.insert(to, d);
        self.active = to;
        self.tab_scrolled = None;
        self.history.open = false;
        self.prefs.open = false;
        if let Some(sp) = self.split.as_mut() {
            sp.shown = false;
        }
    }

    pub(super) fn close_tab(&mut self, idx: usize, force: bool) {
        let Some(doc) = self.docs.get(idx) else { return };
        if doc.is_dirty() && !force {
            self.confirm = Some(Confirm::CloseTab(doc.uid));
            return;
        }
        let was_active = idx == self.active;
        let d = self.docs.remove(idx);
        // a terminal sent back to the bottom panel keeps running
        if let document::View::Terminal(id) = d.view.clone() {
            if self.term.docked_ids().contains(&id) {
                self.term.close(id);
            }
        }
        if let Some(p) = d.path {
            self.closed.push(p);
        }
        if was_active {
            // next tab of the same panel (the one on the right, else on the left)
            let same = self.docs.iter().enumerate().filter(|(_, x)| x.pane == d.pane).map(|(j, _)| j);
            self.active = same.min_by_key(|&j| (j < idx, j.abs_diff(idx))).unwrap_or(idx.min(self.docs.len().saturating_sub(1)));
        } else if idx < self.active {
            self.active -= 1;
        }
    }

    /// Shell chosen in the preferences (None: $SHELL, or cmd on Windows).
    pub(super) fn term_shell(&self) -> Option<String> {
        self.settings.shell.clone().filter(|s| !s.trim().is_empty())
    }
}
