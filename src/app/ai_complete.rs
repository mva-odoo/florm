//! AI completion: when typing pauses at the end of a line, Haiku suggests what follows, shown in
//! grey after the cursor (`Document::ghost`). Tab takes it, Escape or any other key drops it; typing
//! its first letters keeps the rest. Alt+\ asks at once, even when it is turned off.

use super::*;

/// Pause in typing before a suggestion is asked.
const PAUSE: Duration = Duration::from_millis(700);

#[derive(Default)]
pub(super) struct AiComplete {
    /// When to ask, after the last change (None: nothing to ask).
    at: Option<Instant>,
    /// Number of the last request: a late answer, or one for a text changed since, is ignored.
    n: u64,
    /// What the request in flight was asked for: (number, document, version, cursor).
    asked: Option<(u64, u64, u64, usize)>,
    /// The state of the active document seen at the last frame: (document, version, cursor).
    seen: Option<(u64, u64, usize)>,
    /// Asked by the shortcut: even turned off, and the next answer says when there is nothing.
    manual: bool,
    /// The last error was shown in the status bar (not again at each pause).
    error_shown: bool,
}

impl App {
    fn ai_complete_on(&self) -> bool {
        self.settings.ai_complete && crate::features::on(crate::features::Feature::Claude)
    }

    /// The active document, when a suggestion can go at its cursor: text, editable, one cursor
    /// without selection, nothing but closing characters after it on the line.
    fn ai_complete_doc(&self) -> Option<&Document> {
        let d = self.docs.get(self.active)?;
        if d.read_only || d.view != document::View::Text || d.sels.len() != 1 || !d.primary_sel().is_empty() {
            return None;
        }
        let head = d.primary_sel().head;
        let (l, col) = d.line_col(head);
        let rest: String = d.line_text(l).chars().skip(col).collect();
        rest.chars().all(|c| c.is_whitespace() || ")]}>\"'`;:,".contains(c)).then_some(d)
    }

    fn ai_complete_clear(&mut self) {
        for d in &mut self.docs {
            d.ghost = None;
        }
    }

    /// Each frame, after the keys: a change waits for the pause, a move drops the suggestion, the
    /// pause over sends the request.
    pub(super) fn ai_complete_frame(&mut self, ctx: &egui::Context) {
        let on = self.ai_complete_on();
        // turned off: only a suggestion asked by Alt+\ is followed
        if !on && !self.ai.manual && self.ai.asked.is_none() && self.docs.iter().all(|d| d.ghost.is_none()) {
            return;
        }
        let focused = ctx.memory(|m| m.has_focus(editor_id()));
        let Some(d) = self.docs.get_mut(self.active) else { return };
        let state = (d.uid, d.version, d.primary_sel().head);
        if self.ai.seen != Some(state) {
            let was = self.ai.seen.replace(state);
            let changed = was.is_some_and(|(uid, v, _)| uid == state.0 && v != state.1);
            // typed the first letters of the suggestion: the rest stays
            let kept = match (&d.ghost, changed) {
                (Some((at, text)), true) if state.2 > *at => {
                    let typed = d.slice(*at, state.2);
                    text.strip_prefix(typed.as_str()).filter(|r| !r.is_empty()).map(|r| (state.2, r.to_string()))
                }
                _ => None,
            };
            let keeping = kept.is_some();
            d.ghost = kept;
            self.ai.asked = None;
            self.ai.at = (changed && on && !keeping).then(|| Instant::now() + PAUSE);
            if !keeping {
                for o in self.docs.iter_mut().filter(|o| o.uid != state.0) {
                    o.ghost = None;
                }
            }
        }
        if self.lsp.completion.is_some() || !focused {
            // the list of the language server has Tab
            self.ai.at = None;
            return;
        }
        match self.ai.at {
            Some(t) if Instant::now() >= t => {
                self.ai.at = None;
                self.ai_complete_ask();
            }
            Some(t) => ctx.request_repaint_after(t.saturating_duration_since(Instant::now())),
            None => {}
        }
    }

    /// Sends the lines around the cursor to Haiku.
    fn ai_complete_ask(&mut self) {
        let Some(d) = self.ai_complete_doc() else {
            self.ai.manual = false;
            return;
        };
        let head = d.primary_sel().head;
        let line = d.rope.char_to_line(head);
        // 50 lines before, 15 after: enough to guess, few tokens
        let from = d.rope.line_to_char(line.saturating_sub(50));
        let to = d.rope.line_to_char((line + 16).min(d.rope.len_lines()));
        let (before, after) = (d.slice(from, head), d.slice(head, to));
        let key = (d.uid, d.version, head);
        let dir = d.path.as_deref().and_then(|p| self.repo_for(p)).or_else(|| self.root.clone()).unwrap_or_else(std::env::temp_dir);
        let file = d.path.as_ref().map(|p| p.strip_prefix(&dir).unwrap_or(p).display().to_string()).unwrap_or_else(|| d.syntax.clone());
        let accounts = self.claude_accounts();
        let account = match self.settings.ai_complete_account.as_str() {
            "" => crate::claude_account::find(&accounts, &self.settings.claude_account).dir.clone(),
            name => crate::claude_account::find(&accounts, name).dir.clone(),
        };
        self.ai.n += 1;
        let n = self.ai.n;
        self.ai.asked = Some((n, key.0, key.1, key.2));
        self.jobs.spawn(move || JobResult::AiComplete(n, crate::claude::complete(&dir, account.as_deref(), &file, &before, &after)));
    }

    /// The suggestion came: shown if the text and the cursor did not change since.
    pub(super) fn ai_complete_loaded(&mut self, n: u64, r: Result<String, String>) {
        let manual = std::mem::take(&mut self.ai.manual);
        let Some((_, uid, version, head)) = self.ai.asked.filter(|a| a.0 == n) else { return };
        self.ai.asked = None;
        match r {
            Ok(text) if !text.is_empty() => {
                self.ai.error_shown = false;
                if let Some(d) = self.docs.get_mut(self.active).filter(|d| d.uid == uid && d.version == version && d.primary_sel().head == head) {
                    d.ghost = Some((head, text));
                }
            }
            Ok(_) => {
                if manual {
                    self.set_status(t!("Claude n'a rien à proposer ici", "Claude has nothing to suggest here"));
                }
            }
            Err(e) => {
                if manual || !self.ai.error_shown {
                    self.ai.error_shown = true;
                    self.set_error(tf!("Complétion par IA : {}", "AI completion: {}", e.lines().next().unwrap_or("")));
                }
            }
        }
    }

    /// Tab takes the suggestion, Escape drops it (before the other shortcuts see them).
    pub(super) fn ai_complete_keys(&mut self, ctx: &egui::Context) {
        if self.lsp.completion.is_some() || !ctx.memory(|m| m.has_focus(editor_id())) {
            return;
        }
        let Some(i) = self.docs.get(self.active).filter(|d| d.ghost.as_ref().is_some_and(|(at, _)| *at == d.primary_sel().head)).map(|_| self.active) else { return };
        if ctx.input_mut(|inp| inp.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.docs[i].ghost = None;
            return;
        }
        if !ctx.input_mut(|inp| inp.consume_key(egui::Modifiers::NONE, Key::Tab)) {
            return;
        }
        let d = &mut self.docs[i];
        let Some((at, text)) = d.ghost.take() else { return };
        let end = at + text.chars().count();
        d.replace_range(at, at, &text);
        d.sels = vec![Sel::cursor(end)];
        d.primary = 0;
        // taken: no new suggestion right after it (it would spend tokens without being asked)
        self.ai.seen = Some((d.uid, d.version, end));
        self.ai.at = None;
    }

    /// Alt+\: a suggestion at once, even when the completion is turned off.
    pub(super) fn ai_complete_now(&mut self) {
        if self.ai_complete_doc().is_none() {
            self.set_status(t!("La complétion par IA propose en fin de ligne, sans sélection", "AI completion suggests at the end of a line, without a selection"));
            return;
        }
        self.ai.manual = true;
        self.ai.at = None;
        self.ai_complete_ask();
    }

    pub(super) fn ai_complete_toggle(&mut self) {
        self.settings.ai_complete = !self.settings.ai_complete;
        self.persist("ai_complete", self.settings.ai_complete.to_string());
        if !self.settings.ai_complete {
            self.ai_complete_clear();
            self.ai.at = None;
        }
        self.set_status(if self.settings.ai_complete {
            t!("Complétion par IA : activée (Tab prend la proposition)", "AI completion: on (Tab takes the suggestion)")
        } else {
            t!("Complétion par IA : désactivée", "AI completion: off")
        });
    }
}
