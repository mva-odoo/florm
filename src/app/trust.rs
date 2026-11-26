//! Trusted folders: git (the configuration of a repository can name commands), the language
//! servers and the linters set up by the project (flake8 with its plugins, eslint) may run its
//! code, so they only run in a folder the developer trusted. Elsewhere a bar above the file and the
//! Git view offer to trust it (restricted mode).

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
    pub(super) fn trust_folder(&mut self, dir: PathBuf) {
        let s = dir.to_string_lossy().into_owned();
        if !self.settings.trusted_folders.contains(&s) {
            self.settings.trusted_folders.push(s);
        }
        self.persist_trusted();
        for i in 0..self.docs.len() {
            if self.docs[i].path.as_deref().is_some_and(|p| p.starts_with(&dir)) {
                self.analyze(i, false);
            }
        }
        // git runs there now: its status, and the graph of the Git view
        self.git_refresh();
        if self.repo.as_ref().is_some_and(|r| r.starts_with(&dir)) {
            self.load_history();
        }
        self.set_status(tf!("🔓 {} : dossier de confiance", "🔓 {}: trusted folder", dir.display()));
    }

    /// The trust of `dir` taken back (Préférences): its open files go back to restricted mode, the
    /// language servers stop (those of the folders still trusted start again when needed).
    pub(super) fn untrust_folder(&mut self, dir: &str) {
        self.settings.trusted_folders.retain(|t| t != dir);
        self.persist_trusted();
        let now_restricted: Vec<usize> =
            (0..self.docs.len()).filter(|&i| self.docs[i].path.as_deref().is_some_and(|p| !self.trusted(p))).collect();
        self.lsp.manager.reset();
        self.lsp.requests.clear();
        self.lsp.completion = None;
        for i in now_restricted {
            self.docs[i].set_lsp_diagnostics(Vec::new());
            self.analyze(i, false);
        }
        self.git_refresh();
        self.set_status(tf!("🔒 {dir} : plus de confiance (mode restreint)", "🔒 {dir}: no longer trusted (restricted mode)"));
    }

    fn persist_trusted(&mut self) {
        // a JSON list of strings is a TOML list too
        let list = serde_json::to_string(&self.settings.trusted_folders).unwrap_or_else(|_| "[]".into());
        self.persist("trusted_folders", list);
        git::set_trusted(&self.settings.trusted_folders);
    }

    /// Bar above a file of a folder not trusted, when something waits for that (a language
    /// server, eslint, the flake8 configuration of the project).
    pub(super) fn trust_bar(&mut self, ui: &mut egui::Ui, i: usize, pane: u8) {
        let Some(d) = self.docs.get(i).filter(|d| d.view == document::View::Text) else { return };
        let Some(path) = d.path.clone() else { return };
        let waits = (self.settings.lsp && lsp::language_of(&path, &self.settings.lsp_servers).is_some())
            || matches!(lint::lang_of(&path), Some("python" | "javascript"))
            || !lint::generic_linters(&path, &self.settings.linters).is_empty()
            || (crate::features::on(crate::features::Feature::Git) && self.repo_for(&path).is_some());
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
        egui::Panel::top(egui::Id::unique("trust_bar").with(pane))
            .frame(egui::Frame::NONE.fill(bg).inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(t!("🔒 Mode restreint", "🔒 Restricted mode")).color(warn()).strong()).on_hover_text(dir.display().to_string());
                    ui.label(t!(
                        "Git, les serveurs de langage et les linters configurés par ce dossier ne tournent pas : ils peuvent exécuter son code",
                        "Git, the language servers and the linters set up by this folder do not run: they may run its code"
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

impl App {
    /// The Git view of a repository not trusted: why nothing shows, and a button to trust it.
    pub(super) fn untrusted_git_ui(&mut self, ui: &mut egui::Ui, repo: &Path) {
        let dir = self.trust_root(repo).unwrap_or_else(|| repo.to_path_buf());
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string());
        let mut trust = false;
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(t!("🔒 Mode restreint", "🔒 Restricted mode")).color(warn()).strong().size(16.0));
            ui.add_space(6.0);
            ui.label(t!(
                "Git ne tourne pas dans ce dossier : la configuration d'un dépôt (.git/config, filtres de .gitattributes) peut lancer des commandes.",
                "Git does not run in this folder: the configuration of a repository (.git/config, filters of .gitattributes) can run commands."
            ));
            ui.add_space(8.0);
            trust = ui.button(tf!("Faire confiance à « {name} »", "Trust “{name}”")).on_hover_text(dir.display().to_string()).clicked();
        });
        if trust {
            self.trust_folder(dir);
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
