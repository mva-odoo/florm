//! The strip of tabs of the editor (or of one pane): tabs, groups, too many tabs, their menu.

use super::*;

impl App {
    /// Tab strip drawn above the editor area only (not above the sidebar).
    /// Tab strip: every tab (`pane == None`, with the history / split / settings tabs) or those of one panel.
    /// Returns the rectangle of the tab shown, where the editor's card leaves its top border open.
    pub(in crate::app) fn tabs_strip(&mut self, ui: &mut egui::Ui, pane: Option<u8>) -> Option<egui::Rect> {
        let tc = self.hl.tab_colors();
        let (mut activate, mut close) = (None, None);
        let (mut drag_start, mut menu_act) = (None, None);
        // « + » (a new text tab) on the right of the strip
        let mut new_text = false;
        let is_split = self.is_split();
        let mut counts = [0usize; 2];
        for d in &self.docs {
            counts[(d.pane as usize).min(1)] += 1;
        }
        let mut scrolled = None;
        let mut shown_rect = None;
        let mut strip = (egui::Rect::NOTHING, Vec::new());
        let panel_id = egui::Id::unique("tabs").with(pane);
        // the views as tabs of their icon, at the start of the first strip: a click shows the view
        // (its tab), a click on the one shown goes back to the tab before it
        let first = pane.is_none_or(|p| p == 0);
        let icon_tabs = if first { self.icon_tabs() } else { Vec::new() };
        let mut icon_act: Option<IconAct> = None;
        let logo_here = first && !self.show_sidebar;
        // on the window's background, above the editor's card (its border runs under the tabs)
        egui::Panel::top(panel_id).exact_size(TAB_STRIP_H).show_separator_line(false).frame(egui::Frame::NONE.fill(tc.bar)).show(ui, |ui| {
            let bar = ui.max_rect();
            strip.0 = bar;
            // as in Zed: a line under the strip but not under the active tab, which opens onto
            // the path and the text (the other tabs draw it again over their fill)
            ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, egui::Stroke::new(1.0_f32, tc.border));
            let controls = self.strip_controls(pane);
            let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            let mut scroll = bar;
            if let Some(c) = controls {
                // the empty part of the strip moves the window, as the title bar does
                let drag = ui.interact(bar, panel_id.with("drag"), egui::Sense::click_and_drag());
                crate::app::title_bar::drag_window(ui.ctx(), &drag, maximized);
                match c {
                    crate::os::Controls::Side => scroll.max.x -= crate::app::title_bar::WINDOW_BUTTONS_W,
                    _ => scroll.min.x += crate::app::title_bar::MAC_BUTTONS_W,
                }
            }
            // the F of the menus (the sidebar hidden: it is on top of it otherwise) and the icon tabs
            if logo_here || !icon_tabs.is_empty() {
                // the F with the version and the update beside it
                let logo_w = if logo_here { 34.0 + 52.0 + if self.upd.found.is_some() { 22.0 } else { 0.0 } } else { 0.0 };
                let w = logo_w + icon_tabs.len() as f32 * 38.0 + 4.0;
                let r = egui::Rect::from_min_max(scroll.min, egui::pos2(scroll.min.x + w, scroll.max.y));
                scroll.min.x += w;
                ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if logo_here {
                            ui.add_space(6.0);
                            ui.allocate_ui_with_layout(egui::vec2(logo_w - 8.0, TAB_STRIP_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.set_min_size(egui::vec2(logo_w - 8.0, TAB_STRIP_H));
                                self.menu_logo(ui);
                                self.version_and_update(ui);
                            });
                            ui.add_space(2.0);
                        }
                        for (k, t) in icon_tabs.iter().enumerate() {
                            let r = crate::app::icon_tab(ui, t.icon, t.shown, k == 0, t.color, &tc).on_hover_text(&t.tip);
                            // Claude in the sandbox: a small green shield on its icon
                            if t.view == Some(crate::document::View::Claude) && self.settings.claude_sandbox {
                                let at = r.rect.center() + egui::vec2(8.0, 6.0);
                                ui.painter().circle_filled(at, 5.5, if t.shown { tc.active } else { tc.bar });
                                crate::icons::paint_shield(ui.painter(), at, 9.0, crate::highlight::green(), Color32::WHITE);
                            }
                            if t.shown && t.view.is_some() {
                                shown_rect = Some(r.rect);
                            }
                            if r.clicked() {
                                icon_act = Some(match (t.view, t.shown) {
                                    (Some(v), true) => IconAct::Leave(v),
                                    (None, true) => IconAct::HideBottom,
                                    _ => IconAct::Cmd(t.cmd),
                                });
                            }
                            if t.view == Some(crate::document::View::Settings) {
                                // its right click: straight to the languages and the extensions
                                r.context_menu(|ui| {
                                    for (label, cmd) in [
                                        (t!("Préférences", "Preferences"), Cmd::OpenSettings),
                                        (t!("Langages : outils et tree-sitter…", "Languages: tools and tree-sitter…"), Cmd::ShowLanguages),
                                        (t!("Extensions et catalogue…", "Extensions and catalogue…"), Cmd::ShowExtensions),
                                    ] {
                                        if ui.button(label).clicked() {
                                            icon_act = Some(IconAct::Cmd(cmd));
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        }
                    });
                });
            }
            // too many tabs for the strip: ‹ › on the right, and the list of all of them in the ▾ menu
            let (over_id, offset_id) = (panel_id.with("over"), panel_id.with("offset"));
            let over = ui.ctx().data(|d| d.get_temp::<bool>(over_id)).unwrap_or(false);
            let (offset, width) = ui.ctx().data(|d| d.get_temp::<(f32, f32)>(offset_id)).unwrap_or((0.0, 0.0));
            // always on the right: + for a new text tab
            const NEW_W: f32 = 32.0;
            let new_rect = egui::Rect::from_min_max(egui::pos2(scroll.max.x - NEW_W, bar.min.y), egui::pos2(scroll.max.x, bar.max.y));
            scroll.max.x -= NEW_W;
            ui.scope_builder(egui::UiBuilder::new().max_rect(new_rect), |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.add_space(4.0);
                    let plus = ui.add(egui::Button::new(egui::RichText::new("+").size(17.0)).frame(false).min_size(egui::vec2(22.0, 22.0)));
                    if plus.on_hover_text(t!("Nouvel onglet texte vide (Ctrl+N)", "New empty text tab (Ctrl+N)")).clicked() {
                        new_text = true;
                    }
                });
            });
            const ARROWS_W: f32 = 50.0;
            let arrows = over.then(|| {
                let r = egui::Rect::from_min_max(egui::pos2(scroll.max.x - ARROWS_W, bar.min.y), egui::pos2(scroll.max.x, bar.max.y));
                scroll.max.x -= ARROWS_W;
                r
            });
            let mut goto: Option<f32> = None;
            // the wheel moves the strip sideways
            if over && ui.rect_contains_pointer(scroll) {
                let d = ui.input(|i| i.smooth_scroll_delta);
                if d.y != 0.0 && d.x == 0.0 {
                    goto = Some(offset - d.y);
                }
            }
            if let Some(r) = arrows {
                ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let page = (width * 0.75).max(120.0);
                        if ui.small_button("‹").on_hover_text(t!("Onglets à gauche", "Tabs on the left")).clicked() {
                            goto = Some(offset - page);
                        }
                        if ui.small_button("›").on_hover_text(t!("Onglets à droite", "Tabs on the right")).clicked() {
                            goto = Some(offset + page);
                        }
                    });
                });
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(scroll), |ui| {
            let mut area = egui::ScrollArea::horizontal()
                .id_salt(panel_id.with("scroll"))
                .scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Never, ..Default::default() })
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
            if let Some(x) = goto {
                area = area.horizontal_scroll_offset(x.max(0.0));
            }
            let out = area
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // a small gap between the tabs; the first one starts after the card's rounded corner
                        // side by side, as in Zed
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (i, d) in self.docs.iter().enumerate() {
                            if pane.is_some_and(|p| p != d.pane) || crate::app::icon_view(d.view) {
                                continue;
                            }
                            let shown = match pane {
                                Some(p) if p != self.panes.focused => self.panes.active[p as usize] == Some(d.uid),
                                _ => i == self.active,
                            };
                            let active = shown;
                            let error = d.diagnostics.iter().any(|g| g.severity == Severity::Error);
                            // a Claude conversation: working, or finished and not seen yet
                            let tint = if error { Some(err()) } else { self.claude_tab_tint(d.uid) };
                            let (icon, title) = doc_tab(d);
                            let flash = if error { None } else { self.claude_tab_flash(d.uid) };
                            let errors = d.diagnostics.iter().filter(|g| g.severity == Severity::Error).count();
                            // two open files of the same name: their folder after the title
                            let hint = d.path.as_deref().filter(|p| same_name(&self.docs, p)).and_then(|p| p.parent()?.file_name()).map(|f| format!("…/{}", f.to_string_lossy()));
                            let (body, closed) = tab_widget(ui, &title, icon, d.is_dirty(), active, tint, flash, hint.as_deref(), errors, &tc);
                            if tint.is_some() && !error {
                                let tip = if self.claude_tab_tint(d.uid) == Some(ok()) { t!("Claude a fini", "Claude is done") } else { t!("Claude travaille…", "Claude is working…") };
                                let _ = body.clone().on_hover_text(tip);
                            }
                            strip.1.push((body.rect, i));
                            if active {
                                shown_rect = Some(body.rect);
                            }
                            if body.drag_started() {
                                drag_start = Some(i);
                            }
                            if closed || body.middle_clicked() {
                                close = Some(i);
                            } else if body.clicked() {
                                activate = Some(i);
                            }
                            if i == self.active && self.panes.tab_scrolled != Some(i) {
                                body.scroll_to_me(None);
                                scrolled = Some(i);
                            }
                            let alone = counts[(d.pane as usize).min(1)] == 1;
                            body.context_menu(|ui| tab_menu(ui, i, d, is_split, alone, &mut menu_act));
                            if let Some(p) = &d.path {
                                body.on_hover_text(p.display().to_string());
                            } else if title.len() > 24 {
                                // its title may be cut: whole on hover
                                body.on_hover_text(title);
                            }
                        }
                    });
                });
            let width = out.inner_rect.width();
            // with the arrows shown, the strip is narrower: they stay until the tabs fit without them
            let needs = out.content_size.x > width + if over { ARROWS_W } else { 1.0 };
            ui.ctx().data_mut(|d| {
                d.insert_temp(over_id, needs);
                d.insert_temp(offset_id, (out.state.clamped_offset().x, width));
            });
            if needs != over {
                ui.ctx().request_repaint();
            }
            });
            if let Some(c) = controls {
                crate::app::title_bar::strip_buttons(ui, bar, c, maximized);
            }
        });
        self.panes.drop_zones.strips.push((pane.unwrap_or(0), strip.0, strip.1));
        if scrolled.is_some() {
            self.panes.tab_scrolled = scrolled;
        }
        if let Some(i) = drag_start {
            self.panes.tab_drag = Some(i);
        }
        if let Some(i) = activate {
            self.activate_tab(i);
        }
        match icon_act {
            Some(IconAct::Leave(v)) => self.leave_tab(v),
            Some(IconAct::HideBottom) => self.bottom = None,
            Some(IconAct::Cmd(c)) => self.pending.push(c),
            None => {}
        }
        if let Some(i) = close {
            self.close_tab(i, false);
        }
        if new_text {
            self.pending.push(Cmd::NewFile);
        }
        match menu_act {
            Some(TabAct::Split(i, dir)) => self.split_tab(i, Some(dir)),
            Some(TabAct::MoveOther(i)) => self.split_tab(i, None),
            Some(TabAct::Unsplit) => self.unsplit(),
            Some(TabAct::Flip) => {
                self.panes.split_dir = if self.panes.split_dir == SplitDir::Right { SplitDir::Down } else { SplitDir::Right };
            }
            Some(TabAct::Close(i)) => self.close_tab(i, false),
            Some(TabAct::CloseOthers(i)) => {
                let (uid, p) = (self.docs[i].uid, self.docs[i].pane);
                let before = self.docs.len();
                for j in (0..self.docs.len()).rev() {
                    let d = &self.docs[j];
                    if d.uid != uid && d.pane == p && !d.is_dirty() {
                        self.close_tab(j, true);
                    }
                }
                self.active = self.docs.iter().position(|d| d.uid == uid).unwrap_or(0);
                self.panes.tab_scrolled = None;
                let kept = self.docs.iter().filter(|d| d.pane == p && d.uid != uid).count();
                let closed = before - self.docs.len();
                self.set_status(if kept > 0 {
                    tf!("{closed} onglet(s) fermé(s), {kept} non enregistré(s) gardé(s)", "{closed} tab(s) closed, {kept} unsaved kept")
                } else {
                    tf!("{closed} onglet(s) fermé(s)", "{closed} tab(s) closed")
                });
            }
            Some(TabAct::CopyPath(t)) => {
                ui.ctx().copy_text(t.clone());
                self.set_status(tf!("Copié : {t}", "Copied: {t}"));
            }
            Some(TabAct::Reveal(p)) => self.reveal(&p),
            Some(TabAct::Cmd(i, cmd)) => {
                self.activate_tab(i);
                self.pending.push(cmd);
            }
            Some(TabAct::Locate(i)) => {
                self.activate_tab(i);
                if let Some(p) = self.docs.get(i).and_then(|d| d.path.clone()) {
                    self.reveal_in_tree(&p);
                }
            }
            None => {}
        }
        shown_rect
    }
}

/// A view as a tab of its icon.
struct IconTab {
    icon: icons::ViewIcon,
    /// Its tab (None: a view of the bottom panel).
    view: Option<crate::document::View>,
    cmd: Cmd,
    shown: bool,
    color: Color32,
    tip: String,
}

/// What a click on an icon tab asks.
enum IconAct {
    /// Back to the tab before this view.
    Leave(crate::document::View),
    HideBottom,
    Cmd(Cmd),
}

impl App {
    /// The icon tabs: the preferences, Claude, Git, the pull requests (their tab), those turned on
    /// (the views of the bottom panel are at the right of the status bar).
    fn icon_tabs(&self) -> Vec<IconTab> {
        use crate::document::View;
        use crate::features::{on, Feature};
        let tc = self.hl.tab_colors();
        let (accent, muted) = (self.hl.colors.accent, tc.text_inactive);
        let active_view = self.docs.get(self.active).map(|d| d.view);
        let tip = |label: &str, cmd: Cmd| {
            let keys = self.keymap.label(cmd);
            if keys.is_empty() { label.to_string() } else { format!("{label} ({keys})") }
        };
        let mut v = Vec::new();
        for (on, icon, view, label, cmd) in [
            (true, icons::ViewIcon::Settings, View::Settings, t!("Préférences", "Preferences"), Cmd::OpenSettings),
            (on(Feature::Claude), icons::ViewIcon::Claude, View::Claude, "Claude", Cmd::Claude),
            (on(Feature::Git), icons::ViewIcon::Git, View::Git, "Git", Cmd::ShowGit),
            (on(Feature::GitHub), icons::ViewIcon::PullRequests, View::PullRequests, t!("Pull requests (GitHub)", "Pull requests (GitHub)"), Cmd::PullRequests),
            (true, icons::ViewIcon::Search, View::Search, t!("Rechercher dans les fichiers", "Find in files"), Cmd::FindInFilesTab),
        ] {
            if !on {
                continue;
            }
            let shown = active_view == Some(view);
            // Claude at work or done (not seen yet): its color, as its tab had
            let tint = (view == View::Claude).then(|| self.docs.iter().find(|d| d.view == View::Claude).and_then(|d| self.claude_tab_tint(d.uid))).flatten();
            let mut tip = tip(label, cmd);
            if view == View::Claude {
                tip.push_str(if self.settings.claude_sandbox { t!(" — bac à sable", " — sandbox") } else { t!(" — sans bac à sable", " — no sandbox") });
            }
            if view == View::Settings {
                tip.push_str(t!("\nClic droit : Langages (outils, tree-sitter), Extensions", "\nRight click: Languages (tools, tree-sitter), Extensions"));
            }
            v.push(IconTab { icon, view: Some(view), cmd, shown, color: tint.unwrap_or(if shown { accent } else { muted }), tip });
        }
        v
    }
}

/// Another open tab shows a file of the same name as `path`.
fn same_name(docs: &[Document], path: &Path) -> bool {
    let name = path.file_name();
    docs.iter().filter_map(|d| d.path.as_deref()).any(|p| p != path && p.file_name() == name)
}
