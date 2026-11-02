//! The sidebar: the tree of the folder and what it asks for.

use super::*;

/// The sidebar a little darker than the rest, to set the tree apart from the editor.
pub(super) fn sidebar_fill(v: &egui::Visuals) -> Color32 {
    icons::mix(v.panel_fill, Color32::BLACK, if v.dark_mode { 0.15 } else { 0.04 })
}

impl App {
    /// Thin strip at the left edge while the sidebar is hidden, the room the editor's card has on
    /// its right (no more): lit under the pointer, a click brings the sidebar back.
    pub(in crate::app) fn sidebar_strip(&mut self, ctx: &egui::Context) {
        let bar = self.hl.tab_colors().bar;
        egui::SidePanel::left("sidebar_strip").exact_width(6.0).resizable(false).show_separator_line(false).frame(egui::Frame::none().fill(bar)).show(ctx, |ui| {
            let rect = ui.max_rect();
            let r = ui.interact(rect, egui::Id::new("sidebar_strip_btn"), Sense::click());
            if r.hovered() {
                ui.painter().rect_filled(rect.shrink2(egui::vec2(1.5, 0.0)), 2.0, self.hl.colors.accent.gamma_multiply(0.6));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if r.on_hover_text(t!("Afficher la barre latérale (Ctrl+K Ctrl+B)", "Show the sidebar (Ctrl+K Ctrl+B)")).clicked() {
                self.show_sidebar = true;
            }
        });
    }

    pub(in crate::app) fn sidebar(&mut self, ctx: &egui::Context) {
        let mut tree_action = None;
        let panel = sidebar_fill(&ctx.style().visuals);
        let tc = self.hl.tab_colors();
        let shown = egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(260.0)
            .width_range(150.0..=700.0)
            .show_separator_line(false)
            // the window's background around it (the color of the tab strip)…
            .frame(egui::Frame::none().fill(tc.bar).inner_margin(egui::Margin::same(6.0)))
            .show(ctx, |ui| {
            // …and the tree on a card of its own, rounded like the tabs, with a thin border: the
            // place of the tree is plain to see, as in VS Code
            let card = egui::Frame::none()
                .fill(panel)
                .stroke(egui::Stroke::new(1.0_f32, tc.text_inactive.gamma_multiply(0.25)))
                .rounding(CARD_ROUNDING)
                .inner_margin(egui::Margin { left: 6.0, right: 4.0, top: 4.0, bottom: 4.0 });
            card.show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            // what is drawn inside (sticky folders, icons) on the same background
            ui.visuals_mut().panel_fill = panel;
            self.sidebar_mac_buttons(ui);
            // only the tree: the views are reached from the status bar, the bottom panel's ▾ and the palette
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
