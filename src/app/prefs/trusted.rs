//! Préférences → Langages → Dossiers de confiance: the folders trusted, each with a × to take its
//! trust back.

use super::*;

impl App {
    pub(super) fn trusted_folders_prefs(&mut self, ui: &mut egui::Ui, rc: &mut RowCtx) {
        let accent = self.hl.colors.accent;
        let title = t!("Dossiers de confiance", "Trusted folders");
        let desc = t!(
            "Git, les serveurs de langage et les linters configurés par le projet (plugins flake8, eslint) ne tournent que dans ces dossiers (et ceux qu'ils contiennent) : ils peuvent exécuter son code. × retire la confiance : les fichiers ouverts de ce dossier repassent en mode restreint.",
            "Git, the language servers and the linters set up by the project (flake8 plugins, eslint) only run in these folders (and those inside): they may run its code. × takes the trust back: the open files of that folder go back to restricted mode."
        );
        if !rc.matches(&[title, desc, t!("confiance", "trust"), t!("restreint", "restricted")]) {
            return;
        }
        pref_group(ui, rc, title, accent);
        let mut remove: Option<String> = None;
        let folders = self.settings.trusted_folders.clone();
        pref_row(ui, rc, title, desc, |ui| {
            ui.vertical(|ui| {
                if folders.iter().all(|f| f.trim().is_empty()) {
                    ui.label(RichText::new(t!("Aucun pour l'instant : la barre « 🔒 Mode restreint » au-dessus d'un fichier en ajoute un.", "None yet: the “🔒 Restricted mode” bar above a file adds one.")).weak());
                }
                for f in folders.iter().filter(|f| !f.trim().is_empty()) {
                    ui.horizontal(|ui| {
                        if ui.small_button("×").on_hover_text(t!("Ne plus faire confiance à ce dossier", "No longer trust this folder")).clicked() {
                            remove = Some(f.clone());
                        }
                        ui.label(RichText::new(f).monospace());
                    });
                }
            });
        });
        if let Some(f) = remove {
            self.untrust_folder(&f);
        }
    }
}
