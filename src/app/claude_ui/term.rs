//! Claude Code itself in a terminal, in the middle of the Claude tab: started for its
//! conversation, what Florm gives it pasted into its field, and what its hooks tell read back
//! (works, waits, done; its session, title, model, cost and quotas). See `crate::claude_term`.

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
    /// What Florm knows of the project of `dir`, for Claude's system prompt (`claude::florm_context`):
    /// the commit rules, the branch.
    fn florm_context(&self, dir: &Path) -> String {
        let repo = self.repo_for(dir).unwrap_or_else(|| dir.to_path_buf());
        let branch = git::head_branch(dir).map(|b| (self.repo_label(&repo), b));
        claude::florm_context(&claude::FlormContext { commit_rules: claude::commit_rules(dir), repo: branch, version: self.version.clone() })
    }

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
        let _ = std::fs::remove_file(claude_term::files_of(&file));
        if let Err(e) = std::fs::write(&settings_file, claude_term::settings(&exe, &file, self.settings.claude_sandbox).to_string()) {
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
        let system = self.florm_context(&dir);
        let fork = std::mem::take(&mut self.claude.forked) && !session.is_empty();
        let plugin = crate::skills::plugin_dir();
        let argv = claude_term::argv(&settings_file, &claude_term::Start { session_id: &session, fork, prompt: prompt.as_deref(), system: &system, plugin: plugin.as_deref() });
        if fork {
            // the copy gets a session of its own, said by its hooks
            self.claude.session_id = None;
        }
        let mut env: Vec<(String, String)> = account_dir.iter().map(|d| ("CLAUDE_CONFIG_DIR".to_string(), d.to_string_lossy().into_owned())).collect();
        // Florm started from a terminal where Claude Code runs: its `claude` would think it is a
        // child of that one (and keep no transcript, so no resuming): those variables taken out
        env.extend(std::env::vars().map(|(k, _)| k).filter(|k| child_var(k)).map(|k| (k, String::new())));
        // the conversation in the terminal's history, not in a full screen of its own: the wheel,
        // the scrollbar, Maj+↑ and the search (Ctrl+Maj+F) go through all of it
        env.push(("CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN".to_string(), "1".to_string()));
        let ctx = self.egui_ctx.clone();
        match self.term.open_embedded(&ctx, &argv, dir.clone(), &env) {
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

    /// The `claude` of the conversation shown stopped: started again at the next frame (in its
    /// folder, with its account, going on with its session).
    pub(in crate::app) fn claude_term_stop(&mut self) {
        if let Some(id) = self.claude.term.take().filter(|id| *id != 0) {
            self.term.close(id);
        }
        self.claude.term_file = None;
        self.claude.term_state = Default::default();
        self.claude.term_waiting = false;
    }

    /// `text` sent to Claude (a request from a pull request, a conflict…), as a message of the
    /// conversation shown.
    pub(in crate::app) fn claude_send_text(&mut self, text: String) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        if self.claude.dir.is_none() {
            self.set_error(t!("Choisis d'abord le dossier où Claude travaille", "First choose the folder Claude works in"));
            return false;
        }
        self.claude_term_send(text)
    }

    /// `text` sent to the conversation shown, in its terminal: pasted then Enter (or it starts
    /// with it, when it has no terminal yet).
    fn claude_term_send(&mut self, text: String) -> bool {
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
        // a few times a second is enough
        if self.claude_term_read.is_some_and(|t| t.elapsed() < Duration::from_millis(300)) {
            return;
        }
        self.claude_term_read = Some(Instant::now());
        let sound = self.settings.claude_done_sound.clone();
        let mut save: Vec<usize> = Vec::new();
        let mut restart: Vec<usize> = Vec::new();
        // files changed by Claude (read again at once)
        let mut touched = false;
        // the hooks tell the system themselves (at once, even while Florm is hidden and not drawn)
        claude_term::set_away(!self.egui_ctx.input(|i| i.focused));
        let mut limits: Vec<(String, Vec<(String, f32, i64)>)> = Vec::new();
        let accounts = claude_account::accounts(&self.settings.claude_accounts);
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
            let listed = std::fs::metadata(claude_term::files_of(&file)).map_or(0, |m| m.len());
            if listed != c.changed_len {
                c.changed_len = listed;
                for p in claude_term::changed_files(&file) {
                    if !c.changed.contains(&p) {
                        c.changed.push(p);
                    }
                }
                touched = true;
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
                    // `claude` stopped (/exit, Ctrl+D): Enter in its terminal starts it again
                    "ended" => {
                        c.busy = false;
                        c.term_waiting = false;
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
            // a setting changed while it worked: started again now that it is done
            if c.term_restart && !c.busy && !c.term_waiting {
                restart.push(k);
                continue;
            }
            if !st.session_id.is_empty() && c.session_id.as_deref() != Some(st.session_id.as_str()) {
                c.session_id = Some(st.session_id.clone());
                // Enter once `claude` ended goes on with this session
                term.set_command(id, claude_term::argv(&file.with_extension("settings.json"), &claude_term::Start { session_id: &st.session_id, plugin: crate::skills::plugin_dir().as_deref(), ..Default::default() }));
                c.session_account = c.account.clone();
                save.push(k);
            }
            if !st.title.is_empty() && c.wish.is_none() {
                c.wish = Some(st.title.clone());
                save.push(k);
            }
            if !st.limits.is_empty() && st.limits != old.limits {
                // the quotas of its account (the first one: under no name)
                let a = claude_account::find(&accounts, &c.account);
                let key = if a.name == accounts[0].name { String::new() } else { a.name.clone() };
                limits.push((key, st.limits.clone()));
            }
            c.cost = st.cost;
            if st.context > 0 {
                c.context = (st.context, if st.context > 200_000 { 1_000_000 } else { 200_000 });
            }
        }
        if touched {
            self.watch.last_check = Instant::now() - Duration::from_secs(10);
            self.git_refresh();
        }
        for k in restart {
            let c = if k == 0 { &mut self.claude } else { &mut self.claude_others[k - 1] };
            c.drop_term();
        }
        // the terminals of the conversations closed
        let alive: Vec<u64> = std::iter::once(&self.claude).chain(&self.claude_others).filter_map(|c| c.term).collect();
        for id in self.term.embedded_ids() {
            if !alive.contains(&id) {
                self.term.close(id);
            }
        }
        for (key, l) in limits {
            if self.claude_limits.get(&key) != Some(&l) {
                self.claude_limits.insert(key, l);
                write_limits(&self.claude_limits);
            }
        }
        // each conversation whose session or title came, the others too (swapped in to be saved)
        save.dedup();
        for k in save {
            if k == 0 {
                self.claude_save();
            } else if k <= self.claude_others.len() {
                std::mem::swap(&mut self.claude, &mut self.claude_others[k - 1]);
                self.claude_save();
                std::mem::swap(&mut self.claude, &mut self.claude_others[k - 1]);
            }
        }
    }
    /// A setting `claude` starts with changed (its sandbox): every conversation started again with
    /// it, going on with its session; those at work or waiting for you once they are done.
    pub(in crate::app) fn claude_restart_terms(&mut self) {
        let mut later = 0;
        for c in std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).filter(|c| c.term.is_some()) {
            if c.busy || c.term_waiting {
                c.term_restart = true;
                later += 1;
            } else {
                c.drop_term();
            }
        }
        if later > 0 {
            self.set_status(tf!("Claude repartira avec le nouveau réglage quand il aura fini ({later} conversation(s))", "Claude starts again with the new setting once it is done ({later} conversation(s))"));
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
