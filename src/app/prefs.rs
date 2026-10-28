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

/// Title of a group of settings with a line under it, to set the groups apart. Not shown while
/// searching: the rows found are listed under their section.
fn pref_group(ui: &mut egui::Ui, rc: &RowCtx, title: &str, color: Color32) {
    if !rc.filter.is_empty() {
        return;
    }
    ui.add_space(26.0);
    ui.label(RichText::new(title).size(15.5).strong().color(color));
    ui.add_space(2.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().line_segment([egui::pos2(r.left(), r.top()), egui::pos2(r.right(), r.top())], egui::Stroke::new(1.5_f32, color.gamma_multiply(0.6)));
}

/// « Installé » in green, or « Non installé ».
fn install_state(ui: &mut egui::Ui, installed: bool) {
    if installed {
        ui.label(RichText::new(t!("Installé", "Installed")).color(ok()));
    } else {
        ui.label(RichText::new(t!("Non installé", "Not installed")).weak());
    }
}

/// The codes typed in a field: separated by commas or spaces.
fn codes_of(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace()).filter(|c| !c.is_empty()).map(String::from).collect()
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

/// A row whose control is too wide for the right of the text (a field and several buttons): it
/// goes under the text.
fn pref_row_wide(ui: &mut egui::Ui, rc: &mut RowCtx, title: &str, desc: &str, control: impl FnOnce(&mut egui::Ui)) {
    if !rc.matches(&[title, desc]) {
        return;
    }
    rc.header(ui);
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong());
    if !desc.is_empty() {
        ui.add(egui::Label::new(RichText::new(desc).weak().size(12.5)).wrap());
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(control);
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
                ui.label(RichText::new(t!("Préférences", "Preferences")).size(19.0).strong());
                ui.add_space(12.0);
                for (sec, fr, en) in PREF_SECTIONS {
                    let label = t!(fr, en);
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
                        .hint_text(t!("Rechercher un réglage…", "Search a setting…"))
                        .desired_width(420.0)
                        .font(egui::FontId::proportional(15.0)),
                );
                ui.add_space(8.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(860.0));
                    let sections: Vec<PrefSection> = if self.prefs.filter.is_empty() {
                        vec![self.prefs.section]
                    } else {
                        PREF_SECTIONS.iter().map(|(s, _, _)| *s).collect()
                    };
                    let searching = !self.prefs.filter.is_empty();
                    for sec in sections {
                        let name = PREF_SECTIONS.iter().find(|(s, _, _)| *s == sec).map(|(_, fr, en)| t!(*fr, *en)).unwrap_or("");
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
        // to write in the log once the settings are no longer borrowed
        let mut note: Option<String> = None;
        match sec {
            PrefSection::Appearance => {
                pref_row(ui, rc, t!("Langue", "Language"), t!("Langue des menus, des panneaux et des messages (Language).", "Language of the menus, panels and messages (Langue)."), |ui| {
                    let cur = crate::i18n::from_code(&s.language);
                    egui::ComboBox::from_id_salt("pref_language").selected_text(crate::i18n::name(cur)).width(220.0).show_ui(ui, |ui| {
                        for (l, code, name) in crate::i18n::LANGS {
                            if ui.selectable_label(cur == l, name).clicked() {
                                s.language = code.to_string();
                            }
                        }
                    });
                });
                let themes: Vec<(String, Option<highlight::ThemePreview>)> =
                    self.hl.theme_names().into_iter().map(|n| (n.clone(), self.hl.preview(&n))).collect();
                pref_row(ui, rc, t!("Thème", "Theme"), t!("Couleurs de l'éditeur et de l'interface. Les .tmTheme du dossier « themes » de la configuration sont listés aussi.", "Colors of the editor and of the interface. The .tmTheme files of the “themes” folder of the configuration are listed too."), |ui| {
                    egui::ComboBox::from_id_salt("pref_theme").selected_text(&s.theme).width(220.0).show_ui(ui, |ui| {
                        for (t, _) in &themes {
                            ui.selectable_value(&mut s.theme, t.clone(), t);
                        }
                    });
                });
                if rc.matches(&[t!("Thème", "Theme"), t!("couleurs", "colors"), t!("aperçu", "preview")]) {
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
                // a theme of one's own: the colors of the current one, changed here
                let theme_dirs = config::theme_dirs();
                let user_theme = |name: &str| theme_dirs.iter().any(|d| d.join(format!("{name}.tmTheme")).is_file());
                let current_is_user = user_theme(&s.theme);
                let mut theme_act = None;
                let mut drop_theme = false;
                if self.prefs.theme_draft.is_none() {
                    pref_row(ui, rc, t!("Couleurs du thème", "Theme colors"), t!("Un thème à toi : les couleurs de l'éditeur et du code, à partir du thème choisi, enregistré dans le dossier « themes » de la configuration.", "A theme of your own: the colors of the editor and of the code, starting from the chosen theme, saved in the “themes” folder of the configuration."), |ui| {
                        if current_is_user && ui.button(t!("Supprimer ce thème", "Delete this theme")).clicked() {
                            drop_theme = true;
                        }
                        let label = if current_is_user { t!("Modifier ce thème", "Edit this theme").to_string() } else { tf!("Créer un thème à partir de « {} »", "Create a theme from “{}”", s.theme) };
                        if ui.button(label).clicked() {
                            theme_act = Some(None);
                        }
                    });
                } else if rc.matches(&[t!("Couleurs du thème", "Theme colors"), t!("thème", "theme"), t!("couleurs", "colors")]) {
                    let builtin: Vec<String> = self.hl.theme_names().into_iter().filter(|n| !user_theme(n)).collect();
                    let accent = self.hl.colors.accent;
                    if let Some(d) = self.prefs.theme_draft.as_mut() {
                        ui.add_space(10.0);
                        theme_act = d.ui(ui, &builtin, accent).map(Some);
                    }
                }
                match theme_act {
                    // start from the current theme
                    Some(None) => self.prefs.theme_draft = Some(theme_edit::ThemeDraft::from_current(&self.hl, current_is_user)),
                    Some(Some(theme_edit::ThemeAct::Changed)) => {
                        if let Some(d) = &self.prefs.theme_draft {
                            if self.hl.add_theme_xml(&d.name, &d.to_tmtheme()).is_ok() {
                                self.hl.set_theme(&d.name);
                            }
                        }
                    }
                    Some(Some(theme_edit::ThemeAct::Save)) => {
                        if let Some(d) = self.prefs.theme_draft.take() {
                            let xml = d.to_tmtheme();
                            let path = d.path();
                            let written = std::fs::create_dir_all(config::themes_dir()).and_then(|_| std::fs::write(&path, &xml));
                            match written {
                                Ok(()) => {
                                    let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or(d.name);
                                    let _ = self.hl.add_theme_xml(&name, &xml);
                                    self.hl.set_theme(&name);
                                    s.theme = name;
                                }
                                Err(e) => note = Some(tf!("Thème non enregistré : {e}", "Theme not saved: {e}")),
                            }
                        }
                    }
                    Some(Some(theme_edit::ThemeAct::Cancel)) => {
                        if let Some(d) = self.prefs.theme_draft.take() {
                            if !user_theme(&d.name) {
                                self.hl.remove_theme(&d.name);
                            }
                        }
                        self.hl.set_theme(&s.theme);
                    }
                    None => {}
                }
                if drop_theme {
                    for d in &theme_dirs {
                        let _ = std::fs::remove_file(d.join(format!("{}.tmTheme", s.theme)));
                    }
                    self.hl.remove_theme(&s.theme);
                    s.theme = "Florm".into();
                }
                let theme_accent = self.hl.preview(&s.theme).map(|p| p.accent).unwrap_or(self.hl.colors.accent);
                pref_row(ui, rc, t!("Couleur d'accentuation", "Accent color"), t!("Boutons principaux, onglet actif, sélection dans les listes. Par défaut celle du thème.", "Main buttons, active tab, selection in lists. By default the theme's."), |ui| {
                    let mut c = highlight::parse_hex(&s.accent_color).unwrap_or(theme_accent);
                    if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                        s.accent_color = highlight::to_hex(c);
                    }
                    for (hex, tip) in [("#4F8CFF", t!("Bleu", "Blue")), ("#2EA043", t!("Vert", "Green")), ("#D2A8FF", t!("Violet", "Purple")), ("#F0883E", "Orange"), ("#E5534B", t!("Rouge", "Red")), ("#39C5CF", "Cyan")] {
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
                    if !s.accent_color.is_empty() && ui.small_button(t!("↺ Celle du thème", "↺ The theme's")).clicked() {
                        s.accent_color.clear();
                    }
                });
                pref_row(ui, rc, t!("Texte adouci", "Softened text"), t!("Éclaircit les couleurs du texte (code et interface) : le texte paraît moins gras, surtout sur les thèmes clairs.", "Lightens the text colors (code and interface): the text looks less bold, especially on light themes."), |ui| {
                    let mut pct = (s.text_softness * 100.0).round();
                    if ui.add(egui::Slider::new(&mut pct, 0.0..=40.0).step_by(5.0).suffix(" %")).changed() {
                        s.text_softness = pct / 100.0;
                    }
                });
                pref_row(ui, rc, t!("Taille du texte", "Text size"), t!("Taille de la police de l'éditeur (Ctrl+= / Ctrl+- aussi).", "Font size of the editor (Ctrl+= / Ctrl+- too)."), |ui| {
                    ui.add(egui::Slider::new(&mut s.font_size, 9.0..=28.0).step_by(0.5).suffix(" px"));
                });
                if self.prefs.font_list.is_none() && !self.prefs.font_list_loading {
                    self.prefs.font_list_loading = true;
                    self.jobs.spawn(|| JobResult::Fonts(crate::fonts::scan()));
                }
                let fonts = self.prefs.font_list.clone();
                let font_combo = |ui: &mut egui::Ui, id: &str, value: &mut String, mono_only: bool, default: &str| {
                    let shown = if value.is_empty() { tf!("Par défaut ({default})", "Default ({default})") } else { value.clone() };
                    egui::ComboBox::from_id_salt(id).selected_text(shown).width(260.0).height(400.0).show_ui(ui, |ui| {
                        ui.selectable_value(value, String::new(), tf!("Par défaut ({default})", "Default ({default})"));
                        match &fonts {
                            None => {
                                spinner(ui);
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
                pref_row(ui, rc, t!("Police de l'éditeur", "Editor font"), t!("Polices à chasse fixe installées sur le système (code, terminal).", "Monospaced fonts installed on the system (code, terminal)."), |ui| {
                    font_combo(ui, "editor_font", &mut s.editor_font, true, def_editor);
                });
                pref_row(ui, rc, t!("Police de l'interface", "Interface font"), t!("Menus, panneaux, onglets.", "Menus, panels, tabs."), |ui| {
                    font_combo(ui, "ui_font", &mut s.ui_font, false, def_ui);
                });
                pref_row(ui, rc, "Minimap", t!("Aperçu du fichier à droite de l'éditeur.", "Overview of the file on the right of the editor."), |ui| {
                    toggle_switch(ui, &mut s.show_minimap);
                });
                pref_row(ui, rc, t!("Retour à la ligne automatique", "Word wrap"), t!("Les lignes plus longues que l'éditeur continuent sur les rangées suivantes, coupées après un espace et indentées comme la ligne (Alt+Z). Le fichier n'est pas modifié.", "Lines longer than the editor go on over the next rows, cut after a space and indented like the line (Alt+Z). The file is not changed."), |ui| {
                    toggle_switch(ui, &mut s.word_wrap);
                });
                pref_row(ui, rc, t!("Classe et fonction figées en haut", "Sticky class and function"), t!("En défilant, les lignes class / def (ou les titres, les records…) qui contiennent le haut de l'écran y restent ; clic : y aller.", "While scrolling, the class / def lines (or the headings, the records…) containing the top of the screen stay there; click: go to it."), |ui| {
                    toggle_switch(ui, &mut s.sticky_scroll);
                });
                let rulers = &mut self.prefs.rulers;
                pref_row(ui, rc, t!("Règles verticales", "Vertical rulers"), t!("Lignes verticales aux colonnes données, séparées par des virgules (ex. 79, 120). Vide : aucune.", "Vertical lines at the given columns, separated by commas (e.g. 79, 120). Empty: none."), |ui| {
                    let r = ui.add(egui::TextEdit::singleline(rulers).desired_width(120.0).font(egui::TextStyle::Monospace).hint_text("79, 120"));
                    if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        s.rulers = rulers.split(',').filter_map(|c| c.trim().parse().ok()).filter(|c| *c > 0).collect();
                    }
                });
                pref_row(ui, rc, t!("Barre de menus", "Menu bar"), t!("Fichier, Édition, Sélection… en haut de la fenêtre. Masquée, les menus restent accessibles par ☰ en bas à gauche.", "File, Edit, Selection… at the top of the window. Hidden, the menus are still under ☰ at the bottom left."), |ui| {
                    toggle_switch(ui, &mut s.show_menu_bar);
                });
                pref_row(ui, rc, t!("Fil d'Ariane", "Breadcrumbs"), t!("Dossier › fichier › classe › fonction au-dessus de l'éditeur ; clic sur un élément pour y aller.", "Folder › file › class › function above the editor; click an item to go to it."), |ui| {
                    toggle_switch(ui, &mut s.breadcrumbs);
                });
            }
            PrefSection::Editor => {
                pref_row(ui, rc, t!("Largeur des tabulations", "Tab width"), t!("Nombre de colonnes d'une tabulation et d'un niveau d'indentation.", "Number of columns of a tab and of an indentation level."), |ui| {
                    ui.add(egui::Slider::new(&mut s.tab_size, 1..=8));
                });
                pref_row(ui, rc, t!("Indenter avec des espaces", "Indent with spaces"), t!("Tab insère des espaces plutôt qu'un caractère de tabulation (les fichiers existants gardent leur style).", "Tab inserts spaces rather than a tab character (existing files keep their style)."), |ui| {
                    toggle_switch(ui, &mut s.translate_tabs_to_spaces);
                });
                pref_row(ui, rc, t!("Fermeture automatique", "Auto-close brackets"), t!("Ferme automatiquement ( [ { \" ' ` et entoure la sélection.", "Closes ( [ { \" ' ` automatically and surrounds the selection."), |ui| {
                    toggle_switch(ui, &mut s.auto_pair);
                });
                pref_row(ui, rc, t!("Supprimer les espaces en fin de ligne", "Trim trailing whitespace"), t!("À chaque enregistrement.", "On every save."), |ui| {
                    toggle_switch(ui, &mut s.trim_trailing_whitespace_on_save);
                });
                pref_row(ui, rc, t!("Serveurs de langage (LSP)", "Language servers (LSP)"), t!("Complétion (Ctrl+Espace), aller à la définition (F12), renommer (F2). Serveurs détectés : pyright / pylsp, typescript-language-server, rust-analyzer, gopls, clangd.", "Completion (Ctrl+Space), go to definition (F12), rename (F2). Servers detected: pyright / pylsp, typescript-language-server, rust-analyzer, gopls, clangd."), |ui| {
                    toggle_switch(ui, &mut s.lsp);
                });
                pref_row(ui, rc, t!("Recherche dans le dossier en onglet", "Find in folder in a tab"), t!("Ctrl+Shift+F ouvre la recherche dans un onglet de l'éditeur plutôt que dans le panneau du bas (boutons « en onglet » / « en bas » de son en-tête pour changer).", "Ctrl+Shift+F opens the search in an editor tab rather than in the bottom panel (“in a tab” / “at the bottom” buttons of its header to switch)."), |ui| {
                    toggle_switch(ui, &mut s.search_in_tab);
                });
                pref_row(ui, rc, t!("Garder le travail non enregistré", "Keep unsaved work"), t!("À la fermeture (ou en changeant de projet), les modifications non enregistrées sont gardées et rouvertes ensuite, sans question.", "When closing (or switching project), unsaved changes are kept and reopened later, without asking."), |ui| {
                    toggle_switch(ui, &mut s.hot_exit);
                });
            }
            PrefSection::Lint => {
                let accent = self.hl.colors.accent;
                pref_group(ui, rc, t!("Quand vérifier", "When to check"), accent);
                pref_row(ui, rc, t!("Vérifier pendant la frappe", "Check while typing"), t!("Les fichiers Python sont vérifiés une demi-seconde après la dernière touche, sans enregistrer.", "Python files are checked half a second after the last key, without saving."), |ui| {
                    toggle_switch(ui, &mut s.lint_live);
                });
                pref_row(ui, rc, t!("Vérifier à l'enregistrement", "Check on save"), t!("Lance le linter (Python, JavaScript) à chaque enregistrement.", "Runs the linter (Python, JavaScript) on every save."), |ui| {
                    toggle_switch(ui, &mut s.lint_on_save);
                });
                let status: Vec<(&str, bool)> =
                    ["flake8", "ruff", "pyflakes", "pylint"].iter().map(|n| (*n, lint::is_installed(n))).collect();
                let installed = |n: &str| status.iter().any(|(m, i)| *m == n && *i);

                pref_group(ui, rc, "Python", accent);
                pref_row(ui, rc, t!("Linter Python", "Python linter"), t!("« Automatique » prend flake8, sinon ruff, sinon pyflakes ; un linter qui plante est signalé en ligne 1 et le suivant prend le relais. ruff est le plus rapide et le plus léger (quelques Mo, en Rust). La configuration du projet (setup.cfg, .flake8, tox.ini, pyproject.toml) est lue.", "“Automatic” takes flake8, else ruff, else pyflakes; a linter that crashes is reported on line 1 and the next one takes over. ruff is the fastest and lightest (a few MB, in Rust). The project configuration (setup.cfg, .flake8, tox.ini, pyproject.toml) is read."), |ui| {
                    let label = |n: &str| match n {
                        "auto" => t!("Automatique", "Automatic").to_string(),
                        n => format!("{n}{}", if installed(n) { t!(" — installé", " — installed") } else { t!(" — non installé", " — not installed") }),
                    };
                    egui::ComboBox::from_id_salt("pref_linter").selected_text(label(&s.python_linter)).width(220.0).show_ui(ui, |ui| {
                        for n in ["auto", "flake8", "ruff", "pyflakes", "pylint"] {
                            ui.selectable_value(&mut s.python_linter, n.to_string(), label(n));
                        }
                    });
                });
                let args = &mut self.prefs.lint_args;
                pref_row(ui, rc, t!("Arguments du linter", "Linter arguments"), t!("Ajoutés à la ligne de commande, séparés par des espaces. Ex. : --max-line-length=120 (en automatique, ruff ne reçoit que la longueur de ligne).", "Added to the command line, separated by spaces. E.g.: --max-line-length=120 (in automatic mode, ruff only gets the line length)."), |ui| {
                    let r = ui.add(egui::TextEdit::singleline(args).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text("--max-line-length=120"));
                    if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        s.python_lint_args = args.split_whitespace().map(String::from).collect();
                    }
                });

                // each linter: where it comes from, then the codes it should not report
                let about = [
                    ("flake8", tf!("pycodestyle + pyflakes, codes E / W / F. Installé par Florm dans {} (sans toucher au Python du système).", "pycodestyle + pyflakes, codes E / W / F. Installed by Florm in {} (without touching the system's Python).", lint::tools_dir().display())),
                    ("ruff", t!("Le plus rapide (en Rust), mêmes codes que flake8 et bien d'autres (I, B, UP…). Cherché dans le PATH (pip install ruff).", "The fastest (in Rust), same codes as flake8 and many more (I, B, UP…). Looked for in the PATH (pip install ruff).").to_string()),
                    ("pylint", t!("Le plus complet et le plus lent, codes C / R / W / E (ou leur nom). Utilisé seulement quand il est choisi plus haut.", "The most complete and the slowest, codes C / R / W / E (or their name). Used only when chosen above.").to_string()),
                    ("eslint", t!("JavaScript et TypeScript, à l'enregistrement. Celui du projet (node_modules/.bin) d'abord, sinon celui du PATH ; sa configuration est lue.", "JavaScript and TypeScript, on save. The project's one (node_modules/.bin) first, else the one in the PATH; its configuration is read.").to_string()),
                ];
                for (k, (name, example)) in lint::WITH_CODES.iter().enumerate() {
                    let title = if *name == "eslint" { "JavaScript — ESLint" } else { name };
                    pref_group(ui, rc, title, accent);
                    let desc = about.iter().find(|(n, _)| n == name).map_or("", |(_, d)| d.as_str());
                    pref_row(ui, rc, name, desc, |ui| match *name {
                        "flake8" => {
                            if ui.button(if installed(name) { t!("Mettre à jour", "Update") } else { t!("Installer flake8", "Install flake8") }).clicked() {
                                actions.push(Cmd::InstallFlake8);
                            }
                            install_state(ui, installed(name));
                        }
                        "eslint" => install_state(ui, crate::lsp::in_path("eslint").is_some()),
                        _ => install_state(ui, installed(name)),
                    });
                    let buf = &mut self.prefs.lint_ignore[k];
                    let desc = tf!("Problèmes de {name} à ne pas afficher, séparés par des virgules : un code, un préfixe (E5 : tous les E5xx){}.", "Problems of {name} not to show, separated by commas: a code, a prefix (E5: all the E5xx){}.", if matches!(*name, "pylint" | "eslint") { t!(" ou le nom d'une règle", " or the name of a rule") } else { "" });
                    pref_row(ui, rc, &tf!("Codes ignorés par {name}", "Codes ignored by {name}"), &desc, |ui| {
                        let r = ui.add(egui::TextEdit::singleline(buf).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text(*example));
                        if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            *s.lint_ignore_mut(name) = codes_of(buf);
                        }
                    });
                }

                pref_group(ui, rc, "pyflakes", accent);
                pref_row(ui, rc, "pyflakes", t!("Le dernier recours de « Automatique », seulement les erreurs. Ses messages n'ont pas de code : rien à ignorer (flake8 l'inclut, avec des codes).", "The last resort of “Automatic”, errors only. Its messages have no code: nothing to ignore (flake8 includes it, with codes)."), |ui| {
                    install_state(ui, installed("pyflakes"));
                });
                pref_group(ui, rc, t!("XML et JSON", "XML and JSON"), accent);
                pref_row(ui, rc, t!("XML et JSON", "XML and JSON"), t!("La syntaxe est vérifiée par Florm lui-même, sans rien installer.", "The syntax is checked by Florm itself, with nothing to install."), |ui| {
                    ui.label(RichText::new(t!("Intégré", "Built in")).color(ok()));
                });
            }
            PrefSection::Git => {
                pref_row(ui, rc, t!("Branche et remote dans l'arborescence", "Branch and remote in the tree"), t!("Sur le dossier de chaque dépôt git : sa branche, la branche distante suivie (↑ à pousser, ↓ à récupérer) et le nombre de fichiers modifiés ; survol : les remotes.", "On the folder of each git repository: its branch, the tracked remote branch (↑ to push, ↓ to pull) and the number of changed files; hover: the remotes."), |ui| {
                    toggle_switch(ui, &mut s.tree_git_info);
                });
                pref_row(ui, rc, t!("Diff dans l'éditeur", "Diff in the editor"), t!("Lignes ajoutées en vert et anciennes lignes en rouge directement dans le code (Ctrl+Alt+D).", "Added lines in green and old lines in red right in the code (Ctrl+Alt+D)."), |ui| {
                    toggle_switch(ui, &mut s.inline_diff);
                });
                let mut blame = self.blame_mode;
                pref_row(ui, rc, t!("Blame de la ligne", "Line blame"), t!("Auteur et commit de la ligne courante en fin de ligne (Ctrl+B). Non mémorisé entre les sessions.", "Author and commit of the current line at the end of the line (Ctrl+B). Not kept between sessions."), |ui| {
                    if toggle_switch(ui, &mut blame).changed() {
                        actions.push(Cmd::GitBlame);
                    }
                });
            }
            PrefSection::PullRequests => {
                pref_row(ui, rc, t!("État affiché au départ", "State shown at the start"), t!("Les pull requests affichées à l'ouverture de la vue : ouvertes, fusionnées, fermées ou toutes.", "The pull requests shown when the view opens: open, merged, closed or all."), |ui| {
                    let cur = super::pulls::STATES.iter().find(|(k, _, _)| *k == s.pr_default_state).map_or("", |(_, fr, en)| t!(*fr, *en));
                    egui::ComboBox::from_id_salt("pref_pr_state").selected_text(cur).width(220.0).show_ui(ui, |ui| {
                        for (key, fr, en) in super::pulls::STATES {
                            if ui.selectable_label(s.pr_default_state == key, t!(fr, en)).clicked() {
                                s.pr_default_state = key.to_string();
                            }
                        }
                    });
                });
                pref_row(ui, rc, t!("Conversations résolues", "Resolved conversations"), t!("Affichées dans une pull request, sinon masquées (le bouton « afficher les conversations résolues » reste disponible).", "Shown in a pull request, otherwise hidden (the “show resolved conversations” button stays available)."), |ui| {
                    toggle_switch(ui, &mut s.pr_show_resolved);
                });
                pref_row(ui, rc, t!("Nouvelle PR en brouillon", "New PR as a draft"), t!("La case « Brouillon » est cochée à la création d'une pull request.", "The “Draft” box is checked when a pull request is created."), |ui| {
                    toggle_switch(ui, &mut s.pr_draft);
                });
            }
            PrefSection::Claude => {
                pref_row(ui, rc, t!("Claude en onglet", "Claude in a tab"), t!("Ctrl+Alt+C ouvre Claude dans un onglet de l'éditeur plutôt que dans le panneau du bas (boutons « en onglet » / « en bas » de son en-tête pour changer).", "Ctrl+Alt+C opens Claude in an editor tab rather than in the bottom panel (“in a tab” / “at the bottom” buttons of its header to switch)."), |ui| {
                    toggle_switch(ui, &mut s.claude_in_tab);
                });
                pref_row(ui, rc, t!("Claude a fini : clignoter", "Claude is done: blink"), t!("Quand Claude a fini de répondre et que sa conversation n'est pas affichée, son onglet clignote quelques secondes, puis reste vert jusqu'à ce que tu la regardes.", "When Claude is done answering and its conversation is not shown, its tab blinks a few seconds, then stays green until you look at it."), |ui| {
                    toggle_switch(ui, &mut s.claude_done_blink);
                });
                pref_row(ui, rc, t!("Claude a fini : son", "Claude is done: sound"), t!("Joué à la fin de chaque réponse de Claude. ▶ pour l'écouter.", "Played at the end of each answer of Claude. ▶ to listen to it."), |ui| {
                    let name = |k: &str| crate::sound::SOUNDS.iter().find(|x| x.0 == k).map_or(t!("Aucun", "None"), |x| t!(x.1, x.2));
                    ui.add_enabled_ui(!s.claude_done_sound.is_empty(), |ui| {
                        if ui.small_button("▶").on_hover_text(t!("Écouter", "Listen")).clicked() {
                            crate::sound::play(&s.claude_done_sound);
                        }
                    });
                    egui::ComboBox::from_id_salt("claude_done_sound").selected_text(name(&s.claude_done_sound)).show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.claude_done_sound, String::new(), t!("Aucun", "None"));
                        for (key, fr, en) in crate::sound::SOUNDS {
                            if ui.selectable_value(&mut s.claude_done_sound, key.to_string(), t!(fr, en)).clicked() {
                                crate::sound::play(key);
                            }
                        }
                    });
                });
            }
            PrefSection::Docker => {
                pref_row(ui, rc, t!("Conteneur des tests Odoo", "Odoo tests container"), t!("Le conteneur où tournent les tests Odoo (Tests dans la vue Docker).", "The container the Odoo tests run in (Tests in the Docker view)."), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.odoo_test_container).desired_width(260.0).font(egui::TextStyle::Monospace));
                });
                pref_row_wide(ui, rc, t!("Commande des tests Odoo", "Odoo tests command"), t!("Lancée dans le conteneur, {tags} est remplacé par les tags. Vide : la commande par défaut.", "Run in the container, {tags} is replaced by the tags. Empty: the default command."), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.odoo_test_command).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text(crate::odoo_test::DEFAULT_COMMAND));
                });
            }
            PrefSection::Debug => {
                pref_row(ui, rc, t!("Mode", "Mode"), t!("Lancer : Florm lance le programme sous le débogueur. S'attacher : F5 attend le programme lancé avec debugpy --listen. Dans les deux modes, dès qu'un point d'arrêt est posé, Florm attend tout seul un programme lancé avec debugpy et s'y attache.", "Launch: Florm runs the program under the debugger. Attach: F5 waits for the program started with debugpy --listen. In both modes, as soon as a breakpoint is set, Florm waits by itself for a program started with debugpy and attaches to it."), |ui| {
                    ui.selectable_value(&mut s.debug_mode, "launch".to_string(), t!("Lancer", "Launch"));
                    ui.selectable_value(&mut s.debug_mode, "attach".to_string(), t!("S'attacher", "Attach"));
                });
                pref_row(ui, rc, t!("Port (s'attacher)", "Port (attach)"), t!("Le port sur lequel debugpy écoute (dans le conteneur, pour Docker).", "The port debugpy listens on (in the container, for Docker)."), |ui| {
                    ui.add(egui::DragValue::new(&mut s.debug_port).range(1..=65535));
                });
                pref_row_wide(ui, rc, t!("Commande sur cet ordinateur", "Command on this computer"), t!("Lancée sous le débogueur (lancer), dans le panneau du bas. Vide : l'odoo-bin du dossier.", "Run under the debugger (launch), in the bottom panel. Empty: the odoo-bin of the folder."), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.debug_command).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text("./odoo-bin -c odoo.conf -d ma_base --workers 0"));
                });
                pref_row_wide(ui, rc, t!("Commande dans le conteneur", "Command in the container"), t!("Lancée dans le conteneur sous le débogueur. Vide : la commande par défaut.", "Run in the container under the debugger. Empty: the default command."), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.debug_docker_command).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text(super::debugger::DOCKER_COMMAND));
                });
                pref_row(ui, rc, t!("Python de cet ordinateur", "Python of this computer"), t!("Avec debugpy installé. Vide : python3 (python sous Windows).", "With debugpy installed. Empty: python3 (python on Windows)."), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.debug_python).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text("python3"));
                });
                pref_row(ui, rc, t!("Exceptions", "Exceptions"), t!("Où le débogueur s'arrête sur une exception. Aussi réglable dans la vue Débogage.", "Where the debugger stops on an exception. Also set in the Debugging view."), |ui| {
                    for (key, label) in [("uncaught", t!("non interceptées", "uncaught")), ("raised", t!("toutes", "all raised")), ("userUnhandled", t!("hors de mon code", "leaving my code"))] {
                        let mut on = s.debug_exceptions.iter().any(|e| e == key);
                        if ui.checkbox(&mut on, label).changed() {
                            s.debug_exceptions.retain(|e| e != key);
                            if on {
                                s.debug_exceptions.push(key.to_string());
                            }
                        }
                    }
                });
                pref_row(ui, rc, t!("Mon code seulement", "My code only"), t!("N'entre pas dans le code d'Odoo ni des bibliothèques (justMyCode).", "Does not step into the code of Odoo nor of the libraries (justMyCode)."), |ui| {
                    toggle_switch(ui, &mut s.debug_just_my_code);
                });
            }
            PrefSection::Terminal => {
                let shell = &mut self.prefs.shell;
                pref_row(ui, rc, "Shell", t!("Shell lancé dans le terminal intégré (ex. bash, zsh, fish, powershell). Vide : $SHELL (Linux / macOS) ou cmd (Windows). Pris en compte pour les nouveaux terminaux.", "Shell run in the built-in terminal (e.g. bash, zsh, fish, powershell). Empty: $SHELL (Linux / macOS) or cmd (Windows). Applies to new terminals."), |ui| {
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
                        RichText::new(t!("Clique sur un raccourci puis appuie sur la nouvelle combinaison (Ctrl+K puis une touche pour un enchaînement). Échap annule.", "Click a shortcut then press the new combination (Ctrl+K then a key for a chord). Escape cancels."))
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
                            ui.label(tf!(
                                "« {} » est déjà utilisé par « {} ». L'attribuer à « {} » ?",
                                "“{}” is already used by “{}”. Assign it to “{}”?",
                                keymap::seq_label(seq),
                                other.map(name).unwrap_or_default(),
                                name(*cmd)
                            ));
                            ui.horizontal(|ui| {
                                if ui.button(RichText::new(t!("Remplacer", "Replace")).color(warn())).clicked() {
                                    key_action = Some(KeyAction::Confirm);
                                }
                                if ui.button(t!("Annuler", "Cancel")).clicked() {
                                    key_action = Some(KeyAction::Cancel);
                                }
                            });
                        });
                    ui.add_space(8.0);
                }
                let capturing = self.prefs.capture;
                for (id, label, cmd) in commands::ALL.iter().filter(|(_, l, _)| !l.is_empty()) {
                    let label = &commands::label(id, label);
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
                                    None => t!("Appuyez sur une combinaison…", "Press a combination…").to_string(),
                                },
                                self.hl.colors.accent,
                            ),
                            None if keys.is_empty() => (t!("Ajouter", "Add").to_string(), ui.visuals().weak_text_color()),
                            None => (keys.clone(), ui.visuals().strong_text_color()),
                        };
                        let chip = egui::Button::new(RichText::new(text).monospace().color(color))
                            .fill(ui.visuals().widgets.inactive.bg_fill)
                            .min_size(egui::vec2(210.0, 24.0));
                        if ui.add(chip).on_hover_text(t!("Cliquer pour changer", "Click to change")).clicked() {
                            key_action = Some(KeyAction::Capture(*cmd));
                        }
                        if !keys.is_empty() && ui.button("×").on_hover_text(t!("Supprimer ce raccourci", "Remove this shortcut")).clicked() {
                            key_action = Some(KeyAction::Unbind(*cmd));
                        }
                        if customized {
                            if ui.button("↺").on_hover_text(t!("Revenir au raccourci par défaut", "Back to the default shortcut")).clicked() {
                                key_action = Some(KeyAction::Reset(*cmd));
                            }
                            ui.label(RichText::new(t!("modifié", "changed")).small().color(self.hl.colors.accent));
                        }
                        ui.label(RichText::new(*id).weak().small());
                    });
                }
                if let Some(a) = key_action {
                    self.prefs.key_action = Some(a);
                }
            }
            PrefSection::Templates => {
                if rc.matches(&["Modèles", "modèle", "template", "snippet", "déclencheur", "trigger"]) {
                    rc.header(ui);
                    if self.templates.is_empty() {
                        self.templates = crate::templates::load_all();
                    }
                    let accent = self.hl.colors.accent;
                    let act = template_edit::section(ui, &self.templates, &mut self.prefs.template, accent);
                    let mut reload = false;
                    match act {
                        Some(template_edit::TplAct::Edit(k)) => self.prefs.template = self.templates.get(k).map(template_edit::TemplateDraft::of),
                        Some(template_edit::TplAct::New) => self.prefs.template = Some(template_edit::TemplateDraft::new()),
                        Some(template_edit::TplAct::Save) => {
                            if let Some(d) = self.prefs.template.as_mut() {
                                d.error = d.save().err();
                                reload = d.error.is_none();
                            }
                        }
                        Some(template_edit::TplAct::Delete) => {
                            if let Some(d) = self.prefs.template.take() {
                                if let Err(e) = d.delete() {
                                    self.log(tf!("Modèle non supprimé : {e}", "Template not deleted: {e}"));
                                }
                                reload = true;
                            }
                        }
                        Some(template_edit::TplAct::Cancel) => self.prefs.template = None,
                        None => {}
                    }
                    if reload {
                        self.templates = crate::templates::load_all();
                    }
                }
            }
            PrefSection::Advanced => {
                let exclude = &mut self.prefs.exclude;
                pref_row_wide(
                    ui,
                    rc,
                    t!("Fichiers exclus", "Excluded files"),
                    t!("Ni dans « aller au fichier » (Ctrl+P), ni dans l'index des symboles, ni dans la recherche. Séparés par des virgules, syntaxe .gitignore (dossier/ ou *.ext). Le .gitignore des dépôts et .git sont toujours appliqués.", "Not in “go to file” (Ctrl+P), nor in the symbol index, nor in the search. Separated by commas, .gitignore syntax (folder/ or *.ext). The .gitignore of the repositories and .git always apply."),
                    |ui| {
                        let r = ui.add(egui::TextEdit::singleline(exclude).desired_width(320.0).font(egui::TextStyle::Monospace).hint_text("__pycache__/, *.pyc"));
                        if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            s.exclude = exclude.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
                        }
                    },
                );
                pref_row(ui, rc, t!("Taille de l'index des symboles", "Size of the symbol index"), t!("Fichiers source lus au plus (définitions F12, survol, complétion). Les dépôts des fichiers ouverts passent en premier ; plus grand = plus de mémoire et un chargement plus long.", "Most source files read (F12 definitions, hover, completion). The repositories of the open files come first; bigger = more memory and a longer load."), |ui| {
                    // fixed choices: each change builds the index again
                    let label = |n: usize| tf!("{} fichiers", "{} files", n.to_string().as_bytes().rchunks(3).rev().map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join(t!(" ", ",")));
                    egui::ComboBox::from_id_salt("pref_index_max").selected_text(label(s.index_max_files)).show_ui(ui, |ui| {
                        for n in [50_000, 150_000, 300_000, 600_000, 1_500_000] {
                            ui.selectable_value(&mut s.index_max_files, n, label(n));
                        }
                    });
                });
                pref_row(ui, rc, t!("Chercher les mises à jour", "Check for updates"), &tf!("Au démarrage, Florm regarde s'il existe une version plus récente que la {} sur GitHub et la propose en bas à droite.", "At startup, Florm checks GitHub for a version newer than {} and offers it at the bottom right.", crate::update::current()), |ui| {
                    toggle_switch(ui, &mut s.check_updates);
                });
                let shared = &mut self.prefs.shared;
                let shared_dir = crate::shared::dir();
                let busy = self.shared_busy;
                let mut shared_op = None;
                let mut open_shared = false;
                let help = match &shared_dir {
                    Some(d) => tf!("Récupérée dans {}. Publier envoie les changements (préférences, modèles, thèmes) sur GitHub.", "Fetched in {}. Publish sends the changes (preferences, templates, themes) to GitHub.", d.display()),
                    None => t!("Un dépôt GitHub avec settings.toml, templates/, themes/ et plugins/, utilisé sur chaque ordinateur : « login » (= login/florm-config), « owner/repo » ou une adresse git. « Créer » le fait sur GitHub (privé, avec gh) à partir de cet ordinateur.", "A GitHub repository with settings.toml, templates/, themes/ and plugins/, used on every computer: “login” (= login/florm-config), “owner/repo” or a git address. “Create” makes it on GitHub (private, with gh) from this computer.").to_string(),
                };
                pref_row_wide(ui, rc, t!("Configuration partagée", "Shared configuration"), &help, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(shared).desired_width(220.0).font(egui::TextStyle::Monospace).hint_text("login/florm-config"));
                    if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        s.shared_config = shared.trim().to_string();
                    }
                    ui.add_enabled_ui(!busy, |ui| {
                        if shared_dir.is_some() {
                            if ui.button(t!("Synchroniser", "Synchronize")).clicked() {
                                shared_op = Some(crate::shared::Op::Sync);
                            }
                            if ui.button(t!("Publier", "Publish")).clicked() {
                                shared_op = Some(crate::shared::Op::Publish);
                            }
                            if ui.button(t!("Ouvrir le dossier", "Open the folder")).clicked() {
                                open_shared = true;
                            }
                        } else if ui.button(t!("Créer sur GitHub", "Create on GitHub")).on_hover_text(t!("Nouveau dépôt privé avec les préférences, modèles, thèmes et plugins de cet ordinateur", "New private repository with the preferences, templates, themes and plugins of this computer")).clicked() {
                            s.shared_config = shared.trim().to_string();
                            shared_op = Some(crate::shared::Op::Create);
                        }
                    });
                    if busy {
                        ui.spinner();
                    }
                });
                if let Some(op) = shared_op {
                    self.shared_op = Some(op);
                }
                if open_shared {
                    if let Some(d) = crate::shared::dir() {
                        let p = if d.join("settings.toml").is_file() { d.join("settings.toml") } else { d.join("README.md") };
                        self.open_path(p);
                    }
                }
                pref_row(ui, rc, t!("Fichier de préférences", "Preferences file"), &config::settings_path().display().to_string(), |ui| {
                    if ui.button(t!("Ouvrir settings.toml", "Open settings.toml")).clicked() {
                        actions.push(Cmd::OpenSettingsFile);
                    }
                });
                pref_row(ui, rc, "Plugins", &config::plugins_dir().display().to_string(), |ui| {
                    if ui.button(t!("Recharger", "Reload")).clicked() {
                        actions.push(Cmd::ReloadPlugins);
                    }
                    if ui.button(t!("Ouvrir le dossier", "Open the folder")).clicked() {
                        actions.push(Cmd::OpenPluginsFolder);
                    }
                });
                pref_row(ui, rc, t!("Historique de recherche", "Search history"), t!("Recherches et filtres mémorisés.", "Remembered searches and filters."), |ui| {
                    if ui.button(t!("Effacer", "Clear")).clicked() {
                        actions.push(Cmd::ClearSearchHistory);
                    }
                });
            }
        }
        if let Some(n) = note {
            self.log(n);
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
        commands::ALL.iter().find(|(_, _, c)| *c == cmd).map(|(id, l, _)| commands::label(id, l).to_string()).unwrap_or_else(|| format!("{cmd:?}"))
    }

    fn write_key(&mut self, key: String, value: Option<String>) {
        if let Err(e) = config::set_key(&key, value.as_deref()) {
            self.log(tf!("Raccourci non enregistré : {e}", "Shortcut not saved: {e}"));
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
            Some(other) if other == cmd => self.set_status(t!("C'est déjà ce raccourci", "That is already the shortcut")),
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
        self.set_status(tf!("Raccourci {} enregistré", "Shortcut {} saved", keymap::seq_label(&seq)));
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
        self.set_status(t!("Raccourci par défaut rétabli", "Default shortcut restored"));
    }

    /// Saves changed settings to settings.toml and applies their effects right away.
    pub(super) fn apply_settings_change(&mut self, before: &Settings) {
        let s = self.settings.clone();
        if s.language != before.language {
            crate::i18n::set(crate::i18n::from_code(&s.language));
            self.persist("language", config::toml_str(&s.language));
        }
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
            ("word_wrap", s.word_wrap, before.word_wrap),
            ("sticky_scroll", s.sticky_scroll, before.sticky_scroll),
            ("breadcrumbs", s.breadcrumbs, before.breadcrumbs),
            ("show_menu_bar", s.show_menu_bar, before.show_menu_bar),
            ("search_in_tab", s.search_in_tab, before.search_in_tab),
            ("claude_in_tab", s.claude_in_tab, before.claude_in_tab),
            ("claude_done_blink", s.claude_done_blink, before.claude_done_blink),
            ("tree_git_info", s.tree_git_info, before.tree_git_info),
            ("auto_pair", s.auto_pair, before.auto_pair),
            ("trim_trailing_whitespace_on_save", s.trim_trailing_whitespace_on_save, before.trim_trailing_whitespace_on_save),
            ("hot_exit", s.hot_exit, before.hot_exit),
            ("lsp", s.lsp, before.lsp),
            ("lint_live", s.lint_live, before.lint_live),
            ("lint_on_save", s.lint_on_save, before.lint_on_save),
            ("inline_diff", s.inline_diff, before.inline_diff),
            ("check_updates", s.check_updates, before.check_updates),
        ];
        for (key, now, was) in bools {
            if now != was {
                self.persist(key, now.to_string());
            }
        }
        let mut lint_changed = s.python_linter != before.python_linter || s.python_lint_args != before.python_lint_args || s.lint_live != before.lint_live;
        if s.claude_done_sound != before.claude_done_sound {
            self.persist("claude_done_sound", config::toml_str(&s.claude_done_sound));
        }
        if s.odoo_test_container != before.odoo_test_container {
            self.persist("odoo_test_container", config::toml_str(&s.odoo_test_container));
        }
        if s.odoo_test_command != before.odoo_test_command {
            self.persist("odoo_test_command", config::toml_str(&s.odoo_test_command));
        }
        if s.debug_docker_command != before.debug_docker_command {
            self.persist("debug_docker_command", config::toml_str(&s.debug_docker_command));
        }
        if s.debug_command != before.debug_command {
            self.persist("debug_command", config::toml_str(&s.debug_command));
        }
        if s.debug_python != before.debug_python {
            self.persist("debug_python", config::toml_str(&s.debug_python));
        }
        if s.debug_mode != before.debug_mode {
            self.persist("debug_mode", config::toml_str(&s.debug_mode));
        }
        if s.debug_port != before.debug_port {
            self.persist("debug_port", s.debug_port.to_string());
        }
        if s.debug_exceptions != before.debug_exceptions {
            self.persist("debug_exceptions", config::toml_str_array(&s.debug_exceptions));
        }
        if s.debug_just_my_code != before.debug_just_my_code {
            self.persist("debug_just_my_code", s.debug_just_my_code.to_string());
        }
        if s.pr_default_state != before.pr_default_state || s.pr_show_resolved != before.pr_show_resolved {
            self.pr.apply_settings(&s.pr_default_state, s.pr_show_resolved);
            self.reload_prs();
        }
        if s.pr_default_state != before.pr_default_state {
            self.persist("pr_default_state", config::toml_str(&s.pr_default_state));
        }
        if s.pr_show_resolved != before.pr_show_resolved {
            self.persist("pr_show_resolved", s.pr_show_resolved.to_string());
        }
        if s.pr_draft != before.pr_draft {
            self.persist("pr_draft", s.pr_draft.to_string());
        }
        if s.python_linter != before.python_linter {
            self.persist("python_linter", config::toml_str(&s.python_linter));
        }
        if s.python_lint_args != before.python_lint_args {
            self.persist("python_lint_args", config::toml_str_array(&s.python_lint_args));
        }
        let ignored = [
            ("lint_ignore_flake8", &s.lint_ignore_flake8, &before.lint_ignore_flake8),
            ("lint_ignore_ruff", &s.lint_ignore_ruff, &before.lint_ignore_ruff),
            ("lint_ignore_pylint", &s.lint_ignore_pylint, &before.lint_ignore_pylint),
            ("lint_ignore_eslint", &s.lint_ignore_eslint, &before.lint_ignore_eslint),
        ];
        for (key, now, was) in ignored {
            if now != was {
                self.persist(key, config::toml_str_array(now));
                lint_changed = true;
            }
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
        if s.rulers != before.rulers {
            self.persist("rulers", format!("[{}]", s.rulers.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ")));
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
        if s.shared_config != before.shared_config {
            self.persist("shared_config", config::toml_str(&s.shared_config));
            if s.shared_config.is_empty() {
                self.reload_shared();
            } else if self.shared_op.is_none() {
                self.shared_op = Some(crate::shared::Op::Sync);
            }
        }
        if let Some(op) = self.shared_op.take() {
            self.shared_job(op);
        }
    }
}
