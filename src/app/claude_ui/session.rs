//! The session of a conversation, in the panel on its right (beside the review of the changes):
//! its context and what it cost, the quotas of the plan, its folder and branch, the model, and
//! what Claude Code has besides (memory, subagents, commands, MCP servers).

use super::*;
use crate::app::claude_tools::{claude_home, mcp_state, ToolAct};

/// What the panel shows besides the conversation itself.
pub(in crate::app) struct SessionInfo<'a> {
    pub(in crate::app) project: String,
    pub(in crate::app) branch: Option<String>,
    pub(in crate::app) limits: &'a [(String, f32, i64)],
    pub(in crate::app) commands: usize,
    pub(in crate::app) allow: &'a [String],
}

/// A thin bar, `used` (0 to 1) of it filled: in the accent, orange past 70 %, red past 90 %.
fn bar(ui: &mut egui::Ui, used: f32, accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), Sense::hover());
    let color = if used > 0.9 {
        err()
    } else if used > 0.7 {
        warn()
    } else {
        accent
    };
    let p = ui.painter();
    p.rect_filled(rect, 2.0, ui.visuals().widgets.inactive.bg_fill);
    p.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * used.clamp(0.0, 1.0), rect.height())), 2.0, color);
}

/// A line of the panel: its label on the left, its value on the right.
fn row(ui: &mut egui::Ui, label: &str, value: impl Into<egui::WidgetText>) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(value)).inner
    })
    .inner
}

/// A group of lines on a light background, under its title.
fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    let fill = ui.visuals().faint_bg_color;
    egui::Frame::none().fill(fill).rounding(10.0).inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(title).strong());
        add(ui);
    });
    ui.add_space(8.0);
}

/// The panel; what was asked in it (open a memory file, read the MCP servers, allow one…).
pub(in crate::app) fn session_ui(ui: &mut egui::Ui, c: &mut ClaudeUi, info: &SessionInfo, accent: Color32) -> Option<ToolAct> {
    let mut act = None;
    if !c.tools.lists_read {
        act = Some(ToolAct::ReadLists);
    }
    let weak = ui.visuals().weak_text_color();
    egui::ScrollArea::vertical().id_salt(("claude_session", c.id)).auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        card(ui, t!("Session", "Session"), |ui| {
            let (tokens, window) = c.context;
            let used = if window > 0 { tokens as f32 / window as f32 } else { 0.0 };
            let value = if window > 0 { format!("{:.0} %  ·  {:.2} $", used * 100.0, c.cost) } else { format!("–  ·  {:.2} $", c.cost) };
            let tip = if window > 0 {
                tf!("{} / {} tokens lus par Claude pour sa dernière réponse ; plein, il résume le début de la conversation. Le coût est une estimation.", "{} / {} tokens read by Claude for its last answer; when full, it summarizes the start of the conversation. The cost is an estimate.", thousands(tokens), thousands(window))
            } else {
                t!("Connu à la première réponse de Claude", "Known at Claude's first answer").to_string()
            };
            row(ui, t!("Contexte", "Context"), value).on_hover_text(tip);
            bar(ui, used, accent);
            for (window, label) in [("five_hour", t!("Quota 5 h", "5 h quota")), ("seven_day", t!("Quota 7 j", "7 d quota"))] {
                match info.limits.iter().find(|l| l.0 == window) {
                    Some((_, used, reset)) => {
                        // its window is over since the last answer: back to zero
                        let used = if *reset > 0 && *reset <= now_secs() { 0.0 } else { *used };
                        row(ui, label, format!("{:.0} %", used * 100.0)).on_hover_text(tf!("Quota de ton abonnement, remis à zéro {}", "Quota of your subscription, reset {}", until(*reset)));
                        bar(ui, used, accent);
                    }
                    None => {
                        row(ui, label, RichText::new("–").color(weak)).on_hover_text(t!("Quota de ton abonnement : connu à la première réponse de Claude", "Quota of your subscription: known at Claude's first answer"));
                    }
                }
            }
        });
        card(ui, t!("Projet", "Project"), |ui| {
            let color = project_color(&info.project, ui.visuals().dark_mode);
            let name = if info.project.is_empty() { t!("(sans dossier)", "(no folder)").to_string() } else { info.project.clone() };
            ui.label(RichText::new(format!("● {name}")).color(color).strong()).on_hover_text(c.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_default());
            if let Some(w) = &c.worktree {
                ui.label(format!("🌿 {}", w.branch)).on_hover_text(t!("La conversation travaille dans un worktree, sur sa propre branche", "The conversation works in a worktree, on its own branch"));
            } else if let Some(b) = &info.branch {
                ui.label(format!("🔀 {b}"));
            }
            if !c.add_dirs.is_empty() {
                let dirs = c.add_dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join("\n");
                row(ui, t!("Dossiers en plus", "Extra folders"), c.add_dirs.len().to_string()).on_hover_text(dirs);
            }
            let (alias, label) = claude::MODELS[c.model];
            let model = if c.model_id.is_empty() || !claude::model_is(&c.model_id, alias) { claude::label(label).to_string() } else { claude::model_name(&c.model_id) };
            row(ui, t!("Modèle", "Model"), model);
            row(ui, t!("Mode", "Mode"), claude::label(claude::MODES[c.mode].1));
        });
        card(ui, t!("Claude Code", "Claude Code"), |ui| {
            ui.horizontal(|ui| {
                ui.label(t!("Mémoire", "Memory"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(t!("perso", "mine")).on_hover_text(t!("Ma mémoire perso (~/.claude/CLAUDE.md), lue dans tous les projets", "My personal memory (~/.claude/CLAUDE.md), read in every project")).clicked() {
                        act = Some(ToolAct::Open(claude_home().join("CLAUDE.md"), ""));
                    }
                    if let Some(d) = &c.dir {
                        if ui.small_button(t!("projet", "project")).on_hover_text(t!("CLAUDE.md du projet : ce que Claude lit au début de chaque conversation", "CLAUDE.md of the project: what Claude reads at the start of every conversation")).clicked() {
                            act = Some(ToolAct::Open(d.join("CLAUDE.md"), ""));
                        }
                    }
                });
            });
            let agents = c.tools.agents.iter().map(|(n, ..)| n.as_str()).collect::<Vec<_>>().join(", ");
            row(ui, t!("Agents", "Agents"), c.tools.agents.len().to_string()).on_hover_text(if agents.is_empty() { t!("Aucun agent (⋯ → Agents pour en créer un)", "No agent (⋯ → Agents to create one)").to_string() } else { agents });
            row(ui, t!("Commandes", "Commands"), info.commands.to_string()).on_hover_text(t!("Tape / dans le champ pour les voir", "Type / in the field to see them"));
            let t = &c.tools;
            ui.horizontal(|ui| {
                ui.label(t!("Serveurs MCP", "MCP servers"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if t.mcp_busy {
                        spinner(ui);
                    } else if ui.small_button("⟳").on_hover_text(t!("Lire la liste (claude mcp list : démarre chaque serveur pour le vérifier, quelques secondes)", "Read the list (claude mcp list: starts each server to check it, a few seconds)")).clicked() {
                        act = Some(ToolAct::McpList);
                    }
                    if let Some(Ok(list)) = &t.mcp {
                        let up = list.iter().filter(|m| mcp_state(&m.status).1 == Some(true)).count();
                        ui.label(format!("{up}/{}", list.len())).on_hover_text(t!("Connectés / en tout", "Connected / in all"));
                    }
                });
            });
            match &t.mcp {
                Some(Err(e)) => {
                    ui.label(RichText::new(format!("⚠ {e}")).small().color(err()));
                }
                Some(Ok(list)) if list.is_empty() => {
                    ui.label(RichText::new(t!("Aucun serveur (⋯ → Serveurs MCP pour en ajouter)", "No server (⋯ → MCP servers to add one)")).small().color(weak));
                }
                Some(Ok(list)) => {
                    for m in list {
                        ui.horizontal(|ui| {
                            let rule = format!("mcp__{}", m.name);
                            let mut on = info.allow.contains(&rule);
                            if ui.checkbox(&mut on, &m.name).on_hover_text(tf!("Coché : Claude utilise ses outils sans demander (règle {rule})", "Ticked: Claude uses its tools without asking (rule {rule})")).changed() {
                                act = Some(ToolAct::McpAllow(m.name.clone(), on));
                            }
                            let (status, good) = mcp_state(&m.status);
                            let color = match good {
                                Some(true) => ok(),
                                Some(false) => err(),
                                None => weak,
                            };
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(status).small().color(color)).on_hover_text(&m.target);
                            });
                        });
                    }
                }
                None => {}
            }
        });
        // the last answer: what it read and wrote, and its cost
        let last = c.items.iter().rev().find_map(|i| match i {
            Item::Usage { input, cached, output, cost } => Some((*input, *cached, *output, *cost)),
            _ => None,
        });
        let asked = c.items.iter().filter(|i| matches!(i, Item::User(..))).count();
        if let Some((input, cached, output, cost)) = last {
            card(ui, t!("Dernière réponse", "Last answer"), |ui| {
                row(ui, t!("Lus", "Read"), tf!("{} tokens", "{} tokens", thousands(input))).on_hover_text(tf!("Dont {} lus dans le cache (moins chers)", "Of which {} read from the cache (cheaper)", thousands(cached)));
                row(ui, t!("Écrits", "Written"), tf!("{} tokens", "{} tokens", thousands(output)));
                row(ui, t!("Coût", "Cost"), format!("{cost:.2} $"));
                row(ui, t!("Messages", "Messages"), asked.to_string()).on_hover_text(t!("Tes messages dans cette conversation", "Your messages in this conversation"));
            });
        }
    });
    act
}
