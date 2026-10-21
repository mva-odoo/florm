//! Preferences page and editable shortcuts.

use super::*;

/// A theme in the preferences: its background with a few lines of "code" in its colors.
fn theme_card(ui: &mut egui::Ui, name: &str, pv: &highlight::ThemePreview, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(168.0, 92.0), Sense::click());
    let p = ui.painter();
    let stroke = if selected {
        egui::Stroke::new(2.5_f32, ui.visuals().selection.stroke.color)
    } else if resp.hovered() {
        egui::Stroke::new(1.5_f32, ui.visuals().weak_text_color())
    } else {
        egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color)
    };
    p.rect_filled(rect, 8.0, pv.bg);
    // lines of "code"
    let bars: [(f32, f32, Color32); 4] = [(10.0, 50.0, pv.code[0]), (64.0, 60.0, pv.fg), (10.0, 80.0, pv.code[1]), (24.0, 60.0, pv.code[2])];
    for (k, (x, w, c)) in bars.iter().enumerate() {
        let y = rect.top() + 12.0 + k as f32 * 11.0;
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.left() + x, y), egui::vec2(*w, 5.0)), 2.5, *c);
    }
    p.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.right() - 26.0, rect.top() + 10.0), egui::vec2(16.0, 16.0)), 8.0, pv.accent);
    let band = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 26.0), rect.max);
    let band_bg = icons::mix(pv.bg, pv.fg, 0.08);
    p.rect_filled(band, egui::Rounding { nw: 0.0, ne: 0.0, sw: 8.0, se: 8.0 }, band_bg);
    p.with_clip_rect(band.shrink(4.0)).text(band.left_center() + egui::vec2(10.0, 0.0), Align2::LEFT_CENTER, name, egui::FontId::proportional(12.5), pv.fg);
    p.rect_stroke(rect, 8.0, stroke);
    resp
}

/// iOS-like switch.
fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = egui::vec2(36.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let v = ui.visuals();
    let off_bg = v.widgets.inactive.bg_fill;
    let on_bg = v.selection.stroke.color;
    let mix = |a: Color32, b: Color32| {
        let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
        Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
    };
    ui.painter().rect_filled(rect, rect.height() / 2.0, mix(off_bg, on_bg));
    let x = egui::lerp((rect.left() + 10.0)..=(rect.right() - 10.0), t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 7.0, Color32::WHITE);
    resp
}

/// Search filter + section header shown only above the first visible row.
struct RowCtx {
    filter: String,
    header: Option<(String, Color32)>,
}

impl RowCtx {
    fn matches(&self, texts: &[&str]) -> bool {
        let f = self.filter.to_lowercase();
        f.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(&f))
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        if let Some((h, color)) = self.header.take() {
            ui.add_space(14.0);
            ui.label(RichText::new(h).small().strong().color(color));
        }
    }
}

/// One setting: title + explanation on the left, control on the right.
/// Hidden when it does not match the search filter.
fn pref_row(ui: &mut egui::Ui, rc: &mut RowCtx, title: &str, desc: &str, control: impl FnOnce(&mut egui::Ui)) {
    if !rc.matches(&[title, desc]) {
        return;
    }
    rc.header(ui);
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let text_w = (ui.available_width() - 300.0).max(200.0);
        ui.vertical(|ui| {
            ui.set_width(text_w);
            ui.label(RichText::new(title).strong());
            if !desc.is_empty() {
                ui.label(RichText::new(desc).weak().size(12.5));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
    ui.add_space(10.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [egui::pos2(r.left(), r.top()), egui::pos2(r.right(), r.top())],
        egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
}

impl App {
    pub(super) fn prefs_ui(&mut self, ui: &mut egui::Ui) {
        let panel = ui.visuals().panel_fill;
        egui::SidePanel::left("prefs_nav")
            .exact_width(210.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin::symmetric(12.0, 16.0)))
            .show_inside(ui, |ui| {
                ui.label(RichText::new("Préférences").size(19.0).strong());
                ui.add_space(12.0);
                for (sec, label) in PREF_SECTIONS {
                    let selected = self.prefs.section == sec && self.prefs.filter.is_empty();
                    let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), Sense::click());
                    let v = ui.visuals();
                    if selected {
                        ui.painter().rect_filled(rect, 7.0, v.selection.bg_fill);
                    } else if r.hovered() {
                        ui.painter().rect_filled(rect, 7.0, v.widgets.hovered.weak_bg_fill);
                    }
                    let color = if selected { v.strong_text_color() } else { v.text_color() };
                    ui.painter().text(
                        egui::pos2(rect.left() + 12.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(14.0),
                        color,
                    );
                    if r.clicked() {
                        self.prefs.section = sec;
                        self.prefs.filter.clear();
                    }
                    ui.add_space(2.0);
                }
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(self.hl.colors.bg).inner_margin(egui::Margin::symmetric(28.0, 18.0)))
            .show_inside(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.prefs.filter)
                        .hint_text("Rechercher un réglage…")
                        .desired_width(420.0)
                        .font(egui::FontId::proportional(15.0)),
                );
                ui.add_space(8.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(860.0));
                    let sections: Vec<PrefSection> = if self.prefs.filter.is_empty() {
                        vec![self.prefs.section]
                    } else {
                        PREF_SECTIONS.iter().map(|(s, _)| *s).collect()
                    };
                    let searching = !self.prefs.filter.is_empty();
                    for sec in sections {
                        let name = PREF_SECTIONS.iter().find(|(s, _)| *s == sec).map(|(_, n)| *n).unwrap_or("");
                        let mut rc = RowCtx { filter: self.prefs.filter.clone(), header: None };
                        if searching {
                            rc.header = Some((name.to_uppercase(), self.hl.colors.accent));
                        } else {
                            ui.label(RichText::new(name).size(22.0).strong());
                            ui.add_space(4.0);
                        }
                        self.prefs_section(ui, sec, &mut rc);
                    }
                });
            });
    }

    fn prefs_section(&mut self, ui: &mut egui::Ui, sec: PrefSection, rc: &mut RowCtx) {
        let before = self.settings.clone();
        let mut actions: Vec<Cmd> = Vec::new();
        let s = &mut self.settings;
        match sec {
            PrefSection::Appearance => {
                let themes: Vec<(String, Option<highlight::ThemePreview>)> =
                    self.hl.theme_names().into_iter().map(|n| (n.clone(), self.hl.preview(&n))).collect();
                pref_row(ui, rc, "Thème", "Couleurs de l'éditeur et de l'interface. Les .tmTheme du dossier « themes » de la configuration sont listés aussi.", |ui| {
                    egui::ComboBox::from_id_salt("pref_theme").selected_text(&s.theme).width(220.0).show_ui(ui, |ui| {
                        for (t, _) in &themes {
                            ui.selectable_value(&mut s.theme, t.clone(), t);
                        }
                    });
                });
                if rc.matches(&["Thème", "couleurs", "aperçu"]) {
                    // the themes side by side, with a few colors of each: a click chooses it
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                        for (name, pv) in &themes {
                            let Some(pv) = pv else { continue };
                            if theme_card(ui, name, pv, s.theme == *name).clicked() {
                                s.theme = name.clone();
                            }
                        }
                    });
                    ui.add_space(8.0);
                }
                let theme_accent = self.hl.preview(&s.theme).map(|p| p.accent).unwrap_or(self.hl.colors.accent);
                pref_row(ui, rc, "Couleur d'accentuation", "Boutons principaux, onglet actif, sélection dans les listes. Par défaut celle du thème.", |ui| {
                    let mut c = highlight::parse_hex(&s.accent_color).unwrap_or(theme_accent);
                    if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                        s.accent_color = highlight::to_hex(c);
                    }
                    for (hex, tip) in [("#4F8CFF", "Bleu"), ("#2EA043", "Vert"), ("#D2A8FF", "Violet"), ("#F0883E", "Orange"), ("#E5534B", "Rouge"), ("#39C5CF", "Cyan")] {
                        let col = highlight::parse_hex(hex).unwrap();
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                        ui.painter().circle_filled(r.center(), 8.0, col);
                        if s.accent_color.eq_ignore_ascii_case(hex) {
                            ui.painter().circle_stroke(r.center(), 9.5, egui::Stroke::new(1.5_f32, ui.visuals().strong_text_color()));
                        }
                        if resp.on_hover_text(tip).clicked() {
                            s.accent_color = hex.to_string();
                        }
                    }
                    if !s.accent_color.is_empty() && ui.small_button("↺ Celle du thème").clicked() {
                        s.accent_color.clear();
                    }
                });
                pref_row(ui, rc, "Texte adouci", "Éclaircit les couleurs du texte (code et interface) : le texte paraît moins gras, surtout sur les thèmes clairs.", |ui| {
                    let mut pct = (s.text_softness * 100.0).round();
                    if ui.add(egui::Slider::new(&mut pct, 0.0..=40.0).step_by(5.0).suffix(" %")).changed() {
                        s.text_softness = pct / 100.0;
                    }
                });
                pref_row(ui, rc, "Taille du texte", "Taille de la police de l'éditeur (Ctrl+= / Ctrl+- aussi).", |ui| {
                    ui.add(egui::Slider::new(&mut s.font_size, 9.0..=28.0).step_by(0.5).suffix(" px"));
                });
                if self.prefs.font_list.is_none() && !self.prefs.font_list_loading {
                    self.prefs.font_list_loading = true;
                    self.jobs.spawn(|| JobResult::Fonts(crate::fonts::scan()));
                }
                let fonts = self.prefs.font_list.clone();
                let font_combo = |ui: &mut egui::Ui, id: &str, value: &mut String, mono_only: bool, default: &str| {
                    let shown = if value.is_empty() { format!("Par défaut ({default})") } else { value.clone() };
                    egui::ComboBox::from_id_salt(id).selected_text(shown).width(260.0).height(400.0).show_ui(ui, |ui| {
                        ui.selectable_value(value, String::new(), format!("Par défaut ({default})"));
                        match &fonts {
                            None => {
                                ui.spinner();
                            }
                            Some(list) => {
                                for f in list.iter().filter(|f| !mono_only || f.mono) {
                                    ui.selectable_value(value, f.family.clone(), &f.family);
                                }
                            }
                        }
                    });
                };
                let def_editor = crate::fonts::default_editor_font().first().copied().unwrap_or("Hack");
                let def_ui = crate::fonts::default_ui_font().first().copied().unwrap_or("Ubuntu");
                pref_row(ui, rc, "Police de l'éditeur", "Polices à chasse fixe installées sur le système (code, terminal).", |ui| {
                    font_combo(ui, "editor_font", &mut s.editor_font, true, def_editor);
                });
                pref_row(ui, rc, "Police de l'interface", "Menus, panneaux, onglets.", |ui| {
                    font_combo(ui, "ui_font", &mut s.ui_font, false, def_ui);
                });
                pref_row(ui, rc, "Minimap", "Aperçu du fichier à droite de l'éditeur.", |ui| {
                    toggle_switch(ui, &mut s.show_minimap);
                });
                pref_row(ui, rc, "Barre de menus", "Fichier, Édition, Sélection… en haut de la fenêtre. Masquée, les menus restent accessibles par ☰ en bas à gauche.", |ui| {
                    toggle_switch(ui, &mut s.show_menu_bar);
                });
                pref_row(ui, rc, "Fil d'Ariane", "Dossier › fichier › classe › fonction au-dessus de l'éditeur ; clic sur un élément pour y aller.", |ui| {
                    toggle_switch(ui, &mut s.breadcrumbs);
                });
            }
            PrefSection::Editor => {
                pref_row(ui, rc, "Largeur des tabulations", "Nombre de colonnes d'une tabulation et d'un niveau d'indentation.", |ui| {
                    ui.add(egui::Slider::new(&mut s.tab_size, 1..=8));
                });
                pref_row(ui, rc, "Indenter avec des espaces", "Tab insère des espaces plutôt qu'un caractère de tabulation (les fichiers existants gardent leur style).", |ui| {
                    toggle_switch(ui, &mut s.translate_tabs_to_spaces);
                });
                pref_row(ui, rc, "Fermeture automatique", "Ferme automatiquement ( [ { \" ' ` et entoure la sélection.", |ui| {
                    toggle_switch(ui, &mut s.auto_pair);
                });
                pref_row(ui, rc, "Supprimer les espaces en fin de ligne", "À chaque enregistrement.", |ui| {
                    toggle_switch(ui, &mut s.trim_trailing_whitespace_on_save);
                });
                pref_row(ui, rc, "Serveurs de langage (LSP)", "Complétion (Ctrl+Espace), aller à la définition (F12), renommer (F2). Serveurs détectés : pyright / pylsp, typescript-language-server, rust-analyzer, gopls, clangd.", |ui| {
                    toggle_switch(ui, &mut s.lsp);
                });
                pref_row(ui, rc, "Recherche dans le dossier en onglet", "Ctrl+Shift+F ouvre la recherche dans un onglet de l'éditeur plutôt que dans le panneau du bas (boutons ⇱ / ⇲ pour changer).", |ui| {
                    toggle_switch(ui, &mut s.search_in_tab);
                });
                pref_row(ui, rc, "Garder le travail non enregistré", "À la fermeture (ou en changeant de projet), les modifications non enregistrées sont gardées et rouvertes ensuite, sans question.", |ui| {
                    toggle_switch(ui, &mut s.hot_exit);
                });
            }
            PrefSection::Lint => {
                pref_row(ui, rc, "Vérifier pendant la frappe", "Les fichiers Python sont vérifiés une demi-seconde après la dernière touche, sans enregistrer.", |ui| {
                    toggle_switch(ui, &mut s.lint_live);
                });
                pref_row(ui, rc, "Vérifier à l'enregistrement", "Lance le linter (Python, JavaScript) à chaque enregistrement.", |ui| {
                    toggle_switch(ui, &mut s.lint_on_save);
                });
                let status: Vec<(&str, bool)> =
                    ["flake8", "ruff", "pyflakes", "pylint"].iter().map(|n| (*n, lint::is_installed(n))).collect();
                pref_row(ui, rc, "Linter Python", "« Automatique » prend flake8, sinon ruff, sinon pyflakes. La configuration du projet (setup.cfg, .flake8, tox.ini, pyproject.toml) est lue.", |ui| {
                    let label = |n: &str| match n {
                        "auto" => "Automatique".to_string(),
                        n => {
                            let ok = status.iter().any(|(m, i)| *m == n && *i);
                            format!("{n}{}", if ok { " — installé" } else { " — non installé" })
                        }
                    };
                    egui::ComboBox::from_id_salt("pref_linter").selected_text(label(&s.python_linter)).width(220.0).show_ui(ui, |ui| {
                        for n in ["auto", "flake8", "ruff", "pyflakes", "pylint"] {
                            ui.selectable_value(&mut s.python_linter, n.to_string(), label(n));
                        }
                    });
                });
                let args = &mut self.prefs.lint_args;
                pref_row(ui, rc, "Arguments du linter", "Ajoutés à la ligne de commande, séparés par des espaces. Ex. : --max-line-length=120 --ignore=E203", |ui| {
                    let r = ui.add(egui::TextEdit::singleline(args).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text("--max-line-length=120"));
                    if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        s.python_lint_args = args.split_whitespace().map(String::from).collect();
                    }
                });
                let installed = status.iter().find(|(n, _)| *n == "flake8").is_some_and(|(_, i)| *i);
                pref_row(ui, rc, "flake8", &format!("Installé dans {} (sans toucher au Python du système).", lint::tools_dir().display()), |ui| {
                    if installed {
                        ui.label(RichText::new("Installé").color(ok()));
                        if ui.button("Mettre à jour").clicked() {
                            actions.push(Cmd::InstallFlake8);
                        }
                    } else if ui.button("Installer flake8").clicked() {
                        actions.push(Cmd::InstallFlake8);
                    }
                });
            }
            PrefSection::Git => {
                pref_row(ui, rc, "Branche et remote dans l'arborescence", "Sur le dossier de chaque dépôt git : sa branche, la branche distante suivie (↑ à pousser, ↓ à récupérer) et le nombre de fichiers modifiés ; survol : les remotes.", |ui| {
                    toggle_switch(ui, &mut s.tree_git_info);
                });
                pref_row(ui, rc, "Diff dans l'éditeur", "Lignes ajoutées en vert et anciennes lignes en rouge directement dans le code (Ctrl+Alt+D).", |ui| {
                    toggle_switch(ui, &mut s.inline_diff);
                });
                let mut blame = self.blame_mode;
                pref_row(ui, rc, "Blame de la ligne", "Auteur et commit de la ligne courante en fin de ligne (Ctrl+B). Non mémorisé entre les sessions.", |ui| {
                    if toggle_switch(ui, &mut blame).changed() {
                        actions.push(Cmd::GitBlame);
                    }
                });
            }
            PrefSection::Terminal => {
                let shell = &mut self.prefs.shell;
                pref_row(ui, rc, "Shell", "Shell lancé dans le terminal intégré (ex. bash, zsh, fish, powershell). Vide : $SHELL (Linux / macOS) ou cmd (Windows). Pris en compte pour les nouveaux terminaux.", |ui| {
                    let r = ui.add(egui::TextEdit::singleline(shell).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text("/bin/bash"));
                    if r.lost_focus() {
                        s.shell = (!shell.trim().is_empty()).then(|| shell.trim().to_string());
                    }
                });
            }
            PrefSection::Keys => {
                let mut key_action = None;
                if rc.filter.is_empty() {
                    ui.label(
                        RichText::new("Clique sur un raccourci puis appuie sur la nouvelle combinaison (Ctrl+K puis une touche pour un enchaînement). Échap annule.")
                            .weak()
                            .size(12.5),
                    );
                    ui.add_space(6.0);
                }
                if let Some((cmd, seq)) = &self.prefs.proposed {
                    rc.header(ui);
                    let other = self.keymap.lookup_seq(seq);
                    let name = |c: Cmd| self.cmd_name(c);
                    egui::Frame::none()
                        .stroke(egui::Stroke::new(1.0_f32, warn()))
                        .rounding(8.0)
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.label(format!(
                                "« {} » est déjà utilisé par « {} ». L'attribuer à « {} » ?",
                                keymap::seq_label(seq),
                                other.map(name).unwrap_or_default(),
                                name(*cmd)
                            ));
                            ui.horizontal(|ui| {
                                if ui.button(RichText::new("Remplacer").color(warn())).clicked() {
                                    key_action = Some(KeyAction::Confirm);
                                }
                                if ui.button("Annuler").clicked() {
                                    key_action = Some(KeyAction::Cancel);
                                }
                            });
                        });
                    ui.add_space(8.0);
                }
                let capturing = self.prefs.capture;
                for (id, label, cmd) in commands::ALL.iter().filter(|(_, l, _)| !l.is_empty()) {
                    let keys = self.keymap.label(*cmd);
                    if !rc.matches(&[label, &keys, id]) {
                        continue;
                    }
                    rc.header(ui);
                    let customized = self.shortcut_customized(*cmd);
                    ui.horizontal(|ui| {
                        let label_w = (ui.available_width() - 420.0).max(220.0);
                        // fixed-width label column so the shortcut chips line up
                        let (r, _) = ui.allocate_exact_size(egui::vec2(label_w, 28.0), Sense::hover());
                        ui.painter().with_clip_rect(r).text(
                            egui::pos2(r.left(), r.center().y),
                            Align2::LEFT_CENTER,
                            *label,
                            egui::FontId::proportional(14.0),
                            ui.visuals().text_color(),
                        );
                        let this = capturing.filter(|c| c.cmd == *cmd);
                        let (text, color) = match this {
                            Some(c) => (
                                match c.first {
                                    Some(f) => format!("{} …", f.label()),
                                    None => "Appuyez sur une combinaison…".to_string(),
                                },
                                self.hl.colors.accent,
                            ),
                            None if keys.is_empty() => ("Ajouter".to_string(), ui.visuals().weak_text_color()),
                            None => (keys.clone(), ui.visuals().strong_text_color()),
                        };
                        let chip = egui::Button::new(RichText::new(text).monospace().color(color))
                            .fill(ui.visuals().widgets.inactive.bg_fill)
                            .min_size(egui::vec2(210.0, 24.0));
                        if ui.add(chip).on_hover_text("Cliquer pour changer").clicked() {
                            key_action = Some(KeyAction::Capture(*cmd));
                        }
                        if !keys.is_empty() && ui.button("×").on_hover_text("Supprimer ce raccourci").clicked() {
                            key_action = Some(KeyAction::Unbind(*cmd));
                        }
                        if customized {
                            if ui.button("↺").on_hover_text("Revenir au raccourci par défaut").clicked() {
                                key_action = Some(KeyAction::Reset(*cmd));
                            }
                            ui.label(RichText::new("modifié").small().color(self.hl.colors.accent));
                        }
                        ui.label(RichText::new(*id).weak().small());
                    });
                }
                if let Some(a) = key_action {
                    self.prefs.key_action = Some(a);
                }
            }
            PrefSection::Advanced => {
                let exclude = &mut self.prefs.exclude;
                pref_row(
                    ui,
                    rc,
                    "Fichiers exclus",
                    "Ni dans « aller au fichier » (Ctrl+P), ni dans l'index des symboles, ni dans la recherche. Séparés par des virgules, syntaxe .gitignore (dossier/ ou *.ext). Le .gitignore des dépôts et .git sont toujours appliqués.",
                    |ui| {
                        let r = ui.add(egui::TextEdit::singleline(exclude).desired_width(320.0).font(egui::TextStyle::Monospace).hint_text("__pycache__/, *.pyc"));
                        if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            s.exclude = exclude.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
                        }
                    },
                );
                pref_row(ui, rc, "Taille de l'index des symboles", "Fichiers source lus au plus (définitions F12, survol, complétion). Les dépôts des fichiers ouverts passent en premier ; plus grand = plus de mémoire et un chargement plus long.", |ui| {
                    // fixed choices: each change builds the index again
                    let label = |n: usize| format!("{} fichiers", n.to_string().as_bytes().rchunks(3).rev().map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join(" "));
                    egui::ComboBox::from_id_salt("pref_index_max").selected_text(label(s.index_max_files)).show_ui(ui, |ui| {
                        for n in [50_000, 150_000, 300_000, 600_000, 1_500_000] {
                            ui.selectable_value(&mut s.index_max_files, n, label(n));
                        }
                    });
                });
                pref_row(ui, rc, "Fichier de préférences", &config::settings_path().display().to_string(), |ui| {
                    if ui.button("Ouvrir settings.toml").clicked() {
                        actions.push(Cmd::OpenSettingsFile);
                    }
                });
                pref_row(ui, rc, "Plugins", &config::plugins_dir().display().to_string(), |ui| {
                    if ui.button("Recharger").clicked() {
                        actions.push(Cmd::ReloadPlugins);
                    }
                    if ui.button("Ouvrir le dossier").clicked() {
                        actions.push(Cmd::OpenPluginsFolder);
                    }
                });
                pref_row(ui, rc, "Historique de recherche", "Recherches et filtres mémorisés.", |ui| {
                    if ui.button("Effacer").clicked() {
                        actions.push(Cmd::ClearSearchHistory);
                    }
                });
            }
        }
        self.pending.extend(actions);
        self.apply_settings_change(&before);
        if let Some(a) = self.prefs.key_action.take() {
            match a {
                KeyAction::Capture(cmd) => {
                    self.prefs.proposed = None;
                    self.prefs.capture = Some(Capture { cmd, first: None });
                    // no text field must receive the keys being recorded
                    if let Some(id) = ui.ctx().memory(|m| m.focused()) {
                        ui.ctx().memory_mut(|m| m.surrender_focus(id));
                    }
                }
                KeyAction::Unbind(cmd) => self.unbind_shortcut(cmd),
                KeyAction::Reset(cmd) => self.reset_shortcut(cmd),
                KeyAction::Confirm => {
                    if let Some((cmd, seq)) = self.prefs.proposed.take() {
                        self.bind_shortcut(cmd, seq);
                    }
                }
                KeyAction::Cancel => self.prefs.proposed = None,
            }
        }
    }

    // ------------------------------------------------------------------ editable shortcuts

    /// Human name of a command (palette label, or "plugin : title").
    fn cmd_name(&self, cmd: Cmd) -> String {
        if let Cmd::Plugin(k) = cmd {
            if let Some(&(pi, ci)) = self.plugin_cmds.get(k) {
                return format!("plugin {} : {}", self.plugins[pi].name, self.plugins[pi].commands[ci].title);
            }
        }
        commands::ALL.iter().find(|(_, _, c)| *c == cmd).map(|(_, l, _)| l.to_string()).unwrap_or_else(|| format!("{cmd:?}"))
    }

    fn write_key(&mut self, key: String, value: Option<String>) {
        if let Err(e) = config::set_key(&key, value.as_deref()) {
            self.log(format!("Raccourci non enregistré : {e}"));
        }
        match value {
            Some(v) => {
                self.settings.keys.insert(key, v);
            }
            None => {
                self.settings.keys.remove(&key);
            }
        }
    }

    /// [keys] entries (exact text) describing `seq`.
    fn user_entries_for(&self, seq: &[Combo]) -> Vec<String> {
        self.settings.keys.keys().filter(|k| keymap::parse_seq(k).as_deref() == Some(seq)).cloned().collect()
    }

    fn shortcut_customized(&self, cmd: Cmd) -> bool {
        let Some(id) = commands::id_of(cmd) else { return false };
        let defaults = Keymap::default_seqs(cmd);
        self.settings
            .keys
            .iter()
            .any(|(k, v)| v == id || (v == UNBOUND && keymap::parse_seq(k).is_some_and(|s| defaults.contains(&s))))
    }

    /// Removes every current shortcut of `cmd` (built-in ones are disabled with "none").
    fn clear_shortcuts_of(&mut self, cmd: Cmd) {
        let defaults = Keymap::default_seqs(cmd);
        for seq in self.keymap.seqs_of(cmd) {
            for k in self.user_entries_for(&seq) {
                self.write_key(k, None);
            }
            if defaults.contains(&seq) {
                self.write_key(keymap::seq_config(&seq), Some(UNBOUND.to_string()));
            }
        }
    }

    /// Called when a combination was recorded: binds it, or asks first if it is taken.
    pub(super) fn propose_shortcut(&mut self, cmd: Cmd, seq: Seq) {
        match self.keymap.lookup_seq(&seq) {
            Some(other) if other == cmd => self.set_status("C'est déjà ce raccourci"),
            Some(_) => self.prefs.proposed = Some((cmd, seq)),
            None => self.bind_shortcut(cmd, seq),
        }
    }

    fn bind_shortcut(&mut self, cmd: Cmd, seq: Seq) {
        let Some(id) = commands::id_of(cmd) else { return };
        self.clear_shortcuts_of(cmd);
        for k in self.user_entries_for(&seq) {
            self.write_key(k, None);
        }
        self.write_key(keymap::seq_config(&seq), Some(id.to_string()));
        self.rebuild_keymap();
        self.set_status(format!("Raccourci {} enregistré", keymap::seq_label(&seq)));
    }

    fn unbind_shortcut(&mut self, cmd: Cmd) {
        self.clear_shortcuts_of(cmd);
        self.rebuild_keymap();
    }

    fn reset_shortcut(&mut self, cmd: Cmd) {
        let Some(id) = commands::id_of(cmd) else { return };
        let defaults = Keymap::default_seqs(cmd);
        let entries: Vec<String> = self
            .settings
            .keys
            .iter()
            .filter(|(k, v)| *v == id || (*v == UNBOUND && keymap::parse_seq(k).is_some_and(|s| defaults.contains(&s))))
            .map(|(k, _)| k.clone())
            .collect();
        for k in entries {
            self.write_key(k, None);
        }
        self.rebuild_keymap();
        self.set_status("Raccourci par défaut rétabli");
    }

    /// Saves changed settings to settings.toml and applies their effects right away.
    fn apply_settings_change(&mut self, before: &Settings) {
        let s = self.settings.clone();
        if s.theme != before.theme {
            if self.hl.set_theme(&s.theme) {
                self.visuals_dirty = true;
            }
            self.persist("theme", config::toml_str(&s.theme));
        }
        if s.font_size != before.font_size {
            self.persist("font_size", format!("{:.1}", s.font_size));
        }
        if s.accent_color != before.accent_color || s.text_softness != before.text_softness {
            self.hl.set_overrides(highlight::parse_hex(&s.accent_color), s.text_softness);
            self.visuals_dirty = true;
            self.persist("accent_color", config::toml_str(&s.accent_color));
            self.persist("text_softness", format!("{:.2}", s.text_softness));
        }
        if s.editor_font != before.editor_font || s.ui_font != before.ui_font {
            self.egui_ctx.set_fonts(crate::fonts::definitions(&s.editor_font, &s.ui_font));
            self.persist("editor_font", config::toml_str(&s.editor_font));
            self.persist("ui_font", config::toml_str(&s.ui_font));
        }
        if s.tab_size != before.tab_size || s.translate_tabs_to_spaces != before.translate_tabs_to_spaces {
            for d in self.docs.iter_mut() {
                d.tab_size = s.tab_size;
            }
            self.persist("tab_size", s.tab_size.to_string());
            self.persist("translate_tabs_to_spaces", s.translate_tabs_to_spaces.to_string());
        }
        let bools = [
            ("show_minimap", s.show_minimap, before.show_minimap),
            ("breadcrumbs", s.breadcrumbs, before.breadcrumbs),
            ("show_menu_bar", s.show_menu_bar, before.show_menu_bar),
            ("search_in_tab", s.search_in_tab, before.search_in_tab),
            ("tree_git_info", s.tree_git_info, before.tree_git_info),
            ("auto_pair", s.auto_pair, before.auto_pair),
            ("trim_trailing_whitespace_on_save", s.trim_trailing_whitespace_on_save, before.trim_trailing_whitespace_on_save),
            ("hot_exit", s.hot_exit, before.hot_exit),
            ("lsp", s.lsp, before.lsp),
            ("lint_live", s.lint_live, before.lint_live),
            ("lint_on_save", s.lint_on_save, before.lint_on_save),
            ("inline_diff", s.inline_diff, before.inline_diff),
        ];
        for (key, now, was) in bools {
            if now != was {
                self.persist(key, now.to_string());
            }
        }
        let lint_changed = s.python_linter != before.python_linter || s.python_lint_args != before.python_lint_args || s.lint_live != before.lint_live;
        if s.python_linter != before.python_linter {
            self.persist("python_linter", config::toml_str(&s.python_linter));
        }
        if s.python_lint_args != before.python_lint_args {
            self.persist("python_lint_args", config::toml_str_array(&s.python_lint_args));
        }
        if lint_changed {
            for d in self.docs.iter_mut() {
                d.lint_version = u64::MAX; // re-lint with the new settings
            }
        }
        if s.lsp != before.lsp || s.lsp_servers != before.lsp_servers {
            self.lsp.manager.reset();
            self.lsp.notes.clear();
            self.lsp.completion = None;
        }
        if s.exclude != before.exclude {
            self.persist("exclude", config::toml_str_array(&s.exclude));
        }
        if s.index_max_files != before.index_max_files {
            self.persist("index_max_files", s.index_max_files.to_string());
        }
        if s.exclude != before.exclude || s.index_max_files != before.index_max_files {
            // file list and symbol index made again with the new rules
            self.index.symbols_for = None;
            self.refresh_index();
        }
        if s.shell != before.shell {
            self.persist("shell", config::toml_str(s.shell.as_deref().unwrap_or("")));
        }
    }
}
