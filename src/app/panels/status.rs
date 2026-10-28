//! The status bar (message, position, branch, problems, update) and the find bar of the editor.

use super::*;

impl App {
    /// The update, at the bottom right of the status bar (next to the version): its menu
    /// installs or downloads it. Once installed, Florm restarts by itself (see `update`).
    pub(super) fn update_symbol(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

    pub(in crate::app) fn status_bar(&mut self, ctx: &egui::Context) {
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
    pub(super) fn lsp_status_line(&self) -> Option<(String, bool, bool)> {
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

    pub(in crate::app) fn find_bar(&mut self, ctx: &egui::Context) {
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
                let te = crate::input::add(ui,
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
                    let te = crate::input::add(ui, replace_field(&mut self.find.replacement, t!("Remplacer par ($1 pour les groupes en mode regex)", "Replace with ($1 for the groups in regex mode)")).desired_width(w)).on_hover_text(replace_tip());
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
}
