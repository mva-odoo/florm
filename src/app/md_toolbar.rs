//! The Markdown toolbar above the source of a .md file: its buttons add the syntax around the
//! selection, or in front of its lines. The source stays what is edited: the text can always be
//! typed and changed by hand.

use super::*;
use crate::document::{self, Document, Edit, EditKind};

/// The buttons of the toolbar, in order: (label, tip, what it does).
const FORMATS: [(&str, &str, Fmt); 13] = [
    ("B", "Gras : **texte**", Fmt::Wrap("**", "**")),
    ("I", "Italique : *texte*", Fmt::Wrap("*", "*")),
    ("S", "Barré : ~~texte~~", Fmt::Wrap("~~", "~~")),
    ("`c`", "Code en ligne : `code`", Fmt::Wrap("`", "`")),
    ("Lien", "Lien : [texte](adresse)", Fmt::Link),
    ("H1", "Titre de niveau 1", Fmt::Line(Prefix::Heading(1))),
    ("H2", "Titre de niveau 2", Fmt::Line(Prefix::Heading(2))),
    ("H3", "Titre de niveau 3", Fmt::Line(Prefix::Heading(3))),
    ("•", "Liste à puces", Fmt::Line(Prefix::Bullet)),
    ("1.", "Liste numérotée", Fmt::Line(Prefix::Numbered)),
    ("☐", "Case à cocher (tâche)", Fmt::Line(Prefix::Task)),
    ("❝", "Citation", Fmt::Line(Prefix::Quote)),
    ("```", "Bloc de code", Fmt::CodeBlock),
];

/// What a button does to the text.
#[derive(Clone, Copy)]
enum Fmt {
    /// The text between two markers (the selection, or the cursor between them).
    Wrap(&'static str, &'static str),
    Link,
    CodeBlock,
    /// A horizontal rule, a table to fill in.
    Rule,
    Table,
    /// A prefix at the start of each line of the selection.
    Line(Prefix),
}

/// A kind of line: its prefix, and how the prefixes of other kinds are recognised and removed.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Prefix {
    Heading(usize),
    Bullet,
    Numbered,
    Quote,
    Task,
}

impl Prefix {
    /// The prefix of the line `k` (its number in the list, for a numbered list).
    fn text(self, k: usize) -> String {
        match self {
            Prefix::Heading(n) => format!("{} ", "#".repeat(n)),
            Prefix::Bullet => "- ".into(),
            Prefix::Numbered => format!("{}. ", k + 1),
            Prefix::Quote => "> ".into(),
            Prefix::Task => "- [ ] ".into(),
        }
    }

    /// The prefix a line starts with (of any kind), and the rest of the line after it.
    fn split(line: &str) -> (Option<Prefix>, &str) {
        let body = line.trim_start();
        let bullets = [("- [ ] ", Prefix::Task), ("- [x] ", Prefix::Task), ("- ", Prefix::Bullet), ("* ", Prefix::Bullet), ("> ", Prefix::Quote)];
        if let Some((p, k)) = bullets.into_iter().find(|(p, _)| body.starts_with(p)) {
            return (Some(k), &body[p.len()..]);
        }
        let hashes = body.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && body[hashes..].starts_with(' ') {
            return (Some(Prefix::Heading(hashes)), &body[hashes + 1..]);
        }
        let digits = body.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && body[digits..].starts_with(". ") {
            return (Some(Prefix::Numbered), &body[digits + 2..]);
        }
        (None, body)
    }
}

/// Whether the document is Markdown (by its syntax, or its .md extension).
pub(super) fn is_markdown(d: &Document) -> bool {
    d.syntax.contains("Markdown") || d.path.as_deref().is_some_and(|p| p.extension().is_some_and(|e| e == "md" || e == "markdown"))
}

/// The lines with `kind` set on each, or removed when they all have it already. The indentation
/// stays; the prefix of another kind is replaced.
fn prefixed(lines: &[String], kind: Prefix) -> Vec<String> {
    let all_have = lines.iter().all(|l| Prefix::split(l).0 == Some(kind));
    lines
        .iter()
        .enumerate()
        .map(|(k, l)| {
            let indent = &l[..l.len() - l.trim_start().len()];
            let rest = Prefix::split(l).1;
            if all_have {
                format!("{indent}{rest}")
            } else {
                format!("{indent}{}{rest}", kind.text(k))
            }
        })
        .collect()
}

/// Applies `fmt` to the main selection of `d`: the syntax around it, or on its lines.
fn md_format(d: &mut Document, fmt: Fmt) {
    let sel = d.sels.get(d.primary).copied().unwrap_or(document::Sel::cursor(0));
    let (a, b) = (sel.start(), sel.end());
    match fmt {
        Fmt::Wrap(open, close) => {
            let inner = d.slice(a, b);
            let text = format!("{open}{inner}{close}");
            let (o, n) = (open.chars().count(), inner.chars().count());
            d.edit(EditKind::Other, vec![Edit { start: a, end: b, text, anchor: o, head: o + n }]);
        }
        Fmt::Link => {
            let inner = d.slice(a, b);
            let label = if inner.is_empty() { t!("texte", "text").to_string() } else { inner.clone() };
            let text = format!("[{label}](https://)");
            let anchor = 1 + label.chars().count() + 2;
            d.edit(EditKind::Other, vec![Edit { start: a, end: b, text: text.clone(), anchor, head: anchor + "https://".len() }]);
        }
        Fmt::CodeBlock => {
            let inner = d.slice(a, b);
            let text = format!("```\n{inner}\n```");
            let (o, n) = (4, inner.chars().count());
            d.edit(EditKind::Other, vec![Edit { start: a, end: b, text, anchor: o, head: o + n }]);
        }
        Fmt::Rule => {
            let text = "\n---\n".to_string();
            let n = text.chars().count();
            d.edit(EditKind::Other, vec![Edit { start: a, end: b, text, anchor: n, head: n }]);
        }
        Fmt::Table => {
            let text = format!("| {c} | {c} |\n| --- | --- |\n|  |  |\n", c = t!("Colonne", "Column"));
            let anchor = 2;
            let head = anchor + t!("Colonne", "Column").chars().count();
            d.edit(EditKind::Other, vec![Edit { start: a, end: b, text, anchor, head }]);
        }
        Fmt::Line(kind) => {
            let (first, last) = (d.rope.char_to_line(a), d.rope.char_to_line(b));
            // a selection ending at the start of a line does not include that line
            let last = if b > a && b == d.rope.line_to_char(last) && last > first { last - 1 } else { last };
            let lines: Vec<String> = (first..=last).map(|l| d.line_text(l)).collect();
            let fresh = prefixed(&lines, kind);
            let start = d.rope.line_to_char(first);
            let end = d.rope.line_to_char(last) + d.line_len(last);
            let text = fresh.join("\n");
            let n = text.chars().count();
            d.edit(EditKind::Other, vec![Edit { start, end, text, anchor: 0, head: n }]);
        }
    }
}

impl App {
    /// The toolbar of a Markdown source, under its breadcrumbs. Nothing for other files.
    pub(super) fn md_toolbar_ui(&mut self, ui: &mut egui::Ui, i: usize, pane: u8) {
        if !self.settings.markdown_toolbar {
            return;
        }
        let Some(d) = self.docs.get(i) else { return };
        if d.view != document::View::Text || !is_markdown(d) {
            return;
        }
        let mut act = None;
        let bg = icons::mix(ui.visuals().panel_fill, ui.visuals().strong_text_color(), 0.04);
        egui::TopBottomPanel::top(egui::Id::new("md_toolbar").with(pane))
            .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(6.0, 3.0)))
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (label, tip, fmt) in FORMATS {
                        if ui.small_button(label).on_hover_text(tip).clicked() {
                            act = Some(fmt);
                        }
                    }
                    ui.separator();
                    if ui.small_button("――").on_hover_text(t!("Ligne horizontale", "Horizontal rule")).clicked() {
                        act = Some(Fmt::Rule);
                    }
                    if ui.small_button("▦").on_hover_text(t!("Tableau (à compléter)", "Table (to fill in)")).clicked() {
                        act = Some(Fmt::Table);
                    }
                });
            });
        if let Some(fmt) = act {
            self.md_apply(i, fmt);
        }
    }

    /// Applies `fmt` to the main selection of document `i`, and gives the editor the focus back.
    fn md_apply(&mut self, i: usize, fmt: Fmt) {
        let Some(d) = self.docs.get_mut(i) else { return };
        md_format(d, fmt);
        self.pending_focus = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_prefix_is_set_then_removed_on_the_same_kind() {
        assert_eq!(prefixed(&lines(&["a", "b"]), Prefix::Bullet), lines(&["- a", "- b"]));
        assert_eq!(prefixed(&lines(&["- a", "- b"]), Prefix::Bullet), lines(&["a", "b"]));
        assert_eq!(prefixed(&lines(&["a", "b"]), Prefix::Numbered), lines(&["1. a", "2. b"]));
    }

    #[test]
    fn another_prefix_is_replaced_and_the_indentation_kept() {
        assert_eq!(prefixed(&lines(&["## Titre"]), Prefix::Heading(1)), lines(&["# Titre"]));
        assert_eq!(prefixed(&lines(&["  - sous-point"]), Prefix::Quote), lines(&["  > sous-point"]));
        assert_eq!(prefixed(&lines(&["# Titre"]), Prefix::Heading(1)), lines(&["Titre"]));
    }

    #[test]
    fn the_kinds_of_line_are_recognised() {
        assert_eq!(Prefix::split("- [ ] tâche").0, Some(Prefix::Task));
        assert_eq!(Prefix::split("### Titre").0, Some(Prefix::Heading(3)));
        assert_eq!(Prefix::split("12. item").0, Some(Prefix::Numbered));
        assert_eq!(Prefix::split("#hashtag").0, None);
    }

    #[test]
    fn the_syntax_wraps_the_selection_and_keeps_it_selected() {
        let mut d = Document::from_text(1, "un mot ici\n", 4, true);
        d.sels = vec![document::Sel::range(3, 6)];
        d.primary = 0;
        md_format(&mut d, Fmt::Wrap("**", "**"));
        assert_eq!(d.rope.to_string(), "un **mot** ici\n");
        // the selection is the word again, between the markers
        assert_eq!((d.sels[0].start(), d.sels[0].end()), (5, 8));
    }

    #[test]
    fn without_a_selection_the_cursor_goes_between_the_markers() {
        let mut d = Document::from_text(1, "ab\n", 4, true);
        d.sels = vec![document::Sel::cursor(1)];
        md_format(&mut d, Fmt::Wrap("*", "*"));
        assert_eq!(d.rope.to_string(), "a**b\n");
        assert_eq!((d.sels[0].start(), d.sels[0].end()), (2, 2));
    }

    #[test]
    fn a_list_applies_to_the_lines_selected() {
        let mut d = Document::from_text(1, "a\nb\nc\n", 4, true);
        d.sels = vec![document::Sel::range(0, 3)];
        md_format(&mut d, Fmt::Line(Prefix::Bullet));
        assert_eq!(d.rope.to_string(), "- a\n- b\nc\n");
    }
}
