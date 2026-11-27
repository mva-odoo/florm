//! The keys of the editor given to Vim (setting vim_mode, see `crate::vim`): in normal and visual
//! modes, the text typed and the keys without Ctrl are Vim's; in insert mode only Escape is (the
//! editor types the rest, recorded for `.`). The shortcuts with Ctrl stay Florm's.

use super::*;
use crate::vim::{self, Mode, Out};

impl App {
    /// Vim's normal or visual mode: Tab, the suggestions, Copilot are not the editor's then.
    pub(super) fn vim_normal(&self) -> bool {
        self.settings.vim_mode && self.vim.mode != Mode::Insert
    }

    pub(super) fn vim_keys(&mut self, ctx: &egui::Context) {
        if !self.settings.vim_mode || self.overlay.is_some() || self.confirm.is_some() || self.special_shown() || !ctx.memory(|m| m.has_focus(editor_id())) {
            return;
        }
        let Some(d) = self.docs.get_mut(self.active).filter(|d| d.view == document::View::Text) else { return };
        let events = ctx.input(|i| i.events.clone());
        let mut consumed = vec![false; events.len()];
        let mut outs = Vec::new();
        if self.vim.mode == Mode::Insert {
            for (k, ev) in events.iter().enumerate() {
                match ev {
                    Event::Key { key: Key::Escape, pressed: true, modifiers, .. } if modifiers.is_none() => {
                        consumed[k] = true;
                        self.lsp.completion = None;
                        self.lsp.signature = None;
                        d.ghost = None;
                        self.vim.key(d, vim::ESC);
                    }
                    Event::Key { key: Key::OpenBracket, pressed: true, modifiers, .. } if modifiers.ctrl && !modifiers.alt => {
                        consumed[k] = true;
                        self.vim.key(d, vim::ESC);
                    }
                    // recorded for `.` (typed by the editor)
                    Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if modifiers.is_none() && self.lsp.completion.is_none() => self.vim.typed("\n"),
                    Event::Key { key: Key::Backspace, pressed: true, modifiers, .. } if modifiers.is_none() => self.vim.typed(&vim::BACKSPACE.to_string()),
                    Event::Text(t) => self.vim.typed(t),
                    _ => {}
                }
            }
        } else {
            // a search result shown by Florm: the cursor at its start (not a visual selection)
            let s = d.primary_sel();
            if !s.is_empty() && self.find.matches.contains(&(s.start(), s.end())) {
                d.sels = vec![Sel::cursor(s.start())];
            }
            self.vim.sync(d);
            let version = d.version;
            // Ctrl+V arrives as a key and as a paste: one visual block for both
            let mut ctrl_v = false;
            for (k, ev) in events.iter().enumerate() {
                let key = match ev {
                    Event::Text(t) => {
                        consumed[k] = true;
                        for c in t.chars() {
                            outs.extend(self.vim.key(d, c));
                        }
                        continue;
                    }
                    // Ctrl+V comes as a paste: in normal mode, the visual block
                    Event::Paste(_) => {
                        consumed[k] = true;
                        if !ctrl_v {
                            ctrl_v = true;
                            Some(vim::ctrl('v'))
                        } else {
                            None
                        }
                    }
                    Event::Key { key, pressed: true, modifiers, .. } => {
                        let ctrl = modifiers.ctrl || modifiers.command;
                        match key {
                            _ if ctrl && !modifiers.alt && !modifiers.shift => match key {
                                Key::R | Key::D | Key::U | Key::F | Key::B => Some(vim::ctrl(key.name().chars().next().unwrap_or('r'))),
                                Key::V if !ctrl_v => {
                                    ctrl_v = true;
                                    Some(vim::ctrl('v'))
                                }
                                Key::V => {
                                    consumed[k] = true;
                                    None
                                }
                                Key::OpenBracket => Some(vim::ESC),
                                _ => None,
                            },
                            // the other shortcuts (Alt, F keys) stay Florm's
                            _ if ctrl || modifiers.alt => None,
                            // nothing to cancel: Escape is Florm's (a panel closed, zen mode left…)
                            Key::Escape if self.vim.mode == Mode::Normal && self.vim.pending.is_empty() => None,
                            Key::Escape => Some(vim::ESC),
                            Key::ArrowLeft => Some('h'),
                            Key::ArrowRight => Some('l'),
                            Key::ArrowUp => Some('k'),
                            Key::ArrowDown | Key::Enter => Some('j'),
                            Key::Backspace => Some('h'),
                            Key::Delete => Some('x'),
                            Key::Home => Some('0'),
                            Key::End => Some('$'),
                            Key::PageUp => Some(vim::ctrl('b')),
                            Key::PageDown => Some(vim::ctrl('f')),
                            // letters, digits, Tab…: their text comes as Event::Text, not typed
                            other if !matches!(other, Key::F1 | Key::F2 | Key::F3 | Key::F4 | Key::F5 | Key::F6 | Key::F7 | Key::F8 | Key::F9 | Key::F10 | Key::F11 | Key::F12) => {
                                consumed[k] = true;
                                None
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(c) = key {
                    consumed[k] = true;
                    outs.extend(self.vim.key(d, c));
                }
            }
            // a read-only tab: what Vim changed is undone
            if d.read_only && d.version != version {
                d.undo();
                self.vim = vim::Vim::new();
                self.set_status(t!("Fichier en lecture seule", "Read-only file"));
            }
        }
        if consumed.iter().any(|c| *c) {
            ctx.input_mut(|inp| {
                let mut k = 0;
                inp.events.retain(|_| {
                    let keep = !consumed.get(k).copied().unwrap_or(false);
                    k += 1;
                    keep
                });
            });
        }
        for o in outs {
            self.vim_out(ctx, o);
        }
    }

    fn vim_out(&mut self, ctx: &egui::Context, o: Out) {
        match o {
            Out::Find(_) => self.exec(ctx, Cmd::Find),
            Out::FindNext(fwd) => self.vim_find(fwd),
            Out::Word(w, fwd) => {
                self.find.query = w;
                self.find.regex = false;
                self.find.whole = true;
                self.find.key = None;
                self.vim_find(fwd);
            }
            Out::CommandLine => self.open_prompt(Prompt::VimCommand),
            Out::Clipboard(text, lines) => {
                self.last_line_copy = lines.then(|| text.clone());
                ctx.copy_text(text);
            }
            Out::SaveClose => {
                if self.save(self.active) {
                    self.close_tab(self.active, false);
                }
            }
            Out::Close => self.close_tab(self.active, false),
            Out::PasteClipboard(after, n) => match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                Ok(text) => {
                    if let Some(d) = self.docs.get_mut(self.active) {
                        self.vim.put_text(d, &text.replace("\r\n", "\n"), after, n);
                    }
                }
                Err(e) => self.set_error(tf!("Presse-papiers illisible : {e}", "Cannot read the clipboard: {e}")),
            },
        }
    }

    /// n / N / * / #: the next result, the cursor at its start (normal mode).
    fn vim_find(&mut self, forward: bool) {
        if self.find.query.is_empty() {
            self.set_status(t!("Rien à chercher : / pour chercher", "Nothing to search: / to search"));
            return;
        }
        self.find_step(forward, false);
        if let Some(d) = self.docs.get_mut(self.active) {
            let s = d.primary_sel();
            d.sels = vec![Sel::cursor(s.start())];
        }
    }

    /// The command line of Vim (`:w`, `:q`, `:wq`, `:42`, `:s/a/b/g`, `:%s/a/b/g`…).
    pub(super) fn vim_command(&mut self, text: &str) {
        let cmd = text.trim().trim_start_matches(':').trim();
        let ctx = self.egui_ctx.clone();
        match cmd {
            "w" | "write" => self.save_formatted(self.active),
            "wa" | "wall" => self.exec(&ctx, Cmd::SaveAll),
            "q" | "quit" | "close" | "bd" => self.close_tab(self.active, false),
            "q!" | "quit!" | "bd!" => self.close_tab(self.active, true),
            "wq" | "x" | "xit" => {
                if self.save(self.active) {
                    self.close_tab(self.active, false);
                }
            }
            "noh" | "nohlsearch" => self.find.open = false,
            "sp" | "split" => self.exec(&ctx, Cmd::SplitDown),
            "vs" | "vsp" | "vsplit" => self.exec(&ctx, Cmd::SplitRight),
            _ if !cmd.is_empty() && cmd.chars().all(|c| c.is_ascii_digit()) => {
                let line: usize = cmd.parse().unwrap_or(1);
                if let Some(d) = self.docs.get_mut(self.active) {
                    d.goto_line(line.saturating_sub(1), 0);
                }
            }
            _ if cmd.starts_with("e ") || cmd.starts_with("edit ") => {
                let name = cmd.split_once(' ').map_or("", |(_, n)| n.trim());
                let path = Path::new(name);
                let path = if path.is_absolute() { path.to_path_buf() } else { self.root.clone().unwrap_or_default().join(path) };
                self.open_path(path);
            }
            _ => {
                let Some(d) = self.docs.get_mut(self.active) else { return };
                match vim::substitute(d, cmd) {
                    Some(Ok(0)) => self.set_status(t!("Motif introuvable", "Pattern not found")),
                    Some(Ok(n)) => self.set_status(tf!("{n} remplacement(s)", "{n} replacement(s)")),
                    Some(Err(e)) => self.set_error(tf!("Motif refusé : {e}", "Pattern refused: {e}")),
                    None => self.set_error(tf!("Commande Vim inconnue : {cmd}", "Unknown Vim command: {cmd}")),
                }
            }
        }
        self.pending_focus = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The new features driven by real key events in the whole app, without a window: Vim, Tab on
    /// a snippet and on Emmet, Ctrl+I and its proposal, inlay hints drawn.
    #[test]
    fn new_editor_features_from_the_keyboard() {
        let dir = std::env::temp_dir().join(format!("florm_keys_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = dir.join("config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("settings.toml"), "check_updates = false\nlsp = false\nai_complete = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &cfg);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(dir.clone()));
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
        let frame = |app: &mut App, events: Vec<Event>| {
            app.process_jobs();
            let _ = crate::run_ui(&ctx, egui::RawInput { screen_rect: screen, events, ..Default::default() }, |ui| {
                let c = ui.ctx().clone();
                app.completion_keys(&c);
                app.ai_complete_keys(&c);
                app.quick_fix_keys(&c);
                app.inline_ai_keys(&c);
                app.vim_keys(&c);
                app.handle_keys(&c);
                app.editor_input(&c);
                app.update_find();
                app.status_bar(ui);
                app.central(ui);
                app.inline_ai_ui(&c);
            });
        };
        let key = |k: Key, modifiers: egui::Modifiers| Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers };
        // a typed char: its key and its text, as egui gives them
        let typed = |s: &str| -> Vec<Event> {
            s.chars()
                .flat_map(|c| {
                    let k = Key::from_name(&c.to_ascii_uppercase().to_string());
                    k.map(|k| key(k, egui::Modifiers::NONE)).into_iter().chain([Event::Text(c.to_string())])
                })
                .collect()
        };
        let esc = || vec![key(Key::Escape, egui::Modifiers::NONE)];
        let text = |app: &App| app.docs[app.active].rope.to_string();

        // ---- Vim
        let py = dir.join("a.py");
        std::fs::write(&py, "def f():\n    return 1\n").unwrap();
        app.open_path(py.clone());
        frame(&mut app, Vec::new());
        app.focus_editor(&ctx);
        frame(&mut app, Vec::new());
        app.settings.vim_mode = true;
        for k in ["j", "w", "w"] {
            frame(&mut app, typed(k));
        }
        assert_eq!(app.docs[app.active].primary_sel().head, 20, "j then w w: on the 1");
        frame(&mut app, typed("ciw"));
        assert_eq!(app.vim.mode, Mode::Insert);
        frame(&mut app, typed("42"));
        frame(&mut app, esc());
        assert_eq!(app.vim.mode, Mode::Normal);
        assert_eq!(text(&app), "def f():\n    return 42\n");
        frame(&mut app, typed("ggdd"));
        assert_eq!(text(&app), "    return 42\n");
        frame(&mut app, typed("u"));
        assert_eq!(text(&app), "def f():\n    return 42\n");
        frame(&mut app, typed(":"));
        assert!(app.overlay.is_some(), ": opens the command line");
        app.overlay = None;
        app.vim_command("w");
        frame(&mut app, Vec::new());
        assert_eq!(std::fs::read_to_string(&py).unwrap(), "def f():\n    return 42\n", ":w saved");
        // Escape with nothing to cancel stays Florm's; Tab does not expand anything in normal mode
        frame(&mut app, vec![key(Key::Tab, egui::Modifiers::NONE)]);
        assert_eq!(text(&app), "def f():\n    return 42\n");
        app.settings.vim_mode = false;
        app.vim = crate::vim::Vim::new();

        // ---- a snippet with places, then Emmet, from Tab
        let html = dir.join("p.html");
        std::fs::write(&html, "").unwrap();
        app.open_path(html);
        frame(&mut app, Vec::new());
        app.focus_editor(&ctx);
        frame(&mut app, Vec::new());
        frame(&mut app, vec![Event::Text("ul>li.x*2".into())]);
        frame(&mut app, vec![key(Key::Tab, egui::Modifiers::NONE)]);
        assert_eq!(text(&app), "<ul>\n    <li class=\"x\"></li>\n    <li class=\"x\"></li>\n</ul>", "Emmet by Tab");
        frame(&mut app, vec![Event::Text("one".into())]);
        frame(&mut app, vec![key(Key::Tab, egui::Modifiers::NONE)]);
        frame(&mut app, vec![Event::Text("two".into())]);
        assert!(text(&app).contains("<li class=\"x\">one</li>") && text(&app).contains("<li class=\"x\">two</li>"), "Tab goes to the next place: {}", text(&app));

        // ---- Ctrl+I: the field, a proposal on green, Escape puts the old text back
        app.open_path(py.clone());
        frame(&mut app, Vec::new());
        app.focus_editor(&ctx);
        frame(&mut app, Vec::new());
        frame(&mut app, vec![key(Key::I, egui::Modifiers::COMMAND)]);
        assert!(app.inline_ai.ask.is_some(), "Ctrl+I opens the field");
        frame(&mut app, Vec::new());
        let d = &app.docs[app.active];
        let (uid, version) = (d.uid, d.version);
        let line_end = d.rope.line_to_char(0) + d.line_len(0);
        app.inline_ai.n += 1;
        let n = app.inline_ai.n;
        app.inline_edit_ready(n, uid, version, (0, line_end), "x".into(), Ok("def g(a):".into()));
        assert!(app.docs[app.active].proposal.is_some());
        assert!(text(&app).starts_with("def g(a):\n"));
        app.focus_editor(&ctx);
        frame(&mut app, Vec::new());
        frame(&mut app, Vec::new());
        assert!(app.docs[app.active].proposal_screen.is_some(), "the green lines are on the screen");
        frame(&mut app, esc());
        assert_eq!(text(&app), "def f():\n    return 42\n", "Escape undoes the proposal");
        assert!(app.docs[app.active].proposal.is_none());

        // ---- the results of a search changed in a tab, written back by Ctrl+S
        let (m1, m2) = (dir.join("m1.py"), dir.join("m2.py"));
        std::fs::write(&m1, "a = 1\nfoo = 2\nb = 3\nc = 4\n").unwrap();
        std::fs::write(&m2, "foo()\n").unwrap();
        let hit = |line: usize| crate::search::Hit { line, col: 0, len: 3, preview: String::new(), start: 0, end: 3 };
        app.search.done.pattern = "foo".into();
        app.search.files = vec![
            crate::search::FileHits { path: m1.clone(), rel: "m1.py".into(), hits: vec![hit(1)], partial: false },
            crate::search::FileHits { path: m2.clone(), rel: "m2.py".into(), hits: vec![hit(0)], partial: false },
        ];
        app.open_multibuffer();
        assert_eq!(text(&app), "▌ m1.py:1\na = 1\nfoo = 2\nb = 3\n\n▌ m2.py:1\nfoo()\n");
        let edited = text(&app).replace("foo = 2", "bar = 2\nbaz = 9").replace("foo()", "bar()");
        app.docs[app.active].set_text(&edited);
        // m2 changed meanwhile: left as it is
        std::fs::write(&m2, "other()\n").unwrap();
        assert!(app.save(app.active));
        assert_eq!(std::fs::read_to_string(&m1).unwrap(), "a = 1\nbar = 2\nbaz = 9\nb = 3\nc = 4\n");
        assert_eq!(std::fs::read_to_string(&m2).unwrap(), "other()\n");
        assert!(!app.docs[app.active].is_dirty());
        assert!(app.docs.iter().all(|d| d.path.as_ref() != Some(&m1)), "the file opened for it is closed again");

        // ---- inlay hints drawn while typing
        let d = &mut app.docs[app.active];
        d.inlays = vec![(5, "a: ".into()), (8, " -> int".into())];
        for _ in 0..3 {
            frame(&mut app, vec![Event::Text("z".into())]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
