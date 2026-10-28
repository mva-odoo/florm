//! The strip of tabs of the editor (or of one pane): tabs, groups, too many tabs, their menu.

use super::*;

impl App {
    /// Tab strip drawn above the editor area only (not above the sidebar).
    /// Tab strip: every tab (`pane == None`, with the history / split / settings tabs) or those of one panel.
    pub(in crate::app) fn tabs_strip(&mut self, ui: &mut egui::Ui, pane: Option<u8>) {
        let tc = self.hl.tab_colors();
        let (mut activate, mut close) = (None, None);
        let (mut drag_start, mut menu_act) = (None, None);
        let is_split = self.is_split();
        let mut counts = [0usize; 2];
        for d in &self.docs {
            counts[(d.pane as usize).min(1)] += 1;
        }
        let mut scrolled = None;
        let mut strip = (egui::Rect::NOTHING, Vec::new());
        let panel_id = egui::Id::new("tabs").with(pane);
        egui::TopBottomPanel::top(panel_id).exact_height(TAB_STRIP_H).frame(egui::Frame::none().fill(tc.bar)).show_inside(ui, |ui| {
            let bar = ui.max_rect();
            strip.0 = bar;
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
            // too many tabs for the strip: ‹ › and the list of all of them on its right
            let (over_id, offset_id) = (panel_id.with("over"), panel_id.with("offset"));
            let over = ui.ctx().data(|d| d.get_temp::<bool>(over_id)).unwrap_or(false);
            let (offset, width) = ui.ctx().data(|d| d.get_temp::<(f32, f32)>(offset_id)).unwrap_or((0.0, 0.0));
            const ARROWS_W: f32 = 76.0;
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
                ui.allocate_ui_at_rect(r, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let page = (width * 0.75).max(120.0);
                        if ui.small_button("‹").on_hover_text(t!("Onglets à gauche", "Tabs on the left")).clicked() {
                            goto = Some(offset - page);
                        }
                        if ui.small_button("›").on_hover_text(t!("Onglets à droite", "Tabs on the right")).clicked() {
                            goto = Some(offset + page);
                        }
                        ui.menu_button("▾", |ui| {
                            ui.set_min_width(240.0);
                            egui::ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                                for (i, d) in self.docs.iter().enumerate().filter(|(_, d)| pane.is_none_or(|p| p == d.pane)) {
                                    let (_, title) = doc_tab(d);
                                    let label = if d.is_dirty() { format!("● {title}") } else { title };
                                    let r = ui.selectable_label(i == self.active, label);
                                    let r = match &d.path {
                                        Some(p) => r.on_hover_text(p.display().to_string()),
                                        None => r,
                                    };
                                    if r.clicked() {
                                        activate = Some(i);
                                        ui.close_menu();
                                    }
                                }
                            });
                        })
                        .response
                        .on_hover_text(t!("Tous les onglets", "All the tabs"));
                    });
                });
            }
            ui.allocate_ui_at_rect(scroll, |ui| {
            let mut area = egui::ScrollArea::horizontal()
                .id_salt(panel_id.with("scroll"))
                .drag_to_scroll(false)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
            if let Some(x) = goto {
                area = area.horizontal_scroll_offset(x.max(0.0));
            }
            let out = area
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.add_space(6.0);
                        let mut groups: Vec<String> = Vec::new();
                        for g in self.docs.iter().filter_map(|d| d.group.clone()) {
                            if !groups.contains(&g) {
                                groups.push(g);
                            }
                        }
                        let mut prev_group: Option<&str> = None;
                        for (i, d) in self.docs.iter().enumerate() {
                            if pane.is_some_and(|p| p != d.pane) {
                                continue;
                            }
                            // where a group starts: its label, which folds it
                            if let Some(g) = d.group.as_deref() {
                                let folded = self.folded_groups.contains(g);
                                if prev_group != Some(g) {
                                    let count = self.docs.iter().filter(|x| x.group.as_deref() == Some(g) && pane.is_none_or(|p| p == x.pane)).count();
                                    let r = group_pill(ui, g, count, folded);
                                    if r.clicked() {
                                        menu_act = Some(TabAct::FoldGroup(g.to_string()));
                                    }
                                    r.context_menu(|ui| group_menu(ui, g, &mut menu_act));
                                }
                                prev_group = Some(g);
                                if folded && i != self.active {
                                    continue;
                                }
                            } else {
                                prev_group = None;
                            }
                            let shown = match pane {
                                Some(p) if p != self.focused_pane => self.pane_active[p as usize] == Some(d.uid),
                                _ => i == self.active,
                            };
                            let active = shown;
                            let error = d.diagnostics.iter().any(|g| g.severity == Severity::Error);
                            // a Claude conversation: working, or finished and not seen yet
                            let tint = if error { Some(err()) } else { self.claude_tab_tint(d.uid) };
                            let (icon, title) = doc_tab(d);
                            let flash = if error { None } else { self.claude_tab_flash(d.uid) };
                            let (body, closed) = tab_widget(ui, &title, icon, d.is_dirty(), active, tint, flash, &tc);
                            if tint.is_some() && !error {
                                let tip = if self.claude_tab_tint(d.uid) == Some(ok()) { t!("Claude a fini", "Claude is done") } else { t!("Claude travaille…", "Claude is working…") };
                                let _ = body.clone().on_hover_text(tip);
                            }
                            // the tabs of a group: a band of its color on top
                            if let Some(g) = &d.group {
                                let r = body.rect;
                                ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(r.left() + 4.0, r.top() + 4.0), egui::pos2(r.right() - 4.0, r.top() + 6.5)), 1.5, group_color(g));
                            }
                            strip.1.push((body.rect, i));
                            if body.drag_started() {
                                drag_start = Some(i);
                            }
                            if closed || body.middle_clicked() {
                                close = Some(i);
                            } else if body.clicked() {
                                activate = Some(i);
                            }
                            if i == self.active && self.tab_scrolled != Some(i) {
                                body.scroll_to_me(None);
                                scrolled = Some(i);
                            }
                            let alone = counts[(d.pane as usize).min(1)] == 1;
                            body.context_menu(|ui| tab_menu(ui, i, d, is_split, alone, &groups, &mut menu_act));
                            if let Some(p) = &d.path {
                                body.on_hover_text(p.display().to_string());
                            }
                        }
                    });
                });
            let width = out.inner_rect.width();
            // with the arrows shown, the strip is narrower: they stay until the tabs fit without them
            let needs = out.content_size.x > width + if over { ARROWS_W } else { 1.0 };
            ui.ctx().data_mut(|d| {
                d.insert_temp(over_id, needs);
                d.insert_temp(offset_id, (out.state.offset.x, width));
            });
            if needs != over {
                ui.ctx().request_repaint();
            }
            });
            if let Some(c) = controls {
                crate::app::title_bar::strip_buttons(ui, bar, c, maximized);
            }
        });
        self.drop_zones.strips.push((pane.unwrap_or(0), strip.0, strip.1));
        if scrolled.is_some() {
            self.tab_scrolled = scrolled;
        }
        if let Some(i) = drag_start {
            self.tab_drag = Some(i);
        }
        if let Some(i) = activate {
            self.activate_tab(i);
        }
        if let Some(i) = close {
            self.close_tab(i, false);
        }
        match menu_act {
            Some(TabAct::Split(i, dir)) => self.split_tab(i, Some(dir)),
            Some(TabAct::MoveOther(i)) => self.split_tab(i, None),
            Some(TabAct::Unsplit) => self.unsplit(),
            Some(TabAct::Flip) => {
                self.split_dir = if self.split_dir == SplitDir::Right { SplitDir::Down } else { SplitDir::Right };
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
                self.tab_scrolled = None;
                let kept = self.docs.iter().filter(|d| d.pane == p && d.uid != uid).count();
                let closed = before - self.docs.len();
                self.set_status(if kept > 0 {
                    tf!("{closed} onglet(s) fermé(s), {kept} non enregistré(s) gardé(s)", "{closed} tab(s) closed, {kept} unsaved kept")
                } else {
                    tf!("{closed} onglet(s) fermé(s)", "{closed} tab(s) closed")
                });
            }
            Some(TabAct::CopyPath(t)) => {
                ui.ctx().output_mut(|o| o.copied_text = t.clone());
                self.set_status(tf!("Copié : {t}", "Copied: {t}"));
            }
            Some(TabAct::Reveal(p)) => self.reveal(&p),
            Some(TabAct::Group(i, g)) => {
                let g = g.unwrap_or_else(|| {
                    (1..).map(|n| tf!("Groupe {n}", "Group {n}")).find(|n| !self.docs.iter().any(|d| d.group.as_ref() == Some(n))).unwrap_or_default()
                });
                self.set_tab_group(i, Some(g));
            }
            Some(TabAct::Ungroup(i)) => self.set_tab_group(i, None),
            Some(TabAct::FoldGroup(g)) => {
                if !self.folded_groups.remove(&g) {
                    self.folded_groups.insert(g);
                }
            }
            Some(TabAct::CloseGroup(g)) => {
                let before = self.docs.len();
                let keep = self.docs.get(self.active).map(|d| d.uid);
                for j in (0..self.docs.len()).rev() {
                    if self.docs[j].group.as_ref() == Some(&g) && !self.docs[j].is_dirty() {
                        self.close_tab(j, true);
                    }
                }
                if let Some(k) = keep.and_then(|u| self.docs.iter().position(|d| d.uid == u)) {
                    self.active = k;
                }
                self.active = self.active.min(self.docs.len().saturating_sub(1));
                self.tab_scrolled = None;
                let kept = self.docs.iter().filter(|d| d.group.as_ref() == Some(&g)).count();
                self.folded_groups.remove(&g);
                let closed = before - self.docs.len();
                self.set_status(if kept > 0 {
                    tf!("{closed} onglet(s) fermé(s), {kept} non enregistré(s) gardé(s)", "{closed} tab(s) closed, {kept} unsaved kept")
                } else {
                    tf!("{closed} onglet(s) fermé(s)", "{closed} tab(s) closed")
                });
            }
            Some(TabAct::Dissolve(g)) => {
                for d in self.docs.iter_mut().filter(|d| d.group.as_ref() == Some(&g)) {
                    d.group = None;
                }
                self.folded_groups.remove(&g);
            }
            Some(TabAct::RenameGroup(old, new)) => {
                for d in self.docs.iter_mut().filter(|d| d.group.as_ref() == Some(&old)) {
                    d.group = Some(new.clone());
                }
                if self.folded_groups.remove(&old) {
                    self.folded_groups.insert(new);
                }
            }
            Some(TabAct::Locate(i)) => {
                self.activate_tab(i);
                if let Some(p) = self.docs.get(i).and_then(|d| d.path.clone()) {
                    self.reveal_in_tree(&p);
                }
            }
            None => {}
        }
    }
}
