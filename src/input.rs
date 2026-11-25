//! The look of the one-line text fields: room around the text, rounded corners, a thin border
//! that lights up under the pointer and takes the accent color while typing.

use eframe::egui::{self, Margin, Response, Shape, Stroke, TextEdit, Ui};

/// Room between the border and the text.
const MARGIN: Margin = Margin { left: 9, right: 9, top: 5, bottom: 5 };

/// Adds `edit` (a `TextEdit::singleline`, set up as usual) with the look of Florm's fields; the
/// response is the text field's own.
pub fn add(ui: &mut Ui, edit: TextEdit) -> Response {
    // the frame goes behind the text: its place is kept, it is painted once the state is known
    let behind = ui.painter().add(Shape::Noop);
    let ir = egui::Frame::NONE.inner_margin(MARGIN).show(ui, |ui| ui.add(edit.frame(egui::Frame::NONE)));
    let (r, rect) = (ir.inner, ir.response.rect);
    let v = ui.visuals();
    // as in Zed: the line color, the accent once it has the keyboard, a little rounded
    let stroke = if r.has_focus() {
        Stroke::new(1.0_f32, v.selection.stroke.color)
    } else if ui.rect_contains_pointer(rect) {
        Stroke::new(1.0_f32, v.widgets.active.bg_fill)
    } else {
        Stroke::new(1.0_f32, v.widgets.noninteractive.bg_stroke.color)
    };
    let fill = if r.enabled() { v.extreme_bg_color } else { v.faint_bg_color };
    ui.painter().set(behind, Shape::rect_filled(rect, 4.0, fill));
    ui.painter().rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Middle);
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
        let _ = crate::run_ui(&ctx, Default::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                heights.0 = ui.add(TextEdit::singleline(&mut a)).rect.height();
                let r = add(ui, TextEdit::singleline(&mut b).desired_width(100.0));
                heights.1 = r.rect.height() + MARGIN.sum().y;
            });
        });
        assert!(heights.1 > heights.0 + 4.0, "{heights:?}");
    }
}
