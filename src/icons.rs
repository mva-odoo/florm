//! File-type badges and folder icons, drawn as vectors (no icon font needed).

use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Short label and color for a file name.
pub fn file_kind(name: &str) -> (&'static str, Color32) {
    let lower = name.to_lowercase();
    let special = match lower.as_str() {
        "__manifest__.py" | "__openerp__.py" => Some(("OD", 0xa2_4689)),
        "dockerfile" | "docker-compose.yml" | "docker-compose.yaml" => Some(("DK", 0x2496ed)),
        "makefile" => Some(("MK", 0x8a9aae)),
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some(("GI", 0xf1502f)),
        "license" | "license.md" | "license.txt" => Some(("LI", 0xd0bf41)),
        "cargo.toml" | "cargo.lock" => Some(("RS", 0xdea584)),
        "package.json" | "package-lock.json" => Some(("NP", 0xcb3837)),
        "requirements.txt" | "pyproject.toml" | "setup.py" | "setup.cfg" => Some(("PY", 0x4b8bbe)),
        _ => None,
    };
    let (label, color) = special.unwrap_or_else(|| {
        let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        match ext {
            "py" | "pyw" | "pyi" => ("PY", 0x4b8bbe),
            "js" | "mjs" | "cjs" | "jsx" => ("JS", 0xe8d44d),
            "ts" | "tsx" => ("TS", 0x3178c6),
            "xml" | "xsd" | "xsl" | "xslt" | "rng" | "csproj" | "plist" => ("<>", 0xf08d49),
            "html" | "htm" | "jinja" | "j2" | "vue" | "svelte" => ("<>", 0xe44d26),
            "svg" => ("SV", 0xffb13b),
            "css" => ("#", 0x2f7ef5),
            "scss" | "sass" | "less" => ("S", 0xcf649a),
            "json" | "jsonc" | "json5" => ("{}", 0xcbcb41),
            "yml" | "yaml" => ("Y", 0xcb4b5a),
            "toml" => ("T", 0xb5794c),
            "ini" | "cfg" | "conf" | "env" | "properties" => ("CF", 0x8a9aae),
            "md" | "markdown" => ("MD", 0x519aba),
            "rst" | "txt" | "log" => ("TX", 0x8a9aae),
            "rs" => ("RS", 0xdea584),
            "go" => ("GO", 0x00add8),
            "java" | "kt" | "kts" => ("JV", 0xb07219),
            "c" | "h" => ("C", 0x6e8fd0),
            "cpp" | "cc" | "hpp" | "cxx" => ("C+", 0x6e8fd0),
            "cs" => ("C#", 0x68217a),
            "php" => ("PH", 0x777bb4),
            "rb" => ("RB", 0xcc342d),
            "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => ("$", 0x89e051),
            "sql" => ("DB", 0xe38c00),
            "csv" | "tsv" | "xlsx" | "xls" | "ods" => ("CS", 0x2e9b5b),
            "po" | "pot" | "mo" => ("PO", 0x5e81ac),
            "png" | "jpg" | "jpeg" | "gif" | "ico" | "webp" | "bmp" => ("IM", 0xa074c4),
            "pdf" => ("PD", 0xe5252a),
            "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" => ("ZP", 0x8a9aae),
            "lock" => ("LK", 0x8a9aae),
            "diff" | "patch" => ("±", 0x98c379),
            _ => ("", 0x8a9aae),
        }
    });
    (label, rgb(color))
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Rounded badge with the file-type label. `bg` is the color behind the icon.
/// Colors are opaque on purpose (translucent fills render inconsistently on some drivers).
pub fn paint_file(painter: &Painter, rect: Rect, name: &str, bg: Color32, light: bool) {
    let (label, color) = file_kind(name);
    let r = Rect::from_center_size(rect.center(), Vec2::splat(rect.height().min(rect.width())));
    if label.is_empty() {
        // generic document: folded-corner sheet outline
        let w = r.width() * 0.62;
        let doc = Rect::from_center_size(r.center(), Vec2::new(w, r.height() * 0.84));
        let fold = w * 0.35;
        let pts = vec![
            doc.left_top(),
            Pos2::new(doc.right() - fold, doc.top()),
            Pos2::new(doc.right(), doc.top() + fold),
            doc.right_bottom(),
            doc.left_bottom(),
        ];
        painter.add(Shape::closed_line(pts, Stroke::new(1.2_f32, mix(bg, color, 0.85))));
        return;
    }
    let fill = mix(bg, color, if light { 0.20 } else { 0.24 });
    let text = if light { mix(color, Color32::BLACK, 0.35) } else { mix(color, Color32::WHITE, 0.15) };
    painter.rect_filled(r, 3.5, fill);
    let size = if label.chars().count() >= 2 { r.height() * 0.56 } else { r.height() * 0.68 };
    painter.text(r.center() + Vec2::new(0.0, 0.5), Align2::CENTER_CENTER, label, FontId::monospace(size), text);
}

/// Folder (closed or open) in `color`.
pub fn paint_folder(painter: &Painter, rect: Rect, open: bool, color: Color32, bg: Color32) {
    let s = rect.height().min(rect.width());
    let r = Rect::from_center_size(rect.center(), Vec2::new(s, s * 0.8));
    let back = mix(bg, color, 0.75);
    // tab
    let tab = Rect::from_min_size(r.left_top(), Vec2::new(r.width() * 0.45, r.height() * 0.3));
    painter.rect_filled(tab, 1.5, back);
    // body
    let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + r.height() * 0.18), r.right_bottom());
    painter.rect_filled(body, 2.0, back);
    if open {
        // front flap leaning forward
        let front = mix(bg, color, 1.0);
        let y0 = body.top() + body.height() * 0.38;
        let pts = vec![
            Pos2::new(body.left() + s * 0.12, y0),
            Pos2::new(body.right() + s * 0.06, y0),
            Pos2::new(body.right() - s * 0.06, body.bottom()),
            Pos2::new(body.left(), body.bottom()),
        ];
        painter.add(Shape::convex_polygon(pts, front, Stroke::NONE));
    } else {
        let line = Rect::from_min_max(Pos2::new(body.left(), body.top() + 2.0), Pos2::new(body.right(), body.top() + 3.0));
        painter.rect_filled(line, 0.0, mix(bg, color, 1.0));
    }
}

/// Small chevron (▸ / ▾) for expandable rows.
pub fn paint_chevron(painter: &Painter, center: Pos2, open: bool, color: Color32) {
    let s = 3.5;
    let pts = if open {
        vec![Pos2::new(center.x - s, center.y - s * 0.5), Pos2::new(center.x + s, center.y - s * 0.5), Pos2::new(center.x, center.y + s * 0.6)]
    } else {
        vec![Pos2::new(center.x - s * 0.5, center.y - s), Pos2::new(center.x + s * 0.6, center.y), Pos2::new(center.x - s * 0.5, center.y + s)]
    };
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(file_kind("models.py").0, "PY");
        assert_eq!(file_kind("__manifest__.py").0, "OD");
        assert_eq!(file_kind("view.XML").0, "<>");
        assert_eq!(file_kind("fr.po").0, "PO");
        assert_eq!(file_kind("unknown.zzz").0, "");
        assert_eq!(file_kind("Dockerfile").0, "DK");
    }
}
