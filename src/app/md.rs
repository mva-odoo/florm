//! Markdown whose code blocks are drawn apart, each with a button to copy it (a command, code).

use super::*;

/// A piece of Markdown: text, or a fenced code block (its language and its code).
#[derive(Debug, PartialEq)]
pub(super) enum MdPart {
    Text(String),
    Code { lang: String, code: String },
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
pub(super) fn markdown(ui: &mut egui::Ui, cache: &mut egui_commonmark::CommonMarkCache, text: &str) {
    for part in parts(text) {
        match part {
            MdPart::Text(t) => {
                egui_commonmark::CommonMarkViewer::new().show(ui, cache, &t);
            }
            MdPart::Code { lang, code } => code_block(ui, &lang, &code),
        }
    }
}

/// Markdown whose tasks (`- [ ]`, `- [x]`) have a checkbox to tick them off; the other lines are plain markdown. `checks`
/// keeps the boxes changed by a click, by message (`msg`) and item text: a box starts as its
/// marker says (`[x]`: ticked).
pub(super) fn markdown_checks(ui: &mut egui::Ui, cache: &mut egui_commonmark::CommonMarkCache, text: &str, checks: &mut HashSet<(usize, String)>, msg: usize) {
    for part in parts(text) {
        let t = match part {
            MdPart::Text(t) => t,
            MdPart::Code { lang, code } => {
                code_block(ui, &lang, &code);
                continue;
            }
        };
        let lines: Vec<&str> = t.lines().collect();
        let mut prose = String::new();
        let mut i = 0;
        while i < lines.len() {
            let Some(item) = list_item(lines[i]) else {
                prose.push_str(lines[i]);
                prose.push('\n');
                i += 1;
                continue;
            };
            if !prose.trim().is_empty() {
                egui_commonmark::CommonMarkViewer::new().show(ui, cache, &prose);
            }
            prose.clear();
            // the indented lines after the item are part of it (its sub-items and text)
            let mut body = item.text.to_string();
            i += 1;
            while i < lines.len() && lines[i].starts_with("  ") && !lines[i].trim().is_empty() {
                body.push('\n');
                body.push_str(lines[i].trim_start());
                i += 1;
            }
            let key = (msg, item.text.to_string());
            let mut now = item.checked ^ checks.contains(&key);
            ui.horizontal_top(|ui| {
                if ui.checkbox(&mut now, "").changed() {
                    if now != item.checked {
                        checks.insert(key.clone());
                    } else {
                        checks.remove(&key);
                    }
                }
                ui.vertical(|ui| {
                    egui_commonmark::CommonMarkViewer::new().show(ui, cache, &body);
                });
            });
        }
        if !prose.trim().is_empty() {
            egui_commonmark::CommonMarkViewer::new().show(ui, cache, &prose);
        }
    }
}

/// A task of the top level: its box (`[x]` ticked) and its text.
struct ListItem<'a> {
    checked: bool,
    text: &'a str,
}

fn list_item(line: &str) -> Option<ListItem<'_>> {
    // indented lines belong to the item above
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let rest = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
        .or_else(|| {
            let d = line.find(". ")?;
            (d > 0 && line[..d].bytes().all(|b| b.is_ascii_digit())).then(|| &line[d + 2..])
        })?;
    // only a task (`- [ ]`, `- [x]`) has a box: a plain bullet stays text
    let (checked, text) = match rest.get(..3) {
        Some("[ ]") => (false, &rest[3..]),
        Some("[x]") | Some("[X]") => (true, &rest[3..]),
        _ => return None,
    };
    Some(ListItem { checked, text: text.trim() })
}

/// A block of code: its language and the copy button on top, the code below (scrolls sideways).
pub(super) fn code_block(ui: &mut egui::Ui, lang: &str, code: &str) {
    let v = ui.visuals().clone();
    ui.add_space(4.0);
    egui::Frame::none()
        .fill(v.extreme_bg_color)
        .stroke(egui::Stroke::new(1.0_f32, v.widgets.noninteractive.bg_stroke.color))
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
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
                    ui.add(egui::Label::new(RichText::new(code).monospace().size(12.5)).extend());
                });
            });
        });
    ui.add_space(4.0);
}

/// « 🗐 label », then « ✔ Copié » a moment after a click.
pub(super) fn copy_button(ui: &mut egui::Ui, text: &str, label: &str) {
    let id = egui::Id::new(("copied", text));
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
mod list_tests {
    use super::*;

    #[test]
    fn list_items_and_their_boxes() {
        assert!(list_item("- [x] fait").is_some_and(|i| i.checked && i.text == "fait"));
        assert!(list_item("- [ ] à faire").is_some_and(|i| !i.checked && i.text == "à faire"));
        // plain bullets, indented lines and prose have no box
        assert!(list_item("- faire ceci").is_none());
        assert!(list_item("2. Deux").is_none());
        assert!(list_item("  - [ ] sous-point").is_none());
        assert!(list_item("Une phrase.").is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
