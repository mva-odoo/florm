//! The bottom panel: its tabs, its views, and moving a view between it and an editor tab.

use super::*;

impl App {
    /// Right click on a view (button of the sidebar, tab of the bottom panel): in the bottom panel,
    /// taken out of the editor area if it was there; `toggle`: a click on the one shown hides it.
    pub(in crate::app) fn show_bottom(&mut self, tab: BottomTab, toggle: bool) {
        if toggle && self.bottom == Some(tab) {
            self.bottom = None;
            return;
        }
        let in_editor = |v: document::View| move |d: &Document| d.view == v;
        let tab_of = match tab {
            BottomTab::Search => self.docs.iter().position(in_editor(document::View::Search)),
            // each Claude tab is a conversation of its own: it stays
            BottomTab::Pulls => self.docs.iter().position(in_editor(document::View::PullRequests)),
            BottomTab::Docker => self.docs.iter().position(in_editor(document::View::Docker)),
            BottomTab::Debug => self.docs.iter().position(in_editor(document::View::Debug)),
            BottomTab::Git => self.docs.iter().position(in_editor(document::View::Git)),
            _ => None,
        };
        if let Some(i) = tab_of {
            self.close_tab(i, true);
        }
        self.bottom = Some(tab);
        match tab {
            BottomTab::Search => self.search.focus = true,
            BottomTab::Terminal => self.term.want_focus = true,
            BottomTab::Claude => self.claude_prepare(),
            BottomTab::Git => self.git_view_load(),
            BottomTab::Pulls => self.pulls_load(),
            BottomTab::Docker => self.docker_load(),
            BottomTab::Debug => self.debug.focus_console = true,
            BottomTab::Problems | BottomTab::Output => {}
        }
    }

    /// Left click on a view: in the editor area, with all the room (problems and output only
    /// exist in the bottom panel).
    pub(in crate::app) fn show_big(&mut self, tab: BottomTab) {
        match tab {
            BottomTab::Search => self.open_search_tab(),
            BottomTab::Terminal => self.open_terminal_tab(),
            BottomTab::Claude => {
                if self.bottom == Some(BottomTab::Claude) {
                    self.bottom = None;
                }
                self.move_claude(true);
            }
            BottomTab::Git => self.open_git_view(),
            BottomTab::Pulls => self.open_pulls(),
            BottomTab::Docker => self.open_docker(),
            BottomTab::Debug => self.open_debug(),
            BottomTab::Problems | BottomTab::Output => self.show_bottom(tab, false),
        }
    }

    /// The view of `tab` is shown in an editor tab (the bottom panel then only says so).
    pub(super) fn view_in_tab(&self, tab: BottomTab) -> bool {
        let tab_of = |v: document::View| self.docs.iter().any(|d| d.view == v);
        match tab {
            BottomTab::Search => tab_of(document::View::Search),
            BottomTab::Claude => !self.claude_has_bottom(),
            BottomTab::Git => tab_of(document::View::Git),
            BottomTab::Pulls => tab_of(document::View::PullRequests),
            BottomTab::Docker => tab_of(document::View::Docker),
            BottomTab::Debug => tab_of(document::View::Debug),
            BottomTab::Terminal | BottomTab::Problems | BottomTab::Output => false,
        }
    }

    /// The view of `tab` back from its editor tab into the bottom panel.
    pub(super) fn bring_back(&mut self, tab: BottomTab) {
        match tab {
            BottomTab::Search => {
                if let Some(i) = self.docs.iter().position(|d| d.view == document::View::Search) {
                    self.close_tab(i, true);
                }
                self.settings.search_in_tab = false;
                self.persist("search_in_tab", "false".into());
            }
            BottomTab::Claude => self.move_claude(false),
            _ => self.show_bottom(tab, false),
        }
    }

    pub(in crate::app) fn bottom_panel(&mut self, ctx: &egui::Context) {
        if let Some(t) = self.bottom {
            self.last_bottom = t;
        }
        // Claude, git and the pull requests need room for their bars and content
        let min_h = if matches!(self.bottom, Some(BottomTab::Claude | BottomTab::Git | BottomTab::Pulls | BottomTab::Docker | BottomTab::Debug)) { 360.0 } else { 80.0 };
        egui::TopBottomPanel::bottom("bottom").resizable(true).default_height(230.0).height_range(min_h..=900.0).show(
            ctx,
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let nd: usize = self.docs.iter().map(|d| d.diagnostics.len()).sum();
                    let problems = if nd > 0 { tf!("Problèmes {nd}", "Problems {nd}") } else { t!("Problèmes", "Problems").to_string() };
                    for (tab, icon, label) in BottomTab::shown() {
                        let label = if tab == BottomTab::Problems { problems.clone() } else { label.to_string() };
                        let r = nav_tab(ui, icon, &label, self.bottom == Some(tab));
                        if tab == BottomTab::Claude && self.bottom != Some(tab) && self.claude_bottom_done() {
                            ui.painter().rect_stroke(r.rect, 4.0, egui::Stroke::new(1.5, ok()));
                        }
                        if r.clicked() && !tab.bottom_only() {
                            self.show_big(tab);
                        } else if r.clicked() || r.secondary_clicked() {
                            self.show_bottom(tab, false);
                        }
                        r.on_hover_text(BottomTab::hint(tab));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").clicked() {
                            self.bottom = None;
                        }
                    });
                });
                ui.separator();
                match self.bottom {
                    // already shown in a tab: not twice
                    // shown in an editor tab: not twice, the same notice for all
                    Some(tab) if self.view_in_tab(tab) => {
                        if crate::view_bar::in_tab_notice(ui) {
                            self.bring_back(tab);
                        }
                    }
                    Some(BottomTab::Claude) => self.claude_show(ui, None),
                    Some(BottomTab::Git) => {
                        let fill = ui.visuals().panel_fill;
                        egui::TopBottomPanel::top("git_bar_bottom")
                            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 4.0, right: 4.0, top: 2.0, bottom: 4.0 }))
                            .show_inside(ui, |ui| self.git_bar(ui));
                        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.history_ui(ui));
                    }
                    Some(BottomTab::Pulls) => {
                        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.pulls_ui(ui));
                    }
                    Some(BottomTab::Docker) => {
                        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.docker_ui(ui));
                    }
                    Some(BottomTab::Debug) => {
                        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.debug_ui(ui));
                    }
                    Some(BottomTab::Search) => self.search_ui(ui, false),
                    Some(BottomTab::Problems) => self.problems_ui(ui),
                    Some(BottomTab::Terminal) => {
                        if self.term.cwd.is_none() {
                            self.term.cwd = self.root.clone();
                        }
                        let shell = self.term_shell();
                        self.term.ui(ui, shell.as_deref(), self.hl.colors, self.hl.is_light());
                    }
                    Some(BottomTab::Output) => {
                        if ui.small_button(t!("Effacer", "Clear")).clicked() {
                            self.output.clear();
                        }
                        egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                            for l in &self.output {
                                ui.add(egui::Label::new(RichText::new(l).monospace()).extend());
                            }
                        });
                    }
                    None => {}
                }
            },
        );
    }

    pub(super) fn problems_ui(&mut self, ui: &mut egui::Ui) {
        let mut goto = None;
        if let Some(linter) = self.python_linter.clone() {
            ui.horizontal(|ui| {
                if linter == "syntaxe" {
                    ui.label(
                        RichText::new(t!("Python : aucun linter installé, seules les erreurs de syntaxe sont détectées.", "Python: no linter installed, only syntax errors are detected.")).color(warn()),
                    );
                    if ui.button(t!("Installer flake8", "Install flake8")).on_hover_text(format!("pip install --target {}", lint::tools_dir().display())).clicked() {
                        self.pending.push(Cmd::InstallFlake8);
                    }
                } else {
                    ui.label(RichText::new(format!("Python : {linter}")).weak().small());
                }
            });
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut any = false;
            for (i, d) in self.docs.iter().enumerate() {
                if d.diagnostics.is_empty() {
                    continue;
                }
                any = true;
                ui.label(RichText::new(d.title()).strong());
                for g in &d.diagnostics {
                    let (icon, col) = if g.severity == Severity::Error { ("✖", err()) } else { ("⚠", warn()) };
                    let text = format!("  {icon} {}:{}  {}  [{}]", g.line + 1, g.col + 1, g.message, g.source);
                    let r = ui.add(egui::Label::new(RichText::new(text).color(col).monospace()).sense(Sense::click()));
                    if r.clicked() {
                        goto = Some((i, g.line, g.col));
                    }
                }
            }
            if !any {
                ui.label(RichText::new(t!("Aucun problème détecté (le lint Python/JS s'exécute à l'enregistrement).", "No problem detected (Python/JS lint runs on save).")).weak());
            }
        });
        if let Some((i, l, c)) = goto {
            self.active = i;
            self.docs[i].goto_line(l, c);
            ui.ctx().memory_mut(|m| m.request_focus(editor_id()));
        }
    }
}
