//! The Florm logo: a gradient "F" drawn as a folded ribbon, as the logos of Sublime Text and
//! VS Code: the stem folds over to the right into the top bar (the back of the ribbon, darker,
//! shows in the fold, and the fold casts a shadow), the middle bar comes out from under the stem.
//! One function gives its color at each point (`shade`): a tiny rasterizer makes the window icon,
//! the packaging icons and the textures of the logo in the app (cached at the size shown).

use eframe::egui::{
    self, Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2,
};

pub const CYAN: Color32 = Color32::from_rgb(0x3b, 0xd5, 0xf5);
pub const BLUE: Color32 = Color32::from_rgb(0x5b, 0x8c, 0xf8);
pub const PURPLE: Color32 = Color32::from_rgb(0x9b, 0x5c, 0xf6);

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Cyan (top-left) → blue → purple (bottom-right); `t` in 0..1.
pub fn gradient(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        lerp(CYAN, BLUE, t * 2.0)
    } else {
        lerp(BLUE, PURPLE, (t - 0.5) * 2.0)
    }
}

/// Convex rounded rectangle in unit coordinates, radii [tl, tr, br, bl].
fn rrect(x0: f32, y0: f32, x1: f32, y1: f32, r: [f32; 4]) -> Vec<Vec2> {
    let corners = [
        (x0 + r[0], y0 + r[0], std::f32::consts::PI, r[0]),
        (x1 - r[1], y0 + r[1], 1.5 * std::f32::consts::PI, r[1]),
        (x1 - r[2], y1 - r[2], 0.0, r[2]),
        (x0 + r[3], y1 - r[3], 0.5 * std::f32::consts::PI, r[3]),
    ];
    let mut pts = Vec::new();
    for (cx, cy, start, rad) in corners {
        if rad <= 0.0 {
            // sharp corner: the arc center is the corner itself
            pts.push(Vec2::new(cx, cy));
            continue;
        }
        for i in 0..=8 {
            let a = start + i as f32 / 8.0 * std::f32::consts::FRAC_PI_2;
            pts.push(Vec2::new(cx + rad * a.cos(), cy + rad * a.sin()));
        }
    }
    pts
}

/// The three convex pieces of the "F" (top bar, stem, middle bar).
fn pieces() -> [Vec<Vec2>; 3] {
    [
        rrect(0.20, 0.04, 0.96, 0.25, [0.10, 0.105, 0.105, 0.0]),
        rrect(0.20, 0.04, 0.42, 0.96, [0.10, 0.0, 0.05, 0.10]),
        rrect(0.20, 0.45, 0.80, 0.64, [0.0, 0.095, 0.095, 0.0]),
    ]
}

fn grad_at(p: Vec2) -> Color32 {
    gradient((p.x * 0.45 + p.y * 0.75) / 1.1)
}

/// `c` darker by `k` (0: as is, 1: black).
fn darken(c: Color32, k: f32) -> Color32 {
    let f = |x: u8| (x as f32 * (1.0 - k.clamp(0.0, 1.0))).round() as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// `c` lighter by `k` (0: as is, 1: white).
fn lighten(c: Color32, k: f32) -> Color32 {
    lerp(c, Color32::WHITE, k.clamp(0.0, 1.0))
}

/// Inside a convex polygon (counterclockwise in screen coordinates, as `rrect` makes them).
fn inside(poly: &[Vec2], p: Vec2) -> bool {
    let n = poly.len();
    (0..n).all(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        (b - a).x * (p - a).y - (b - a).y * (p - a).x >= -1e-6
    })
}

/// Right edge of the stem, where the bars leave it; bottom of the top bar.
const STEM_RIGHT: f32 = 0.42;
const TOP_BOTTOM: f32 = 0.25;

/// 0 before `a`, 1 after `b`, smooth in between.
fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Color of the logo at `p` (unit coordinates), None around the letter.
fn shade(pieces: &[Vec<Vec2>; 3], p: Vec2) -> Option<Color32> {
    let (top, stem, mid) = (inside(&pieces[0], p), inside(&pieces[1], p), inside(&pieces[2], p));
    let base = grad_at(p);
    // the fold, from the outer corner (0.20, 0.04) to the inner one: `d` > 0 above it
    let (ax, ay, bx, by) = (0.20, 0.04, STEM_RIGHT, TOP_BOTTOM);
    let d = ((p.x - ax) * (by - ay) - (p.y - ay) * (bx - ax)) / (bx - ax).hypot(by - ay);
    let in_corner = top && p.x <= STEM_RIGHT;
    if top && (!in_corner || d > 0.0) {
        // the top bar is the back of the ribbon, folded over: dark at the fold, lit further on;
        // a thin light crease along the fold
        if in_corner && d < 0.008 {
            return Some(lighten(base, 0.35));
        }
        return Some(darken(base, 0.40 * (1.0 - smooth(0.22, 0.90, p.x))));
    }
    if stem {
        // the front of the ribbon; the shadow the fold casts on it below the top bar
        let k = if p.y > TOP_BOTTOM { 0.24 * (1.0 - smooth(TOP_BOTTOM, TOP_BOTTOM + 0.10, p.y)) } else { 0.0 };
        return Some(darken(base, k));
    }
    if mid {
        // it comes out from under the stem: in its shadow first
        return Some(darken(base, 0.34 * (1.0 - smooth(STEM_RIGHT, STEM_RIGHT + 0.22, p.x))));
    }
    None
}

/// The letter rasterized `size`×`size`, 4×4 supersampled; `to_unit` maps a point of the image (0..1)
/// to the logo's unit coordinates. RGBA, transparent around the letter.
fn render(size: usize, to_unit: impl Fn(f32, f32) -> Vec2) -> Vec<u8> {
    let pieces = pieces();
    let mut rgba = Vec::with_capacity(size * size * 4);
    let ss = 4;
    for y in 0..size {
        for x in 0..size {
            let (mut r, mut g, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
            for sy in 0..ss {
                for sx in 0..ss {
                    let u = (x as f32 + (sx as f32 + 0.5) / ss as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / ss as f32) / size as f32;
                    if let Some(c) = shade(&pieces, to_unit(u, v)) {
                        r += c.r() as f32;
                        g += c.g() as f32;
                        b += c.b() as f32;
                        a += 255.0;
                    }
                }
            }
            let alpha = a / (ss * ss) as f32;
            let k = if a > 0.0 { 255.0 / a } else { 0.0 };
            rgba.extend([(r * k) as u8, (g * k) as u8, (b * k) as u8, alpha as u8]);
        }
    }
    rgba
}

/// Paints the logo in `rect` (square). `detail` adds the code lines, braces and glow.
pub fn paint(painter: &Painter, rect: Rect, detail: bool) {
    let s = rect.width().min(rect.height());
    let o = rect.center() - Vec2::splat(s / 2.0);
    let at = |p: Vec2| o + p * s;
    if detail {
        // soft glow behind the letter
        for k in (1..=10).map(|i| i as f32 * 0.009) {
            let alpha = 7u8;
            for (i, poly) in pieces().iter().enumerate() {
                let c = Vec2::new(0.5, 0.5);
                let pts: Vec<Pos2> = poly.iter().map(|p| at(c + (*p - c) * (1.0 + k))).collect();
                let col = if i == 2 { PURPLE } else { CYAN };
                painter.add(Shape::convex_polygon(pts, Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), alpha), Stroke::NONE));
            }
        }
    }
    // the letter: a texture rendered at the pixel size shown, kept for the next frames
    let ctx = painter.ctx();
    let px = (s * ctx.pixels_per_point()).round().clamp(8.0, 1024.0) as usize;
    let id = egui::Id::new(("florm_logo", px));
    let tex = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
        let image = egui::ColorImage::from_rgba_unmultiplied([px, px], &render(px, |u, v| Vec2::new(u, v)));
        let tex = ctx.load_texture("florm_logo", image, egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
        tex
    });
    let square = Rect::from_min_size(o, Vec2::splat(s));
    painter.image(tex.id(), square, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    if !detail {
        return;
    }
    // dark "document" with code lines inside the letter
    let doc = Rect::from_min_max(at(Vec2::new(0.25, 0.29)), at(Vec2::new(0.62, 0.86)));
    painter.rect_filled(doc, 0.03 * s, Color32::from_rgba_unmultiplied(0x0b, 0x13, 0x22, 235));
    let lines: [(f32, f32); 7] = [(0.0, 0.10), (0.0, 0.26), (0.05, 0.20), (0.05, 0.12), (0.0, 0.22), (0.05, 0.16), (0.0, 0.08)];
    for (i, (indent, len)) in lines.iter().enumerate() {
        let y = 0.34 + i as f32 * 0.068;
        let x = 0.29 + indent;
        let r = Rect::from_min_max(at(Vec2::new(x, y)), at(Vec2::new(x + len, y + 0.03)));
        painter.rect_filled(r, 0.015 * s, gradient(0.15 + i as f32 * 0.11));
    }
    // text cursor
    let cur = Rect::from_min_max(at(Vec2::new(0.555, 0.405)), at(Vec2::new(0.567, 0.47)));
    painter.rect_filled(cur, 0.0, Color32::from_rgb(0xe8, 0xf6, 0xff));
    // braces around: "{" on the left, ">" on the right
    let w = (0.028 * s).max(1.5);
    let brace = [
        Vec2::new(0.13, 0.43),
        Vec2::new(0.10, 0.45),
        Vec2::new(0.10, 0.52),
        Vec2::new(0.065, 0.545),
        Vec2::new(0.10, 0.57),
        Vec2::new(0.10, 0.64),
        Vec2::new(0.13, 0.66),
    ];
    painter.add(Shape::line(brace.iter().map(|p| at(*p)).collect(), Stroke::new(w, CYAN)));
    let chevron = [Vec2::new(0.86, 0.47), Vec2::new(0.92, 0.545), Vec2::new(0.86, 0.62)];
    painter.add(Shape::line(chevron.iter().map(|p| at(*p)).collect(), Stroke::new(w, PURPLE)));
}

/// Empty margin around the icon, as a fraction of its side: macOS draws icons as given (Apple's
/// grid: an 824 px square in a 1024 px canvas), the other systems expect a full-bleed image.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub const MACOS_INSET: f32 = 100.0 / 1024.0;
#[cfg(target_os = "macos")]
pub const ICON_INSET: f32 = MACOS_INSET;
#[cfg(not(target_os = "macos"))]
pub const ICON_INSET: f32 = 0.0;
/// Height of the F relative to the icon (alone on a transparent background, it fills it).
const LETTER: f32 = 0.98;
/// Horizontal point of the F (unit coordinates) put at the center of the icon.
const LETTER_CX: f32 = 0.53;

/// Window icon: the folded F alone on a transparent background, `inset` of empty margin around it.
pub fn icon(size: usize, inset: f32) -> egui::IconData {
    let side = 1.0 - 2.0 * inset;
    let rgba = render(size, |x, y| {
        let (u, v) = ((x - inset) / side, (y - inset) / side);
        // Letter scaled to LETTER. Its box (x 0.20..0.96) is wider on the right than its ink (the
        // stem is on the left): centering the box would push the F to the right, centering the ink
        // to the left, so the point halfway between both centers goes in the middle.
        Vec2::new((u - 0.5) / LETTER + LETTER_CX, (v - 0.5) / LETTER + 0.5)
    });
    egui::IconData { rgba, width: size as u32, height: size as u32 }
}

/// Regenerates the packaging icons: `cargo test --release write_icons -- --ignored`.
#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn write_icons() {
        for (path, inset) in [("packaging/florm.png", 0.0), ("packaging/macos/florm.png", super::MACOS_INSET)] {
            let icon = super::icon(512, inset);
            image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba).unwrap().save(path).unwrap();
        }
        // Windows: every size drawn on its own (sharper than a scaled 256 px one)
        let frames: Vec<_> = [256u32, 64, 48, 32, 24, 16]
            .iter()
            .map(|&n| {
                let icon = super::icon(n as usize, 0.0);
                image::codecs::ico::IcoFrame::as_png(&icon.rgba, n, n, image::ExtendedColorType::Rgba8).unwrap()
            })
            .collect();
        let file = std::fs::File::create("packaging/windows/florm.ico").unwrap();
        image::codecs::ico::IcoEncoder::new(file).encode_images(&frames).unwrap();
    }

    #[test]
    fn icon_is_transparent_around_the_letter() {
        let icon = super::icon(64, 0.0);
        let alpha = |x: usize, y: usize| icon.rgba[(y * 64 + x) * 4 + 3];
        // the corners are empty, the stem of the F is solid
        assert_eq!((alpha(0, 0), alpha(63, 63), alpha(63, 0)), (0, 0, 0));
        assert_eq!(alpha(24, 32), 255);
    }
}
