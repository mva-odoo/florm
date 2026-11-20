//! Claude Code itself in a terminal, in the middle of the Claude tab (Préférences → Claude dans un
//! terminal): started for its conversation, what Florm gives it pasted into its field, and what
//! its hooks tell read back (works, waits, done; its session, title, model and cost). See
//! `crate::claude_term`.

use super::*;
use crate::claude_term;

/// How something given to Claude is written into its field: `@file` (Claude Code reads it), an
/// image by its path, a text (lines, diff, problems) as it is.
pub(super) fn term_text(a: &Attach, dir: Option<&Path>) -> String {
    if let Some((label, p)) = &a.image {
        return format!("{label} : {} ", p.display());
    }
    if a.note.is_none() && a.lines.is_none() {
        if let Some(rel) = rel_path(dir, &a.path).filter(|r| !r.is_empty()) {
            return format!("@{rel} ");
        }
    }
    format!("{}\n", a.prompt(dir))
}

/// A variable that Claude Code gives to what it runs (its session, its process): set, the `claude`
/// of the terminal would take itself for one of its children.
fn child_var(k: &str) -> bool {
    matches!(k, "CLAUDECODE" | "CLAUDE_CODE_ENTRYPOINT" | "CLAUDE_CODE_EXECPATH" | "CLAUDE_PID") || k.starts_with("CLAUDE_CODE_CHILD") || k.starts_with("CLAUDE_CODE_SESSION")
}

impl App {
    /// The terminal of the conversation shown, started if it has none yet: `claude` with the
    /// settings of Florm, going on with its session if it has one.
    pub(in crate::app) fn claude_term_start(&mut self) {
        // the tests draw the Claude tab: they do not run the real `claude`
        if self.claude.term.is_some() || cfg!(test) {
            return;
        }
        let Some(dir) = self.claude.dir.clone() else { return };
        static N: AtomicU64 = AtomicU64::new(1);
        let key = (u64::from(std::process::id()) << 20) | N.fetch_add(1, Ordering::Relaxed);
        let (file, settings_file) = claude_term::files(key);
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("florm"));
        let _ = std::fs::create_dir_all(claude_term::dir());
        let _ = std::fs::remove_file(&file);
        if let Err(e) = std::fs::write(&settings_file, claude_term::settings(&exe, &file).to_string()) {
            self.claude.term = Some(0);
            self.set_error(tf!("Claude : réglages non écrits ({e})", "Claude: settings not written ({e})"));
            return;
        }
        let accounts = self.claude_accounts();
        let account_dir = claude_account::find(&accounts, &self.claude.account).dir.clone();
        // a session its account no longer knows (deleted, another account): a new conversation
        // rather than `claude` stopping at once on « No conversation found »
        let mut session = self.claude.session_id.clone().unwrap_or_default();
        if !session.is_empty() && !claude_account::has_session(account_dir.as_deref(), &session) {
            session.clear();
            self.claude.session_id = None;
        }
        let prompt = self.claude.term_prompt.take();
        let argv = claude_term::argv(&settings_file, &session, prompt.as_deref());
        let mut env: Vec<(String, String)> = account_dir.iter().map(|d| ("CLAUDE_CONFIG_DIR".to_string(), d.to_string_lossy().into_owned())).collect();
        // Florm started from a terminal where Claude Code runs: its `claude` would think it is a
        // child of that one (and keep no transcript, so no resuming): those variables taken out
        env.extend(std::env::vars().map(|(k, _)| k).filter(|k| child_var(k)).map(|k| (k, String::new())));
        let ctx = self.egui_ctx.clone();
        match self.term.open_embedded(&ctx, &argv, dir, &env) {
            Ok(id) => {
                self.claude.term = Some(id);
                self.claude.term_file = Some(file);
                self.claude.focus = true;
            }
            Err(e) => {
                // not tried again at each frame: the error says what to do
                self.claude.term = Some(0);
                self.set_error(tf!("Claude : {e}", "Claude: {e}"));
            }
        }
    }

    /// `text` sent to the conversation shown, in its terminal: pasted then Enter (or it starts
    /// with it, when it has no terminal yet).
    pub(in crate::app) fn claude_term_send(&mut self, text: String) -> bool {
        match self.claude.term {
            Some(id) if self.term.ready(id) => {
                self.term.paste(id, &text);
                self.term.type_text(id, "\r");
            }
            Some(_) => {
                self.claude.term_pending.push_str(&text);
                self.claude.term_submit = true;
            }
            None => {
                self.claude.term_prompt = Some(text);
                self.claude_term_start();
            }
        }
        true
    }

    /// Text given to Claude in a terminal (✳ in the margin, + Contexte, a picture…): pasted into
    /// its field, not sent: the message goes on from there.
    pub(in crate::app) fn claude_term_give(&mut self, text: String, label: &str) {
        if self.claude.term.is_none() {
            self.claude_term_start();
        }
        match self.claude.term {
            Some(id) if self.term.ready(id) => {
                self.term.paste(id, &text);
            }
            _ => self.claude.term_pending.push_str(&text),
        }
        self.set_status(tf!("✳ {label} écrit dans le champ de Claude — Ctrl+Alt+C pour y aller", "✳ {label} written in Claude's field — Ctrl+Alt+C to go there"));
    }

    /// What the hooks of each conversation in a terminal wrote: its state in the list and the
    /// status bar, its title, its session (to go on after a restart), its cost and context; the
    /// text waiting to be pasted, once `claude` shows its field; the terminals of the
    /// conversations closed.
    pub(in crate::app) fn claude_term_tick(&mut self) {
        if !self.settings.claude_terminal {
            return;
        }
        // a few times a second is enough
        if self.claude_term_read.is_some_and(|t| t.elapsed() < Duration::from_millis(300)) {
            return;
        }
        self.claude_term_read = Some(Instant::now());
        let sound = self.settings.claude_done_sound.clone();
        let mut save = false;
        let term = &mut self.term;
        for (k, c) in std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).enumerate() {
            let (Some(id), Some(file)) = (c.term, c.term_file.clone()) else { continue };
            if !c.term_pending.is_empty() && term.ready(id) {
                let text = std::mem::take(&mut c.term_pending);
                term.paste(id, &text);
                if std::mem::take(&mut c.term_submit) {
                    term.type_text(id, "\r");
                }
            }
            let st = claude_term::read(&file);
            if st == c.term_state {
                continue;
            }
            let old = std::mem::replace(&mut c.term_state, st.clone());
            if st.state != old.state {
                match st.state.as_str() {
                    "busy" => {
                        c.busy = true;
                        c.term_waiting = false;
                    }
                    "waiting" => {
                        c.busy = false;
                        c.term_waiting = true;
                        c.unseen = true;
                        c.done_at = Some(Instant::now());
                    }
                    "done" => {
                        c.busy = false;
                        c.term_waiting = false;
                        c.unseen = true;
                        c.done_at = Some(Instant::now());
                        crate::sound::play(&sound);
                    }
                    _ => {}
                }
            }
            if !st.session_id.is_empty() && c.session_id.as_deref() != Some(st.session_id.as_str()) {
                c.session_id = Some(st.session_id.clone());
                // Enter once `claude` ended goes on with this session
                term.set_command(id, claude_term::argv(&file.with_extension("settings.json"), &st.session_id, None));
                save |= k == 0;
            }
            if !st.title.is_empty() && !c.items.iter().any(|i| matches!(i, Item::User(..))) {
                c.items.push(Item::User(st.title.clone(), Vec::new(), Vec::new(), Vec::new()));
                c.wish.get_or_insert(st.title.clone());
                save |= k == 0;
            }
            c.cost = st.cost;
            if st.context > 0 {
                c.context = (st.context, if st.context > 200_000 { 1_000_000 } else { 200_000 });
            }
        }
        // the terminals of the conversations closed
        let alive: Vec<u64> = std::iter::once(&self.claude).chain(&self.claude_others).filter_map(|c| c.term).collect();
        for id in self.term.embedded_ids() {
            if !alive.contains(&id) {
                self.term.close(id);
            }
        }
        if save {
            self.claude_save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_of_a_parent_claude_are_left_out() {
        for k in ["CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_SESSION_ID", "CLAUDE_PID"] {
            assert!(child_var(k), "{k}");
        }
        for k in ["CLAUDE_CONFIG_DIR", "CLAUDE_EFFORT", "CLAUDE_CODE_USE_BEDROCK"] {
            assert!(!child_var(k), "{k}");
        }
    }

    #[test]
    fn what_goes_into_the_field() {
        let dir = PathBuf::from("/p");
        let file = Attach { path: dir.join("src/a.py"), lines: None, note: None, image: None };
        assert_eq!(term_text(&file, Some(&dir)), "@src/a.py ");
        let lines = Attach { path: dir.join("src/a.py"), lines: Some((3, 4, "x = 1\ny = 2".into())), note: None, image: None };
        assert!(term_text(&lines, Some(&dir)).contains("src/a.py:3-4") && term_text(&lines, Some(&dir)).contains("```py\nx = 1"));
        let img = Attach::image(dir.clone(), ("Capture de Florm".into(), PathBuf::from("/tmp/c.png")));
        assert_eq!(term_text(&img, Some(&dir)), "Capture de Florm : /tmp/c.png ");
    }
}
