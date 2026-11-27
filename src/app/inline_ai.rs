//! Inline edit with Claude (Ctrl+I), as in Cursor, Zed or VS Code: a field above the selection (or
//! the cursor line) for the request; Claude rewrites only those lines, its text put in place on a
//! green background with « ✓ Garder » (Ctrl+Enter), « × Annuler » (Escape) and « ↻ Redemander ».
//! Typing elsewhere keeps it; Ctrl+Z undoes it like any change.

use super::*;

#[derive(Default)]
pub(super) struct InlineAi {
    /// The field open: document, its lines (first, last), the request typed.
    pub(super) ask: Option<Ask>,
    /// Number of the last request sent: an answer for an older one (cancelled) is dropped.
    pub(super) n: u64,
}

pub(super) struct Ask {
    uid: u64,
    lines: (usize, usize),
    input: String,
    /// Sent, Claude writing: since when.
    waiting: Option<Instant>,
    error: Option<String>,
    focus: bool,
}

impl App {
    /// Ctrl+I: the field of the request above the lines of the selection (or of the cursor).
    pub(super) fn inline_ai_open(&mut self, request: String) {
        let Some(d) = self.docs.get_mut(self.active).filter(|d| d.view == document::View::Text) else { return };
        if d.read_only {
            self.set_status(t!("Fichier en lecture seule", "Read-only file"));
            return;
        }
        // a proposal still shown: kept first
        d.proposal = None;
        let s = d.primary_sel();
        let a = d.rope.char_to_line(s.start());
        let mut b = d.rope.char_to_line(s.end());
        // a selection ending at the start of a line does not take that line
        if b > a && s.end() == d.rope.line_to_char(b) {
            b -= 1;
        }
        self.inline_ai.ask = Some(Ask { uid: d.uid, lines: (a, b), input: request, waiting: None, error: None, focus: true });
    }

    fn inline_ai_send(&mut self) {
        let Some(ask) = self.inline_ai.ask.as_mut() else { return };
        let request = ask.input.trim().to_string();
        if request.is_empty() || ask.waiting.is_some() {
            return;
        }
        let Some(d) = self.docs.iter().find(|d| d.uid == ask.uid) else { return };
        let Some(path) = d.path.clone() else {
            ask.error = Some(t!("Enregistre d'abord le fichier : Claude a besoin de son nom", "Save the file first: Claude needs its name").into());
            return;
        };
        let (a, b) = ask.lines;
        let last = d.rope.len_lines().saturating_sub(1);
        let (a, b) = (a.min(last), b.min(last));
        let start = d.rope.line_to_char(a);
        let end = d.rope.line_to_char(b) + d.line_len(b);
        let (uid, version, text) = (d.uid, d.version, d.rope.to_string());
        ask.waiting = Some(Instant::now());
        ask.error = None;
        self.inline_ai.n += 1;
        let n = self.inline_ai.n;
        let dir = self.repo_for(&path).or_else(|| self.root.clone()).or_else(|| path.parent().map(Path::to_path_buf)).unwrap_or_else(config::home);
        let name = path.strip_prefix(&dir).unwrap_or(&path).display().to_string();
        self.jobs.spawn(move || {
            let result = crate::claude::inline_edit(&dir, &name, &text, (a, b), &request);
            JobResult::InlineEdit { n, uid, version, start, end, request, result }
        });
    }

    /// Claude's answer: put in place (on a green background) if the text did not change meanwhile.
    pub(super) fn inline_edit_ready(&mut self, n: u64, uid: u64, version: u64, (start, end): (usize, usize), request: String, result: Result<String, String>) {
        if n != self.inline_ai.n || self.inline_ai.ask.as_ref().is_none_or(|a| a.uid != uid) {
            return;
        }
        let text = match result {
            Ok(t) => t,
            Err(e) => {
                if let Some(a) = self.inline_ai.ask.as_mut() {
                    a.waiting = None;
                    a.error = Some(e);
                }
                return;
            }
        };
        let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) else { return };
        if d.version != version {
            if let Some(a) = self.inline_ai.ask.as_mut() {
                a.waiting = None;
                a.error = Some(t!("Le fichier a changé pendant que Claude écrivait : redemande", "The file changed while Claude was writing: ask again").into());
            }
            return;
        }
        let old = d.slice(start, end);
        if old == text {
            if let Some(a) = self.inline_ai.ask.as_mut() {
                a.waiting = None;
                a.error = Some(t!("Claude n'a rien changé", "Claude changed nothing").into());
            }
            return;
        }
        d.replace_range(start, end, &text);
        let new_end = start + text.chars().count();
        d.sels = vec![Sel::cursor(new_end)];
        d.primary = 0;
        d.scroll_to_cursor = true;
        d.proposal = Some(document::Proposal { start, end: new_end, old, request, version: d.version });
        self.inline_ai.ask = None;
        self.pending_focus = true;
    }

    /// Keeps (true) or undoes the proposal of the active document.
    fn inline_ai_decide(&mut self, keep: bool) {
        let Some(d) = self.docs.get_mut(self.active) else { return };
        let Some(p) = d.proposal.take() else { return };
        if !keep && d.version == p.version {
            d.replace_range(p.start, p.end, &p.old);
            d.sels = vec![Sel::cursor(p.start)];
            d.primary = 0;
        }
        self.pending_focus = true;
    }

    /// Escape and Ctrl+Enter of the inline edit, before the shortcuts of Florm take them (Escape
    /// closes panels, Ctrl+Enter inserts a line).
    pub(super) fn inline_ai_keys(&mut self, ctx: &egui::Context) {
        if self.inline_ai.ask.is_some() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                // an answer still coming is dropped
                self.inline_ai.ask = None;
                self.inline_ai.n += 1;
                self.pending_focus = true;
            }
            return;
        }
        let has = self.docs.get(self.active).is_some_and(|d| d.proposal.as_ref().is_some_and(|p| p.version == d.version));
        if !has || self.overlay.is_some() || !ctx.memory(|m| m.has_focus(editor_id())) {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Enter)) {
            self.inline_ai_decide(true);
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.inline_ai_decide(false);
        }
    }

    /// The field of the request, the wait, and the bar of a proposal: each frame.
    pub(super) fn inline_ai_ui(&mut self, ctx: &egui::Context) {
        // a proposal kept by any other change of the text
        let asking = self.inline_ai.ask.as_ref().map(|a| (a.uid, a.lines));
        for d in &mut self.docs {
            if d.proposal.as_ref().is_some_and(|p| p.version != d.version) {
                d.proposal = None;
            }
            // the lines asked, for the editor to place the field
            d.ai_lines = asking.filter(|(uid, _)| *uid == d.uid).map(|(_, l)| l);
        }
        self.inline_ai_proposal_bar(ctx);
        self.inline_ai_field(ctx);
    }

    fn inline_ai_field(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.inline_ai.ask.as_ref() else { return };
        let Some(d) = self.docs.get(self.active).filter(|d| d.uid == ask.uid) else {
            // its tab left: the request is dropped
            self.inline_ai.ask = None;
            return;
        };
        // above the first line of the passage, at the left of the text (where the editor drew it)
        let Some((x, y, _)) = d.proposal_screen else { return };
        let accent = self.hl.colors.accent;
        let (mut send, mut close) = (false, false);
        let waiting = ask.waiting;
        let error = ask.error.clone();
        let lines = ask.lines;
        let focus = ask.focus;
        egui::Area::new(egui::Id::unique("inline_ai_field"))
            .order(egui::Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(egui::pos2(x, y - 2.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                    ui.set_width(520.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("✳").color(accent).strong());
                        let what = if lines.0 == lines.1 { tf!("ligne {}", "line {}", lines.0 + 1) } else { tf!("lignes {}–{}", "lines {}–{}", lines.0 + 1, lines.1 + 1) };
                        ui.label(RichText::new(tf!("Claude modifie la {what}", "Claude changes {what}")).small().weak());
                    });
                    let Some(ask) = self.inline_ai.ask.as_mut() else { return };
                    let r = ui.add_enabled(
                        waiting.is_none(),
                        egui::TextEdit::multiline(&mut ask.input)
                            .desired_rows(1)
                            .desired_width(f32::INFINITY)
                            .hint_text(t!("Ce qu'il faut changer (Entrée : envoyer, Maj+Entrée : nouvelle ligne, Échap : fermer)", "What to change (Enter: send, Shift+Enter: new line, Escape: close)")),
                    );
                    if focus {
                        r.request_focus();
                        ask.focus = false;
                    }
                    if r.has_focus() {
                        let (enter, shift) = ui.input(|i| (i.key_pressed(Key::Enter), i.modifiers.shift));
                        if enter && !shift {
                            // the new line Enter typed is taken back
                            if ask.input.ends_with('\n') {
                                ask.input.pop();
                            }
                            send = true;
                        }
                    }
                    ui.horizontal(|ui| {
                        if let Some(t) = waiting {
                            spinner(ui);
                            ui.label(RichText::new(tf!("Claude écrit… {} s", "Claude is writing… {} s", t.elapsed().as_secs())).small().weak());
                            ui.ctx().request_repaint_after(Duration::from_millis(250));
                        } else if let Some(e) = &error {
                            ui.label(RichText::new(format!("⚠ {e}")).small().color(highlight::red()));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text(t!("Fermer (Échap)", "Close (Escape)")).clicked() {
                                close = true;
                            }
                            if waiting.is_none() && ui.add(filled_button(t!("Envoyer", "Send"), accent).small()).clicked() {
                                send = true;
                            }
                        });
                    });
                });
            });
        if close {
            // an answer still coming is dropped
            self.inline_ai.ask = None;
            self.inline_ai.n += 1;
            self.pending_focus = true;
        } else if send {
            self.inline_ai_send();
        }
    }

    fn inline_ai_proposal_bar(&mut self, ctx: &egui::Context) {
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(p) = d.proposal.clone() else { return };
        let (mut keep, mut undo, mut again) = (false, false, false);
        if let Some((x, top, _)) = d.proposal_screen {
            let accent = self.hl.colors.accent;
            let mono = egui::FontId::monospace(self.settings.font_size * 0.9);
            egui::Area::new(egui::Id::unique("inline_ai_bar"))
                .order(egui::Order::Foreground)
                .pivot(Align2::LEFT_BOTTOM)
                .fixed_pos(egui::pos2(x, top - 1.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("✳").color(accent));
                            if ui.add(filled_button(t!("✓ Garder", "✓ Keep"), highlight::green()).small()).on_hover_text("Ctrl+Entrée").clicked() {
                                keep = true;
                            }
                            if ui.small_button(t!("× Annuler", "× Undo")).on_hover_text(t!("Échap : remettre l'ancien texte", "Escape: put the old text back")).clicked() {
                                undo = true;
                            }
                            if ui.small_button(t!("↻ Redemander", "↻ Ask again")).on_hover_text(t!("Remet l'ancien texte et rouvre la demande", "Puts the old text back and opens the request again")).clicked() {
                                again = true;
                            }
                            let old = if p.old.is_empty() { t!("(rien : du code ajouté)", "(nothing: code added)").to_string() } else { p.old.clone() };
                            ui.label(RichText::new(t!("Ancien", "Old")).small().weak().underline()).on_hover_ui(|ui| {
                                ui.set_max_width(700.0);
                                let shown: String = old.lines().take(40).collect::<Vec<_>>().join("\n");
                                egui::Frame::NONE.fill(highlight::red().gamma_multiply(0.15)).inner_margin(6).show(ui, |ui| {
                                    ui.label(RichText::new(shown).font(mono.clone()));
                                });
                            });
                        });
                    });
                });
        }
        if again {
            let request = p.request.clone();
            self.inline_ai_decide(false);
            // the lines of the old text selected, the request written again
            if let Some(d) = self.docs.get_mut(self.active) {
                let end = (p.start + p.old.chars().count()).min(d.len());
                d.sels = vec![Sel::range(p.start, end)];
                d.primary = 0;
            }
            self.inline_ai_open(request);
        } else if keep || undo {
            self.inline_ai_decide(keep);
        }
    }
}
