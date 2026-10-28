//! The bar of the Git view: branch, sync, commit, stash, and the errors of git.

use super::*;

/// The Stash menu: put the changes aside (with a message), then the stashes, newest first, each
/// to look at, apply (it stays), take back (applied then deleted) or delete.
pub(super) fn stash_menu(ui: &mut egui::Ui, g: &mut GitUi, changes: bool, act: &mut Option<super::git_actions::StashAct>) {
    use super::git_actions::StashAct;
    ui.set_min_width(460.0);
    ui.label(RichText::new(t!("Mettre de côté", "Put aside")).strong());
    crate::input::add(ui, egui::TextEdit::singleline(&mut g.stash_msg).hint_text(t!("Message (facultatif)", "Message (optional)")).desired_width(f32::INFINITY));
    ui.horizontal(|ui| {
        let mut untracked = !g.stash_tracked_only;
        if ui.checkbox(&mut untracked, t!("Nouveaux fichiers compris", "New files included")).on_hover_text(t!("Les fichiers pas encore suivis par git partent aussi (--include-untracked)", "Files not tracked by git yet go too (--include-untracked)")).changed() {
            g.stash_tracked_only = !untracked;
        }
        let tip = t!("Range les changements non commités (indexés ou non) et remet les fichiers comme au dernier commit", "Stores the uncommitted changes (staged or not) and puts the files back as at the last commit");
        if ui.add_enabled(changes, egui::Button::new(t!("Mettre de côté", "Put aside"))).on_hover_text(tip).on_disabled_hover_text(t!("Aucun changement à mettre de côté", "No change to put aside")).clicked() {
            *act = Some(StashAct::Push);
            ui.close_menu();
        }
    });
    ui.separator();
    if g.stashes.is_empty() {
        ui.label(RichText::new(t!("Aucun stash dans ce dépôt.", "No stash in this repository.")).weak());
        return;
    }
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        for s in &g.stashes {
            // "On main: message" → "message", the branch apart
            let (branch, what) = s.message.split_once(": ").unwrap_or(("", s.message.as_str()));
            let branch = branch.trim_start_matches("WIP ").trim_start_matches("On ");
            ui.horizontal(|ui| {
                ui.label(RichText::new(&s.name).monospace().small().weak());
                ui.label(RichText::new(what).strong()).on_hover_text(&s.message);
                ui.label(RichText::new(format!("{branch} · {}", git::ago(s.time))).small().weak());
            });
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                if ui.small_button(t!("Voir", "Show")).on_hover_text(t!("Ses changements dans la vue des commits (sans les nouveaux fichiers)", "Its changes in the commits view (without the new files)")).clicked() {
                    *act = Some(StashAct::Show(s.hash.clone(), what.to_string()));
                    ui.close_menu();
                }
                if ui.small_button(t!("Appliquer", "Apply")).on_hover_text(t!("Remet ses changements dans tes fichiers ; le stash reste (git stash apply)", "Puts its changes back in your files; the stash stays (git stash apply)")).clicked() {
                    *act = Some(StashAct::Apply(s.hash.clone()));
                    ui.close_menu();
                }
                if ui.small_button(t!("Reprendre", "Pop")).on_hover_text(t!("Remet ses changements et supprime le stash (git stash pop)", "Puts its changes back and deletes the stash (git stash pop)")).clicked() {
                    *act = Some(StashAct::Pop(s.hash.clone()));
                    ui.close_menu();
                }
                if ui.small_button(RichText::new(t!("Supprimer", "Delete")).color(err())).on_hover_text(t!("git stash drop (demande confirmation)", "git stash drop (asks for confirmation)")).clicked() {
                    *act = Some(StashAct::AskDrop(s.hash.clone(), what.to_string()));
                    ui.close_menu();
                }
            });
            ui.add_space(4.0);
        }
    });
}

/// What the ⋯ menu of the git view asked for, beyond a command.
pub(super) enum GitBarAct {
    Search(git::SearchKind),
    AddRemote,
    RemoveRemote(String),
    AddSubmodule,
    /// `git submodule …` with these arguments.
    Submodules(Vec<String>),
}

impl App {
    /// Top of the git view: branch and its upstream, pull / push / fetch / branches, git errors,
    /// then the commit message and button (the changes themselves are the first rows of the graph).
    pub(in crate::app) fn git_bar(&mut self, ui: &mut egui::Ui) {
        if self.repo.is_none() {
            ui.label(t!("Aucun dépôt git dans ce dossier.", "No git repository in this folder."));
            if let Some(root) = self.root.clone() {
                if ui.button("git init").clicked() {
                    match git::run(&root, &["init"]) {
                        Ok(o) => {
                            self.log(o);
                            self.repos = git::find_repos(&root, 3);
                            if let Some(r) = self.repos.first().cloned() {
                                self.select_repo(r);
                            }
                        }
                        Err(e) => self.log(e),
                    }
                }
            }
            return;
        }
        let bg = ui.visuals().panel_fill;
        let mut dismiss_error = false;
        let (mut chosen_repo, mut reload) = (None, false);
        let repo_name = self.history.repo.as_deref().map(|r| self.repo_label(r)).unwrap_or_default();
        let repo_choices: Vec<(PathBuf, String)> = self.repos.iter().map(|r| (r.clone(), self.repo_label(r))).collect();
        // pull requests of the branch, or the button to open one
        let branch_prs = self.current_branch_prs();
        let (mut open_pr, mut create_pr) = (None, false);
        let main_branch = self.git.status.as_ref().is_some_and(|s| {
            let b = s.branch.as_str();
            let default = self.repo.as_ref().and_then(|r| self.gh.base_default(r));
            Some(b) == default.as_deref() || b == "master" || b == "main" || b.trim_start_matches("saas-").split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        });

        // ⇱ / ⇲: the view moves between the editor area and the bottom panel
        let (big, mut move_to) = (self.has_tab(document::View::Git), None);
        let (mut stash_act, mut stash_open) = (None, false);
        // --- branch, its upstream and the remote actions
        let mut cmds = Vec::new();
        let mut bar_act: Option<GitBarAct> = None;
        let remotes = self.git.remotes.clone();
        let repo_dir = self.history.repo.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // the repository first and large (easy to mistake in a project with several)
            let labels: Vec<String> = repo_choices.iter().map(|(_, l)| l.clone()).collect();
            let current = repo_choices.iter().position(|(r, _)| self.history.repo.as_ref() == Some(r));
            if let Some(k) = crate::view_bar::repo_chip(ui, "history_repo", &repo_name, &labels, current) {
                chosen_repo = Some(repo_choices[k].0.clone());
            }
            if let Some(st) = &self.git.status {
                // the branch and its menu are one: switch, or create a new one
                ui.menu_button(RichText::new(format!("🔀 {} ▾", st.branch)).strong().size(15.0), |ui| {
                    if ui.button(t!("Changer de branche…", "Switch branch…")).clicked() {
                        cmds.push(Cmd::GitCheckout);
                        ui.close_menu();
                    }
                    if ui.button(t!("+ Nouvelle branche…", "+ New branch…")).clicked() {
                        cmds.push(Cmd::GitNewBranch);
                        ui.close_menu();
                    }
                    ui.separator();
                    let b = &st.branch;
                    for (label, cmd) in [
                        (tf!("Fusionner une branche dans « {b} »…", "Merge a branch into “{b}”…"), Cmd::GitMerge),
                        (tf!("Rebaser « {b} » sur une branche…", "Rebase “{b}” onto a branch…"), Cmd::GitRebaseOnto),
                        (t!("Renommer une branche…", "Rename a branch…").to_string(), Cmd::GitRenameBranch),
                        (t!("Supprimer une branche…", "Delete a branch…").to_string(), Cmd::GitDeleteBranch),
                        (t!("Modifier le dernier commit…", "Amend the last commit…").to_string(), Cmd::GitAmend),
                    ] {
                        if ui.button(label).clicked() {
                            cmds.push(cmd);
                            ui.close_menu();
                        }
                    }
                })
                .response
                .on_hover_text(t!("Changer de branche ou en créer une", "Switch branch or create one"));
                match st.upstream() {
                    Some((up, ahead, behind)) => {
                        if ahead > 0 {
                            ui.label(RichText::new(format!("↑{ahead}")).color(ok()).strong())
                                .on_hover_text(tf!("{ahead} commit(s) à pousser vers {up} (Push)", "{ahead} commit(s) to push to {up} (Push)"));
                        }
                        if behind > 0 {
                            ui.label(RichText::new(format!("↓{behind}")).color(warn()).strong())
                                .on_hover_text(tf!("{behind} commit(s) à récupérer de {up} (Pull)", "{behind} commit(s) to pull from {up} (Pull)"));
                        }
                        if ahead == 0 && behind == 0 {
                            ui.label(RichText::new(t!("✔ à jour", "✔ up to date")).color(ok()).small()).on_hover_text(tf!("Identique à {up}", "Same as {up}"));
                        }
                    }
                    None => {
                        ui.label(RichText::new(t!("pas de branche distante", "no remote branch")).weak().small());
                    }
                }
            }
            for pr in branch_prs.iter().take(3) {
                let (state, color) = super::pulls::state_badge(&pr.state, pr.is_draft);
                let text = RichText::new(format!("⇄ #{} {}", pr.number, state.to_lowercase())).color(color);
                if ui.add(egui::Button::new(text).small()).on_hover_text(tf!("{}\n{} → {}\nClic : ouvrir la pull request", "{}\n{} → {}\nClick: open the pull request", pr.title, pr.head_ref_name, pr.base_ref_name)).clicked() {
                    open_pr = Some(pr.number);
                }
            }
            if crate::features::on(crate::features::Feature::GitHub) && !main_branch && !branch_prs.iter().any(|p| p.state == "OPEN") && ui.small_button(t!("+ Créer une PR", "+ Create a PR")).on_hover_text(t!("Pousser la branche et ouvrir une pull request (choix de la branche cible)", "Push the branch and open a pull request (choice of the target branch)")).clicked() {
                create_pr = true;
            }
            if self.git.busy > 0 {
                spinner(ui);
            }
            ui.add_space(8.0);
            ui.menu_button(t!("↕ Synchroniser ▾", "↕ Sync ▾"), |ui| {
                for (label, cmd, tip) in [
                    ("⬇ Pull", Cmd::GitPull, t!("Récupérer et intégrer les commits distants (merge s'il le faut)", "Fetch and merge the remote commits (merge when needed)")),
                    ("⬇ Pull --rebase", Cmd::GitPullRebase, t!("Récupérer les commits distants et rejouer les tiens par-dessus : pas de commit de merge (tes changements non commités sont mis de côté le temps du pull)", "Fetch the remote commits and replay yours on top: no merge commit (your uncommitted changes are put aside during the pull)")),
                    ("⬆ Push", Cmd::GitPush, t!("Envoyer tes commits", "Send your commits")),
                    ("⬆ Push --force-with-lease…", Cmd::GitForcePush, t!("Après un rebase ou un amend : remplacer la branche distante (refusé si quelqu'un y a poussé entre-temps)", "After a rebase or an amend: replace the remote branch (refused if someone pushed to it meanwhile)")),
                    (t!("⬆ Pousser les tags", "⬆ Push the tags"), Cmd::GitPushTags, t!("git push --tags", "git push --tags")),
                    ("Fetch", Cmd::GitFetch, t!("Récupérer sans intégrer", "Fetch without merging")),
                ] {
                    if ui.button(label).on_hover_text(tip).clicked() {
                        cmds.push(cmd);
                        ui.close_menu();
                    }
                }
            })
            .response
            .on_hover_text(t!("Pull, Push, Fetch : récupérer et envoyer les commits", "Pull, Push, Fetch: get and send the commits"));
            // the graph: repository, filter; the rest under ⋯
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let changes = self.git.status.as_ref().is_some_and(|s| !s.entries.is_empty());
                let (git, all) = (&mut self.git, &mut self.history.all);
                let mut stash_shown = false;
                let mut more = |ui: &mut egui::Ui| {
                    let n = git.stashes.len();
                    let label = if n > 0 { format!("Stash ({n})") } else { "Stash".into() };
                    ui.menu_button(label, |ui| {
                        stash_shown = true;
                        stash_menu(ui, git, changes, &mut stash_act);
                    })
                    .response
                    .on_hover_text(t!("Mettre des changements de côté (git stash) et les reprendre plus tard", "Put changes aside (git stash) and take them back later"));
                    if crate::features::on(crate::features::Feature::GitHub) && ui.button(t!("Pull requests GitHub", "GitHub pull requests")).on_hover_text(t!("Commentaires, suggestions, commits", "Comments, suggestions, commits")).clicked() {
                        cmds.push(Cmd::PullRequests);
                        ui.close_menu();
                    }
                    ui.menu_button(t!("🔍 Rechercher dans l'historique", "🔍 Search the history"), |ui| {
                        for k in git::SearchKind::ALL {
                            if ui.button(format!("{}…", k.label())).clicked() {
                                bar_act = Some(GitBarAct::Search(k));
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.button(t!("Reflog : retrouver un commit perdu…", "Reflog: find a lost commit…")).on_hover_text(t!("Où HEAD est passée : commits, checkouts, resets, rebases. Un commit perdu (reset, rebase, branche supprimée) y est encore.", "Where HEAD has been: commits, checkouts, resets, rebases. A lost commit (reset, rebase, deleted branch) is still there.")).clicked() {
                        cmds.push(Cmd::GitReflog);
                        ui.close_menu();
                    }
                    ui.menu_button(tf!("Remotes ({})", "Remotes ({})", remotes.len()), |ui| {
                        for (name, url) in &remotes {
                            ui.horizontal(|ui| {
                                if ui.small_button("×").on_hover_text(t!("Retirer ce remote (rien n'est supprimé sur le serveur)", "Remove this remote (nothing is deleted on the server)")).clicked() {
                                    bar_act = Some(GitBarAct::RemoveRemote(name.clone()));
                                    ui.close_menu();
                                }
                                ui.label(RichText::new(name).strong());
                                ui.label(RichText::new(url).weak().small());
                            });
                        }
                        if ui.button(t!("+ Ajouter un remote…", "+ Add a remote…")).clicked() {
                            bar_act = Some(GitBarAct::AddRemote);
                            ui.close_menu();
                        }
                    });
                    ui.menu_button(t!("Sous-modules", "Submodules"), |ui| {
                        // read when the menu is open only
                        let submodules = repo_dir.as_deref().map(git::submodules).unwrap_or_default();
                        for p in &submodules {
                            ui.label(RichText::new(format!("📁 {p}")).small());
                        }
                        if !submodules.is_empty() {
                            ui.separator();
                            if ui.button(t!("Initialiser et mettre à jour (update --init --recursive)", "Init and update (update --init --recursive)")).clicked() {
                                bar_act = Some(GitBarAct::Submodules(vec!["submodule".into(), "update".into(), "--init".into(), "--recursive".into()]));
                                ui.close_menu();
                            }
                            if ui.button(t!("Suivre leur branche distante (update --remote)", "Follow their remote branch (update --remote)")).clicked() {
                                bar_act = Some(GitBarAct::Submodules(vec!["submodule".into(), "update".into(), "--remote".into(), "--recursive".into()]));
                                ui.close_menu();
                            }
                            if ui.button(t!("Reprendre les URL de .gitmodules (sync)", "Take the URLs of .gitmodules again (sync)")).clicked() {
                                bar_act = Some(GitBarAct::Submodules(vec!["submodule".into(), "sync".into(), "--recursive".into()]));
                                ui.close_menu();
                            }
                        }
                        if ui.button(t!("+ Ajouter un sous-module…", "+ Add a submodule…")).clicked() {
                            bar_act = Some(GitBarAct::AddSubmodule);
                            ui.close_menu();
                        }
                    });
                    if ui.button(t!("Supprimer les fichiers non suivis…", "Delete the untracked files…")).on_hover_text(t!("git clean : la liste est montrée avant", "git clean: the list is shown first")).clicked() {
                        cmds.push(Cmd::GitClean);
                        ui.close_menu();
                    }
                    if crate::features::on(crate::features::Feature::Claude) && ui.button(t!("✨ Consignes des messages de commit (Claude)…", "✨ Commit message rules (Claude)…")).on_hover_text(t!("Ce que Claude doit suivre en écrivant les messages de ce projet (langue, préfixes…) : .florm/commit.md", "What Claude must follow when writing the messages of this project (language, prefixes…): .florm/commit.md")).clicked() {
                        cmds.push(Cmd::GitCommitRules);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.checkbox(all, t!("Toutes les branches dans le graphe", "All branches in the graph")).changed() {
                        reload = true;
                    }
                };
                match crate::view_bar::controls(ui, big, Some(t!("Rafraîchir l'état et le graphe", "Refresh the status and the graph")), Some(&mut more)) {
                    Some(crate::view_bar::BarAct::Move(b)) => move_to = Some(b),
                    Some(crate::view_bar::BarAct::Refresh) => reload = true,
                    None => {}
                }
                // the stashes read when their menu opens
                if stash_shown && !self.git.stash_shown {
                    stash_open = true;
                }
                self.git.stash_shown = stash_shown;
                let h = &mut self.history;
                if h.loading {
                    spinner(ui);
                }
                crate::input::add(ui, egui::TextEdit::singleline(&mut h.filter).hint_text(t!("🔍 Filtrer : message, auteur, hash, branche…", "🔍 Filter: message, author, hash, branch…")).desired_width(220.0))
                    .on_hover_text(t!("↑ ↓ commits · ← → fichiers du diff · Page ↑ ↓ défiler", "↑ ↓ commits · ← → files of the diff · Page ↑ ↓ scroll"));
            });
        });
        self.pending.extend(cmds);

        if stash_open {
            self.stash_reload();
        }
        if let Some(a) = stash_act {
            self.stash_act(a);
        }
        match move_to {
            Some(true) => self.show_big(BottomTab::Git),
            Some(false) => self.show_bottom(BottomTab::Git, false),
            None => {}
        }
        let mut cmds_after = Vec::new();
        // --- errors stay until dismissed (or until a later git command succeeds)
        if let Some((label, e)) = self.git.op_error.clone().or_else(|| self.git.error.clone().map(|e| ("status".into(), e))) {
            egui::Frame::none()
                .fill(icons::mix(bg, err(), 0.14))
                .stroke(egui::Stroke::new(1.0_f32, err()))
                .rounding(6.0)
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tf!("⚠ git {label} a échoué", "⚠ git {label} failed")).color(err()).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text(t!("Masquer", "Hide")).clicked() {
                                dismiss_error = true;
                            }
                            // a push refused because the branch was rewritten (rebase, amend)
                            let refused = label.starts_with("push") && !label.contains("force") && ["non-fast-forward", "[rejected]", "fetch first"].iter().any(|k| e.contains(k));
                            if refused && ui.small_button(t!("⬆ Forcer (--force-with-lease)…", "⬆ Force (--force-with-lease)…")).on_hover_text(t!("Si tu as réécrit la branche (rebase, amend) : remplacer la branche distante. Sinon, fais plutôt un pull.", "If you rewrote the branch (rebase, amend): replace the remote branch. Otherwise, pull instead.")).clicked() {
                                cmds_after.push(Cmd::GitForcePush);
                            }
                            if ui.small_button(t!("Copier", "Copy")).clicked() {
                                ui.ctx().output_mut(|o| o.copied_text = e.clone());
                            }
                        });
                    });
                    egui::ScrollArea::vertical().id_salt("git_err").max_height(110.0).show(ui, |ui| {
                        ui.label(RichText::new(&e).monospace().small());
                    });
                });
        }
        ui.add_space(4.0);

        if dismiss_error {
            self.git.op_error = None;
            self.git.error = None;
        }
        self.pending.extend(cmds_after);
        match bar_act {
            Some(GitBarAct::Search(k)) => self.open_prompt(Prompt::HistorySearch(k)),
            Some(GitBarAct::AddRemote) => self.open_prompt(Prompt::AddRemote),
            Some(GitBarAct::AddSubmodule) => self.open_prompt(Prompt::AddSubmodule),
            Some(GitBarAct::RemoveRemote(name)) => self.confirm_git(
                t!("Retirer le remote", "Remove the remote"),
                tf!("Retirer le remote « {name} » de ce dépôt ?\nSes branches distantes disparaissent du graphe ; rien n'est supprimé sur le serveur.", "Remove the remote “{name}” from this repository?\nIts remote branches leave the graph; nothing is deleted on the server."),
                t!("Retirer", "Remove"),
                "remote remove",
                vec![vec!["remote".into(), "remove".into(), name]],
                None,
            ),
            Some(GitBarAct::Submodules(args)) => {
                if let Some(repo) = self.repo.clone() {
                    self.git_seq_at(repo, "submodule", vec![args]);
                }
            }
            None => {}
        }
        if let Some(r) = chosen_repo {
            self.select_repo(r.clone());
            // the panel may already be on it while the view still shows another one
            if self.history.repo.as_ref() != Some(&r) {
                self.load_history();
            }
        }
        if reload {
            self.git_refresh();
            self.load_history();
            self.gh_forget_branches();
        }
        if let (Some(n), Some(repo)) = (open_pr, self.repo.clone()) {
            self.open_pr(repo, n);
        }
        if create_pr {
            self.start_pr_create();
        }
    }
}
