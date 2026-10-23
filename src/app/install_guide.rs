//! Shown by a view instead of its content when the program it drives is missing (gh, claude,
//! docker): how to install it on this system, each command to copy or to type in a terminal, and
//! the view comes back by itself once the program is found (looked for again every 2 s).

use super::*;
use crate::tools::{System, Tool};

/// Systems a guide can be shown for (the detected one first).
const SYSTEMS: [System; 7] = [System::Arch, System::Debian, System::Fedora, System::Suse, System::OtherLinux, System::Mac, System::Windows];

enum GuideAct {
    Type(String),
    Copy(String),
    Check,
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
        let code_bg = ui.visuals().extreme_bg_color;
        let mut act = None;
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
                for (n, step) in tool.steps(sys).into_iter().enumerate() {
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
                                act = Some(GuideAct::Type(cmd.clone()));
                            }
                            if ui.button(t!("Copier", "Copy")).clicked() {
                                act = Some(GuideAct::Copy(cmd.clone()));
                            }
                        });
                    }
                    ui.add_space(8.0);
                }
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
        match act {
            Some(GuideAct::Type(cmd)) => self.install_type(&cmd, in_tab),
            Some(GuideAct::Copy(cmd)) => {
                ui.ctx().copy_text(cmd);
                self.set_status(t!("Commande copiée", "Command copied"));
            }
            Some(GuideAct::Check) => {
                tool.forget();
                if !tool.installed() {
                    self.set_status(tf!("{} est toujours introuvable", "{} is still not found", tool.program()));
                }
            }
            None => {}
        }
        true
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
            let opened = if in_tab {
                self.term.open_panel(&ctx, shell.as_deref(), None)
            } else {
                self.term.open_tab(&ctx, shell.as_deref(), None).map(|(id, _)| id)
            };
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
