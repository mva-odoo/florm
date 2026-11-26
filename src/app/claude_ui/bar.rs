//! The bar above a conversation with Claude: its folder, a new one, its account, what to give it
//! (« + Contexte »), the conversations kept.

use super::*;

/// What the bar shows besides the conversation.
pub(super) struct BarData<'a> {
    pub(super) dirs: &'a [(PathBuf, String)],
    pub(super) dir_name: &'a str,
    pub(super) history: &'a [(u64, String, String)],
    pub(super) accounts: &'a [claude_account::Account],
    pub(super) account_name: &'a str,
}

/// The keys of the system that copy a screen capture to the clipboard.
fn system_shot_keys() -> &'static str {
    if cfg!(target_os = "macos") {
        t!("Cmd+Ctrl+Maj+4", "Cmd+Ctrl+Shift+4")
    } else if cfg!(windows) {
        t!("Win+Maj+S", "Win+Shift+S")
    } else {
        t!("Impr (ou Maj+Impr pour une zone)", "Print (or Shift+Print for an area)")
    }
}

/// What was asked in the bar, done once it is drawn (when `self` is free).
#[derive(Default)]
pub(super) struct Acts {
    pub(super) dir: Option<PathBuf>,
    pub(super) pick_dir: bool,
    pub(super) ctx_add: Option<CtxAdd>,
    pub(super) read_commits: bool,
    pub(super) clipboard: bool,
    pub(super) open_saved: Option<u64>,
    pub(super) forget_saved: Option<u64>,
    pub(super) account: Option<String>,
    /// « Gérer les comptes… »: Préférences → Claude.
    pub(super) manage_accounts: bool,
    pub(super) restart: bool,
}

/// « + Contexte »: a file, the selection, the diff, a commit, the problems, a picture, written
/// into Claude's field.
fn context_menu(ui: &mut egui::Ui, c: &mut ClaudeUi, a: &mut Acts) {
    let menu = ui.menu_button(t!("+ Contexte", "+ Context"), |ui| {
        ui.set_min_width(260.0);
        for (label, tip, what) in [
            (t!("Fichier ouvert", "Open file"), t!("Le fichier en entier (@fichier : Claude le lit)", "The whole file (@file: Claude reads it)"), CtxAdd::File),
            (t!("Sélection (ou ligne du curseur)", "Selection (or cursor line)"), t!("Les lignes sélectionnées dans l'éditeur, avec leur texte", "The lines selected in the editor, with their text"), CtxAdd::Selection),
            (t!("Tous les onglets ouverts", "All open tabs"), t!("Chaque fichier ouvert dans l'éditeur", "Each file open in the editor"), CtxAdd::Tabs),
            (t!("Changements non commités", "Uncommitted changes"), t!("Le diff de ce qui n'est pas commité (git diff HEAD, nouveaux fichiers compris)", "The diff of what is not committed (git diff HEAD, new files included)"), CtxAdd::Diff),
            (t!("Problèmes du fichier", "Problems of the file"), t!("Les erreurs et avertissements du linter dans le fichier ouvert", "The linter errors and warnings in the open file"), CtxAdd::Problems),
            (t!("Image ou PDF…", "Image or PDF…"), t!("Des images ou des PDF choisis sur le disque (aussi : glisse-les sur Claude)", "Images or PDFs chosen on the disk (also: drop them on Claude)"), CtxAdd::Image),
        ] {
            if ui.button(label).on_hover_text(tip).clicked() {
                a.ctx_add = Some(what);
                ui.close();
            }
        }
        // the last commits of the folder of the conversation, read when the menu opens
        ui.menu_button(t!("Un commit", "A commit"), |ui| {
            ui.set_min_width(420.0);
            if c.recent_commits.is_empty() {
                ui.label(RichText::new(t!("Aucun commit", "No commit")).weak());
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (hash, short, subject, ago) in &c.recent_commits {
                    if ui.button(format!("{short}  {subject}")).on_hover_text(tf!("{ago} — son message et son diff\n(aussi : clic droit sur un commit dans la vue Git)", "{ago} — its message and diff\n(also: right click on a commit in the Git view)")).clicked() {
                        a.ctx_add = Some(CtxAdd::Commit(hash.clone()));
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        let clip = ui
            .add_enabled(c.clip_image, egui::Button::new(t!("Image du presse-papiers", "Image from the clipboard")))
            .on_hover_text(t!("L'image copiée (une capture faite avec Impr, une image copiée ailleurs)", "The copied image (a capture made with Print, an image copied elsewhere)"))
            .on_disabled_hover_text(t!("Pas d'image dans le presse-papiers : fais une capture et rouvre ce menu", "No image in the clipboard: take a capture and open this menu again"));
        if clip.clicked() {
            a.clipboard = true;
            ui.close();
        }
        ui.label(RichText::new(tf!("Capture de l'écran dans le presse-papiers : {}", "Screen capture to the clipboard: {}", system_shot_keys())).small().weak());
        ui.separator();
        ui.label(RichText::new(t!("Écrit dans le champ de Claude, sans l'envoyer. Aussi : clic droit dans l'éditeur, ou ✳ dans la marge pour des lignes", "Written in Claude's field, not sent. Also: right click in the editor, or ✳ in the margin for lines")).small().weak());
    });
    if menu.response.clicked() {
        a.read_commits = true;
        c.clip_image = clipboard_has_image();
    }
    menu.response.on_hover_text(t!("Donner à Claude un fichier, la sélection, le diff, un commit, les problèmes, une image…", "Give Claude a file, the selection, the diff, a commit, the problems, an image…"));
}

/// The bar above the conversation.
pub(super) fn bar(ui: &mut egui::Ui, c: &mut ClaudeUi, accent: Color32, d: BarData<'_>) -> Acts {
    let BarData { dirs, dir_name, history, accounts, account_name } = d;
    let mut a = Acts::default();
    // as the breadcrumb of a file: `✳ Claude › ● florm`, on the editor's background
    crate::view_bar::header(egui::Id::unique("claude_bar").with(c.id), ui)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("✳ Claude").font(crate::view_bar::crumb_font()).color(accent));
                crate::view_bar::sep(ui);
                // its folder is set once something was said
                ui.add_enabled_ui(c.is_empty(), |ui| {
                    let mut labels: Vec<String> = dirs.iter().map(|(_, l)| l.clone()).collect();
                    labels.push(t!("📁 Un autre dossier…", "📁 Another folder…").to_string());
                    let current = dirs.iter().position(|(d, _)| c.dir.as_ref() == Some(d));
                    // in the color of its project, as in the list of the conversations
                    let color = hub::project_color(dir_name, ui.visuals().dark_mode);
                    let text = RichText::new(format!("● {dir_name}")).font(crate::view_bar::crumb_font()).color(color);
                    match crate::view_bar::flat(ui, |ui| crate::pick::combo(ui, ("claude_dir", c.id), text, &labels, current)) {
                        Some(k) if k < dirs.len() => a.dir = Some(dirs[k].0.clone()),
                        Some(_) => a.pick_dir = true,
                        None => {}
                    }
                })
                .response
                .on_disabled_hover_text(t!("Pour changer : « Nouvelle conversation »", "To change: “New conversation”"))
                .on_hover_text(t!("Le dossier où Claude travaille : un dépôt du projet, un projet récent ou n'importe quel dossier", "The folder Claude works in: a repository of the project, a recent project or any folder"));
                context_menu(ui, c, &mut a);
                // the account it runs with (its session follows it); always there, to find how to add one
                ui.menu_button(format!("👤 {account_name}"), |ui| {
                    ui.set_min_width(200.0);
                    for x in accounts {
                        if ui.selectable_label(x.name == account_name, &x.name).clicked() && x.name != account_name {
                            a.account = Some(x.name.clone());
                            ui.close();
                        }
                    }
                    ui.separator();
                    let label = if accounts.len() > 1 { t!("Gérer les comptes…", "Manage the accounts…") } else { t!("+ Ajouter un compte…", "+ Add an account…") };
                    if ui.button(label).on_hover_text(t!("Préférences → Claude → Comptes Claude : un autre abonnement (pro, perso…), sa connexion, ses dossiers", "Preferences → Claude → Claude accounts: another subscription (work, personal…), its login, its folders")).clicked() {
                        a.manage_accounts = true;
                        ui.close();
                    }
                })
                .response
                .on_hover_text(t!("Le compte Claude de cette conversation (son abonnement et ses quotas) ; changé, Claude redémarre avec lui, la conversation continue", "The Claude account of this conversation (its subscription and quotas); changed, Claude starts again with it, the conversation goes on"));
                ui.menu_button(tf!("Historique ({})", "History ({})", history.len()), |ui| {
                    ui.set_min_width(420.0);
                    if history.is_empty() {
                        ui.label(RichText::new(t!("Les conversations sont gardées ici, même après avoir fermé Florm (50 au plus).", "Conversations are kept here, even after closing Florm (50 at most).")).weak());
                    }
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        for (key, title, info) in history {
                            ui.horizontal(|ui| {
                                if ui.button(title).on_hover_text(t!("La rouvrir : Claude reprend avec son contexte", "Reopen it: Claude goes on with its context")).clicked() {
                                    a.open_saved = Some(*key);
                                    ui.close();
                                }
                                ui.label(RichText::new(info).small().weak());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("×").on_hover_text(t!("L'oublier", "Forget it")).clicked() {
                                        a.forget_saved = Some(*key);
                                    }
                                });
                            });
                        }
                    });
                })
                .response
                .on_hover_text(t!("Les conversations d'avant, gardées entre les sessions", "The earlier conversations, kept between sessions"));
                if c.term.is_some() && ui.button("⟳").on_hover_text(t!("Relancer Claude : la conversation reprend où elle en était", "Restart Claude: the conversation goes on where it was")).clicked() {
                    a.restart = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::view_bar::controls(ui, None, None, None);
                });
            });
        });
    a
}
