//! A theme made in the preferences: the colors of the interface and of the code, starting from
//! the current theme, shown live, saved as a .tmTheme in the "themes" folder of the configuration.

use super::*;

/// Colors of the editor: (key in the .tmTheme, label).
const UI: [(&str, &str); 10] = [
    ("background", "Fond"),
    ("foreground", "Texte"),
    ("accent", "Accent de l'interface"),
    ("caret", "Curseur"),
    ("selection", "Sélection"),
    ("lineHighlight", "Ligne du curseur"),
    ("gutter", "Marge des numéros"),
    ("gutterForeground", "Numéros de ligne"),
    ("findHighlight", "Résultats de recherche"),
    ("guide", "Guides d'indentation"),
];

/// Colors of the code: (label, scopes colored, scope whose color is read from the theme).
const SYNTAX: [(&str, &str, &str); 14] = [
    ("Commentaires", "comment, punctuation.definition.comment", "comment.line"),
    ("Mots-clés", "keyword, storage, keyword.operator.logical, keyword.operator.word", "keyword.control"),
    ("Opérateurs", "keyword.operator, punctuation.separator, punctuation.accessor", "keyword.operator"),
    ("Chaînes", "string, punctuation.definition.string", "string.quoted"),
    ("Nombres et constantes", "constant.numeric, constant.language, constant.other, support.constant", "constant.numeric"),
    ("Fonctions", "entity.name.function, support.function, meta.function-call.generic, variable.function", "entity.name.function"),
    ("Classes et types", "entity.name.class, entity.name.type, support.class, support.type, entity.other.inherited-class, storage.type.class", "entity.name.class"),
    ("Paramètres", "variable.parameter", "variable.parameter"),
    ("self / this", "variable.language", "variable.language"),
    ("Décorateurs", "entity.name.function.decorator, meta.decorator, punctuation.definition.decorator", "meta.decorator"),
    ("Balises (XML, HTML)", "entity.name.tag, punctuation.definition.tag", "entity.name.tag"),
    ("Attributs", "entity.other.attribute-name", "entity.other.attribute-name"),
    ("Titres (Markdown)", "markup.heading, entity.name.section", "markup.heading"),
    ("Lignes ajoutées / supprimées (diff)", "markup.inserted", "markup.inserted"),
];

/// A label of `UI` or `SYNTAX` in the language of the interface.
fn label(fr: &'static str) -> &'static str {
    if !crate::i18n::en() {
        return fr;
    }
    match fr {
        "Fond" => "Background",
        "Texte" => "Text",
        "Accent de l'interface" => "Interface accent",
        "Curseur" => "Cursor",
        "Sélection" => "Selection",
        "Ligne du curseur" => "Cursor line",
        "Marge des numéros" => "Line number gutter",
        "Numéros de ligne" => "Line numbers",
        "Résultats de recherche" => "Search results",
        "Guides d'indentation" => "Indentation guides",
        "Commentaires" => "Comments",
        "Mots-clés" => "Keywords",
        "Opérateurs" => "Operators",
        "Chaînes" => "Strings",
        "Nombres et constantes" => "Numbers and constants",
        "Fonctions" => "Functions",
        "Classes et types" => "Classes and types",
        "Paramètres" => "Parameters",
        "Décorateurs" => "Decorators",
        "Balises (XML, HTML)" => "Tags (XML, HTML)",
        "Attributs" => "Attributes",
        "Titres (Markdown)" => "Headings (Markdown)",
        "Lignes ajoutées / supprimées (diff)" => "Added / removed lines (diff)",
        _ => fr,
    }
}

pub(super) struct ThemeDraft {
    pub(super) name: String,
    ui: Vec<Color32>,
    /// Color and italic of each kind of code.
    syntax: Vec<(Color32, bool)>,
    /// Color of the removed lines of a diff.
    deleted: Color32,
}

/// What the editor asked for during the frame.
pub(super) enum ThemeAct {
    Changed,
    Save,
    Cancel,
}

impl ThemeDraft {
    /// The colors of the current theme, under a new name (the same when it is already a theme of
    /// the user: it is edited).
    pub(super) fn from_current(hl: &Highlighting, user_theme: bool) -> Self {
        let s = &hl.theme.settings;
        let c = hl.colors;
        let conv = |o: Option<syntect::highlighting::Color>, d: Color32| o.map_or(d, |x| Color32::from_rgb(x.r, x.g, x.b));
        let ui = vec![
            conv(s.background, c.bg),
            conv(s.foreground, c.fg),
            conv(s.accent, c.accent),
            conv(s.caret, c.caret),
            conv(s.selection, c.selection),
            conv(s.line_highlight, c.line_hl),
            conv(s.gutter, c.gutter_bg),
            conv(s.gutter_foreground, c.gutter_fg),
            conv(s.find_highlight, c.find),
            conv(s.guide, c.guide),
        ];
        let syntax = SYNTAX.iter().map(|(_, _, probe)| hl.scope_style(probe)).collect();
        let name = if user_theme { hl.theme_name.clone() } else { format!("{} (perso)", hl.theme_name) };
        ThemeDraft { name, ui, syntax, deleted: hl.scope_style("markup.deleted").0 }
    }

    /// The theme as a .tmTheme (a plist).
    pub(super) fn to_tmtheme(&self) -> String {
        let hex = |c: Color32| format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let mut out = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n",
        );
        out.push_str(&format!("  <key>name</key><string>{}</string>\n  <key>settings</key>\n  <array>\n    <dict><key>settings</key><dict>\n", esc(&self.name)));
        for ((key, _), c) in UI.iter().zip(&self.ui) {
            out.push_str(&format!("      <key>{key}</key><string>{}</string>\n", hex(*c)));
        }
        out.push_str("    </dict></dict>\n");
        let mut item = |name: &str, scope: &str, c: Color32, italic: bool| {
            let style = if italic { "<key>fontStyle</key><string>italic</string>" } else { "" };
            out.push_str(&format!(
                "    <dict><key>name</key><string>{}</string><key>scope</key><string>{scope}</string>\n      <key>settings</key><dict><key>foreground</key><string>{}</string>{style}</dict></dict>\n",
                esc(name),
                hex(c)
            ));
        };
        for ((label, scope, _), (c, italic)) in SYNTAX.iter().zip(&self.syntax) {
            item(label, scope, *c, *italic);
        }
        item("Diff deleted", "markup.deleted", self.deleted, false);
        item("Invalid", "invalid", self.deleted, false);
        out.push_str("  </array>\n</dict>\n</plist>\n");
        out
    }

    /// The file it is saved to.
    pub(super) fn path(&self) -> PathBuf {
        config::themes_dir().join(format!("{}.tmTheme", self.name.replace(['/', '\\'], "-")))
    }

    /// Name, colors, buttons.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, builtin: &[String], accent: Color32) -> Option<ThemeAct> {
        let mut act = None;
        let picker = |ui: &mut egui::Ui, c: &mut Color32| egui::color_picker::color_edit_button_srgba(ui, c, egui::color_picker::Alpha::Opaque).changed();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("Nom du thème", "Theme name")).strong());
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(260.0));
            });
            let taken = builtin.iter().any(|b| *b == self.name);
            if taken {
                ui.label(RichText::new(t!("Ce nom est celui d'un thème intégré : choisis-en un autre", "This name is the one of a built-in theme: choose another one")).small().color(warn()));
            }
            ui.add_space(6.0);
            ui.columns(2, |cols| {
                cols[0].label(RichText::new(t!("ÉDITEUR ET INTERFACE", "EDITOR AND INTERFACE")).small().strong().color(accent));
                egui::Grid::new("theme_ui_colors").num_columns(2).spacing([12.0, 6.0]).show(&mut cols[0], |ui| {
                    for ((_, fr), c) in UI.iter().zip(self.ui.iter_mut()) {
                        ui.label(label(fr));
                        if picker(ui, c) {
                            act = Some(ThemeAct::Changed);
                        }
                        ui.end_row();
                    }
                });
                cols[1].label(RichText::new("CODE").small().strong().color(accent));
                egui::Grid::new("theme_code_colors").num_columns(3).spacing([12.0, 6.0]).show(&mut cols[1], |ui| {
                    for ((fr, _, _), (c, italic)) in SYNTAX.iter().zip(self.syntax.iter_mut()) {
                        ui.label(label(fr));
                        if picker(ui, c) {
                            act = Some(ThemeAct::Changed);
                        }
                        if ui.checkbox(italic, RichText::new(t!("italique", "italic")).small()).changed() {
                            act = Some(ThemeAct::Changed);
                        }
                        ui.end_row();
                    }
                    ui.label(t!("Lignes supprimées (diff), erreurs", "Removed lines (diff), errors"));
                    if picker(ui, &mut self.deleted) {
                        act = Some(ThemeAct::Changed);
                    }
                    ui.end_row();
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let ok_name = !self.name.trim().is_empty() && !taken;
                let tip = tf!("Écrit {}", "Writes {}", self.path().display());
                if ui.add_enabled(ok_name, filled_button(t!("Enregistrer le thème", "Save the theme"), accent)).on_hover_text(tip).clicked() {
                    act = Some(ThemeAct::Save);
                }
                if ui.button(t!("Annuler", "Cancel")).on_hover_text(t!("Revenir au thème d'avant", "Back to the previous theme")).clicked() {
                    act = Some(ThemeAct::Cancel);
                }
                ui.label(RichText::new(t!("Les couleurs s'appliquent tout de suite, pour voir le résultat.", "The colors apply right away, to see the result.")).small().weak());
            });
        });
        act
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_made_here_loads_back() {
        let mut hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let mut d = ThemeDraft::from_current(&hl, false);
        assert_eq!(d.name, "Florm (perso)");
        d.ui[0] = Color32::from_rgb(1, 2, 3);
        d.syntax[0] = (Color32::from_rgb(200, 10, 20), true);
        hl.add_theme_xml(&d.name, &d.to_tmtheme()).unwrap();
        assert!(hl.set_theme("Florm (perso)"));
        assert_eq!(hl.colors.bg, Color32::from_rgb(1, 2, 3));
        assert_eq!(hl.scope_style("comment.line"), (Color32::from_rgb(200, 10, 20), true));
        // the same colors read back from it
        let again = ThemeDraft::from_current(&hl, true);
        assert_eq!((again.name.as_str(), again.ui[0], again.syntax[0]), ("Florm (perso)", d.ui[0], d.syntax[0]));
    }
}
