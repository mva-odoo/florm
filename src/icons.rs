//! File-type badges and folder icons, drawn as vectors (no icon font needed).

use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Short label and color for a file name.
pub fn file_kind(name: &str) -> (&'static str, Color32) {
    let lower = name.to_lowercase();
    let special = match lower.as_str() {
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
    // an icon of Material Icon Theme, drawn at the pixels of the screen
    if let Some(icon) = material_icon(&name.to_lowercase()) {
        let side = rect.height().min(rect.width());
        let ppp = painter.ctx().pixels_per_point();
        let px = (side * ppp).round().max(8.0) as u32;
        if let Some(tex) = icon_texture(painter.ctx(), icon, px) {
            // on whole pixels: the texture drawn as rasterized, sharp
            let size = px as f32 / ppp;
            let min = (((rect.center() - Vec2::splat(size / 2.0)).to_vec2() * ppp).round() / ppp).to_pos2();
            let r = Rect::from_min_size(min, Vec2::splat(size));
            painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            return;
        }
    }
    let (label, color) = file_kind(name);
    let r = Rect::from_center_size(rect.center(), Vec2::splat(rect.height().min(rect.width())));
    let c = r.center();
    let s = r.width();
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let bold = |t: &str, size: f32, col: Color32| {
        painter.text(c + Vec2::new(0.0, 0.5), Align2::CENTER_CENTER, t, FontId::monospace(size), col);
    };
    // a sheet with a folded corner, its outline in `col`
    let sheet = |col: Color32, lines: bool| {
        let w = s * 0.66;
        let doc = Rect::from_center_size(c, Vec2::new(w, s * 0.86));
        let fold = w * 0.36;
        let pts = vec![doc.left_top(), Pos2::new(doc.right() - fold, doc.top()), Pos2::new(doc.right(), doc.top() + fold), doc.right_bottom(), doc.left_bottom()];
        painter.add(Shape::closed_line(pts, Stroke::new(1.2_f32, col)));
        if lines {
            for k in 0..3 {
                let y = doc.top() + doc.height() * (0.42 + k as f32 * 0.18);
                painter.line_segment([Pos2::new(doc.left() + 2.5, y), Pos2::new(doc.right() - 2.5, y)], Stroke::new(1.0_f32, col.gamma_multiply(0.8)));
            }
        }
    };
    match ext {
        // the two snakes of Python, blue and yellow
        "py" | "pyw" | "pyi" => {
            let (blue, yellow) = (Color32::from_rgb(0x37, 0x76, 0xab), Color32::from_rgb(0xff, 0xd4, 0x3b));
            let u = s / 16.0;
            painter.rect_filled(Rect::from_min_size(r.min + Vec2::new(2.0 * u, 1.5 * u), Vec2::new(8.5 * u, 8.0 * u)), 2.5 * u, blue);
            painter.rect_filled(Rect::from_min_size(r.min + Vec2::new(5.5 * u, 6.5 * u), Vec2::new(8.5 * u, 8.0 * u)), 2.5 * u, yellow);
            // where they cross: the blue one's head over the yellow one
            painter.rect_filled(Rect::from_min_size(r.min + Vec2::new(5.5 * u, 6.5 * u), Vec2::new(5.0 * u, 3.0 * u)), 1.0 * u, blue);
            painter.circle_filled(r.min + Vec2::new(4.5 * u, 3.8 * u), 0.9 * u, Color32::WHITE);
            painter.circle_filled(r.min + Vec2::new(11.5 * u, 12.2 * u), 0.9 * u, Color32::WHITE);
        }
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "mts" | "cts" => {
            let ts = ext.contains('t') && ext != "cjs" && ext != "mjs" && ext != "jsx";
            let (fill, text) = if ts { (Color32::from_rgb(0x31, 0x78, 0xc6), Color32::WHITE) } else { (Color32::from_rgb(0xf7, 0xdf, 0x1e), Color32::from_rgb(0x23, 0x23, 0x23)) };
            let sq = r.shrink(1.5);
            painter.rect_filled(sq, 2.0, fill);
            painter.text(sq.right_bottom() + Vec2::new(-1.0, 0.5), Align2::RIGHT_BOTTOM, if ts { "TS" } else { "JS" }, FontId::monospace(s * 0.42), text);
        }
        "xml" | "xsd" | "xsl" | "xslt" | "rng" | "csproj" | "plist" | "html" | "htm" | "jinja" | "j2" | "vue" | "svelte" | "svg" => {
            let st = Stroke::new(1.6_f32, color);
            let (h, w) = (s * 0.26, s * 0.2);
            painter.line_segment([c + Vec2::new(-s * 0.18, -h), c + Vec2::new(-s * 0.18 - w, 0.0)], st);
            painter.line_segment([c + Vec2::new(-s * 0.18 - w, 0.0), c + Vec2::new(-s * 0.18, h)], st);
            painter.line_segment([c + Vec2::new(s * 0.18, -h), c + Vec2::new(s * 0.18 + w, 0.0)], st);
            painter.line_segment([c + Vec2::new(s * 0.18 + w, 0.0), c + Vec2::new(s * 0.18, h)], st);
            painter.line_segment([c + Vec2::new(s * 0.07, -h - 1.0), c + Vec2::new(-s * 0.07, h + 1.0)], Stroke::new(1.3_f32, color));
        }
        "json" | "jsonc" | "json5" => bold("{}", s * 0.78, color),
        "css" | "scss" | "sass" | "less" => bold("#", s * 0.8, color),
        "md" | "markdown" => {
            let box_ = Rect::from_center_size(c, Vec2::new(s * 0.9, s * 0.62));
            painter.rect_stroke(box_, 2.0, Stroke::new(1.2_f32, color), egui::StrokeKind::Inside);
            painter.text(box_.center() + Vec2::new(-s * 0.1, 0.5), Align2::CENTER_CENTER, "M", FontId::monospace(s * 0.48), color);
            let ax = box_.right() - s * 0.2;
            painter.line_segment([Pos2::new(ax, box_.top() + 3.0), Pos2::new(ax, box_.bottom() - 3.0)], Stroke::new(1.2_f32, color));
            painter.line_segment([Pos2::new(ax - 2.0, box_.bottom() - 5.0), Pos2::new(ax, box_.bottom() - 3.0)], Stroke::new(1.2_f32, color));
            painter.line_segment([Pos2::new(ax + 2.0, box_.bottom() - 5.0), Pos2::new(ax, box_.bottom() - 3.0)], Stroke::new(1.2_f32, color));
        }
        "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => {
            let st = Stroke::new(1.6_f32, color);
            painter.line_segment([c + Vec2::new(-s * 0.32, -s * 0.22), c + Vec2::new(-s * 0.08, 0.0)], st);
            painter.line_segment([c + Vec2::new(-s * 0.08, 0.0), c + Vec2::new(-s * 0.32, s * 0.22)], st);
            painter.line_segment([c + Vec2::new(s * 0.02, s * 0.26), c + Vec2::new(s * 0.34, s * 0.26)], st);
        }
        "png" | "jpg" | "jpeg" | "gif" | "ico" | "webp" | "bmp" => {
            let pic = Rect::from_center_size(c, Vec2::new(s * 0.86, s * 0.7));
            painter.rect_stroke(pic, 2.0, Stroke::new(1.2_f32, color), egui::StrokeKind::Inside);
            painter.circle_filled(pic.left_top() + Vec2::new(s * 0.24, s * 0.22), s * 0.08, color);
            let pts = vec![pic.left_bottom() + Vec2::new(2.0, -2.0), pic.center() + Vec2::new(-s * 0.05, -s * 0.02), pic.center_bottom() + Vec2::new(s * 0.05, -s * 0.18), pic.right_bottom() + Vec2::new(-2.0, -2.0)];
            painter.add(Shape::line(pts, Stroke::new(1.3_f32, color)));
        }
        "csv" | "tsv" | "xlsx" | "xls" | "ods" => {
            let g = Rect::from_center_size(c, Vec2::splat(s * 0.76));
            painter.rect_stroke(g, 1.5, Stroke::new(1.2_f32, color), egui::StrokeKind::Inside);
            for k in 1..3 {
                let t = k as f32 / 3.0;
                painter.line_segment([Pos2::new(g.left(), g.top() + g.height() * t), Pos2::new(g.right(), g.top() + g.height() * t)], Stroke::new(1.0_f32, color));
                painter.line_segment([Pos2::new(g.left() + g.width() * t, g.top()), Pos2::new(g.left() + g.width() * t, g.bottom())], Stroke::new(1.0_f32, color));
            }
        }
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" => {
            let b = Rect::from_center_size(c, Vec2::new(s * 0.72, s * 0.82));
            painter.rect_stroke(b, 2.0, Stroke::new(1.2_f32, color), egui::StrokeKind::Inside);
            for k in 0..4 {
                let y = b.top() + 2.5 + k as f32 * 2.5;
                painter.line_segment([Pos2::new(c.x - 1.2, y), Pos2::new(c.x + 1.2, y)], Stroke::new(1.0_f32, color));
            }
        }
        "lock" => {
            let body = Rect::from_center_size(c + Vec2::new(0.0, s * 0.14), Vec2::new(s * 0.62, s * 0.46));
            painter.circle_stroke(c + Vec2::new(0.0, -s * 0.1), s * 0.2, Stroke::new(1.4_f32, color));
            painter.rect_filled(body, 2.0, color);
        }
        "txt" | "log" | "rst" | "text" => sheet(color, true),
        _ if lower.starts_with(".git") => {
            // the diamond of git with a branch
            let d = s * 0.42;
            let pts = vec![c + Vec2::new(0.0, -d), c + Vec2::new(d, 0.0), c + Vec2::new(0.0, d), c + Vec2::new(-d, 0.0)];
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
            painter.circle_filled(c + Vec2::new(-s * 0.08, s * 0.12), 1.3, Color32::WHITE);
            painter.circle_filled(c + Vec2::new(s * 0.12, -s * 0.08), 1.3, Color32::WHITE);
            painter.line_segment([c + Vec2::new(-s * 0.08, s * 0.12), c + Vec2::new(s * 0.12, -s * 0.08)], Stroke::new(1.0_f32, Color32::WHITE));
        }
        _ if label.is_empty() => sheet(mix(bg, color, 0.85), false),
        // the others: a badge of their color with their letters
        _ => {
            let fill = mix(bg, color, if light { 0.85 } else { 0.75 });
            let l = 0.2126 * fill.r() as f32 + 0.7152 * fill.g() as f32 + 0.0722 * fill.b() as f32;
            let text = if l > 150.0 { Color32::from_rgb(0x20, 0x20, 0x20) } else { Color32::WHITE };
            painter.rect_filled(r.shrink(1.0), 3.0, fill);
            let size = if label.chars().count() >= 2 { s * 0.5 } else { s * 0.64 };
            bold(label, size, text);
        }
    }
}

/// The file icons of Material Icon Theme (MIT, assets/file_icons): (name, SVG).
static FILE_ICONS: &[(&str, &[u8])] = &[
    ("c", include_bytes!("../assets/file_icons/c.svg")),
    ("certificate", include_bytes!("../assets/file_icons/certificate.svg")),
    ("console", include_bytes!("../assets/file_icons/console.svg")),
    ("cpp", include_bytes!("../assets/file_icons/cpp.svg")),
    ("csharp", include_bytes!("../assets/file_icons/csharp.svg")),
    ("css", include_bytes!("../assets/file_icons/css.svg")),
    ("database", include_bytes!("../assets/file_icons/database.svg")),
    ("diff", include_bytes!("../assets/file_icons/diff.svg")),
    ("docker", include_bytes!("../assets/file_icons/docker.svg")),
    ("document", include_bytes!("../assets/file_icons/document.svg")),
    ("git", include_bytes!("../assets/file_icons/git.svg")),
    ("go", include_bytes!("../assets/file_icons/go.svg")),
    ("h", include_bytes!("../assets/file_icons/h.svg")),
    ("html", include_bytes!("../assets/file_icons/html.svg")),
    ("i18n", include_bytes!("../assets/file_icons/i18n.svg")),
    ("image", include_bytes!("../assets/file_icons/image.svg")),
    ("java", include_bytes!("../assets/file_icons/java.svg")),
    ("javascript", include_bytes!("../assets/file_icons/javascript.svg")),
    ("json", include_bytes!("../assets/file_icons/json.svg")),
    ("kotlin", include_bytes!("../assets/file_icons/kotlin.svg")),
    ("less", include_bytes!("../assets/file_icons/less.svg")),
    ("license", include_bytes!("../assets/file_icons/license.svg")),
    ("lock", include_bytes!("../assets/file_icons/lock.svg")),
    ("log", include_bytes!("../assets/file_icons/log.svg")),
    ("lua", include_bytes!("../assets/file_icons/lua.svg")),
    ("makefile", include_bytes!("../assets/file_icons/makefile.svg")),
    ("markdown", include_bytes!("../assets/file_icons/markdown.svg")),
    ("nodejs", include_bytes!("../assets/file_icons/nodejs.svg")),
    ("pdf", include_bytes!("../assets/file_icons/pdf.svg")),
    ("php", include_bytes!("../assets/file_icons/php.svg")),
    ("python", include_bytes!("../assets/file_icons/python.svg")),
    ("python-misc", include_bytes!("../assets/file_icons/python-misc.svg")),
    ("react", include_bytes!("../assets/file_icons/react.svg")),
    ("react_ts", include_bytes!("../assets/file_icons/react_ts.svg")),
    ("readme", include_bytes!("../assets/file_icons/readme.svg")),
    ("ruby", include_bytes!("../assets/file_icons/ruby.svg")),
    ("rust", include_bytes!("../assets/file_icons/rust.svg")),
    ("sass", include_bytes!("../assets/file_icons/sass.svg")),
    ("settings", include_bytes!("../assets/file_icons/settings.svg")),
    ("svg", include_bytes!("../assets/file_icons/svg.svg")),
    ("table", include_bytes!("../assets/file_icons/table.svg")),
    ("toml", include_bytes!("../assets/file_icons/toml.svg")),
    ("typescript", include_bytes!("../assets/file_icons/typescript.svg")),
    ("vue", include_bytes!("../assets/file_icons/vue.svg")),
    ("xml", include_bytes!("../assets/file_icons/xml.svg")),
    ("yaml", include_bytes!("../assets/file_icons/yaml.svg")),
    ("zip", include_bytes!("../assets/file_icons/zip.svg")),
];

/// The icon of Material Icon Theme for the file `name` (lower case), if there is one.
fn material_icon(lower: &str) -> Option<&'static str> {
    let by_name = match lower {
        "dockerfile" | "containerfile" | "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml" => Some("docker"),
        "makefile" | "gnumakefile" => Some("makefile"),
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitkeep" => Some("git"),
        "license" | "license.md" | "license.txt" | "copying" | "copyright" => Some("license"),
        "readme" | "readme.md" | "readme.rst" | "readme.txt" => Some("readme"),
        "package.json" | "package-lock.json" | ".npmrc" | ".nvmrc" => Some("nodejs"),
        "requirements.txt" | "pyproject.toml" | "setup.py" | "setup.cfg" | "pipfile" | "manifest.in" => Some("python-misc"),
        "__manifest__.py" | "__openerp__.py" => Some("python-misc"),
        "cargo.lock" | "package.lock" | "poetry.lock" | "yarn.lock" | "uv.lock" => Some("lock"),
        _ => None,
    };
    if by_name.is_some() {
        return by_name;
    }
    let ext = lower.rsplit_once('.').map(|(_, e)| e)?;
    Some(match ext {
        "py" | "pyw" | "pyi" | "pyx" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "react",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "react_ts",
        "xml" | "xsd" | "xsl" | "xslt" | "rng" | "csproj" | "plist" | "wsdl" => "xml",
        "html" | "htm" | "xhtml" | "jinja" | "j2" => "html",
        "vue" => "vue",
        "svg" => "svg",
        "json" | "jsonc" | "json5" | "webmanifest" => "json",
        "css" => "css",
        "scss" | "sass" => "sass",
        "less" => "less",
        "md" | "markdown" | "mdx" => "markdown",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "ini" | "cfg" | "conf" | "env" | "properties" | "editorconfig" => "settings",
        "rs" => "rust",
        "go" => "go",
        "c" => "c",
        "h" | "hpp" | "hh" | "hxx" => "h",
        "cpp" | "cc" | "cxx" | "c++" => "cpp",
        "cs" => "csharp",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "lua" => "lua",
        "php" => "php",
        "rb" | "rake" | "gemspec" => "ruby",
        "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => "console",
        "sql" | "sqlite" | "db" | "dump" => "database",
        "csv" | "tsv" | "xlsx" | "xls" | "ods" => "table",
        "po" | "pot" | "mo" => "i18n",
        "png" | "jpg" | "jpeg" | "gif" | "ico" | "webp" | "bmp" | "tif" | "tiff" => "image",
        "pdf" => "pdf",
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "whl" => "zip",
        "lock" => "lock",
        "log" => "log",
        "txt" | "rst" | "text" => "document",
        "diff" | "patch" => "diff",
        "pem" | "crt" | "cer" | "key" => "certificate",
        _ => return None,
    })
}

/// The texture of icon `name` drawn at `px` pixels (drawn once per size, then kept).
fn icon_texture(ctx: &egui::Context, name: &'static str, px: u32) -> Option<egui::TextureHandle> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(&'static str, u32), Option<egui::TextureHandle>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(t) = cache.lock().ok()?.get(&(name, px)) {
        return t.clone();
    }
    let bytes = FILE_ICONS.iter().find(|(n, _)| *n == name)?.1;
    let hint = egui::load::SizeHint::Size { width: px, height: px, maintain_aspect_ratio: true };
    let tex = crate::images::rasterize_svg(bytes, hint).ok().map(|img| ctx.load_texture(format!("file_icon_{name}_{px}"), img, egui::TextureOptions::LINEAR));
    cache.lock().ok()?.insert((name, px), tex.clone());
    tex
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

/// The update of Florm: a round badge of a bright green with a white arrow going up over a line
/// (to install), or a circular arrow (installed: restart).
pub fn paint_upgrade(painter: &Painter, rect: Rect, ready: bool, hovered: bool) {
    let c = rect.center();
    let r = rect.height().min(rect.width()) / 2.0;
    let green = if hovered { Color32::from_rgb(0x16, 0xa3, 0x4a) } else { Color32::from_rgb(0x22, 0xc5, 0x5e) };
    // a soft halo, then the badge
    painter.circle_filled(c, r + 1.5, green.gamma_multiply(0.25));
    painter.circle_filled(c, r, green);
    let white = Stroke::new(1.8_f32, Color32::WHITE);
    if ready {
        let pts: Vec<Pos2> = (0..=18).map(|i| {
            let a = -0.6 + i as f32 / 18.0 * 4.6;
            c + Vec2::new(a.cos(), a.sin()) * (r * 0.5)
        }).collect();
        let end = pts[0];
        painter.add(Shape::line(pts, white));
        painter.line_segment([end, end + Vec2::new(-0.2, -3.0)], white);
        painter.line_segment([end, end + Vec2::new(-3.0, 0.4)], white);
    } else {
        let top = c + Vec2::new(0.0, -r * 0.5);
        painter.line_segment([top, c + Vec2::new(0.0, r * 0.28)], white);
        painter.line_segment([top, top + Vec2::new(-r * 0.36, r * 0.36)], white);
        painter.line_segment([top, top + Vec2::new(r * 0.36, r * 0.36)], white);
        painter.line_segment([c + Vec2::new(-r * 0.38, r * 0.55), c + Vec2::new(r * 0.38, r * 0.55)], white);
    }
}

/// A shield (the sandbox of Claude), about `size` high around `center`, filled in `color`, a
/// check mark inside in `mark`.
pub fn paint_shield(painter: &Painter, center: Pos2, size: f32, color: Color32, mark: Color32) {
    let (w, h) = (size * 0.8, size);
    let top = center.y - h / 2.0;
    let mut pts = vec![Pos2::new(center.x, top), Pos2::new(center.x + w / 2.0, top + h * 0.16), Pos2::new(center.x + w / 2.0, top + h * 0.5)];
    // the rounded point at the bottom
    for k in 1..=6 {
        let t = k as f32 / 6.0;
        let x = center.x + w / 2.0 * (1.0 - t);
        let y = top + h * 0.5 + h * 0.5 * (t * (2.0 - t));
        pts.push(Pos2::new(x, y));
    }
    for k in (0..6).rev() {
        let t = k as f32 / 6.0;
        let x = center.x - w / 2.0 * (1.0 - t);
        let y = top + h * 0.5 + h * 0.5 * (t * (2.0 - t));
        pts.push(Pos2::new(x, y));
    }
    pts.push(Pos2::new(center.x - w / 2.0, top + h * 0.16));
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    let st = Stroke::new((size * 0.12).max(1.2), mark);
    let c = center + Vec2::new(0.0, h * 0.04);
    painter.line_segment([c + Vec2::new(-w * 0.22, 0.0), c + Vec2::new(-w * 0.04, h * 0.16)], st);
    painter.line_segment([c + Vec2::new(-w * 0.04, h * 0.16), c + Vec2::new(w * 0.24, -h * 0.16)], st);
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

/// Magnifying glass centered on `center` (search button).
pub fn paint_search(painter: &Painter, center: Pos2, color: Color32) {
    let c = center + Vec2::new(-1.5, -1.5);
    painter.circle_stroke(c, 4.5, Stroke::new(1.5_f32, color));
    painter.line_segment([c + Vec2::splat(3.3), c + Vec2::splat(7.0)], Stroke::new(1.8_f32, color));
}

/// Small terminal window with a `>_` prompt (terminal button).
pub fn paint_terminal(painter: &Painter, center: Pos2, color: Color32) {
    let r = Rect::from_center_size(center, Vec2::new(15.0, 12.0));
    painter.rect_stroke(r, 2.0, Stroke::new(1.3_f32, color), eframe::egui::StrokeKind::Middle);
    let (x, y) = (r.left() + 3.5, r.center().y);
    painter.add(Shape::line(vec![Pos2::new(x, y - 2.5), Pos2::new(x + 2.5, y), Pos2::new(x, y + 2.5)], Stroke::new(1.3_f32, color)));
    painter.line_segment([Pos2::new(x + 4.5, y + 2.5), Pos2::new(x + 8.0, y + 2.5)], Stroke::new(1.3_f32, color));
}

/// Crossed out eye: left out of the index (tree of the folder).
pub fn paint_eye_off(painter: &Painter, center: Pos2, color: Color32) {
    let st = Stroke::new(1.2_f32, color);
    let (w, h) = (6.0, 3.6);
    let arc = |sign: f32| -> Vec<Pos2> {
        (0..=10).map(|k| {
            let t = k as f32 / 10.0 * 2.0 - 1.0;
            Pos2::new(center.x + t * w, center.y + sign * h * (1.0 - t * t))
        }).collect()
    };
    painter.add(Shape::line(arc(-1.0), st));
    painter.add(Shape::line(arc(1.0), st));
    painter.circle_filled(center, 1.6, color);
    painter.line_segment([center + Vec2::new(-5.5, 5.0), center + Vec2::new(5.5, -5.0)], Stroke::new(1.4_f32, color));
}

/// Views that open in an editor tab or from the buttons of the sidebar, drawn like the file badges.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewIcon {
    Search,
    Terminal,
    Claude,
    Git,
    PullRequests,
    Help,
    Welcome,
    Settings,
    Problems,
    Output,
    Debug,
}

/// Claude's color (its asterisk), whatever the theme.
pub const CLAUDE: Color32 = Color32::from_rgb(0xd9, 0x77, 0x57);

/// `icon` about 15 px wide around `center`.
pub fn paint_view(painter: &Painter, center: Pos2, icon: ViewIcon, color: Color32) {
    let st = Stroke::new(1.4_f32, color);
    match icon {
        ViewIcon::Search => paint_search(painter, center, color),
        ViewIcon::Terminal => paint_terminal(painter, center, color),
        ViewIcon::Claude => {
            // asterisk of 8 rounded rays
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4 + 0.2;
                let d = Vec2::new(a.cos(), a.sin());
                painter.line_segment([center + d * 1.5, center + d * 6.5], Stroke::new(1.9_f32, CLAUDE));
            }
        }
        ViewIcon::Git => {
            // branch: a trunk and a branch leaving it
            let (a, b, c) = (center + Vec2::new(-3.5, -5.0), center + Vec2::new(-3.5, 5.0), center + Vec2::new(4.0, -3.0));
            painter.line_segment([a, b], st);
            painter.add(Shape::line(vec![c + Vec2::new(0.0, 2.0), c + Vec2::new(0.0, 4.0), center + Vec2::new(-3.0, 2.5)], st));
            for p in [a, b, c] {
                painter.circle_filled(p, 2.0, color);
            }
        }
        ViewIcon::PullRequests => {
            let (a, b) = (center + Vec2::new(-4.0, -5.0), center + Vec2::new(-4.0, 5.0));
            let (c, top) = (center + Vec2::new(4.0, 5.0), center + Vec2::new(4.0, -4.0));
            painter.line_segment([a, b], st);
            painter.line_segment([c, top], st);
            painter.add(Shape::line(vec![top + Vec2::new(-2.5, -1.0), top + Vec2::new(0.0, 0.0), top + Vec2::new(-2.5, 1.5)], st));
            for p in [a, b, c] {
                painter.circle_filled(p, 2.0, color);
            }
        }
        ViewIcon::Help => {
            painter.circle_stroke(center, 6.5, st);
            painter.text(center + Vec2::new(0.0, 0.5), Align2::CENTER_CENTER, "?", FontId::proportional(10.0), color);
        }
        ViewIcon::Welcome => {
            // house
            let (l, r, t) = (center.x - 6.0, center.x + 6.0, center.y - 6.0);
            painter.add(Shape::line(vec![Pos2::new(l, center.y), Pos2::new(center.x, t), Pos2::new(r, center.y)], st));
            painter.add(Shape::line(
                vec![Pos2::new(l + 2.0, center.y - 1.5), Pos2::new(l + 2.0, center.y + 5.5), Pos2::new(r - 2.0, center.y + 5.5), Pos2::new(r - 2.0, center.y - 1.5)],
                st,
            ));
        }
        ViewIcon::Settings => {
            // gear: ring and 8 teeth
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let d = Vec2::new(a.cos(), a.sin());
                painter.line_segment([center + d * 4.5, center + d * 6.8], Stroke::new(2.2_f32, color));
            }
            painter.circle_stroke(center, 4.3, st);
            painter.circle_filled(center, 1.6, color);
        }
        ViewIcon::Problems => {
            // warning triangle
            let (t, l, r) = (center + Vec2::new(0.0, -6.0), center + Vec2::new(-6.5, 5.5), center + Vec2::new(6.5, 5.5));
            painter.add(Shape::closed_line(vec![t, r, l], st));
            painter.line_segment([center + Vec2::new(0.0, -2.0), center + Vec2::new(0.0, 1.5)], st);
            painter.circle_filled(center + Vec2::new(0.0, 3.6), 0.9, color);
        }
        ViewIcon::Output => {
            // page of text lines
            let r = Rect::from_center_size(center, Vec2::new(11.0, 13.0));
            painter.rect_stroke(r, 1.5, st, eframe::egui::StrokeKind::Middle);
            for (k, w) in [7.0_f32, 5.0, 6.0].into_iter().enumerate() {
                let y = r.top() + 3.5 + k as f32 * 3.0;
                painter.line_segment([Pos2::new(r.left() + 2.5, y), Pos2::new(r.left() + 2.5 + w * 0.85, y)], Stroke::new(1.1_f32, color));
            }
        }
        ViewIcon::Debug => {
            // a bug: its body, its head, three legs on each side
            let body = Rect::from_center_size(center + Vec2::new(0.0, 1.5), Vec2::new(8.0, 10.0));
            painter.rect_stroke(body, 4.0, st, eframe::egui::StrokeKind::Middle);
            painter.circle_stroke(center + Vec2::new(0.0, -5.0), 2.2, st);
            painter.line_segment([Pos2::new(body.left(), center.y + 1.5), Pos2::new(body.right(), center.y + 1.5)], Stroke::new(1.1_f32, color));
            for dy in [-1.5_f32, 1.5, 4.5] {
                let y = center.y + dy;
                painter.line_segment([Pos2::new(body.left(), y), Pos2::new(body.left() - 3.0, y - 1.0)], Stroke::new(1.1_f32, color));
                painter.line_segment([Pos2::new(body.right(), y), Pos2::new(body.right() + 3.0, y - 1.0)], Stroke::new(1.1_f32, color));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(file_kind("models.py").0, "PY");
        assert_eq!(file_kind("view.XML").0, "<>");
        assert_eq!(file_kind("fr.po").0, "PO");
        assert_eq!(file_kind("unknown.zzz").0, "");
        assert_eq!(file_kind("Dockerfile").0, "DK");
    }
}

/// The buttons that change the layout, all drawn alike (a small window of 14×12).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Layout {
    /// Corners pointing out: the panel takes all the room of its view.
    Maximize,
    /// Corners pointing in: back to the normal layout.
    Restore,
    /// The view goes to an editor tab: a window with its tab on top.
    ToTab,
    /// The view goes to the bottom panel: a window with its bottom band filled.
    ToBottom,
}

/// A 20×20 button with a `Layout` icon.
pub fn layout_button(ui: &mut eframe::egui::Ui, kind: Layout, tip: &str) -> eframe::egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(20.0), eframe::egui::Sense::click());
    paint_layout_button(ui, rect, &r, kind);
    r.on_hover_text(tip)
}

pub fn paint_layout_button(ui: &eframe::egui::Ui, rect: Rect, r: &eframe::egui::Response, kind: Layout) {
    let v = ui.visuals();
    if r.hovered() {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let color = if r.hovered() { v.strong_text_color() } else { v.text_color() };
    paint_layout(ui.painter(), rect.center(), kind, color);
}

pub fn paint_layout(p: &Painter, c: Pos2, kind: Layout, color: Color32) {
    let stroke = Stroke::new(1.4_f32, color);
    let win = Rect::from_center_size(c, Vec2::new(14.0, 12.0));
    let fill = color.gamma_multiply(0.55);
    match kind {
        Layout::Maximize | Layout::Restore => {
            let on = kind == Layout::Restore;
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let (corner, arm) = if on { (Vec2::new(2.0 * sx, 2.0 * sy), 4.0) } else { (Vec2::new(6.0 * sx, 6.0 * sy), -4.0) };
                let q = c + corner;
                p.line_segment([q, q + Vec2::new(arm * sx, 0.0)], stroke);
                p.line_segment([q, q + Vec2::new(0.0, arm * sy)], stroke);
            }
        }
        Layout::ToTab => {
            p.rect_stroke(win, 2.0, stroke, eframe::egui::StrokeKind::Middle);
            p.rect_filled(Rect::from_min_max(win.min, Pos2::new(win.min.x + 6.0, win.min.y + 3.5)), 1.0, color);
        }
        Layout::ToBottom => {
            p.rect_stroke(win, 2.0, stroke, eframe::egui::StrokeKind::Middle);
            p.rect_filled(Rect::from_min_max(Pos2::new(win.min.x, win.max.y - 4.5), win.max), 1.0, fill);
        }
    }
}
