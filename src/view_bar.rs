//! What the views (Recherche, Git, GitHub, Claude, Terminal) share, so that
//! they behave the same: their title, the buttons at the right of their header always in the
//! same order (⟳ read again, ⇱ / ⇲ editor tab or bottom panel for those that can be in both,
//! ⋯ the other actions and options), and the notice in the bottom panel when the view is in an
//! editor tab.

use eframe::egui::{self, RichText};

/// What the buttons at the right of a header asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarAct {
    /// Into an editor tab (true) or into the bottom panel.
    Move(bool),
    Refresh,
}

/// Size of the code's font, for the headers drawn like the breadcrumb of a file (set at each frame).
static FONT_SIZE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_font_size(size: f32) {
    FONT_SIZE.store(size.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// The font of the breadcrumb of a file: the code's, a half point smaller.
pub fn crumb_font() -> egui::FontId {
    let size = f32::from_bits(FONT_SIZE.load(std::sync::atomic::Ordering::Relaxed));
    egui::FontId::monospace(if size > 0.0 { (size - 0.5).max(9.0) } else { 13.5 })
}

/// The header of a view in an editor tab, as the breadcrumb of a file: on the editor's
/// background, at least the height of the tab strip, a line under it.
pub fn header(id: impl std::hash::Hash + std::fmt::Debug, ui: &egui::Ui) -> egui::Panel {
    egui::Panel::top(egui::Id::unique(id))
        .min_size(crate::app::TAB_STRIP_H - 8.0)
        .frame(egui::Frame::NONE.fill(ui.visuals().extreme_bg_color).inner_margin(egui::Margin { left: 16, right: 10, top: 4, bottom: 4 }))
}

/// The title of a view, at the left of its header: in the font of the breadcrumb.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(crumb_font()).color(ui.visuals().text_color()));
}

/// « › » between two parts of a header, as in the breadcrumb.
pub fn sep(ui: &mut egui::Ui) {
    ui.label(RichText::new("›").font(crumb_font()).color(ui.visuals().weak_text_color()));
}

/// `add` with buttons without a frame until hovered, as the folders of the breadcrumb.
pub fn flat<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for s in [&mut w.inactive, &mut w.open] {
            s.weak_bg_fill = egui::Color32::TRANSPARENT;
            s.bg_fill = egui::Color32::TRANSPARENT;
            s.bg_stroke = egui::Stroke::NONE;
        }
        add(ui)
    })
    .inner
}

/// The right of a header, drawn first in a right-to-left layout: ⋯ at the end, then ⇱ / ⇲ for a
/// view that can be in both places (`place`: Some(true) when it is in an editor tab), then ⟳ when
/// `refresh` gives its tooltip. `more` fills the ⋯ menu (its secondary actions and options). What
/// the caller adds after it goes on their left.
pub fn controls(ui: &mut egui::Ui, place: Option<bool>, refresh: Option<&str>, more: Option<&mut dyn FnMut(&mut egui::Ui)>) -> Option<BarAct> {
    let mut act = None;
    if let Some(more) = more {
        ui.menu_button(RichText::new("⋯").strong(), |ui| {
            ui.set_min_width(240.0);
            more(ui);
        })
        .response
        .on_hover_text(t!("Autres actions et options", "Other actions and options"));
    }
    if let Some(big) = place {
        let (kind, tip) = if big {
            (crate::icons::Layout::ToBottom, t!("Dans le panneau du bas", "In the bottom panel"))
        } else {
            (crate::icons::Layout::ToTab, t!("En grand, dans un onglet de l'éditeur", "Large, in an editor tab"))
        };
        if crate::icons::layout_button(ui, kind, tip).clicked() {
            act = Some(BarAct::Move(!big));
        }
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
pub fn repo_chip(ui: &mut egui::Ui, id_salt: impl std::hash::Hash + std::fmt::Debug, name: &str, items: &[String], current: Option<usize>) -> Option<usize> {
    let text = RichText::new(name).font(crumb_font()).color(ui.visuals().hyperlink_color);
    if items.len() > 1 {
        return flat(ui, |ui| crate::pick::combo(ui, id_salt, text, items, current));
    }
    // a single repository: nothing to pick here, say where to go instead
    ui.label(text).on_hover_text(t!(
        "Le seul dépôt du dossier ouvert. Pour un autre projet : Fichier → Changer de projet…",
        "The only repository of the opened folder. For another project: File → Switch project…"
    ));
    None
}
