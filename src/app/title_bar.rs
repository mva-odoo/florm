//! Florm's own title bar (as VS Code): the menus, the title and ─ □ ✕ inside the window, which
//! has no system title bar then; dragging the bar moves the window, its edges resize it.

use super::*;
use egui::viewport::ResizeDirection;
use egui::{Pos2, Rect, Stroke, ViewportCommand};

/// Height of the bar.
pub(super) const TITLE_H: f32 = 32.0;
/// Width of a window button (as Windows draws them).
const BUTTON_W: f32 = 46.0;
/// Pointer this close to an edge of the window: resizes it.
const EDGE: f32 = 5.0;
/// Corners take a bit more (easier to catch).
const CORNER: f32 = 12.0;

impl App {
    /// Florm draws the title bar (setting on).
    pub(super) fn own_title_bar(&self) -> bool {
        self.settings.custom_title_bar
    }

    /// The window loses (or gets back) the system's title bar when the setting changes.
    pub(super) fn sync_decorations(&mut self, ctx: &egui::Context) {
        let own = self.own_title_bar();
        if own != self.title_bar_on {
            ctx.send_viewport_cmd(ViewportCommand::Decorations(!own));
            self.title_bar_on = own;
        }
    }

    /// The bar: logo and menus at the left (unless hidden), the title in the middle, ─ □ ✕ at the right.
    pub(super) fn title_bar(&mut self, ctx: &egui::Context) {
        let mut pending = Vec::new();
        let panel = ctx.style().visuals.panel_fill;
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let title = self.last_title.clone();
        egui::TopBottomPanel::top("menu")
            .exact_height(TITLE_H)
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin { left: 10.0, right: 0.0, top: 0.0, bottom: 0.0 }))
            .show_separator_line(false)
            .show(ctx, |ui| {
                let bar = ui.max_rect();
                // under everything else of the bar: what is not a menu or a button moves the window
                let drag = ui.interact(bar, egui::Id::new("title_drag"), Sense::click_and_drag());
                if drag.double_clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                } else if drag.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }
                ui.horizontal_centered(|ui| {
                    // the bar of menus fills the width: where its last menu ends
                    let mut menus_end = 0.0;
                    if self.settings.show_menu_bar {
                        egui::menu::bar(ui, |ui| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
                            logo::paint(ui.painter(), r, false);
                            ui.add_space(2.0);
                            self.menus(ui, &mut pending);
                            menus_end = ui.cursor().left();
                        });
                    } else {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
                        logo::paint(ui.painter(), r, false);
                        menus_end = r.right();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let buttons = window_buttons(ui, maximized);
                        // the title in the middle of the window, or between the menus and the
                        // buttons when it does not fit there (cut with "…")
                        let room = (menus_end + 16.0)..=(buttons.left() - 16.0);
                        if room.end() - room.start() > 40.0 {
                            paint_title(ui, &title, bar, *room.start(), *room.end());
                        }
                    });
                });
            });
        self.pending.extend(pending);
    }

    /// The edges of the window resize it (it has no system frame).
    pub(super) fn resize_edges(&self, ctx: &egui::Context) {
        let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
        if maximized || fullscreen {
            return;
        }
        let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        let Some(dir) = edge_at(ctx.screen_rect(), p) else { return };
        use egui::CursorIcon as C;
        let icon = match dir {
            ResizeDirection::North | ResizeDirection::South => C::ResizeVertical,
            ResizeDirection::East | ResizeDirection::West => C::ResizeHorizontal,
            ResizeDirection::NorthWest | ResizeDirection::SouthEast => C::ResizeNwSe,
            ResizeDirection::NorthEast | ResizeDirection::SouthWest => C::ResizeNeSw,
        };
        ctx.set_cursor_icon(icon);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

/// The edge (or corner) of the window `screen` under the pointer.
fn edge_at(screen: Rect, p: Pos2) -> Option<ResizeDirection> {
    let near = |d: f32, m: f32| (0.0..m).contains(&d);
    let (l, r, t, b) = (p.x - screen.left(), screen.right() - p.x, p.y - screen.top(), screen.bottom() - p.y);
    use ResizeDirection as D;
    Some(match () {
        _ if near(t, EDGE) && near(l, CORNER) || near(l, EDGE) && near(t, CORNER) => D::NorthWest,
        _ if near(t, EDGE) && near(r, CORNER) || near(r, EDGE) && near(t, CORNER) => D::NorthEast,
        _ if near(b, EDGE) && near(l, CORNER) || near(l, EDGE) && near(b, CORNER) => D::SouthWest,
        _ if near(b, EDGE) && near(r, CORNER) || near(r, EDGE) && near(b, CORNER) => D::SouthEast,
        _ if near(t, EDGE) => D::North,
        _ if near(b, EDGE) => D::South,
        _ if near(l, EDGE) => D::West,
        _ if near(r, EDGE) => D::East,
        _ => return None,
    })
}

/// The window title centered on the bar when it fits between `left` and `right`, else just after `left`.
fn paint_title(ui: &egui::Ui, title: &str, bar: Rect, left: f32, right: f32) {
    let color = ui.visuals().weak_text_color();
    let mut job = LayoutJob::simple_singleline(title.to_string(), egui::FontId::proportional(12.5), color);
    job.wrap = egui::text::TextWrapping { max_width: right - left, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let g = ui.painter().layout_job(job);
    let w = g.size().x;
    let x = (bar.center().x - w / 2.0).clamp(left, (right - w).max(left));
    ui.painter().galley(Pos2::new(x, bar.center().y - g.size().y / 2.0), g, color);
}

/// ✕ □ ─ from the right (the layout is right to left); the rect they take.
fn window_buttons(ui: &mut egui::Ui, maximized: bool) -> Rect {
    let h = ui.available_height();
    let mut all = Rect::NOTHING;
    for kind in [Button::Close, Button::Maximize, Button::Minimize] {
        let (rect, r) = ui.allocate_exact_size(egui::vec2(BUTTON_W, h), Sense::click());
        all = all.union(rect);
        let v = ui.visuals();
        let close = matches!(kind, Button::Close);
        let fg = if r.hovered() && close { Color32::WHITE } else if r.hovered() { v.strong_text_color() } else { v.text_color() };
        if r.hovered() {
            let fill = if close {
                if r.is_pointer_button_down_on() { Color32::from_rgb(0xF1, 0x70, 0x7A) } else { Color32::from_rgb(0xE8, 0x11, 0x23) }
            } else {
                v.widgets.hovered.weak_bg_fill
            };
            ui.painter().rect_filled(rect, 0.0, fill);
        }
        paint_button(ui.painter(), rect.center(), kind, maximized, fg);
        let tip = match kind {
            Button::Close => t!("Fermer", "Close"),
            Button::Maximize if maximized => t!("Restaurer", "Restore"),
            Button::Maximize => t!("Agrandir", "Maximize"),
            Button::Minimize => t!("Réduire", "Minimize"),
        };
        if r.on_hover_text(tip).clicked() {
            ui.ctx().send_viewport_cmd(match kind {
                Button::Close => ViewportCommand::Close,
                Button::Maximize => ViewportCommand::Maximized(!maximized),
                Button::Minimize => ViewportCommand::Minimized(true),
            });
        }
    }
    all
}

#[derive(Clone, Copy)]
enum Button {
    Minimize,
    Maximize,
    Close,
}

/// The thin glyphs of the window buttons (10 points wide, as Windows 11).
fn paint_button(p: &egui::Painter, c: Pos2, kind: Button, maximized: bool, color: Color32) {
    let s = Stroke::new(1.0_f32, color);
    let r = 5.0;
    match kind {
        Button::Minimize => {
            p.line_segment([Pos2::new(c.x - r, c.y + 0.5), Pos2::new(c.x + r, c.y + 0.5)], s);
        }
        Button::Maximize if maximized => {
            // two squares: the front one and the corner of the one behind
            let front = Rect::from_min_max(Pos2::new(c.x - r, c.y - r + 2.0), Pos2::new(c.x + r - 2.0, c.y + r));
            p.rect_stroke(front, 1.0, s);
            p.line_segment([Pos2::new(c.x - r + 2.0, c.y - r + 2.0), Pos2::new(c.x - r + 2.0, c.y - r)], s);
            p.line_segment([Pos2::new(c.x - r + 2.0, c.y - r), Pos2::new(c.x + r, c.y - r)], s);
            p.line_segment([Pos2::new(c.x + r, c.y - r), Pos2::new(c.x + r, c.y + r - 2.0)], s);
            p.line_segment([Pos2::new(c.x + r, c.y + r - 2.0), Pos2::new(c.x + r - 2.0, c.y + r - 2.0)], s);
        }
        Button::Maximize => {
            p.rect_stroke(Rect::from_center_size(c, egui::vec2(2.0 * r, 2.0 * r)), 1.0, s);
        }
        Button::Close => {
            p.line_segment([Pos2::new(c.x - r, c.y - r), Pos2::new(c.x + r, c.y + r)], s);
            p.line_segment([Pos2::new(c.x - r, c.y + r), Pos2::new(c.x + r, c.y - r)], s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_and_corners_resize() {
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        assert!(matches!(edge_at(screen, Pos2::new(400.0, 2.0)), Some(ResizeDirection::North)));
        assert!(matches!(edge_at(screen, Pos2::new(798.0, 300.0)), Some(ResizeDirection::East)));
        assert!(matches!(edge_at(screen, Pos2::new(8.0, 598.0)), Some(ResizeDirection::SouthWest)));
        assert!(matches!(edge_at(screen, Pos2::new(797.0, 3.0)), Some(ResizeDirection::NorthEast)));
        assert!(edge_at(screen, Pos2::new(400.0, 300.0)).is_none());
    }
}
