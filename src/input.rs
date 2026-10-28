//! The look of the one-line text fields: room around the text, rounded corners, a thin border
//! that lights up under the pointer and takes the accent color while typing.

use eframe::egui::{self, Margin, Response, Shape, Stroke, TextEdit, Ui};

/// Room between the border and the text.
const MARGIN: Margin = Margin { left: 9.0, right: 9.0, top: 5.0, bottom: 5.0 };

/// Adds `edit` (a `TextEdit::singleline`, set up as usual) with the look of Florm's fields; the
/// response is the text field's own.
pub fn add(ui: &mut Ui, edit: TextEdit) -> Response {
    // the frame goes behind the text: its place is kept, it is painted once the state is known
    let behind = ui.painter().add(Shape::Noop);
    let ir = egui::Frame::none().inner_margin(MARGIN).show(ui, |ui| ui.add(edit.frame(false).margin(Margin::ZERO)));
    let (r, rect) = (ir.inner, ir.response.rect);
    let v = ui.visuals();
    let stroke = if r.has_focus() {
        Stroke::new(1.5_f32, v.selection.stroke.color)
    } else if ui.rect_contains_pointer(rect) {
        Stroke::new(1.0_f32, v.widgets.active.bg_fill)
    } else {
        Stroke::new(1.0_f32, v.widgets.hovered.bg_fill)
    };
    let fill = if r.enabled() { v.extreme_bg_color } else { v.faint_bg_color };
    ui.painter().set(behind, Shape::rect_filled(rect, 7.0, fill));
    ui.painter().rect_stroke(rect, 7.0, stroke);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roomier_than_egui_s_own() {
        let ctx = egui::Context::default();
        let (mut a, mut b) = (String::new(), String::new());
        let mut heights = (0.0, 0.0);
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                heights.0 = ui.add(TextEdit::singleline(&mut a)).rect.height();
                let r = add(ui, TextEdit::singleline(&mut b).desired_width(100.0));
                heights.1 = r.rect.height() + MARGIN.top + MARGIN.bottom;
            });
        });
        assert!(heights.1 > heights.0 + 4.0, "{heights:?}");
    }
}
