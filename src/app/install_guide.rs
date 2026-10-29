//! Shown by a view instead of its content when the program it drives is missing (gh, claude,
//! docker): how to install it on this system, each command to copy or to type in a terminal, and
//! the view comes back by itself once the program is found (looked for again every 2 s). Also
//! under the switch of a part turned off in the preferences: what it needs, installed or not.

use super::*;
use crate::features::Feature;
use crate::tools::{System, Tool};

/// Systems a guide can be shown for (the detected one first).
const SYSTEMS: [System; 7] = [System::Arch, System::Debian, System::Fedora, System::Suse, System::OtherLinux, System::Mac, System::Windows];

enum GuideAct {
    Type(String),
    Copy(String),
    /// A command run in a terminal of the bottom panel (installing debugpy).
    Run(Vec<String>),
    Check,
}

/// The numbered steps of an installation, each command with its buttons.
fn guide_steps(ui: &mut egui::Ui, steps: Vec<crate::tools::Step>, code_bg: Color32, act: &mut Option<GuideAct>) {
    for (n, step) in steps.into_iter().enumerate() {
        ui.label(RichText::new(format!("{}. {}", n + 1, step.label)).strong());
        if let Some(cmd) = step.command {
            ui.horizontal(|ui| {
                egui::Frame::none().fill(code_bg).rounding(4.0).inner_margin(egui::Margin::symmetric(8.0, 5.0)).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&cmd).monospace()).selectable(true));
                });
                if ui
                    .button(t!("Dans le terminal", "In the terminal"))
                    .on_hover_text(t!("Tape la commande dans un terminal ; appuie sur Entrée pour la lancer", "Types the command in a terminal; press Enter to run it"))
                    .clicked()
                {
                    *act = Some(GuideAct::Type(cmd.clone()));
                }
                if ui.button(t!("Copier", "Copy")).clicked() {
                    *act = Some(GuideAct::Copy(cmd.clone()));
                }
            });
        }
        ui.add_space(8.0);
    }
}

/// "✔ name" or "✖ name" (missing).
fn need_line(ui: &mut egui::Ui, name: &str, ok: bool) {
    let (mark, color) = if ok { ("✔", highlight::green()) } else { ("✖", highlight::red()) };
    let state = if ok { t!("installé", "installed") } else { t!("introuvable", "not found") };
    ui.horizontal(|ui| {
        ui.label(RichText::new(mark).color(color));
        ui.label(RichText::new(name).strong());
        ui.label(RichText::new(state).weak());
    });
}

/// A Python given by its path (a virtualenv) or by its name in the PATH.
fn python_found(python: &str) -> bool {
    if python.contains('/') || python.contains('\\') {
        std::path::Path::new(python).is_file()
    } else {
        crate::tools::locate(python)
    }
}

impl App {
    /// The installation guide of `tool` when it is missing (true: shown, the view stops there).
    /// `in_tab`: the view is in an editor tab (the terminal then opens in the bottom panel, and
    /// the other way round, so that both stay in sight).
    pub(super) fn install_guide(&mut self, ui: &mut egui::Ui, tool: Tool, in_tab: bool) -> bool {
        if tool.installed() {
            return false;
        }
        // the program is looked for again a moment later (`Tool::installed` keeps its answer 2 s)
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        let sys_id = egui::Id::new("install_guide_system").with(tool);
        let mut sys = ui.ctx().data(|d| d.get_temp::<usize>(sys_id)).map_or_else(crate::tools::system, |k| SYSTEMS[k]);
        let accent = self.hl.colors.accent;
        let bg = self.hl.colors.bg;
        let code_bg = ui.visuals().extreme_bg_color;
        let mut act = None;
        // the theme's background all over the view (not the default one of the window)
        egui::Frame::none().fill(bg).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical().id_salt(sys_id.with("scroll")).auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::none().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                    ui.set_max_width(760.0);
                    ui.label(RichText::new(tf!("{} n'est pas installé", "{} is not installed", tool.title())).strong().size(17.0));
                    ui.add_space(4.0);
                    ui.label(RichText::new(tool.purpose()).weak());
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(t!("Système :", "System:"));
                        let before = sys;
                        egui::ComboBox::from_id_salt(sys_id).selected_text(sys.name()).show_ui(ui, |ui| {
                            for s in SYSTEMS {
                                ui.selectable_value(&mut sys, s, s.name());
                            }
                        });
                        if sys != before {
                            let k = SYSTEMS.iter().position(|s| *s == sys).unwrap_or(0);
                            ui.ctx().data_mut(|d| d.insert_temp(sys_id, k));
                        }
                        if sys == crate::tools::system() {
                            ui.label(RichText::new(t!("(détecté)", "(detected)")).weak());
                        }
                    });
                    ui.add_space(10.0);
                    guide_steps(ui, tool.steps(sys), code_bg, &mut act);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add(filled_button(t!("Vérifier à nouveau", "Check again"), accent)).clicked() {
                            act = Some(GuideAct::Check);
                        }
                        ui.label(RichText::new(t!("(vérifié aussi tout seul toutes les 2 secondes)", "(also checked by itself every 2 seconds)")).weak());
                    });
                    ui.add_space(6.0);
                    ui.hyperlink_to(t!("Documentation officielle", "Official documentation"), tool.docs_url());
                });
            });
        });
        if let Some(GuideAct::Check) = act {
            tool.forget();
            if !tool.installed() {
                self.set_status(tf!("{} est toujours introuvable", "{} is still not found", tool.program()));
            }
        } else if let Some(act) = act {
            self.guide_act(act, in_tab);
        }
        true
    }

    /// Under the switch of a part turned off (Preferences): whether what it needs is installed,
    /// and if not, how to install it.
    pub(super) fn feature_needs(&mut self, ui: &mut egui::Ui, f: Feature) {
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        let sys = crate::tools::system();
        let code_bg = ui.visuals().extreme_bg_color;
        let accent = self.hl.colors.accent;
        let python = Some(self.settings.debug_python.trim()).filter(|p| !p.is_empty()).unwrap_or(Tool::Python.program()).to_string();
        // debugpy is looked for by running Python: once, then on "Check again" or another Python
        let py_id = egui::Id::new("feature_needs_python");
        let mut act = None;
        let mut py: Option<(String, bool, bool)> = None;
        if f == Feature::Debugger {
            py = ui.ctx().data(|d| d.get_temp::<(String, bool, bool)>(py_id)).filter(|(p, ..)| *p == python);
            if py.is_none() {
                let found = python_found(&python);
                let v = (python.clone(), found, found && crate::dap::has_debugpy(&python));
                ui.ctx().data_mut(|d| d.insert_temp(py_id, v.clone()));
                py = Some(v);
            }
        }
        let tools: &[Tool] = match f {
            Feature::GitHub => &[Tool::Gh],
            Feature::Claude => &[Tool::Claude],
            Feature::Debugger => &[],
        };
        ui.add_space(12.0);
        ui.label(RichText::new(t!("Ce qu'il faut pour l'activer", "What it needs to be turned on")).strong());
        ui.add_space(6.0);
        let mut missing = false;
        for &tool in tools {
            let ok = tool.installed();
            missing |= !ok;
            need_line(ui, tool.title(), ok);
            if !ok {
                ui.indent(tool, |ui| {
                    ui.label(RichText::new(tool.purpose()).weak().size(12.5));
                    ui.add_space(6.0);
                    guide_steps(ui, tool.steps(sys), code_bg, &mut act);
                    ui.hyperlink_to(t!("Documentation officielle", "Official documentation"), tool.docs_url());
                });
            }
        }
        if let Some((_, has_python, has_debugpy)) = py {
            missing |= !(has_python && has_debugpy);
            need_line(ui, &tf!("Python ({python})", "Python ({python})"), has_python);
            if !has_python {
                ui.indent("python", |ui| {
                    ui.label(RichText::new(Tool::Python.purpose()).weak().size(12.5));
                    ui.label(
                        RichText::new(t!(
                            "Ou indique plus bas le Python à utiliser (un virtualenv…), une fois le débogueur activé.",
                            "Or set below the Python to use (a virtualenv…), once the debugger is on."
                        ))
                        .weak()
                        .size(12.5),
                    );
                    ui.add_space(6.0);
                    guide_steps(ui, Tool::Python.steps(sys), code_bg, &mut act);
                    ui.hyperlink_to(t!("Documentation officielle", "Official documentation"), Tool::Python.docs_url());
                });
            }
            need_line(ui, "debugpy", has_debugpy);
            if has_python && !has_debugpy {
                ui.indent("debugpy", |ui| {
                    ui.label(
                        RichText::new(t!("Le module de Python qui fait le lien entre le programme et le débogueur.", "The Python module that links the program to the debugger.")).weak().size(12.5),
                    );
                    ui.add_space(4.0);
                    let install = crate::dap::local_install(&python);
                    if ui.button(t!("Installer debugpy", "Install debugpy")).on_hover_text(install.join(" ")).clicked() {
                        act = Some(GuideAct::Run(install));
                    }
                    ui.add_space(8.0);
                });
            }
        }
        ui.add_space(6.0);
        if missing {
            ui.horizontal(|ui| {
                if ui.add(filled_button(t!("Vérifier à nouveau", "Check again"), accent)).clicked() {
                    act = Some(GuideAct::Check);
                }
                ui.label(
                    RichText::new(t!("Tu peux aussi l'activer tout de suite : sa vue redira ce qui manque.", "You can also turn it on right away: its view will tell again what is missing."))
                        .weak()
                        .size(12.5),
                );
            });
        } else {
            ui.label(RichText::new(t!("Tout est installé : il ne reste qu'à l'activer.", "Everything is installed: it only has to be turned on.")).color(highlight::green()));
        }
        match act {
            Some(GuideAct::Check) => {
                for &tool in tools {
                    tool.forget();
                }
                ui.ctx().data_mut(|d| d.remove::<(String, bool, bool)>(py_id));
            }
            Some(a) => self.guide_act(a, true),
            None => {}
        }
    }

    fn guide_act(&mut self, act: GuideAct, in_tab: bool) {
        match act {
            GuideAct::Type(cmd) => self.install_type(&cmd, in_tab),
            GuideAct::Copy(cmd) => {
                self.egui_ctx.copy_text(cmd);
                self.set_status(t!("Commande copiée", "Command copied"));
            }
            GuideAct::Run(argv) => {
                let ctx = self.egui_ctx.clone();
                match self.term.open_command_panel(&ctx, &argv, self.root.clone()) {
                    Ok(id) => {
                        self.term.set_name(id, t!("Installation de debugpy", "Installing debugpy").to_string());
                        self.bottom = Some(BottomTab::Terminal);
                        self.set_status(t!("Une fois debugpy installé : « Vérifier à nouveau »", "Once debugpy is installed: “Check again”"));
                    }
                    Err(e) => self.set_error(tf!("Terminal impossible à ouvrir : {e}", "Cannot open a terminal: {e}")),
                }
            }
            GuideAct::Check => {}
        }
    }

    /// Types `command` in the guides' terminal (opened if needed), without Enter.
    pub(super) fn install_type(&mut self, command: &str, in_tab: bool) {
        // what was typed before and not run is cleared first
        let clear = if cfg!(windows) { "\x1b" } else { "\x15" };
        let reused = self.install_term.is_some_and(|id| self.term.type_text(id, &format!("{clear}{command}")));
        if !reused {
            if self.term.cwd.is_none() {
                self.term.cwd = self.root.clone();
            }
            let (shell, ctx) = (self.term_shell(), self.egui_ctx.clone());
            let opened = if in_tab { self.term.open_panel(&ctx, shell.as_deref(), None) } else { self.term.open_tab(&ctx, shell.as_deref(), None).map(|(id, _)| id) };
            match opened {
                Ok(id) => {
                    self.term.type_text(id, command);
                    self.install_term = Some(id);
                }
                Err(e) => {
                    self.set_error(tf!("Terminal impossible à ouvrir : {e}", "Cannot open a terminal: {e}"));
                    return;
                }
            }
        }
        let Some(id) = self.install_term else { return };
        self.term.show(id);
        if self.term.docked_ids().contains(&id) {
            let title = self.term.title_of(id).unwrap_or_default();
            self.open_view_tab(document::View::Terminal(id), title);
        } else {
            self.bottom = Some(BottomTab::Terminal);
        }
    }
}
