//! The Florm logo (gradient "F" holding code lines), drawn as vectors so it is
//! sharp at any size, plus a tiny rasterizer for the window icon.

use eframe::egui::{
    self, epaint::Mesh, text::LayoutJob, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, TextFormat, Vec2,
};

pub const CYAN: Color32 = Color32::from_rgb(0x3b, 0xd5, 0xf5);
pub const BLUE: Color32 = Color32::from_rgb(0x5b, 0x8c, 0xf8);
pub const PURPLE: Color32 = Color32::from_rgb(0x9b, 0x5c, 0xf6);
pub const NAVY: Color32 = Color32::from_rgb(0x0d, 0x16, 0x26);

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

/// "FLORM" + "TEXT EDITOR" under the logo, letter-spaced like the artwork (dark ink on the
/// `light` backdrop).
pub fn paint_title(painter: &Painter, center_top: Pos2, size: f32, light: bool) {
    let mut job = LayoutJob::default();
    let fmt = TextFormat {
        font_id: FontId::proportional(size),
        color: if light { Color32::from_rgb(0x1a, 0x24, 0x38) } else { Color32::from_rgb(0xe6, 0xec, 0xf5) },
        extra_letter_spacing: size * 0.12,
        ..Default::default()
    };
    job.append("FLORM", 0.0, fmt);
    let galley = painter.layout_job(job);
    let pos = center_top - Vec2::new(galley.size().x / 2.0, 0.0);
    // subtle shadow then the silver title
    let shadow = if light { Color32::from_rgba_unmultiplied(0x5b, 0x8c, 0xf8, 40) } else { Color32::BLACK };
    painter.galley(pos + Vec2::new(0.0, 2.0), galley.clone(), shadow);
    painter.galley(pos, galley.clone(), Color32::WHITE);
    let mut sub = LayoutJob::default();
    sub.append(
        "TEXT EDITOR",
        0.0,
        TextFormat {
            font_id: FontId::proportional(size * 0.32),
            color: if light { Color32::from_rgb(0x5a, 0x6a, 0x85) } else { Color32::from_rgb(0x9a, 0xa8, 0xc0) },
            extra_letter_spacing: size * 0.09,
            ..Default::default()
        },
    );
    let sg = painter.layout_job(sub);
    let y = center_top.y + galley.size().y + size * 0.15;
    painter.galley(Pos2::new(center_top.x - sg.size().x / 2.0, y), sg, Color32::WHITE);
}

/// Navy background with a faint blueprint grid, as in the artwork; paper white with a light
/// theme.
pub fn paint_backdrop(painter: &Painter, rect: Rect, light: bool) {
    // the blue ink of the grid, glow and outlines, darker on the light backdrop
    let ink = |a: u8| if light { Color32::from_rgba_unmultiplied(0x3a, 0x5a, 0x9a, a + a / 2) } else { Color32::from_rgba_unmultiplied(0x6a, 0x8a, 0xc0, a) };
    painter.rect_filled(rect, 0.0, if light { Color32::from_rgb(0xf4, 0xf7, 0xfc) } else { NAVY });
    let glow = Rect::from_center_size(rect.center() - Vec2::new(0.0, rect.height() * 0.12), Vec2::splat(rect.height() * 0.9));
    for i in 0..12 {
        let k = 1.0 - i as f32 / 12.0 * 0.8;
        let glow_c = if light { Color32::from_rgba_unmultiplied(0x9b, 0xb8, 0xf5, 3) } else { Color32::from_rgba_unmultiplied(0x2a, 0x4a, 0x8a, 2) };
        painter.circle_filled(glow.center(), glow.width() / 2.0 * k, glow_c);
    }
    let step = 48.0;
    let line = ink(10);
    let mut x = rect.left() + (rect.width() % step) / 2.0;
    while x < rect.right() {
        painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0_f32, line));
        x += step;
    }
    let mut y = rect.top() + (rect.height() % step) / 2.0;
    while y < rect.bottom() {
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0_f32, line));
        y += step;
    }
    // corner "circuit" outlines
    let c = ink(28);
    for i in 0..3 {
        let d = 40.0 + i as f32 * 26.0;
        let bl = rect.left_bottom();
        painter.add(Shape::line(
            vec![Pos2::new(bl.x, bl.y - d * 1.6), Pos2::new(bl.x + d * 0.9, bl.y - d * 1.6), Pos2::new(bl.x + d * 1.6, bl.y - d * 0.9), Pos2::new(bl.x + d * 1.6, bl.y)],
            Stroke::new(1.0_f32, c),
        ));
        let tr = rect.right_top();
        painter.add(Shape::line(
            vec![Pos2::new(tr.x - d * 1.6, tr.y), Pos2::new(tr.x - d * 1.6, tr.y + d * 0.9), Pos2::new(tr.x - d * 0.9, tr.y + d * 1.6), Pos2::new(tr.x, tr.y + d * 1.6)],
            Stroke::new(1.0_f32, c),
        ));
    }
}

/// Window icon: navy rounded square with the gradient F, 4×4 supersampled.
pub fn icon(size: usize) -> egui::IconData {
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
                    let u = (x as f32 + (sx as f32 + 0.5) / ss as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / ss as f32) / size as f32;
                    // rounded square background
                    let (dx, dy) = ((u - 0.5).abs() - 0.32, (v - 0.5).abs() - 0.32);
                    let outside = Vec2::new(dx.max(0.0), dy.max(0.0)).length() - 0.18;
                    if outside > 0.0 {
                        continue;
                    }
                    // letter scaled to 80% inside the square
                    let p = Vec2::new((u - 0.1) / 0.8, (v - 0.1) / 0.8);
                    let c = if pieces.iter().any(|poly| inside(poly, p)) { grad_at(p) } else { NAVY };
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
