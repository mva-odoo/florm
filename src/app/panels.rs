//! Panels around the editor: menus, tabs, status bar, sidebar, bottom panels.

use super::*;

mod bottom;
mod git_bar;
mod menus;
mod side_bar;
mod status;
mod tabs;

impl App {
    pub(super) fn central(&mut self, ctx: &egui::Context) {
        self.normalize_panes();
        self.drop_zones = DropZones::default();
        if std::mem::take(&mut self.pending_focus) {
            ctx.memory_mut(|m| m.request_focus(editor_id()));
        }
        let mut welcome = false;
        let mut editor_resp = None;
        self.sync_split_tab();
        // nothing open: the welcome page, in a tab (the tab strip and its window buttons stay)
        if self.docs.is_empty() {
            self.open_view_tab(document::View::Welcome, t!("Accueil", "Welcome").into());
        }
        let two = self.is_split();
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            if two {
                let frame = egui::Frame::none();
                match self.split_dir {
                    SplitDir::Right => {
                        let w = ui.available_width() / 2.0;
                        egui::SidePanel::right("pane_1").resizable(true).default_width(w).min_width(180.0).frame(frame).show_inside(ui, |ui| {
                            let r = self.pane_ui(ui, 1);
                            editor_resp = editor_resp.take().or(r);
                        });
                    }
                    SplitDir::Down => {
                        let h = ui.available_height() / 2.0;
                        egui::TopBottomPanel::bottom("pane_1").resizable(true).default_height(h).min_height(120.0).frame(frame).show_inside(ui, |ui| {
                            let r = self.pane_ui(ui, 1);
                            editor_resp = editor_resp.take().or(r);
                        });
                    }
                }
                egui::CentralPanel::default().frame(frame).show_inside(ui, |ui| {
                    let r = self.pane_ui(ui, 0);
                    editor_resp = editor_resp.take().or(r);
                });
                return;
            }
            self.tabs_strip(ui, None);
            self.drop_zones.panes.push((0, ui.available_rect_before_wrap()));
            if self.active < self.docs.len() {
                self.breadcrumbs_ui(ui, self.active, 0);
                self.conflict_bar(ui, self.active, 0);
                self.disk_bar(ui, self.active, 0);
            }
            if self.active < self.docs.len() {
                editor_resp = self.doc_body(ui, self.active, editor_id(), true);
            } else {
                welcome = true;
            }
        });
        if let Some(r) = editor_resp {
            r.context_menu(|ui| self.editor_context_menu(ui));
        }
        // a click in the other panel's editor makes it the active panel
        if ctx.memory(|m| m.has_focus(editor_view::other_editor_id())) {
            if let Some(i) = self.pane_doc(1 - self.focused_pane) {
                self.active = i;
                ctx.memory_mut(|m| m.request_focus(editor_id()));
            }
        }
        self.resolve_tab_drag(ctx);
        if welcome {
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| self.welcome_ui(ui));
        }
    }

    /// Selected text (single line) or the word under the cursor.
    pub(super) fn word_under_cursor(&self) -> String {
        let Some(d) = self.docs.get(self.active) else { return String::new() };
        let p = d.primary_sel();
        let (a, b) = if p.is_empty() { d.word_at(p.head) } else { (p.start(), p.end()) };
        let t = d.slice(a, b);
        if t.contains('\n') {
            String::new()
        } else {
            t
        }
    }

    /// Right click in the editor: actions on the line / word under the cursor.
    fn editor_context_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(280.0);
        let word = self.word_under_cursor();
        let Some(d) = self.docs.get(self.active) else { return };
        let ro = d.read_only;
        let line = d.rope.char_to_line(d.primary_sel().head);
        let has_change = d.hunks.iter().any(|h| h.contains(line));
        let noqa = noqa_codes(d, line);
        let has_path = d.path.is_some();
        let has_folds = !d.folds.is_empty();
        let has_lsp = self.settings.lsp && d.path.as_deref().and_then(lsp::language_of).is_some();
        let is_python = d.path.as_deref().and_then(lint::lang_of) == Some("python");
        let has_selection = !d.primary_sel().is_empty();
        let in_repo = d.path.as_deref().and_then(|p| self.repo_for(p)).is_some();
        let (km, blame_on, has_root) = (&self.keymap, self.blame_mode, self.root.is_some());
        let mut chosen = None;
        let mut item = |ui: &mut egui::Ui, label: &str, cmd: Cmd, enabled: bool| {
            if !crate::features::cmd_on(cmd) {
                return;
            }
            let mod_key = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
            let shortcut = match cmd {
                // handled by egui's clipboard events, not by the keymap
                Cmd::Copy => format!("{mod_key}+C"),
                Cmd::Cut => format!("{mod_key}+X"),
                Cmd::Paste => format!("{mod_key}+V"),
                _ => km.label(cmd),
            };
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut)).clicked() {
                chosen = Some(cmd);
                ui.close_menu();
            }
        };
        ui.label(RichText::new(tf!("Ligne {}", "Line {}", line + 1)).small().weak());
        item(ui, t!("Couper", "Cut"), Cmd::Cut, !ro);
        item(ui, t!("Copier", "Copy"), Cmd::Copy, true);
        item(ui, t!("Coller", "Paste"), Cmd::Paste, !ro);
        if !word.is_empty() {
            let short: String = word.chars().take(28).collect();
            let short = if short.len() < word.len() { format!("{short}…") } else { short };
            ui.separator();
            item(ui, &tf!("Sélectionner toutes les occurrences de « {short} »", "Select all occurrences of “{short}”"), Cmd::SelectAllOccurrences, true);
            item(ui, &tf!("Rechercher « {short} » dans le dossier", "Find “{short}” in the folder"), Cmd::FindWordInFiles, has_root);
        }
        ui.separator();
        let what = if has_selection { t!("la sélection", "the selection") } else { t!("la ligne", "the line") };
        item(ui, &tf!("✳ Modifier ou expliquer {what} avec Claude, ici…", "✳ Edit or explain {what} with Claude, here…"), Cmd::ClaudeInline, true);
        item(ui, &tf!("✳ Demander à Claude à propos de {what}…", "✳ Ask Claude about {what}…"), Cmd::ClaudeAsk, has_path);
        item(ui, &tf!("✳ Ajouter {what} au contexte de Claude", "✳ Add {what} to Claude's context"), Cmd::ClaudeAddSelection, has_path);
        item(ui, t!("✳ Ajouter le fichier au contexte de Claude", "✳ Add the file to Claude's context"), Cmd::ClaudeAddFile, has_path);
        ui.separator();
        item(ui, t!("Corrections rapides…", "Quick fixes…"), Cmd::QuickFix, true);
        item(ui, t!("Aller à la définition", "Go to definition"), Cmd::GotoDefinition, true);
        item(ui, t!("Rechercher les usages", "Find usages"), Cmd::FindReferences, true);
        item(ui, t!("Suggestions", "Suggestions"), Cmd::Complete, !ro);
        if has_lsp {
            item(ui, t!("Renommer le symbole…", "Rename symbol…"), Cmd::RenameSymbol, !ro);
        }
        if !noqa.is_empty() {
            ui.separator();
            item(ui, &tf!("Ignorer {} sur cette ligne (# noqa)", "Ignore {} on this line (# noqa)", noqa.join(", ")), Cmd::NoqaLine, !ro);
        }
        if is_python {
            ui.separator();
            item(ui, if has_selection { t!("Formater les lignes sélectionnées (ruff)", "Format the selected lines (ruff)") } else { t!("Formater le fichier (ruff)", "Format the file (ruff)") }, Cmd::FormatDocument, !ro);
            item(ui, t!("Corriger automatiquement (ruff --fix)", "Fix automatically (ruff --fix)"), Cmd::FixDocument, !ro);
        }
        ui.separator();
        item(ui, t!("Ajouter / retirer un signet", "Add / remove a bookmark"), Cmd::ToggleBookmark, true);
        item(ui, t!("Commenter / décommenter", "Comment / uncomment"), Cmd::ToggleComment, !ro);
        item(ui, t!("Replier le bloc", "Fold the block"), Cmd::Fold, true);
        item(ui, t!("Tout replier", "Fold all"), Cmd::FoldAll, true);
        item(ui, t!("Tout déplier", "Unfold all"), Cmd::UnfoldAll, has_folds);
        if in_repo {
            ui.separator();
            item(ui, if blame_on { t!("Masquer le blame", "Hide the blame") } else { t!("Blame de la ligne", "Line blame") }, Cmd::GitBlame, true);
            item(ui, t!("Historique de la ligne", "Line history"), Cmd::GitLineHistory, true);
            item(ui, t!("Historique du fichier", "File history"), Cmd::GitFileHistory, true);
            if has_change {
                item(ui, t!("Voir le changement", "Show the change"), Cmd::ShowChange, true);
                item(ui, t!("Revenir en arrière sur ce changement", "Revert this change"), Cmd::GitRevertChange, !ro);
            }
            item(ui, t!("Copier le lien GitHub de la ligne", "Copy the GitHub link of the line"), Cmd::CopyGithubLink, true);
        }
        if has_path {
            ui.separator();
            item(ui, t!("Copier la référence fichier:ligne", "Copy the file:line reference"), Cmd::CopyFileRef, true);
            item(ui, t!("Afficher dans l'arborescence", "Show in the tree"), Cmd::RevealInTree, true);
        }
        if let Some(c) = chosen {
            self.pending.push(c);
        }
    }

    pub(super) fn copy_github_link(&mut self, ctx: &egui::Context) {
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(path) = d.path.clone() else { return };
        let line = d.rope.char_to_line(d.primary_sel().head);
        let head_line = d.head_line(line).unwrap_or(line);
        let Some(repo) = self.repo_for(&path) else {
            self.set_status(t!("Fichier hors d'un dépôt git", "File outside a git repository"));
            return;
        };
        let Some((owner, name)) = git::stdout(&repo, &["remote", "get-url", "origin"]).ok().and_then(|u| git::parse_github(u.trim()))
        else {
            self.set_status(t!("Le remote « origin » n'est pas un dépôt GitHub", "The “origin” remote is not a GitHub repository"));
            return;
        };
        let rev = git::stdout(&repo, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_else(|_| "HEAD".into());
        let rel = path.strip_prefix(&repo).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        let link = format!("https://github.com/{owner}/{name}/blob/{rev}/{rel}#L{}", head_line + 1);
        ctx.output_mut(|o| o.copied_text = link.clone());
        self.set_status(tf!("Lien copié : {link}", "Link copied: {link}"));
    }

    /// Start screen in the style of the Florm artwork: logo, title, shortcuts, recent projects.
    pub(super) fn welcome_ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let light = self.hl.is_light();
        let wc = welcome_colors(light);
        // the cyan of the logo is too pale for titles on paper
        let start_c = if light { Color32::from_rgb(0x0b, 0x86, 0xab) } else { logo::CYAN };
        logo::paint_backdrop(ui.painter(), rect, light);
        let size = (rect.height() * 0.28).clamp(80.0, 220.0);
        let top = rect.top() + (rect.height() * 0.10).max(16.0);
        let logo_rect = egui::Rect::from_center_size(egui::pos2(rect.center().x, top + size / 2.0), egui::Vec2::splat(size));
        logo::paint(ui.painter(), logo_rect, true);
        let title_size = size * 0.30;
        let title_y = logo_rect.bottom() + size * 0.10;
        logo::paint_title(ui.painter(), egui::pos2(rect.center().x, title_y), title_size, light);

        let y = title_y + title_size * 2.2;
        let width = 560.0_f32.min(rect.width() - 32.0);
        let inner = egui::Rect::from_min_max(egui::pos2(rect.center().x - width / 2.0, y), egui::pos2(rect.center().x + width / 2.0, rect.bottom() - 8.0));
        let mut cmd = None;
        let mut open = None;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
            // colors of the backdrop (navy or paper), not of the editor theme
            ui.visuals_mut().override_text_color = Some(wc.text);
            ui.columns(2, |cols| {
                welcome_card(&mut cols[0], t!("DÉMARRER", "START"), start_c, &wc, |ui| {
                    for (label, c) in [
                        (t!("Nouveau fichier", "New file"), Cmd::NewFile),
                        (t!("Ouvrir un dossier…", "Open a folder…"), Cmd::OpenFolder),
                        (t!("Aller au fichier…", "Go to file…"), Cmd::QuickOpen),
                        (t!("Palette de commandes", "Command palette"), Cmd::CommandPalette),
                        (t!("Rechercher dans le dossier", "Find in folder"), Cmd::FindInFiles),
                        (t!("Graphe git", "Git graph"), Cmd::GitHistory),
                    ] {
                        if welcome_row(ui, label, &self.keymap.label(c), true, start_c, &wc).clicked() {
                            cmd = Some(c);
                        }
                    }
                });
                welcome_card(&mut cols[1], t!("RÉCENTS", "RECENT"), logo::PURPLE, &wc, |ui| {
                    for p in self.projects.iter().filter(|p| p.path.is_dir()).take(6) {
                        let parent = p.path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                        let r = welcome_row(ui, &p.name, &parent, false, logo::PURPLE, &wc)
                            .on_hover_text(p.path.display().to_string());
                        if r.clicked() {
                            open = Some(p.path.clone());
                        }
                    }
                    if self.projects.is_empty() {
                        ui.label(RichText::new(t!("Aucun projet récent", "No recent project")).color(wc.dim));
                    }
                });
            });
        });
        if let Some(c) = cmd {
            self.pending.push(c);
        }
        if let Some(p) = open {
            self.switch_project(p);
        }
    }

    pub(super) fn periodic(&mut self, ctx: &egui::Context) {
        self.tests_check();
        // with hot exit, unsaved changes are kept in the session: no question
        let keeps = self.settings.hot_exit && self.root.is_some();
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quit_confirmed
            && !keeps
            && self.docs.iter().any(|d| d.is_dirty())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm::Quit);
        }
        if ctx.input(|i| i.viewport().close_requested()) || self.last_session_save.elapsed() > Duration::from_secs(30) {
            self.save_session();
        }
        // Live lint for in-process linters (XML/JSON), debounced.
        if let Some(d) = self.docs.get_mut(self.active) {
            if d.lint_version != d.version {
                if let Some(lang) = d.path.as_deref().and_then(lint::lang_of).filter(|l| lint::is_in_process(l)) {
                    let el = d.last_change.elapsed();
                    if el >= Duration::from_millis(300) {
                        d.diagnostics = lint::lint_in_process(lang, &d.rope.to_string());
                        d.lint_version = d.version;
                    } else {
                        ctx.request_repaint_after(Duration::from_millis(310) - el);
                    }
                }
            }
        }
        // Live diff against HEAD for the gutter, debounced while typing.
        if let Some(d) = self.docs.get_mut(self.active) {
            if d.diff_version != d.version {
                let el = d.last_change.elapsed();
                if el >= Duration::from_millis(150) || d.diff_version == u64::MAX {
                    d.refresh_diff();
                } else {
                    ctx.request_repaint_after(Duration::from_millis(160) - el);
                }
            }
        }
        // Files changed outside the editor (checked at once when coming back to the window).
        let focused = ctx.input(|i| i.focused);
        if focused && !self.was_focused {
            self.last_disk_check = Instant::now() - Duration::from_secs(10);
            // files may have been changed by another program: the Git view shows them at once
            if self.has_tab(document::View::Git) {
                self.git_refresh();
            }
        }
        self.was_focused = focused;
        // a command run in a terminal (git commit, checkout…): its effect is shown within half a second
        if self.term.has_focus(ctx) && ctx.input(|i| i.key_pressed(Key::Enter)) {
            self.term_enter = Some(Instant::now());
        }
        let busy_term = self.term_enter.is_some_and(|t| t.elapsed() < Duration::from_secs(30));
        let every = if busy_term { Duration::from_millis(500) } else { Duration::from_secs(2) };
        if self.last_disk_check.elapsed() > every {
            self.last_disk_check = Instant::now();
            self.check_git_stamps();
            // folders added or removed by another program show up in the tree
            if self.tree.as_mut().is_some_and(|t| t.refresh_changed()) {
                self.index.files_at = None;
            }
            let mut msgs = Vec::new();
            for d in self.docs.iter_mut() {
                let Some(p) = d.path.clone() else { continue };
                if d.mtime.is_none() {
                    continue;
                }
                match std::fs::metadata(&p).and_then(|m| m.modified()) {
                    Ok(m) if d.mtime != Some(m) => {
                        if d.is_dirty() {
                            // a bar above the text asks what to keep; Ctrl+S asks too
                            d.mtime = Some(m);
                            d.disk_change = Some(document::DiskChange::Changed);
                        } else if d.reload_from_disk().is_ok() {
                            msgs.push(tf!("{} rechargé depuis le disque", "{} reloaded from disk", d.title()));
                        }
                    }
                    Ok(_) => {
                        // back on disk (a checkout that removed then restored it)
                        if d.disk_change == Some(document::DiskChange::Deleted) {
                            d.disk_change = None;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound && d.disk_change.is_none() => {
                        d.disk_change = Some(document::DiskChange::Deleted);
                    }
                    Err(_) => {}
                }
            }
            // several files at once (a checkout): one message for all, none hidden by the next
            if !msgs.is_empty() {
                self.set_status(msgs.join(" · "));
            }
        }
        ctx.request_repaint_after(every);
        let mut title = String::from("Florm");
        if let Some(d) = self.docs.get(self.active) {
            title = format!("{}{} — Florm", d.title(), if d.is_dirty() { " •" } else { "" });
        }
        if let Some(r) = &self.root {
            title = format!("{title} — {}", r.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
        }
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }
}

