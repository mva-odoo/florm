//! A panel of the Git, pull requests, Docker or Claude view can take all the room of its view
//! (the others are hidden) and give it back: the corner button in its header, or Escape.

use eframe::egui::{self, Id, Rect, Sense, Ui};
use std::collections::HashMap;

/// Per view: the panel taking all its room, and the last frame the view was shown (Escape only
/// gives back the room of a view on screen).
type Maxed = HashMap<Id, (&'static str, u64)>;

fn key() -> Id {
    Id::new("maximized_panels")
}

/// The panel of `view` taking all its room (to call once per frame the view is shown).
pub(super) fn maxed(ctx: &egui::Context, view: Id) -> Option<&'static str> {
    let frame = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Maxed>(key()).get_mut(&view).map(|e| {
            e.1 = frame;
            e.0
        })
    })
}

/// Back to the normal layout of `view` (its maximized panel has nothing to show).
pub(super) fn restore(ctx: &egui::Context, view: Id) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Maxed>(key()).remove(&view));
}

/// Escape: the views on screen get their normal layout back; false when none had a panel maximized.
pub(super) fn restore_shown(ctx: &egui::Context) -> bool {
    let frame = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let m = d.get_temp_mut_or_default::<Maxed>(key());
        let n = m.len();
        m.retain(|_, (_, seen)| *seen + 3 < frame);
        m.len() != n
    })
}

fn toggle(ctx: &egui::Context, view: Id, pane: &'static str) {
    let frame = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let m = d.get_temp_mut_or_default::<Maxed>(key());
        if m.get(&view).is_some_and(|e| e.0 == pane) {
            m.remove(&view);
        } else {
            m.insert(view, (pane, frame));
        }
    });
    ctx.request_repaint();
}

/// The button in a row of the panel (a header).
pub(super) fn button(ui: &mut Ui, view: Id, pane: &'static str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
    button_at(ui, rect, view, pane);
}

/// The button over the top right corner of a panel without a header: only while the pointer is on
/// the panel (or while it takes all the room, to give it back).
pub(super) fn corner(ui: &mut Ui, view: Id, pane: &'static str) {
    let r = ui.max_rect();
    let on = maxed(ui.ctx(), view) == Some(pane);
    if !on && !ui.rect_contains_pointer(r) {
        return;
    }
    // left of the scroll bar
    let rect = Rect::from_min_size(egui::pos2(r.right() - 34.0, r.top() + 4.0), egui::vec2(20.0, 20.0));
    ui.painter().rect_filled(rect.expand(2.0), 4.0, ui.visuals().panel_fill);
    button_at(ui, rect, view, pane);
}

pub(super) fn button_at(ui: &mut Ui, rect: Rect, view: Id, pane: &'static str) {
    let on = maxed(ui.ctx(), view) == Some(pane);
    let r = ui.interact(rect, view.with(("maximize", pane)), Sense::click());
    // four corners: pointing out (take all the room) or in (give it back)
    crate::icons::paint_layout_button(ui, rect, &r, if on { crate::icons::Layout::Restore } else { crate::icons::Layout::Maximize });
    let tip = if on { t!("Revenir à la disposition normale (Échap)", "Back to the normal layout (Escape)") } else { t!("Agrandir ce panneau : il prend toute la place de la vue (Échap pour revenir)", "Maximize this panel: it takes the whole view (Escape to come back)") };
    if r.on_hover_text(tip).clicked() {
        toggle(ui.ctx(), view, pane);
    }
}
