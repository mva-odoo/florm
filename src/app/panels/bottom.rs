//! The bottom panel: one view at a time (▾ for the others), where each view opens (its editor tab
//! or here, as ⇱ / ⇲ left it), and moving a view between the two.

use super::*;

impl App {
    /// `tab` in the bottom panel, taken out of the editor area if it was there; `toggle`: hidden
    /// when it is the one shown.
    pub(in crate::app) fn show_bottom(&mut self, tab: BottomTab, toggle: bool) {
        if toggle && self.bottom == Some(tab) {
            self.bottom = None;
            return;
        }
        let in_editor = |v: document::View| move |d: &Document| d.view == v;
        let tab_of = match tab {
            BottomTab::Search => self.docs.iter().position(in_editor(document::View::Search)),
            _ => None,
        };
        if let Some(i) = tab_of {
            self.close_tab(i, true);
        }
        self.bottom = Some(tab);
        match tab {
            BottomTab::Search => self.search.focus = true,
            BottomTab::Terminal => self.term.want_focus = true,
            BottomTab::Problems | BottomTab::Output => {}
        }
    }

    /// `tab` in the editor area, with all the room (problems and output only exist in the bottom panel).
    pub(in crate::app) fn show_big(&mut self, tab: BottomTab) {
        match tab {
            BottomTab::Search => self.open_search_tab(),
            BottomTab::Terminal => self.open_terminal_tab(),
            BottomTab::Problems | BottomTab::Output => self.show_bottom(tab, false),
        }
    }

    /// The view of `tab` is shown in an editor tab (the bottom panel then only says so).
    pub(super) fn view_in_tab(&self, tab: BottomTab) -> bool {
        let tab_of = |v: document::View| self.docs.iter().any(|d| d.view == v);
        match tab {
            BottomTab::Search => tab_of(document::View::Search),
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
            _ => self.show_bottom(tab, false),
        }
    }

    /// `tab` opens in an editor tab rather than in the bottom panel (its last ⇱ / ⇲).
    fn prefers_tab(&self, tab: BottomTab) -> bool {
        tab == BottomTab::Search && self.settings.search_in_tab
    }

    /// Opens `tab` in its one place: its editor tab when it has one, else where ⇱ / ⇲ put it last.
    /// The shortcuts, the palette, the status bar and the ▾ menus all go through here.
    pub(in crate::app) fn open_tool(&mut self, tab: BottomTab) {
        match tab {
            _ if self.view_in_tab(tab) || self.prefers_tab(tab) => self.show_big(tab),
            _ => self.show_bottom(tab, false),
        }
    }

    /// The views, one row each (icon, name, what is waiting, shortcut), and the terminals of the
    /// panel under « Terminal »: the ▾ of the bottom panel and the right click on its button in the status bar.
    pub(super) fn tools_menu(&self, ui: &mut egui::Ui, current: Option<BottomTab>) -> Option<Pick> {
        ui.visuals_mut().override_text_color = None;
        ui.set_min_width(270.0);
        let nd: usize = self.docs.iter().map(|d| d.diagnostics.len()).sum();
        let mut chosen = None;
        for (tab, icon, label) in BottomTab::shown() {
            let note = match tab {
                BottomTab::Problems if nd > 0 => Some((nd.to_string(), warn())),
                _ => None,
            };
            if tool_row(ui, icon, label, note, &self.keymap.label(tab.cmd()), current == Some(tab)).clicked() {
                chosen = Some(Pick::View(tab));
                ui.close_menu();
            }
            if tab == BottomTab::Terminal {
                for (id, title, shown, ended) in self.term.panel_sessions() {
                    let note = ended.then(|| (t!("terminé", "ended"), ui.visuals().weak_text_color()));
                    let (open, close) = sub_row(ui, &title, note, shown && current == Some(tab), true, true);
                    if close {
                        chosen = Some(Pick::CloseTerm(id));
                    } else if open {
                        chosen = Some(Pick::Term(id));
                        ui.close_menu();
                    }
                }
                if sub_row(ui, t!("+ Nouveau terminal", "+ New terminal"), None, false, false, true).0 {
                    chosen = Some(Pick::NewTerm);
                    ui.close_menu();
                }
            }
        }
        chosen
    }

    /// What the views menu chose; `here`: from the bottom panel's ▾ (the view comes into it).
    pub(super) fn apply_pick(&mut self, pick: Pick, here: bool) {
        match pick {
            Pick::View(tab) if here => self.show_bottom(tab, false),
            Pick::View(tab) => self.open_tool(tab),
            Pick::Term(id) => {
                self.bottom = Some(BottomTab::Terminal);
                self.term.show(id);
            }
            Pick::NewTerm => self.new_panel_terminal(),
            Pick::CloseTerm(id) => self.term.close(id),
        }
    }

    /// A new terminal in the bottom panel, shown.
    fn new_panel_terminal(&mut self) {
        if self.term.cwd.is_none() {
            self.term.cwd = self.root.clone();
        }
        let shell = self.term_shell();
        let ctx = self.egui_ctx.clone();
        self.term.open(&ctx, shell.as_deref(), None);
        self.bottom = Some(BottomTab::Terminal);
    }

    pub(in crate::app) fn bottom_panel(&mut self, ctx: &egui::Context) {
        if let Some(t) = self.bottom {
            self.last_bottom = t;
        }
        let min_h = 80.0;
        // room on the sides and under its content, so that it does not touch the status bar
        let frame = egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin { left: 8.0, right: 8.0, top: 2.0, bottom: 8.0 });
        egui::TopBottomPanel::bottom("bottom").resizable(true).default_height(230.0).height_range(min_h..=900.0).frame(frame).show(
            ctx,
            |ui| {
                // one view at a time, no row of tabs: its name, ▾ for the others
                ui.horizontal(|ui| {
                    let cur = self.bottom.unwrap_or(self.last_bottom);
                    let (_, icon, label) = BottomTab::shown().find(|(t, ..)| *t == cur).unwrap_or(BottomTab::ALL[0]);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 20.0), Sense::hover());
                    icons::paint_view(ui.painter(), rect.center(), icon, ui.visuals().strong_text_color());
                    let mut chosen = None;
                    let menu = ui.menu_button(RichText::new(format!("{label}  ▾")).strong(), |ui| chosen = self.tools_menu(ui, Some(cur)));
                    menu.response.on_hover_text(t!("Changer de vue (une seule à la fois dans le panneau du bas)", "Switch view (one at a time in the bottom panel)"));
                    let term = self.bottom == Some(BottomTab::Terminal);
                    // the terminal's folder, where its row of tabs was
                    if let Some(cwd) = self.term.shown_cwd().filter(|_| term) {
                        ui.label(RichText::new(cwd.display().to_string()).small().weak())
                            .on_hover_text(t!("Ctrl+C : interrompre (ou copier la sélection) · Ctrl+V : coller · clic droit : menu\nLes autres terminaux sont dans ▾", "Ctrl+C: interrupt (or copy the selection) · Ctrl+V: paste · right click: menu\nThe other terminals are in ▾"));
                    }
                    if let Some(p) = chosen {
                        self.apply_pick(p, true);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let keys = self.keymap.label(Cmd::TogglePanel);
                        if ui.small_button("×").on_hover_text(tf!("Fermer le panneau ({keys})", "Close the panel ({keys})")).clicked() {
                            self.bottom = None;
                        }
                        if term {
                            if icons::layout_button(ui, icons::Layout::ToTab, t!("Ce terminal dans un onglet de l'éditeur", "This terminal in an editor tab")).clicked() {
                                self.term.dock_shown();
                            }
                            if ui.small_button("+").on_hover_text(t!("Nouveau terminal", "New terminal")).clicked() {
                                self.new_panel_terminal();
                            }
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
                        // only the lines in sight are laid out (up to 5000 kept)
                        let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, row_h, self.output.len(), |ui, range| {
                            for l in &self.output[range] {
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

/// What the ▾ on the right of the tab strip chose: a view in an editor tab, or a command.
pub(super) enum NewTab {
    View(BottomTab),
    Cmd(Cmd),
}

impl App {
    /// The ▾ on the right of the tab strip: each view, with its icon, to open in an editor tab
    /// (the terminal, the problems and the output stay in the bottom panel), then the preferences
    /// and the welcome page.
    pub(super) fn new_tab_menu(&self, ui: &mut egui::Ui) -> Option<NewTab> {
        ui.set_min_width(260.0);
        let mut chosen = None;
        ui.label(egui::RichText::new(t!("Ouvrir dans un onglet", "Open in a tab")).small().weak());
        for (tab, icon, label) in BottomTab::shown().filter(|(t, ..)| !matches!(t, BottomTab::Problems | BottomTab::Output | BottomTab::Terminal)) {
            if tool_row(ui, icon, label, None, &self.keymap.label(tab.cmd()), false).clicked() {
                chosen = Some(NewTab::View(tab));
                ui.close_menu();
            }
        }
        ui.separator();
        for (cmd, icon, label) in [
            (Cmd::NewTerminalTab, icons::ViewIcon::Terminal, t!("Terminal", "Terminal")),
            (Cmd::OpenSettings, icons::ViewIcon::Settings, t!("Préférences", "Preferences")),
            (Cmd::ShowWelcome, icons::ViewIcon::Welcome, t!("Accueil", "Welcome")),
        ] {
            if tool_row(ui, icon, label, None, &self.keymap.label(cmd), false).clicked() {
                chosen = Some(NewTab::Cmd(cmd));
                ui.close_menu();
            }
        }
        chosen
    }
}

/// What the views menu chose.
pub(super) enum Pick {
    View(BottomTab),
    /// A terminal of the bottom panel, shown there.
    Term(u64),
    NewTerm,
    CloseTerm(u64),
}

/// A row under a view in the views menu (a terminal, a conversation): its title, a note in
/// color after it, × on hover when `closable`. Returns (open, close).
fn sub_row(ui: &mut egui::Ui, title: &str, note: Option<(&str, Color32)>, shown: bool, closable: bool, mono: bool) -> (bool, bool) {
    let w = ui.available_width().max(260.0);
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 22.0), Sense::click());
    let x_rect = egui::Rect::from_center_size(egui::pos2(rect.right() - 14.0, rect.center().y), egui::vec2(18.0, 18.0));
    let x = ui.interact(x_rect, r.id.with("close"), Sense::click());
    let v = ui.visuals();
    let hovered = r.hovered() || x.hovered();
    if hovered || shown {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let c = if hovered || shown { v.strong_text_color() } else { v.text_color() };
    let font = if mono { egui::FontId::monospace(12.0) } else { egui::FontId::proportional(12.5) };
    let y = rect.center().y;
    let name = ui.painter().text(egui::pos2(rect.left() + 28.0, y), Align2::LEFT_CENTER, title, font, c);
    if let Some((text, col)) = note {
        ui.painter().text(egui::pos2(name.right() + 8.0, y), Align2::LEFT_CENTER, text, egui::FontId::proportional(12.0), col);
    }
    if hovered && closable {
        let xc = if x.hovered() { v.strong_text_color() } else { v.weak_text_color() };
        ui.painter().text(x_rect.center(), Align2::CENTER_CENTER, "×", egui::FontId::proportional(14.0), xc);
    }
    let close = closable && x.clicked();
    (r.clicked() && !close, close)
}

/// A row of the views menu: icon, name, a note (count, ●) and the shortcut on the right.
fn tool_row(ui: &mut egui::Ui, icon: icons::ViewIcon, label: &str, note: Option<(String, Color32)>, shortcut: &str, current: bool) -> egui::Response {
    let w = ui.available_width().max(260.0);
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 24.0), Sense::click());
    let v = ui.visuals();
    if r.hovered() || current {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let c = if r.hovered() || current { v.strong_text_color() } else { v.text_color() };
    let y = rect.center().y;
    icons::paint_view(ui.painter(), egui::pos2(rect.left() + 12.0, y), icon, c);
    let font = egui::FontId::proportional(13.0);
    let name = ui.painter().text(egui::pos2(rect.left() + 28.0, y), Align2::LEFT_CENTER, label, font.clone(), c);
    if let Some((text, col)) = note {
        ui.painter().text(egui::pos2(name.right() + 8.0, y), Align2::LEFT_CENTER, text, font.clone(), col);
    }
    ui.painter().text(egui::pos2(rect.right() - 8.0, y), Align2::RIGHT_CENTER, shortcut, egui::FontId::proportional(12.0), v.weak_text_color());
    r
}
