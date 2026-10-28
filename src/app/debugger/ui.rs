//! The debug view: its bar, settings, variables / watches / call stack / breakpoints, console,
//! and what their buttons do.

use super::*;

/// Something clicked in the view, done once it is drawn.
pub(super) enum DebugAct {
    Start,
    Stop,
    Restart,
    Step(&'static str),
    Toggle(String, i64, bool),
    Frame(i64, usize),
    LoadStack(i64),
    Open(PathBuf, usize),
    RemoveBp(PathBuf, usize),
    ConditionChanged(PathBuf),
    ClearBps,
    Watch(String),
    Unwatch(usize),
    Eval(String),
    Copy(String),
    Install(Vec<String>),
    Exceptions,
    Target(String),
    Move(bool),
}

/// Open by default: the first scope (the local variables), not the others nor the variables.
pub(super) fn is_open(open: &HashSet<String>, shut: &HashSet<String>, path: &str, default: bool) -> bool {
    open.contains(path) || (default && !shut.contains(path))
}

pub(super) fn reason_label(reason: &str) -> String {
    match reason {
        "breakpoint" => t!("point d'arrêt", "breakpoint").into(),
        "step" => t!("pas à pas", "step").into(),
        "exception" => t!("exception", "exception").into(),
        "pause" => t!("pause", "pause").into(),
        "entry" => t!("début", "entry").into(),
        "goto" => "goto".into(),
        r => r.to_string(),
    }
}

/// A row of a tree: indent, ▸ / ▾ when it opens, name, value, type (on hover).
pub(super) fn tree_row(ui: &mut egui::Ui, depth: usize, open: Option<bool>, name: &str, value: &str, ty: &str, weak: Color32) -> egui::Response {
    let r = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(depth as f32 * 14.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), Sense::hover());
        if let Some(o) = open {
            icons::paint_chevron(ui.painter(), rect.center(), o, weak);
        }
        let mono = egui::TextStyle::Monospace;
        if value.is_empty() {
            ui.label(RichText::new(name).strong());
        } else {
            ui.label(RichText::new(name).text_style(mono.clone()).color(highlight::blue()));
            ui.add(egui::Label::new(RichText::new(value).text_style(mono)).truncate());
        }
    });
    let r = ui.interact(r.response.rect, ui.id().with(("tree_row", depth, name, r.response.rect.top() as i32)), Sense::click());
    let r = if ty.is_empty() { r } else { r.on_hover_text(format!("{name} : {ty}\n{value}")) };
    if open.is_some() && r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// The children of `reference` in the variables tree, and theirs when they are open.
#[allow(clippy::too_many_arguments)]
pub(super) fn var_rows(ui: &mut egui::Ui, s: &Session, open: &HashSet<String>, shut: &HashSet<String>, reference: i64, path: &str, depth: usize, weak: Color32, act: &mut Option<DebugAct>) {
    let Some(vars) = s.vars.get(&reference) else {
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0 + 16.0);
            spinner(ui);
        });
        return;
    };
    if vars.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0 + 16.0);
            ui.label(RichText::new(t!("(vide)", "(empty)")).small().color(weak));
        });
    }
    for v in vars {
        let p = format!("{path}\u{1}{}", v.name);
        let is = v.reference > 0 && is_open(open, shut, &p, false);
        let r = tree_row(ui, depth, (v.reference > 0).then_some(is), &v.name, &v.value, &v.ty, weak);
        if r.clicked() && v.reference > 0 {
            *act = Some(DebugAct::Toggle(p.clone(), v.reference, !is));
        }
        r.context_menu(|ui| {
            if ui.button(t!("Copier la valeur", "Copy the value")).clicked() {
                *act = Some(DebugAct::Copy(v.value.clone()));
                ui.close_menu();
            }
            if let Some(e) = &v.expr {
                if ui.button(t!("Copier l'expression", "Copy the expression")).clicked() {
                    *act = Some(DebugAct::Copy(e.clone()));
                    ui.close_menu();
                }
                if ui.button(t!("Espionner", "Watch")).clicked() {
                    *act = Some(DebugAct::Watch(e.clone()));
                    ui.close_menu();
                }
            }
        });
        if is {
            var_rows(ui, s, open, shut, v.reference, &p, depth + 1, weak, act);
        }
    }
}

impl App {
    pub(in crate::app) fn debug_ui(&mut self, ui: &mut egui::Ui) {
        let mut act: Option<DebugAct> = None;
        let fill = ui.visuals().panel_fill;
        let weak = ui.visuals().weak_text_color();
        let big = self.docs.iter().any(|d| d.view == document::View::Debug);
        egui::TopBottomPanel::top("debug_bar")
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 6.0, bottom: 6.0 }))
            .show_inside(ui, |ui| self.debug_bar(ui, big, &mut act));
        egui::SidePanel::left("debug_side")
            .resizable(true)
            .default_width(360.0)
            .width_range(220.0..=900.0)
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 8.0, right: 8.0, top: 6.0, bottom: 6.0 }))
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.debug_side(ui, weak, &mut act));
            });
        egui::CentralPanel::default().frame(egui::Frame::none().fill(self.hl.colors.bg).inner_margin(egui::Margin::same(6.0))).show_inside(ui, |ui| self.debug_console(ui, &mut act));
        if let Some(a) = act {
            self.debug_act(a);
        }
    }

    pub(super) fn debug_bar(&mut self, ui: &mut egui::Ui, big: bool, act: &mut Option<DebugAct>) {
        let weak = ui.visuals().weak_text_color();
        ui.horizontal(|ui| {
            crate::view_bar::title(ui, t!("Débogage", "Debug"));
            ui.add_space(6.0);
            let attach = self.settings.debug_mode == "attach";
            let listen = Some(self.debug_listen_command());
            let live = self.debug.session.as_ref();
            let paused = live.is_some_and(|s| s.paused.is_some());
            match live {
                None => {
                    if self.debug.preparing.is_some() {
                        spinner(ui);
                        if ui.button(t!("■ Annuler", "■ Cancel")).clicked() {
                            *act = Some(DebugAct::Stop);
                        }
                    } else if attach {
                        if ui.add(filled_button(t!("▶ Attendre le programme", "▶ Wait for the program"), self.hl.colors.accent)).on_hover_text(t!("Attendre que le programme écoute avec debugpy, puis s'y attacher (F5)", "Wait for the program to listen with debugpy, then attach to it (F5)")).clicked() {
                            *act = Some(DebugAct::Start);
                        }
                    } else if ui.add(filled_button(t!("▶ Démarrer", "▶ Start"), self.hl.colors.accent)).on_hover_text(t!("Lancer le programme sous le débogueur (F5)", "Run the program under the debugger (F5)")).clicked() {
                        *act = Some(DebugAct::Start);
                    }
                    if !self.debug.bps.values().any(|v| !v.is_empty()) {
                        ui.label(RichText::new(t!("Pose un point d'arrêt (F9) : Florm attend tout seul le programme lancé avec debugpy.", "Put a breakpoint (F9): Florm waits by itself for the program started with debugpy.")).small().color(weak));
                    } else if self.debug.auto_off {
                        ui.label(RichText::new(t!("Attente arrêtée à la main (F5 pour reprendre)", "Waiting stopped by hand (F5 to resume)")).small().color(weak));
                    }
                }
                Some(s) if !s.connected => {
                    // waiting for a program started with debugpy; F5 can still launch the one of the view
                    if !attach && ui.add(filled_button(t!("▶ Démarrer", "▶ Start"), self.hl.colors.accent)).on_hover_text(t!("Lancer le programme sous le débogueur (F5) au lieu d'attendre", "Run the program under the debugger (F5) instead of waiting")).clicked() {
                        *act = Some(DebugAct::Start);
                    }
                    spinner(ui);
                    if ui.button(t!("■ Arrêter l'attente", "■ Stop waiting")).on_hover_text(t!("Ne plus attendre le programme (F5 pour attendre à nouveau)", "No longer wait for the program (F5 to wait again)")).clicked() {
                        *act = Some(DebugAct::Stop);
                    }
                }
                Some(_) => {
                    let b = |ui: &mut egui::Ui, label: &str, tip: &str, enabled: bool| ui.add_enabled(enabled, egui::Button::new(label)).on_hover_text(tip).on_disabled_hover_text(tip).clicked();
                    if paused {
                        if b(ui, t!("▶ Continuer", "▶ Continue"), t!("Continuer jusqu'au prochain point d'arrêt (F5)", "Continue to the next breakpoint (F5)"), true) {
                            *act = Some(DebugAct::Step("continue"));
                        }
                    } else if b(ui, "‖ Pause", t!("Mettre le programme en pause là où il est (F6)", "Pause the program where it is (F6)"), true) {
                        *act = Some(DebugAct::Step("pause"));
                    }
                    for (label, cmd, tip) in [
                        ("↷", "next", t!("Pas à pas principal : la ligne suivante, sans entrer dans les appels (F10)", "Step over: the next line, without entering the calls (F10)")),
                        ("↓", "stepIn", t!("Pas à pas détaillé : entrer dans l'appel (F11)", "Step into: enter the call (F11)")),
                        ("↑", "stepOut", t!("Pas à pas sortant : finir la fonction (Maj+F11)", "Step out: finish the function (Shift+F11)")),
                    ] {
                        if b(ui, label, tip, paused) {
                            *act = Some(DebugAct::Step(cmd));
                        }
                    }
                    if b(ui, "⟳", t!("Redémarrer (Ctrl+Maj+F5)", "Restart (Ctrl+Shift+F5)"), true) {
                        *act = Some(DebugAct::Restart);
                    }
                    if b(ui, t!("■ Arrêter", "■ Stop"), t!("Arrêter le débogage (Maj+F5)", "Stop debugging (Shift+F5)"), true) {
                        *act = Some(DebugAct::Stop);
                    }
                }
            }
            if let Some(s) = live {
                let state = match &s.paused {
                    Some(p) => {
                        let mut t = tf!("{} : en pause, {}", "{}: paused, {}", s.title, reason_label(&p.reason));
                        if !p.text.is_empty() && p.reason != "exception" {
                            t.push_str(&format!(" ({})", p.text));
                        }
                        RichText::new(t).color(highlight::yellow())
                    }
                    None if s.stopping.is_some() => RichText::new(t!("arrêt…", "stopping…")).color(weak),
                    None if !s.connected => RichText::new(tf!("en attente du programme (port {})…", "waiting for the program (port {})…", s.cfg.port)).color(weak),
                    None => RichText::new(tf!("{} : en cours", "{}: running", s.title)).color(highlight::green()),
                };
                ui.label(state);
            } else if let Some(t) = &self.debug.preparing {
                ui.label(RichText::new(tf!("préparation de {t}…", "preparing {t}…")).color(weak));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(crate::view_bar::BarAct::Move(b)) = crate::view_bar::controls(ui, big, None, None) {
                    *act = Some(DebugAct::Move(b));
                }
                // attach: the command that starts the program under debugpy, to paste in a terminal
                if let Some(cmd) = listen.filter(|_| !self.debug.session.as_ref().is_some_and(|s| s.connected)) {
                    if ui.button(t!("Copier la commande", "Copy the command")).on_hover_text(tf!("{cmd}\n\nÀ lancer dans un terminal (celui de Florm ou le tien) ; dans un conteneur, comme commande d'Odoo (command: du docker-compose.yml).", "{cmd}\n\nTo run in a terminal (Florm's or yours); in a container, as the command of Odoo (command: of docker-compose.yml).")).clicked() {
                        *act = Some(DebugAct::Copy(cmd));
                    }
                }
            });
        });
        if let Some(e) = self.debug.session.as_ref().and_then(|s| s.exception.clone()) {
            ui.label(RichText::new(format!("⚠ {e}")).color(highlight::red()).monospace());
        }
        // what is debugged: only between two sessions
        let locked = self.debug.session.is_some() || self.debug.preparing.is_some();
        // folded into one line: where, how, what (Modifier… unfolds it)
        let summary = self.debug_summary();
        ui.horizontal(|ui| {
            ui.label(RichText::new(summary).small().monospace().color(ui.visuals().weak_text_color()));
            if !locked {
                let label = if self.debug.editing { t!("✔ Fermer", "✔ Close") } else { t!("Modifier…", "Edit…") };
                if ui.small_button(label).on_hover_text(t!("Où le programme tourne, lancer ou s'attacher, la commande", "Where the program runs, launch or attach, the command")).clicked() {
                    self.debug.editing = !self.debug.editing;
                }
            }
        });
        if self.debug.editing && !locked {
            self.debug_settings_row(ui, act);
        }
        match &self.debug.failure {
            Some(dap::Failure::NoDebugpy { install, place }) => {
                let (install, place) = (install.clone(), place.clone());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tf!("debugpy n'est pas installé {place} : il fait le lien entre Python et le débogueur.", "debugpy is not installed {place}: it links Python to the debugger.")).color(highlight::red()));
                    if ui.button(t!("Installer debugpy", "Install debugpy")).on_hover_text(install.join(" ")).clicked() {
                        *act = Some(DebugAct::Install(install));
                    }
                });
            }
            Some(dap::Failure::Other(e)) => {
                ui.label(RichText::new(e).color(highlight::red()));
            }
            None => {}
        }
    }

    pub(super) fn debug_settings_row(&mut self, ui: &mut egui::Ui, act: &mut Option<DebugAct>) {
        let weak = ui.visuals().weak_text_color();
        let here = t!("Cet ordinateur", "This computer").to_string();
        let containers = if crate::docker::available() { self.debug_containers() } else { Some(Vec::new()) };
        let target = self.settings.debug_target.clone();
        let mode_attach = self.settings.debug_mode == "attach";
        let default_local = self.debug_default_local();
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("Où", "Where")).small().color(weak));
            let mut items = vec![here.clone()];
            let names: Vec<String> = containers.clone().unwrap_or_default();
            items.extend(names.iter().map(|n| format!("🐳 {n}")));
            // the container chosen, even when it is not running now
            if !target.is_empty() && !names.contains(&target) {
                items.push(tf!("🐳 {target} (arrêté ?)", "🐳 {target} (stopped?)"));
            }
            let current = if target.is_empty() { Some(0) } else { items.iter().position(|i| i.strip_prefix("🐳 ").is_some_and(|n| n == target || n.starts_with(&format!("{target} ")))) };
            let shown = current.and_then(|k| items.get(k)).cloned().unwrap_or_else(|| here.clone());
            if let Some(k) = crate::pick::combo(ui, "debug_target", shown, &items, current) {
                let name = if k == 0 { String::new() } else { items[k].trim_start_matches("🐳 ").split(" (").next().unwrap_or("").to_string() };
                *act = Some(DebugAct::Target(name));
            }
            if containers.is_none() {
                spinner(ui);
            }
            ui.add_space(6.0);
            let mut attach = mode_attach;
            ui.selectable_value(&mut attach, false, t!("Lancer", "Launch")).on_hover_text(t!("Florm lance le programme sous le débogueur", "Florm runs the program under the debugger"));
            ui.selectable_value(&mut attach, true, t!("S'attacher", "Attach")).on_hover_text(t!("Le programme tourne déjà avec debugpy --listen : Florm s'y connecte", "The program already runs with debugpy --listen: Florm connects to it"));
            if attach != mode_attach {
                self.settings.debug_mode = if attach { "attach".into() } else { "launch".into() };
                self.persist("debug_mode", config::toml_str(&self.settings.debug_mode));
            }
            ui.add_space(6.0);
            if attach {
                ui.label(RichText::new("Port").small().color(weak));
                let mut port = self.settings.debug_port as u32;
                if ui.add(egui::DragValue::new(&mut port).range(1..=65535)).changed() {
                    self.settings.debug_port = port as u16;
                    self.persist("debug_port", port.to_string());
                }
            } else {
                ui.label(RichText::new(t!("Commande", "Command")).small().color(weak));
                let (value, hint, key) = if target.is_empty() {
                    (&mut self.settings.debug_command, if default_local.is_empty() { "./odoo-bin -c odoo.conf -d ma_base --workers 0".to_string() } else { default_local.clone() }, "debug_command")
                } else {
                    (&mut self.settings.debug_docker_command, DOCKER_COMMAND.to_string(), "debug_docker_command")
                };
                let r = crate::input::add(ui, egui::TextEdit::singleline(value).hint_text(hint).font(egui::TextStyle::Monospace).desired_width((ui.available_width() - 190.0).max(160.0)));
                if r.lost_focus() {
                    let v = config::toml_str(value);
                    self.persist(key, v);
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        *act = Some(DebugAct::Start);
                    }
                }
            }
            let mut mine = self.settings.debug_just_my_code;
            if ui.checkbox(&mut mine, t!("Mon code seulement", "My code only")).on_hover_text(t!("Ne pas entrer dans le code d'Odoo ni des bibliothèques (justMyCode) : les points d'arrêt n'y sont pas pris non plus", "Do not step into the code of Odoo nor of the libraries (justMyCode): the breakpoints there are not taken either")).changed() {
                self.settings.debug_just_my_code = mine;
                self.persist("debug_just_my_code", mine.to_string());
            }
        });
        let help = match (target.is_empty(), mode_attach) {
            (true, false) => t!("Lance la commande ici, dans le panneau du bas, sous le débogueur. --workers 0 : un seul processus, chaque requête passe par les points d'arrêt.", "Runs the command here, in the bottom panel, under the debugger. --workers 0: one process, every request goes through the breakpoints.").to_string(),
            (false, false) => t!("Lance la commande dans le conteneur, sous le débogueur, à côté de l'Odoo qui y tourne (un autre port). Ses fichiers montés s'ouvrent ici, les autres en lecture seule. Clic droit sur le ▶ d'un test (ou Alt+F6) : déboguer ce test.", "Runs the command in the container, under the debugger, next to the Odoo running there (another port). Its mounted files open here, the others read only. Right click on the ▶ of a test (or Alt+F6): debug this test.").to_string(),
            (true, true) => tf!("Avec un point d'arrêt, Florm attend tout seul que le programme écoute, puis s'y attache, à chaque lancement. Lancez-le avec « Copier la commande » (python3 -m debugpy --listen {} ./odoo-bin … --workers 0), dans le terminal de Florm ou le vôtre.", "With a breakpoint, Florm waits by itself for the program to listen, then attaches to it, at each run. Start it with “Copy the command” (python3 -m debugpy --listen {} ./odoo-bin … --workers 0), in Florm's terminal or yours.", self.settings.debug_port),
            (false, true) => tf!("Dans le conteneur, Odoo doit tourner avec debugpy (command: du docker-compose.yml) : python3 -m debugpy --listen 0.0.0.0:{} /usr/bin/odoo … --workers 0. Florm s'y connecte par docker exec : aucun port à publier.", "In the container, Odoo must run with debugpy (command: of docker-compose.yml): python3 -m debugpy --listen 0.0.0.0:{} /usr/bin/odoo … --workers 0. Florm connects to it through docker exec: no port to publish.", self.settings.debug_port),
        };
        ui.label(RichText::new(help).small().color(weak));
    }

    pub(super) fn debug_side(&mut self, ui: &mut egui::Ui, weak: Color32, act: &mut Option<DebugAct>) {
        let mono = egui::TextStyle::Monospace;
        let header = |ui: &mut egui::Ui, id: &str, title: &str, open: bool, add: &mut dyn FnMut(&mut egui::Ui)| {
            egui::CollapsingHeader::new(RichText::new(title).strong()).id_salt(id).default_open(open).show(ui, |ui| add(ui));
        };
        let d = &self.debug;
        let s = d.session.as_ref();
        let paused = s.is_some_and(|s| s.paused.is_some());

        // ------------------------------------------------ variables
        header(ui, "debug_vars", t!("Variables", "Variables"), true, &mut |ui| {
            let Some(s) = s.filter(|_| paused) else {
                ui.label(RichText::new(t!("Visibles quand le programme est en pause.", "Shown when the program is paused.")).small().color(weak));
                return;
            };
            if s.scopes.is_empty() {
                spinner(ui);
            }
            for (k, sc) in s.scopes.iter().enumerate() {
                let open = is_open(&d.open, &d.shut, &sc.name, k == 0);
                if tree_row(ui, 0, Some(open), &sc.name, "", "", weak).clicked() {
                    *act = Some(DebugAct::Toggle(sc.name.clone(), sc.reference, !open));
                }
                if open {
                    var_rows(ui, s, &d.open, &d.shut, sc.reference, &sc.name, 1, weak, act);
                }
            }
        });

        // ------------------------------------------------ watch
        let (watch, draft) = (&self.debug.watch, &mut self.debug.watch_draft);
        let session = self.debug.session.as_ref();
        egui::CollapsingHeader::new(RichText::new(t!("Espion", "Watch")).strong()).id_salt("debug_watch").default_open(true).show(ui, |ui| {
            for (k, e) in watch.iter().enumerate() {
                ui.horizontal(|ui| {
                    let v = session.and_then(|s| s.watch.get(e));
                    ui.label(RichText::new(e).text_style(mono.clone()).color(highlight::blue()));
                    match v {
                        Some(Ok(v)) => {
                            ui.add(egui::Label::new(RichText::new(format!("= {v}")).text_style(mono.clone())).truncate()).on_hover_text(v);
                        }
                        Some(Err(e)) => {
                            ui.add(egui::Label::new(RichText::new(e).small().color(highlight::red())).truncate());
                        }
                        None if paused => {
                            spinner(ui);
                        }
                        None => {}
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text(t!("Ne plus espionner", "Stop watching")).clicked() {
                            *act = Some(DebugAct::Unwatch(k));
                        }
                    });
                });
            }
            let r = crate::input::add(ui, egui::TextEdit::singleline(draft).hint_text(t!("+ expression à espionner (self.state…)", "+ expression to watch (self.state…)")).font(egui::TextStyle::Small).desired_width(f32::INFINITY));
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !draft.trim().is_empty() {
                *act = Some(DebugAct::Watch(std::mem::take(draft).trim().to_string()));
            }
        });
        let d = &self.debug;
        let s = d.session.as_ref();

        // ------------------------------------------------ call stack
        header(ui, "debug_stack", t!("Pile d'appels", "Call stack"), true, &mut |ui| {
            let Some(s) = s.filter(|_| paused) else {
                ui.label(RichText::new(t!("Visible quand le programme est en pause.", "Shown when the program is paused.")).small().color(weak));
                return;
            };
            let p = s.paused.as_ref().expect("paused");
            let mut threads = s.threads.clone();
            if !threads.iter().any(|(t, _)| *t == p.thread) {
                threads.insert(0, (p.thread, String::new()));
            }
            for (t, name) in threads {
                let stack = s.stacks.get(&t);
                let label = if t == p.thread { tf!("{name} — en pause ({})", "{name} — paused ({})", reason_label(&p.reason)) } else { name.clone() };
                let r = tree_row(ui, 0, Some(stack.is_some()), &label, "", "", weak);
                if r.clicked() && stack.is_none() {
                    *act = Some(DebugAct::LoadStack(t));
                }
                let Some(frames) = stack else { continue };
                for (k, f) in frames.iter().enumerate() {
                    let sel = s.frame == Some((t, k));
                    let file = f.remote.as_deref().map_or(String::new(), |r| format!("{}:{}", r.rsplit('/').next().unwrap_or(r), f.line));
                    let r = ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        let r = ui.selectable_label(sel, RichText::new(&f.name).text_style(mono.clone()));
                        ui.label(RichText::new(&file).small().color(weak));
                        r
                    });
                    let tip = f.remote.clone().unwrap_or_default();
                    if r.inner.on_hover_text(tip).clicked() {
                        *act = Some(DebugAct::Frame(t, k));
                    }
                }
            }
        });

        // ------------------------------------------------ breakpoints
        let n: usize = d.bps.values().map(Vec::len).sum();
        egui::CollapsingHeader::new(RichText::new(tf!("Points d'arrêt ({n})", "Breakpoints ({n})")).strong()).id_salt("debug_bps").default_open(true).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(t!("Exceptions :", "Exceptions:")).small().color(weak));
                for (key, label, tip) in [
                    ("uncaught", t!("non interceptées", "uncaught"), t!("S'arrêter sur une exception que rien n'intercepte", "Stop on an exception nothing catches")),
                    ("raised", t!("toutes", "all raised"), t!("S'arrêter sur chaque exception levée, même interceptée (Odoo en lève beaucoup)", "Stop on every exception raised, even caught (Odoo raises many)")),
                    ("userUnhandled", t!("hors de mon code", "leaving my code"), t!("S'arrêter quand une exception sort du code du projet", "Stop when an exception leaves the code of the project")),
                ] {
                    let mut on = self.settings.debug_exceptions.iter().any(|e| e == key);
                    if ui.checkbox(&mut on, RichText::new(label).small()).on_hover_text(tip).changed() {
                        self.settings.debug_exceptions.retain(|e| e != key);
                        if on {
                            self.settings.debug_exceptions.push(key.to_string());
                        }
                        *act = Some(DebugAct::Exceptions);
                    }
                }
            });
            if n == 0 {
                ui.label(RichText::new(t!("Clic à gauche des numéros de ligne (ou F9) pour en mettre un.", "Click left of the line numbers (or F9) to put one.")).small().color(weak));
            } else if ui.small_button(t!("Tout retirer", "Remove all")).clicked() {
                *act = Some(DebugAct::ClearBps);
            }
            let root = self.root.clone();
            for (path, list) in self.debug.bps.iter_mut() {
                let rel = root.as_ref().and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path).display().to_string();
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                for b in list.iter_mut() {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), Sense::hover());
                        match &b.state {
                            Some((false, _)) => {
                                ui.painter().circle_stroke(rect.center(), 4.0, egui::Stroke::new(1.5_f32, highlight::red()));
                            }
                            _ => {
                                ui.painter().circle_filled(rect.center(), 4.0, highlight::red());
                            }
                        }
                        let tip = match &b.state {
                            Some((false, m)) if !m.is_empty() => format!("{rel}:{}\n{m}", b.line + 1),
                            Some((false, _)) => tf!("{rel}:{}\nPas encore pris par le débogueur", "{rel}:{}\nNot taken by the debugger yet", b.line + 1),
                            _ => format!("{rel}:{}", b.line + 1),
                        };
                        if ui.link(format!("{name}:{}", b.line + 1)).on_hover_text(tip).clicked() {
                            *act = Some(DebugAct::Open(path.clone(), b.line));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text(t!("Retirer", "Remove")).clicked() {
                                *act = Some(DebugAct::RemoveBp(path.clone(), b.line));
                            }
                            let r = crate::input::add(ui, egui::TextEdit::singleline(&mut b.condition).hint_text(t!("condition", "condition")).font(egui::TextStyle::Small).desired_width(ui.available_width()));
                            let r = r.on_hover_text(t!("S'arrêter seulement quand elle est vraie, ex. self.id == 7 (Entrée)", "Stop only when it is true, e.g. self.id == 7 (Enter)"));
                            if r.lost_focus() {
                                *act = Some(DebugAct::ConditionChanged(path.clone()));
                            }
                        });
                    });
                }
            }
        });
    }

    pub(super) fn debug_console(&mut self, ui: &mut egui::Ui, act: &mut Option<DebugAct>) {
        let mono = egui::TextStyle::Monospace;
        egui::TopBottomPanel::bottom("debug_repl").frame(egui::Frame::none().inner_margin(egui::Margin { left: 0.0, right: 0.0, top: 4.0, bottom: 0.0 })).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("›").text_style(mono.clone()).color(self.hl.colors.accent));
                let paused = self.debug.session.as_ref().is_some_and(|s| s.paused.is_some());
                let hint = if paused { t!("Python dans le cadre en pause : self.env['res.partner'].search([]), x = 2…", "Python in the paused frame: self.env['res.partner'].search([]), x = 2…") } else { t!("Python, une fois le programme en pause", "Python, once the program is paused") };
                let r = crate::input::add(ui, egui::TextEdit::singleline(&mut self.debug.repl).hint_text(hint).font(mono.clone()).desired_width(ui.available_width() - 60.0));
                if std::mem::take(&mut self.debug.focus_console) {
                    r.request_focus();
                }
                if r.has_focus() {
                    if let Some(h) = history_arrows(ui, &self.debug.repl_hist, &mut self.debug.repl_pos) {
                        self.debug.repl = h;
                    }
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.debug.repl_pos = None;
                    *act = Some(DebugAct::Eval(std::mem::take(&mut self.debug.repl)));
                    r.request_focus();
                }
                if ui.small_button(t!("Effacer", "Clear")).clicked() {
                    self.debug.console.clear();
                }
            });
        });
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let rh = ui.fonts(|f| f.row_height(&font));
        let n = self.debug.console.len();
        let colors = self.hl.colors;
        egui::ScrollArea::both().id_salt("debug_console").auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, rh, n, |ui, rows| {
            for (cat, text) in &self.debug.console[rows] {
                let c = match cat {
                    Cat::Out => colors.fg,
                    Cat::Err => highlight::red(),
                    Cat::Info => colors.gutter_fg,
                    Cat::Input => colors.accent,
                    Cat::Result => highlight::green(),
                };
                let r = ui.add(egui::Label::new(RichText::new(text).text_style(mono.clone()).color(c)).extend().sense(Sense::click()));
                r.context_menu(|ui| {
                    if ui.button(t!("Copier", "Copy")).clicked() {
                        *act = Some(DebugAct::Copy(text.clone()));
                        ui.close_menu();
                    }
                });
            }
        });
    }

    pub(super) fn debug_act(&mut self, a: DebugAct) {
        match a {
            DebugAct::Start => self.debug_start_or_continue(),
            DebugAct::Stop => self.debug_stop_by_hand(),
            DebugAct::Restart => self.debug_restart(),
            DebugAct::Step(c) => self.debug_step(c),
            DebugAct::Toggle(path, reference, open) => {
                let d = &mut self.debug;
                if open {
                    d.shut.remove(&path);
                    d.open.insert(path.clone());
                } else {
                    d.open.remove(&path);
                    d.shut.insert(path.clone());
                }
                if let Some(s) = d.session.as_mut().filter(|s| open && reference > 0 && !s.vars.contains_key(&reference)) {
                    let seq = s.client.request("variables", json!({ "variablesReference": reference }));
                    s.pending.insert(seq, Req::Vars(reference, path));
                }
            }
            DebugAct::Frame(t, k) => self.debug_select_frame(t, k),
            DebugAct::LoadStack(t) => {
                if let Some(s) = self.debug.session.as_mut() {
                    let seq = s.client.request("stackTrace", json!({ "threadId": t, "startFrame": 0, "levels": STACK_LEVELS }));
                    s.pending.insert(seq, Req::Stack(t));
                }
            }
            DebugAct::Open(p, line) => self.open_at(p, line),
            DebugAct::RemoveBp(p, line) => self.debug_remove_bp(p, line),
            DebugAct::ConditionChanged(p) => {
                self.debug.dirty.insert(p);
            }
            DebugAct::ClearBps => self.debug_clear_breakpoints(),
            DebugAct::Watch(e) => {
                if !self.debug.watch.contains(&e) {
                    self.debug.watch.push(e.clone());
                    self.debug_watch_eval(&e);
                }
            }
            DebugAct::Unwatch(k) => {
                if k < self.debug.watch.len() {
                    self.debug.watch.remove(k);
                }
            }
            DebugAct::Eval(e) => self.debug_eval(e),
            DebugAct::Copy(t) => self.egui_ctx.output_mut(|o| o.copied_text = t),
            DebugAct::Install(argv) => {
                let ctx = self.egui_ctx.clone();
                match self.term.open_command_panel(&ctx, &argv, self.root.clone()) {
                    Ok(id) => {
                        let title = t!("Installation de debugpy", "Installing debugpy").to_string();
                        self.term.set_name(id, title);
                        self.bottom = Some(BottomTab::Terminal);
                        self.debug.failure = None;
                        self.set_status(t!("Une fois debugpy installé : F5", "Once debugpy is installed: F5"));
                    }
                    Err(e) => self.set_error(tf!("Terminal impossible à ouvrir : {e}", "Cannot open a terminal: {e}")),
                }
            }
            DebugAct::Exceptions => {
                let v = config::toml_str_array(&self.settings.debug_exceptions);
                self.persist("debug_exceptions", v);
                if self.debug.session.as_ref().is_some_and(|s| s.configured) {
                    self.debug_send_exceptions();
                }
            }
            DebugAct::Target(name) => self.debug_set_target(name),
            DebugAct::Move(true) => self.show_big(BottomTab::Debug),
            DebugAct::Move(false) => self.show_bottom(BottomTab::Debug, false),
        }
    }

    /// Where, how and what is debugged, in one line.
    pub(super) fn debug_summary(&mut self) -> String {
        let cfg = self.debug_config();
        let place = match &cfg.target {
            Target::Local => t!("cet ordinateur", "this computer").to_string(),
            Target::Docker(c) => format!("🐳 {c}"),
        };
        let what = match cfg.mode {
            Mode::Attach => tf!("s'attacher au port {}", "attach to port {}", cfg.port),
            Mode::Launch if cfg.command.trim().is_empty() => t!("lancer : commande à indiquer", "launch: command to enter").to_string(),
            Mode::Launch => tf!("lancer {}", "launch {}", cfg.command),
        };
        format!("{place} · {what}")
    }

    /// Names of the running containers (`None` until they are read), read again every few
    /// seconds while they are shown.
    pub(super) fn debug_containers(&mut self) -> Option<Vec<String>> {
        let d = &mut self.debug;
        let stale = d.containers.as_ref().is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(10));
        if stale && d.containers_asked.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            d.containers_asked = Some(Instant::now());
            self.jobs.spawn(|| JobResult::DebugContainers(crate::docker::list().map(|l| l.into_iter().filter(|c| c.running()).map(|c| c.name).collect())));
        }
        self.egui_ctx.request_repaint_after(Duration::from_secs(10));
        self.debug.containers.as_ref().map(|(_, l)| l.clone())
    }

    /// The container the debugger works in (empty: this computer).
    pub(in crate::app) fn debug_set_target(&mut self, name: String) {
        if self.settings.debug_target != name {
            self.settings.debug_target = name.clone();
            self.persist("debug_target", config::toml_str(&name));
        }
        self.debug.failure = None;
    }
}
