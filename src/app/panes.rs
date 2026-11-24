//! Editor panels: split view, view tabs, document bodies.

use super::*;

impl App {
    pub(super) fn is_split(&self) -> bool {
        self.docs.iter().any(|d| d.pane == 1)
    }

    /// Places new documents in the active panel, closes a panel left empty, and makes the
    /// active panel follow the active document (however it was activated).
    pub(super) fn normalize_panes(&mut self) {
        let focused = self.panes.focused;
        for d in self.docs.iter_mut().filter(|d| d.pane == document::NO_PANE) {
            d.pane = focused;
        }
        if !self.docs.iter().any(|d| d.pane == 0) {
            self.docs.iter_mut().for_each(|d| d.pane = 0);
        }
        self.panes.focused = self.docs.get(self.active).map_or(0, |d| d.pane);
        if let Some(d) = self.docs.get(self.active) {
            self.panes.active[self.panes.focused as usize] = Some(d.uid);
        }
        let other = 1 - self.panes.focused as usize;
        let ok = self.panes.active[other].is_some_and(|u| self.docs.iter().any(|d| d.uid == u && d.pane as usize == other));
        if !ok {
            self.panes.active[other] = self.docs.iter().rev().find(|d| d.pane as usize == other).map(|d| d.uid);
        }
    }

    /// Document shown in `pane`.
    pub(super) fn pane_doc(&self, pane: u8) -> Option<usize> {
        if pane == self.panes.focused {
            return (self.active < self.docs.len()).then_some(self.active);
        }
        let uid = self.panes.active[pane as usize]?;
        self.docs.iter().position(|d| d.uid == uid)
    }

    /// Sends tab `i` to the other panel (creating the split in `dir` if needed) and focuses it.
    pub(super) fn split_tab(&mut self, i: usize, dir: Option<SplitDir>) {
        let Some(d) = self.docs.get(i) else { return };
        let pane = d.pane;
        let alone = self.docs.iter().filter(|x| x.pane == pane).count() == 1;
        if alone && !self.is_split() {
            self.set_status(t!("Ouvre un deuxième fichier pour diviser l'éditeur (un fichier ne peut être que dans un panneau).", "Open a second file to split the editor (a file can only be in one pane)."));
            return;
        }
        if let Some(dir) = dir {
            self.panes.split_dir = dir;
        }
        self.docs[i].pane = 1 - pane;
        self.activate_tab(i);
    }

    pub(super) fn unsplit(&mut self) {
        self.docs.iter_mut().for_each(|d| d.pane = 0);
        self.panes.focused = 0;
    }

    pub(super) fn activate_tab(&mut self, i: usize) {
        let (old, new) = (self.docs.get(self.active).map(|d| d.uid), self.docs.get(i).map(|d| d.uid));
        if old != new && old.is_some() {
            self.panes.prev_tab = old;
        }
        self.active = i;
        self.pending_focus = true;
    }

    /// One panel of a split editor: its tabs and its editor.
    pub(super) fn pane_ui(&mut self, ui: &mut egui::Ui, pane: u8) -> Option<egui::Response> {
        let gap = self.tabs_strip(ui, Some(pane));
        let rect = ui.available_rect_before_wrap();
        self.panes.drop_zones.panes.push((pane, rect));
        let i = self.pane_doc(pane)?;
        self.breadcrumbs_ui(ui, i, pane);
        self.conflict_bar(ui, i, pane);
        self.disk_bar(ui, i, pane);
        self.trust_bar(ui, i, pane);
        self.md_toolbar_ui(ui, i, pane);
        let focused = pane == self.panes.focused;
        let id = if focused { editor_id() } else { editor_view::other_editor_id() };
        let r = self.doc_body(ui, i, id, focused);
        self.editor_card(ui, rect, gap);
        focused.then_some(r?)
    }

    /// Body of tab `i`: the editor, an image, or a Markdown preview. Returns the editor's response.
    pub(super) fn doc_body(&mut self, ui: &mut egui::Ui, i: usize, id: egui::Id, focused: bool) -> Option<egui::Response> {
        match self.docs.get(i)?.view {
            document::View::Text => {
                let tests = self.test_marks(i);
                let exec_line = self.trace_exec_line(i);
                let trace = self.docs[i].path.as_deref().and_then(crate::trace::lang_of);
                let matches: &[(usize, usize)] = if focused && self.find.open { &self.find.matches } else { &[] };
                let doc = self.docs.get_mut(i)?;
                let s = &self.settings;
                let opts = editor_view::ViewOpts {
                    font_size: s.font_size,
                    minimap: s.show_minimap,
                    wrap: s.word_wrap,
                    inline_diff: s.inline_diff,
                    sticky: s.sticky_scroll,
                    rulers: &s.rulers,
                    notes: doc.path.as_ref().and_then(|p| self.pr.notes.get(p)).map_or(&[], |v| &v[..]),
                    tests: &tests,
                    trace,
                    exec_line,
                    rainbow: s.rainbow_brackets,
                    whitespace: s.show_whitespace,
                };
                let r = editor_view::show(ui, doc, &self.hl, &opts, matches, id);
                let bp = doc.bp_pick.take();
                if let Some(t) = doc.test_pick.take() {
                    self.run_tests(t);
                }
                if let Some(line) = bp {
                    self.trace_toggle_at(i, line);
                }
                self.conflict_picked(i);
                Some(r)
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
                egui::CentralPanel::default().frame(egui::Frame::none().fill(self.hl.colors.bg)).show_inside(ui, |ui| self.welcome_ui(ui));
                None
            }
            document::View::Claude => {
                let uid = self.docs[i].uid;
                // one tab for every conversation: their list on the left, always there, even with
                // a single conversation: its « + » starts the next one
                let mut act = None;
                let fill = ui.visuals().panel_fill;
                egui::SidePanel::left(egui::Id::new("claude_list").with(uid))
                    .resizable(true)
                    .default_width(250.0)
                    .width_range(180.0..=420.0)
                    .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 8.0, right: 6.0, top: 0.0, bottom: 6.0 }))
                    .show_inside(ui, |ui| act = self.claude_list_ui(ui, uid));
                egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.claude_show(ui, uid));
                if let Some(a) = act {
                    self.claude_list_act(a, uid);
                }
                None
            }
            document::View::PullRequests => {
                egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.pulls_ui(ui));
                None
            }
            document::View::Git => {
                let fill = ui.visuals().panel_fill;
                egui::TopBottomPanel::top(egui::Id::new("git_bar").with(self.docs[i].uid))
                    .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 8.0, bottom: 4.0 }))
                    .show_inside(ui, |ui| self.git_bar(ui));
                egui::CentralPanel::default().show_inside(ui, |ui| self.history_ui(ui));
                None
            }
            document::View::Settings => {
                egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.prefs_ui(ui));
                None
            }
            document::View::History => {
                egui::CentralPanel::default().show_inside(ui, |ui| self.line_history_ui(ui));
                None
            }
            document::View::Help => {
                let (key, text) = if crate::i18n::en() {
                    ("guide_en", include_str!("../../assets/guide.en.md"))
                } else {
                    ("guide", include_str!("../../assets/guide.md"))
                };
                self.guide_view(ui, key, text);
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
        self.open_view_tab(document::View::Search, t!("Recherche", "Search").into());
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
            Ok((id, title)) => self.open_view_tab(document::View::Terminal(id), title),
            Err(e) => self.set_error(tf!("Terminal impossible à ouvrir : {e}", "Cannot open a terminal: {e}")),
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
                self.open_view_tab(document::View::Terminal(id), title);
            }
        }
        for d in self.docs.iter_mut() {
            if let document::View::Terminal(id) = d.view {
                if let Some(t) = self.term.title_of(id) {
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
                    ui.label(RichText::new(if kb < 1024.0 { tf!("{kb:.1} Ko", "{kb:.1} KB") } else { tf!("{:.1} Mo", "{:.1} MB", kb / 1024.0) }).weak());
                }
                ui.separator();
                if ui.selectable_label(zoom == 0.0, t!("Ajuster", "Fit")).clicked() {
                    zoom = 0.0;
                }
                for (label, z) in [("50 %", 0.5), ("100 %", 1.0), ("200 %", 2.0)] {
                    if ui.selectable_label(zoom == z, label).clicked() {
                        zoom = z;
                    }
                }
                if ui.button(t!("Afficher dans le gestionnaire de fichiers", "Show in the file manager")).clicked() {
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
            ui.label(RichText::new(t!("Le fichier Markdown de cet aperçu a été fermé.", "The Markdown file of this preview was closed.")).weak());
            return;
        };
        // the text cut into pieces once per version, not at each frame
        if self.md_preview.as_ref().is_none_or(|p| p.key != (src, d.version)) {
            // its images: files of the project (of its folder when it is outside), or web addresses
            let dir = d.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
            let root = match (&self.root, &dir) {
                (Some(r), Some(dir)) if dir.starts_with(r) => Some(r.clone()),
                _ => dir.clone(),
            };
            let old = self.md_preview.take().filter(|p| p.key.0 == src);
            let chunks = md_chunks(&d.rope.to_string());
            // the heights measured before kept while the pieces are the same: no jump while typing
            let same = |k: usize, c: &MdChunk| old.as_ref().is_some_and(|o| o.chunks.get(k) == Some(c));
            let heights = chunks.iter().enumerate().map(|(k, c)| if same(k, c) { old.as_ref().map_or(0.0, |o| o.heights[k]) } else { 0.0 }).collect();
            let shown = chunks
                .iter()
                .enumerate()
                .map(|(k, c)| match c {
                    _ if same(k, c) => old.as_ref().map_or(String::new(), |o| o.shown[k].clone()),
                    MdChunk::Mermaid(_) => String::new(),
                    MdChunk::Md(md) => md::rewrite_images(md, |dest| match (&dir, &root) {
                        (Some(dir), Some(root)) => md::preview_image(dir, root, dest),
                        _ => crate::os::web_url(dest).map(|_| dest.to_string()),
                    }),
                })
                .collect();
            self.md_preview = Some(MdPreview { key: (src, d.version), chunks, shown, heights, width: old.map_or(0.0, |o| o.width) });
        }
        let bg = self.hl.colors.bg;
        let cache = &mut self.md_cache;
        let Some(p) = self.md_preview.as_mut() else { return };
        egui::CentralPanel::default().frame(egui::Frame::none().fill(bg)).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt(("md_preview", src)).auto_shrink([false, false]).show(ui, |ui| {
                let w = ui.available_width().min(880.0);
                let pad = ((ui.available_width() - w) / 2.0).max(16.0);
                if (p.width - w).abs() > 0.5 {
                    p.width = w;
                    p.heights.iter_mut().for_each(|h| *h = 0.0);
                }
                egui::Frame::none().inner_margin(egui::Margin { left: pad, right: pad, top: 16.0, bottom: 40.0 }).show(ui, |ui| {
                    ui.set_max_width(w);
                    let clip = ui.clip_rect();
                    for (k, chunk) in p.chunks.iter().enumerate() {
                        let top = ui.cursor().top();
                        // a piece measured and out of sight: only its place is kept
                        let h = p.heights[k];
                        if h > 0.0 && (top > clip.bottom() || top + h < clip.top()) {
                            ui.allocate_space(egui::vec2(w, h));
                            continue;
                        }
                        match chunk {
                            MdChunk::Mermaid(code) => {
                                ui.add_space(6.0);
                                super::mermaid::show(ui, code);
                                ui.add_space(10.0);
                            }
                            MdChunk::Md(_) => {
                                let viewer = egui_commonmark::CommonMarkViewer::new().explicit_image_uri_scheme(true);
                                viewer.max_image_width(Some(w as usize)).show(ui, cache, &p.shown[k]);
                            }
                        }
                        p.heights[k] = ui.cursor().top() - top;
                    }
                });
            });
        });
    }

    /// The guide: its intro, then a row of buttons to each `##` section (they scroll to it), then
    /// the sections, each under a rule.
    fn guide_view(&mut self, ui: &mut egui::Ui, key: &str, text: &str) {
        let bg = self.hl.colors.bg;
        let accent = self.hl.colors.accent;
        // cut into sections once (55 KB of Markdown), not at each frame
        if self.guide.as_ref().is_none_or(|g| g.key != key) {
            let (intro, sections) = md::sections(text);
            let heights = vec![0.0; sections.len()];
            self.guide = Some(GuideView { key: key.to_string(), intro, sections, heights, width: 0.0 });
        }
        let Some(g) = self.guide.as_mut() else { return };
        let mut go_to = None;
        let cache = &mut self.md_cache;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(bg)).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt(key).auto_shrink([false, false]).show(ui, |ui| {
                let w = ui.available_width().min(880.0);
                let pad = ((ui.available_width() - w) / 2.0).max(16.0);
                if (g.width - w).abs() > 0.5 {
                    g.width = w;
                    g.heights.iter_mut().for_each(|h| *h = 0.0);
                }
                egui::Frame::none().inner_margin(egui::Margin { left: pad, right: pad, top: 16.0, bottom: 40.0 }).show(ui, |ui| {
                    ui.set_max_width(w);
                    egui_commonmark::CommonMarkViewer::new().show(ui, cache, &g.intro);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                        for (i, (title, _)) in g.sections.iter().enumerate() {
                            let chip = egui::Button::new(RichText::new(title).small().color(accent)).rounding(12.0);
                            if ui.add(chip).clicked() {
                                go_to = Some(i);
                            }
                        }
                    });
                    ui.add_space(12.0);
                    let clip = ui.clip_rect();
                    for (i, (_, body)) in g.sections.iter().enumerate() {
                        let top = ui.cursor().top();
                        // a section measured and out of sight: only its place is kept
                        let h = g.heights[i];
                        if h > 0.0 && go_to != Some(i) && (top > clip.bottom() || top + h < clip.top()) {
                            ui.allocate_space(egui::vec2(w, h));
                            continue;
                        }
                        let r = ui.scope(|ui| {
                            ui.separator();
                            egui_commonmark::CommonMarkViewer::new().show(ui, cache, body);
                        });
                        g.heights[i] = ui.cursor().top() - top;
                        if go_to == Some(i) {
                            r.response.scroll_to_me(Some(egui::Align::TOP));
                        }
                    }
                });
            });
        });
    }

    /// Opens (or shows) the rendered preview of the active Markdown file, in its panel or beside it.
    pub(super) fn markdown_preview(&mut self, side: bool) {
        let Some(d) = self.docs.get(self.active) else { return };
        let is_md = d.syntax.contains("Markdown") || d.path.as_deref().is_some_and(|p| p.extension().is_some_and(|e| e == "md" || e == "markdown"));
        if !is_md {
            self.set_status(t!("L'aperçu est pour les fichiers Markdown (.md)", "The preview is for Markdown files (.md)"));
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
        p.title_override = Some(tf!("Aperçu : {title}", "Preview: {title}"));
        p.pane = if side { 1 - src_pane } else { src_pane };
        self.docs.push(p);
        if side {
            // keep typing in the source, the preview follows beside it
            self.panes.split_dir = SplitDir::Right;
            self.active = src_idx;
        } else {
            self.active = self.docs.len() - 1;
        }
    }

    /// Bar above a file changed or deleted by another program while it had unsaved changes:
    /// which version to keep (saving without choosing asks first).
    pub(super) fn disk_bar(&mut self, ui: &mut egui::Ui, i: usize, pane: u8) {
        let Some(change) = self.docs.get(i).and_then(|d| d.disk_change) else { return };
        let (mut reload, mut keep, mut close) = (false, false, false);
        let bg = icons::mix(ui.visuals().panel_fill, warn(), 0.15);
        egui::TopBottomPanel::top(egui::Id::new("disk_change").with(pane))
            .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(10.0, 4.0)))
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    match change {
                        document::DiskChange::Changed => {
                            ui.label(RichText::new(t!("⚠ Ce fichier a été modifié par un autre programme", "⚠ This file was changed by another program")).color(warn()).strong());
                            reload = ui.button(t!("Recharger la version du disque", "Reload the version on disk")).on_hover_text(t!("Tes changements non enregistrés sont remplacés (Ctrl+Z pour les retrouver)", "Your unsaved changes are replaced (Ctrl+Z to get them back)")).clicked();
                            keep = ui.button(t!("Garder la mienne", "Keep mine")).on_hover_text(t!("Le prochain enregistrement écrase la version du disque", "The next save overwrites the version on disk")).clicked();
                        }
                        document::DiskChange::Deleted => {
                            ui.label(RichText::new(t!("⚠ Ce fichier a été supprimé du disque", "⚠ This file was deleted from the disk")).color(warn()).strong());
                            close = ui.button(t!("Fermer l'onglet", "Close the tab")).clicked();
                            keep = ui.button(t!("Le garder", "Keep it")).on_hover_text(t!("Ctrl+S le recrée", "Ctrl+S creates it again")).clicked();
                        }
                    }
                });
            });
        if reload {
            if let Err(e) = self.docs[i].reload_from_disk() {
                self.set_error(tf!("Rechargement impossible : {e}", "Cannot reload: {e}"));
            }
        }
        if keep {
            self.docs[i].disk_change = None;
        }
        if close {
            self.close_tab(i, false);
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
        // the views (Claude, Git, GitHub, Docker, terminal…) have their own bar: no room lost
        if !matches!(d.view, document::View::Text | document::View::Image | document::View::Preview(_)) {
            return;
        }
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
            symbols::path_at(d.symbols_lazy(), line).iter().map(|s| (s.name.clone(), s.kind, s.line)).collect();
        // the symbols kept while typing: computed again once it pauses
        if let Some(w) = d.symbols_wait() {
            ui.ctx().request_repaint_after(w);
        }
        let is_md = d.view == document::View::Text && d.syntax.contains("Markdown");
        let (mut goto, mut list, mut preview, mut open, mut reveal) = (None, false, false, None, None);
        let bg = self.hl.colors.bg;
        let text = ui.visuals().text_color();
        let pill = ui.visuals().widgets.hovered.weak_bg_fill;
        egui::TopBottomPanel::top(egui::Id::new("crumbs").with(pane))
            .exact_height(24.0)
            .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(10.0, 0.0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    // pale and small: the pinned lines of the editor already show where you are
                    let weak = ui.visuals().weak_text_color();
                    let sep = |ui: &mut egui::Ui| {
                        ui.label(RichText::new("›").size(13.0).color(weak));
                    };
                    for (dname, dir) in &dirs {
                        match dir {
                            // a folder: its files and folders in a menu
                            Some(dir) => {
                                let r = ui.menu_button(RichText::new(dname).size(12.0).color(weak), |ui| {
                                    ui.set_max_height(420.0);
                                    if ui.button(t!("Afficher dans l'arborescence", "Show in the tree")).clicked() {
                                        reveal = Some(dir.clone());
                                        ui.close_menu();
                                    }
                                    ui.separator();
                                    // read again every 2 s, not at every frame (slow on network folders)
                                    type Entries = std::sync::Arc<Vec<(String, PathBuf, bool)>>;
                                    let id = egui::Id::new(("crumb_entries", dir));
                                    let cached = ui.ctx().data(|d| d.get_temp::<(Instant, Entries)>(id)).filter(|(t, _)| t.elapsed() < Duration::from_secs(2));
                                    let entries = match cached {
                                        Some((_, e)) => e,
                                        None => {
                                            let mut e: Vec<(String, PathBuf, bool)> = std::fs::read_dir(dir)
                                                .map(|rd| {
                                                    rd.flatten()
                                                        .filter(|e| e.file_name() != ".git")
                                                        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path(), e.path().is_dir()))
                                                        .collect()
                                                })
                                                .unwrap_or_default();
                                            e.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
                                            let e: Entries = std::sync::Arc::new(e);
                                            ui.ctx().data_mut(|d| d.insert_temp(id, (Instant::now(), e.clone())));
                                            e
                                        }
                                    };
                                    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                                        for (name, path, is_dir) in entries.iter().cloned() {
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
                                ui.label(RichText::new(dname).size(12.0).color(weak));
                            }
                        }
                        sep(ui);
                    }
                    // the file name, in a pill: where you are in the path
                    let r = egui::Frame::none()
                        .fill(pill)
                        .rounding(5.0)
                        .inner_margin(egui::Margin::symmetric(6.0, 1.0))
                        .show(ui, |ui| ui.add(egui::Label::new(RichText::new(&file).size(12.0).strong().color(text)).sense(Sense::click())).on_hover_text(t!("Symboles du fichier (Ctrl+R)", "Symbols of the file (Ctrl+R)")))
                        .inner;
                    if r.clicked() {
                        list = true;
                    }
                    for (name, kind, l) in &chain {
                        sep(ui);
                        let text = RichText::new(format!("{} {name}", kind.icon())).size(12.0).color(accent);
                        if ui.add(egui::Label::new(text).sense(Sense::click())).on_hover_text(tf!("Aller à la ligne {}", "Go to line {}", l + 1)).clicked() {
                            goto = Some(*l);
                        }
                    }
                    if is_md {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(t!("Aperçu", "Preview")).on_hover_text(t!("Aperçu Markdown à côté (Ctrl+K V)", "Markdown preview to the side (Ctrl+K V)")).clicked() {
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
        let Some(from) = self.panes.tab_drag else { return };
        let Some(p) = ctx.pointer_latest_pos() else { return };
        if from >= self.docs.len() {
            self.panes.tab_drag = None;
            return;
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        let accent = self.hl.colors.accent;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tab_drop")));
        let src = self.docs[from].pane;
        let split = self.is_split();
        let alone = self.docs.iter().filter(|d| d.pane == src).count() == 1;
        let zones = std::mem::take(&mut self.panes.drop_zones);
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
                hint(*r, t!("Déplacer dans ce panneau", "Move to this pane"));
                target = Some(TabDrop::Pane(*pane));
            } else if !split && !alone {
                if p.x > r.left() + r.width() * 0.6 {
                    hint(egui::Rect::from_min_max(egui::pos2(r.center().x, r.top()), r.max), t!("Diviser à droite", "Split right"));
                    target = Some(TabDrop::Split(SplitDir::Right));
                } else if p.y > r.top() + r.height() * 0.6 {
                    hint(egui::Rect::from_min_max(egui::pos2(r.left(), r.center().y), r.max), t!("Diviser en bas", "Split down"));
                    target = Some(TabDrop::Split(SplitDir::Down));
                }
            }
        }
        if !ctx.input(|i| i.pointer.primary_down()) {
            self.panes.tab_drag = None;
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
        self.panes.tab_scrolled = None;
    }

    pub(super) fn close_tab(&mut self, idx: usize, force: bool) {
        let Some(doc) = self.docs.get(idx) else { return };
        if doc.is_dirty() && !force {
            self.confirm = Some(Confirm::CloseTab(doc.uid));
            return;
        }
        let was_active = idx == self.active;
        let d = self.docs.remove(idx);
        if d.view == document::View::History {
            self.line_history = None;
        }
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

/// A piece of a Markdown preview, ready to show.
#[derive(PartialEq)]
pub(super) enum MdChunk {
    /// A Mermaid diagram drawn apart.
    Mermaid(String),
    /// Markdown (HTML written in it already turned into Markdown).
    Md(String),
}

/// The preview of a Markdown file, cut into pieces (at the headings) for one version of its text:
/// only the pieces in sight are laid out at each frame.
pub(super) struct MdPreview {
    /// (uid of the document, its version).
    key: (u64, u64),
    chunks: Vec<MdChunk>,
    /// The Markdown pieces as shown: their images checked (`md::preview_image`).
    shown: Vec<String>,
    /// Height of each piece at the last frame it was shown (0: not measured yet).
    heights: Vec<f32>,
    width: f32,
}

/// The guide shown, cut into its sections.
pub(super) struct GuideView {
    /// Which guide (its language).
    key: String,
    intro: String,
    sections: Vec<(String, String)>,
    /// Height of each section at the last frame it was shown (0: not measured yet).
    heights: Vec<f32>,
    width: f32,
}

/// `text` cut into pieces: Mermaid diagrams apart, the rest cut before each heading (outside code).
fn md_chunks(text: &str) -> Vec<MdChunk> {
    let mut out = Vec::new();
    let mut pending = String::new();
    let flush = |out: &mut Vec<MdChunk>, md: &mut String| {
        if !md.trim().is_empty() {
            out.push(MdChunk::Md(md::html_to_md(md)));
        }
        md.clear();
    };
    for part in md::parts(text) {
        match part {
            md::MdPart::Code { lang, code } if lang.eq_ignore_ascii_case("mermaid") && super::mermaid::parse_sequence(&code).is_some() => {
                flush(&mut out, &mut pending);
                out.push(MdChunk::Mermaid(code));
            }
            md::MdPart::Code { lang, code } => {
                let fence = if code.contains("```") { "~~~~" } else { "```" };
                pending.push_str(&format!("{fence}{lang}\n{code}\n{fence}\n"));
            }
            md::MdPart::Text(t) => {
                for line in t.split_inclusive('\n') {
                    if line.starts_with('#') && !pending.trim().is_empty() {
                        flush(&mut out, &mut pending);
                    }
                    pending.push_str(line);
                }
            }
            // not made by `parts`
            md::MdPart::Table { .. } => {}
        }
    }
    flush(&mut out, &mut pending);
    out
}
