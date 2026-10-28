//! Claude right in the editor, on the selection (Ctrl+I): a request in a small window, Claude
//! proposes the new code (shown as a diff) or answers the question. Nothing changes in the file
//! before « Remplacer » (Ctrl+Z undoes it), and « Continuer dans le chat » hands it to the Claude tab.

use super::*;
use crate::claude::{self, Inline};

pub(super) struct InlineAsk {
    /// Number of the question: a late answer to an earlier one is ignored.
    n: u64,
    /// Document (uid), its file, and the range asked about (chars) with its text then.
    uid: u64,
    path: Option<PathBuf>,
    start: usize,
    end: usize,
    original: String,
    /// Lines (1-based) of the range, for the title.
    lines: (usize, usize),
    pub(super) ask: String,
    busy: bool,
    result: Option<Result<Inline, String>>,
    focus: bool,
}

/// What the window asked for during the frame.
enum InlineAct {
    Send,
    Replace,
    ToChat,
    Close,
}

impl App {
    /// The window on the selection of the active editor (else on the line of the cursor).
    pub(super) fn inline_ai_open(&mut self) {
        let Some(d) = self.docs.get(self.active).filter(|d| d.view == document::View::Text) else { return };
        let s = d.primary_sel();
        let (mut a, mut b) = (s.start(), s.end());
        if a == b {
            // no selection: the whole line of the cursor
            let l = d.rope.char_to_line(a);
            a = d.rope.line_to_char(l);
            b = d.rope.line_to_char(l) + d.rope.line(l).len_chars();
        }
        let first = d.rope.char_to_line(a);
        let last = d.rope.char_to_line(b.saturating_sub(usize::from(b > a))).max(first);
        let ask = self.inline.take().map(|x| x.ask).unwrap_or_default();
        self.inline = Some(InlineAsk {
            n: 0,
            uid: d.uid,
            path: d.path.clone(),
            start: a,
            end: b,
            original: d.slice(a, b),
            lines: (first + 1, last + 1),
            ask,
            busy: false,
            result: None,
            focus: true,
        });
    }

    pub(super) fn inline_ai_loaded(&mut self, n: u64, r: Result<Inline, String>) {
        if let Some(st) = self.inline.as_mut().filter(|st| st.n == n) {
            st.busy = false;
            st.result = Some(r);
        }
    }

    pub(super) fn inline_ai_ui(&mut self, ctx: &egui::Context) {
        let accent = self.hl.colors.accent;
        let cache = &mut self.md_cache;
        let Some(st) = self.inline.as_mut() else { return };
        let mut act = None;
        let mut open = true;
        let name = st.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| t!("sans titre", "untitled").into());
        let place = if st.lines.0 == st.lines.1 { format!("{name}:{}", st.lines.0) } else { format!("{name}:{}-{}", st.lines.0, st.lines.1) };
        egui::Window::new(RichText::new(format!("✳ Claude · {place}")).strong())
            .id(egui::Id::new("inline_ai"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .anchor(Align2::CENTER_TOP, [0.0, 70.0])
            .show(ctx, |ui| {
                let resp = crate::input::add(ui,
                    egui::TextEdit::singleline(&mut st.ask)
                        .hint_text(t!("Que faire ? « gère le cas vide », « simplifie », « explique »… (Entrée)", "What to do? “handle the empty case”, “simplify”, “explain”… (Enter)"))
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut st.focus) {
                    resp.request_focus();
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !st.ask.trim().is_empty() && !st.busy {
                    act = Some(InlineAct::Send);
                }
                ui.horizontal(|ui| {
                    let label = if st.result.is_some() { t!("Redemander", "Ask again") } else { t!("Proposer", "Suggest") };
                    if ui.add_enabled(!st.busy && !st.ask.trim().is_empty(), filled_button(label, accent)).on_hover_text(t!("Entrée", "Enter")).clicked() {
                        act = Some(InlineAct::Send);
                    }
                    if ui
                        .add_enabled(!st.ask.trim().is_empty(), egui::Button::new(t!("Continuer dans le chat", "Go on in the chat")))
                        .on_hover_text(t!("Donne la sélection et ta demande à l'onglet Claude (à relire puis envoyer)", "Gives the selection and your request to the Claude tab (to read over then send)"))
                        .clicked()
                    {
                        act = Some(InlineAct::ToChat);
                    }
                    if st.busy {
                        spinner(ui);
                        ui.label(RichText::new(t!("Claude réfléchit…", "Claude is thinking…")).weak());
                    }
                });
                match &st.result {
                    None => {}
                    Some(Err(e)) => {
                        ui.add_space(4.0);
                        ui.label(RichText::new(format!("⚠ {e}")).color(err()));
                    }
                    Some(Ok(Inline::Explain(text))) => {
                        ui.separator();
                        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                            super::md::markdown(ui, cache, text);
                        });
                    }
                    Some(Ok(Inline::Code(code))) => {
                        ui.separator();
                        ui.label(RichText::new(t!("Proposition (rien n'est changé avant « Remplacer ») :", "Suggestion (nothing changes before “Replace”):")).small().weak());
                        egui::ScrollArea::both().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
                            diff_lines(ui, &st.original, code);
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!st.busy, filled_button(t!("✔ Remplacer", "✔ Replace"), ok())).on_hover_text(t!("Remplace la sélection (Ctrl+Z pour annuler)", "Replaces the selection (Ctrl+Z to undo)")).clicked() {
                                act = Some(InlineAct::Replace);
                            }
                            super::md::copy_button(ui, code, t!("Copier", "Copy"));
                            if ui.button(t!("Annuler", "Cancel")).clicked() {
                                act = Some(InlineAct::Close);
                            }
                        });
                    }
                }
            });
        if !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            act = Some(InlineAct::Close);
        }
        match act {
            Some(InlineAct::Send) => self.inline_ai_send(),
            Some(InlineAct::Replace) => self.inline_ai_replace(),
            Some(InlineAct::ToChat) => self.inline_ai_to_chat(),
            Some(InlineAct::Close) => self.inline = None,
            None => {}
        }
    }

    pub(super) fn inline_ai_send(&mut self) {
        static N: AtomicU64 = AtomicU64::new(1);
        let Some(st) = self.inline.as_ref() else { return };
        let Some(d) = self.docs.iter().find(|d| d.uid == st.uid) else { return };
        // 40 lines around, for the context
        let (first, last) = (st.lines.0 - 1, st.lines.1 - 1);
        let n_lines = d.rope.len_lines();
        let before: String = (first.saturating_sub(40)..first).map(|l| d.line_text(l)).collect::<Vec<_>>().join("\n");
        let after: String = ((last + 1).min(n_lines)..(last + 41).min(n_lines)).map(|l| d.line_text(l)).collect::<Vec<_>>().join("\n");
        let dir = st.path.as_deref().and_then(|p| self.repo_for(p)).or_else(|| self.root.clone()).unwrap_or_else(|| PathBuf::from("."));
        let file = st.path.as_ref().map(|p| p.strip_prefix(&dir).unwrap_or(p).display().to_string()).unwrap_or_else(|| t!("sans titre", "untitled").into());
        let (n, selection, ask) = (N.fetch_add(1, Ordering::Relaxed), st.original.clone(), st.ask.trim().to_string());
        if let Some(st) = self.inline.as_mut() {
            st.n = n;
            st.busy = true;
            st.result = None;
        }
        self.jobs.spawn(move || JobResult::InlineAi(n, claude::inline(&dir, &file, &before, &selection, &after, &ask)));
    }

    /// The proposed code instead of the range (found again if the file changed since).
    fn inline_ai_replace(&mut self) {
        let Some(st) = self.inline.as_ref() else { return };
        let Some(Ok(Inline::Code(code))) = &st.result else { return };
        let Some(i) = self.docs.iter().position(|d| d.uid == st.uid) else {
            self.set_status(t!("Le fichier a été fermé", "The file was closed"));
            return;
        };
        let d = &mut self.docs[i];
        let (a, b) = if d.slice(st.start, st.end.min(d.rope.len_chars())) == st.original {
            (st.start, st.end)
        } else {
            let text = d.rope.to_string();
            match text.find(&st.original) {
                Some(pos) => {
                    let a = text[..pos].chars().count();
                    (a, a + st.original.chars().count())
                }
                None => {
                    self.set_status(t!("La sélection a changé depuis la demande : rien n'est remplacé", "The selection changed since the request: nothing is replaced"));
                    return;
                }
            }
        };
        // the end of line of the selection stays
        let mut new = code.clone();
        if st.original.ends_with('\n') && !new.ends_with('\n') {
            new.push('\n');
        }
        d.replace_range(a, b, &new);
        d.scroll_to_cursor = true;
        self.inline = None;
        self.activate_tab(i);
        self.pending_focus = true;
        self.set_status(t!("Remplacé par la proposition de Claude (Ctrl+Z pour annuler)", "Replaced by Claude's suggestion (Ctrl+Z to undo)"));
    }

    /// The selection and the request handed to the Claude tab (not sent).
    fn inline_ai_to_chat(&mut self) {
        let Some(st) = self.inline.take() else { return };
        let Some(i) = self.docs.iter().position(|d| d.uid == st.uid) else { return };
        if st.path.is_some() {
            self.claude_attach_lines(i, st.lines.0 - 1, st.lines.1 - 1);
        }
        let dir = st.path.as_deref().and_then(|p| self.repo_for(p)).or_else(|| self.claude.dir.clone()).or_else(|| self.root.clone());
        match dir {
            Some(dir) => self.claude_prefill(dir, st.ask.trim().to_string()),
            None => self.open_claude(),
        }
    }
}

/// The selection and the proposal line by line: removed in red, added in green.
fn diff_lines(ui: &mut egui::Ui, old: &str, new: &str) {
    let v = ui.visuals().clone();
    let base = v.extreme_bg_color;
    let font = egui::FontId::monospace(12.5);
    egui::Frame::none().fill(base).rounding(6.0).inner_margin(egui::Margin::symmetric(6.0, 4.0)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let diff = similar::TextDiff::from_lines(old, new);
        for change in diff.iter_all_changes() {
            let (sign, color, tint) = match change.tag() {
                similar::ChangeTag::Delete => ('-', err(), Some(icons::mix(base, err(), 0.16))),
                similar::ChangeTag::Insert => ('+', ok(), Some(icons::mix(base, ok(), 0.16))),
                similar::ChangeTag::Equal => (' ', v.weak_text_color(), None),
            };
            let line = change.value().trim_end_matches(['\n', '\r']).replace('\t', "    ");
            let mut text = RichText::new(format!("{sign} {line}")).font(font.clone()).color(color);
            if let Some(t) = tint {
                text = text.background_color(t);
            }
            ui.add(egui::Label::new(text).extend());
        }
    });
}
