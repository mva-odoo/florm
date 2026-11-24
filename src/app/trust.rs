//! Trusted folders: the language servers and the linters set up by the project (flake8 with its
//! plugins, eslint) may run its code, so they only run in a folder the developer trusted. Elsewhere
//! a bar above the file offers to trust it (restricted mode).

use super::*;

impl App {
    /// The folder trusted or not for `path`: the folder open when the file is in it, else its
    /// repository, else its folder.
    pub(super) fn trust_root(&self, path: &Path) -> Option<PathBuf> {
        self.root.clone().filter(|r| path.starts_with(r)).or_else(|| self.repo_for(path)).or_else(|| path.parent().map(Path::to_path_buf))
    }

    /// `path` is in a trusted folder (settings `trusted_folders`).
    pub(super) fn trusted(&self, path: &Path) -> bool {
        trusted_in(&self.settings.trusted_folders, path)
    }

    /// `dir` trusted from now on (written in settings.toml): its open files are linted again,
    /// their language servers start.
    fn trust_folder(&mut self, dir: PathBuf) {
        let s = dir.to_string_lossy().into_owned();
        if !self.settings.trusted_folders.contains(&s) {
            self.settings.trusted_folders.push(s);
        }
        // a JSON list of strings is a TOML list too
        let list = serde_json::to_string(&self.settings.trusted_folders).unwrap_or_else(|_| "[]".into());
        self.persist("trusted_folders", list);
        for i in 0..self.docs.len() {
            if self.docs[i].path.as_deref().is_some_and(|p| p.starts_with(&dir)) {
                self.analyze(i, false);
            }
        }
        self.set_status(tf!("🔓 {} : dossier de confiance", "🔓 {}: trusted folder", dir.display()));
    }

    /// Bar above a file of a folder not trusted, when something waits for that (a language
    /// server, eslint, the flake8 configuration of the project).
    pub(super) fn trust_bar(&mut self, ui: &mut egui::Ui, i: usize, pane: u8) {
        let Some(d) = self.docs.get(i).filter(|d| d.view == document::View::Text) else { return };
        let Some(path) = d.path.clone() else { return };
        let waits = (self.settings.lsp && lsp::language_of(&path).is_some()) || matches!(lint::lang_of(&path), Some("python" | "javascript"));
        if !waits || self.trusted(&path) {
            return;
        }
        let Some(dir) = self.trust_root(&path) else { return };
        if self.trust_later.contains(&dir) {
            return;
        }
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string());
        let (mut trust, mut later) = (false, false);
        let bg = icons::mix(ui.visuals().panel_fill, warn(), 0.12);
        egui::TopBottomPanel::top(egui::Id::new("trust_bar").with(pane))
            .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(10.0, 4.0)))
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(t!("🔒 Mode restreint", "🔒 Restricted mode")).color(warn()).strong()).on_hover_text(dir.display().to_string());
                    ui.label(t!(
                        "les serveurs de langage et les linters configurés par ce dossier ne tournent pas : ils peuvent exécuter son code",
                        "the language servers and the linters set up by this folder do not run: they may run its code"
                    ));
                    trust = ui.button(tf!("Faire confiance à « {name} »", "Trust “{name}”")).on_hover_text(dir.display().to_string()).clicked();
                    later = ui.button(t!("Plus tard", "Later")).on_hover_text(t!("Jusqu'à la fermeture de Florm", "Until Florm is closed")).clicked();
                });
            });
        if trust {
            self.trust_folder(dir);
        } else if later {
            self.trust_later.insert(dir);
        }
    }
}

/// `path` is in one of the folders `trusted` (or is one).
fn trusted_in(trusted: &[String], path: &Path) -> bool {
    trusted.iter().filter(|t| !t.trim().is_empty()).any(|t| path.starts_with(Path::new(t.trim())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_trusted_covers_what_is_inside() {
        let list = vec!["/home/a/projet".to_string(), String::new()];
        assert!(trusted_in(&list, Path::new("/home/a/projet/src/main.rs")));
        assert!(trusted_in(&list, Path::new("/home/a/projet")));
        // a folder whose name only begins the same is not inside
        assert!(!trusted_in(&list, Path::new("/home/a/projet2/main.rs")));
        assert!(!trusted_in(&list, Path::new("/home/a/main.rs")));
    }
}
