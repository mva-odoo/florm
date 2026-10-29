//! The Florm logo (gradient "F" holding code lines), drawn as vectors so it is
//! sharp at any size, plus a tiny rasterizer for the window icon.

use eframe::egui::{
    self, epaint::Mesh, Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2,
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
    for poly in pieces() {
        // gradient fill: triangle fan from the centroid with per-vertex colors
        let mut mesh = Mesh::default();
        let center = poly.iter().fold(Vec2::ZERO, |a, p| a + *p) / poly.len() as f32;
        mesh.colored_vertex(at(center), grad_at(center));
        for p in &poly {
            mesh.colored_vertex(at(*p), grad_at(*p));
        }
        let n = poly.len() as u32;
        for i in 0..n {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
        }
        painter.add(Shape::mesh(mesh));
        // anti-aliased edge in the same colors
        let pts: Vec<Pos2> = poly.iter().map(|p| at(*p)).collect();
        for (a, b) in pts.iter().zip(pts.iter().cycle().skip(1)) {
            let m = (*a - o) / s;
            painter.line_segment([*a, *b], Stroke::new(1.0_f32, grad_at(m)));
        }
    }
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

/// Window icon: the gradient F alone on a transparent background, 4×4 supersampled, `inset` of
/// empty margin around it.
pub fn icon(size: usize, inset: f32) -> egui::IconData {
    let pieces = pieces();
    let inside = |poly: &[Vec2], p: Vec2| {
        let n = poly.len();
        (0..n).all(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            (b - a).x * (p - a).y - (b - a).y * (p - a).x >= -1e-6
        })
    };
    let mut rgba = Vec::with_capacity(size * size * 4);
    let ss = 4;
    for y in 0..size {
        for x in 0..size {
            let (mut r, mut g, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
            for sy in 0..ss {
                for sx in 0..ss {
                    let side = 1.0 - 2.0 * inset;
                    let u = ((x as f32 + (sx as f32 + 0.5) / ss as f32) / size as f32 - inset) / side;
                    let v = ((y as f32 + (sy as f32 + 0.5) / ss as f32) / size as f32 - inset) / side;
                    // Letter scaled to LETTER. Its box (x 0.20..0.96) is wider on the right than its
                    // ink (the stem is on the left): centering the box would push the F to the
                    // right, centering the ink to the left, so the point halfway between both
                    // centers goes in the middle.
                    let p = Vec2::new((u - 0.5) / LETTER + LETTER_CX, (v - 0.5) / LETTER + 0.5);
                    // around the letter: transparent
                    if !pieces.iter().any(|poly| inside(poly, p)) {
                        continue;
                    }
                    let c = grad_at(p);
                    r += c.r() as f32;
                    g += c.g() as f32;
                    b += c.b() as f32;
                    a += 255.0;
                }
            }
            let n = (ss * ss) as f32;
            let alpha = a / n;
            let k = if a > 0.0 { 255.0 / a } else { 0.0 };
            rgba.extend([(r * k) as u8, (g * k) as u8, (b * k) as u8, alpha as u8]);
        }
    }
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
