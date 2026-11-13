//! The « Versions » window: every version side by side (`odoo/20.0`, `enterprise/20.0`…) in one
//! table, a row per version and a column per repository, with the state of each folder, the
//! version worked on, and what to do with each: update, add where it is missing, remove.

use super::*;
use crate::git::{VersionState, Versions};
use std::collections::HashMap;

pub(super) struct VersionsView {
    /// The repositories with versions (`odoo`, `enterprise`…).
    groups: Vec<Versions>,
    /// The state of each version's folder (None: being read).
    pub(super) states: HashMap<PathBuf, Option<Result<VersionState, String>>>,
    /// The versions with an action under way (update, add, remove); "" for the fetch.
    pub(super) busy: HashSet<String>,
    /// What the last action said: its title, then each repository and what git said.
    pub(super) last: Option<(String, Vec<(String, Result<String, String>)>)>,
    /// « Supprimer » clicked on this version: the confirmation is shown.
    confirm_remove: Option<String>,
}

/// What a row of the window asked for during the frame.
enum Act {
    Work(String),
    All,
    Update(String),
    Add(String, Option<usize>),
    Remove(String, bool),
    Terminal(PathBuf),
    Fetch,
    Refresh,
    New,
}

/// The order of the rows: `master` first, then the newest versions (`20.0`, `saas-19.1`, `19.0`…).
fn version_key(name: &str) -> (bool, i64, String) {
    let digits: String = name.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let (major, minor) = digits.split_once('.').unwrap_or((&digits, "0"));
    let n = major.parse::<i64>().unwrap_or(-1) * 100 + minor.parse::<i64>().unwrap_or(0);
    (name != "master", -n, name.to_string())
}

/// The columns: `odoo` first, then `enterprise`, then the others by name.
fn order_groups(groups: &mut [Versions]) {
    groups.sort_by_key(|g| {
        let n = group_name(g);
        (match n.as_str() { "odoo" => 0, "enterprise" => 1, _ => 2 }, n)
    });
}

fn group_name(v: &Versions) -> String {
    v.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

impl App {
    /// The window, for the versions of the opened folder (else the ones around `dir`).
    pub(super) fn open_versions(&mut self, dir: PathBuf) {
        let groups = if self.versions.is_empty() { git::versions_near(&dir) } else { self.versions.clone() };
        if groups.is_empty() {
            self.set_status(t!("Pas de versions côte à côte ici (dossier/master, dossier/17.0… en worktrees git)", "No versions side by side here (folder/master, folder/17.0… as git worktrees)"));
            return;
        }
        self.versions_view = Some(VersionsView { groups, states: HashMap::new(), busy: HashSet::new(), last: None, confirm_remove: None });
        self.versions_view_refresh();
    }

    /// The versions read again, then the state of each folder (one job per folder: `git status`
    /// is slow on a big repository).
    pub(super) fn versions_view_refresh(&mut self) {
        let fresh = match &self.root {
            Some(_) if !self.versions.is_empty() => self.versions.clone(),
            Some(r) => git::versions_near(r),
            None => Vec::new(),
        };
        let Some(v) = self.versions_view.as_mut() else { return };
        if !fresh.is_empty() {
            v.groups = fresh;
        }
        order_groups(&mut v.groups);
        v.states.clear();
        let dirs: Vec<PathBuf> = v.groups.iter().flat_map(|g| g.names.iter().map(|n| g.dir.join(n))).collect();
        for d in dirs {
            v.states.insert(d.clone(), None);
            self.jobs.spawn(move || {
                let r = git::version_state(&d);
                JobResult::VersionState(d, r)
            });
        }
    }

    pub(super) fn version_state_loaded(&mut self, dir: PathBuf, r: Result<VersionState, String>) {
        if let Some(v) = self.versions_view.as_mut() {
            if v.states.contains_key(&dir) {
                v.states.insert(dir, Some(r));
            }
        }
    }

    /// An action of the window done: what git said, then everything read again.
    pub(super) fn versions_done(&mut self, root: PathBuf, key: String, title: String, results: Vec<(String, Result<String, String>)>, repos: Vec<PathBuf>) {
        for (name, r) in &results {
            let (Ok(out) | Err(out)) = r;
            if !out.is_empty() {
                self.log(format!("{title} · {name} : {out}"));
            }
        }
        let failed = results.iter().filter(|(_, r)| r.is_err()).count();
        if failed == 0 {
            self.set_status(title.clone());
        } else {
            self.set_error(tf!("{title} : échec dans {failed} dépôt(s)", "{title}: failed in {failed} repositories"));
        }
        if let Some(v) = self.versions_view.as_mut() {
            v.busy.remove(&key);
            v.last = Some((title, results));
        }
        if self.root.as_ref() == Some(&root) {
            self.load_versions();
            self.refresh_index();
            self.apply_repos(root, repos);
            self.exec_refresh_tree();
        }
        self.versions_view_refresh();
    }

    /// Runs `f` on each repository in a job, then `versions_done`.
    fn versions_job(&mut self, key: String, title: String, work: Vec<(String, Box<dyn FnOnce() -> Result<String, String> + Send>)>) {
        let Some(root) = self.root.clone() else { return };
        if let Some(v) = self.versions_view.as_mut() {
            v.busy.insert(key.clone());
            v.last = None;
        }
        self.jobs.spawn(move || {
            let results = work.into_iter().map(|(name, f)| (name, f())).collect();
            JobResult::VersionsDone { root: root.clone(), key, title, results, repos: git::find_repos(&root, 3) }
        });
    }

    pub(super) fn versions_view_ui(&mut self, ctx: &egui::Context) {
        let accent = self.hl.colors.accent;
        let current = self.version.clone();
        let Some(v) = self.versions_view.as_mut() else { return };
        let mut names: Vec<String> = v.groups.iter().flat_map(|g| g.names.iter().cloned()).collect();
        names.sort_by_key(|n| version_key(n));
        names.dedup();
        let mut act: Option<Act> = None;
        let mut open = true;
        egui::Window::new(t!("Versions côte à côte", "Versions side by side"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(780.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(t!(
                        "Chaque version est un dossier (un worktree git) sur sa branche, qui partage l'historique du dépôt principal (master) : pas de clone en plus. « Travailler dessus » garde seulement cette version dans Ctrl+P, F12 et la recherche ; les autres restent dans l'arborescence, grisées.",
                        "Each version is a folder (a git worktree) on its branch, sharing the history of the main repository (master): no extra clone. “Work on it” keeps only this version in go to file, F12 and the search; the others stay in the tree, dimmed."
                    ))
                    .small()
                    .weak(),
                );
                ui.add_space(8.0);
                egui::ScrollArea::vertical().max_height(ctx.screen_rect().height() * 0.6).show(ui, |ui| {
                    egui::Grid::new("versions_grid").num_columns(v.groups.len() + 2).spacing([18.0, 10.0]).striped(true).show(ui, |ui| {
                        ui.label(RichText::new(t!("Version", "Version")).weak());
                        for g in &v.groups {
                            ui.label(RichText::new(group_name(g)).weak());
                        }
                        ui.label("");
                        ui.end_row();
                        for name in &names {
                            let is_current = *name == current;
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(if is_current { "●" } else { " " }).color(accent));
                                let text = RichText::new(name).monospace().strong();
                                ui.label(if is_current { text.color(accent) } else { text });
                                if is_current {
                                    ui.label(RichText::new(t!("courante", "current")).small().color(accent));
                                }
                            });
                            let busy = v.busy.contains(name);
                            let mut dirty: Vec<String> = Vec::new();
                            for (k, g) in v.groups.iter().enumerate() {
                                let dir = g.dir.join(name);
                                if !g.names.contains(name) {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("—").weak());
                                        if !busy && ui.small_button("+").on_hover_text(tf!("Ajouter {name} dans {} seulement", "Add {name} in {} only", group_name(g))).clicked() {
                                            act = Some(Act::Add(name.clone(), Some(k)));
                                        }
                                    });
                                    continue;
                                }
                                let main = dir == g.main;
                                match v.states.get(&dir) {
                                    Some(Some(Ok(s))) => {
                                        if s.changes > 0 {
                                            dirty.push(format!("{}/{name}", group_name(g)));
                                        }
                                        let r = ui.horizontal(|ui| {
                                            if s.ahead == 0 && s.behind == 0 && s.changes == 0 {
                                                ui.label(RichText::new(if s.upstream.is_empty() { "✔" } else { t!("✔ à jour", "✔ up to date") }).color(ok()));
                                            }
                                            if s.behind > 0 {
                                                ui.label(RichText::new(format!("↓{}", s.behind)).color(warn()));
                                            }
                                            if s.ahead > 0 {
                                                ui.label(RichText::new(format!("↑{}", s.ahead)).color(accent));
                                            }
                                            if s.changes > 0 {
                                                ui.label(RichText::new(tf!("●{} modif.", "●{} changed", s.changes)).color(warn()));
                                            }
                                            if s.branch != *name {
                                                ui.label(RichText::new(&s.branch).small().weak());
                                            }
                                            if main {
                                                ui.label(RichText::new(t!("dépôt", "repository")).small().weak());
                                            }
                                        });
                                        let tracked = if s.upstream.is_empty() { t!("ne suit aucune branche distante", "tracks no remote branch").to_string() } else { tf!("suit {}", "tracks {}", s.upstream) };
                                        r.response.on_hover_text(format!(
                                            "{}\n{} · {tracked}\n\n{}",
                                            dir.display(),
                                            s.branch,
                                            t!("↓ à récupérer (depuis le dernier fetch), ↑ à pousser, ● fichiers modifiés", "↓ to pull (since the last fetch), ↑ to push, ● changed files")
                                        ));
                                    }
                                    Some(Some(Err(e))) => {
                                        ui.label(RichText::new("✖").color(err())).on_hover_text(e);
                                    }
                                    _ => {
                                        spinner(ui);
                                    }
                                }
                            }
                            ui.horizontal(|ui| {
                                if busy {
                                    spinner(ui);
                                    return;
                                }
                                if is_current {
                                    if ui.button(t!("Ne plus filtrer", "Stop filtering")).on_hover_text(t!("Toutes les versions de nouveau dans Ctrl+P, F12 et la recherche", "Every version back in go to file, F12 and the search")).clicked() {
                                        act = Some(Act::All);
                                    }
                                } else if ui.button(t!("Travailler dessus", "Work on it")).on_hover_text(t!("Seulement cette version dans Ctrl+P, F12 et la recherche, dans tous les dépôts", "Only this version in go to file, F12 and the search, in every repository")).clicked() {
                                    act = Some(Act::Work(name.clone()));
                                }
                                if ui.button(t!("Mettre à jour", "Update")).on_hover_text(t!("git pull --ff-only dans chaque dépôt de cette version (jamais de commit de merge)", "git pull --ff-only in each repository of this version (never a merge commit)")).clicked() {
                                    act = Some(Act::Update(name.clone()));
                                }
                                ui.menu_button("⋯", |ui| {
                                    for g in v.groups.iter().filter(|g| g.names.contains(name)) {
                                        if ui.button(tf!("Terminal dans {}/{name}", "Terminal in {}/{name}", group_name(g))).clicked() {
                                            act = Some(Act::Terminal(g.dir.join(name)));
                                            ui.close_menu();
                                        }
                                    }
                                    if v.groups.iter().any(|g| !g.names.contains(name)) && ui.button(t!("Ajouter dans les dépôts où elle manque", "Add it to the repositories without it")).clicked() {
                                        act = Some(Act::Add(name.clone(), None));
                                        ui.close_menu();
                                    }
                                    let is_main = v.groups.iter().any(|g| g.dir.join(name) == g.main);
                                    ui.separator();
                                    if ui.add_enabled(!is_main, egui::Button::new(RichText::new(t!("Supprimer cette version…", "Remove this version…")).color(err()))).on_disabled_hover_text(t!("C'est le dépôt principal : il ne se supprime pas d'ici", "It is the main repository: it cannot be removed from here")).clicked() {
                                        v.confirm_remove = Some(name.clone());
                                        ui.close_menu();
                                    }
                                });
                            });
                            ui.end_row();
                            // the confirmation, right under its row
                            if v.confirm_remove.as_deref() == Some(name.as_str()) {
                                ui.label("");
                                let span = v.groups.len() + 1;
                                ui.vertical(|ui| {
                                    let dirs: Vec<String> = v.groups.iter().filter(|g| g.names.contains(name)).map(|g| format!("{}/{name}", group_name(g))).collect();
                                    ui.label(RichText::new(tf!("Supprimer les dossiers {} ? La branche {name} reste dans chaque dépôt.", "Remove the folders {}? The branch {name} stays in each repository.", dirs.join(", "))).color(warn()));
                                    if !dirty.is_empty() {
                                        ui.label(RichText::new(tf!("Fichiers modifiés non committés dans {} : ils seront perdus.", "Changed files not committed in {}: they will be lost.", dirty.join(", "))).color(err()));
                                    }
                                    ui.horizontal(|ui| {
                                        let label = if dirty.is_empty() { t!("Supprimer", "Remove") } else { t!("Supprimer quand même", "Remove anyway") };
                                        if ui.add(filled_button(label, err())).clicked() {
                                            act = Some(Act::Remove(name.clone(), !dirty.is_empty()));
                                        }
                                        if ui.button(t!("Annuler", "Cancel")).clicked() {
                                            v.confirm_remove = None;
                                        }
                                    });
                                });
                                for _ in 1..span {
                                    ui.label("");
                                }
                                ui.end_row();
                            }
                        }
                    });
                });
                if let Some((title, results)) = &v.last {
                    ui.add_space(6.0);
                    ui.label(RichText::new(title).strong());
                    for (name, r) in results {
                        match r {
                            Ok(_) => ui.label(RichText::new(format!("✔ {name}")).color(ok())),
                            Err(e) => ui.label(RichText::new(format!("✖ {name} : {}", e.lines().last().unwrap_or(""))).color(err())).on_hover_text(e),
                        };
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(filled_button(t!("+ Nouvelle version…", "+ New version…"), accent)).clicked() {
                        act = Some(Act::New);
                    }
                    if v.busy.contains("") {
                        spinner(ui);
                    } else if ui.button(t!("⟳ Vérifier sur le serveur", "⟳ Check the server")).on_hover_text(t!("git fetch --all dans chaque dépôt : les ↓ à récupérer sont recomptés", "git fetch --all in each repository: the ↓ to pull are counted again")).clicked() {
                        act = Some(Act::Fetch);
                    }
                    if ui.button(t!("Rafraîchir", "Refresh")).clicked() {
                        act = Some(Act::Refresh);
                    }
                });
            });
        if !open || (self.version_add.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape))) {
            self.versions_view = None;
            return;
        }
        let Some(act) = act else { return };
        let groups = self.versions_view.as_ref().map(|v| v.groups.clone()).unwrap_or_default();
        match act {
            Act::Work(n) => self.set_version(n),
            Act::All => self.set_version(String::new()),
            Act::Refresh => self.versions_view_refresh(),
            Act::New => {
                if let Some(r) = self.root.clone() {
                    self.open_add_version(r);
                }
            }
            Act::Terminal(d) => {
                let shell = self.term_shell();
                self.term.open(ctx, shell.as_deref(), Some(d));
                self.bottom = Some(BottomTab::Terminal);
            }
            Act::Fetch => {
                let work = groups.iter().map(|g| (group_name(g), Box::new({ let m = g.main.clone(); move || git::run(&m, &["fetch", "--all"]) }) as Box<dyn FnOnce() -> Result<String, String> + Send>)).collect();
                self.versions_job(String::new(), t!("Vérifié sur le serveur (git fetch)", "Checked on the server (git fetch)").into(), work);
            }
            Act::Update(n) => {
                let work = groups
                    .iter()
                    .filter(|g| g.names.contains(&n))
                    .map(|g| (group_name(g), Box::new({ let d = g.dir.join(&n); move || git::pull_version(&d) }) as Box<dyn FnOnce() -> Result<String, String> + Send>))
                    .collect();
                self.versions_job(n.clone(), tf!("Version {n} mise à jour", "Version {n} updated"), work);
            }
            Act::Add(n, only) => {
                let work = groups
                    .iter()
                    .enumerate()
                    .filter(|(k, g)| !g.names.contains(&n) && only.is_none_or(|o| o == *k))
                    .map(|(_, g)| (group_name(g), Box::new({ let (g, n) = (g.clone(), n.clone()); move || git::add_version(&g, &n) }) as Box<dyn FnOnce() -> Result<String, String> + Send>))
                    .collect();
                self.versions_job(n.clone(), tf!("Version {n} ajoutée", "Version {n} added"), work);
            }
            Act::Remove(n, force) => {
                if let Some(v) = self.versions_view.as_mut() {
                    v.confirm_remove = None;
                }
                // the version worked on removed: every version back in the index
                if self.version == n {
                    self.set_version(String::new());
                }
                let work = groups
                    .iter()
                    .filter(|g| g.names.contains(&n))
                    .map(|g| (group_name(g), Box::new({ let (g, n) = (g.clone(), n.clone()); move || git::remove_version(&g, &n, force) }) as Box<dyn FnOnce() -> Result<String, String> + Send>))
                    .collect();
                self.versions_job(n.clone(), tf!("Version {n} supprimée", "Version {n} removed"), work);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_listed_newest_first() {
        let mut v = vec!["16.0", "saas-18.3", "20.0", "master", "17.0", "saas-19.1", "19.0", "18.0", "17.0-fix"];
        v.sort_by_key(|n| version_key(n));
        assert_eq!(v, ["master", "20.0", "saas-19.1", "19.0", "saas-18.3", "18.0", "17.0", "17.0-fix", "16.0"]);
    }
}
