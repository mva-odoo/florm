//! The sidebar: the tree of the folder and what it asks for.

use super::*;

/// The sidebar on the panels' surface, as in Zed.
pub(super) fn sidebar_fill(v: &egui::Visuals) -> Color32 {
    v.panel_fill
}

impl App {
    /// Thin strip at the left edge while the sidebar is hidden, the room the editor's card has on
    /// its right (no more): lit under the pointer, a click brings the sidebar back.
    pub(in crate::app) fn sidebar_strip(&mut self, ui: &mut egui::Ui) {
        let bar = self.hl.tab_colors().bar;
        egui::Panel::left("sidebar_strip").exact_size(6.0).resizable(false).show_separator_line(false).frame(egui::Frame::NONE.fill(bar)).show(ui, |ui| {
            let rect = ui.max_rect();
            let r = ui.interact(rect, egui::Id::unique("sidebar_strip_btn"), Sense::click());
            if r.hovered() {
                ui.painter().rect_filled(rect.shrink2(egui::vec2(1.5, 0.0)), 2.0, self.hl.colors.accent.gamma_multiply(0.6));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if r.on_hover_text(t!("Afficher la barre latérale (Ctrl+B)", "Show the sidebar (Ctrl+B)")).clicked() {
                self.show_sidebar = true;
            }
        });
    }

    pub(in crate::app) fn sidebar(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        let mut tree_action = None;
        let panel = sidebar_fill(&ctx.global_style().visuals);
        let tc = self.hl.tab_colors();
        // the width fitted at the last frame (double click on its border), kept by egui from then on
        let fit_to = ctx.data_mut(|d| d.remove_temp::<f32>(egui::Id::unique("sidebar_fit")));
        let shown = egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(260.0)
            .size_range(fit_to.map_or(150.0..=700.0, |w| w..=w))
            // flat, a shade apart from the editor, a thin line between them
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(panel))
            .show(ui, |ui| {
            let edge = ui.max_rect();
            ui.painter().vline(edge.right() - 0.5, edge.y_range(), egui::Stroke::new(1.0_f32, tc.border));
            // at the top, the tab strip goes on over the sidebar: the window's buttons (macOS), the F
            // of the menus, the folder and the name of the project
            let projects: Vec<(String, PathBuf)> = self.projects.iter().filter(|p| crate::stat_cache::is_dir(&p.path)).map(|p| (p.name.clone(), p.path.clone())).collect();
            egui::Panel::top("sidebar_top").exact_size(TAB_STRIP_H).show_separator_line(false).frame(egui::Frame::NONE.fill(tc.bar)).show(ui, |ui| {
                let bar = ui.max_rect();
                ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, egui::Stroke::new(1.0_f32, tc.border));
                ui.visuals_mut().panel_fill = tc.bar;
                let mut row = bar;
                if let Some(c) = self.top_controls(true) {
                    // the empty part moves the window, as a title bar does
                    let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                    let drag = ui.interact(bar, egui::Id::unique("sidebar_top_drag"), Sense::click_and_drag());
                    crate::app::title_bar::drag_window(ui.ctx(), &drag, maximized);
                    crate::app::title_bar::strip_buttons(ui, bar, c, maximized);
                    row.min.x += crate::app::title_bar::MAC_BUTTONS_W;
                }
                row.min.x += 6.0;
                row.max.x -= 4.0;
                ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
                    ui.horizontal_centered(|ui| {
                        self.menu_logo(ui);
                        self.version_and_update(ui);
                    });
                });
            });
            let card = egui::Frame::NONE.inner_margin(egui::Margin { left: 4, right: 4, top: 4, bottom: 4 });
            card.show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            // what is drawn inside (sticky folders, icons) on the same background
            ui.visuals_mut().panel_fill = panel;
            let pats = self.settings.exclude.clone();
            // only the tree: the views are reached from the status bar, the bottom panel's ▾ and the palette
            match self.tree.as_mut() {
                Some(tree) => {
                    // the name of the project above the tree, staying there while it scrolls
                    let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::hover());
                    ui.scope_builder(egui::UiBuilder::new().max_rect(row.shrink2(egui::vec2(6.0, 0.0))), |ui| {
                        ui.horizontal_centered(|ui| {
                            if let Some(a) = sidebar::header(ui, tree, &projects) {
                                tree_action = Some(a);
                            }
                        });
                    });
                    let active = self.docs.get(self.active).and_then(|d| d.path.clone());
                    let empty = HashMap::new();
                    let info = if self.settings.tree_git_info { &self.repo_info } else { &empty };
                    if !self.tree_ex.as_ref().is_some_and(|(r, p, _)| *r == tree.path && *p == pats) {
                        let ex = crate::walk::Exclusions::new(&tree.path, &pats);
                        self.tree_ex = Some((tree.path.clone(), pats, ex));
                    }
                    let ex = &self.tree_ex.as_ref().expect("just set").2;
                    if let Some(a) = sidebar::show(ui, tree, ex, active.as_deref(), &self.git.map, info, &mut self.tree_scroll, &mut self.tree_nav) {
                        tree_action = Some(a);
                    }
                }
                None => {
                    ui.label(t!("Aucun dossier ouvert.", "No folder open."));
                    if ui.button(t!("Ouvrir un dossier…", "Open a folder…")).clicked() {
                        self.pending.push(Cmd::OpenFolder);
                    }
                }
            }
            });
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
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sidebar_fit"), width));
                ctx.request_repaint();
            }
        }
        if let Some(a) = tree_action {
            self.tree_action(ctx, a);
        }
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
            TreeAction::CopyPath(p) => ctx.copy_text(p.display().to_string()),
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
