//! Comparing: two files or folders (from the tree), the file and the clipboard, the file and
//! another path; and the local history of a file (its versions at each save, outside git).

use super::*;
use crate::local_history;

/// The versions of a file shown in the "local history" window.
pub(super) struct LocalHistory {
    path: PathBuf,
    versions: Vec<(u64, PathBuf)>,
}

impl App {
    /// The diff of `a` and `b` in a tab (in the background: folders can be long to compare).
    pub(super) fn compare_paths(&mut self, a: PathBuf, b: PathBuf) {
        let name = |p: &Path| self.rel_label(p);
        let (la, lb) = (name(&a), name(&b));
        self.compare_labeled(a, b, la, lb);
    }

    fn compare_labeled(&mut self, a: PathBuf, b: PathBuf, la: String, lb: String) {
        let title = format!("⇄ {} ↔ {}", short(&la), short(&lb));
        self.set_status(tf!("Comparaison de {la} et {lb}…", "Comparing {la} and {lb}…"));
        self.jobs.spawn(move || JobResult::Compare(title, local_history::diff(&a, &b, &la, &lb)));
    }

    pub(super) fn compare_loaded(&mut self, title: String, r: Result<String, String>) {
        match r {
            Ok(d) if d.trim().is_empty() => self.set_status(t!("Identiques : aucune différence", "Identical: no difference")),
            Ok(d) => {
                self.new_scratch(title, d, "Diff", true);
                self.set_status(t!("Différences : en rouge l'ancien (à gauche), en vert le nouveau", "Differences: in red the old one (left), in green the new one"));
            }
            Err(e) => self.set_error(tf!("Comparaison impossible : {e}", "Cannot compare: {e}")),
        }
    }

    /// The path for a title: relative to the project when inside it.
    fn rel_label(&self, p: &Path) -> String {
        let rel = self.root.as_ref().and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p);
        rel.display().to_string()
    }

    /// The text of the active tab (as it is in the editor, saved or not) written apart.
    fn active_text_file(&mut self) -> Option<(PathBuf, String)> {
        let d = self.docs.get(self.active)?;
        let label = d.path.as_deref().map(|p| self.rel_label(p)).unwrap_or_else(|| d.title());
        let ext = d.path.as_deref().and_then(|p| p.extension()).map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        match local_history::temp_file(&format!("editor{ext}"), &d.rope.to_string()) {
            Ok(p) => Some((p, label)),
            Err(e) => {
                self.set_error(e);
                None
            }
        }
    }

    /// The file being edited against the text of the clipboard.
    pub(super) fn compare_clipboard(&mut self) {
        let clip = match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
            Ok(t) => t,
            Err(_) => {
                self.set_status(t!("Le presse-papiers ne contient pas de texte", "The clipboard holds no text"));
                return;
            }
        };
        let Some((mine, label)) = self.active_text_file() else { return };
        match local_history::temp_file("clipboard", &clip) {
            Ok(c) => self.compare_labeled(mine, c, label, t!("presse-papiers", "clipboard").into()),
            Err(e) => self.set_error(e),
        }
    }

    /// The file being edited against another file (or its folder against another folder).
    pub(super) fn compare_with(&mut self, other: PathBuf) {
        if !other.exists() {
            self.set_error(tf!("Introuvable : {}", "Not found: {}", other.display()));
            return;
        }
        if other.is_dir() {
            let Some(dir) = self.docs.get(self.active).and_then(|d| d.path.as_deref()?.parent().map(Path::to_path_buf)).or_else(|| self.root.clone()) else { return };
            self.compare_paths(dir, other);
            return;
        }
        let Some((mine, label)) = self.active_text_file() else { return };
        let lb = self.rel_label(&other);
        self.compare_labeled(mine, other, label, lb);
    }

    /// The window of the versions of the active file.
    pub(super) fn open_local_history(&mut self) {
        let Some(path) = self.docs.get(self.active).and_then(|d| d.path.clone()) else {
            self.set_status(t!("L'historique local est celui d'un fichier enregistré", "The local history is the one of a saved file"));
            return;
        };
        let versions = local_history::versions(&path);
        self.local_history = Some(LocalHistory { path, versions });
    }

    pub(super) fn local_history_ui(&mut self, ctx: &egui::Context) {
        let Some(lh) = &self.local_history else { return };
        let mut open = true;
        let (mut compare, mut restore) = (None, None);
        let name = lh.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        egui::Window::new(tf!("Historique local — {name}", "Local history — {name}"))
            .id(egui::Id::new("local_history"))
            .open(&mut open)
            .collapsible(false)
            .default_size([460.0, 380.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(t!("Une version à chaque enregistrement, gardée par Florm hors de git (les 50 dernières).", "A version at each save, kept by Florm outside git (the last 50).")).small().weak());
                ui.separator();
                if lh.versions.is_empty() {
                    ui.label(RichText::new(t!("Pas encore de version : elles viennent avec les enregistrements (Ctrl+S).", "No version yet: they come with the saves (Ctrl+S).")).weak());
                    return;
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (k, (at, p)) in lh.versions.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let size = std::fs::metadata(p).map_or(0, |m| m.len());
                            ui.label(git::ago((*at / 1000) as i64)).on_hover_text(format!("{at}"));
                            ui.label(RichText::new(tf!("{} octets", "{} bytes", size)).small().weak());
                            if k == 0 {
                                ui.label(RichText::new(t!("(la dernière)", "(the last one)")).small().weak());
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(t!("Restaurer", "Restore")).on_hover_text(t!("Remet ce texte dans l'éditeur (Ctrl+Z pour revenir), sans enregistrer", "Puts this text back in the editor (Ctrl+Z to come back), without saving")).clicked() {
                                    restore = Some(p.clone());
                                }
                                if ui.button(t!("Comparer", "Compare")).on_hover_text(t!("Cette version (rouge) et le texte de l'éditeur (vert)", "This version (red) and the text of the editor (green)")).clicked() {
                                    compare = Some((*at, p.clone()));
                                }
                            });
                        });
                    }
                });
            });
        let path = lh.path.clone();
        if !open {
            self.local_history = None;
        }
        if let Some((at, p)) = compare {
            if let Some(i) = self.docs.iter().position(|d| d.path.as_ref() == Some(&path)) {
                self.activate_tab(i);
                if let Some((mine, label)) = self.active_text_file() {
                    self.compare_labeled(p, mine, tf!("{label} ({})", "{label} ({})", git::ago((at / 1000) as i64)), label);
                }
            }
        }
        if let Some(p) = restore {
            match std::fs::read_to_string(&p) {
                Ok(text) => {
                    if let Some(i) = self.docs.iter().position(|d| d.path.as_ref() == Some(&path)) {
                        self.activate_tab(i);
                        self.docs[i].replace_all(&text);
                        self.set_status(t!("Version restaurée dans l'éditeur (Ctrl+Z pour revenir, Ctrl+S pour l'enregistrer)", "Version restored in the editor (Ctrl+Z to come back, Ctrl+S to save it)"));
                    }
                }
                Err(e) => self.set_error(e.to_string()),
            }
        }
    }
}

/// The end of a long path, for a tab title.
fn short(label: &str) -> String {
    let name = label.rsplit(['/', '\\']).next().unwrap_or(label);
    if name.is_empty() { label.to_string() } else { name.to_string() }
}
