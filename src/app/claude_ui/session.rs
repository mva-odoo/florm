//! The session of a conversation, in the list of the conversations: under the one shown, its
//! context, what it cost and its model; at the bottom, the quotas of the plan (from the status
//! line of Claude Code).

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

/// A line: its label on the left, its value on the right.
fn row(ui: &mut egui::Ui, label: &str, value: impl Into<egui::WidgetText>) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).small());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(value)).inner
    })
    .inner
}

/// The details of the conversation shown, under its row in the list: its context and cost, its
/// model and account.
pub(super) fn details_ui(ui: &mut egui::Ui, c: &ClaudeUi, account: &str, accent: Color32) {
    egui::Frame::NONE.inner_margin(egui::Margin { left: 12, right: 10, top: 0, bottom: 8 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 4.0;
        let (tokens, window) = c.context;
        let used = if window > 0 { tokens as f32 / window as f32 } else { 0.0 };
        let value = if window > 0 { format!("{:.0} %  ·  {:.2} $", used * 100.0, c.cost) } else { format!("–  ·  {:.2} $", c.cost) };
        let tip = if window > 0 {
            tf!("{} / {} tokens lus par Claude pour sa dernière réponse ; plein, il résume le début de la conversation (/compact). Le coût est une estimation.", "{} / {} tokens read by Claude for its last answer; when full, it summarizes the start of the conversation (/compact). The cost is an estimate.", thousands(tokens), thousands(window))
        } else {
            t!("Connu à la première réponse de Claude", "Known at Claude's first answer").to_string()
        };
        row(ui, t!("Contexte", "Context"), RichText::new(value).small()).on_hover_text(tip);
        bar(ui, used, accent);
        // the model its status line gives (changed in Claude with /model)
        let model = if c.term_state.model.is_empty() { "–" } else { c.term_state.model.as_str() };
        row(ui, t!("Modèle", "Model"), RichText::new(model).small()).on_hover_text(t!("Se change dans Claude : /model", "Changed in Claude: /model"));
        if !account.is_empty() {
            row(ui, t!("Compte", "Account"), RichText::new(account).small());
        }
    });
}

/// The quotas of the plan (over 5 hours and 7 days), at the bottom of the list: they are the
/// account's, not one conversation's.
pub(super) fn quotas_ui(ui: &mut egui::Ui, limits: &[(String, f32, i64)], accent: Color32) {
    let weak = ui.visuals().weak_text_color();
    ui.spacing_mut().item_spacing.y = 3.0;
    ui.add_space(6.0);
    ui.label(RichText::new(t!("UTILISATION", "USAGE")).small().strong().color(weak));
    for (window, label) in [("five_hour", t!("Quota 5 h", "5 h quota")), ("seven_day", t!("Quota 7 j", "7 d quota"))] {
        match limits.iter().find(|l| l.0 == window) {
            Some((_, used, reset)) => {
                // its window is over since: back to zero
                let used = if *reset > 0 && *reset <= now_secs() { 0.0 } else { *used };
                row(ui, label, RichText::new(format!("{:.0} %", used * 100.0)).small()).on_hover_text(tf!("Quota de ton abonnement, remis à zéro {}", "Quota of your subscription, reset {}", until(*reset)));
                bar(ui, used, accent);
            }
            None => {
                row(ui, label, RichText::new("–").small().color(weak)).on_hover_text(t!("Quota de ton abonnement : connu à la première réponse de Claude", "Quota of your subscription: known at Claude's first answer"));
            }
        }
    }
}
