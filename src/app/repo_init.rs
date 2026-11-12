//! New git repository from the tree (right click on a folder outside any repository): `git init`,
//! the remote to add, then everything committed (`git add .`) or an empty commit, and pushed.

use super::*;
use crate::git::FirstCommit;

pub(super) struct RepoInitUi {
    dir: PathBuf,
    remote: String,
    branch: String,
    first: FirstCommit,
    message: String,
    pub(super) busy: bool,
    pub(super) error: Option<String>,
    focus: bool,
}

impl App {
    /// The window of a new repository in `dir`.
    pub(super) fn init_repo(&mut self, dir: PathBuf) {
        self.repo_init = Some(RepoInitUi {
            dir,
            remote: String::new(),
            branch: "main".into(),
            first: FirstCommit::All,
            message: t!("Premier commit", "Initial commit").into(),
            busy: false,
            error: None,
            focus: true,
        });
    }

    pub(super) fn repo_init_ui(&mut self, ctx: &egui::Context) {
        let accent = self.hl.colors.accent;
        let Some(st) = self.repo_init.as_mut() else { return };
        let mut open = true;
        let (mut go, mut cancel) = (false, false);
        egui::Window::new(t!("Nouveau dépôt git", "New git repository"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("repo_init_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label(RichText::new(t!("Dossier", "Folder")).weak());
                    ui.label(RichText::new(st.dir.display().to_string()).monospace().color(accent));
                    ui.end_row();
                    ui.label(RichText::new(t!("Remote", "Remote")).weak());
                    let r = crate::input::add(ui, egui::TextEdit::singleline(&mut st.remote).desired_width(400.0).hint_text("git@github.com:moi/projet.git"));
                    if std::mem::take(&mut st.focus) {
                        r.request_focus();
                    }
                    r.on_hover_text(t!("L'adresse du dépôt distant, ajoutée comme « origin » (vide : pas de remote, rien n'est poussé). Sur GitHub, crée d'abord le dépôt vide.", "The address of the remote repository, added as “origin” (empty: no remote, nothing is pushed). On GitHub, create the empty repository first."));
                    ui.end_row();
                    ui.label(RichText::new(t!("Branche", "Branch")).weak());
                    crate::input::add(ui, egui::TextEdit::singleline(&mut st.branch).desired_width(160.0));
                    ui.end_row();
                    ui.label(RichText::new(t!("Ensuite", "Then")).weak());
                    ui.vertical(|ui| {
                        ui.radio_value(&mut st.first, FirstCommit::All, t!("Tout ajouter (git add .) et faire le premier commit", "Add everything (git add .) and make the first commit"));
                        ui.radio_value(&mut st.first, FirstCommit::Empty, t!("Un commit vide, sans aucun fichier", "An empty commit, without any file"));
                        ui.radio_value(&mut st.first, FirstCommit::Nothing, t!("Rien : juste git init", "Nothing: just git init"));
                    });
                    ui.end_row();
                    if st.first != FirstCommit::Nothing {
                        ui.label(RichText::new(t!("Message", "Message")).weak());
                        crate::input::add(ui, egui::TextEdit::singleline(&mut st.message).desired_width(400.0));
                        ui.end_row();
                    }
                });
                let push = !st.remote.trim().is_empty() && st.first != FirstCommit::Nothing;
                ui.add_space(4.0);
                ui.label(
                    RichText::new(if push {
                        tf!("Le commit sera poussé sur origin/{}.", "The commit will be pushed to origin/{}.", st.branch.trim())
                    } else {
                        t!("Rien ne sera poussé.", "Nothing will be pushed.").to_string()
                    })
                    .small()
                    .weak(),
                );
                if let Some(e) = &st.error {
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                        ui.label(RichText::new(e).color(err()).monospace().small());
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let label = if push { t!("Créer et pousser", "Create and push") } else { t!("Créer le dépôt", "Create the repository") };
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.add_enabled(!st.busy, filled_button(label, accent)).clicked() || (enter && !st.busy) {
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
            self.repo_init = None;
            return;
        }
        if !go {
            return;
        }
        let Some(root) = self.root.clone() else { return };
        st.busy = true;
        st.error = None;
        let (dir, remote, branch, first, message) = (st.dir.clone(), st.remote.clone(), st.branch.clone(), st.first, st.message.clone());
        self.set_status(t!("Création du dépôt git…", "Creating the git repository…"));
        self.jobs.spawn(move || {
            let result = git::init_repo(&dir, &remote, &branch, first, &message);
            JobResult::RepoInit { root: root.clone(), dir, result, repos: git::find_repos(&root, 3) }
        });
    }
}
