//! Markdown whose code blocks are drawn apart, each with a button to copy it (a command, code).

use super::*;

/// A piece of Markdown: text, or a fenced code block (its language and its code).
#[derive(Debug, PartialEq)]
pub(super) enum MdPart {
    Text(String),
    Code { lang: String, code: String },
    /// A table (`| a | b |` lines under a `|---|---|` one), cut apart by `blocks`: drawn by Florm,
    /// its cells wrapped to the width (the viewer's grid does not wrap them, and two of its
    /// tables in one answer, a code block between, shared their column widths and overlapped).
    Table { header: Vec<String>, rows: Vec<Vec<String>> },
}

/// `text` cut around its fenced code blocks (``` or ~~~); a block not closed yet (an answer being
/// written) goes to the end.
pub(super) fn parts(text: &str) -> Vec<MdPart> {
    let mut out = Vec::new();
    let mut buf = String::new();
    // the block being read: language, fence, code
    let mut code: Option<(String, String, String)> = None;
    for line in text.split('\n') {
        let t = line.trim_start();
        let fence_line = line.len() - t.len() <= 3;
        if let Some((_, fence, body)) = code.as_mut() {
            let ch = fence.chars().next().unwrap_or('`');
            let tt = t.trim_end();
            if !(fence_line && tt.len() >= fence.len() && tt.chars().all(|c| c == ch)) {
                body.push_str(line);
                body.push('\n');
                continue;
            }
            if let Some((lang, _, body)) = code.take() {
                out.push(MdPart::Code { lang, code: body.trim_end_matches('\n').to_string() });
            }
            continue;
        }
        if fence_line && (t.starts_with("```") || t.starts_with("~~~")) {
            let ch = if t.starts_with('`') { '`' } else { '~' };
            let n = t.chars().take_while(|c| *c == ch).count();
            if !buf.trim().is_empty() {
                out.push(MdPart::Text(std::mem::take(&mut buf)));
            }
            buf.clear();
            code = Some((t[n..].trim().to_string(), ch.to_string().repeat(n), String::new()));
        } else {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if let Some((lang, _, body)) = code {
        out.push(MdPart::Code { lang, code: body.trim_end_matches('\n').to_string() });
    }
    if !buf.trim().is_empty() {
        out.push(MdPart::Text(buf));
    }
    out
}

/// `parts` with the tables of its texts apart too, to be drawn.
pub(super) fn blocks(text: &str) -> Vec<MdPart> {
    let mut out = Vec::new();
    for part in parts(text) {
        let MdPart::Text(t) = part else {
            out.push(part);
            continue;
        };
        let lines: Vec<&str> = t.split('\n').collect();
        let mut buf = String::new();
        let mut i = 0;
        while i < lines.len() {
            let row = |l: &str| {
                let t = l.trim();
                (l.len() - l.trim_start().len() <= 3 && t.starts_with('|')).then(|| table_cells(t))
            };
            let header = row(lines[i]);
            let rule = lines.get(i + 1).and_then(|l| row(l)).filter(|c| !c.is_empty() && c.iter().all(|c| table_rule(c)));
            let (Some(header), Some(_)) = (header, rule) else {
                buf.push_str(lines[i]);
                buf.push('\n');
                i += 1;
                continue;
            };
            if !buf.trim().is_empty() {
                out.push(MdPart::Text(std::mem::take(&mut buf)));
            }
            buf.clear();
            i += 2;
            let mut rows = Vec::new();
            while let Some(mut cells) = lines.get(i).and_then(|l| row(l)) {
                cells.resize(header.len(), String::new());
                rows.push(cells);
                i += 1;
            }
            out.push(MdPart::Table { header, rows });
        }
        if !buf.trim().is_empty() {
            out.push(MdPart::Text(buf));
        }
    }
    out
}

/// The cells of a table line (`| a | b |`): a `|` in code (`` `a|b` ``) or escaped (`\|`) is text.
fn table_cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').filter(|l| !l.ends_with('\\')).unwrap_or(line);
    let (mut cells, mut cell, mut code) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '`' => {
                code = !code;
                cell.push(c);
            }
            '|' if !code => cells.push(std::mem::take(&mut cell).trim().to_string()),
            _ => cell.push(c),
        }
    }
    cells.push(cell.trim().to_string());
    cells
}

/// A cell of the line under the header: `---`, `:--`, `--:`, `:-:`.
fn table_rule(cell: &str) -> bool {
    let c = cell.trim().trim_start_matches(':').trim_end_matches(':');
    !c.is_empty() && c.chars().all(|c| c == '-')
}

/// A table: the columns share the width (the narrow ones keep theirs, the others are cut in
/// proportion and their text wraps), one row out of two on a light background.
fn table(ui: &mut egui::Ui, cache: &mut egui_commonmark::CommonMarkCache, header: &[String], rows: &[Vec<String>]) {
    let n = header.len().max(1);
    let gap = 14.0;
    let v = ui.visuals().clone();
    ui.add_space(4.0);
    egui::Frame::NONE
        .stroke(egui::Stroke::new(1.0_f32, v.widgets.noninteractive.bg_stroke.color))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            let avail = (ui.available_width() - gap * (n - 1) as f32).max(40.0 * n as f32);
            // the width of each column on one line (the marks of bold and code left out)
            let font = egui::TextStyle::Body.resolve(ui.style());
            let natural: Vec<f32> = (0..n)
                .map(|k| {
                    std::iter::once(header).chain(rows.iter().map(|r| r.as_slice())).filter_map(|r| r.get(k)).map(|c| {
                        let plain: String = c.chars().filter(|c| !matches!(c, '`' | '*')).collect();
                        ui.fonts_mut(|f| f.layout_no_wrap(plain, font.clone(), Color32::WHITE).size().x) + 6.0
                    }).fold(20.0_f32, f32::max)
                })
                .collect();
            let widths = column_widths(&natural, avail);
            let total: f32 = widths.iter().sum::<f32>() + gap * (n - 1) as f32;
            let stripe = icons::mix(v.panel_fill, v.text_color(), if v.dark_mode { 0.05 } else { 0.035 });
            let mut line = |ui: &mut egui::Ui, cells: &[String], strong: bool| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (k, w) in widths.iter().enumerate() {
                        let cell = cells.get(k).map(String::as_str).unwrap_or("");
                        ui.allocate_ui_with_layout(egui::vec2(*w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(*w);
                            if strong {
                                ui.label(RichText::new(cell.replace(['`', '*'], "")).strong());
                            } else {
                                remote_viewer().show(ui, cache, &remote(&html_text(cell)));
                            }
                        });
                    }
                })
                .response
                .rect
            };
            let head = line(ui, header, true);
            ui.painter().hline(head.left()..=head.left() + total, head.bottom() + 2.0, v.widgets.noninteractive.bg_stroke);
            ui.add_space(3.0);
            for (k, r) in rows.iter().enumerate() {
                let bg = ui.painter().add(egui::Shape::Noop);
                let rect = line(ui, r, false);
                if k % 2 == 1 {
                    let rect = egui::Rect::from_min_max(egui::pos2(rect.left() - 4.0, rect.top() - 1.0), egui::pos2(rect.left() + total + 4.0, rect.bottom() + 1.0));
                    ui.painter().set(bg, egui::Shape::rect_filled(rect, 2.0, stripe));
                }
            }
        });
    ui.add_space(4.0);
}

/// The widths of columns whose text on one line takes `natural`, in `avail`: all of them when
/// they fit; else the narrow ones keep theirs and the rest is shared in proportion.
fn column_widths(natural: &[f32], avail: f32) -> Vec<f32> {
    if natural.iter().sum::<f32>() <= avail {
        return natural.to_vec();
    }
    let mut kept = vec![false; natural.len()];
    loop {
        let free = avail - natural.iter().zip(&kept).filter(|(_, k)| **k).map(|(w, _)| w).sum::<f32>();
        let wide: Vec<usize> = (0..natural.len()).filter(|i| !kept[*i]).collect();
        let share = free / wide.len().max(1) as f32;
        let narrow: Vec<usize> = wide.iter().copied().filter(|i| natural[*i] <= share).collect();
        if narrow.is_empty() || narrow.len() == wide.len() {
            let sum: f32 = wide.iter().map(|i| natural[*i]).sum();
            return (0..natural.len()).map(|i| if kept[i] { natural[i] } else { (free * natural[i] / sum).max(40.0) }).collect();
        }
        for i in narrow {
            kept[i] = true;
        }
    }
}

/// The HTML of `text` written in Markdown, as the viewer leaves HTML out: `<br>`, `<b>`, `<img>`,
/// `<a href>`, `<details>` / `<summary>`, `<kbd>`…; the tags only around text (`<p align>`, `<div>`…)
/// taken away, the comments too. Code (blocks and `…`) is left as it is.
pub(super) fn html_to_md(text: &str) -> String {
    if !text.contains('<') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    for part in parts(text) {
        match part {
            MdPart::Text(t) => out.push_str(&html_text(&t)),
            MdPart::Code { lang, code } => {
                let fence = if code.contains("```") { "~~~~" } else { "```" };
                out.push_str(&format!("{fence}{lang}\n{code}\n{fence}\n"));
            }
            // not made by `parts`
            MdPart::Table { .. } => {}
        }
    }
    // `parts` ends each piece with a new line
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

fn html_text(t: &str) -> String {
    use std::sync::OnceLock;
    struct Res {
        comment: regex::Regex,
        br: regex::Regex,
        img: regex::Regex,
        src: regex::Regex,
        alt: regex::Regex,
        link: regex::Regex,
        summary: regex::Regex,
        heading: regex::Regex,
        hr: regex::Regex,
        pairs: Vec<(regex::Regex, &'static str)>,
        strip: regex::Regex,
    }
    static RES: OnceLock<Res> = OnceLock::new();
    let r = RES.get_or_init(|| {
        let re = |s: &str| regex::Regex::new(s).unwrap();
        Res {
            comment: re(r"(?s)<!--.*?-->"),
            br: re(r"(?i)<br\s*/?>"),
            img: re(r"(?i)<img\b[^>]*>"),
            src: re(r#"(?i)\bsrc\s*=\s*["']([^"']+)["']"#),
            alt: re(r#"(?i)\balt\s*=\s*["']([^"']*)["']"#),
            link: re(r#"(?is)<a\b[^>]*?\bhref\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a\s*>"#),
            summary: re(r"(?is)<summary\b[^>]*>(.*?)</summary\s*>"),
            heading: re(r"(?i)^\s*<h([1-6])\b[^>]*>(.*?)</h[1-6]\s*>\s*$"),
            hr: re(r"(?i)^\s*<hr\s*/?>\s*$"),
            pairs: vec![
                (re(r"(?i)</?(b|strong)\s*>"), "**"),
                (re(r"(?i)</?(i|em)\s*>"), "*"),
                (re(r"(?i)</?(code|kbd|tt|samp)\s*>"), "`"),
                (re(r"(?i)</?(s|del|strike)\s*>"), "~~"),
            ],
            strip: re(r"(?i)</?(p|div|span|details|center|sup|sub|u|ins|mark|small|big|font|section|article|header|footer|picture|source|a|nobr|wbr|abbr|cite|q|dl|dt|dd|figure|figcaption)\b[^>]*>"),
        }
    });
    let t = r.comment.replace_all(t, "");
    let mut out = String::with_capacity(t.len());
    for (n, line) in t.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        if !line.contains('<') {
            out.push_str(line);
            continue;
        }
        if r.hr.is_match(line) {
            out.push_str("---");
            continue;
        }
        let line = match r.heading.captures(line) {
            Some(c) => format!("{} {}", "#".repeat(c[1].parse().unwrap_or(1)), &c[2]),
            None => line.to_string(),
        };
        let in_table = line.trim_start().starts_with('|');
        // the code spans (between `) stay as they are
        for (k, seg) in line.split('`').enumerate() {
            if k > 0 {
                out.push('`');
            }
            if k % 2 == 1 {
                out.push_str(seg);
                continue;
            }
            let mut s = r.br.replace_all(seg, if in_table { " " } else { "  \n" }).into_owned();
            s = r
                .img
                .replace_all(&s, |c: &regex::Captures| {
                    let tag = &c[0];
                    let src = r.src.captures(tag).map(|m| m[1].to_string()).unwrap_or_default();
                    let alt = r.alt.captures(tag).map(|m| m[1].to_string()).unwrap_or_default();
                    if src.is_empty() { String::new() } else { format!("![{alt}]({src})") }
                })
                .into_owned();
            s = r.link.replace_all(&s, "[$2]($1)").into_owned();
            s = r.summary.replace_all(&s, "**▸ $1**").into_owned();
            for (re, md) in &r.pairs {
                s = re.replace_all(&s, *md).into_owned();
            }
            s = r.strip.replace_all(&s, "").into_owned();
            out.push_str(&s);
        }
    }
    out
}

/// Markdown cut at its `## ` headings: the text before the first one, then each section as
/// (its title, its text with the heading). A `## ` inside a code block is not a heading.
pub(super) fn sections(text: &str) -> (String, Vec<(String, String)>) {
    let mut intro = String::new();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut in_code = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if line.len() - t.len() <= 3 && (t.starts_with("```") || t.starts_with("~~~")) {
            in_code = !in_code;
        }
        if !in_code && line.starts_with("## ") {
            out.push((line[3..].trim().to_string(), String::new()));
        }
        match out.last_mut() {
            Some((_, body)) => body.push_str(line),
            None => intro.push_str(line),
        }
    }
    (intro, out)
}

/// Markdown, its code blocks with a « Copier » button.
/// The bare addresses of `text` (https://…) made into links, so a click opens them in the
/// browser; the ones already in a link (`<…>`, `[…](…)`) or in code (`…`) are left as they are.
/// The viewer for a text that comes from elsewhere (a pull request, a comment, Claude): its
/// images only from an address with a scheme (`remote`), never a path of this machine.
pub(super) fn remote_viewer() -> egui_commonmark::CommonMarkViewer<'static> {
    egui_commonmark::CommonMarkViewer::new().explicit_image_uri_scheme(true)
}

/// A text from elsewhere ready for `remote_viewer`: its addresses made links (`autolink`), and
/// only its http(s) images kept (an image `![](file:///dev/zero)` would be read from the disk
/// without end; on Windows `\\host\share\a.png` would connect to that machine).
pub(super) fn remote(text: &str) -> String {
    rewrite_images(&autolink(text), |dest| crate::os::web_url(dest).map(|_| dest.to_string()))
}

/// The options egui_commonmark parses with (its `parser_options`, not exported).
fn viewer_options() -> pulldown_cmark::Options {
    use pulldown_cmark::Options;
    Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_FOOTNOTES | Options::ENABLE_DEFINITION_LIST
}

/// The images of `text` given to `keep` as the viewer will see them: kept when it gives back the
/// same address, pointed at the address it gives, or written as their text when it gives None.
/// Parsed like the viewer does (`file\:///` or `file&#58;///` are only a `file:` address once
/// parsed: a search in the text would miss them).
pub(super) fn rewrite_images(text: &str, keep: impl Fn(&str) -> Option<String>) -> String {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let escape = |alt: &str| {
        let mut out = String::with_capacity(alt.len());
        for c in alt.chars() {
            match c {
                '\n' | '\r' => out.push(' '),
                c if c.is_ascii_punctuation() => {
                    out.push('\\');
                    out.push(c);
                }
                c => out.push(c),
            }
        }
        out
    };
    let mut text = text.to_string();
    // the addresses given by `keep`: kept as they are when the text is parsed again
    let mut given = std::collections::HashSet::new();
    // a rewrite can change how the text around is read: parsed again until nothing changes
    for _ in 0..4 {
        let mut edits = Vec::new();
        let mut events = Parser::new_ext(&text, viewer_options()).into_offset_iter();
        while let Some((ev, range)) = events.next() {
            let Event::Start(Tag::Image { dest_url, .. }) = ev else { continue };
            // its text up to its end; an image inside it (the viewer would load that one) left out
            let (mut alt, mut depth, mut nested) = (String::new(), 1, false);
            for (ev, _) in events.by_ref() {
                match ev {
                    Event::Start(Tag::Image { .. }) => {
                        depth += 1;
                        nested = true;
                    }
                    Event::End(TagEnd::Image) => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Event::Text(t) | Event::Code(t) => alt.push_str(&t),
                    _ => {}
                }
            }
            if given.contains(&*dest_url) && !nested {
                continue;
            }
            match keep(&dest_url) {
                Some(d) if d == *dest_url && !nested => {}
                Some(d) if !d.contains(['<', '>', '\n', '\r']) => {
                    edits.push((range, format!("![{}](<{d}>)", escape(&alt))));
                    given.insert(d);
                }
                _ => edits.push((range, escape(&alt))),
            }
        }
        if edits.is_empty() {
            return text;
        }
        for (range, with) in edits.into_iter().rev() {
            text.replace_range(range, &with);
        }
    }
    // still not settled: no image at all
    text.replace('!', "!\u{2060}")
}

/// Largest image file shown in a Markdown preview.
const PREVIEW_IMAGE_MAX: u64 = 20 * 1024 * 1024;

/// What an image `dest` of a Markdown file of folder `dir` shows in its preview: an http(s)
/// address as it is; a relative path when it is a regular file (not a link, not a device) of at
/// most 20 MB inside `root` (the project), as a `file://` address; nothing otherwise (another
/// scheme, an absolute or `\\host\share` path, a way out of the project).
pub(super) fn preview_image(dir: &Path, root: &Path, dest: &str) -> Option<String> {
    if crate::os::web_url(dest).is_some() {
        return Some(dest.to_string());
    }
    // `a.png?raw=true` (written for GitHub): the file is `a.png`
    let dest = dest.split(['?', '#']).next().unwrap_or("");
    let scheme = dest.split_once(':').is_some_and(|(s, _)| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)));
    let rel = Path::new(dest);
    if dest.is_empty() || scheme || rel.has_root() || rel.is_absolute() || dest.starts_with(['/', '\\']) || dest.contains('\0') {
        return None;
    }
    let path = dir.join(rel);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.file_type().is_file() || meta.len() > PREVIEW_IMAGE_MAX {
        return None;
    }
    let real = crate::os::canon(&path);
    real.starts_with(crate::os::canon(root)).then(|| format!("file://{}", real.display()))
}

pub(super) fn autolink(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let mut rest = line;
        let mut in_code = false;
        while let Some(at) = ["https://", "http://"].iter().filter_map(|p| rest.find(p)).min() {
            let (before, from) = rest.split_at(at);
            in_code ^= before.matches('`').count() % 2 == 1;
            out.push_str(before);
            let end = from.find(|c: char| c.is_whitespace() || "<>\"'`".contains(c)).unwrap_or(from.len());
            let mut url = &from[..end];
            // the punctuation that ends a sentence, or a ")" around the address, is not part of it
            loop {
                let t = url.trim_end_matches(['.', ',', ';', ':', '!', '?']);
                let t = if t.ends_with(')') && t.matches('(').count() < t.matches(')').count() { &t[..t.len() - 1] } else { t };
                if t.len() == url.len() {
                    break;
                }
                url = t;
            }
            // already a link: `<…>`, `[…](…)`, the text of a link, an HTML attribute (a lone « ( »
            // is a sentence in parentheses: « (https://…) » is made a link)
            let linked = before.ends_with(['<', '[', '"']) || before.ends_with("](");
            if in_code || linked || url.len() <= "https://".len() {
                out.push_str(url);
            } else {
                out.push('<');
                out.push_str(url);
                out.push('>');
            }
            rest = &from[url.len()..];
        }
        out.push_str(rest);
    }
    out
}

/// `hl`: to color the code blocks (None: plain).
pub(super) fn markdown(ui: &mut egui::Ui, cache: &mut egui_commonmark::CommonMarkCache, hl: Option<&Highlighting>, text: &str) {
    for (k, part) in blocks(text).into_iter().enumerate() {
        match part {
            // an id of its own: the viewer names what it draws from its place in the text it is given
            MdPart::Text(t) => {
                ui.push_id(k, |ui| remote_viewer().show(ui, cache, &remote(&html_text(&t))));
            }
            MdPart::Table { header, rows } => {
                ui.push_id(k, |ui| table(ui, cache, &header, &rows));
            }
            MdPart::Code { lang, code } if lang.eq_ignore_ascii_case("mermaid") && super::mermaid::show(ui, &code) => {}
            MdPart::Code { lang, code } => code_block(ui, hl, &lang, &code),
        }
    }
}

thread_local! {
    /// The colored code blocks already laid out: (key of language, code, theme and size) →
    /// (text, frame it was last drawn). Those not drawn for a while go.
    static CODE_JOBS: std::cell::RefCell<HashMap<u64, (egui::text::LayoutJob, u64)>> = Default::default();
}

/// The code colored for its language (as in the editor, with its theme), or None when the
/// language is not known.
fn code_job(ui: &egui::Ui, hl: &Highlighting, lang: &str, code: &str, font: &egui::FontId) -> Option<egui::text::LayoutJob> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (lang, code, hl.generation, &hl.theme_name, font.size.to_bits()).hash(&mut h);
    let key = h.finish();
    let frame = ui.ctx().cumulative_pass_nr();
    if let Some(job) = CODE_JOBS.with(|c| c.borrow_mut().get_mut(&key).map(|(j, f)| {
        *f = frame;
        j.clone()
    })) {
        return Some(job);
    }
    let spans = hl.code_spans(lang, code)?;
    let mut job = egui::text::LayoutJob::default();
    let format = |color| egui::TextFormat { font_id: font.clone(), color, ..Default::default() };
    let mut at = 0;
    for (color, r) in spans {
        if r.start > at {
            job.append(&code[at..r.start], 0.0, format(hl.colors.fg));
        }
        job.append(&code[r.clone()], 0.0, format(color));
        at = r.end;
    }
    if at < code.len() {
        job.append(&code[at..], 0.0, format(hl.colors.fg));
    }
    CODE_JOBS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 300 {
            c.retain(|_, (_, f)| frame.saturating_sub(*f) < 120);
        }
        c.insert(key, (job.clone(), frame));
    });
    Some(job)
}

/// A block of code: its language and the copy button on top, the code below, colored for its
/// language (scrolls sideways).
pub(super) fn code_block(ui: &mut egui::Ui, hl: Option<&Highlighting>, lang: &str, code: &str) {
    let v = ui.visuals().clone();
    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(v.extreme_bg_color)
        .stroke(egui::Stroke::new(1.0_f32, v.widgets.noninteractive.bg_stroke.color))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let shell = matches!(lang, "bash" | "sh" | "shell" | "console" | "fish" | "zsh");
                let what = if lang.is_empty() { "code" } else { lang };
                ui.label(RichText::new(what).small().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    copy_button(ui, code, if shell { t!("Copier la commande", "Copy the command") } else { t!("Copier", "Copy") });
                });
            });
            // id by place, not by content: it changes at each letter of an answer being written
            // (and two same blocks would share one); the bar under the code, not over its last line
            let id = ui.next_auto_id().with("code_block");
            ui.scope(|ui| {
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                egui::ScrollArea::horizontal().id_salt(id).show(ui, |ui| {
                    let font = egui::FontId::monospace(12.5);
                    match hl.and_then(|hl| code_job(ui, hl, lang, code, &font)) {
                        Some(job) => ui.add(egui::Label::new(job).extend()),
                        None => ui.add(egui::Label::new(RichText::new(code).font(font)).extend()),
                    };
                });
            });
        });
    ui.add_space(4.0);
}

/// « 🗐 label », then « ✔ Copié » a moment after a click.
pub(super) fn copy_button(ui: &mut egui::Ui, text: &str, label: &str) {
    let id = egui::Id::unique(("copied", text));
    let now = ui.input(|i| i.time);
    let recent = ui.data(|d| d.get_temp::<f64>(id)).is_some_and(|t| now - t < 1.5);
    let shown = if recent { t!("✔ Copié", "✔ Copied").to_string() } else { format!("🗐 {label}") };
    if ui.small_button(shown).on_hover_text(t!("Copier dans le presse-papiers", "Copy to the clipboard")).clicked() {
        ui.ctx().copy_text(text.to_string());
        ui.data_mut(|d| d.insert_temp(id, now));
    }
    if recent {
        ui.ctx().request_repaint_after(Duration::from_millis(300));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click on an address of a text (a comment of a pull request…) asks to open it in the browser.
    #[test]
    fn a_click_on_a_link_opens_it() {
        let ctx = egui::Context::default();
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let mut cache = egui_commonmark::CommonMarkCache::default();
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0)));
        let mut at = None;
        let mut opened = None;
        for frame in 0..6 {
            let mut events = Vec::new();
            if let Some(p) = at {
                events.push(egui::Event::PointerMoved(p));
                if frame >= 4 {
                    let pressed = frame == 4;
                    events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
                }
            }
            let out = crate::run_ui(&ctx, egui::RawInput { screen_rect: screen, events, ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    markdown(ui, &mut cache, Some(&hl), "Publiée : https://github.com/o/r/releases/tag/v1");
                });
            });
            // where the link is: the end of the line
            if at.is_none() {
                at = Some(egui::pos2(260.0, 18.0));
            }
            opened = opened.or(out.platform_output.commands.iter().find_map(|c| if let egui::OutputCommand::OpenUrl(u) = c { Some(u.url.clone()) } else { None }));
        }
        assert_eq!(opened.as_deref(), Some("https://github.com/o/r/releases/tag/v1"));
    }

    /// The images of a text from elsewhere: only http(s) ones, however the address is written
    /// (the parser decodes escapes and entities before the viewer sees the address).
    #[test]
    fn remote_images_are_only_web_ones() {
        let images = |md: &str| {
            let mut out = Vec::new();
            for ev in pulldown_cmark::Parser::new_ext(md, viewer_options()) {
                if let pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { dest_url, .. }) = ev {
                    out.push(dest_url.to_string());
                }
            }
            out
        };
        for bad in [
            "![a](file:///dev/zero)",
            "![a](file\\:///dev/zero)",
            "![a](file&#58;///dev/zero)",
            "![a](fil&#x65;:///dev/zero)",
            "![a](<file:///dev/zero>)",
            "![a](\\\\evil\\s\\a.png)",
            "![a](//evil/s/a.png)",
            "![a](/dev/zero)",
            "![a][r]\n\n[r]: file&#58;///dev/zero",
            "[![a](file:///dev/zero)](https://x.io)",
            "![![in](file:///dev/zero)](https://x.io/a.png)",
            "| a |\n|---|\n| ![x](file\\:///dev/zero) |",
            "<img src=\"file:///dev/zero\">",
        ] {
            let md = remote(&html_to_md(bad));
            assert!(images(&md).iter().all(|d| d.starts_with("https://")), "{bad} → {md}");
        }
        assert_eq!(images(&remote("![logo](https://x.io/a.png) et ![b](file:///dev/zero)")), ["https://x.io/a.png"]);
        // the text of a refused image stays
        assert_eq!(remote("voir ![le schéma](file:///x.png)"), "voir le schéma");
    }

    #[cfg(unix)]
    #[test]
    fn preview_images_are_files_of_the_project() {
        let root = std::env::temp_dir().join(format!("florm_md_img_{}", std::process::id()));
        let docs = root.join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(root.join("a.png"), b"png").unwrap();
        std::fs::write(docs.join("b.png"), b"png").unwrap();
        let _ = std::fs::remove_file(docs.join("zero.png"));
        std::os::unix::fs::symlink("/dev/zero", docs.join("zero.png")).unwrap();
        let at = |dest: &str| preview_image(&docs, &root, dest);
        let canon = crate::os::canon(&root);
        assert_eq!(at("b.png"), Some(format!("file://{}", canon.join("docs/b.png").display())));
        assert_eq!(at("../a.png?raw=true"), Some(format!("file://{}", canon.join("a.png").display())));
        assert_eq!(at("https://x.io/a.png").as_deref(), Some("https://x.io/a.png"));
        for bad in ["zero.png", "../../../../../../dev/zero", "/dev/zero", "/etc/hostname", "file:///dev/zero", "data:image/png;base64,AA", "../", "\\\\evil\\s\\a.png", "missing.png"] {
            assert_eq!(at(bad), None, "{bad}");
        }
        // what the preview shows: the refused image as its text
        let md = rewrite_images("![z](zero.png) ![b](b.png)", |d| preview_image(&docs, &root, d));
        assert_eq!(md, format!("z ![b](<file://{}>)", canon.join("docs/b.png").display()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_html_is_written_in_markdown() {
        assert_eq!(html_to_md("a<br/>b"), "a  \nb");
        assert_eq!(html_to_md("| a<br>b | c |"), "| a b | c |");
        assert_eq!(html_to_md("<b>gras</b> et <em>penché</em>, <kbd>Ctrl</kbd>"), "**gras** et *penché*, `Ctrl`");
        assert_eq!(html_to_md(r#"<p align="center"><img src="logo.png" alt="Logo" width="80"></p>"#), "![Logo](logo.png)");
        assert_eq!(html_to_md(r#"voir <a href="https://x.io">le site</a>"#), "voir [le site](https://x.io)");
        assert_eq!(html_to_md("<details>\n<summary>Plus</summary>\n\ncaché\n</details>"), "\n**▸ Plus**\n\ncaché\n");
        assert_eq!(html_to_md("<h2>Titre</h2>\n<!-- note\nsur deux lignes -->fin\n<hr>"), "## Titre\nfin\n---");
        // code and text that is not HTML stay as they are
        assert_eq!(html_to_md("`<br>` et Vec<String>"), "`<br>` et Vec<String>");
        assert_eq!(html_to_md("```html\n<b>x</b>\n```\n"), "```html\n<b>x</b>\n```\n");
    }

    #[test]
    fn bare_addresses_become_links() {
        assert_eq!(autolink("Suivi : https://github.com/a/b/runs/1."), "Suivi : <https://github.com/a/b/runs/1>.");
        assert_eq!(autolink("(voir https://x.io/a_(b))"), "(voir <https://x.io/a_(b)>)");
        // right after a parenthesis, as Claude often writes it
        assert_eq!(autolink("en cours (https://github.com/o/r/actions/runs/4) ;"), "en cours (<https://github.com/o/r/actions/runs/4>) ;");
        // already links, or code: unchanged
        for t in ["<https://x.io>", "[x](https://x.io)", "[https://x.io](https://x.io)", "`curl https://x.io`", "a `b` https://x.io `c`"] {
            let want = if t.starts_with("a `b`") { "a `b` <https://x.io> `c`".to_string() } else { t.to_string() };
            assert_eq!(autolink(t), want);
        }
    }

    #[test]
    fn code_blocks_apart() {
        let t = "Lance :\n```bash\ncargo test\n```\nPuis :\n~~~\na\n\nb\n~~~\nfin";
        assert_eq!(
            parts(t),
            vec![
                MdPart::Text("Lance :\n".into()),
                MdPart::Code { lang: "bash".into(), code: "cargo test".into() },
                MdPart::Text("Puis :\n".into()),
                MdPart::Code { lang: String::new(), code: "a\n\nb".into() },
                MdPart::Text("fin\n".into()),
            ]
        );
        // being written: the block not closed yet is code
        assert_eq!(parts("```py\nx = 1"), vec![MdPart::Code { lang: "py".into(), code: "x = 1".into() }]);
        // a fence of 4 backticks holds a line of 3
        assert_eq!(parts("````md\n```\n````"), vec![MdPart::Code { lang: "md".into(), code: "```".into() }]);
        assert_eq!(parts("pas de code"), vec![MdPart::Text("pas de code\n".into())]);
    }

    #[test]
    fn sections_at_headings() {
        let t = "# Titre\n\nAccueil\n\n## Un\na\n```\n## pas un titre\n```\n## Deux\nb";
        let (intro, secs) = sections(t);
        assert_eq!(intro, "# Titre\n\nAccueil\n\n");
        assert_eq!(
            secs,
            vec![
                ("Un".to_string(), "## Un\na\n```\n## pas un titre\n```\n".to_string()),
                ("Deux".to_string(), "## Deux\nb".to_string()),
            ]
        );
        assert_eq!(sections("sans titre"), ("sans titre".to_string(), vec![]));
    }

    #[test]
    fn tables_apart() {
        let t = "Avant\n\n| A | `a|b` |\n|:--|--:|\n| 1 | x \\| y |\n| 2 |\n\nAprès\n```sh\nls\n```\n| pas | une table |\n";
        let b = blocks(t);
        assert_eq!(b.len(), 5, "{b:?}");
        assert_eq!(b[0], MdPart::Text("Avant\n\n".into()));
        // a | in code or escaped stays in its cell; a short row is filled
        assert_eq!(b[1], MdPart::Table { header: vec!["A".into(), "`a|b`".into()], rows: vec![vec!["1".into(), "x | y".into()], vec!["2".into(), String::new()]] });
        assert!(matches!(&b[2], MdPart::Text(t) if t.starts_with("\nAprès")));
        // no line of dashes under it: not a table
        assert!(matches!(&b[3], MdPart::Code { .. }));
        assert!(matches!(&b[4], MdPart::Text(t) if t.starts_with("| pas |")));
        assert_eq!(column_widths(&[50.0, 100.0], 300.0), vec![50.0, 100.0]);
        // the narrow column keeps its width, the two wide ones share the rest in proportion
        let w = column_widths(&[50.0, 600.0, 300.0], 350.0);
        assert_eq!(w[0], 50.0);
        assert!((w[1] - 200.0).abs() < 0.1 && (w[2] - 100.0).abs() < 0.1, "{w:?}");
    }

    /// Two wide tables, a code block between (as in an answer of Claude): nothing goes past the
    /// width, the text after them included.
    #[test]
    fn wide_tables_stay_in_the_width() {
        let long = "une cellule avec beaucoup de texte qui doit revenir à la ligne dans sa colonne ".repeat(3);
        let text = format!(
            "| | A | B |\n|---|---|---|\n| Isolation | {long} | `saas create --product syndic` {long} |\n\n```\nPOST /projects\n```\n\n| Ordre | Contenu |\n|---|---|\n| ✓ | {long} |\n\n1. {long}\n"
        );
        let ctx = egui::Context::default();
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let mut cache = egui_commonmark::CommonMarkCache::default();
        let width = 600.0;
        let mut right = 0.0;
        for _ in 0..3 {
            let _ = crate::run_ui(&ctx, egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 2000.0))), ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    markdown(ui, &mut cache, Some(&hl), &text);
                    right = ui.min_rect().right();
                });
            });
        }
        assert!(right <= width, "the answer goes to x = {right}, past {width}");
    }
}
