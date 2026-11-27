//! The status bar (the bottom panel's button, position, message; then what is going on: branch,
//! Claude, problems; update) and the find bar of the editor.

use super::*;

impl App {
    /// Beside the F at the top left: the version of Florm (a click looks for an update), and the
    /// update when there is one, a round button of its icon alone in the accent color: its card
    /// says what is new and installs it. Once installed, Florm restarts by itself (see `update`).
    pub(in crate::app) fn version_and_update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let accent = self.hl.colors.accent;
        let muted = self.hl.tab_colors().text_inactive;
        let version = egui::Button::new(RichText::new(format!("v{}", crate::update::current())).size(11.5).color(muted)).frame(false);
        if ui.add(version).on_hover_text(t!("Version de Florm · clic : rechercher une mise à jour", "Florm version · click: check for an update")).clicked() {
            self.pending.push(Cmd::CheckUpdate);
        }
        // a round green badge, its icon alone (the room of the bar is for the project): it stands out
        let badge = || egui::Button::new("").frame(false).min_size(egui::vec2(20.0, 20.0));
        let paint = |ui: &egui::Ui, r: &egui::Response, ready: bool| {
            icons::paint_upgrade(ui.painter(), egui::Rect::from_center_size(r.rect.center(), egui::vec2(16.0, 16.0)), ready, r.hovered());
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        };
        if let (Some(u), true) = (&self.upd.found, self.upd.installing) {
            ui.add(egui::Spinner::new().size(14.0)).on_hover_text(tf!("Mise à jour vers Florm {}…", "Updating to Florm {}…", u.version));
        } else if let (Some(u), true) = (&self.upd.found, self.upd.ready.is_some()) {
            let tip = tf!("Florm {} est installée : clic pour redémarrer (fichiers non enregistrés : la question habituelle)", "Florm {} is installed: click to restart (unsaved files: the usual question)", u.version);
            let r = ui.add(badge());
            paint(ui, &r, true);
            if r.on_hover_text(tip).clicked() {
                self.restart_for_update(&ctx);
            }
        } else if let Some(u) = self.upd.found.clone() {
            ui.scope(|ui| {
                ui.spacing_mut().button_padding = egui::vec2(0.0, 0.0);
                let tip = tf!("Florm {} est disponible : clic pour voir ce qui change et l'installer", "Florm {} is available: click to see what changes and install it", u.version);
                let r = egui::containers::menu::MenuButton::from_button(badge()).ui(ui, |ui| self.update_card(ui, &u, accent)).0;
                paint(ui, &r, false);
                r.on_hover_text(tip);
            });
        }
    }

    /// The card of an update: what it is, the button to install it, what is new, the other ways.
    fn update_card(&mut self, ui: &mut egui::Ui, u: &crate::update::Update, accent: Color32) {
        ui.visuals_mut().override_text_color = None;
        ui.set_width(400.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.add_space(2.0);
        ui.label(RichText::new(tf!("Florm {} est disponible", "Florm {} is available", u.version)).size(16.0).strong());
        ui.label(RichText::new(tf!("Vous avez la {}. Vos préférences et vos fichiers sont gardés.", "You have {}. Your preferences and files are kept.", crate::update::current())).weak());
        ui.add_space(4.0);
        let install = filled_button(t!("Installer et redémarrer", "Install and restart"), accent).min_size(egui::vec2(0.0, 28.0));
        if ui.add(install).on_hover_text(t!("Florm télécharge la nouvelle version, la vérifie et remplace celle-ci. Sous Linux, un paquet système demande le mot de passe.", "Florm downloads the new version, checks it and replaces this one. On Linux, a system package asks for the password.")).clicked() {
            self.install_update();
            ui.close();
        }
        if !u.news.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(t!("Nouveautés", "What's new")).strong());
            egui::ScrollArea::vertical().max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
                for line in &u.news {
                    let (tag, text) = news_tag(line);
                    ui.horizontal_top(|ui| {
                        // a column of its own for the badges: the texts start at the same place
                        ui.allocate_ui_with_layout(egui::vec2(74.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.set_width(74.0);
                            if let Some((label, color)) = tag {
                                egui::Frame::NONE.fill(color.gamma_multiply(0.18)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(5, 1)).show(ui, |ui| {
                                    ui.label(RichText::new(label).size(11.0).color(color).strong());
                                });
                            }
                        });
                        ui.add(egui::Label::new(text).wrap());
                    });
                }
            });
        }
        ui.add_space(4.0);
        ui.separator();
        // the other ways, quieter: in the browser, or later
        ui.horizontal_wrapped(|ui| {
            let link = |ui: &mut egui::Ui, text: &str| ui.add(egui::Button::new(RichText::new(text).small()).frame(false));
            if link(ui, t!("Notes et fichiers ↗", "Notes and files ↗")).on_hover_text(t!("La page de la version sur GitHub", "The page of the version on GitHub")).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(&u.page));
                ui.close();
            }
            if let Some((name, url)) = &u.file {
                let tip = tf!("{name}, dans le navigateur ; installez-le comme la première fois : il remplace l'ancienne version, les préférences sont gardées.", "{name}, in the browser; install it like the first time: it replaces the old version, the preferences are kept.");
                if link(ui, t!("Télécharger le fichier ↗", "Download the file ↗")).on_hover_text(tip).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    ui.close();
                }
            }
            if link(ui, t!("Plus tard", "Later")).on_hover_text(t!("Masquée jusqu'au prochain démarrage", "Hidden until the next start")).clicked() {
                self.upd.found = None;
                ui.close();
            }
        });
    }

    pub(in crate::app) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        let (mut toggle_spaces, mut convert_utf8) = (false, false);
        // as in Zed: the bars a step beyond the panels, a line above
        let s = self.hl.surfaces();
        egui::Panel::bottom("status")
            .exact_size(28.0)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(s.bar).inner_margin(egui::Margin::symmetric(8, 0)))
            .show(ui, |ui| {
            let edge = ui.max_rect();
            ui.painter().hline(edge.x_range(), edge.top() + 0.5, egui::Stroke::new(1.0_f32, s.border));
            ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(12.5));
            ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(12.5));
            // Zed's muted text: readable, a step below the text of the editor
            let muted = crate::icons::mix(ui.visuals().text_color(), s.bar, 0.22);
            ui.visuals_mut().override_text_color = Some(muted);
            ui.horizontal_centered(|ui| {
                // only for a text (not on the welcome page, a diff, the preferences…)
                if let Some(d) = self.docs.get(self.active).filter(|d| d.view == crate::document::View::Text) {
                    let p = d.primary_sel();
                    let (l, c) = d.line_col(p.head);
                    if self.settings.vim_mode {
                        let v = &self.vim;
                        ui.label(RichText::new(format!("-- {} --", v.mode.label())).strong().color(self.hl.colors.accent));
                        if !v.pending.is_empty() {
                            ui.label(RichText::new(&v.pending).monospace());
                        }
                    }
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
                    // at the right end, the views of the bottom panel as tabs of their icon:
                    // Terminal, Débogueur, Problèmes, Sortie (a click shows it, again hides it)
                    self.bottom_tabs(ui);
                    // everything else on the left, after the position and the message: the branch,
                    // Claude, the file's details
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        if let Some(st) = &self.git.status {
                            let mut label = match (&self.repo, self.repos.len() > 1) {
                                (Some(r), true) => format!("🔀 {} · {}", self.repo_label(r), st.branch),
                                _ => format!("🔀 {}", st.branch),
                            };
                            // commits to push / to pull, and the changed files
                            if let Some((_, ahead, behind)) = st.upstream() {
                                if ahead > 0 {
                                    label.push_str(&format!(" ↑{ahead}"));
                                }
                                if behind > 0 {
                                    label.push_str(&format!(" ↓{behind}"));
                                }
                            }
                            if !st.entries.is_empty() {
                                label.push_str(&format!(" ●{}", st.entries.len()));
                            }
                            // the branch, what it tracks and the remotes (what the top of the sidebar showed)
                            let legend = t!("↑ à pousser, ↓ à récupérer, ● fichiers modifiés · clic : Git", "↑ to push, ↓ to pull, ● changed files · click: Git");
                            let tip = match self.repo.as_ref().and_then(|r| self.repo_info.get(r)) {
                                Some((_, t)) => format!("{t}\n\n{legend}"),
                                None => legend.to_string(),
                            };
                            if ui.add(egui::Button::new(label).frame(false)).on_hover_text(tip).clicked() {
                                self.pending.push(Cmd::ShowGit);
                            }
                        }
                        // the rest of the file's details and the language server in a menu
                        let mut pending = Vec::new();
                        let lsp_line = self.lsp_status_line();
                        ui.menu_button("⋯", |ui| {
                            ui.visuals_mut().override_text_color = None;
                            if let Some(d) = self.docs.get(self.active) {
                                if ui.button(tf!("Syntaxe : {}…", "Syntax: {}…", d.syntax)).clicked() {
                                    pending.push(Cmd::SetSyntax);
                                    ui.close();
                                }
                                let ind = if d.use_spaces { tf!("Espaces : {}", "Spaces: {}", d.tab_size) } else { tf!("Tabulations : {}", "Tabs: {}", d.tab_size) };
                                if ui.button(tf!("Indentation : {ind}", "Indentation: {ind}")).on_hover_text(t!("Clic : passer aux espaces / aux tabulations", "Click: switch to spaces / tabs")).clicked() {
                                    toggle_spaces = true;
                                    ui.close();
                                }
                                ui.label(tf!("Fin de ligne : {}", "Line ending: {}", if d.crlf { "CRLF (Windows)" } else { "LF" }));
                                ui.label(tf!("Encodage : {}", "Encoding: {}", d.encoding.label()));
                            }
                            if let Some((text, warning, install)) = &lsp_line {
                                ui.separator();
                                let t = if *warning { RichText::new(text).color(warn()) } else { RichText::new(text) };
                                if let Some(cmd) = install {
                                    let tip = if *cmd == Cmd::InstallPylsp {
                                        t!("Installe python-lsp-server pour Florm (pip --target, sans toucher au Python système). Ou installe pyright.", "Installs python-lsp-server for Florm (pip --target, without touching the system Python). Or install pyright.")
                                    } else {
                                        t!("Préférences → Langages : les outils de ce langage, à installer", "Preferences → Languages: the tools of this language, to install")
                                    };
                                    if ui.button(t).on_hover_text(tip).clicked() {
                                        pending.push(*cmd);
                                        ui.close();
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
                        // what is going on, only while it is: Claude
                        let weak = muted;
                        // the conversations: finished and not seen yet, waiting for you, working
                        let (busy, waiting, done) = self.claude_states();
                        if let Some((id, lit)) = done {
                            let col = if lit { ok() } else { weak };
                            if state_chip(ui, icons::ViewIcon::Claude, "✔", col).on_hover_text(t!("Claude a fini de répondre · clic : voir la réponse", "Claude has finished answering · click: see the answer")).clicked() {
                                self.claude_go_to(id);
                            }
                        }
                        if !waiting.is_empty() {
                            let n = waiting.len();
                            let text = if n > 1 { tf!("❓ {n} attendent", "❓ {n} waiting") } else { t!("❓ attend", "❓ waiting").to_string() };
                            // what it waits for, as Claude Code says it (a permission, a question)
                            let what = self.claude_waiting_for().unwrap_or_else(|| t!("Claude attend ta réponse à ses questions", "Claude waits for your answer to its questions").to_string());
                            let tip = tf!("{what} · clic : y aller (Ctrl+Alt+W : la suivante)", "{what} · click: go there (Ctrl+Alt+W: the next one)");
                            if state_chip(ui, icons::ViewIcon::Claude, &text, warn()).on_hover_text(tip).clicked() {
                                self.claude_next_waiting();
                            }
                        }
                        if busy > 0 {
                            let text = if busy > 1 { tf!("{busy} au travail", "{busy} working") } else { t!("au travail", "working").to_string() };
                            if state_chip(ui, icons::ViewIcon::Claude, &text, self.hl.colors.accent).on_hover_text(t!("Claude répond", "Claude is answering")).clicked() {
                                self.open_claude();
                            }
                        }
                    });
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
    /// What the language server of the current file does: (text, a warning, the command a click runs).
    pub(super) fn lsp_status_line(&self) -> Option<(String, bool, Option<Cmd>)> {
        let path = self.docs.get(self.active).and_then(|d| d.path.as_deref()).filter(|_| self.settings.lsp)?;
        let fam = lsp::language_of(path, &self.settings.lsp_servers)?.0;
        let running = self.lsp.manager.clients.iter().flatten().find(|c| c.family == fam);
        match (running, self.lsp.notes.get(fam)) {
            (Some(c), _) if c.ready => Some((tf!("Serveur de langage : {}", "Language server: {}", c.name), false, None)),
            (Some(c), _) => Some((tf!("Serveur de langage : {} (démarrage…)", "Language server: {} (starting…)", c.name), false, None)),
            (None, Some(_)) if fam == "python" => Some((t!("Serveur de langage : installer pylsp", "Language server: install pylsp").into(), true, Some(Cmd::InstallPylsp))),
            // a language one does not always want a server for: said, not a warning; its pack
            // (Preferences → Languages) installs it
            (None, Some(why)) => Some((
                tf!("Pas de serveur de langage : {why} — ⬇ Langages…", "No language server: {why} — ⬇ Languages…"),
                lsp::usually_wanted(fam) || self.settings.lsp_servers.contains_key(fam),
                Some(Cmd::ShowLanguages),
            )),
            _ => None,
        }
    }

    pub(in crate::app) fn find_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        let (mut action, mut scope_toggled) = (None, false);
        egui::Panel::bottom("find").show(ui, |ui| {
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

impl App {
    /// The views of the bottom panel as tabs of their icon, at the right end of the status bar
    /// (laid out right to left): Terminal, Débogueur, Problèmes (their number), Sortie.
    fn bottom_tabs(&mut self, ui: &mut egui::Ui) {
        let tc = self.hl.tab_colors();
        let (accent, muted) = (self.hl.colors.accent, tc.text_inactive);
        let errs: usize = self.docs.iter().flat_map(|d| d.diagnostics.iter()).filter(|g| g.severity == Severity::Error).count();
        let warns: usize = self.docs.iter().map(|d| d.diagnostics.len()).sum::<usize>() - errs;
        let debug_color = match self.debug_status() {
            Some(true) => Some(warn()),
            Some(false) => Some(ok()),
            None => None,
        };
        let keys = |c: Cmd| {
            let k = self.keymap.label(c);
            if k.is_empty() { String::new() } else { format!(" ({k})") }
        };
        let problems = if errs + warns > 0 { Some((format!("{}", errs + warns), if errs > 0 { err() } else { warn() })) } else { None };
        let tabs = [
            (BottomTab::Output, icons::ViewIcon::Output, format!("{}{}", t!("Sortie", "Output"), keys(Cmd::ShowOutput)), Cmd::ShowOutput, None, None),
            (BottomTab::Problems, icons::ViewIcon::Problems, tf!("Problèmes : {errs} erreur(s), {warns} avertissement(s){}", "Problems: {errs} error(s), {warns} warning(s){}", keys(Cmd::ShowProblems)), Cmd::ShowProblems, problems.as_ref().map(|p| p.1), problems.as_ref().map(|p| p.0.clone())),
            (BottomTab::Debug, icons::ViewIcon::Debug, format!("{}{}", t!("Débogueur", "Debugger"), keys(Cmd::DebugStart)), Cmd::DebugShow, debug_color, None),
            (BottomTab::Terminal, icons::ViewIcon::Terminal, format!("Terminal{}", keys(Cmd::ToggleTerminal)), Cmd::ToggleTerminal, None, None),
        ];
        ui.spacing_mut().item_spacing.x = 0.0;
        for (k, (tab, icon, tip, cmd, color, note)) in tabs.into_iter().enumerate() {
            let shown = self.bottom == Some(tab);
            let color = color.unwrap_or(if shown { accent } else { muted });
            let note_g = note.map(|n| ui.painter().layout_no_wrap(n, egui::FontId::proportional(11.5), color));
            let w = 34.0 + note_g.as_ref().map_or(0.0, |g| g.size().x + 4.0);
            let (rect, r) = ui.allocate_exact_size(egui::vec2(w, ui.available_height()), Sense::click());
            let rect = egui::Rect::from_min_max(rect.min.round(), rect.max.round());
            let p = ui.painter();
            // the one shown on the panel's background: they join
            let fill = if shown { self.hl.bottom_paper() } else if r.hovered() { tc.close_hover } else { tc.bar };
            p.rect_filled(rect, 0.0, fill);
            let line = egui::Stroke::new(1.0_f32, tc.border);
            // laid out from the right: each one draws its left line, the last one (rightmost) its right
            p.vline(rect.left() + 0.5, rect.y_range(), line);
            if k == 0 {
                p.vline(rect.right() - 0.5, rect.y_range(), line);
            }
            let icon_at = egui::pos2(rect.left() + 17.0, rect.center().y);
            icons::paint_view(p, icon_at, icon, color);
            if let Some(g) = note_g {
                p.galley(egui::pos2(rect.left() + 28.0, rect.center().y - g.size().y / 2.0), g, color);
            }
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if r.on_hover_text(tip).clicked() {
                if shown {
                    self.bottom = None;
                } else {
                    self.pending.push(cmd);
                }
            }
        }
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(6.0);
    }
}

/// A state of the status bar: the icon of its view and a short text, without a frame.
fn state_chip(ui: &mut egui::Ui, icon: icons::ViewIcon, text: &str, color: Color32) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(12.5), color);
    let (rect, r) = ui.allocate_exact_size(egui::vec2(galley.size().x + 26.0, 22.0), Sense::click());
    if r.hovered() {
        ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    icons::paint_view(ui.painter(), egui::pos2(rect.left() + 10.0, rect.center().y), icon, color);
    ui.painter().galley(egui::pos2(rect.left() + 20.0, rect.center().y - galley.size().y / 2.0), galley, color);
    r
}

/// The badge of a news line (`[ADD] Zone : …`: Nouveau, accent) and its text without the tag.
fn news_tag(line: &str) -> (Option<(&'static str, Color32)>, &str) {
    let tags = [("[ADD]", t!("Nouveau", "New"), Color32::from_rgb(0x6c, 0x9e, 0xf8)), ("[IMP]", t!("Amélioré", "Improved"), ok()), ("[FIX]", t!("Corrigé", "Fixed"), warn())];
    for (tag, label, color) in tags {
        if let Some(rest) = line.strip_prefix(tag) {
            return (Some((label, color)), rest.trim());
        }
    }
    (None, line)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar at the top (the sidebar hidden: the tab strip): the F opens the menus, the version
    /// beside it, then the views as tabs of their icon (a click opens the view, not a tab more).
    #[test]
    fn the_top_bar_has_the_menus_the_version_and_the_views() {
        let dir = std::env::temp_dir().join(format!("florm_status_f_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, None);
        app.show_sidebar = false;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 700.0));
        let frame = |app: &mut App, events: Vec<egui::Event>| {
            let out = crate::run_ui(&ctx, egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                app.tabs_strip(ui, None);
            });
            let mut texts = Vec::new();
            fn walk(s: &egui::Shape, out: &mut Vec<String>) {
                match s {
                    egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                    egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                    _ => {}
                }
            }
            out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
            texts
        };
        let mac = crate::os::window_controls() == crate::os::Controls::Mac;
        let x0 = if mac { crate::app::title_bar::MAC_BUTTONS_W } else { 0.0 };
        let before = frame(&mut app, Vec::new());
        let before = frame(&mut app, Vec::new()).into_iter().chain(before).collect::<Vec<_>>();
        assert!(before.iter().any(|t| *t == format!("v{}", crate::update::current())), "{before:?}");
        assert!(!before.iter().any(|t| t == "Édition"));
        let click = |app: &mut App, at: egui::Pos2| {
            let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            frame(app, vec![egui::Event::PointerMoved(at)]);
            frame(app, vec![press(true)]);
            frame(app, vec![press(false)]);
            frame(app, Vec::new())
        };
        // the F: the first thing at the left of the bar
        let after = click(&mut app, egui::pos2(x0 + 19.0, TAB_STRIP_H / 2.0));
        for want in ["Fichier", "Édition", "Git", "Outils"] {
            assert!(after.iter().any(|t| t == want), "{want}: {after:?}");
        }
        // close the menus, then the first icon tab: the preferences
        frame(&mut app, vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
        app.pending.clear();
        let logo_w = 34.0 + 52.0;
        click(&mut app, egui::pos2(x0 + logo_w + 19.0, TAB_STRIP_H / 2.0));
        assert!(app.pending.contains(&Cmd::OpenSettings), "{:?}", app.pending);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
