//! Panels around the editor: menus, tabs, status bar, sidebar, bottom panels.

use super::*;

impl App {
    pub(super) fn menu_bar(&mut self, ctx: &egui::Context) {
        if !self.settings.show_menu_bar {
            return;
        }
        let mut pending = Vec::new();
        let project = self.root.as_ref().and_then(|r| r.file_name()).map(|n| n.to_string_lossy().into_owned());
        let panel = ctx.style().visuals.panel_fill;
        egui::TopBottomPanel::top("menu")
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin::symmetric(10.0, 5.0)))
            .show_separator_line(false)
            .show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
                logo::paint(ui.painter(), r, false);
                ui.add_space(2.0);
                self.menus(ui, &mut pending);
                // project name, right-aligned, after the menus
                if let Some(p) = &project {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(p).weak());
                    });
                }
            });
        });
        self.pending.extend(pending);
    }

    /// The menus (in the menu bar, or under ☰ in the status bar when the bar is hidden).
    fn menus(&self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>) {
        let km = &self.keymap;
        let plugins = &self.plugins;
        let plugin_cmds = &self.plugin_cmds;
        let mut item = |ui: &mut egui::Ui, label: &str, cmd: Cmd| {
            if ui.add(egui::Button::new(label).shortcut_text(km.label(cmd))).clicked() {
                pending.push(cmd);
                ui.close_menu();
            }
        };
        ui.menu_button("Fichier", |ui| {
            item(ui, "Nouveau fichier", Cmd::NewFile);
            item(ui, "Ouvrir un fichier…", Cmd::OpenFile);
            item(ui, "Ouvrir un dossier…", Cmd::OpenFolder);
            item(ui, "Changer de projet…", Cmd::SwitchProject);
            item(ui, "Fermer le dossier", Cmd::CloseFolder);
            item(ui, "Rouvrir l'onglet fermé", Cmd::ReopenClosed);
            ui.separator();
            item(ui, "Enregistrer", Cmd::Save);
            item(ui, "Enregistrer sous…", Cmd::SaveAs);
            item(ui, "Tout enregistrer", Cmd::SaveAll);
            ui.separator();
            item(ui, "Fermer l'onglet", Cmd::CloseTab);
            item(ui, "Préférences", Cmd::OpenSettings);
            item(ui, "Ouvrir settings.toml", Cmd::OpenSettingsFile);
            item(ui, "Quitter", Cmd::Quit);
        });
        ui.menu_button("Édition", |ui| {
            item(ui, "Annuler", Cmd::Undo);
            item(ui, "Rétablir", Cmd::Redo);
            ui.separator();
            item(ui, "Dupliquer la ligne", Cmd::DuplicateLine);
            item(ui, "Supprimer la ligne", Cmd::DeleteLine);
            item(ui, "Monter la ligne", Cmd::MoveLineUp);
            item(ui, "Descendre la ligne", Cmd::MoveLineDown);
            item(ui, "Joindre les lignes", Cmd::JoinLines);
            item(ui, "Commenter / décommenter", Cmd::ToggleComment);
            item(ui, "Indenter", Cmd::Indent);
            item(ui, "Désindenter", Cmd::Unindent);
            item(ui, "Trier les lignes", Cmd::SortLines);
            item(ui, "MAJUSCULES", Cmd::UpperCase);
            item(ui, "minuscules", Cmd::LowerCase);
        });
        ui.menu_button("Sélection", |ui| {
            item(ui, "Tout sélectionner", Cmd::SelectAll);
            item(ui, "Sélectionner la ligne", Cmd::SelectLine);
            item(ui, "Ajouter l'occurrence suivante", Cmd::SelectNext);
            item(ui, "Toutes les occurrences", Cmd::SelectAllOccurrences);
            item(ui, "Diviser en lignes", Cmd::SplitLines);
            item(ui, "Curseur au-dessus", Cmd::AddCursorAbove);
            item(ui, "Curseur en dessous", Cmd::AddCursorBelow);
            item(ui, "Crochet correspondant", Cmd::GotoBracket);
        });
        ui.menu_button("Rechercher", |ui| {
            item(ui, "Rechercher…", Cmd::Find);
            item(ui, "Remplacer…", Cmd::Replace);
            item(ui, "Suivant", Cmd::FindNext);
            item(ui, "Rechercher dans le dossier…", Cmd::FindInFiles);
            item(ui, "Résultat global suivant", Cmd::NextResult);
            item(ui, "Précédent", Cmd::FindPrev);
            ui.separator();
            item(ui, "Aller au fichier…", Cmd::QuickOpen);
            item(ui, "Aller à la ligne…", Cmd::GotoLine);
        });
        ui.menu_button("Affichage", |ui| {
            item(ui, "Palette de commandes", Cmd::CommandPalette);
            item(ui, "Barre latérale", Cmd::ToggleSidebar);
            item(ui, "Terminal", Cmd::ToggleTerminal);
            item(ui, "Problèmes", Cmd::ShowProblems);
            item(ui, "Sortie", Cmd::ShowOutput);
            item(ui, "Minimap", Cmd::ToggleMinimap);
            item(ui, "Barre de menus", Cmd::ToggleMenuBar);
            ui.separator();
            item(ui, "Syntaxe…", Cmd::SetSyntax);
            item(ui, "Thème…", Cmd::SelectTheme);
            item(ui, "Zoom avant", Cmd::ZoomIn);
            item(ui, "Zoom arrière", Cmd::ZoomOut);
        });
        ui.menu_button("Git", |ui| {
            item(ui, "Panneau Git", Cmd::ShowGit);
            item(ui, "Commit…", Cmd::GitCommit);
            item(ui, "Pull", Cmd::GitPull);
            item(ui, "Push", Cmd::GitPush);
            item(ui, "Fetch", Cmd::GitFetch);
            ui.separator();
            item(ui, "Changer de branche…", Cmd::GitCheckout);
            item(ui, "Nouvelle branche…", Cmd::GitNewBranch);
            item(ui, "Diff du fichier", Cmd::GitDiffFile);
            item(ui, "Historique (graphe)", Cmd::GitHistory);
            item(ui, "Diff dans l'éditeur (vert / rouge)", Cmd::ToggleInlineDiff);
            item(ui, "Blame de la ligne", Cmd::GitBlame);
            item(ui, "Historique de la ligne", Cmd::GitLineHistory);
        });
        ui.menu_button("Outils", |ui| {
            item(ui, "Linter le fichier", Cmd::Lint);
            ui.separator();
            for (k, &(pi, ci)) in plugin_cmds.iter().enumerate() {
                item(ui, &plugins[pi].commands[ci].title, Cmd::Plugin(k));
            }
            if !plugin_cmds.is_empty() {
                ui.separator();
            }
            item(ui, "Insérer un modèle (vue Odoo, menu…)…", Cmd::InsertTemplate);
            item(ui, "Nouveau modèle…", Cmd::NewTemplate);
            item(ui, "Dossier des modèles", Cmd::OpenTemplatesFolder);
            ui.separator();
            item(ui, "Recharger les plugins", Cmd::ReloadPlugins);
            item(ui, "Dossier des plugins", Cmd::OpenPluginsFolder);
        });
        ui.menu_button("Aide", |ui| {
            item(ui, "Guide de Florm (recherche, Git, terminal, modèles…)", Cmd::ShowGuide);
            item(ui, "Accueil", Cmd::ShowWelcome);
            item(ui, "Palette de commandes", Cmd::CommandPalette);
            item(ui, "Raccourcis et préférences", Cmd::OpenSettings);
        });
    }

    /// Tab strip drawn above the editor area only (not above the sidebar).
    /// Tab strip: every tab (`pane == None`, with the history / split / settings tabs) or those of one panel.
    pub(super) fn tabs_strip(&mut self, ui: &mut egui::Ui, pane: Option<u8>) {
        let tc = self.hl.tab_colors();
        let (mut activate, mut close, mut close_history, mut close_prefs) = (None, None, false, false);
        let (mut show_history, mut show_split, mut drag_start, mut menu_act) = (false, false, None, None);
        let split_on = self.split.as_ref().is_some_and(|s| s.shown) && !self.prefs.open;
        let special = pane.is_none() && (self.history.open || self.prefs.open || split_on);
        let is_split = self.is_split();
        let mut counts = [0usize; 2];
        for d in &self.docs {
            counts[(d.pane as usize).min(1)] += 1;
        }
        let mut scrolled = None;
        let mut strip = (egui::Rect::NOTHING, Vec::new());
        let panel_id = egui::Id::new("tabs").with(pane);
        egui::TopBottomPanel::top(panel_id).exact_height(TAB_STRIP_H).frame(egui::Frame::none().fill(tc.bar)).show_inside(ui, |ui| {
            let bar = ui.max_rect();
            strip.0 = bar;
            egui::ScrollArea::horizontal()
                .id_salt(panel_id.with("scroll"))
                .drag_to_scroll(false)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.add_space(6.0);
                        for (i, d) in self.docs.iter().enumerate() {
                            if pane.is_some_and(|p| p != d.pane) {
                                continue;
                            }
                            let shown = match pane {
                                Some(p) if p != self.focused_pane => self.pane_active[p as usize] == Some(d.uid),
                                _ => i == self.active,
                            };
                            let active = shown && !special;
                            let error = d.diagnostics.iter().any(|g| g.severity == Severity::Error);
                            let icon_name = match &d.path {
                                Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()),
                                None if d.syntax == "Diff" => Some("x.diff".to_string()),
                                None => Some(String::new()),
                            };
                            let (body, closed) = tab_widget(ui, &d.title(), icon_name.as_deref(), d.is_dirty(), active, error, &tc);
                            strip.1.push((body.rect, i));
                            if body.drag_started() {
                                drag_start = Some(i);
                            }
                            if closed || body.middle_clicked() {
                                close = Some(i);
                            } else if body.clicked() {
                                activate = Some(i);
                            }
                            if i == self.active && self.tab_scrolled != Some(i) {
                                body.scroll_to_me(None);
                                scrolled = Some(i);
                            }
                            let alone = counts[(d.pane as usize).min(1)] == 1;
                            body.context_menu(|ui| tab_menu(ui, i, d, is_split, alone, &mut menu_act));
                            if let Some(p) = &d.path {
                                body.on_hover_text(p.display().to_string());
                            }
                        }
                        if pane.is_some() {
                            return;
                        }
                        if self.history.open {
                            let on = !split_on && !self.prefs.open;
                            let (body, closed) = tab_widget(ui, "🔀 Historique Git", None, false, on, false, &tc);
                            close_history = closed;
                            show_history = body.clicked() && !closed;
                        }
                        if let Some(sp) = &self.split {
                            let title = format!("✂ Découpage {}", sp.info.short);
                            let (body, closed) = tab_widget(ui, &title, None, false, split_on, false, &tc);
                            // the split stays in progress: × only switches away from it
                            if closed {
                                activate = activate.or(Some(self.active));
                            } else if body.clicked() {
                                show_split = true;
                            }
                            body.on_hover_text("Découpage en cours — « Terminer » ou « Annuler le découpage » dans l'onglet");
                        }
                        if self.prefs.open {
                            let (_, closed) = tab_widget(ui, "⚙ Préférences", None, false, true, false, &tc);
                            close_prefs = closed;
                        }
                    });
                });
        });
        self.drop_zones.strips.push((pane.unwrap_or(0), strip.0, strip.1));
        if scrolled.is_some() {
            self.tab_scrolled = scrolled;
        }
        if let Some(i) = drag_start {
            self.tab_drag = Some(i);
        }
        if let Some(i) = activate {
            self.activate_tab(i);
        }
        if show_history {
            self.prefs.open = false;
            if let Some(sp) = self.split.as_mut() {
                sp.shown = false;
            }
        }
        if show_split {
            self.prefs.open = false;
            if let Some(sp) = self.split.as_mut() {
                sp.shown = true;
            }
        }
        if let Some(i) = close {
            self.close_tab(i, false);
        }
        if close_history {
            self.history.open = false;
        }
        if close_prefs {
            self.prefs.open = false;
        }
        match menu_act {
            Some(TabAct::Split(i, dir)) => self.split_tab(i, Some(dir)),
            Some(TabAct::MoveOther(i)) => self.split_tab(i, None),
            Some(TabAct::Unsplit) => self.unsplit(),
            Some(TabAct::Flip) => {
                self.split_dir = if self.split_dir == SplitDir::Right { SplitDir::Down } else { SplitDir::Right };
            }
            Some(TabAct::Close(i)) => self.close_tab(i, false),
            Some(TabAct::CloseOthers(i)) => {
                let (uid, p) = (self.docs[i].uid, self.docs[i].pane);
                let before = self.docs.len();
                for j in (0..self.docs.len()).rev() {
                    let d = &self.docs[j];
                    if d.uid != uid && d.pane == p && !d.is_dirty() {
                        self.close_tab(j, true);
                    }
                }
                self.active = self.docs.iter().position(|d| d.uid == uid).unwrap_or(0);
                self.tab_scrolled = None;
                let kept = self.docs.iter().filter(|d| d.pane == p && d.uid != uid).count();
                let closed = before - self.docs.len();
                self.set_status(if kept > 0 {
                    format!("{closed} onglet(s) fermé(s), {kept} non enregistré(s) gardé(s)")
                } else {
                    format!("{closed} onglet(s) fermé(s)")
                });
            }
            Some(TabAct::CopyPath(t)) => {
                ui.ctx().output_mut(|o| o.copied_text = t.clone());
                self.set_status(format!("Copié : {t}"));
            }
            Some(TabAct::Reveal(p)) => self.reveal(&p),
            Some(TabAct::Locate(i)) => {
                self.activate_tab(i);
                if let Some(p) = self.docs.get(i).and_then(|d| d.path.clone()) {
                    self.reveal_in_tree(&p);
                }
            }
            None => {}
        }
    }

    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        let (mut toggle_spaces, mut convert_utf8) = (false, false);
        let panel = ctx.style().visuals.panel_fill;
        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin::symmetric(12.0, 0.0)))
            .show(ctx, |ui| {
            ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(12.5));
            ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(12.5));
            ui.visuals_mut().override_text_color = Some(ui.visuals().weak_text_color());
            ui.horizontal_centered(|ui| {
                if !self.settings.show_menu_bar {
                    let mut pending = Vec::new();
                    ui.menu_button("☰", |ui| {
                        ui.visuals_mut().override_text_color = None;
                        self.menus(ui, &mut pending);
                        ui.separator();
                        if ui.button("Afficher la barre de menus").clicked() {
                            pending.push(Cmd::ToggleMenuBar);
                            ui.close_menu();
                        }
                    })
                    .response
                    .on_hover_text("Menus (barre de menus masquée)");
                    self.pending.extend(pending);
                }
                if let Some(d) = self.docs.get(self.active) {
                    let p = d.primary_sel();
                    let (l, c) = d.line_col(p.head);
                    ui.label(format!("Ligne {}, Colonne {}", l + 1, c + 1));
                    if d.sels.len() > 1 {
                        ui.label(format!("· {} sélections", d.sels.len()));
                    } else if !p.is_empty() {
                        ui.label(format!("· {} caractères sélectionnés", p.end() - p.start()));
                    }
                    if let Some(g) = d.diagnostics.iter().find(|g| g.line == l) {
                        let col = if g.severity == Severity::Error { err() } else { warn() };
                        ui.label(RichText::new(&g.message).color(col));
                    }
                }
                if let Some((msg, t)) = &self.status {
                    if t.elapsed() < Duration::from_secs(6) {
                        ui.label(RichText::new(msg).weak());
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // the rest of the file's details, the terminal and the language server in a menu
                    let mut pending = Vec::new();
                    let lsp_line = self.lsp_status_line();
                    ui.menu_button("⋯", |ui| {
                        ui.visuals_mut().override_text_color = None;
                        if let Some(d) = self.docs.get(self.active) {
                            if ui.button(format!("Syntaxe : {}…", d.syntax)).clicked() {
                                pending.push(Cmd::SetSyntax);
                                ui.close_menu();
                            }
                            let ind = if d.use_spaces { format!("Espaces : {}", d.tab_size) } else { format!("Tabulations : {}", d.tab_size) };
                            if ui.button(format!("Indentation : {ind}")).on_hover_text("Clic : passer aux espaces / aux tabulations").clicked() {
                                toggle_spaces = true;
                                ui.close_menu();
                            }
                            ui.label(format!("Fin de ligne : {}", if d.crlf { "CRLF (Windows)" } else { "LF" }));
                            ui.label(format!("Encodage : {}", d.encoding.label()));
                        }
                        ui.separator();
                        if ui.button(format!(">_ Terminal en bas    {}", self.keymap.label(Cmd::ToggleTerminal))).clicked() {
                            pending.push(Cmd::ToggleTerminal);
                            ui.close_menu();
                        }
                        if ui.button(format!("Nouveau terminal dans un onglet    {}", self.keymap.label(Cmd::NewTerminalTab))).clicked() {
                            pending.push(Cmd::NewTerminalTab);
                            ui.close_menu();
                        }
                        if let Some((text, warning, install)) = &lsp_line {
                            ui.separator();
                            let t = if *warning { RichText::new(text).color(warn()) } else { RichText::new(text) };
                            if *install {
                                if ui.button(t).on_hover_text("Installe python-lsp-server pour Florm (pip --target, sans toucher au Python système). Ou installe pyright.").clicked() {
                                    pending.push(Cmd::InstallPylsp);
                                    ui.close_menu();
                                }
                            } else {
                                ui.label(t);
                            }
                        }
                    })
                    .response
                    .on_hover_text("Syntaxe, indentation, fin de ligne, encodage, terminal");
                    self.pending.extend(pending);
                    if let Some(d) = self.docs.get(self.active) {
                        // only shown when it is not UTF-8: the file is saved back in its encoding
                        if d.encoding != document::Encoding::Utf8
                            && ui
                                .add(egui::Button::new(RichText::new(d.encoding.label()).color(warn())).frame(false))
                                .on_hover_text("Le fichier n'est pas en UTF-8 : il est enregistré dans son encodage.\nClic : l'enregistrer en UTF-8 désormais.")
                                .clicked()
                        {
                            convert_utf8 = true;
                        }
                    }
                    // problems: only when there are some
                    let errs: usize = self
                        .docs
                        .iter()
                        .flat_map(|d| d.diagnostics.iter())
                        .filter(|g| g.severity == Severity::Error)
                        .count();
                    let warns: usize = self.docs.iter().map(|d| d.diagnostics.len()).sum::<usize>() - errs;
                    if errs + warns > 0 {
                        let txt = RichText::new(format!("✖ {errs}  ⚠ {warns}")).color(if errs > 0 { err() } else { warn() });
                        if ui.add(egui::Button::new(txt).frame(false)).on_hover_text("Problèmes (Ctrl+Shift+M)").clicked() {
                            self.pending.push(Cmd::ShowProblems);
                        }
                    }
                    if let Some(st) = &self.git.status {
                        let label = match (&self.repo, self.repos.len() > 1) {
                            (Some(r), true) => format!("🔀 {} · {}", self.repo_label(r), st.branch),
                            _ => format!("🔀 {}", st.branch),
                        };
                        if ui.add(egui::Button::new(label).frame(false)).clicked() {
                            self.pending.push(Cmd::ShowGit);
                        }
                    }
                });
            });
        });
        if toggle_spaces {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.use_spaces = !d.use_spaces;
            }
        }
        if convert_utf8 {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.convert_to_utf8();
                self.set_status("Sera enregistré en UTF-8 (Ctrl+S)");
            }
        }
    }

    /// Language server of the active file, for the status bar menu: (text, is a warning, the
    /// click installs pylsp).
    fn lsp_status_line(&self) -> Option<(String, bool, bool)> {
        let fam = self.settings.lsp.then(|| self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(lsp::language_of)).flatten()?.0;
        let running = self.lsp.manager.clients.iter().flatten().find(|c| c.family == fam);
        match (running, self.lsp.notes.get(fam)) {
            (Some(c), _) if c.ready => Some((format!("Serveur de langage : {}", c.name), false, false)),
            (Some(c), _) => Some((format!("Serveur de langage : {} (démarrage…)", c.name), false, false)),
            (None, Some(_)) if fam == "python" => Some(("Serveur de langage : installer pylsp".into(), true, true)),
            (None, Some(why)) => Some((format!("Pas de serveur de langage : {why}"), true, false)),
            _ => None,
        }
    }

    pub(super) fn find_bar(&mut self, ctx: &egui::Context) {
        let (mut action, mut scope_toggled) = (None, false);
        egui::TopBottomPanel::bottom("find").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let f = &mut self.find;
                if ui.toggle_value(&mut f.case, "Aa").on_hover_text("Respecter la casse").changed()
                    | ui.toggle_value(&mut f.regex, ".*").on_hover_text("Expression régulière").changed()
                    | ui.toggle_value(&mut f.whole, "« »").on_hover_text("Mot entier").changed()
                {
                    action = Some(FindAction::Incremental);
                }
                if ui
                    .toggle_value(&mut f.in_sel, "Sél.")
                    .on_hover_text("Chercher et remplacer seulement dans la sélection actuelle (zone teintée)")
                    .changed()
                {
                    scope_toggled = true;
                }
                let w = (ui.available_width() - 330.0).max(120.0);
                let te = ui.add(
                    egui::TextEdit::singleline(&mut f.query)
                        .desired_width(w)
                        .hint_text("Rechercher (Entrée : suivant, Maj+Entrée : précédent, Alt+Entrée : tout sélectionner)")
                        .font(egui::TextStyle::Monospace),
                );
                if f.focus {
                    te.request_focus();
                    f.focus = false;
                }
                if te.has_focus() {
                    if let Some(t) = history_arrows(ui, &self.state.search_history, &mut self.search.hist_pos) {
                        f.query = t;
                        action = Some(FindAction::Incremental);
                    }
                }
                if te.changed() {
                    action = Some(FindAction::Incremental);
                }
                if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    let m = ui.input(|i| i.modifiers);
                    action = Some(if m.alt {
                        FindAction::All
                    } else if m.shift {
                        FindAction::Prev
                    } else {
                        FindAction::Next
                    });
                    if !m.alt {
                        te.request_focus();
                    }
                }
                if ui.button("◀").on_hover_text("Précédent").clicked() {
                    action = Some(FindAction::Prev);
                }
                if ui.button("▶").on_hover_text("Suivant").clicked() {
                    action = Some(FindAction::Next);
                }
                if ui.button("Tout").on_hover_text("Sélectionner tous les résultats (multi-curseur)").clicked() {
                    action = Some(FindAction::All);
                }
                match &f.error {
                    Some(e) => ui.label(RichText::new(e.lines().last().unwrap_or("")).color(err())),
                    None => ui.label(RichText::new(format!("{} résultat(s)", f.matches.len())).weak()),
                };
            });
            if self.find.replace_mode {
                ui.horizontal(|ui| {
                    ui.add_space(110.0);
                    let w = (ui.available_width() - 330.0).max(120.0);
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut self.find.replacement)
                            .desired_width(w)
                            .hint_text("Remplacer par ($1 pour les groupes en mode regex)")
                            .font(egui::TextStyle::Monospace),
                    );
                    if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        action = Some(FindAction::ReplaceOne);
                        te.request_focus();
                    }
                    if ui.button("Remplacer").clicked() {
                        action = Some(FindAction::ReplaceOne);
                    }
                    let all = if self.find.in_sel { "Tout remplacer dans la sélection" } else { "Tout remplacer" };
                    if ui.button(all).clicked() {
                        action = Some(FindAction::ReplaceAll);
                    }
                    ui.toggle_value(&mut self.find.preserve, "Garder la casse")
                        .on_hover_text("chat → chien, Chat → Chien, CHAT → CHIEN");
                });
            }
            ui.add_space(2.0);
        });
        if scope_toggled {
            let in_sel = self.find.in_sel;
            if let Some(d) = self.docs.get_mut(self.active) {
                let p = d.primary_sel();
                if !in_sel {
                    d.find_scope = None;
                } else if p.is_empty() {
                    self.find.in_sel = false;
                    self.set_status("Sélectionne d'abord le texte dans lequel chercher");
                } else {
                    d.find_scope = Some((p.start(), p.end()));
                }
            }
            action = Some(FindAction::Incremental);
        }
        if matches!(action, Some(FindAction::Next | FindAction::Prev | FindAction::All)) {
            UiState::remember(&mut self.state.search_history, &self.find.query);
            project::save_state(&self.state);
            self.search.hist_pos = None;
        }
        match action {
            Some(FindAction::Incremental) => self.find_step(true, true),
            Some(FindAction::Next) => self.find_step(true, false),
            Some(FindAction::Prev) => self.find_step(false, false),
            Some(FindAction::All) => self.find_select_all(ctx),
            Some(FindAction::ReplaceOne) => self.replace_one(),
            Some(FindAction::ReplaceAll) => self.replace_all(),
            None => {}
        }
    }

    /// Thin strip at the left edge while the sidebar is hidden: a click brings it back.
    pub(super) fn sidebar_strip(&mut self, ctx: &egui::Context) {
        let panel = ctx.style().visuals.panel_fill;
        egui::SidePanel::left("sidebar_strip").exact_width(10.0).resizable(false).frame(egui::Frame::none().fill(panel)).show(ctx, |ui| {
            let rect = ui.max_rect();
            let r = ui.interact(rect, egui::Id::new("sidebar_strip_btn"), Sense::click());
            let c = if r.hovered() { self.hl.colors.accent } else { ui.visuals().weak_text_color().gamma_multiply(0.5) };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "›", egui::FontId::proportional(14.0), c);
            if r.on_hover_text("Afficher la barre latérale (Ctrl+K Ctrl+B)").clicked() {
                self.show_sidebar = true;
            }
        });
    }

    pub(super) fn sidebar(&mut self, ctx: &egui::Context) {
        let mut tree_action = None;
        let panel = ctx.style().visuals.panel_fill;
        let shown = egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(260.0)
            .width_range(150.0..=700.0)
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin { left: 10.0, right: 8.0, top: 4.0, bottom: 6.0 }))
            .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // git is in its own view (Ctrl+Shift+G, or the branch in the status bar)
                for (tab, label) in [(SideTab::Files, "Fichiers"), (SideTab::Projects, "Projets")] {
                    if nav_tab(ui, label, self.side_tab == tab).clicked() {
                        self.side_tab = tab;
                    }
                }
            });
            ui.add_space(6.0);
            match self.side_tab {
                SideTab::Files => match self.tree.as_mut() {
                    Some(tree) => {
                        let active = self.docs.get(self.active).and_then(|d| d.path.clone());
                        let empty = HashMap::new();
                        let info = if self.settings.tree_git_info { &self.repo_info } else { &empty };
                        tree_action =
                            sidebar::show(ui, tree, active.as_deref(), &self.git.map, info, &mut self.tree_scroll, &mut self.tree_nav);
                    }
                    None => {
                        ui.label("Aucun dossier ouvert.");
                        if ui.button("Ouvrir un dossier…").clicked() {
                            self.pending.push(Cmd::OpenFolder);
                        }
                    }
                },
                SideTab::Projects => self.projects_ui(ui),
            }
        });
        // double click on the border between the sidebar and the editor: hide the sidebar
        let edge = shown.response.rect.right();
        let hide = ctx.input(|i| {
            i.pointer.button_double_clicked(egui::PointerButton::Primary)
                && i.pointer.interact_pos().is_some_and(|p| (p.x - edge).abs() <= 5.0 && shown.response.rect.y_range().contains(p.y))
        });
        if hide {
            self.show_sidebar = false;
            self.set_status("Barre latérale masquée : clic sur le bord gauche ou Ctrl+K Ctrl+B pour la réafficher");
        }
        if let Some(a) = tree_action {
            self.tree_action(ctx, a);
        }
    }

    fn tree_action(&mut self, ctx: &egui::Context, a: TreeAction) {
        match a {
            TreeAction::Open(p) => {
                self.open_path(p);
                self.focus_editor(ctx);
            }
            TreeAction::NewFile(d) => self.open_prompt(Prompt::NewFile(d)),
            TreeAction::NewFolder(d) => self.open_prompt(Prompt::NewFolder(d)),
            TreeAction::Rename(p) => self.open_prompt(Prompt::Rename(p)),
            TreeAction::Move(src, dir) => self.move_entry(src, dir),
            TreeAction::Delete(p) => self.confirm = Some(Confirm::Delete(p)),
            TreeAction::CopyPath(p) => ctx.output_mut(|o| o.copied_text = p.display().to_string()),
            TreeAction::Reveal(p) => self.reveal(&p),
            TreeAction::Terminal(d) => {
                let shell = self.term_shell();
                self.term.open(ctx, shell.as_deref(), Some(d));
                self.bottom = Some(BottomTab::Terminal);
            }
            TreeAction::Refresh => self.exec_refresh_tree(),
        }
    }

    /// Top of the git view: branch and its upstream, pull / push / fetch / branches, git errors,
    /// then the commit message and button (the changes themselves are the first rows of the graph).
    fn git_bar(&mut self, ui: &mut egui::Ui) {
        if self.repo.is_none() {
            ui.label("Aucun dépôt git dans ce dossier.");
            if let Some(root) = self.root.clone() {
                if ui.button("git init").clicked() {
                    match git::run(&root, &["init"]) {
                        Ok(o) => {
                            self.log(o);
                            self.repos = git::find_repos(&root, 3);
                            if let Some(r) = self.repos.first().cloned() {
                                self.select_repo(r);
                            }
                        }
                        Err(e) => self.log(e),
                    }
                }
            }
            return;
        }
        let accent = self.hl.colors.accent;
        let bg = ui.visuals().panel_fill;
        let mut ops: Vec<(&str, Vec<String>)> = Vec::new();
        let (mut do_commit, mut dismiss_error) = (false, false);

        // --- branch, its upstream and the remote actions
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if let Some(st) = &self.git.status {
                ui.label(RichText::new(format!("🔀 {}", st.branch)).strong().size(15.0));
                match st.upstream() {
                    Some((up, ahead, behind)) => {
                        if ahead > 0 {
                            ui.label(RichText::new(format!("↑{ahead}")).color(ok()).strong())
                                .on_hover_text(format!("{ahead} commit(s) à pousser vers {up} (Push)"));
                        }
                        if behind > 0 {
                            ui.label(RichText::new(format!("↓{behind}")).color(warn()).strong())
                                .on_hover_text(format!("{behind} commit(s) à récupérer de {up} (Pull)"));
                        }
                        if ahead == 0 && behind == 0 {
                            ui.label(RichText::new("✔ à jour").color(ok()).small()).on_hover_text(format!("Identique à {up}"));
                        }
                    }
                    None => {
                        ui.label(RichText::new("pas de branche distante").weak().small());
                    }
                }
            }
            if self.git.busy > 0 {
                ui.spinner();
            }
            ui.add_space(8.0);
            for (label, cmd, tip) in [
                ("⬇ Pull", Cmd::GitPull, "Récupérer et intégrer les commits distants"),
                ("⬆ Push", Cmd::GitPush, "Envoyer tes commits"),
                ("Fetch", Cmd::GitFetch, "Récupérer sans intégrer"),
                ("Branche ▾", Cmd::GitCheckout, "Changer de branche"),
                ("+ Branche", Cmd::GitNewBranch, "Nouvelle branche"),
            ] {
                if ui.button(label).on_hover_text(tip).clicked() {
                    self.pending.push(cmd);
                }
            }
        });

        // --- errors stay until dismissed (or until a later git command succeeds)
        if let Some((label, e)) = self.git.op_error.clone().or_else(|| self.git.error.clone().map(|e| ("status".into(), e))) {
            egui::Frame::none()
                .fill(icons::mix(bg, err(), 0.14))
                .stroke(egui::Stroke::new(1.0_f32, err()))
                .rounding(6.0)
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("⚠ git {label} a échoué")).color(err()).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text("Masquer").clicked() {
                                dismiss_error = true;
                            }
                            if ui.small_button("Copier").clicked() {
                                ui.ctx().output_mut(|o| o.copied_text = e.clone());
                            }
                        });
                    });
                    egui::ScrollArea::vertical().id_salt("git_err").max_height(110.0).show(ui, |ui| {
                        ui.label(RichText::new(&e).monospace().small());
                    });
                });
        }
        // --- commit
        let (n_staged, n_changed) = self.git.status.as_ref().map_or((0, 0), |st| {
            (st.entries.iter().filter(|e| e.staged()).count(), st.entries.iter().filter(|e| e.unstaged() && e.x != '?').count())
        });
        ui.horizontal(|ui| {
            let (label, tip) = if n_staged > 0 {
                (format!("✔ Commit ({n_staged} indexé(s))"), "Valide les fichiers indexés")
            } else {
                (format!("✔ Commit tout ({n_changed})"), "Rien d'indexé : valide toutes les modifications des fichiers suivis")
            };
            let can = !self.git.commit_msg.trim().is_empty() && (n_staged > 0 || n_changed > 0);
            // the button on the right, the message takes the rest of the line
            let mut clicked = false;
            let resp = ui
                .with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    clicked = ui.add_enabled(can, filled_button(label, accent)).on_hover_text(tip).clicked();
                    ui.add(
                        egui::TextEdit::multiline(&mut self.git.commit_msg)
                            .hint_text("Message du commit (Ctrl+Entrée)")
                            .desired_rows(1)
                            .desired_width(f32::INFINITY),
                    )
                })
                .inner;
            if self.git.focus_commit {
                resp.request_focus();
                self.git.focus_commit = false;
            }
            let ctrl_enter = resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
            if can && (clicked || ctrl_enter) {
                do_commit = true;
            }
        });
        ui.add_space(4.0);

        if dismiss_error {
            self.git.op_error = None;
            self.git.error = None;
        }
        if do_commit {
            let msg = self.git.commit_msg.trim().to_string();
            let mut args = vec!["commit".to_string()];
            if n_staged == 0 {
                args.push("-a".into()); // nothing staged: commit all tracked changes
            }
            args.push("-m".into());
            args.push(msg);
            ops.push(("commit", args));
            self.git.commit_msg.clear();
        }
        for (label, args) in ops {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            self.git_job(label, &a);
        }
    }

    pub(super) fn bottom_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom").resizable(true).default_height(230.0).height_range(80.0..=900.0).show(
            ctx,
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let nd: usize = self.docs.iter().map(|d| d.diagnostics.len()).sum();
                    let problems = if nd > 0 { format!("Problèmes {nd}") } else { "Problèmes".to_string() };
                    for (tab, label) in [
                        (BottomTab::Search, "Recherche".to_string()),
                        (BottomTab::Problems, problems),
                        (BottomTab::Terminal, "Terminal".to_string()),
                        (BottomTab::Output, "Sortie".to_string()),
                    ] {
                        if nav_tab(ui, &label, self.bottom == Some(tab)).clicked() {
                            self.bottom = Some(tab);
                            match tab {
                                BottomTab::Search => self.search.focus = true,
                                BottomTab::Terminal => self.term.want_focus = true,
                                _ => {}
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").clicked() {
                            self.bottom = None;
                        }
                    });
                });
                ui.separator();
                match self.bottom {
                    // already shown in a tab: not twice
                    Some(BottomTab::Search) if self.docs.iter().any(|d| d.view == document::View::Search) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("La recherche est dans un onglet de l'éditeur.").weak());
                            if ui.button("La remettre ici").clicked() {
                                if let Some(i) = self.docs.iter().position(|d| d.view == document::View::Search) {
                                    self.close_tab(i, true);
                                }
                                self.settings.search_in_tab = false;
                                self.persist("search_in_tab", "false".into());
                            }
                        });
                    }
                    Some(BottomTab::Search) => self.search_ui(ui, false),
                    Some(BottomTab::Problems) => self.problems_ui(ui),
                    Some(BottomTab::Terminal) => {
                        if self.term.cwd.is_none() {
                            self.term.cwd = self.root.clone();
                        }
                        let shell = self.term_shell();
                        self.term.ui(ui, shell.as_deref(), self.hl.colors, self.hl.is_light());
                    }
                    Some(BottomTab::Output) => {
                        if ui.small_button("Effacer").clicked() {
                            self.output.clear();
                        }
                        egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                            for l in &self.output {
                                ui.add(egui::Label::new(RichText::new(l).monospace()).extend());
                            }
                        });
                    }
                    None => {}
                }
            },
        );
    }

    fn problems_ui(&mut self, ui: &mut egui::Ui) {
        let mut goto = None;
        if let Some(linter) = self.python_linter.clone() {
            ui.horizontal(|ui| {
                if linter == "syntaxe" {
                    ui.label(
                        RichText::new("Python : aucun linter installé, seules les erreurs de syntaxe sont détectées.").color(warn()),
                    );
                    if ui.button("Installer flake8").on_hover_text(format!("pip install --target {}", lint::tools_dir().display())).clicked() {
                        self.pending.push(Cmd::InstallFlake8);
                    }
                } else {
                    ui.label(RichText::new(format!("Python : {linter}")).weak().small());
                }
            });
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut any = false;
            for (i, d) in self.docs.iter().enumerate() {
                if d.diagnostics.is_empty() {
                    continue;
                }
                any = true;
                ui.label(RichText::new(d.title()).strong());
                for g in &d.diagnostics {
                    let (icon, col) = if g.severity == Severity::Error { ("✖", err()) } else { ("⚠", warn()) };
                    let text = format!("  {icon} {}:{}  {}  [{}]", g.line + 1, g.col + 1, g.message, g.source);
                    let r = ui.add(egui::Label::new(RichText::new(text).color(col).monospace()).sense(Sense::click()));
                    if r.clicked() {
                        goto = Some((i, g.line, g.col));
                    }
                }
            }
            if !any {
                ui.label(RichText::new("Aucun problème détecté (le lint Python/JS s'exécute à l'enregistrement).").weak());
            }
        });
        if let Some((i, l, c)) = goto {
            self.active = i;
            self.docs[i].goto_line(l, c);
            ui.ctx().memory_mut(|m| m.request_focus(editor_id()));
        }
    }

    pub(super) fn central(&mut self, ctx: &egui::Context) {
        self.normalize_panes();
        self.drop_zones = DropZones::default();
        if std::mem::take(&mut self.pending_focus) {
            ctx.memory_mut(|m| m.request_focus(editor_id()));
        }
        let mut welcome = false;
        let mut editor_resp = None;
        let special = self.prefs.open || self.history.open || self.split.as_ref().is_some_and(|s| s.shown);
        let two = !special && self.is_split();
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
            if !self.docs.is_empty() || special {
                self.tabs_strip(ui, None);
            }
            if self.prefs.open {
                egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.prefs_ui(ui));
                return;
            }
            if self.split.as_ref().is_some_and(|s| s.shown) {
                egui::CentralPanel::default().show_inside(ui, |ui| self.split_ui(ui));
                return;
            }
            if self.history.open {
                let fill = ui.visuals().panel_fill;
                egui::TopBottomPanel::top("git_bar")
                    .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 8.0, bottom: 4.0 }))
                    .show_inside(ui, |ui| self.git_bar(ui));
                egui::CentralPanel::default().show_inside(ui, |ui| self.history_ui(ui));
                return;
            }
            self.drop_zones.panes.push((0, ui.available_rect_before_wrap()));
            if self.active < self.docs.len() {
                self.breadcrumbs_ui(ui, self.active, 0);
                self.conflict_bar(ui, self.active, 0);
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
        ui.label(RichText::new(format!("Ligne {}", line + 1)).small().weak());
        item(ui, "Couper", Cmd::Cut, !ro);
        item(ui, "Copier", Cmd::Copy, true);
        item(ui, "Coller", Cmd::Paste, !ro);
        if !word.is_empty() {
            let short: String = word.chars().take(28).collect();
            let short = if short.len() < word.len() { format!("{short}…") } else { short };
            ui.separator();
            item(ui, &format!("Sélectionner toutes les occurrences de « {short} »"), Cmd::SelectAllOccurrences, true);
            item(ui, &format!("Rechercher « {short} » dans le dossier"), Cmd::FindWordInFiles, has_root);
        }
        ui.separator();
        item(ui, "Aller à la définition", Cmd::GotoDefinition, true);
        item(ui, "Rechercher les usages", Cmd::FindReferences, true);
        item(ui, "Suggestions", Cmd::Complete, !ro);
        if has_lsp {
            item(ui, "Renommer le symbole…", Cmd::RenameSymbol, !ro);
        }
        if !noqa.is_empty() {
            ui.separator();
            item(ui, &format!("Ignorer {} sur cette ligne (# noqa)", noqa.join(", ")), Cmd::NoqaLine, !ro);
        }
        if is_python {
            ui.separator();
            item(ui, if has_selection { "Formater les lignes sélectionnées (ruff)" } else { "Formater le fichier (ruff)" }, Cmd::FormatDocument, !ro);
            item(ui, "Corriger automatiquement (ruff --fix)", Cmd::FixDocument, !ro);
        }
        ui.separator();
        item(ui, "Ajouter / retirer un signet", Cmd::ToggleBookmark, true);
        item(ui, "Commenter / décommenter", Cmd::ToggleComment, !ro);
        item(ui, "Replier le bloc", Cmd::Fold, true);
        item(ui, "Tout replier", Cmd::FoldAll, true);
        item(ui, "Tout déplier", Cmd::UnfoldAll, has_folds);
        item(ui, "Dupliquer la ligne", Cmd::DuplicateLine, !ro);
        item(ui, "Supprimer la ligne", Cmd::DeleteLine, !ro);
        item(ui, "Monter la ligne", Cmd::MoveLineUp, !ro);
        item(ui, "Descendre la ligne", Cmd::MoveLineDown, !ro);
        if in_repo {
            ui.separator();
            item(ui, if blame_on { "Masquer le blame" } else { "Blame de la ligne" }, Cmd::GitBlame, true);
            item(ui, "Historique de la ligne", Cmd::GitLineHistory, true);
            if has_change {
                item(ui, "Voir le changement", Cmd::ShowChange, true);
                item(ui, "Revenir en arrière sur ce changement", Cmd::GitRevertChange, !ro);
            }
            item(ui, "Copier le lien GitHub de la ligne", Cmd::CopyGithubLink, true);
        }
        if has_path {
            ui.separator();
            item(ui, "Copier le chemin du fichier", Cmd::CopyFilePath, true);
            item(ui, "Copier la référence fichier:ligne", Cmd::CopyFileRef, true);
            item(ui, "Afficher dans l'arborescence", Cmd::RevealInTree, true);
            item(ui, "Afficher dans le gestionnaire de fichiers", Cmd::RevealInOs, true);
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
            self.set_status("Fichier hors d'un dépôt git");
            return;
        };
        let Some((owner, name)) = git::stdout(&repo, &["remote", "get-url", "origin"]).ok().and_then(|u| git::parse_github(u.trim()))
        else {
            self.set_status("Le remote « origin » n'est pas un dépôt GitHub");
            return;
        };
        let rev = git::stdout(&repo, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_else(|_| "HEAD".into());
        let rel = path.strip_prefix(&repo).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        let link = format!("https://github.com/{owner}/{name}/blob/{rev}/{rel}#L{}", head_line + 1);
        ctx.output_mut(|o| o.copied_text = link.clone());
        self.set_status(format!("Lien copié : {link}"));
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
                welcome_card(&mut cols[0], "DÉMARRER", start_c, &wc, |ui| {
                    for (label, c) in [
                        ("Nouveau fichier", Cmd::NewFile),
                        ("Ouvrir un dossier…", Cmd::OpenFolder),
                        ("Aller au fichier…", Cmd::QuickOpen),
                        ("Palette de commandes", Cmd::CommandPalette),
                        ("Rechercher dans le dossier", Cmd::FindInFiles),
                        ("Graphe git", Cmd::GitHistory),
                    ] {
                        if welcome_row(ui, label, &self.keymap.label(c), true, start_c, &wc).clicked() {
                            cmd = Some(c);
                        }
                    }
                });
                welcome_card(&mut cols[1], "RÉCENTS", logo::PURPLE, &wc, |ui| {
                    for p in self.projects.iter().filter(|p| p.path.is_dir()).take(6) {
                        let parent = p.path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                        let r = welcome_row(ui, &p.name, &parent, false, logo::PURPLE, &wc)
                            .on_hover_text(p.path.display().to_string());
                        if r.clicked() {
                            open = Some(p.path.clone());
                        }
                    }
                    if self.projects.is_empty() {
                        ui.label(RichText::new("Aucun projet récent").color(wc.dim));
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
        }
        self.was_focused = focused;
        if self.last_disk_check.elapsed() > Duration::from_secs(2) {
            self.last_disk_check = Instant::now();
            self.check_git_stamps();
            let mut msgs = Vec::new();
            for d in self.docs.iter_mut() {
                let Some(p) = d.path.clone() else { continue };
                let Ok(m) = std::fs::metadata(&p).and_then(|m| m.modified()) else { continue };
                if d.mtime.is_some() && d.mtime != Some(m) {
                    if d.is_dirty() {
                        d.mtime = Some(m);
                        msgs.push(format!("{} a été modifié sur le disque", d.title()));
                    } else if d.reload_from_disk().is_ok() {
                        msgs.push(format!("{} rechargé depuis le disque", d.title()));
                    }
                }
            }
            for m in msgs {
                self.set_status(m);
            }
        }
        ctx.request_repaint_after(Duration::from_secs(2));
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

    /// Shows `path` in the file tree: sidebar opened, folders expanded, entry scrolled into view.
    pub(super) fn reveal_in_tree(&mut self, path: &Path) {
        let Some(tree) = self.tree.as_mut() else {
            self.set_status("Aucun dossier ouvert");
            return;
        };
        let path = crate::os::canon(path);
        if !tree.reveal(&path) {
            self.set_status("Ce fichier n'est pas dans le dossier ouvert");
            return;
        }
        self.show_sidebar = true;
        self.side_tab = SideTab::Files;
        self.tree_nav.cursor = Some(path.clone());
        self.tree_scroll = Some(path);
    }

    pub(super) fn reveal(&mut self, path: &Path) {
        if let Err(e) = crate::os::reveal(path) {
            self.set_status(e);
        }
    }
}
