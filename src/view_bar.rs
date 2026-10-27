//! What the views (Recherche, Git, GitHub, Docker, Claude, Débogage, Terminal) share, so that
//! they behave the same: their title, the buttons at the right of their header always in the
//! same order (⟳ read again, ⇱ / ⇲ editor tab or bottom panel, ⋯ the other actions and options),
//! and the notice in the bottom panel when the view is in an editor tab.

use eframe::egui::{self, RichText};

/// What the buttons at the right of a header asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarAct {
    /// Into an editor tab (true) or into the bottom panel.
    Move(bool),
    Refresh,
}

/// The title of a view, at the left of its header.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong().size(15.0));
}

/// The right of a header, drawn first in a right-to-left layout: ⋯ at the end, then ⇱ / ⇲ (`big`:
/// the view is in an editor tab), then ⟳ when `refresh` gives its tooltip. `more` fills the ⋯
/// menu (its secondary actions and options). What the caller adds after it goes on their left.
pub fn controls(ui: &mut egui::Ui, big: bool, refresh: Option<&str>, more: Option<&mut dyn FnMut(&mut egui::Ui)>) -> Option<BarAct> {
    let mut act = None;
    if let Some(more) = more {
        ui.menu_button(RichText::new("⋯").strong(), |ui| {
            ui.set_min_width(240.0);
            more(ui);
        })
        .response
        .on_hover_text(t!("Autres actions et options", "Other actions and options"));
    }
    let (label, tip) = if big {
        ("⇲", t!("Dans le panneau du bas", "In the bottom panel"))
    } else {
        ("⇱", t!("En grand, dans un onglet de l'éditeur", "Large, in an editor tab"))
    };
    if ui.small_button(label).on_hover_text(tip).clicked() {
        act = Some(BarAct::Move(!big));
    }
    if let Some(tip) = refresh {
        if ui.small_button("⟳").on_hover_text(tip).clicked() {
            act = Some(BarAct::Refresh);
        }
    }
    act
}

/// In the bottom panel, for a view shown in an editor tab: the same sentence for each, and a
/// button to bring it back. True when clicked.
pub fn in_tab_notice(ui: &mut egui::Ui) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(t!("Cette vue est dans un onglet de l'éditeur.", "This view is in an editor tab.")).weak());
        ui.button(t!("La ramener ici", "Bring it back here")).clicked()
    })
    .inner
}

/// The repository (or folder) a view works on, large and tinted at the left of its header, so it is not
/// mistaken for another. With several, a click lists them (filterable); returns the index chosen.
pub fn repo_chip(ui: &mut egui::Ui, id_salt: impl std::hash::Hash, name: &str, items: &[String], current: Option<usize>) -> Option<usize> {
    let accent = ui.visuals().hyperlink_color;
    let text = RichText::new(name).strong().size(14.0).color(accent);
    if items.len() > 1 {
        return crate::pick::combo(ui, id_salt, text, items, current);
    }
    let fill = crate::icons::mix(ui.visuals().panel_fill, accent, 0.15);
    egui::Frame::none().fill(fill).rounding(5.0).inner_margin(egui::Margin::symmetric(8.0, 2.0)).show(ui, |ui| ui.label(text));
    None
}
