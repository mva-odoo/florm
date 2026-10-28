//! Panels around the editor: menus, tabs, status bar, sidebar, bottom panels.

use super::*;

impl App {
    pub(super) fn menu_bar(&mut self, ctx: &egui::Context) {
        if self.own_title_bar() {
            // menus hidden: no bar, the window buttons are in the tab strip
            if self.settings.show_menu_bar {
                self.title_bar(ctx);
            }
            return;
        }
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
    pub(super) fn menus(&self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>) {
        let km = &self.keymap;
        let plugins = &self.plugins;
        let plugin_cmds = &self.plugin_cmds;
        let mut item = |ui: &mut egui::Ui, label: &str, cmd: Cmd| {
            if ui.add(egui::Button::new(label).shortcut_text(km.label(cmd))).clicked() {
                pending.push(cmd);
                ui.close_menu();
            }
        };
        ui.menu_button(t!("Fichier", "File"), |ui| {
            item(ui, t!("Nouveau fichier", "New file"), Cmd::NewFile);
            item(ui, t!("Ouvrir un fichier…", "Open a file…"), Cmd::OpenFile);
            item(ui, t!("Ouvrir un dossier…", "Open a folder…"), Cmd::OpenFolder);
            item(ui, t!("Se connecter (SSH, WSL)…", "Connect (SSH, WSL)…"), Cmd::RemoteConnect);
            item(ui, t!("Changer de projet…", "Switch project…"), Cmd::SwitchProject);
            item(ui, t!("Fermer le dossier", "Close the folder"), Cmd::CloseFolder);
            item(ui, t!("Rouvrir l'onglet fermé", "Reopen closed tab"), Cmd::ReopenClosed);
            ui.separator();
            item(ui, t!("Enregistrer", "Save"), Cmd::Save);
            item(ui, t!("Enregistrer sous…", "Save as…"), Cmd::SaveAs);
            item(ui, t!("Tout enregistrer", "Save all"), Cmd::SaveAll);
            ui.separator();
            item(ui, t!("Fermer l'onglet", "Close tab"), Cmd::CloseTab);
            item(ui, t!("Préférences", "Preferences"), Cmd::OpenSettings);
            item(ui, t!("Ouvrir settings.toml", "Open settings.toml"), Cmd::OpenSettingsFile);
            item(ui, t!("Quitter", "Quit"), Cmd::Quit);
        });
        ui.menu_button(t!("Édition", "Edit"), |ui| {
            item(ui, t!("Annuler", "Undo"), Cmd::Undo);
            item(ui, t!("Rétablir", "Redo"), Cmd::Redo);
            ui.separator();
            item(ui, t!("Dupliquer la ligne", "Duplicate line"), Cmd::DuplicateLine);
            item(ui, t!("Supprimer la ligne", "Delete line"), Cmd::DeleteLine);
            item(ui, t!("Monter la ligne", "Move line up"), Cmd::MoveLineUp);
            item(ui, t!("Descendre la ligne", "Move line down"), Cmd::MoveLineDown);
            item(ui, t!("Joindre les lignes", "Join lines"), Cmd::JoinLines);
            item(ui, t!("Commenter / décommenter", "Comment / uncomment"), Cmd::ToggleComment);
            item(ui, t!("Indenter", "Indent"), Cmd::Indent);
            item(ui, t!("Désindenter", "Unindent"), Cmd::Unindent);
            item(ui, t!("Trier les lignes", "Sort lines"), Cmd::SortLines);
            item(ui, t!("MAJUSCULES", "UPPERCASE"), Cmd::UpperCase);
            item(ui, t!("minuscules", "lowercase"), Cmd::LowerCase);
        });
        ui.menu_button(t!("Sélection", "Selection"), |ui| {
            item(ui, t!("Tout sélectionner", "Select all"), Cmd::SelectAll);
            item(ui, t!("Sélectionner la ligne", "Select line"), Cmd::SelectLine);
            item(ui, t!("Ajouter l'occurrence suivante", "Add next occurrence"), Cmd::SelectNext);
            item(ui, t!("Toutes les occurrences", "All occurrences"), Cmd::SelectAllOccurrences);
            item(ui, t!("Diviser en lignes", "Split into lines"), Cmd::SplitLines);
            item(ui, t!("Curseur au-dessus", "Cursor above"), Cmd::AddCursorAbove);
            item(ui, t!("Curseur en dessous", "Cursor below"), Cmd::AddCursorBelow);
            item(ui, t!("Crochet correspondant", "Matching bracket"), Cmd::GotoBracket);
        });
        ui.menu_button(t!("Rechercher", "Find"), |ui| {
            item(ui, t!("Rechercher…", "Find…"), Cmd::Find);
            item(ui, t!("Remplacer…", "Replace…"), Cmd::Replace);
            item(ui, t!("Suivant", "Next"), Cmd::FindNext);
            item(ui, t!("Rechercher dans le dossier…", "Find in folder…"), Cmd::FindInFiles);
            item(ui, t!("Résultat global suivant", "Next global result"), Cmd::NextResult);
            item(ui, t!("Précédent", "Previous"), Cmd::FindPrev);
            ui.separator();
            item(ui, t!("Aller au fichier…", "Go to file…"), Cmd::QuickOpen);
            item(ui, t!("Aller à la ligne…", "Go to line…"), Cmd::GotoLine);
        });
        ui.menu_button(t!("Affichage", "View"), |ui| {
            item(ui, t!("Palette de commandes", "Command palette"), Cmd::CommandPalette);
            item(ui, t!("Barre latérale", "Sidebar"), Cmd::ToggleSidebar);
            item(ui, "Terminal", Cmd::ToggleTerminal);
            item(ui, t!("Problèmes", "Problems"), Cmd::ShowProblems);
            item(ui, t!("Sortie", "Output"), Cmd::ShowOutput);
            item(ui, "Minimap", Cmd::ToggleMinimap);
            item(ui, t!("Retour à la ligne automatique", "Word wrap"), Cmd::ToggleWordWrap);
            item(ui, t!("Barre de menus", "Menu bar"), Cmd::ToggleMenuBar);
            ui.separator();
            item(ui, t!("Syntaxe…", "Syntax…"), Cmd::SetSyntax);
            item(ui, t!("Thème…", "Theme…"), Cmd::SelectTheme);
            item(ui, t!("Zoom avant", "Zoom in"), Cmd::ZoomIn);
            item(ui, t!("Zoom arrière", "Zoom out"), Cmd::ZoomOut);
        });
        ui.menu_button("Git", |ui| {
            item(ui, t!("Panneau Git", "Git panel"), Cmd::ShowGit);
            item(ui, "Commit…", Cmd::GitCommit);
            item(ui, t!("Revue IA des changements", "AI review of the changes"), Cmd::GitAiReview);
            item(ui, "Pull", Cmd::GitPull);
            item(ui, "Push", Cmd::GitPush);
            item(ui, "Fetch", Cmd::GitFetch);
            ui.separator();
            item(ui, t!("Changer de branche…", "Switch branch…"), Cmd::GitCheckout);
            item(ui, t!("Nouvelle branche…", "New branch…"), Cmd::GitNewBranch);
            item(ui, t!("Diff du fichier", "Diff of the file"), Cmd::GitDiffFile);
            item(ui, t!("Historique (graphe)", "History (graph)"), Cmd::GitHistory);
            item(ui, t!("Pull requests GitHub", "GitHub pull requests"), Cmd::PullRequests);
            item(ui, t!("Créer une pull request…", "Create a pull request…"), Cmd::CreatePr);
            item(ui, t!("Diff dans l'éditeur (vert / rouge)", "Diff in the editor (green / red)"), Cmd::ToggleInlineDiff);
            item(ui, t!("Blame de la ligne", "Line blame"), Cmd::GitBlame);
            item(ui, t!("Historique de la ligne", "Line history"), Cmd::GitLineHistory);
            item(ui, t!("Historique du fichier", "File history"), Cmd::GitFileHistory);
            item(ui, t!("Rechercher dans l'historique…", "Search the history…"), Cmd::GitSearchHistory);
            item(ui, t!("Reflog (retrouver un commit perdu)", "Reflog (find a lost commit)"), Cmd::GitReflog);
        });
        ui.menu_button(t!("Outils", "Tools"), |ui| {
            item(ui, t!("Linter le fichier", "Lint the file"), Cmd::Lint);
            ui.separator();
            for (k, &(pi, ci)) in plugin_cmds.iter().enumerate() {
                item(ui, &plugins[pi].commands[ci].title, Cmd::Plugin(k));
            }
            if !plugin_cmds.is_empty() {
                ui.separator();
            }
            item(ui, "✳ Claude", Cmd::Claude);
            // also when docker is missing: the view then tells how to install it
            item(ui, t!("Docker (conteneurs et logs)", "Docker (containers and logs)"), Cmd::Docker);
            item(ui, t!("Débogage (F5)", "Debug (F5)"), Cmd::Debug);
            ui.separator();
            item(ui, t!("Insérer un modèle…", "Insert a template…"), Cmd::InsertTemplate);
            item(ui, t!("Nouveau modèle…", "New template…"), Cmd::NewTemplate);
            item(ui, t!("Dossier des modèles", "Templates folder"), Cmd::OpenTemplatesFolder);
            ui.separator();
            item(ui, t!("Recharger les plugins", "Reload plugins"), Cmd::ReloadPlugins);
            item(ui, t!("Dossier des plugins", "Plugins folder"), Cmd::OpenPluginsFolder);
            ui.separator();
            item(ui, t!("Synchroniser la configuration partagée", "Synchronize the shared configuration"), Cmd::SharedSync);
            item(ui, t!("Publier la configuration partagée", "Publish the shared configuration"), Cmd::SharedPublish);
        });
        ui.menu_button(t!("Aide", "Help"), |ui| {
            item(ui, t!("Guide de Florm (recherche, Git, terminal, modèles…)", "Florm guide (search, Git, terminal, templates…)"), Cmd::ShowGuide);
            item(ui, t!("Accueil", "Welcome"), Cmd::ShowWelcome);
            item(ui, t!("Palette de commandes", "Command palette"), Cmd::CommandPalette);
            item(ui, t!("Raccourcis et préférences", "Shortcuts and preferences"), Cmd::OpenSettings);
            ui.separator();
            item(ui, t!("Rechercher une mise à jour", "Check for an update"), Cmd::CheckUpdate);
        });
    }

    /// Tab strip drawn above the editor area only (not above the sidebar).
    /// Tab strip: every tab (`pane == None`, with the history / split / settings tabs) or those of one panel.
    pub(super) fn tabs_strip(&mut self, ui: &mut egui::Ui, pane: Option<u8>) {
        let tc = self.hl.tab_colors();
        let (mut activate, mut close) = (None, None);
        let (mut drag_start, mut menu_act) = (None, None);
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
            let controls = self.strip_controls(pane);
            let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            let mut scroll = bar;
            if let Some(c) = controls {
                // the empty part of the strip moves the window, as the title bar does
                let drag = ui.interact(bar, panel_id.with("drag"), egui::Sense::click_and_drag());
                crate::app::title_bar::drag_window(ui.ctx(), &drag, maximized);
                match c {
                    crate::os::Controls::Side => scroll.max.x -= crate::app::title_bar::WINDOW_BUTTONS_W,
                    _ => scroll.min.x += crate::app::title_bar::MAC_BUTTONS_W,
                }
            }
            // too many tabs for the strip: ‹ › and the list of all of them on its right
            let (over_id, offset_id) = (panel_id.with("over"), panel_id.with("offset"));
            let over = ui.ctx().data(|d| d.get_temp::<bool>(over_id)).unwrap_or(false);
            let (offset, width) = ui.ctx().data(|d| d.get_temp::<(f32, f32)>(offset_id)).unwrap_or((0.0, 0.0));
            const ARROWS_W: f32 = 76.0;
            let arrows = over.then(|| {
                let r = egui::Rect::from_min_max(egui::pos2(scroll.max.x - ARROWS_W, bar.min.y), egui::pos2(scroll.max.x, bar.max.y));
                scroll.max.x -= ARROWS_W;
                r
            });
            let mut goto: Option<f32> = None;
            // the wheel moves the strip sideways
            if over && ui.rect_contains_pointer(scroll) {
                let d = ui.input(|i| i.smooth_scroll_delta);
                if d.y != 0.0 && d.x == 0.0 {
                    goto = Some(offset - d.y);
                }
            }
            if let Some(r) = arrows {
                ui.allocate_ui_at_rect(r, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let page = (width * 0.75).max(120.0);
                        if ui.small_button("‹").on_hover_text(t!("Onglets à gauche", "Tabs on the left")).clicked() {
                            goto = Some(offset - page);
                        }
                        if ui.small_button("›").on_hover_text(t!("Onglets à droite", "Tabs on the right")).clicked() {
                            goto = Some(offset + page);
                        }
                        ui.menu_button("▾", |ui| {
                            ui.set_min_width(240.0);
                            egui::ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                                for (i, d) in self.docs.iter().enumerate().filter(|(_, d)| pane.is_none_or(|p| p == d.pane)) {
                                    let (_, title) = doc_tab(d);
                                    let label = if d.is_dirty() { format!("● {title}") } else { title };
                                    let r = ui.selectable_label(i == self.active, label);
                                    let r = match &d.path {
                                        Some(p) => r.on_hover_text(p.display().to_string()),
                                        None => r,
                                    };
                                    if r.clicked() {
                                        activate = Some(i);
                                        ui.close_menu();
                                    }
                                }
                            });
                        })
                        .response
                        .on_hover_text(t!("Tous les onglets", "All the tabs"));
                    });
                });
            }
            ui.allocate_ui_at_rect(scroll, |ui| {
            let mut area = egui::ScrollArea::horizontal()
                .id_salt(panel_id.with("scroll"))
                .drag_to_scroll(false)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
            if let Some(x) = goto {
                area = area.horizontal_scroll_offset(x.max(0.0));
            }
            let out = area
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
                            let active = shown;
                            let error = d.diagnostics.iter().any(|g| g.severity == Severity::Error);
                            // a Claude conversation: working, or finished and not seen yet
                            let tint = if error { Some(err()) } else { self.claude_tab_tint(d.uid) };
                            let (icon, title) = doc_tab(d);
                            let flash = if error { None } else { self.claude_tab_flash(d.uid) };
                            let (body, closed) = tab_widget(ui, &title, icon, d.is_dirty(), active, tint, flash, &tc);
                            if tint.is_some() && !error {
                                let tip = if self.claude_tab_tint(d.uid) == Some(ok()) { t!("Claude a fini", "Claude is done") } else { t!("Claude travaille…", "Claude is working…") };
                                let _ = body.clone().on_hover_text(tip);
                            }
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
                    });
                });
            let width = out.inner_rect.width();
            // with the arrows shown, the strip is narrower: they stay until the tabs fit without them
            let needs = out.content_size.x > width + if over { ARROWS_W } else { 1.0 };
            ui.ctx().data_mut(|d| {
                d.insert_temp(over_id, needs);
                d.insert_temp(offset_id, (out.state.offset.x, width));
            });
            if needs != over {
                ui.ctx().request_repaint();
            }
            });
            if let Some(c) = controls {
                crate::app::title_bar::strip_buttons(ui, bar, c, maximized);
            }
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
        if let Some(i) = close {
            self.close_tab(i, false);
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
                    tf!("{closed} onglet(s) fermé(s), {kept} non enregistré(s) gardé(s)", "{closed} tab(s) closed, {kept} unsaved kept")
                } else {
                    tf!("{closed} onglet(s) fermé(s)", "{closed} tab(s) closed")
                });
            }
            Some(TabAct::CopyPath(t)) => {
                ui.ctx().output_mut(|o| o.copied_text = t.clone());
                self.set_status(tf!("Copié : {t}", "Copied: {t}"));
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

    /// The update, at the bottom right of the status bar (next to the version): its menu
    /// installs or downloads it. Once installed, Florm restarts by itself (see `update`).
    fn update_symbol(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let link = ui.visuals().hyperlink_color;
        if let (Some(u), true) = (&self.update, self.update_installing) {
            ui.label(RichText::new(tf!("Mise à jour vers Florm {}…", "Updating to Florm {}…", u.version)).weak());
            spinner(ui);
        } else if let (Some(u), true) = (&self.update, self.update_ready.is_some()) {
            let text = RichText::new(tf!("⟳ Redémarrer (Florm {})", "⟳ Restart (Florm {})", u.version)).color(link);
            if ui.add(egui::Button::new(text).frame(false)).on_hover_text(t!("La nouvelle version est installée : Florm se relance (fichiers non enregistrés : la question habituelle)", "The new version is installed: Florm restarts (unsaved files: the usual question)")).clicked() {
                self.restart_for_update(ctx);
            }
        } else if let Some(u) = self.update.clone() {
            let text = RichText::new(format!("⬆ Florm {}", u.version)).color(link);
            ui.menu_button(text, |ui| {
                ui.visuals_mut().override_text_color = None;
                ui.label(tf!("Nouvelle version (vous avez la {})", "New version (you have {})", crate::update::current()));
                if ui.button(t!("Installer et redémarrer", "Install and restart")).on_hover_text(t!("Florm télécharge la nouvelle version, la vérifie et remplace celle-ci ; les préférences sont gardées. Sous Linux, un paquet système demande le mot de passe.", "Florm downloads the new version, checks it and replaces this one; the preferences are kept. On Linux, a system package asks for the password.")).clicked() {
                    self.install_update();
                    ui.close_menu();
                }
                if let Some((name, url)) = &u.file {
                    if ui.button(tf!("Télécharger {name}", "Download {name}")).on_hover_text(t!("Dans le navigateur ; installez-le comme la première fois : il remplace l'ancienne version, les préférences sont gardées.", "In the browser; install it like the first time: it replaces the old version, the preferences are kept.")).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        ui.close_menu();
                    }
                }
                if ui.button(t!("Notes de version et tous les fichiers", "Release notes and all the files")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&u.page));
                    ui.close_menu();
                }
                if ui.button(t!("Masquer", "Hide")).on_hover_text(t!("Jusqu'au prochain démarrage", "Until the next start")).clicked() {
                    self.update = None;
                    ui.close_menu();
                }
            })
            .response
            .on_hover_text(tf!("Florm {} est disponible", "Florm {} is available", u.version));
        }
    }

    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        let (mut toggle_spaces, mut convert_utf8) = (false, false);
        let panel = ctx.style().visuals.panel_fill;
        egui::TopBottomPanel::bottom("status")
            .exact_height(26.0)
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
                        if ui.button(t!("Afficher la barre de menus", "Show the menu bar")).clicked() {
                            pending.push(Cmd::ToggleMenuBar);
                            ui.close_menu();
                        }
                    })
                    .response
                    .on_hover_text(t!("Menus (barre de menus masquée)", "Menus (menu bar hidden)"));
                    self.pending.extend(pending);
                }
                if self.settings.tools_in_status_bar {
                    self.sidebar_tools(ui, true);
                    ui.separator();
                }
                if let Some(d) = self.docs.get(self.active) {
                    let p = d.primary_sel();
                    let (l, c) = d.line_col(p.head);
                    ui.label(tf!("Ligne {}, Colonne {}", "Line {}, Column {}", l + 1, c + 1));
                    if d.sels.len() > 1 {
                        ui.label(tf!("· {} sélections", "· {} selections", d.sels.len()));
                    } else if !p.is_empty() {
                        ui.label(tf!("· {} caractères sélectionnés", "· {} characters selected", p.end() - p.start()));
                    }
                    if let Some(g) = d.diagnostics.iter().find(|g| g.line == l) {
                        let col = if g.severity == Severity::Error { err() } else { warn() };
                        ui.label(RichText::new(&g.message).color(col));
                    }
                }
                // the message of the moment; a click shows the last ones (one may have gone too fast)
                if !self.status_log.is_empty() {
                    let text = match self.status.as_ref().filter(|m| m.shown()) {
                        Some(m) if m.error => RichText::new(format!("⚠ {}", m.text)).color(err()),
                        Some(m) => RichText::new(&m.text).weak(),
                        None => RichText::new("💬").weak(),
                    };
                    ui.menu_button(text, |ui| {
                        ui.visuals_mut().override_text_color = None;
                        ui.set_max_width(560.0);
                        ui.label(RichText::new(t!("Derniers messages", "Last messages")).small().weak());
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            for m in self.status_log.iter().rev() {
                                let secs = m.at.elapsed().as_secs();
                                let ago = if secs < 60 { tf!("{secs} s", "{secs} s") } else if secs < 3600 { tf!("{} min", "{} min", secs / 60) } else { tf!("{} h", "{} h", secs / 3600) };
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(ago).small().weak());
                                    let text = if m.error { RichText::new(format!("⚠ {}", m.text)).color(err()) } else { RichText::new(&m.text) };
                                    ui.add(egui::Label::new(text).wrap());
                                });
                            }
                        });
                    })
                    .response
                    .on_hover_text(t!("Derniers messages", "Last messages"));
                    // the message disappears by itself: a repaint when its time is over
                    if let Some(m) = self.status.as_ref().filter(|m| m.shown()) {
                        let left = Duration::from_secs(if m.error { 15 } else { 6 }).saturating_sub(m.at.elapsed());
                        ctx.request_repaint_after(left);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let version = egui::Button::new(RichText::new(format!("v{}", crate::update::current())).small()).frame(false);
                    if ui.add(version).on_hover_text(t!("Version de Florm · clic : rechercher une mise à jour", "Florm version · click: check for an update")).clicked() {
                        self.pending.push(Cmd::CheckUpdate);
                    }
                    self.update_symbol(ui, ctx);
                    // the rest of the file's details and the language server in a menu
                    let mut pending = Vec::new();
                    let lsp_line = self.lsp_status_line();
                    ui.menu_button("⋯", |ui| {
                        ui.visuals_mut().override_text_color = None;
                        if let Some(d) = self.docs.get(self.active) {
                            if ui.button(tf!("Syntaxe : {}…", "Syntax: {}…", d.syntax)).clicked() {
                                pending.push(Cmd::SetSyntax);
                                ui.close_menu();
                            }
                            let ind = if d.use_spaces { tf!("Espaces : {}", "Spaces: {}", d.tab_size) } else { tf!("Tabulations : {}", "Tabs: {}", d.tab_size) };
                            if ui.button(tf!("Indentation : {ind}", "Indentation: {ind}")).on_hover_text(t!("Clic : passer aux espaces / aux tabulations", "Click: switch to spaces / tabs")).clicked() {
                                toggle_spaces = true;
                                ui.close_menu();
                            }
                            ui.label(tf!("Fin de ligne : {}", "Line ending: {}", if d.crlf { "CRLF (Windows)" } else { "LF" }));
                            ui.label(tf!("Encodage : {}", "Encoding: {}", d.encoding.label()));
                        }
                        if let Some((text, warning, install)) = &lsp_line {
                            ui.separator();
                            let t = if *warning { RichText::new(text).color(warn()) } else { RichText::new(text) };
                            if *install {
                                if ui.button(t).on_hover_text(t!("Installe python-lsp-server pour Florm (pip --target, sans toucher au Python système). Ou installe pyright.", "Installs python-lsp-server for Florm (pip --target, without touching the system Python). Or install pyright.")).clicked() {
                                    pending.push(Cmd::InstallPylsp);
                                    ui.close_menu();
                                }
                            } else {
                                ui.label(t);
                            }
                        }
                    })
                    .response
                    .on_hover_text(t!("Syntaxe, indentation, fin de ligne, encodage, serveur de langage", "Syntax, indentation, line ending, encoding, language server"));
                    self.pending.extend(pending);
                    if let Some(d) = self.docs.get(self.active) {
                        // only shown when it is not UTF-8: the file is saved back in its encoding
                        if d.encoding != document::Encoding::Utf8
                            && ui
                                .add(egui::Button::new(RichText::new(d.encoding.label()).color(warn())).frame(false))
                                .on_hover_text(t!("Le fichier n'est pas en UTF-8 : il est enregistré dans son encodage.\nClic : l'enregistrer en UTF-8 désormais.", "The file is not in UTF-8: it is saved in its encoding.\nClick: save it in UTF-8 from now on."))
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
                        if ui.add(egui::Button::new(txt).frame(false)).on_hover_text(t!("Problèmes (Ctrl+Shift+M)", "Problems (Ctrl+Shift+M)")).clicked() {
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
                self.set_status(t!("Sera enregistré en UTF-8 (Ctrl+S)", "Will be saved in UTF-8 (Ctrl+S)"));
            }
        }
    }

    /// Language server of the active file, for the status bar menu: (text, is a warning, the
    /// click installs pylsp).
    fn lsp_status_line(&self) -> Option<(String, bool, bool)> {
        let fam = self.settings.lsp.then(|| self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(lsp::language_of)).flatten()?.0;
        let running = self.lsp.manager.clients.iter().flatten().find(|c| c.family == fam);
        match (running, self.lsp.notes.get(fam)) {
            (Some(c), _) if c.ready => Some((tf!("Serveur de langage : {}", "Language server: {}", c.name), false, false)),
            (Some(c), _) => Some((tf!("Serveur de langage : {} (démarrage…)", "Language server: {} (starting…)", c.name), false, false)),
            (None, Some(_)) if fam == "python" => Some((t!("Serveur de langage : installer pylsp", "Language server: install pylsp").into(), true, true)),
            (None, Some(why)) => Some((tf!("Pas de serveur de langage : {why}", "No language server: {why}"), true, false)),
            _ => None,
        }
    }

    pub(super) fn find_bar(&mut self, ctx: &egui::Context) {
        let (mut action, mut scope_toggled) = (None, false);
        egui::TopBottomPanel::bottom("find").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let f = &mut self.find;
                if ui.toggle_value(&mut f.case, "Aa").on_hover_text(t!("Respecter la casse", "Match case")).changed()
                    | ui.toggle_value(&mut f.regex, ".*").on_hover_text(t!("Expression régulière", "Regular expression")).changed()
                    | ui.toggle_value(&mut f.whole, "« »").on_hover_text(t!("Mot entier", "Whole word")).changed()
                {
                    action = Some(FindAction::Incremental);
                }
                if ui
                    .toggle_value(&mut f.in_sel, t!("Sél.", "Sel."))
                    .on_hover_text(t!("Chercher et remplacer seulement dans la sélection actuelle (zone teintée)", "Find and replace only in the current selection (tinted area)"))
                    .changed()
                {
                    scope_toggled = true;
                }
                let w = (ui.available_width() - 330.0).max(120.0);
                let te = ui.add(
                    egui::TextEdit::singleline(&mut f.query)
                        .desired_width(w)
                        .hint_text(t!("Rechercher (Entrée : suivant, Maj+Entrée : précédent, Alt+Entrée : tout sélectionner)", "Find (Enter: next, Shift+Enter: previous, Alt+Enter: select all)"))
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
                if ui.button("◀").on_hover_text(t!("Précédent", "Previous")).clicked() {
                    action = Some(FindAction::Prev);
                }
                if ui.button("▶").on_hover_text(t!("Suivant", "Next")).clicked() {
                    action = Some(FindAction::Next);
                }
                if ui.button(t!("Tout", "All")).on_hover_text(t!("Sélectionner tous les résultats (multi-curseur)", "Select all the results (multi-cursor)")).clicked() {
                    action = Some(FindAction::All);
                }
                match &f.error {
                    Some(e) => ui.label(RichText::new(e.lines().last().unwrap_or("")).color(err())),
                    None => ui.label(RichText::new(tf!("{} résultat(s)", "{} result(s)", f.matches.len())).weak()),
                };
            });
            if self.find.replace_mode {
                ui.horizontal(|ui| {
                    ui.add_space(110.0);
                    let w = (ui.available_width() - 330.0).max(120.0);
                    let te = ui.add(replace_field(&mut self.find.replacement, t!("Remplacer par ($1 pour les groupes en mode regex)", "Replace with ($1 for the groups in regex mode)")).desired_width(w)).on_hover_text(replace_tip());
                    if te.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift) {
                        action = Some(FindAction::ReplaceOne);
                    }
                    if ui.button(t!("Remplacer", "Replace")).clicked() {
                        action = Some(FindAction::ReplaceOne);
                    }
                    let all = if self.find.in_sel { t!("Tout remplacer dans la sélection", "Replace all in the selection") } else { t!("Tout remplacer", "Replace all") };
                    if ui.button(all).clicked() {
                        action = Some(FindAction::ReplaceAll);
                    }
                    ui.toggle_value(&mut self.find.preserve, t!("Garder la casse", "Keep case"))
                        .on_hover_text(t!("chat → chien, Chat → Chien, CHAT → CHIEN", "cat → dog, Cat → Dog, CAT → DOG"));
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
                    self.set_status(t!("Sélectionne d'abord le texte dans lequel chercher", "First select the text to search in"));
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
        let panel = sidebar_fill(&ctx.style().visuals);
        egui::SidePanel::left("sidebar_strip").exact_width(10.0).resizable(false).frame(egui::Frame::none().fill(panel)).show(ctx, |ui| {
            let rect = ui.max_rect();
            let r = ui.interact(rect, egui::Id::new("sidebar_strip_btn"), Sense::click());
            let c = if r.hovered() { self.hl.colors.accent } else { ui.visuals().weak_text_color().gamma_multiply(0.5) };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "›", egui::FontId::proportional(14.0), c);
            if r.on_hover_text(t!("Afficher la barre latérale (Ctrl+K Ctrl+B)", "Show the sidebar (Ctrl+K Ctrl+B)")).clicked() {
                self.show_sidebar = true;
            }
        });
    }

    pub(super) fn sidebar(&mut self, ctx: &egui::Context) {
        let mut tree_action = None;
        let panel = sidebar_fill(&ctx.style().visuals);
        let shown = egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(260.0)
            .width_range(150.0..=700.0)
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin { left: 10.0, right: 8.0, top: 4.0, bottom: 6.0 }))
            .show(ctx, |ui| {
            // what is drawn inside (sticky folders, icons) on the same background
            ui.visuals_mut().panel_fill = panel;
            self.sidebar_mac_buttons(ui);
            // the views of the bottom panel, icons only: left click in the editor area, right click
            // there; projects are under the folder name (Ctrl+Alt+P). At the top or the bottom of the sidebar.
            let top = self.settings.sidebar_tools_top;
            let in_bar = self.settings.tools_in_status_bar;
            let frame = egui::Frame::none().inner_margin(if top { egui::Margin { left: 0.0, right: 0.0, top: 0.0, bottom: 6.0 } } else { egui::Margin { left: 0.0, right: 0.0, top: 6.0, bottom: 0.0 } });
            if in_bar {
            } else if top {
                egui::TopBottomPanel::top("sidebar_tools").frame(frame).show_inside(ui, |ui| self.sidebar_tools(ui, false));
            } else {
                egui::TopBottomPanel::bottom("sidebar_tools").frame(frame).show_inside(ui, |ui| self.sidebar_tools(ui, false));
            }
            match self.tree.as_mut() {
                Some(tree) => {
                    let active = self.docs.get(self.active).and_then(|d| d.path.clone());
                    let empty = HashMap::new();
                    let info = if self.settings.tree_git_info { &self.repo_info } else { &empty };
                    let projects: Vec<(String, PathBuf)> = self.projects.iter().filter(|p| p.path.is_dir()).map(|p| (p.name.clone(), p.path.clone())).collect();
                    if !self.tree_ex.as_ref().is_some_and(|(r, pats, _)| *r == tree.path && *pats == self.settings.exclude) {
                        let ex = crate::walk::Exclusions::new(&tree.path, &self.settings.exclude);
                        self.tree_ex = Some((tree.path.clone(), self.settings.exclude.clone(), ex));
                    }
                    let ex = &self.tree_ex.as_ref().expect("just set").2;
                    tree_action = sidebar::show(ui, tree, &projects, ex, active.as_deref(), &self.git.map, info, &mut self.tree_scroll, &mut self.tree_nav);
                }
                None => {
                    ui.label(t!("Aucun dossier ouvert.", "No folder open."));
                    if ui.button(t!("Ouvrir un dossier…", "Open a folder…")).clicked() {
                        self.pending.push(Cmd::OpenFolder);
                    }
                }
            }
        });
        // double click on the border between the sidebar and the editor: the width fits the longest name
        let edge = shown.response.rect.right();
        let fit = ctx.input(|i| {
            i.pointer.button_double_clicked(egui::PointerButton::Primary)
                && i.pointer.interact_pos().is_some_and(|p| (p.x - edge).abs() <= 5.0 && shown.response.rect.y_range().contains(p.y))
        });
        if fit {
            if let Some(tree) = self.tree.as_ref() {
                let width = sidebar::fit_width(ctx, tree).clamp(150.0, 700.0);
                let r = shown.response.rect;
                // the panel's width is kept by egui between frames: this one is the next frame's
                let rect = egui::Rect::from_min_max(r.min, egui::pos2(r.min.x + width, r.max.y));
                ctx.data_mut(|d| d.insert_persisted(egui::Id::new("sidebar"), egui::containers::panel::PanelState { rect }));
                ctx.request_repaint();
            }
        }
        if let Some(a) = tree_action {
            self.tree_action(ctx, a);
        }
    }

    /// The icons of the views (`sidebar`), with the menu that chooses which are shown.
    /// `in_bar`: in the status bar (one row, smaller) rather than in the sidebar.
    fn sidebar_tools(&mut self, ui: &mut egui::Ui, in_bar: bool) {
        let top = self.settings.sidebar_tools_top;
        let size = if in_bar { egui::vec2(24.0, 22.0) } else { egui::vec2(26.0, 24.0) };
        let row = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
            if in_bar {
                ui.horizontal(|ui| add(ui));
            } else {
                ui.horizontal_wrapped(|ui| add(ui));
            }
        };
        row(ui, &mut |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let hidden = self.settings.sidebar_hidden.clone();
            let shown = |key: &str| !hidden.iter().any(|h| h == key);
            // preferences, left of the search (the first view)
            if shown("settings") {
                let (rect, r) = ui.allocate_exact_size(size, Sense::click());
                if r.hovered() {
                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let c = if r.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
                icons::paint_view(ui.painter(), rect.center(), icons::ViewIcon::Settings, c);
                if r.on_hover_text(t!("Préférences", "Preferences")).clicked() {
                    self.pending.push(Cmd::OpenSettings);
                }
            }
            // problems and output are in the status bar, the debugger opens by itself: not here
            for (tab, icon, label) in BottomTab::shown().filter(|(t, ..)| t.in_sidebar() && shown(&t.key())) {
                let (rect, r) = ui.allocate_exact_size(size, Sense::click());
                let on = self.bottom == Some(tab);
                // Claude finished in the bottom panel, not seen yet: in green
                let done = tab == BottomTab::Claude && !on && self.claude_bottom_done();
                if done {
                    ui.painter().rect_filled(rect, 4.0, icons::mix(ui.visuals().panel_fill, ok(), 0.3));
                } else if r.hovered() || on {
                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let c = if done { ok() } else if r.hovered() || on { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
                icons::paint_view(ui.painter(), rect.center(), icon, c);
                let r = r.on_hover_text(format!("{label}\n{}", BottomTab::hint(tab)));
                if r.clicked() && !tab.bottom_only() {
                    self.show_big(tab);
                } else if r.clicked() || r.secondary_clicked() {
                    self.show_bottom(tab, true);
                }
            }
            // which icons are shown
            let mut changed = None;
            let mut placed = None;
            let mut to_bar = None;
            let menu = ui.menu_button(RichText::new("▾").weak(), |ui| {
                ui.visuals_mut().override_text_color = None;
                ui.label(RichText::new(t!("Icônes affichées", "Icons shown")).small().weak());
                let mut items = vec![("settings".to_string(), t!("Préférences", "Preferences").to_string())];
                items.extend(BottomTab::shown().filter(|(t, ..)| t.in_sidebar()).map(|(t, _, l)| (t.key(), l.to_string())));
                for (key, label) in items {
                    let mut on = shown(&key);
                    if ui.checkbox(&mut on, label).changed() {
                        changed = Some((key, on));
                    }
                }
                ui.separator();
                if ui.add_enabled(!hidden.is_empty(), egui::Button::new(t!("Tout afficher", "Show all"))).clicked() {
                    changed = Some((String::new(), true));
                    ui.close_menu();
                }
                let mut bar = in_bar;
                if ui.checkbox(&mut bar, t!("Dans la barre d'état (en bas)", "In the status bar (at the bottom)")).changed() {
                    to_bar = Some(bar);
                }
                if !in_bar {
                    let mut at_top = top;
                    if ui.checkbox(&mut at_top, t!("En haut de la barre latérale", "At the top of the sidebar")).changed() {
                        placed = Some(at_top);
                    }
                }
            });
            if let Some(bar) = to_bar {
                self.settings.tools_in_status_bar = bar;
                self.persist("tools_in_status_bar", bar.to_string());
            }
            if let Some(at_top) = placed {
                self.settings.sidebar_tools_top = at_top;
                self.persist("sidebar_tools_top", at_top.to_string());
            }
            menu.response.on_hover_text(t!("Afficher ou masquer ces icônes (les vues restent dans la palette et les menus)", "Show or hide these icons (the views stay in the palette and the menus)"));
            if let Some((key, on)) = changed {
                let h = &mut self.settings.sidebar_hidden;
                match (key.is_empty(), on) {
                    (true, _) => h.clear(),
                    (false, true) => h.retain(|x| *x != key),
                    (false, false) => h.push(key),
                }
                let v = config::toml_str_array(h);
                self.persist("sidebar_hidden", v);
            }
        });
    }

    fn tree_action(&mut self, ctx: &egui::Context, a: TreeAction) {
        match a {
            TreeAction::Open(p) => {
                self.open_path(p);
                self.focus_editor(ctx);
            }
            TreeAction::Show(p) => {
                self.open_path(p);
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
            TreeAction::Claude(p) => self.claude_attach_path(p),
            TreeAction::SwitchProject(p) => self.switch_project(p),
            TreeAction::Exclude(p, on) => self.exclude_folder(&p, on),
            TreeAction::OpenFolder => self.pending.push(Cmd::OpenFolder),
            TreeAction::CopyFile(p) => self.copy_entry(&p),
            TreeAction::Paste(d) => self.paste_into(d),
            TreeAction::InitRepo(d) => self.init_repo(d),
        }
    }

    /// Top of the git view: branch and its upstream, pull / push / fetch / branches, git errors,
    /// then the commit message and button (the changes themselves are the first rows of the graph).
    pub(super) fn git_bar(&mut self, ui: &mut egui::Ui) {
        if self.repo.is_none() {
            ui.label(t!("Aucun dépôt git dans ce dossier.", "No git repository in this folder."));
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
        let bg = ui.visuals().panel_fill;
        let mut dismiss_error = false;
        let (mut chosen_repo, mut reload) = (None, false);
        let repo_name = self.history.repo.as_deref().map(|r| self.repo_label(r)).unwrap_or_default();
        let repo_choices: Vec<(PathBuf, String)> = self.repos.iter().map(|r| (r.clone(), self.repo_label(r))).collect();
        // pull requests of the branch, or the button to open one
        let branch_prs = self.current_branch_prs();
        let (mut open_pr, mut create_pr) = (None, false);
        let main_branch = self.git.status.as_ref().is_some_and(|s| {
            let b = s.branch.as_str();
            let default = self.repo.as_ref().and_then(|r| self.gh.base_default(r));
            Some(b) == default.as_deref() || b == "master" || b == "main" || b.trim_start_matches("saas-").split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        });

        // ⇱ / ⇲: the view moves between the editor area and the bottom panel
        let (big, mut move_to) = (self.has_tab(document::View::Git), None);
        let (mut stash_act, mut stash_open) = (None, false);
        // --- branch, its upstream and the remote actions
        let mut cmds = Vec::new();
        let mut bar_act: Option<GitBarAct> = None;
        let remotes = self.git.remotes.clone();
        let repo_dir = self.history.repo.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // the repository first and large (easy to mistake in a project with several)
            let labels: Vec<String> = repo_choices.iter().map(|(_, l)| l.clone()).collect();
            let current = repo_choices.iter().position(|(r, _)| self.history.repo.as_ref() == Some(r));
            if let Some(k) = crate::view_bar::repo_chip(ui, "history_repo", &repo_name, &labels, current) {
                chosen_repo = Some(repo_choices[k].0.clone());
            }
            if let Some(st) = &self.git.status {
                // the branch and its menu are one: switch, or create a new one
                ui.menu_button(RichText::new(format!("🔀 {} ▾", st.branch)).strong().size(15.0), |ui| {
                    if ui.button(t!("Changer de branche…", "Switch branch…")).clicked() {
                        cmds.push(Cmd::GitCheckout);
                        ui.close_menu();
                    }
                    if ui.button(t!("+ Nouvelle branche…", "+ New branch…")).clicked() {
                        cmds.push(Cmd::GitNewBranch);
                        ui.close_menu();
                    }
                    ui.separator();
                    let b = &st.branch;
                    for (label, cmd) in [
                        (tf!("Fusionner une branche dans « {b} »…", "Merge a branch into “{b}”…"), Cmd::GitMerge),
                        (tf!("Rebaser « {b} » sur une branche…", "Rebase “{b}” onto a branch…"), Cmd::GitRebaseOnto),
                        (t!("Renommer une branche…", "Rename a branch…").to_string(), Cmd::GitRenameBranch),
                        (t!("Supprimer une branche…", "Delete a branch…").to_string(), Cmd::GitDeleteBranch),
                        (t!("Modifier le dernier commit…", "Amend the last commit…").to_string(), Cmd::GitAmend),
                    ] {
                        if ui.button(label).clicked() {
                            cmds.push(cmd);
                            ui.close_menu();
                        }
                    }
                })
                .response
                .on_hover_text(t!("Changer de branche ou en créer une", "Switch branch or create one"));
                match st.upstream() {
                    Some((up, ahead, behind)) => {
                        if ahead > 0 {
                            ui.label(RichText::new(format!("↑{ahead}")).color(ok()).strong())
                                .on_hover_text(tf!("{ahead} commit(s) à pousser vers {up} (Push)", "{ahead} commit(s) to push to {up} (Push)"));
                        }
                        if behind > 0 {
                            ui.label(RichText::new(format!("↓{behind}")).color(warn()).strong())
                                .on_hover_text(tf!("{behind} commit(s) à récupérer de {up} (Pull)", "{behind} commit(s) to pull from {up} (Pull)"));
                        }
                        if ahead == 0 && behind == 0 {
                            ui.label(RichText::new(t!("✔ à jour", "✔ up to date")).color(ok()).small()).on_hover_text(tf!("Identique à {up}", "Same as {up}"));
                        }
                    }
                    None => {
                        ui.label(RichText::new(t!("pas de branche distante", "no remote branch")).weak().small());
                    }
                }
            }
            for pr in branch_prs.iter().take(3) {
                let (state, color) = super::pulls::state_badge(&pr.state, pr.is_draft);
                let text = RichText::new(format!("⇄ #{} {}", pr.number, state.to_lowercase())).color(color);
                if ui.add(egui::Button::new(text).small()).on_hover_text(tf!("{}\n{} → {}\nClic : ouvrir la pull request", "{}\n{} → {}\nClick: open the pull request", pr.title, pr.head_ref_name, pr.base_ref_name)).clicked() {
                    open_pr = Some(pr.number);
                }
            }
            if !main_branch && !branch_prs.iter().any(|p| p.state == "OPEN") && ui.small_button(t!("+ Créer une PR", "+ Create a PR")).on_hover_text(t!("Pousser la branche et ouvrir une pull request (choix de la branche cible)", "Push the branch and open a pull request (choice of the target branch)")).clicked() {
                create_pr = true;
            }
            if self.git.busy > 0 {
                spinner(ui);
            }
            ui.add_space(8.0);
            ui.menu_button(t!("↕ Synchroniser ▾", "↕ Sync ▾"), |ui| {
                for (label, cmd, tip) in [
                    ("⬇ Pull", Cmd::GitPull, t!("Récupérer et intégrer les commits distants (merge s'il le faut)", "Fetch and merge the remote commits (merge when needed)")),
                    ("⬇ Pull --rebase", Cmd::GitPullRebase, t!("Récupérer les commits distants et rejouer les tiens par-dessus : pas de commit de merge (tes changements non commités sont mis de côté le temps du pull)", "Fetch the remote commits and replay yours on top: no merge commit (your uncommitted changes are put aside during the pull)")),
                    ("⬆ Push", Cmd::GitPush, t!("Envoyer tes commits", "Send your commits")),
                    ("⬆ Push --force-with-lease…", Cmd::GitForcePush, t!("Après un rebase ou un amend : remplacer la branche distante (refusé si quelqu'un y a poussé entre-temps)", "After a rebase or an amend: replace the remote branch (refused if someone pushed to it meanwhile)")),
                    (t!("⬆ Pousser les tags", "⬆ Push the tags"), Cmd::GitPushTags, t!("git push --tags", "git push --tags")),
                    ("Fetch", Cmd::GitFetch, t!("Récupérer sans intégrer", "Fetch without merging")),
                ] {
                    if ui.button(label).on_hover_text(tip).clicked() {
                        cmds.push(cmd);
                        ui.close_menu();
                    }
                }
            })
            .response
            .on_hover_text(t!("Pull, Push, Fetch : récupérer et envoyer les commits", "Pull, Push, Fetch: get and send the commits"));
            // the graph: repository, filter; the rest under ⋯
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let changes = self.git.status.as_ref().is_some_and(|s| !s.entries.is_empty());
                let (git, all) = (&mut self.git, &mut self.history.all);
                let mut stash_shown = false;
                let mut more = |ui: &mut egui::Ui| {
                    let n = git.stashes.len();
                    let label = if n > 0 { format!("Stash ({n})") } else { "Stash".into() };
                    ui.menu_button(label, |ui| {
                        stash_shown = true;
                        stash_menu(ui, git, changes, &mut stash_act);
                    })
                    .response
                    .on_hover_text(t!("Mettre des changements de côté (git stash) et les reprendre plus tard", "Put changes aside (git stash) and take them back later"));
                    if ui.button(t!("Pull requests GitHub", "GitHub pull requests")).on_hover_text(t!("Commentaires, suggestions, commits", "Comments, suggestions, commits")).clicked() {
                        cmds.push(Cmd::PullRequests);
                        ui.close_menu();
                    }
                    ui.menu_button(t!("🔍 Rechercher dans l'historique", "🔍 Search the history"), |ui| {
                        for k in git::SearchKind::ALL {
                            if ui.button(format!("{}…", k.label())).clicked() {
                                bar_act = Some(GitBarAct::Search(k));
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.button(t!("Reflog : retrouver un commit perdu…", "Reflog: find a lost commit…")).on_hover_text(t!("Où HEAD est passée : commits, checkouts, resets, rebases. Un commit perdu (reset, rebase, branche supprimée) y est encore.", "Where HEAD has been: commits, checkouts, resets, rebases. A lost commit (reset, rebase, deleted branch) is still there.")).clicked() {
                        cmds.push(Cmd::GitReflog);
                        ui.close_menu();
                    }
                    ui.menu_button(tf!("Remotes ({})", "Remotes ({})", remotes.len()), |ui| {
                        for (name, url) in &remotes {
                            ui.horizontal(|ui| {
                                if ui.small_button("×").on_hover_text(t!("Retirer ce remote (rien n'est supprimé sur le serveur)", "Remove this remote (nothing is deleted on the server)")).clicked() {
                                    bar_act = Some(GitBarAct::RemoveRemote(name.clone()));
                                    ui.close_menu();
                                }
                                ui.label(RichText::new(name).strong());
                                ui.label(RichText::new(url).weak().small());
                            });
                        }
                        if ui.button(t!("+ Ajouter un remote…", "+ Add a remote…")).clicked() {
                            bar_act = Some(GitBarAct::AddRemote);
                            ui.close_menu();
                        }
                    });
                    ui.menu_button(t!("Sous-modules", "Submodules"), |ui| {
                        // read when the menu is open only
                        let submodules = repo_dir.as_deref().map(git::submodules).unwrap_or_default();
                        for p in &submodules {
                            ui.label(RichText::new(format!("📁 {p}")).small());
                        }
                        if !submodules.is_empty() {
                            ui.separator();
                            if ui.button(t!("Initialiser et mettre à jour (update --init --recursive)", "Init and update (update --init --recursive)")).clicked() {
                                bar_act = Some(GitBarAct::Submodules(vec!["submodule".into(), "update".into(), "--init".into(), "--recursive".into()]));
                                ui.close_menu();
                            }
                            if ui.button(t!("Suivre leur branche distante (update --remote)", "Follow their remote branch (update --remote)")).clicked() {
                                bar_act = Some(GitBarAct::Submodules(vec!["submodule".into(), "update".into(), "--remote".into(), "--recursive".into()]));
                                ui.close_menu();
                            }
                            if ui.button(t!("Reprendre les URL de .gitmodules (sync)", "Take the URLs of .gitmodules again (sync)")).clicked() {
                                bar_act = Some(GitBarAct::Submodules(vec!["submodule".into(), "sync".into(), "--recursive".into()]));
                                ui.close_menu();
                            }
                        }
                        if ui.button(t!("+ Ajouter un sous-module…", "+ Add a submodule…")).clicked() {
                            bar_act = Some(GitBarAct::AddSubmodule);
                            ui.close_menu();
                        }
                    });
                    if ui.button(t!("Supprimer les fichiers non suivis…", "Delete the untracked files…")).on_hover_text(t!("git clean : la liste est montrée avant", "git clean: the list is shown first")).clicked() {
                        cmds.push(Cmd::GitClean);
                        ui.close_menu();
                    }
                    if ui.button(t!("✨ Consignes des messages de commit (Claude)…", "✨ Commit message rules (Claude)…")).on_hover_text(t!("Ce que Claude doit suivre en écrivant les messages de ce projet (langue, préfixes…) : .florm/commit.md", "What Claude must follow when writing the messages of this project (language, prefixes…): .florm/commit.md")).clicked() {
                        cmds.push(Cmd::GitCommitRules);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.checkbox(all, t!("Toutes les branches dans le graphe", "All branches in the graph")).changed() {
                        reload = true;
                    }
                };
                match crate::view_bar::controls(ui, big, Some(t!("Rafraîchir l'état et le graphe", "Refresh the status and the graph")), Some(&mut more)) {
                    Some(crate::view_bar::BarAct::Move(b)) => move_to = Some(b),
                    Some(crate::view_bar::BarAct::Refresh) => reload = true,
                    None => {}
                }
                // the stashes read when their menu opens
                if stash_shown && !self.git.stash_shown {
                    stash_open = true;
                }
                self.git.stash_shown = stash_shown;
                let h = &mut self.history;
                if h.loading {
                    spinner(ui);
                }
                ui.add(egui::TextEdit::singleline(&mut h.filter).hint_text(t!("🔍 Filtrer : message, auteur, hash, branche…", "🔍 Filter: message, author, hash, branch…")).desired_width(220.0))
                    .on_hover_text(t!("↑ ↓ commits · ← → fichiers du diff · Page ↑ ↓ défiler", "↑ ↓ commits · ← → files of the diff · Page ↑ ↓ scroll"));
            });
        });
        self.pending.extend(cmds);

        if stash_open {
            self.stash_reload();
        }
        if let Some(a) = stash_act {
            self.stash_act(a);
        }
        match move_to {
            Some(true) => self.show_big(BottomTab::Git),
            Some(false) => self.show_bottom(BottomTab::Git, false),
            None => {}
        }
        let mut cmds_after = Vec::new();
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
                        ui.label(RichText::new(tf!("⚠ git {label} a échoué", "⚠ git {label} failed")).color(err()).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text(t!("Masquer", "Hide")).clicked() {
                                dismiss_error = true;
                            }
                            // a push refused because the branch was rewritten (rebase, amend)
                            let refused = label.starts_with("push") && !label.contains("force") && ["non-fast-forward", "[rejected]", "fetch first"].iter().any(|k| e.contains(k));
                            if refused && ui.small_button(t!("⬆ Forcer (--force-with-lease)…", "⬆ Force (--force-with-lease)…")).on_hover_text(t!("Si tu as réécrit la branche (rebase, amend) : remplacer la branche distante. Sinon, fais plutôt un pull.", "If you rewrote the branch (rebase, amend): replace the remote branch. Otherwise, pull instead.")).clicked() {
                                cmds_after.push(Cmd::GitForcePush);
                            }
                            if ui.small_button(t!("Copier", "Copy")).clicked() {
                                ui.ctx().output_mut(|o| o.copied_text = e.clone());
                            }
                        });
                    });
                    egui::ScrollArea::vertical().id_salt("git_err").max_height(110.0).show(ui, |ui| {
                        ui.label(RichText::new(&e).monospace().small());
                    });
                });
        }
        ui.add_space(4.0);

        if dismiss_error {
            self.git.op_error = None;
            self.git.error = None;
        }
        self.pending.extend(cmds_after);
        match bar_act {
            Some(GitBarAct::Search(k)) => self.open_prompt(Prompt::HistorySearch(k)),
            Some(GitBarAct::AddRemote) => self.open_prompt(Prompt::AddRemote),
            Some(GitBarAct::AddSubmodule) => self.open_prompt(Prompt::AddSubmodule),
            Some(GitBarAct::RemoveRemote(name)) => self.confirm_git(
                t!("Retirer le remote", "Remove the remote"),
                tf!("Retirer le remote « {name} » de ce dépôt ?\nSes branches distantes disparaissent du graphe ; rien n'est supprimé sur le serveur.", "Remove the remote “{name}” from this repository?\nIts remote branches leave the graph; nothing is deleted on the server."),
                t!("Retirer", "Remove"),
                "remote remove",
                vec![vec!["remote".into(), "remove".into(), name]],
                None,
            ),
            Some(GitBarAct::Submodules(args)) => {
                if let Some(repo) = self.repo.clone() {
                    self.git_seq_at(repo, "submodule", vec![args]);
                }
            }
            None => {}
        }
        if let Some(r) = chosen_repo {
            self.select_repo(r.clone());
            // the panel may already be on it while the view still shows another one
            if self.history.repo.as_ref() != Some(&r) {
                self.load_history();
            }
        }
        if reload {
            self.git_refresh();
            self.load_history();
            self.gh_forget_branches();
        }
        if let (Some(n), Some(repo)) = (open_pr, self.repo.clone()) {
            self.open_pr(repo, n);
        }
        if create_pr {
            self.start_pr_create();
        }
    }

    /// Right click on a view (button of the sidebar, tab of the bottom panel): in the bottom panel,
    /// taken out of the editor area if it was there; `toggle`: a click on the one shown hides it.
    pub(super) fn show_bottom(&mut self, tab: BottomTab, toggle: bool) {
        if toggle && self.bottom == Some(tab) {
            self.bottom = None;
            return;
        }
        let in_editor = |v: document::View| move |d: &Document| d.view == v;
        let tab_of = match tab {
            BottomTab::Search => self.docs.iter().position(in_editor(document::View::Search)),
            // each Claude tab is a conversation of its own: it stays
            BottomTab::Pulls => self.docs.iter().position(in_editor(document::View::PullRequests)),
            BottomTab::Docker => self.docs.iter().position(in_editor(document::View::Docker)),
            BottomTab::Debug => self.docs.iter().position(in_editor(document::View::Debug)),
            BottomTab::Git => self.docs.iter().position(in_editor(document::View::Git)),
            _ => None,
        };
        if let Some(i) = tab_of {
            self.close_tab(i, true);
        }
        self.bottom = Some(tab);
        match tab {
            BottomTab::Search => self.search.focus = true,
            BottomTab::Terminal => self.term.want_focus = true,
            BottomTab::Claude => self.claude_prepare(),
            BottomTab::Git => self.git_view_load(),
            BottomTab::Pulls => self.pulls_load(),
            BottomTab::Docker => self.docker_load(),
            BottomTab::Debug => self.debug.focus_console = true,
            BottomTab::Problems | BottomTab::Output => {}
        }
    }

    /// Left click on a view: in the editor area, with all the room (problems and output only
    /// exist in the bottom panel).
    pub(super) fn show_big(&mut self, tab: BottomTab) {
        match tab {
            BottomTab::Search => self.open_search_tab(),
            BottomTab::Terminal => self.open_terminal_tab(),
            BottomTab::Claude => {
                if self.bottom == Some(BottomTab::Claude) {
                    self.bottom = None;
                }
                self.move_claude(true);
            }
            BottomTab::Git => self.open_git_view(),
            BottomTab::Pulls => self.open_pulls(),
            BottomTab::Docker => self.open_docker(),
            BottomTab::Debug => self.open_debug(),
            BottomTab::Problems | BottomTab::Output => self.show_bottom(tab, false),
        }
    }

    /// The view of `tab` is shown in an editor tab (the bottom panel then only says so).
    fn view_in_tab(&self, tab: BottomTab) -> bool {
        let tab_of = |v: document::View| self.docs.iter().any(|d| d.view == v);
        match tab {
            BottomTab::Search => tab_of(document::View::Search),
            BottomTab::Claude => !self.claude_has_bottom(),
            BottomTab::Git => tab_of(document::View::Git),
            BottomTab::Pulls => tab_of(document::View::PullRequests),
            BottomTab::Docker => tab_of(document::View::Docker),
            BottomTab::Debug => tab_of(document::View::Debug),
            BottomTab::Terminal | BottomTab::Problems | BottomTab::Output => false,
        }
    }

    /// The view of `tab` back from its editor tab into the bottom panel.
    fn bring_back(&mut self, tab: BottomTab) {
        match tab {
            BottomTab::Search => {
                if let Some(i) = self.docs.iter().position(|d| d.view == document::View::Search) {
                    self.close_tab(i, true);
                }
                self.settings.search_in_tab = false;
                self.persist("search_in_tab", "false".into());
            }
            BottomTab::Claude => self.move_claude(false),
            _ => self.show_bottom(tab, false),
        }
    }

    pub(super) fn bottom_panel(&mut self, ctx: &egui::Context) {
        if let Some(t) = self.bottom {
            self.last_bottom = t;
        }
        // Claude, git and the pull requests need room for their bars and content
        let min_h = if matches!(self.bottom, Some(BottomTab::Claude | BottomTab::Git | BottomTab::Pulls | BottomTab::Docker | BottomTab::Debug)) { 360.0 } else { 80.0 };
        egui::TopBottomPanel::bottom("bottom").resizable(true).default_height(230.0).height_range(min_h..=900.0).show(
            ctx,
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let nd: usize = self.docs.iter().map(|d| d.diagnostics.len()).sum();
                    let problems = if nd > 0 { tf!("Problèmes {nd}", "Problems {nd}") } else { t!("Problèmes", "Problems").to_string() };
                    for (tab, icon, label) in BottomTab::shown() {
                        let label = if tab == BottomTab::Problems { problems.clone() } else { label.to_string() };
                        let r = nav_tab(ui, icon, &label, self.bottom == Some(tab));
                        if tab == BottomTab::Claude && self.bottom != Some(tab) && self.claude_bottom_done() {
                            ui.painter().rect_stroke(r.rect, 4.0, egui::Stroke::new(1.5, ok()));
                        }
                        if r.clicked() && !tab.bottom_only() {
                            self.show_big(tab);
                        } else if r.clicked() || r.secondary_clicked() {
                            self.show_bottom(tab, false);
                        }
                        r.on_hover_text(BottomTab::hint(tab));
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
                    // shown in an editor tab: not twice, the same notice for all
                    Some(tab) if self.view_in_tab(tab) => {
                        if crate::view_bar::in_tab_notice(ui) {
                            self.bring_back(tab);
                        }
                    }
                    Some(BottomTab::Claude) => self.claude_show(ui, None),
                    Some(BottomTab::Git) => {
                        let fill = ui.visuals().panel_fill;
                        egui::TopBottomPanel::top("git_bar_bottom")
                            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 4.0, right: 4.0, top: 2.0, bottom: 4.0 }))
                            .show_inside(ui, |ui| self.git_bar(ui));
                        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.history_ui(ui));
                    }
                    Some(BottomTab::Pulls) => {
                        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.pulls_ui(ui));
                    }
                    Some(BottomTab::Docker) => {
                        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.docker_ui(ui));
                    }
                    Some(BottomTab::Debug) => {
                        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.debug_ui(ui));
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
                        if ui.small_button(t!("Effacer", "Clear")).clicked() {
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
                        RichText::new(t!("Python : aucun linter installé, seules les erreurs de syntaxe sont détectées.", "Python: no linter installed, only syntax errors are detected.")).color(warn()),
                    );
                    if ui.button(t!("Installer flake8", "Install flake8")).on_hover_text(format!("pip install --target {}", lint::tools_dir().display())).clicked() {
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
                ui.label(RichText::new(t!("Aucun problème détecté (le lint Python/JS s'exécute à l'enregistrement).", "No problem detected (Python/JS lint runs on save).")).weak());
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

    /// Shows `path` in the file tree: sidebar opened, folders expanded, entry scrolled into view.
    pub(super) fn reveal_in_tree(&mut self, path: &Path) {
        let Some(tree) = self.tree.as_mut() else {
            self.set_status(t!("Aucun dossier ouvert", "No folder open"));
            return;
        };
        let path = crate::os::canon(path);
        if !tree.reveal(&path) {
            self.set_status(t!("Ce fichier n'est pas dans le dossier ouvert", "This file is not in the open folder"));
            return;
        }
        self.show_sidebar = true;
        self.tree_nav.cursor = Some(path.clone());
        self.tree_scroll = Some(path);
    }

    pub(super) fn reveal(&mut self, path: &Path) {
        if let Err(e) = crate::os::reveal(path) {
            self.set_error(e);
        }
    }
}

/// The Stash menu: put the changes aside (with a message), then the stashes, newest first, each
/// to look at, apply (it stays), take back (applied then deleted) or delete.
fn stash_menu(ui: &mut egui::Ui, g: &mut GitUi, changes: bool, act: &mut Option<super::git_actions::StashAct>) {
    use super::git_actions::StashAct;
    ui.set_min_width(460.0);
    ui.label(RichText::new(t!("Mettre de côté", "Put aside")).strong());
    ui.add(egui::TextEdit::singleline(&mut g.stash_msg).hint_text(t!("Message (facultatif)", "Message (optional)")).desired_width(f32::INFINITY));
    ui.horizontal(|ui| {
        let mut untracked = !g.stash_tracked_only;
        if ui.checkbox(&mut untracked, t!("Nouveaux fichiers compris", "New files included")).on_hover_text(t!("Les fichiers pas encore suivis par git partent aussi (--include-untracked)", "Files not tracked by git yet go too (--include-untracked)")).changed() {
            g.stash_tracked_only = !untracked;
        }
        let tip = t!("Range les changements non commités (indexés ou non) et remet les fichiers comme au dernier commit", "Stores the uncommitted changes (staged or not) and puts the files back as at the last commit");
        if ui.add_enabled(changes, egui::Button::new(t!("Mettre de côté", "Put aside"))).on_hover_text(tip).on_disabled_hover_text(t!("Aucun changement à mettre de côté", "No change to put aside")).clicked() {
            *act = Some(StashAct::Push);
            ui.close_menu();
        }
    });
    ui.separator();
    if g.stashes.is_empty() {
        ui.label(RichText::new(t!("Aucun stash dans ce dépôt.", "No stash in this repository.")).weak());
        return;
    }
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        for s in &g.stashes {
            // "On main: message" → "message", the branch apart
            let (branch, what) = s.message.split_once(": ").unwrap_or(("", s.message.as_str()));
            let branch = branch.trim_start_matches("WIP ").trim_start_matches("On ");
            ui.horizontal(|ui| {
                ui.label(RichText::new(&s.name).monospace().small().weak());
                ui.label(RichText::new(what).strong()).on_hover_text(&s.message);
                ui.label(RichText::new(format!("{branch} · {}", git::ago(s.time))).small().weak());
            });
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                if ui.small_button(t!("Voir", "Show")).on_hover_text(t!("Ses changements dans la vue des commits (sans les nouveaux fichiers)", "Its changes in the commits view (without the new files)")).clicked() {
                    *act = Some(StashAct::Show(s.hash.clone(), what.to_string()));
                    ui.close_menu();
                }
                if ui.small_button(t!("Appliquer", "Apply")).on_hover_text(t!("Remet ses changements dans tes fichiers ; le stash reste (git stash apply)", "Puts its changes back in your files; the stash stays (git stash apply)")).clicked() {
                    *act = Some(StashAct::Apply(s.hash.clone()));
                    ui.close_menu();
                }
                if ui.small_button(t!("Reprendre", "Pop")).on_hover_text(t!("Remet ses changements et supprime le stash (git stash pop)", "Puts its changes back and deletes the stash (git stash pop)")).clicked() {
                    *act = Some(StashAct::Pop(s.hash.clone()));
                    ui.close_menu();
                }
                if ui.small_button(RichText::new(t!("Supprimer", "Delete")).color(err())).on_hover_text(t!("git stash drop (demande confirmation)", "git stash drop (asks for confirmation)")).clicked() {
                    *act = Some(StashAct::AskDrop(s.hash.clone(), what.to_string()));
                    ui.close_menu();
                }
            });
            ui.add_space(4.0);
        }
    });
}

/// What the ⋯ menu of the git view asked for, beyond a command.
enum GitBarAct {
    Search(git::SearchKind),
    AddRemote,
    RemoveRemote(String),
    AddSubmodule,
    /// `git submodule …` with these arguments.
    Submodules(Vec<String>),
}

/// The sidebar a little darker than the rest, to set the tree apart from the editor.
fn sidebar_fill(v: &egui::Visuals) -> Color32 {
    icons::mix(v.panel_fill, Color32::BLACK, if v.dark_mode { 0.15 } else { 0.04 })
}
