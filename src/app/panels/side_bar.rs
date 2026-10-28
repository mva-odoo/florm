//! The sidebar: the tree of the folder, the icons of the views, what the tree asks for.

use super::*;

/// The sidebar a little darker than the rest, to set the tree apart from the editor.
pub(super) fn sidebar_fill(v: &egui::Visuals) -> Color32 {
    icons::mix(v.panel_fill, Color32::BLACK, if v.dark_mode { 0.15 } else { 0.04 })
}

impl App {
    /// Thin strip at the left edge while the sidebar is hidden: a click brings it back.
    pub(in crate::app) fn sidebar_strip(&mut self, ctx: &egui::Context) {
        let panel = sidebar_fill(&ctx.style().visuals);
        egui::SidePanel::left("sidebar_strip").exact_width(10.0).resizable(false).frame(egui::Frame::none().fill(panel)).show(ctx, |ui| {
            let rect = ui.max_rect();
            let r = ui.interact(rect, egui::Id::new("sidebar_strip_btn"), Sense::click());
            let c = if r.hovered() { self.hl.colors.accent } else { ui.visuals().weak_text_color().gamma_multiply(0.5) };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "›", egui::FontId::proportional(14.0), c);
            if r.on_hover_text(t!("Afficher la barre latérale (Ctrl+K Ctrl+B)", "Show the sidebar (Ctrl+K Ctrl+B)")).clicked() {
                self.show_sidebar = true;
            }
        });
    }

    pub(in crate::app) fn sidebar(&mut self, ctx: &egui::Context) {
        let mut tree_action = None;
        let panel = sidebar_fill(&ctx.style().visuals);
        let shown = egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(260.0)
            .width_range(150.0..=700.0)
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin { left: 10.0, right: 8.0, top: 4.0, bottom: 6.0 }))
            .show(ctx, |ui| {
            // what is drawn inside (sticky folders, icons) on the same background
            ui.visuals_mut().panel_fill = panel;
            self.sidebar_mac_buttons(ui);
            // the views of the bottom panel, icons only: left click in the editor area, right click
            // there; projects are under the folder name (Ctrl+Alt+P). At the top or the bottom of the sidebar.
            let top = self.settings.sidebar_tools_top;
            let in_bar = self.settings.tools_in_status_bar;
            let frame = egui::Frame::none().inner_margin(if top { egui::Margin { left: 0.0, right: 0.0, top: 0.0, bottom: 6.0 } } else { egui::Margin { left: 0.0, right: 0.0, top: 6.0, bottom: 0.0 } });
            if in_bar {
            } else if top {
                egui::TopBottomPanel::top("sidebar_tools").frame(frame).show_inside(ui, |ui| self.sidebar_tools(ui, false));
            } else {
                egui::TopBottomPanel::bottom("sidebar_tools").frame(frame).show_inside(ui, |ui| self.sidebar_tools(ui, false));
            }
            match self.tree.as_mut() {
                Some(tree) => {
                    let active = self.docs.get(self.active).and_then(|d| d.path.clone());
                    let empty = HashMap::new();
                    let info = if self.settings.tree_git_info { &self.repo_info } else { &empty };
                    let projects: Vec<(String, PathBuf)> = self.projects.iter().filter(|p| p.path.is_dir()).map(|p| (p.name.clone(), p.path.clone())).collect();
                    if !self.tree_ex.as_ref().is_some_and(|(r, pats, _)| *r == tree.path && *pats == self.settings.exclude) {
                        let ex = crate::walk::Exclusions::new(&tree.path, &self.settings.exclude);
                        self.tree_ex = Some((tree.path.clone(), self.settings.exclude.clone(), ex));
                    }
                    let ex = &self.tree_ex.as_ref().expect("just set").2;
                    tree_action = sidebar::show(ui, tree, &projects, ex, active.as_deref(), &self.git.map, info, &mut self.tree_scroll, &mut self.tree_nav);
                }
                None => {
                    ui.label(t!("Aucun dossier ouvert.", "No folder open."));
                    if ui.button(t!("Ouvrir un dossier…", "Open a folder…")).clicked() {
                        self.pending.push(Cmd::OpenFolder);
                    }
                }
            }
        });
        // double click on the border between the sidebar and the editor: the width fits the longest name
        let edge = shown.response.rect.right();
        let fit = ctx.input(|i| {
            i.pointer.button_double_clicked(egui::PointerButton::Primary)
                && i.pointer.interact_pos().is_some_and(|p| (p.x - edge).abs() <= 5.0 && shown.response.rect.y_range().contains(p.y))
        });
        if fit {
            if let Some(tree) = self.tree.as_ref() {
                let width = sidebar::fit_width(ctx, tree).clamp(150.0, 700.0);
                let r = shown.response.rect;
                // the panel's width is kept by egui between frames: this one is the next frame's
                let rect = egui::Rect::from_min_max(r.min, egui::pos2(r.min.x + width, r.max.y));
                ctx.data_mut(|d| d.insert_persisted(egui::Id::new("sidebar"), egui::containers::panel::PanelState { rect }));
                ctx.request_repaint();
            }
        }
        if let Some(a) = tree_action {
            self.tree_action(ctx, a);
        }
    }

    /// The icons of the views (`sidebar`), with the menu that chooses which are shown.
    /// `in_bar`: in the status bar (one row, smaller) rather than in the sidebar.
    pub(super) fn sidebar_tools(&mut self, ui: &mut egui::Ui, in_bar: bool) {
        let top = self.settings.sidebar_tools_top;
        let size = if in_bar { egui::vec2(24.0, 22.0) } else { egui::vec2(26.0, 24.0) };
        let row = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
            if in_bar {
                ui.horizontal(|ui| add(ui));
            } else {
                ui.horizontal_wrapped(|ui| add(ui));
            }
        };
        row(ui, &mut |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let hidden = self.settings.sidebar_hidden.clone();
            let shown = |key: &str| !hidden.iter().any(|h| h == key);
            // preferences, left of the search (the first view)
            if shown("settings") {
                let (rect, r) = ui.allocate_exact_size(size, Sense::click());
                if r.hovered() {
                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let c = if r.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
                icons::paint_view(ui.painter(), rect.center(), icons::ViewIcon::Settings, c);
                if r.on_hover_text(t!("Préférences", "Preferences")).clicked() {
                    self.pending.push(Cmd::OpenSettings);
                }
            }
            // problems and output are in the status bar, the debugger opens by itself: not here
            for (tab, icon, label) in BottomTab::shown().filter(|(t, ..)| t.in_sidebar() && shown(&t.key())) {
                let (rect, r) = ui.allocate_exact_size(size, Sense::click());
                let on = self.bottom == Some(tab);
                // Claude finished in the bottom panel, not seen yet: in green
                let done = tab == BottomTab::Claude && !on && self.claude_bottom_done();
                if done {
                    ui.painter().rect_filled(rect, 4.0, icons::mix(ui.visuals().panel_fill, ok(), 0.3));
                } else if r.hovered() || on {
                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let c = if done { ok() } else if r.hovered() || on { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
                icons::paint_view(ui.painter(), rect.center(), icon, c);
                let r = r.on_hover_text(format!("{label}\n{}", BottomTab::hint(tab)));
                if r.clicked() && !tab.bottom_only() {
                    self.show_big(tab);
                } else if r.clicked() || r.secondary_clicked() {
                    self.show_bottom(tab, true);
                }
            }
            // which icons are shown
            let mut changed = None;
            let mut placed = None;
            let mut to_bar = None;
            let menu = ui.menu_button(RichText::new("▾").weak(), |ui| {
                ui.visuals_mut().override_text_color = None;
                ui.label(RichText::new(t!("Icônes affichées", "Icons shown")).small().weak());
                let mut items = vec![("settings".to_string(), t!("Préférences", "Preferences").to_string())];
                items.extend(BottomTab::shown().filter(|(t, ..)| t.in_sidebar()).map(|(t, _, l)| (t.key(), l.to_string())));
                for (key, label) in items {
                    let mut on = shown(&key);
                    if ui.checkbox(&mut on, label).changed() {
                        changed = Some((key, on));
                    }
                }
                ui.separator();
                if ui.add_enabled(!hidden.is_empty(), egui::Button::new(t!("Tout afficher", "Show all"))).clicked() {
                    changed = Some((String::new(), true));
                    ui.close_menu();
                }
                let mut bar = in_bar;
                if ui.checkbox(&mut bar, t!("Dans la barre d'état (en bas)", "In the status bar (at the bottom)")).changed() {
                    to_bar = Some(bar);
                }
                if !in_bar {
                    let mut at_top = top;
                    if ui.checkbox(&mut at_top, t!("En haut de la barre latérale", "At the top of the sidebar")).changed() {
                        placed = Some(at_top);
                    }
                }
            });
            if let Some(bar) = to_bar {
                self.settings.tools_in_status_bar = bar;
                self.persist("tools_in_status_bar", bar.to_string());
            }
            if let Some(at_top) = placed {
                self.settings.sidebar_tools_top = at_top;
                self.persist("sidebar_tools_top", at_top.to_string());
            }
            menu.response.on_hover_text(t!("Afficher ou masquer ces icônes (les vues restent dans la palette et les menus)", "Show or hide these icons (the views stay in the palette and the menus)"));
            if let Some((key, on)) = changed {
                let h = &mut self.settings.sidebar_hidden;
                match (key.is_empty(), on) {
                    (true, _) => h.clear(),
                    (false, true) => h.retain(|x| *x != key),
                    (false, false) => h.push(key),
                }
                let v = config::toml_str_array(h);
                self.persist("sidebar_hidden", v);
            }
        });
    }

    pub(super) fn tree_action(&mut self, ctx: &egui::Context, a: TreeAction) {
        match a {
            TreeAction::Open(p) => {
                self.open_path(p);
                self.focus_editor(ctx);
            }
            TreeAction::Show(p) => {
                self.open_path(p);
            }
            TreeAction::NewFile(d) => self.open_prompt(Prompt::NewFile(d)),
            TreeAction::NewFolder(d) => self.open_prompt(Prompt::NewFolder(d)),
            TreeAction::Rename(p) => self.open_prompt(Prompt::Rename(p)),
            TreeAction::Move(src, dir) => self.move_entry(src, dir),
            TreeAction::Delete(p) => self.confirm = Some(Confirm::Delete(p)),
            TreeAction::CopyPath(p) => ctx.output_mut(|o| o.copied_text = p.display().to_string()),
            TreeAction::Reveal(p) => self.reveal(&p),
            TreeAction::Terminal(d) => {
                let shell = self.term_shell();
                self.term.open(ctx, shell.as_deref(), Some(d));
                self.bottom = Some(BottomTab::Terminal);
            }
            TreeAction::Refresh => self.exec_refresh_tree(),
            TreeAction::Claude(p) => self.claude_attach_path(p),
            TreeAction::Compare(a, b) => self.compare_paths(a, b),
            TreeAction::SwitchProject(p) => self.switch_project(p),
            TreeAction::Exclude(p, on) => self.exclude_folder(&p, on),
            TreeAction::OpenFolder => self.pending.push(Cmd::OpenFolder),
            TreeAction::CopyFile(p) => self.copy_entry(&p),
            TreeAction::Paste(d) => self.paste_into(d),
            TreeAction::InitRepo(d) => self.init_repo(d),
        }
    }

    /// Shows `path` in the file tree: sidebar opened, folders expanded, entry scrolled into view.
    pub(in crate::app) fn reveal_in_tree(&mut self, path: &Path) {
        let Some(tree) = self.tree.as_mut() else {
            self.set_status(t!("Aucun dossier ouvert", "No folder open"));
            return;
        };
        let path = crate::os::canon(path);
        if !tree.reveal(&path) {
            self.set_status(t!("Ce fichier n'est pas dans le dossier ouvert", "This file is not in the open folder"));
            return;
        }
        self.show_sidebar = true;
        self.tree_nav.cursor = Some(path.clone());
        self.tree_scroll = Some(path);
    }

    pub(in crate::app) fn reveal(&mut self, path: &Path) {
        if let Err(e) = crate::os::reveal(path) {
            self.set_error(e);
        }
    }
}
