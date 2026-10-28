//! Keyboard input and commands.

use super::*;

impl App {
    pub(super) fn handle_keys(&mut self, ctx: &egui::Context) {
        if let Some(cap) = self.prefs.capture {
            self.capture_shortcut(ctx, cap);
            return;
        }
        let editor_focused = !self.special_shown() && ctx.memory(|m| m.has_focus(editor_id()));
        let term_focused = self.term.has_focus(ctx);
        if let Some(t) = self.drop_alt_text {
            if Instant::now() < t {
                ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(t) if t.chars().count() == 1)));
            } else {
                self.drop_alt_text = None;
            }
        }
        let events = ctx.input(|i| i.events.clone());
        let mut consumed = vec![false; events.len()];
        let mut cmds = Vec::new();
        for (i, ev) in events.iter().enumerate() {
            let Event::Key { key, pressed: true, modifiers, .. } = ev else { continue };
            let combo = Combo::from_event(*key, *modifiers);
            if !self.chord && self.keymap.is_chord_start(&combo) {
                self.chord = true;
                consumed[i] = true;
                continue;
            }
            let in_chord = std::mem::take(&mut self.chord);
            match self.keymap.lookup(combo, in_chord) {
                Some(cmd) => {
                    // a confirmation dialog holds the keyboard: Échap cancels it, Entrée and Tab
                    // go to its buttons
                    if self.confirm.is_some() && cmd != Cmd::Escape {
                        continue;
                    }
                    if cmd.is_editor() && (!editor_focused || self.overlay.is_some()) {
                        continue;
                    }
                    // in the terminal, keys go to the shell, except a few global shortcuts
                    if term_focused
                        && !matches!(cmd, Cmd::ToggleTerminal | Cmd::CommandPalette | Cmd::QuickOpen | Cmd::ToggleSidebar)
                    {
                        continue;
                    }
                    consumed[i] = true;
                    cmds.push(cmd);
                }
                None if in_chord => consumed[i] = true,
                None => {}
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
        // Alt+letter also arrives as typed text ("l" for Alt+L): drop the text of the key press
        // when it triggered a shortcut
        let alt_shortcut = events
            .iter()
            .zip(&consumed)
            .any(|(e, used)| *used && matches!(e, Event::Key { modifiers, .. } if modifiers.alt && !modifiers.ctrl && !modifiers.command));
        if alt_shortcut {
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(t) if t.chars().count() == 1)));
            // in case the text comes with the next frame
            self.drop_alt_text = Some(Instant::now() + Duration::from_millis(80));
        }
        for c in cmds {
            self.exec(ctx, c);
        }
    }

    /// Records the next key combination for the shortcut editor.
    fn capture_shortcut(&mut self, ctx: &egui::Context, cap: Capture) {
        let events = ctx.input(|i| i.events.clone());
        let mut first = cap.first;
        let mut result: Option<Option<Seq>> = None; // Some(None) = cancelled
        for ev in &events {
            let Event::Key { key, pressed: true, modifiers, .. } = ev else { continue };
            let combo = Combo::from_event(*key, *modifiers);
            if *key == Key::Escape && first.is_none() {
                result = Some(None);
                break;
            }
            if first.is_none() && self.keymap.is_chord_start(&combo) {
                first = Some(combo);
                continue;
            }
            result = Some(Some(first.into_iter().chain([combo]).collect()));
            break;
        }
        // the keys being recorded must not reach shortcuts or text fields
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_))));
        self.chord = false;
        match result {
            Some(None) => self.prefs.capture = None,
            Some(Some(seq)) => {
                self.prefs.capture = None;
                self.propose_shortcut(cap.cmd, seq);
            }
            None => self.prefs.capture = Some(Capture { first, ..cap }),
        }
    }

    pub(super) fn editor_input(&mut self, ctx: &egui::Context) {
        if self.overlay.is_some() || self.confirm.is_some() || self.special_shown() || !ctx.memory(|m| m.has_focus(editor_id())) {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let auto = self.settings.auto_pair;
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        for ev in events {
            match ev {
                Event::Text(t) => {
                    doc.insert_text(&t, auto);
                    // suggestions after "." or two letters of a word (if a server handles the file)
                    let last = t.chars().last();
                    let word = |c: char| c.is_alphanumeric() || c == '_';
                    let head = doc.primary_sel().head;
                    let typed = (head.saturating_sub(2)..head).all(|p| doc.char_at(p).is_some_and(word));
                    let has_server = self.settings.lsp && doc.path.as_deref().and_then(lsp::language_of).is_some();
                    if self.lsp.completion.is_none() && ((has_server && last == Some('.')) || (last.is_some_and(word) && typed)) {
                        self.lsp.complete_at = Some(Instant::now() + Duration::from_millis(120));
                    }
                }
                Event::Copy => {
                    let (t, lw) = doc.copy_text();
                    self.last_line_copy = lw.then(|| t.clone());
                    ctx.output_mut(|o| o.copied_text = t);
                }
                Event::Cut => {
                    let (t, lw) = doc.cut();
                    self.last_line_copy = lw.then(|| t.clone());
                    ctx.output_mut(|o| o.copied_text = t);
                }
                Event::Paste(t) => {
                    let lw = self.last_line_copy.as_deref() == Some(t.replace("\r\n", "\n").as_str());
                    doc.paste(&t, lw);
                }
                _ => {}
            }
        }
    }

    pub(super) fn exec(&mut self, ctx: &egui::Context, cmd: Cmd) {
        use Cmd::*;
        // a shortcut of a part turned off (GitHub, the debugger, Claude)
        if let Some(f) = crate::features::of_cmd(cmd).filter(|f| !crate::features::on(*f)) {
            self.set_status(tf!("{} est désactivé (Préférences)", "{} is turned off (Preferences)", crate::features::name(f)));
            return;
        }
        if matches!(cmd, GitRevertChange | GitRevertFile) {
            if let Some(d) = self.docs.get_mut(self.active) {
                if d.diff_version != d.version {
                    d.refresh_diff();
                }
                let line = d.rope.char_to_line(d.primary_sel().head);
                let msg = if cmd == GitRevertChange && !d.hunks.iter().any(|h| h.contains(line)) {
                    t!("Pas de changement git sur cette ligne", "No git change on this line")
                } else if cmd == GitRevertFile && d.git_base.is_none() {
                    t!("Ce fichier n'est pas suivi par git", "This file is not tracked by git")
                } else if cmd == GitRevertChange {
                    t!("Changement git annulé (Ctrl+Z pour le récupérer) — à enregistrer", "Git change undone (Ctrl+Z to get it back) — to save")
                } else {
                    t!("Fichier remis comme au dernier commit (Ctrl+Z pour revenir) — à enregistrer", "File put back as at the last commit (Ctrl+Z to go back) — to save")
                };
                self.set_status(msg);
            }
        }
        if cmd.is_editor() {
            let auto = self.settings.auto_pair;
            if let Some(d) = self.docs.get_mut(self.active) {
                d.exec(cmd, auto);
            }
            return;
        }
        match cmd {
            Escape => {
                let peek_open = self.docs.get(self.active).is_some_and(|d| d.peek.is_some());
                if self.overlay.take().is_some() || self.confirm.take().is_some() || self.line_history.take().is_some() {
                } else if maximize::restore_shown(ctx) {
                } else if self.showing(document::View::Split) {
                    self.leave_tab(document::View::Split);
                } else if self.showing(document::View::Git) && self.history_detail_open() {
                    self.close_history_detail();
                } else if self.showing(document::View::Git) {
                    self.leave_tab(document::View::Git);
                } else if self.showing(document::View::Settings) {
                    self.leave_tab(document::View::Settings);
                } else if peek_open {
                    self.docs[self.active].peek = None;
                } else if self.find.open {
                    self.find.open = false;
                } else if let Some(d) = self.docs.get_mut(self.active) {
                    if d.sels.len() > 1 {
                        let p = d.primary_sel();
                        d.sels = vec![p];
                        d.primary = 0;
                    }
                }
                self.focus_editor(ctx);
            }
            CommandPalette => self.open_overlay(Mode::Commands),
            QuickOpen => {
                // the list is rebuilt in the background (new files), the last one is shown meanwhile
                if self.index.files.is_empty() || self.index.files_at.map_or(true, |t| t.elapsed() > Duration::from_secs(5)) {
                    self.refresh_index();
                }
                self.open_overlay(Mode::Files);
            }
            GotoLine | GotoLineSelect => {
                self.goto_select = cmd == GotoLineSelect;
                self.open_overlay(Mode::GotoLine);
            }
            Find | Replace => {
                self.find.open = true;
                self.find.replace_mode = cmd == Replace;
                self.find.focus = true;
                if let Some(d) = self.docs.get(self.active) {
                    let p = d.primary_sel();
                    if !p.is_empty() {
                        let t = d.slice(p.start(), p.end());
                        if !t.contains('\n') {
                            self.find.query = t;
                        }
                    }
                }
            }
            FindInFiles | FindInFilesTab => {
                if cmd == FindInFilesTab || self.settings.search_in_tab {
                    self.open_search_tab();
                } else {
                    self.bottom = Some(BottomTab::Search);
                }
                self.search.focus = true;
                if let Some(d) = self.docs.get(self.active) {
                    let p = d.primary_sel();
                    let t = d.slice(p.start(), p.end());
                    if !t.is_empty() && !t.contains('\n') {
                        self.search.q.pattern = t;
                    }
                }
            }
            NextResult => self.step_result(true),
            PrevResult => self.step_result(false),
            SwitchProject => self.open_overlay(Mode::Projects),
            RenameProject => match self.projects.iter().find(|p| Some(&p.path) == self.root.as_ref()).cloned() {
                Some(p) => self.open_overlay_with(Mode::Prompt(Prompt::RenameProject(p.path)), Vec::new(), p.name),
                None => self.set_status(t!("Aucun dossier ouvert", "No folder open")),
            },
            CloseFolder => self.close_folder(),
            FindNext => self.find_step(true, false),
            FindPrev => self.find_step(false, false),
            FindAll => self.find_select_all(ctx),
            Save => {
                self.save(self.active);
            }
            SaveAs => {
                if !self.native_dialog(crate::os::Pick::SaveAs) {
                    self.open_prompt(Prompt::SaveAs);
                }
            }
            SaveAll => {
                for i in 0..self.docs.len() {
                    if self.docs[i].is_dirty() && self.docs[i].path.is_some() {
                        self.save(i);
                    }
                }
            }
            NewFile => self.new_doc(),
            OpenFile => {
                if !self.native_dialog(crate::os::Pick::File) {
                    self.open_prompt(Prompt::OpenFile);
                }
            }
            OpenFolder => {
                if !self.native_dialog(crate::os::Pick::Folder) {
                    self.open_prompt(Prompt::OpenFolder);
                }
            }
            OpenFolderPath => self.open_prompt(Prompt::OpenFolder),
            RemoteConnect => self.open_overlay(Mode::Remotes),
            RemoteDisconnect => self.remote_disconnect(),
            CloseTab => self.close_tab(self.active, false),
            ReopenClosed => {
                if let Some(p) = self.closed.pop() {
                    self.open_path(p);
                }
            }
            NextTab if !self.docs.is_empty() => self.active = (self.active + 1) % self.docs.len(),
            PrevTab if !self.docs.is_empty() => self.active = (self.active + self.docs.len() - 1) % self.docs.len(),
            ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            SplitRight | SplitDown => {
                let dir = if cmd == SplitRight { SplitDir::Right } else { SplitDir::Down };
                if self.is_split() {
                    self.split_dir = dir;
                } else if self.active < self.docs.len() {
                    self.split_tab(self.active, Some(dir));
                }
            }
            Unsplit => self.unsplit(),
            FocusOtherPane => {
                if let Some(i) = self.pane_doc(1 - self.focused_pane) {
                    self.activate_tab(i);
                }
            }
            NewTerminalTab => self.open_terminal_tab(),
            TogglePanel => match self.bottom {
                Some(_) => {
                    self.bottom = None;
                    self.focus_editor(ctx);
                }
                None => {
                    let tab = self.last_bottom;
                    self.bottom = Some(tab);
                    match tab {
                        BottomTab::Search => self.search.focus = true,
                        BottomTab::Terminal => self.term.want_focus = true,
                        BottomTab::Claude => self.claude.focus = true,
                        _ => {}
                    }
                }
            },
            ToggleTerminal => {
                if self.bottom == Some(BottomTab::Terminal) {
                    self.bottom = None;
                    self.focus_editor(ctx);
                } else {
                    self.bottom = Some(BottomTab::Terminal);
                    self.term.want_focus = true;
                }
            }
            ShowProblems => self.bottom = Some(BottomTab::Problems),
            ShowOutput => self.bottom = Some(BottomTab::Output),
            ShowGit => self.open_git_view(),
            PullRequests => self.open_pulls(),
            Docker => self.open_docker(),
            TestAgain => match self.settings.odoo_test_history.first().cloned() {
                Some(t) => self.run_tests(t),
                None => self.run_tests(String::new()),
            },
            TestHere => match self.test_tags_here() {
                Some(t) => self.run_tests(t),
                None => self.set_status(t!("Pas de test ni de module Odoo sous le curseur", "No Odoo test or module under the cursor")),
            },
            DebugTestHere => match self.test_tags_here() {
                Some(t) => self.debug_test(t),
                None => self.set_status(t!("Pas de test ni de module Odoo sous le curseur", "No Odoo test or module under the cursor")),
            },
            Debug => self.open_debug(),
            DebugStart => self.debug_start_or_continue(),
            DebugStop => self.debug_stop_by_hand(),
            DebugRestart => self.debug_restart(),
            DebugPause => self.debug_step("pause"),
            StepOver => self.debug_step("next"),
            StepInto => self.debug_step("stepIn"),
            StepOut => self.debug_step("stepOut"),
            ToggleBreakpoint => self.debug_toggle_breakpoint(),
            ClearBreakpoints => self.debug_clear_breakpoints(),
            CreatePr => self.start_pr_create(),
            Claude => self.open_claude(),
            SuggestCommit => self.suggest_commit_message(),
            ClaudeAddSelection => self.claude_attach_selection(false),
            ClaudeAsk => self.claude_attach_selection(true),
            ClaudeInline => self.inline_ai_open(),
            ClaudeResumeFile => self.claude_resume_file(),
            ClaudeAddFile => {
                if let Some(p) = self.docs.get(self.active).and_then(|d| d.path.clone()) {
                    self.claude_attach_path(p);
                }
            }
            ShowFiles => {
                // a second time: back to the editor
                if self.show_sidebar && ctx.memory(|m| m.has_focus(sidebar::tree_id())) {
                    self.focus_editor(ctx);
                } else {
                    self.show_sidebar = true;
                    // the keyboard goes to the tree, on the current file
                    if let Some(p) = self.docs.get(self.active).and_then(|d| d.path.clone()) {
                        if self.tree_nav.cursor.is_none() || !ctx.memory(|m| m.has_focus(sidebar::tree_id())) {
                            self.reveal_in_tree(&p);
                        }
                    }
                    self.tree_nav.focus = true;
                }
            }
            ToggleMenuBar => {
                self.settings.show_menu_bar = !self.settings.show_menu_bar;
                self.persist("show_menu_bar", self.settings.show_menu_bar.to_string());
                if !self.settings.show_menu_bar {
                    self.set_status(t!("Barre de menus masquée : les menus restent dans ☰ en bas à gauche", "Menu bar hidden: the menus are still under ☰ at the bottom left"));
                }
            }
            LocalHistory => self.open_local_history(),
            ListTodos => self.list_todos(),
            OpenSql => self.open_sql(),
            CompareClipboard => self.compare_clipboard(),
            CompareWith => self.open_prompt(Prompt::CompareWith),
            RevealInTree => {
                if let Some(p) = self.docs.get(self.active).and_then(|d| d.path.clone()) {
                    self.reveal_in_tree(&p);
                }
            }
            NavBack => self.nav_go(true),
            NavForward => self.nav_go(false),
            Complete => self.request_completion(ctx),
            GotoDefinition => self.goto_definition(ctx),
            QuickFix => self.quick_fix_open(ctx),
            RenameSymbol => {
                let i = self.active;
                match self.lsp_for(i, ctx, true) {
                    None => {
                        self.set_status(t!("Renommer dans tout le projet demande un serveur de langage (pour Python : « installer pylsp » dans la barre d'état)", "Renaming in the whole project needs a language server (for Python: “install pylsp” in the status bar)"));
                    }
                    Some(_) => {
                        let w = self.word_under_cursor();
                        self.open_overlay_with(Mode::Prompt(Prompt::RenameSymbol), Vec::new(), w);
                    }
                }
            }
            InstallPylsp => {
                self.set_status(t!("Installation de pylsp (pip)…", "Installing pylsp (pip)…"));
                self.log(format!("pip install --target {} python-lsp-server", lint::tools_dir().display()));
                self.jobs.spawn(|| JobResult::ToolInstalled(lint::install_python_tool("python-lsp-server")));
            }
            LspRestart => {
                self.lsp.manager.reset();
                self.lsp.requests.clear();
                self.lsp.completion = None;
                self.lsp.notes.clear();
                self.set_status(t!("Serveurs de langage redémarrés", "Language servers restarted"));
            }
            MarkdownPreview => self.markdown_preview(false),
            MarkdownPreviewSide => self.markdown_preview(true),
            GotoSymbol => {
                if self.docs.get(self.active).is_some() {
                    self.open_overlay(Mode::Symbols);
                    if self.overlay.as_ref().is_some_and(|o| o.items.is_empty()) {
                        self.overlay = None;
                        self.set_status(t!("Aucun symbole reconnu dans ce fichier", "No symbol recognized in this file"));
                    }
                }
            }
            Fold | Unfold | FoldAll | UnfoldAll => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    match cmd {
                        Fold => d.fold_at_cursor(),
                        Unfold => d.unfold_at_cursor(),
                        FoldAll => d.fold_all(),
                        _ => d.unfold_all(),
                    }
                }
            }
            ToggleBreadcrumbs => {
                self.settings.breadcrumbs = !self.settings.breadcrumbs;
                self.persist("breadcrumbs", self.settings.breadcrumbs.to_string());
            }
            ToggleWordWrap => {
                self.settings.word_wrap = !self.settings.word_wrap;
                self.persist("word_wrap", self.settings.word_wrap.to_string());
                self.set_status(if self.settings.word_wrap { t!("Retour à la ligne automatique activé (Alt+Z)", "Word wrap on (Alt+Z)") } else { t!("Retour à la ligne automatique désactivé (Alt+Z)", "Word wrap off (Alt+Z)") });
            }
            ToggleMinimap => {
                self.settings.show_minimap = !self.settings.show_minimap;
                self.persist("show_minimap", self.settings.show_minimap.to_string());
            }
            Copy | Cut => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    let (t, linewise) = if cmd == Cut && !d.read_only { d.cut() } else { d.copy_text() };
                    self.last_line_copy = linewise.then(|| t.clone());
                    ctx.output_mut(|o| o.copied_text = t);
                }
            }
            Paste => match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                Ok(t) => {
                    let linewise = self.last_line_copy.as_deref() == Some(t.replace("\r\n", "\n").as_str());
                    if let Some(d) = self.docs.get_mut(self.active) {
                        d.paste(&t, linewise);
                    }
                }
                Err(e) => self.set_error(tf!("Presse-papiers illisible : {e}", "Clipboard unreadable: {e}")),
            },
            CopyFilePath | CopyFileRef => {
                if let Some(d) = self.docs.get(self.active) {
                    if let Some(p) = &d.path {
                        let text = if cmd == CopyFilePath {
                            p.display().to_string()
                        } else {
                            let rel = self.root.as_ref().and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p);
                            format!("{}:{}", rel.display(), d.rope.char_to_line(d.primary_sel().head) + 1)
                        };
                        ctx.output_mut(|o| o.copied_text = text.clone());
                        self.set_status(tf!("Copié : {text}", "Copied: {text}"));
                    }
                }
            }
            CopyGithubLink => self.copy_github_link(ctx),
            RevealInOs => {
                if let Some(p) = self.docs.get(self.active).and_then(|d| d.path.clone()) {
                    self.reveal(&p);
                }
            }
            FindWordInFiles => {
                let w = self.word_under_cursor();
                if !w.is_empty() {
                    self.search.q.pattern = w;
                    self.bottom = Some(BottomTab::Search);
                    self.start_search();
                }
            }
            FindReferences => self.find_references(ctx),
            ToggleBookmark | NextBookmark | PrevBookmark | ClearBookmarks => {
                if let Some(d) = self.docs.get_mut(self.active).filter(|d| d.view == document::View::Text) {
                    let msg = match cmd {
                        ToggleBookmark if d.toggle_bookmark() => t!("Signet ajouté (F8 / Maj+F8 : suivant / précédent)", "Bookmark added (F8 / Shift+F8: next / previous)"),
                        ToggleBookmark => t!("Signet retiré", "Bookmark removed"),
                        ClearBookmarks => {
                            d.bookmarks.clear();
                            t!("Signets du fichier retirés", "Bookmarks of the file removed")
                        }
                        _ if d.goto_bookmark(cmd == NextBookmark) => "",
                        _ => t!("Aucun signet dans ce fichier (Ctrl+F2 pour en poser un)", "No bookmark in this file (Ctrl+F2 to set one)"),
                    };
                    if !msg.is_empty() {
                        self.set_status(msg);
                    }
                }
            }
            ListBookmarks => self.list_bookmarks(),
            FormatDocument => self.python_rewrite(lint::Rewrite::Format),
            FixDocument => self.python_rewrite(lint::Rewrite::Fix),
            NoqaLine => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    let line = d.rope.char_to_line(d.primary_sel().head);
                    let codes = noqa_codes(d, line);
                    if !codes.is_empty() && !d.read_only {
                        d.add_noqa(line, &codes);
                    }
                }
            }
            InstallFlake8 => {
                self.set_status(t!("Installation de flake8 (pip)…", "Installing flake8 (pip)…"));
                self.log(format!("pip install --target {} flake8", lint::tools_dir().display()));
                self.jobs.spawn(|| JobResult::ToolInstalled(lint::install_python_tool("flake8")));
            }
            ShowChange => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    let line = d.rope.char_to_line(d.primary_sel().head);
                    d.peek = d.hunks.iter().find(|h| h.contains(line)).map(|h| h.mark_lines().start);
                }
            }
            ClearSearchHistory => {
                self.state.search_history.clear();
                self.state.include_history.clear();
                self.state.exclude_history.clear();
                project::save_state(&self.state);
                self.set_status(t!("Historique de recherche effacé", "Search history cleared"));
            }
            GitBlame => {
                self.blame_mode = !self.blame_mode;
                self.blame_req = None;
                if !self.blame_mode {
                    for d in self.docs.iter_mut() {
                        d.blame = None;
                    }
                }
                self.set_status(if self.blame_mode {
                    t!("Blame activé (Ctrl+B pour masquer, Ctrl+Alt+B : historique de la ligne)", "Blame on (Ctrl+B to hide, Ctrl+Alt+B: line history)")
                } else {
                    t!("Blame masqué", "Blame hidden")
                });
            }
            GitLineHistory => self.open_line_history(),
            GitFileHistory => self.open_file_history(),
            GitSearchHistory => self.open_prompt(Prompt::HistorySearch(git::SearchKind::Message)),
            GitReflog => self.open_reflog(),
            GitPullRebase => self.git_job("pull --rebase", &["pull", "--rebase", "--autostash"]),
            GitForcePush => self.force_push(),
            GitPushTags => match self.default_remote() {
                Some(r) => self.git_job("push --tags", &["push", &r, "--tags"]),
                None => self.set_error(t!("Aucun remote où pousser", "No remote to push to")),
            },
            GitMerge => self.pick_branch(BranchPick::Merge, None),
            GitRebaseOnto => self.pick_branch(BranchPick::Rebase, None),
            GitRenameBranch => self.pick_branch(BranchPick::Rename, None),
            GitDeleteBranch => self.pick_branch(BranchPick::Delete, None),
            GitAmend => {
                self.open_git_view();
                self.start_amend();
            }
            GitClean => self.ask_clean(),
            GitCommitRules => self.open_commit_rules(),
            ToggleInlineDiff => {
                self.settings.inline_diff = !self.settings.inline_diff;
                self.persist("inline_diff", self.settings.inline_diff.to_string());
                self.set_status(if self.settings.inline_diff {
                    t!("Diff dans l'éditeur : activé (Ctrl+Alt+D pour masquer)", "Diff in the editor: on (Ctrl+Alt+D to hide)")
                } else {
                    t!("Diff dans l'éditeur : désactivé", "Diff in the editor: off")
                });
            }
            ZoomIn | ZoomOut => {
                let d = if cmd == ZoomIn { 1.0 } else { -1.0 };
                self.settings.font_size = (self.settings.font_size + d).clamp(7.0, 48.0);
                self.persist("font_size", format!("{:.1}", self.settings.font_size));
            }
            SetSyntax => self.open_overlay(Mode::Syntax),
            SelectTheme => self.open_overlay(Mode::Themes),
            Lint => {
                self.analyze(self.active, false);
                self.bottom = Some(BottomTab::Problems);
            }
            OpenSettings => self.open_settings(),
            OpenSettingsFile => {
                self.open_path(config::settings_path());
            }
            ReloadPlugins => {
                self.reload_plugins();
                self.set_status(tf!("{} plugin(s) chargé(s)", "{} plugin(s) loaded", self.plugins.len()));
            }
            CheckUpdate => self.check_update(true),
            SharedSync => self.shared_job(crate::shared::Op::Sync),
            SharedPublish => self.shared_job(crate::shared::Op::Publish),
            ShowGuide => self.open_view_tab(document::View::Help, t!("Guide de Florm", "Florm guide").into()),
            ShowWelcome => self.open_view_tab(document::View::Welcome, t!("Accueil", "Welcome").into()),
            InsertTemplate => self.open_overlay(Mode::Templates),
            OpenTemplatesFolder => {
                let dir = crate::templates::dir();
                crate::templates::install_readme(&dir);
                self.open_path(dir.join("README.md"));
                self.set_status(tf!("Dossier des modèles : {} — un fichier par modèle", "Templates folder: {} — one file per template", dir.display()));
            }
            NewTemplate => {
                let d = self.docs.get(self.active).filter(|d| d.view == document::View::Text);
                let body = d.map(|d| d.slice(d.primary_sel().start(), d.primary_sel().end())).unwrap_or_default();
                let ext = d
                    .and_then(|d| d.path.as_deref())
                    .and_then(|p| p.extension())
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                self.open_prompt(Prompt::NewTemplate { body, ext });
            }
            OpenPluginsFolder => {
                let dir = config::plugins_dir();
                let _ = std::fs::create_dir_all(&dir);
                self.log(tf!("Dossier des plugins : {}", "Plugins folder: {}", dir.display()));
                self.open_path(dir.join("README.md"));
            }
            RefreshTree => {
                if let Some(t) = self.tree.as_mut() {
                    t.refresh();
                }
                self.refresh_index();
            }
            Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            GitRefresh => self.git_refresh(),
            GitPull => self.git_job("pull", &["pull"]),
            GitPush => self.git_job("push", &["push", "-u", "origin", "HEAD"]),
            GitFetch => self.git_job("fetch", &["fetch", "--all", "--prune"]),
            GitStageAll => self.git_job("add", &["add", "-A"]),
            GitCommit => {
                self.open_git_view();
                self.git.focus_commit = true;
            }
            GitStash => self.stash_act(super::git_actions::StashAct::Push),
            GitStashPop => {
                self.stash_reload();
                match self.git.stashes.first() {
                    Some(s) => {
                        let hash = s.hash.clone();
                        self.stash_act(super::git_actions::StashAct::Pop(hash));
                    }
                    None => self.set_status(t!("Aucun stash à reprendre", "No stash to take back")),
                }
            }
            GitAiReview => {
                self.open_git_view();
                self.show_wip(false);
                self.ai_review_wip();
            }
            GitCheckout => self.pick_branch(BranchPick::Checkout, None),
            GitNewBranch => self.open_prompt(Prompt::NewBranch),
            GitDiffFile => {
                let path = self.docs.get(self.active).and_then(|d| d.path.clone());
                match path.as_deref().and_then(|p| Some((p, self.repo_for(p)?))) {
                    Some((p, repo)) => {
                        let rel = p.strip_prefix(&repo).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
                        self.select_repo(repo);
                        self.show_diff(rel, false);
                    }
                    None => self.set_status(t!("Fichier hors d'un dépôt git", "File outside a git repository")),
                }
            }
            GitHistory => {
                if self.showing(document::View::Git) {
                    self.leave_tab(document::View::Git);
                } else {
                    // hidden, the graph may have missed changes: read again
                    self.show_tab(document::View::Git);
                    self.load_history();
                    self.git_refresh();
                }
            }
            Plugin(k) => self.run_plugin(k),
            _ => {}
        }
    }
}
