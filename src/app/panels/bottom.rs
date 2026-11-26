//! The bottom panel: its views in a row of tabs (each terminal, the debugger, the problems, the
//! output), + for a new terminal. The search is not here: it is a page of its own (an editor tab).

use super::*;

impl App {
    /// `tab` in the bottom panel; `toggle`: hidden when it is the one shown.
    pub(in crate::app) fn show_bottom(&mut self, tab: BottomTab, toggle: bool) {
        if toggle && self.bottom == Some(tab) {
            self.bottom = None;
            return;
        }
        self.bottom = Some(tab);
        if tab == BottomTab::Terminal {
            self.term.want_focus = true;
        }
    }

    /// Opens `tab`: the shortcuts, the palette, the status bar and the menus all go through here.
    pub(in crate::app) fn open_tool(&mut self, tab: BottomTab) {
        self.show_bottom(tab, false);
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

    pub(in crate::app) fn bottom_panel(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        if let Some(t) = self.bottom {
            self.last_bottom = t;
        }
        let min_h = 80.0;
        // room on the sides and under its content, so that it does not touch the status bar
        let frame = egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(egui::Margin { left: 8, right: 8, top: 2, bottom: 8 });
        egui::Panel::bottom("bottom").resizable(true).default_size(230.0).size_range(min_h..=900.0).frame(frame).show(
            ui,
            |ui| {
                // its views in a row of tabs: each terminal, the debugger, the problems, the output; + a terminal
                let cur = self.bottom.unwrap_or(self.last_bottom);
                let nd: usize = self.docs.iter().map(|d| d.diagnostics.len()).sum();
                let mut pick = None;
                let mut close_term = None;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let sessions = self.term.panel_sessions();
                    if sessions.is_empty() {
                        if bottom_tab(ui, icons::ViewIcon::Terminal, "Terminal", None, cur == BottomTab::Terminal, false).0 {
                            pick = Some(Pick::View(BottomTab::Terminal));
                        }
                    }
                    for (id, title, shown, ended) in &sessions {
                        let note = ended.then(|| (t!("terminé", "ended").to_string(), ui.visuals().weak_text_color()));
                        let (open, close, _) = bottom_tab(ui, icons::ViewIcon::Terminal, title, note, cur == BottomTab::Terminal && *shown, true);
                        if close {
                            close_term = Some(*id);
                        } else if open {
                            pick = Some(Pick::Term(*id));
                        }
                    }
                    let debug_note = match self.debug_status() {
                        Some(true) => Some(("⏸".to_string(), warn())),
                        Some(false) => Some(("●".to_string(), ok())),
                        None => None,
                    };
                    for (tab, icon, label, note) in [
                        (BottomTab::Debug, icons::ViewIcon::Debug, t!("Débogueur", "Debugger"), debug_note),
                        (BottomTab::Problems, icons::ViewIcon::Problems, t!("Problèmes", "Problems"), (nd > 0).then(|| (nd.to_string(), warn()))),
                        (BottomTab::Output, icons::ViewIcon::Output, t!("Sortie", "Output"), None),
                    ] {
                        let keys = self.keymap.label(tab.cmd());
                        let r = bottom_tab(ui, icon, label, note, cur == tab, false);
                        if r.0 {
                            pick = Some(Pick::View(tab));
                        }
                        if !keys.is_empty() {
                            r.2.on_hover_text(format!("{label} ({keys})"));
                        }
                    }
                    ui.add_space(4.0);
                    if ui.small_button("+").on_hover_text(t!("Nouveau terminal", "New terminal")).clicked() {
                        pick = Some(Pick::NewTerm);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let keys = self.keymap.label(Cmd::TogglePanel);
                        if ui.small_button("×").on_hover_text(tf!("Fermer le panneau ({keys})", "Close the panel ({keys})")).clicked() {
                            self.bottom = None;
                        }
                        let term = cur == BottomTab::Terminal && !sessions.is_empty();
                        if term && icons::layout_button(ui, icons::Layout::ToTab, t!("Ce terminal dans un onglet de l'éditeur", "This terminal in an editor tab")).clicked() {
                            self.term.dock_shown();
                        }
                        // the terminal's folder, on the right of the tabs
                        if let Some(cwd) = self.term.shown_cwd().filter(|_| term) {
                            ui.label(RichText::new(cwd.display().to_string()).small().weak())
                                .on_hover_text(t!("Ctrl+C : interrompre (ou copier la sélection) · Ctrl+V : coller · clic droit : menu", "Ctrl+C: interrupt (or copy the selection) · Ctrl+V: paste · right click: menu"));
                        }
                    });
                });
                if let Some(id) = close_term {
                    self.term.close(id);
                }
                if let Some(p) = pick {
                    self.apply_pick(p, true);
                }
                ui.separator();
                match self.bottom {
                    Some(BottomTab::Problems) => self.problems_ui(ui),
                    Some(BottomTab::Debug) => self.debug_panel(ui),
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

/// What the ▾ on the right of the tab strip chose: a command opening its tab.
pub(super) enum NewTab {
    Cmd(Cmd),
}

impl App {
    /// The ▾ on the right of the tab strip: the search, a terminal, the preferences and the welcome
    /// page, each in an editor tab.
    pub(super) fn new_tab_menu(&self, ui: &mut egui::Ui) -> Option<NewTab> {
        ui.set_min_width(260.0);
        let mut chosen = None;
        ui.label(egui::RichText::new(t!("Ouvrir dans un onglet", "Open in a tab")).small().weak());
        for (cmd, icon, label) in [
            (Cmd::FindInFilesTab, icons::ViewIcon::Search, t!("Recherche", "Search")),
            (Cmd::NewTerminalTab, icons::ViewIcon::Terminal, t!("Terminal", "Terminal")),
            (Cmd::OpenSettings, icons::ViewIcon::Settings, t!("Préférences", "Preferences")),
            (Cmd::ShowWelcome, icons::ViewIcon::Welcome, t!("Accueil", "Welcome")),
        ] {
            if tool_row(ui, icon, label, None, &self.keymap.label(cmd), false).clicked() {
                chosen = Some(NewTab::Cmd(cmd));
                ui.close();
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
}

/// A tab of the bottom panel: its icon, its name, a note in color after it (a count, a state),
/// × on hover when `closable`; the one shown on the editor's background, underlined in the
/// accent. Returns (clicked, × clicked, its response).
fn bottom_tab(ui: &mut egui::Ui, icon: icons::ViewIcon, label: &str, note: Option<(String, Color32)>, shown: bool, closable: bool) -> (bool, bool, egui::Response) {
    let font = egui::FontId::proportional(12.5);
    let text_w = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x).min(180.0);
    let note_w = note.as_ref().map_or(0.0, |(n, _)| ui.fonts_mut(|f| f.layout_no_wrap(n.clone(), font.clone(), Color32::WHITE).size().x) + 6.0);
    let w = 10.0 + 16.0 + 6.0 + text_w + note_w + if closable { 20.0 } else { 10.0 };
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 26.0), Sense::click());
    let x_rect = egui::Rect::from_center_size(egui::pos2(rect.right() - 12.0, rect.center().y), egui::vec2(16.0, 16.0));
    let x = closable.then(|| ui.interact(x_rect, r.id.with("close"), Sense::click()));
    let v = ui.visuals();
    let hovered = ui.rect_contains_pointer(rect);
    let accent = v.selection.stroke.color;
    if shown {
        ui.painter().rect_filled(rect, egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, v.extreme_bg_color);
        ui.painter().hline(rect.x_range(), rect.bottom() - 1.0, egui::Stroke::new(2.0_f32, accent));
    } else if hovered {
        ui.painter().rect_filled(rect, 5.0, v.widgets.hovered.weak_bg_fill);
    }
    let color = if shown || hovered { v.strong_text_color() } else { v.text_color() };
    let y = rect.center().y;
    icons::paint_view(ui.painter(), egui::pos2(rect.left() + 10.0 + 8.0, y), icon, color);
    let clip = ui.painter().with_clip_rect(egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + 32.0 + text_w, rect.bottom())));
    let name = clip.text(egui::pos2(rect.left() + 32.0, y), Align2::LEFT_CENTER, label, font.clone(), color);
    if let Some((n, c)) = &note {
        ui.painter().text(egui::pos2(name.right().min(rect.left() + 32.0 + text_w) + 6.0, y), Align2::LEFT_CENTER, n, font, *c);
    }
    let mut close = false;
    if let Some(x) = x {
        if hovered {
            let xc = if x.hovered() { v.strong_text_color() } else { v.weak_text_color() };
            ui.painter().text(x_rect.center(), Align2::CENTER_CENTER, "×", egui::FontId::proportional(14.0), xc);
        }
        close = x.clicked();
    }
    let r = if text_w >= 180.0 { r.on_hover_text(label) } else { r };
    (r.clicked() && !close, close, r)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The bottom panel shows its views as tabs (no search: it is a page), + for a terminal; a
    /// click on a tab shows it.
    #[test]
    fn the_bottom_panel_has_tabs() {
        let dir = std::env::temp_dir().join(format!("florm_bottom_tabs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, None);
        app.bottom = Some(BottomTab::Output);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 700.0));
        let frame = |app: &mut App, events: Vec<egui::Event>| {
            let out = crate::run_ui(&ctx, egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| app.bottom_panel(ui));
            let mut texts = Vec::new();
            fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
                match s {
                    egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()))),
                    egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                    _ => {}
                }
            }
            out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
            texts
        };
        frame(&mut app, Vec::new());
        let texts = frame(&mut app, Vec::new());
        let names: Vec<&str> = texts.iter().map(|(t, _)| t.as_str()).collect();
        for want in ["Terminal", "Débogueur", "Problèmes", "Sortie", "+", "Effacer"] {
            assert!(names.contains(&want), "{want}: {names:?}");
        }
        assert!(!names.contains(&"Recherche"), "{names:?}");
        // a click on « Problèmes »
        let at = texts.iter().find(|(t, _)| t == "Problèmes").unwrap().1.center();
        let click = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&mut app, vec![egui::Event::PointerMoved(at), click(true)]);
        frame(&mut app, vec![click(false)]);
        assert_eq!(app.bottom, Some(BottomTab::Problems));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
