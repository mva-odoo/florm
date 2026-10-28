//! Préférences → Modèles: the templates (built-in and one's own) listed, and one edited: its
//! name (a word is also its trigger), the files it is for, its text. Saved as a file of the
//! templates folder (`name.ext`), the format read by templates.rs.

use super::*;
use crate::templates::{self, Template};

/// Extensions offered: (extension, label); "" for any file.
/// "Tous les fichiers" in the language of the interface; the other labels are names.
fn lang_label(l: &'static str) -> &'static str {
    if l.is_empty() || l == LANGS[0].1 {
        t!("Tous les fichiers", "All files")
    } else {
        l
    }
}

const LANGS: [(&str, &str); 12] = [
    ("", "Tous les fichiers"),
    ("py", "Python"),
    ("xml", "XML"),
    ("js", "JavaScript"),
    ("ts", "TypeScript"),
    ("json", "JSON"),
    ("html", "HTML"),
    ("css", "CSS"),
    ("scss", "SCSS"),
    ("md", "Markdown"),
    ("csv", "CSV"),
    ("sh", "Shell"),
];

pub(super) struct TemplateDraft {
    /// File of the template being changed (None: a new one, or a built-in to copy).
    orig: Option<PathBuf>,
    /// Copied from a built-in one (saving it makes one's own that replaces it).
    from_builtin: bool,
    name: String,
    ext: String,
    body: String,
    pub(super) error: Option<String>,
}

/// What the section asked for during the frame.
pub(super) enum TplAct {
    Edit(usize),
    New,
    Save,
    Delete,
    Cancel,
}

impl TemplateDraft {
    pub(super) fn of(t: &Template) -> Self {
        TemplateDraft { orig: t.path.clone(), from_builtin: t.path.is_none(), name: t.name.clone(), ext: t.ext.clone(), body: t.body.clone(), error: None }
    }

    pub(super) fn new() -> Self {
        TemplateDraft { orig: None, from_builtin: false, name: String::new(), ext: "py".into(), body: String::new(), error: None }
    }

    /// The file it goes to, in templates folder `dir`.
    fn path_in(&self, dir: &Path) -> PathBuf {
        let name = self.name.trim();
        dir.join(if self.ext.is_empty() { name.to_string() } else { format!("{name}.{}", self.ext) })
    }

    /// Writes it (the old file goes when it was renamed); the error to show otherwise.
    pub(super) fn save(&mut self) -> Result<(), String> {
        self.save_in(&templates::dir())
    }

    fn save_in(&mut self, dir: &Path) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') || name == "README" {
            return Err(t!("Le nom ne peut pas être vide, commencer par un point ni contenir / ou \\", "The name cannot be empty, start with a dot or contain / or \\").into());
        }
        let path = self.path_in(dir);
        if self.orig.as_ref() != Some(&path) && path.exists() {
            return Err(tf!("Un modèle « {} » existe déjà", "A template “{}” already exists", path.file_name().unwrap_or_default().to_string_lossy()));
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(&path, &self.body).map_err(|e| e.to_string())?;
        if let Some(old) = self.orig.as_ref().filter(|o| **o != path) {
            let _ = std::fs::remove_file(old);
        }
        self.orig = Some(path);
        self.from_builtin = false;
        Ok(())
    }

    pub(super) fn delete(&self) -> Result<(), String> {
        match &self.orig {
            Some(p) => std::fs::remove_file(p).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// Name, files, text and buttons.
    fn ui(&mut self, ui: &mut egui::Ui, accent: Color32, act: &mut Option<TplAct>) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if self.from_builtin {
                ui.label(RichText::new(t!("Modèle intégré : l'enregistrer en fait le tien, qui le remplace (même nom ou même déclencheur).", "Built-in template: saving it makes it yours, replacing it (same name or same trigger).")).small().weak());
            }
            egui::Grid::new("template_fields").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(RichText::new(t!("Nom", "Name")).strong());
                crate::input::add(ui, egui::TextEdit::singleline(&mut self.name).hint_text(t!("ipdb, Mon en-tête…", "ipdb, My header…")).desired_width(320.0));
                ui.end_row();
                ui.label("");
                let trigger = self.name.trim();
                let word = trigger.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && trigger.chars().all(|c| c.is_alphanumeric() || c == '_');
                let hint = if word {
                    tf!("Déclencheur : tape « {trigger} » puis Tab (ou choisis-le dans les suggestions)", "Trigger: type “{trigger}” then Tab (or choose it in the suggestions)")
                } else {
                    t!("Avec des espaces : seulement dans la liste (Ctrl+Alt+N). Un seul mot devient aussi un déclencheur.", "With spaces: only in the list (Ctrl+Alt+N). A single word also becomes a trigger.").to_string()
                };
                ui.label(RichText::new(hint).small().weak());
                ui.end_row();
                ui.label(RichText::new(t!("Fichiers", "Files")).strong());
                let label = LANGS.iter().find(|(e, _)| *e == self.ext).map_or_else(|| format!(".{}", self.ext), |(_, l)| lang_label(l).to_string());
                egui::ComboBox::from_id_salt("template_ext").selected_text(label).width(200.0).show_ui(ui, |ui| {
                    for (e, l) in LANGS {
                        ui.selectable_value(&mut self.ext, e.to_string(), lang_label(l));
                    }
                });
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(RichText::new(t!("Texte", "Text")).strong());
            ui.add(egui::TextEdit::multiline(&mut self.body).code_editor().desired_rows(14).desired_width(f32::INFINITY));
            ui.label(
                RichText::new(t!("Variables remplies d'après le fichier : ${model} (sale.order), ${model_} (sale_order), ${Model} (SaleOrder), ${name} (Sale Order), ${module} (le module Odoo) ; $0 : où va le curseur.", "Variables filled from the file: ${model} (sale.order), ${model_} (sale_order), ${Model} (SaleOrder), ${name} (Sale Order), ${module} (the Odoo module); $0: where the cursor goes."))
                    .small()
                    .weak(),
            );
            if let Some(e) = &self.error {
                ui.label(RichText::new(e).color(err()));
            }
            ui.horizontal(|ui| {
                if ui.add(filled_button(t!("Enregistrer", "Save"), accent)).on_hover_text(tf!("Écrit {}", "Writes {}", self.path_in(&templates::dir()).display())).clicked() {
                    *act = Some(TplAct::Save);
                }
                if ui.button(t!("Fermer", "Close")).clicked() {
                    *act = Some(TplAct::Cancel);
                }
                if self.orig.is_some() && ui.button(RichText::new(t!("Supprimer", "Delete")).color(err())).on_hover_text(t!("Supprime son fichier", "Deletes its file")).clicked() {
                    *act = Some(TplAct::Delete);
                }
            });
        });
    }
}

/// The list of the templates, then the one being edited.
pub(super) fn section(ui: &mut egui::Ui, all: &[Template], draft: &mut Option<TemplateDraft>, accent: Color32) -> Option<TplAct> {
    let mut act = None;
    ui.horizontal(|ui| {
        if ui.add(filled_button(t!("+ Nouveau modèle", "+ New template"), accent)).clicked() {
            act = Some(TplAct::New);
        }
        ui.label(RichText::new(tf!("Dans {}", "In {}", templates::dir().display())).small().weak());
    });
    ui.add_space(8.0);
    let row = |ui: &mut egui::Ui, k: usize, t: &Template, act: &mut Option<TplAct>| {
        ui.horizontal(|ui| {
            if ui.link(&t.name).clicked() {
                *act = Some(TplAct::Edit(k));
            }
            if !t.trigger.is_empty() && t.trigger != t.name {
                ui.label(RichText::new(&t.trigger).monospace().small().color(accent));
            }
            let ext = if t.ext.is_empty() { "tous".to_string() } else { format!(".{}", t.ext) };
            ui.label(RichText::new(ext).small().weak());
            let first = t.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
            ui.label(RichText::new(first.chars().take(60).collect::<String>()).monospace().small().weak());
        });
    };
    let mine: Vec<(usize, &Template)> = all.iter().enumerate().filter(|(_, t)| t.path.is_some()).collect();
    ui.label(RichText::new(t!("LES TIENS", "YOURS")).small().strong().color(accent));
    if mine.is_empty() {
        ui.label(RichText::new(t!("Aucun pour l'instant : « + Nouveau modèle », ou un intégré à modifier.", "None yet: “+ New template”, or a built-in one to change.")).weak());
    }
    for (k, t) in mine {
        row(ui, k, t, &mut act);
    }
    ui.add_space(8.0);
    ui.label(RichText::new(t!("INTÉGRÉS", "BUILT IN")).small().strong().color(accent));
    for (k, t) in all.iter().enumerate().filter(|(_, t)| t.path.is_none()) {
        row(ui, k, t, &mut act);
    }
    if let Some(d) = draft.as_mut() {
        ui.add_space(12.0);
        d.ui(ui, accent, &mut act);
    }
    act
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_saved_renamed_and_deleted() {
        let dir = std::env::temp_dir().join(format!("florm_tpl_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut d = TemplateDraft::new();
        d.name = "logme".into();
        d.body = "_logger.info($0)".into();
        d.save_in(&dir).unwrap();
        let all = templates::load(&dir);
        let t = all.iter().find(|t| t.name == "logme").unwrap();
        assert_eq!((t.ext.as_str(), t.trigger.as_str(), t.body.as_str()), ("py", "logme", "_logger.info($0)"));
        // renamed: the old file goes
        let mut d = TemplateDraft::of(t);
        d.name = "log_info".into();
        d.save_in(&dir).unwrap();
        let names: Vec<String> = templates::load(&dir).into_iter().filter(|t| t.path.is_some()).map(|t| t.name).collect();
        assert_eq!(names, ["log_info"]);
        assert!(TemplateDraft { name: "a/b".into(), ..TemplateDraft::new() }.save_in(&dir).is_err());
        d.delete().unwrap();
        assert!(templates::load(&dir).iter().all(|t| t.path.is_none()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
