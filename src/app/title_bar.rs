//! The window without the system's title bar (as Zed): the bar at the top is the tab strip, which
//! goes on over the sidebar; ─ □ ✕ at its end (round buttons at its start on macOS, none under a
//! tiling window manager); dragging it moves the window, the window's edges resize it.

use super::*;
use crate::os::{self, Controls};
use egui::viewport::ResizeDirection;
use egui::{Color32, Pos2, Rect, Stroke, ViewportCommand};

/// Radius of the window's corners when it does not fill the screen.
/// The corners of the window: square, as in Zed.
const WINDOW_ROUNDING: f32 = 0.0;

/// Makes the pixels outside the rounded corners transparent (blending cannot erase them: GL
/// clears, row by row, through the scissor). The border drawn after smooths the edge.
fn clear_corners(radius: f32) -> eframe::egui_glow::CallbackFn {
    eframe::egui_glow::CallbackFn::new(move |info, painter| {
        use eframe::glow::HasContext as _;
        let gl = painter.gl();
        let [w, h] = info.screen_size_px;
        let (w, h) = (w as i32, h as i32);
        let rad = (radius * info.pixels_per_point).round();
        unsafe {
            gl.enable(eframe::glow::SCISSOR_TEST);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            for dy in 0..rad as i32 {
                // pixels of this row whose centre is outside the circle
                let v = rad - (dy as f32 + 0.5);
                let n = (rad - (rad * rad - v * v).max(0.0).sqrt() - 0.5).ceil().max(0.0) as i32;
                if n == 0 {
                    continue;
                }
                // GL rows count from the bottom
                for y in [dy, h - 1 - dy] {
                    for x in [0, w - n] {
                        gl.scissor(x, y, n, 1);
                        gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                    }
                }
            }
        }
    })
}

/// Width of a window button (as Windows draws them).
const BUTTON_W: f32 = 46.0;
/// Room the three buttons take at the right of the tab strip (menus hidden, Windows and Linux).
pub(super) const WINDOW_BUTTONS_W: f32 = 3.0 * BUTTON_W;
/// Room the round buttons take at the left of the tab strip (menus hidden, macOS).
pub(super) const MAC_BUTTONS_W: f32 = 72.0;
/// Pointer this close to an edge of the window: resizes it.
const EDGE: f32 = 5.0;
/// Corners take a bit more (easier to catch).
const CORNER: f32 = 12.0;

impl App {
    /// Florm draws the title bar (see `os::draws_title_bar`).
    pub(super) fn own_title_bar(&self) -> bool {
        os::draws_title_bar()
    }

    /// The window buttons in the tab strip of `pane` (None: the only one): at the right end of the
    /// last one (Windows, Linux), at the left of the first one (macOS) when the sidebar is hidden.
    pub(super) fn strip_controls(&self, pane: Option<u8>) -> Option<Controls> {
        let c = os::window_controls();
        let here = match (c, pane) {
            (Controls::Side, None | Some(1)) => true,
            (Controls::Mac, None | Some(0)) => !self.show_sidebar,
            _ => false,
        };
        here.then_some(c)
    }

    /// The window buttons at the top of the sidebar: macOS (the top left corner of the window).
    pub(super) fn top_controls(&self, _sidebar: bool) -> Option<Controls> {
        (os::window_controls() == Controls::Mac).then_some(Controls::Mac)
    }

    /// The window's edge drawn by Florm: a thin line in a grey touched by the accent, a lighter one
    /// just inside, so the window stands out from what is behind (a light theme on a light desktop,
    /// a tiling window manager without title bars). None in full screen.
    pub(super) fn window_border(&self, ctx: &egui::Context) {
        let (fullscreen, maximized) = ctx.input(|i| (i.viewport().fullscreen.unwrap_or(false), i.viewport().maximized.unwrap_or(false)));
        if fullscreen {
            return;
        }
        // rounded corners when the window does not fill the screen and has no system decorations;
        // a tiling compositor (Hyprland…) says « maximized » for every window: rounded there too
        let round = (!maximized || os::window_controls() == Controls::None)
            && os::rounded_window()
            && os::TRANSPARENT_WINDOW.load(std::sync::atomic::Ordering::Relaxed);
        let radius = if round { WINDOW_ROUNDING } else { 0.0 };
        let tc = self.hl.tab_colors();
        let r = ctx.content_rect();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::unique("window_border")));
        if radius > 0.0 {
            p.add(egui::PaintCallback { rect: r, callback: std::sync::Arc::new(clear_corners(radius)) });
        }
        let light = self.hl.is_light();
        let edge = crate::icons::mix(crate::icons::mix(tc.bar, tc.text_inactive, if light { 0.45 } else { 0.7 }), tc.accent, 0.45);
        p.rect_stroke(r.shrink(0.75), (radius - 0.75).max(0.0), Stroke::new(1.5_f32, edge), egui::StrokeKind::Middle);
        let inner = if light { Color32::from_white_alpha(170) } else { Color32::from_white_alpha(22) };
        p.rect_stroke(r.shrink(2.0), (radius - 2.0).max(0.0), Stroke::new(1.0_f32, inner), egui::StrokeKind::Middle);
    }

    /// The window loses (or gets back) the system's title bar when the setting changes.
    pub(super) fn sync_decorations(&mut self, ctx: &egui::Context) {
        let own = self.own_title_bar();
        if own != self.title_bar_on {
            ctx.send_viewport_cmd(ViewportCommand::Decorations(!own));
            self.title_bar_on = own;
        }
    }

    /// The edges of the window resize it (it has no system frame).
    pub(super) fn resize_edges(&self, ctx: &egui::Context) {
        let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
        if maximized || fullscreen {
            return;
        }
        let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        let Some(dir) = edge_at(ctx.content_rect(), p) else { return };
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

/// A drag on a bar moves the window; a double click maximizes it.
pub(super) fn drag_window(ctx: &egui::Context, resp: &egui::Response, maximized: bool) {
    if resp.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if resp.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// The window buttons at the end of the tab strip (`bar`), when the menus are hidden.
pub(super) fn strip_buttons(ui: &mut egui::Ui, bar: Rect, controls: Controls, maximized: bool) {
    match controls {
        Controls::Side => {
            let r = Rect::from_min_max(Pos2::new(bar.right() - WINDOW_BUTTONS_W, bar.top()), bar.right_bottom());
            ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    window_buttons(ui, maximized);
                });
            });
        }
        Controls::Mac => {
            let r = Rect::from_min_max(Pos2::new(bar.left() + 10.0, bar.top()), Pos2::new(bar.left() + MAC_BUTTONS_W, bar.bottom()));
            let ctx = ui.ctx().clone();
            ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                ui.horizontal_centered(|ui| mac_buttons(ui, &ctx, maximized));
            });
        }
        Controls::None => {}
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

/// ✕ □ ─ from the right (the layout is right to left).
fn window_buttons(ui: &mut egui::Ui, maximized: bool) {
    let h = ui.available_height();
    for kind in [Button::Close, Button::Maximize, Button::Minimize] {
        let (rect, r) = ui.allocate_exact_size(egui::vec2(BUTTON_W, h), Sense::click());
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
            send(ui.ctx(), kind, maximized);
        }
    }
}

/// macOS: round buttons at the left, in the system's colors.
fn mac_buttons(ui: &mut egui::Ui, ctx: &egui::Context, maximized: bool) {
    ui.spacing_mut().item_spacing.x = 8.0;
    for (kind, color, tip) in [
        (Button::Close, Color32::from_rgb(0xFF, 0x5F, 0x57), t!("Fermer", "Close")),
        (Button::Minimize, Color32::from_rgb(0xFE, 0xBC, 0x2E), t!("Réduire", "Minimize")),
        (Button::Maximize, Color32::from_rgb(0x28, 0xC8, 0x40), if maximized { t!("Restaurer", "Restore") } else { t!("Agrandir", "Maximize") }),
    ] {
        let (rect, r) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::click());
        ui.painter().circle_filled(rect.center(), 6.0, color);
        if r.on_hover_text(tip).clicked() {
            send(ctx, kind, maximized);
        }
    }
}

/// What a window button does.
fn send(ctx: &egui::Context, kind: Button, maximized: bool) {
    ctx.send_viewport_cmd(match kind {
        Button::Close => ViewportCommand::Close,
        Button::Maximize => ViewportCommand::Maximized(!maximized),
        Button::Minimize => ViewportCommand::Minimized(true),
    });
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
            p.rect_stroke(front, 1.0, s, egui::StrokeKind::Middle);
            p.line_segment([Pos2::new(c.x - r + 2.0, c.y - r + 2.0), Pos2::new(c.x - r + 2.0, c.y - r)], s);
            p.line_segment([Pos2::new(c.x - r + 2.0, c.y - r), Pos2::new(c.x + r, c.y - r)], s);
            p.line_segment([Pos2::new(c.x + r, c.y - r), Pos2::new(c.x + r, c.y + r - 2.0)], s);
            p.line_segment([Pos2::new(c.x + r, c.y + r - 2.0), Pos2::new(c.x + r - 2.0, c.y + r - 2.0)], s);
        }
        Button::Maximize => {
            p.rect_stroke(Rect::from_center_size(c, egui::vec2(2.0 * r, 2.0 * r)), 1.0, s, egui::StrokeKind::Middle);
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
