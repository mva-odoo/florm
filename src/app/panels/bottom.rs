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
        if let Some(t) = self.bottom {
            self.last_bottom = t;
        }
        let min_h = 80.0;
        // as in Zed: a grey strip at the top (the terminals' tabs, or the name of the view), the
        // content on an off-white (the tab of the view at the right of the status bar too)
        let tc = self.hl.tab_colors();
        let paper = self.hl.bottom_paper();
        egui::Panel::bottom("bottom").resizable(true).default_size(230.0).size_range(min_h..=900.0).frame(egui::Frame::NONE.fill(paper)).show(
            ui,
            |ui| {
                let cur = self.bottom.unwrap_or(self.last_bottom);
                let mut pick = None;
                let mut close_term = None;
                let sessions = if cur == BottomTab::Terminal { self.term.panel_sessions() } else { Vec::new() };
                let (strip, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(strip, 0.0, tc.bar);
                p.hline(strip.x_range(), strip.top() + 0.5, egui::Stroke::new(1.0_f32, tc.border));
                p.hline(strip.x_range(), strip.bottom() - 0.5, egui::Stroke::new(1.0_f32, tc.border));
                ui.scope_builder(egui::UiBuilder::new().max_rect(strip), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if cur == BottomTab::Terminal {
                            if sessions.is_empty() && term_tab(ui, "Terminal", None, true, false, &tc, paper).0 {
                                pick = Some(Pick::View(BottomTab::Terminal));
                            }
                            for (id, title, shown, ended) in &sessions {
                                let note = ended.then(|| t!("terminé", "ended"));
                                let (open, close) = term_tab(ui, title, note, *shown, true, &tc, paper);
                                if close {
                                    close_term = Some(*id);
                                } else if open {
                                    pick = Some(Pick::Term(*id));
                                }
                            }
                        } else {
                            // the name of the view
                            let (icon, title, note) = match cur {
                                BottomTab::Debug => (icons::ViewIcon::Debug, t!("Débogueur", "Debugger"), match self.debug_status() {
                                    Some(true) => Some((t!("en pause", "paused").to_string(), warn())),
                                    Some(false) => Some((t!("en cours", "running").to_string(), ok())),
                                    None => None,
                                }),
                                BottomTab::Problems => {
                                    let nd: usize = self.docs.iter().map(|d| d.diagnostics.len()).sum();
                                    (icons::ViewIcon::Problems, t!("Problèmes", "Problems"), (nd > 0).then(|| (nd.to_string(), warn())))
                                }
                                _ => (icons::ViewIcon::Output, t!("Sortie", "Output"), None),
                            };
                            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 30.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.add_space(6.0);
                                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
                                icons::paint_view(ui.painter(), r.center(), icon, tc.text_active);
                                ui.label(RichText::new(title).size(12.5).color(tc.text_active));
                                if let Some((n, c)) = note {
                                    ui.label(RichText::new(n).size(12.0).color(c));
                                }
                            });
                        }
                        // on the right: + (a terminal), what goes with the view, × to close
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.add_space(6.0);
                            let keys = self.keymap.label(Cmd::TogglePanel);
                            if ui.small_button("×").on_hover_text(tf!("Fermer le panneau ({keys})", "Close the panel ({keys})")).clicked() {
                                self.bottom = None;
                            }
                            if cur == BottomTab::Terminal && ui.small_button("+").on_hover_text(t!("Nouveau terminal", "New terminal")).clicked() {
                                pick = Some(Pick::NewTerm);
                            }
                            let term = cur == BottomTab::Terminal && !sessions.is_empty();
                            if term && icons::layout_button(ui, icons::Layout::ToTab, t!("Ce terminal dans un onglet de l'éditeur", "This terminal in an editor tab")).clicked() {
                                self.term.dock_shown();
                            }
                            if let Some(cwd) = self.term.shown_cwd().filter(|_| term) {
                                ui.label(RichText::new(cwd.display().to_string()).small().weak())
                                    .on_hover_text(t!("Ctrl+C : interrompre (ou copier la sélection) · Ctrl+V : coller · clic droit : menu", "Ctrl+C: interrupt (or copy the selection) · Ctrl+V: paste · right click: menu"));
                            }
                            if cur == BottomTab::Output && ui.small_button(t!("Effacer", "Clear")).clicked() {
                                self.output.clear();
                            }
                        });
                    });
                });
                if let Some(id) = close_term {
                    self.term.close(id);
                }
                if let Some(p) = pick {
                    self.apply_pick(p, true);
                }
                let margin = egui::Margin { left: 10, right: 8, top: if cur == BottomTab::Terminal { 4 } else { 6 }, bottom: 0 };
                egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
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
                });
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

/// What the views menu chose.
pub(super) enum Pick {
    View(BottomTab),
    /// A terminal of the bottom panel, shown there.
    Term(u64),
    NewTerm,
}

/// A tab of a terminal, right on the terminals (as the tabs of the editor on the text): the one shown
/// on the white of the terminal, the others a step darker with the line under them; × on hover.
/// Returns (clicked, × clicked).
fn term_tab(ui: &mut egui::Ui, title: &str, note: Option<&str>, shown: bool, closable: bool, tc: &crate::highlight::TabColors, white: Color32) -> (bool, bool) {
    let font = egui::FontId::proportional(12.5);
    let color = if shown { tc.text_active } else { tc.text_inactive };
    let label = match note {
        Some(n) => format!("{title} · {n}"),
        None => title.to_string(),
    };
    let galley = ui.painter().layout_no_wrap(label, font, color);
    let text_w = galley.size().x.min(200.0);
    let w = (12.0 + 16.0 + 6.0 + text_w + if closable { 22.0 } else { 12.0 }).round();
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 30.0), Sense::click());
    let rect = egui::Rect::from_min_max(rect.min.round(), rect.max.round());
    let x_rect = egui::Rect::from_center_size(egui::pos2(rect.right() - 13.0, rect.center().y), egui::vec2(16.0, 16.0));
    let x = closable.then(|| ui.interact(x_rect, r.id.with("close"), Sense::click()));
    let hovered = ui.rect_contains_pointer(rect);
    let p = ui.painter();
    let fill = if shown { white } else if hovered { tc.close_hover } else { tc.bar };
    p.rect_filled(rect, 0.0, fill);
    let line = egui::Stroke::new(1.0_f32, tc.border);
    p.vline(rect.right() - 0.5, rect.y_range(), line);
    if shown {
        p.vline(rect.left() + 0.5, rect.y_range(), line);
    }
    if shown {
        // open onto the terminal: the line under the row stops at it
        p.hline(rect.shrink2(egui::vec2(1.0, 0.0)).x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0_f32, white));
    }
    icons::paint_view(p, egui::pos2(rect.left() + 12.0 + 8.0, rect.center().y), icons::ViewIcon::Terminal, color);
    p.with_clip_rect(egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + 34.0 + text_w, rect.bottom())))
        .galley(egui::pos2(rect.left() + 34.0, rect.center().y - galley.size().y / 2.0), galley, color);
    let mut close = false;
    if let Some(x) = x {
        if hovered {
            let xc = if x.hovered() { tc.text_active } else { tc.text_inactive };
            p.text(x_rect.center(), Align2::CENTER_CENTER, "×", egui::FontId::proportional(14.0), xc);
        }
        close = x.clicked();
    }
    (r.clicked() && !close, close)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bottom panel: a title line for the views (chosen at the right of the status bar, no tabs
    /// of theirs), the terminals' tabs and + for another one; the
    /// tabs of the status bar show a view, and hide it again.
    #[test]
    fn the_bottom_panel_shows_its_view() {
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
        let frame = |app: &mut App, status: bool, events: Vec<egui::Event>| {
            let out = crate::run_ui(&ctx, egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                if status {
                    app.status_bar(ui);
                } else {
                    app.bottom_panel(ui);
                }
            });
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
        frame(&mut app, false, Vec::new());
        let names: Vec<String> = frame(&mut app, false, Vec::new()).into_iter().map(|(t, _)| t).collect();
        // its title line: its name, its actions
        assert!(names.iter().any(|t| t == "Sortie") && names.iter().any(|t| t == "Effacer") && names.iter().any(|t| t == "×"), "{names:?}");
        for not in ["Terminal", "Débogueur", "Problèmes", "+"] {
            assert!(!names.iter().any(|t| t == not), "{not}: {names:?}");
        }
        // the terminal: + for another one
        app.bottom = Some(BottomTab::Terminal);
        frame(&mut app, false, Vec::new());
        let names: Vec<String> = frame(&mut app, false, Vec::new()).into_iter().map(|(t, _)| t).collect();
        assert!(names.iter().any(|t| t == "+"), "{names:?}");
        // the tab of the output at the right of the status bar (the rightmost): it hides it
        app.bottom = Some(BottomTab::Output);
        let at = egui::pos2(screen.right() - 8.0 - 17.0, screen.bottom() - 14.0);
        let click = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&mut app, true, Vec::new());
        frame(&mut app, true, vec![egui::Event::PointerMoved(at)]);
        frame(&mut app, true, vec![click(true)]);
        frame(&mut app, true, vec![click(false)]);
        assert_eq!(app.bottom, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
