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
                    copy_button(ui, code, if shell { "Copier la commande" } else { "Copier" });
                });
            });
            egui::ScrollArea::horizontal().id_salt(("code_block", code)).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(code).monospace().size(12.5)).extend());
            });
        });
    ui.add_space(4.0);
}

/// « 🗐 label », then « ✔ Copié » a moment after a click.
pub(super) fn copy_button(ui: &mut egui::Ui, text: &str, label: &str) {
    let id = egui::Id::new(("copied", text));
    let now = ui.input(|i| i.time);
    let recent = ui.data(|d| d.get_temp::<f64>(id)).is_some_and(|t| now - t < 1.5);
    let shown = if recent { "✔ Copié".to_string() } else { format!("🗐 {label}") };
    if ui.small_button(shown).on_hover_text("Copier dans le presse-papiers").clicked() {
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
}
