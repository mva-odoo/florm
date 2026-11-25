//! The « Débogueur » view of the bottom panel: what to run and the buttons (▶ ⏸ ↷ ↓ ↑ ⟳ ■), then
//! the call stack, the variables and the console side by side.

use super::*;

/// What was clicked in the panel, done once it is drawn.
enum Act {
    Cmd(Cmd),
    Choose(String),
    Frame(usize),
    Toggle(i64),
    Evaluate(String),
    Clear,
}

impl App {
    pub(in crate::app) fn debug_panel(&mut self, ui: &mut egui::Ui) {
        if self.debug.configs.is_empty() && self.debug.configs_err.is_none() && self.debug.phase == Phase::Idle {
            // read once when the panel first shows (and again at each F5)
            self.debug_read_configs();
            if self.debug.configs.is_empty() {
                self.debug.configs_err = Some(String::new());
            }
        }
        let mut act: Option<Act> = None;
        let accent = self.hl.colors.accent;
        let keys = |c: Cmd| self.keymap.label(c);
        // the field of the console, out of the state while it is drawn
        let mut input = std::mem::take(&mut self.debug.input);
        let d = &self.debug;
        // --- the bar: what to run, the buttons, where it is
        ui.horizontal(|ui| {
            let mut labels: Vec<String> = vec![t!("Fichier ouvert", "Open file").into(), t!("Test sous le curseur", "Test under the cursor").into()];
            labels.extend(d.configs.iter().map(|c| c.name.clone()));
            labels.push(t!("⚙ Modifier .florm/debug.toml…", "⚙ Edit .florm/debug.toml…").into());
            let current = match d.choice.as_str() {
                "" => 0,
                "test" => 1,
                n => d.configs.iter().position(|c| c.name == n).map_or(0, |k| k + 2),
            };
            let enabled = d.phase == Phase::Idle;
            ui.add_enabled_ui(enabled, |ui| {
                if let Some(k) = crate::pick::combo(ui, "debug_choice", RichText::new(&labels[current]), &labels, Some(current)) {
                    act = Some(match k {
                        0 => Act::Choose(String::new()),
                        1 => Act::Choose("test".into()),
                        k if k == labels.len() - 1 => Act::Cmd(Cmd::DebugConfigs),
                        k => Act::Choose(d.configs[k - 2].name.clone()),
                    });
                }
            })
            .response
            .on_hover_text(t!("Ce que F5 lance : le fichier ouvert, le test sous le curseur, ou une configuration du projet (.florm/debug.toml)", "What F5 starts: the open file, the test under the cursor, or a configuration of the project (.florm/debug.toml)"));
            let stopped = d.phase == Phase::Stopped;
            let running = d.phase == Phase::Running;
            let mut button = |ui: &mut egui::Ui, text: &str, on: bool, tip: String, cmd: Cmd| {
                if ui.add_enabled(on, egui::Button::new(text)).on_hover_text(tip).clicked() {
                    act = Some(Act::Cmd(cmd));
                }
            };
            let start_tip = if stopped { tf!("Continuer ({})", "Continue ({})", keys(Cmd::DebugStart)) } else { tf!("Lancer ({})", "Start ({})", keys(Cmd::DebugStart)) };
            button(ui, "▶", d.phase == Phase::Idle || stopped, start_tip, Cmd::DebugStart);
            button(ui, "⏸", running, t!("Mettre en pause", "Pause").into(), Cmd::DebugPause);
            button(ui, "↷", stopped, tf!("Ligne suivante ({})", "Next line ({})", keys(Cmd::DebugStepOver)), Cmd::DebugStepOver);
            button(ui, "↓", stopped, tf!("Entrer dans la fonction ({})", "Step into the function ({})", keys(Cmd::DebugStepInto)), Cmd::DebugStepInto);
            button(ui, "↑", stopped, tf!("Sortir de la fonction ({})", "Step out of the function ({})", keys(Cmd::DebugStepOut)), Cmd::DebugStepOut);
            button(ui, "⟳", d.cfg.is_some() && d.phase != Phase::Starting, tf!("Relancer ({})", "Restart ({})", keys(Cmd::DebugRestart)), Cmd::DebugRestart);
            button(ui, "■", d.session.is_some(), tf!("Arrêter ({})", "Stop ({})", keys(Cmd::DebugStop)), Cmd::DebugStop);
            let (text, color) = match d.phase {
                Phase::Idle => (t!("● point d'arrêt : clic dans la marge ou F9 ; F5 lance", "● breakpoint: click in the gutter or F9; F5 starts").to_string(), ui.visuals().weak_text_color()),
                Phase::Starting => (t!("démarrage…", "starting…").to_string(), ui.visuals().weak_text_color()),
                Phase::Running => (t!("en cours", "running").to_string(), ok()),
                Phase::Stopped => {
                    let why = match d.reason.as_str() {
                        "breakpoint" => t!("point d'arrêt", "breakpoint"),
                        "step" => t!("pas à pas", "step"),
                        "exception" => t!("exception", "exception"),
                        "pause" => t!("pause", "pause"),
                        "entry" => t!("début du programme", "start of the program"),
                        _ => "",
                    };
                    (if why.is_empty() { t!("⏸ en pause", "⏸ paused").to_string() } else { tf!("⏸ en pause : {why}", "⏸ paused: {why}") }, warn())
                }
            };
            ui.label(RichText::new(text).small().color(color));
            if let Some(s) = &d.session {
                ui.label(RichText::new(format!("· {}", s.name)).small().weak()).on_hover_text(t!("L'adaptateur de débogage de cette session", "The debug adapter of this session"));
            }
            if let Some(e) = d.configs_err.as_ref().filter(|e| !e.is_empty()) {
                ui.label(RichText::new(format!("⚠ {e}")).small().color(err()));
            }
        });
        ui.separator();
        // --- the stack on the left, the console on the right, the variables in the middle (each
        // side 40 % at most: the variables keep room in a narrow window)
        let id = ui.id();
        let side_max = (ui.available_width() * 0.4).max(100.0);
        egui::SidePanel::left(id.with("debug_stack")).resizable(true).default_width((ui.available_width() * 0.25).min(260.0)).width_range(90.0..=side_max).frame(egui::Frame::none().inner_margin(egui::Margin { right: 6.0, ..Default::default() })).show_inside(ui, |ui| {
            ui.label(RichText::new(t!("PILE D'APPELS", "CALL STACK")).small().strong().color(ui.visuals().weak_text_color()));
            egui::ScrollArea::vertical().id_salt("debug_stack_scroll").auto_shrink([false, false]).show(ui, |ui| {
                if d.frames.is_empty() {
                    ui.label(RichText::new(if d.phase == Phase::Running { t!("Le programme tourne.", "The program is running.") } else { t!("Vide tant que rien n'est arrêté.", "Empty until something is stopped.") }).weak());
                }
                for (k, f) in d.frames.iter().enumerate() {
                    let place = f.path.as_ref().map(|p| format!("{}:{}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), f.line + 1)).unwrap_or_default();
                    let text = RichText::new(format!("{}  {place}", f.name)).monospace().size(12.0);
                    let text = if f.path.is_none() { text.weak() } else { text };
                    let r = ui.add(egui::SelectableLabel::new(k == d.frame, text));
                    let r = match &f.path {
                        Some(p) => r.on_hover_text(format!("{}:{}", p.display(), f.line + 1)),
                        None => r,
                    };
                    if r.clicked() {
                        act = Some(Act::Frame(k));
                    }
                }
            });
        });
        egui::SidePanel::right(id.with("debug_console")).resizable(true).default_width((ui.available_width() * 0.35).min(420.0)).width_range(90.0..=side_max).frame(egui::Frame::none().inner_margin(egui::Margin { left: 6.0, ..Default::default() })).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("CONSOLE", "CONSOLE")).small().strong().color(ui.visuals().weak_text_color()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(t!("Effacer", "Clear")).clicked() {
                        act = Some(Act::Clear);
                    }
                });
            });
            // the field at the bottom: an expression evaluated where it is stopped
            egui::TopBottomPanel::bottom(id.with("debug_input")).frame(egui::Frame::none()).show_separator_line(false).show_inside(ui, |ui| {
                let hint = if d.phase == Phase::Stopped { t!("Expression (Entrée) : évaluée là où le programme est arrêté", "Expression (Enter): evaluated where the program is stopped") } else { t!("Expression (Entrée), une fois le programme arrêté", "Expression (Enter), once the program is stopped") };
                let r = ui.add(egui::TextEdit::singleline(&mut input).hint_text(hint).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !input.trim().is_empty() {
                    act = Some(Act::Evaluate(std::mem::take(&mut input).trim().to_string()));
                    r.request_focus();
                }
            });
            let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::both().id_salt("debug_console_scroll").auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, row_h, d.console.len(), |ui, range| {
                for (kind, l) in &d.console[range] {
                    let text = RichText::new(l).monospace();
                    let text = match kind {
                        Line::Out => text,
                        Line::Err => text.color(err()),
                        Line::Info => text.color(ui.visuals().weak_text_color()),
                    };
                    ui.add(egui::Label::new(text).extend());
                }
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(6.0, 0.0))).show_inside(ui, |ui| {
            ui.label(RichText::new(t!("VARIABLES", "VARIABLES")).small().strong().color(ui.visuals().weak_text_color()));
            egui::ScrollArea::both().id_salt("debug_vars_scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                if d.scopes.is_empty() {
                    ui.label(RichText::new(t!("Celles de la ligne où le programme est arrêté.", "Those of the line where the program is stopped.")).weak());
                }
                for (name, r) in &d.scopes {
                    let open = d.open.contains(r);
                    let head = RichText::new(format!("{} {name}", if open { "▾" } else { "▸" })).strong();
                    if ui.add(egui::Label::new(head).sense(Sense::click())).clicked() && *r > 0 {
                        act = Some(Act::Toggle(*r));
                    }
                    if open {
                        var_rows(ui, d, *r, 1, accent, &mut act);
                    }
                }
            });
        });
        self.debug.input = input;
        let Some(a) = act else { return };
        match a {
            Act::Cmd(c) => self.debug_command(c),
            Act::Choose(c) => self.debug.choice = c,
            Act::Frame(k) => self.debug_select_frame(k),
            Act::Toggle(r) => {
                if !self.debug.open.remove(&r) {
                    self.debug.open.insert(r);
                    if !self.debug.vars.contains_key(&r) {
                        self.debug.request("variables", json!({"variablesReference": r}), Pending::Variables(r));
                    }
                }
            }
            Act::Evaluate(e) => self.debug_evaluate(e),
            Act::Clear => self.debug.console.clear(),
        }
    }
}

/// The variables of `reference`, those opened with their children under them.
fn var_rows(ui: &mut egui::Ui, d: &DebugUi, reference: i64, depth: usize, accent: Color32, act: &mut Option<Act>) {
    let Some(vars) = d.vars.get(&reference) else {
        ui.label(RichText::new("…").weak());
        return;
    };
    for v in vars {
        let open = v.reference > 0 && d.open.contains(&v.reference);
        let r = ui
            .horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0);
                let arrow = if v.reference > 0 { if open { "▾ " } else { "▸ " } } else { "  " };
                let mut job = egui::text::LayoutJob::default();
                let mono = egui::FontId::monospace(12.5);
                let fmt = |c: Color32| egui::TextFormat::simple(mono.clone(), c);
                job.append(arrow, 0.0, fmt(ui.visuals().weak_text_color()));
                job.append(&v.name, 0.0, fmt(accent));
                job.append(" = ", 0.0, fmt(ui.visuals().weak_text_color()));
                job.append(&v.value, 0.0, fmt(ui.visuals().text_color()));
                ui.add(egui::Label::new(job).sense(Sense::click()).truncate())
            })
            .inner;
        let tip = if v.kind.is_empty() { v.value.clone() } else { format!("{} : {}\n{}", v.name, v.kind, v.value) };
        if r.on_hover_text(tip).clicked() && v.reference > 0 {
            *act = Some(Act::Toggle(v.reference));
        }
        if open && depth < 12 {
            var_rows(ui, d, v.reference, depth + 1, accent, act);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel drawn with a stopped session's data (a stack, variables opened, a console), in a
    /// narrow and a wide window: what it says is there, nothing panics (egui checks its sizes).
    #[test]
    fn the_panel_shows_a_stop() {
        let dir = std::env::temp_dir().join(format!("florm_debug_panel_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, None);
        let d = &mut app.debug;
        d.phase = Phase::Stopped;
        d.reason = "breakpoint".into();
        d.frames = vec![Frame { id: 1, name: "f".into(), path: Some(dir.join("prog.py")), line: 2 }, Frame { id: 2, name: "<module>".into(), path: None, line: 4 }];
        d.scopes = vec![("Locals".into(), 7)];
        d.open.insert(7);
        d.open.insert(8);
        d.vars.insert(7, vec![Var { name: "total".into(), value: "42".into(), kind: "int".into(), reference: 0 }, Var { name: "items".into(), value: "[1, 2]".into(), kind: "list".into(), reference: 8 }]);
        d.vars.insert(8, vec![Var { name: "0".into(), value: "1".into(), kind: "int".into(), reference: 0 }]);
        d.log(Line::Out, "hello\nworld");
        for w in [500.0, 1600.0] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, 400.0));
            let mut texts = String::new();
            for _ in 0..2 {
                let out = ctx.run(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.debug_panel(ui));
                });
                fn walk(s: &egui::Shape, out: &mut String) {
                    match s {
                        egui::Shape::Text(t) => {
                            out.push_str(t.galley.text());
                            out.push('|');
                        }
                        egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                        _ => {}
                    }
                }
                texts.clear();
                out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
            }
            for want in ["PILE D'APPELS", "prog.py:3", "total = 42", "items = [1, 2]", "0 = 1", "hello", "world", "point d'arrêt"] {
                assert!(texts.contains(want), "{w}: {want} in {texts}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
