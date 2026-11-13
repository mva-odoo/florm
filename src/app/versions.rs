//! Versions side by side (see `git::Versions`: `odoo/master`, `odoo/17.0`…): a new version added as a
//! worktree in each repository at once (`odoo/20.0`, `enterprise/20.0`, `design-themes/20.0`).

use super::*;
use crate::git::Versions;

pub(super) struct AddVersionUi {
    /// The repositories with versions, and whether the new one is added to it.
    groups: Vec<(Versions, bool)>,
    branch: String,
    /// Branches known in the first repository (local and remote), for the suggestions.
    pub(super) branches: Vec<String>,
    pub(super) fetching: bool,
    pub(super) fetch_error: Option<String>,
    pub(super) busy: bool,
    /// What happened in each repository, after "Add".
    pub(super) results: Vec<(String, Result<String, String>)>,
    focus: bool,
}

/// Name shown for a repository with versions (`odoo`).
fn group_name(v: &Versions) -> String {
    v.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The known branches matching what is typed: the ones starting with it first.
fn suggestions<'a>(branches: &'a [String], typed: &str) -> Vec<&'a String> {
    let typed = typed.trim().to_lowercase();
    if typed.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<&String> = branches.iter().filter(|b| b.to_lowercase().contains(&typed)).collect();
    v.sort_by_key(|b| !b.to_lowercase().starts_with(&typed));
    v.truncate(30);
    v
}

impl App {
    /// The window to add a version, for the repositories with versions around `dir`.
    pub(super) fn open_add_version(&mut self, dir: PathBuf) {
        let groups = git::versions_near(&dir);
        let Some(first) = groups.first() else {
            self.set_status(t!("Pas de versions côte à côte ici (dossier/master, dossier/17.0… en worktrees git)", "No versions side by side here (folder/master, folder/17.0… as git worktrees)"));
            return;
        };
        let main = first.main.clone();
        self.version_add = Some(AddVersionUi {
            groups: groups.into_iter().map(|g| (g, true)).collect(),
            branch: String::new(),
            branches: Vec::new(),
            fetching: false,
            fetch_error: None,
            busy: false,
            results: Vec::new(),
            focus: true,
        });
        self.jobs.spawn(move || match git::version_branches(&main) {
            Ok(branches) => JobResult::VersionBranches { branches, error: None },
            Err(e) => JobResult::VersionBranches { branches: Vec::new(), error: Some(e) },
        });
    }

    pub(super) fn version_add_ui(&mut self, ctx: &egui::Context) {
        let accent = self.hl.colors.accent;
        let Some(st) = self.version_add.as_mut() else { return };
        let mut open = true;
        let (mut go, mut cancel, mut fetch) = (false, false, false);
        let folder = git::version_folder(&st.branch);
        egui::Window::new(t!("Ajouter une version", "Add a version"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("version_add_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label(RichText::new(t!("Branche", "Branch")).weak());
                    ui.horizontal(|ui| {
                        let r = crate::input::add(ui, egui::TextEdit::singleline(&mut st.branch).desired_width(320.0).hint_text("20.0, saas-18.3, 17.0-fix-invoice…"));
                        if std::mem::take(&mut st.focus) {
                            r.request_focus();
                        }
                        if st.fetching {
                            spinner(ui);
                        } else if ui.button("⟳").on_hover_text(t!("git fetch --all dans chaque dépôt : les branches récentes arrivent dans la liste", "git fetch --all in each repository: the recent branches come into the list")).clicked() {
                            fetch = true;
                        }
                    });
                    ui.end_row();
                    let found = suggestions(&st.branches, &st.branch);
                    if !found.is_empty() && !found.iter().any(|b| **b == st.branch.trim()) {
                        ui.label("");
                        egui::ScrollArea::vertical().max_height(150.0).id_salt("version_add_branches").show(ui, |ui| {
                            for b in found {
                                if ui.selectable_label(false, RichText::new(b).monospace()).clicked() {
                                    st.branch = b.clone();
                                }
                            }
                        });
                        ui.end_row();
                    }
                    ui.label(RichText::new(t!("Dépôts", "Repositories")).weak());
                    ui.vertical(|ui| {
                        for (g, on) in st.groups.iter_mut() {
                            ui.horizontal(|ui| {
                                ui.checkbox(on, group_name(g));
                                ui.label(RichText::new(g.names.join(", ")).small().weak());
                                if !folder.is_empty() && g.dir.join(&folder).exists() {
                                    ui.label(RichText::new(t!("déjà là", "already there")).small().color(warn()));
                                }
                            });
                        }
                    });
                    ui.end_row();
                });
                if !folder.is_empty() {
                    ui.add_space(4.0);
                    let dirs: Vec<String> = st.groups.iter().filter(|(_, on)| *on).map(|(g, _)| format!("{}/{folder}", group_name(g))).collect();
                    ui.label(RichText::new(tf!("Nouveaux dossiers : {}", "New folders: {}", dirs.join(", "))).small().weak());
                }
                if let Some(e) = &st.fetch_error {
                    ui.label(RichText::new(e.lines().last().unwrap_or("")).small().color(warn())).on_hover_text(e);
                }
                for (name, r) in &st.results {
                    match r {
                        Ok(_) => ui.label(RichText::new(format!("✔ {name}")).color(ok())),
                        Err(e) => ui.label(RichText::new(format!("✖ {name} : {}", e.lines().last().unwrap_or(""))).color(err())).on_hover_text(e),
                    };
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let ready = !st.busy && !folder.is_empty() && st.groups.iter().any(|(_, on)| *on);
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.add_enabled(ready, filled_button(t!("Ajouter", "Add"), accent)).clicked() || (enter && ready) {
                        go = true;
                    }
                    if ui.button(t!("Annuler", "Cancel")).clicked() {
                        cancel = true;
                    }
                    if st.busy {
                        spinner(ui);
                    }
                });
            });
        if !open || cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.version_add = None;
            return;
        }
        if fetch {
            st.fetching = true;
            st.fetch_error = None;
            let mains: Vec<PathBuf> = st.groups.iter().map(|(g, _)| g.main.clone()).collect();
            self.jobs.spawn(move || {
                let errors: Vec<String> = mains.iter().filter_map(|m| git::run(m, &["fetch", "--all"]).err()).collect();
                let (branches, error) = match git::version_branches(&mains[0]) {
                    Ok(b) => (b, None),
                    Err(e) => (Vec::new(), Some(e)),
                };
                JobResult::VersionBranches { branches, error: error.or_else(|| (!errors.is_empty()).then(|| errors.join("\n"))) }
            });
        }
        if !go {
            return;
        }
        let Some(root) = self.root.clone() else { return };
        st.busy = true;
        st.results.clear();
        let branch = st.branch.trim().to_string();
        let groups: Vec<Versions> = st.groups.iter().filter(|(_, on)| *on).map(|(g, _)| g.clone()).collect();
        self.set_status(tf!("Ajout de la version {branch}…", "Adding version {branch}…"));
        self.jobs.spawn(move || {
            // one after the other: a repository that fails does not stop the next ones
            let results = groups.iter().map(|g| (group_name(g), git::add_version(g, &branch))).collect();
            JobResult::VersionAdded { root: root.clone(), results, repos: git::find_repos(&root, 3) }
        });
    }

    pub(super) fn version_branches_loaded(&mut self, branches: Vec<String>, error: Option<String>) {
        let Some(st) = self.version_add.as_mut() else { return };
        st.fetching = false;
        if !branches.is_empty() {
            st.branches = branches;
        }
        st.fetch_error = error;
    }

    pub(super) fn version_added(&mut self, root: PathBuf, results: Vec<(String, Result<String, String>)>, repos: Vec<PathBuf>) {
        for (name, r) in &results {
            let (Ok(out) | Err(out)) = r;
            self.log(format!("{name} : {out}"));
        }
        let failed = results.iter().filter(|(_, r)| r.is_err()).count();
        if failed == 0 {
            self.version_add = None;
            self.set_status(tf!("Version ajoutée dans {} dépôt(s)", "Version added in {} repositories", results.len()));
        } else {
            match self.version_add.as_mut() {
                // the window stays, with what went wrong in each repository
                Some(st) => {
                    st.busy = false;
                    st.results = results;
                }
                None => self.set_error(tf!("Version : échec dans {failed} dépôt(s)", "Version: failed in {failed} repositories")),
            }
        }
        self.apply_repos(root, repos);
        self.exec_refresh_tree();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_suggestions_start_with_what_is_typed() {
        let b: Vec<String> = ["17.0", "17.0-fix-mva", "saas-17.2", "18.0"].iter().map(|s| s.to_string()).collect();
        assert_eq!(suggestions(&b, "17"), ["17.0", "17.0-fix-mva", "saas-17.2"]);
        assert_eq!(suggestions(&b, " SAAS"), ["saas-17.2"]);
        assert!(suggestions(&b, "").is_empty());
    }
}
