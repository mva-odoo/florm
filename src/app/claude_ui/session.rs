//! The session of a conversation, in the panel on its right: its context and what it cost, the
//! quotas of the plan, its folder, branch, account and model (from the status line of Claude Code).

use super::*;

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
    egui::Frame::none().fill(fill).rounding(8.0).inner_margin(egui::Margin::symmetric(10.0, 7.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new(title).small().strong().color(ui.visuals().weak_text_color()));
        add(ui);
    });
    ui.add_space(6.0);
}

/// The panel.
/// Returns the file changed by Claude clicked in its list (to open).
/// Returns the file changed by Claude clicked in its list (to open), and whether 🗺 was clicked.
pub(super) fn session_ui(ui: &mut egui::Ui, c: &ClaudeUi, project: &str, branch: Option<&str>, limits: &[(String, f32, i64)], accent: Color32) -> (Option<PathBuf>, bool) {
    let weak = ui.visuals().weak_text_color();
    let mut open = None;
    let mut explain = false;
    egui::ScrollArea::vertical().id_salt(("claude_session", c.id)).auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        card(ui, t!("Utilisation", "Usage"), |ui| {
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
                match limits.iter().find(|l| l.0 == window) {
                    Some((_, used, reset)) => {
                        // its window is over since: back to zero
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
            let color = project_color(project, ui.visuals().dark_mode);
            let name = if project.is_empty() { t!("(sans dossier)", "(no folder)") } else { project };
            ui.label(RichText::new(format!("● {name}")).color(color).strong()).on_hover_text(c.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_default());
            if let Some(b) = branch {
                ui.label(format!("🔀 {b}"));
            }
            // the model its status line gives (changed in Claude with /model)
            let model = if c.term_state.model.is_empty() { "–" } else { c.term_state.model.as_str() };
            row(ui, t!("Modèle", "Model"), model).on_hover_text(t!("Se change dans Claude : /model", "Changed in Claude: /model"));
        });
        // what it changed, the last first: a click opens it
        if !c.changed.is_empty() {
            card(ui, &tf!("Fichiers modifiés ({})", "Files changed ({})", c.changed.len()), |ui| {
                for p in c.changed.iter().rev() {
                    let rel = c.dir.as_deref().and_then(|d| p.strip_prefix(d).ok()).unwrap_or(p);
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let gone = !p.exists();
                    let text = if gone { RichText::new(format!("{name} ×")).color(weak) } else { RichText::new(name) };
                    let tip = if gone { tf!("{} (supprimé depuis)", "{} (deleted since)", rel.display()) } else { rel.display().to_string() };
                    if ui.add(egui::Label::new(text).truncate().sense(Sense::click())).on_hover_text(tip).clicked() && !gone {
                        open = Some(p.clone());
                    }
                }
                let tip = t!("Claude explique ici les changements non commités du dossier, idée par idée", "Claude explains here the uncommitted changes of the folder, idea by idea");
                if ui.small_button(t!("🗺 Expliquer ces changements", "🗺 Explain these changes")).on_hover_text(tip).clicked() {
                    explain = true;
                }
            });
        }
        ui.label(RichText::new(t!("Dans Claude : /model, /mcp, /memory, /agents, /usage, Maj+Tab pour le mode", "In Claude: /model, /mcp, /memory, /agents, /usage, Shift+Tab for the mode")).small().color(weak));
    });
    (open, explain)
}
