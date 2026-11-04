//! Préférences → Claude → Comptes: the Claude accounts (work, personal…), each logged in once
//! in a terminal of Florm; the one of the new conversations and of the commit messages.

use super::*;
use crate::claude_account::{self, Status};

#[derive(Default)]
pub(in crate::app) struct AccountsUi {
    /// What `claude auth status` said of each account (by name; None: being asked).
    status: HashMap<String, Option<Result<Status, String>>>,
    /// The name of the account being added.
    new_name: String,
    /// The account being renamed: its index and its new name.
    rename: Option<(usize, String)>,
    /// The terminal of a login running, and the account.
    login: Option<(u64, String)>,
    /// The account a folder is being chosen for (in the chooser of the system).
    picking: Option<String>,
}

/// What was clicked in the list of the accounts.
enum AccAct {
    Login(usize),
    Default(usize),
    Remove(usize),
    Rename(usize, String),
    Add(String),
    Refresh,
    /// Choose a folder for the account: one whose conversations use it, or its configuration.
    Pick(usize, crate::os::Pick),
    /// The folder rule (index in `claude_account_folders`) taken away.
    Unfolder(usize),
}

impl App {
    /// The state of each account asked again (at the first showing, after a login).
    fn claude_accounts_refresh(&mut self) {
        for a in self.claude_accounts() {
            self.prefs.accounts.status.insert(a.name.clone(), None);
            let (name, dir) = (a.name, a.dir);
            self.jobs.spawn(move || JobResult::ClaudeAuth(name, claude_account::status(dir.as_deref())));
        }
    }

    pub(in crate::app) fn claude_auth_loaded(&mut self, name: String, r: Result<Status, String>) {
        self.prefs.accounts.status.insert(name, Some(r));
    }

    pub(super) fn claude_accounts_prefs(&mut self, ui: &mut egui::Ui, rc: &mut RowCtx) {
        let title = t!("Comptes Claude", "Claude accounts");
        let desc = t!(
            "Un compte par abonnement (pro, perso…), chacun connecté une fois. Le menu 👤 d'une conversation choisit son compte ; celui marqué « par défaut » sert aux nouvelles conversations et aux messages de commit, descriptions de PR et revues.",
            "One account per subscription (work, personal…), each logged in once. The 👤 menu of a conversation chooses its account; the one marked “default” is used by new conversations and by the commit messages, PR descriptions and reviews."
        );
        if !rc.matches(&[title, desc, t!("compte", "account"), t!("connexion", "login")]) {
            return;
        }
        // a login ended in its terminal: the states are asked again
        if let Some((id, _)) = &self.prefs.accounts.login {
            if self.term.exited(*id) || self.term.title_of(*id).is_none() {
                self.prefs.accounts.login = None;
                self.claude_accounts_refresh();
            }
        }
        let accounts = self.claude_accounts();
        if accounts.iter().any(|a| !self.prefs.accounts.status.contains_key(&a.name)) {
            self.claude_accounts_refresh();
        }
        let default = claude_account::find(&accounts, &self.settings.claude_account).name.clone();
        let accent = self.hl.colors.accent;
        let mut act: Option<AccAct> = None;
        rc.header(ui);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).strong());
            if ui.small_button("⟳").on_hover_text(t!("Revérifier les connexions", "Check the logins again")).clicked() {
                act = Some(AccAct::Refresh);
            }
        });
        ui.add(egui::Label::new(RichText::new(desc).weak().size(12.5)).wrap());
        ui.add_space(6.0);
        let a_ui = &mut self.prefs.accounts;
        let logging = a_ui.login.as_ref().map(|(_, n)| n.clone());
        for (k, a) in accounts.iter().enumerate() {
            egui::Frame::none().fill(ui.visuals().faint_bg_color).rounding(6.0).inner_margin(egui::Margin::symmetric(10.0, 8.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    match &mut a_ui.rename {
                        Some((i, name)) if *i == k => {
                            let r = crate::input::add(ui, egui::TextEdit::singleline(name).desired_width(160.0));
                            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if enter || ui.small_button("✔").clicked() {
                                act = Some(AccAct::Rename(k, name.clone()));
                            }
                        }
                        _ => {
                            ui.label(RichText::new(format!("👤 {}", a.name)).strong());
                            if ui.small_button("✏").on_hover_text(t!("Renommer", "Rename")).clicked() {
                                a_ui.rename = Some((k, a.name.clone()));
                            }
                        }
                    }
                    if a.name == default {
                        ui.label(RichText::new(t!("par défaut", "default")).small().color(accent));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if accounts.len() > 1 && ui.small_button("×").on_hover_text(t!("Retirer ce compte de Florm (son dossier et sa connexion restent)", "Remove this account from Florm (its folder and its login stay)")).clicked() {
                            act = Some(AccAct::Remove(k));
                        }
                        if a.name != default && ui.small_button(t!("Par défaut", "Default")).on_hover_text(t!("Pour les nouvelles conversations et les messages de commit…", "For new conversations and the commit messages…")).clicked() {
                            act = Some(AccAct::Default(k));
                        }
                        let logged = matches!(a_ui.status.get(&a.name), Some(Some(Ok(s))) if s.logged_in);
                        let label = if logged { t!("Changer de compte…", "Switch account…") } else { t!("Se connecter…", "Log in…") };
                        if ui.add_enabled(logging.is_none(), egui::Button::new(label).small()).on_hover_text(t!("claude auth login dans un terminal de Florm : le navigateur s'ouvre pour la connexion", "claude auth login in a terminal of Florm: the browser opens for the login")).clicked() {
                            act = Some(AccAct::Login(k));
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    match a_ui.status.get(&a.name) {
                        _ if logging.as_deref() == Some(a.name.as_str()) => {
                            spinner(ui);
                            ui.label(RichText::new(t!("connexion dans le terminal…", "logging in, in the terminal…")).small().weak());
                        }
                        Some(None) | None => {
                            spinner(ui);
                        }
                        Some(Some(Ok(s))) if s.logged_in => {
                            let plan = if s.plan.is_empty() { String::new() } else { format!(" · {}", s.plan) };
                            ui.label(RichText::new(format!("✔ {}{plan}", s.email)).small().color(ok()));
                        }
                        Some(Some(Ok(_))) => {
                            ui.label(RichText::new(t!("Pas connecté", "Not logged in")).small().color(warn()));
                        }
                        Some(Some(Err(e))) => {
                            ui.label(RichText::new(e.lines().next().unwrap_or("")).small().color(err()));
                        }
                    }
                    let folder = claude_account::config_dir(a.dir.as_deref());
                    ui.label(RichText::new(folder.display().to_string()).small().weak().monospace())
                        .on_hover_text(t!("Le dossier de configuration de ce compte (sa connexion, ses sessions)", "The configuration folder of this account (its login, its sessions)"));
                    if ui.small_button("📁").on_hover_text(t!("Choisir un autre dossier de configuration (un dossier déjà connecté, par exemple)", "Choose another configuration folder (one already logged in, for instance)")).clicked() {
                        act = Some(AccAct::Pick(k, crate::os::Pick::AccountConfig));
                    }
                });
                // the folders whose conversations use this account
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(t!("Utilisé dans :", "Used in:")).small().weak());
                    for (r, rule) in self.settings.claude_account_folders.iter().enumerate() {
                        let Some((f, name)) = rule.rsplit_once('=') else { continue };
                        if name.trim() != a.name {
                            continue;
                        }
                        let short = Path::new(f.trim()).file_name().map_or(f.to_string(), |n| n.to_string_lossy().into_owned());
                        ui.label(RichText::new(format!("📁 {short}")).small().color(accent)).on_hover_text(f.trim());
                        if ui.small_button("×").on_hover_text(t!("Ce dossier n'utilise plus ce compte", "This folder no longer uses this account")).clicked() {
                            act = Some(AccAct::Unfolder(r));
                        }
                    }
                    if ui.small_button(t!("+ Dossier…", "+ Folder…")).on_hover_text(t!("Les nouvelles conversations de ce dossier (et de ses sous-dossiers) utilisent ce compte", "New conversations of this folder (and of its subfolders) use this account")).clicked() {
                        act = Some(AccAct::Pick(k, crate::os::Pick::AccountProject));
                    }
                });
            });
            ui.add_space(4.0);
        }
        ui.horizontal(|ui| {
            let r = crate::input::add(ui, egui::TextEdit::singleline(&mut a_ui.new_name).desired_width(180.0).hint_text(t!("Nom du compte (Perso…)", "Name of the account (Personal…)")));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let name = a_ui.new_name.trim().to_string();
            let can = !name.is_empty() && !name.contains('=') && !accounts.iter().any(|a| a.name == name);
            if ui.add_enabled(can, egui::Button::new(t!("+ Ajouter un compte", "+ Add an account"))).clicked() || (enter && can) {
                act = Some(AccAct::Add(name));
            }
        });
        ui.add_space(10.0);
        let Some(act) = act else { return };
        // the list is written in full: the usual account gets its name once there are two
        let mut specs: Vec<String> = accounts.iter().map(|a| claude_account::spec(&a.name, a.dir.as_deref())).collect();
        match act {
            AccAct::Refresh => {
                self.claude_accounts_refresh();
                return;
            }
            AccAct::Pick(k, kind) => {
                self.prefs.accounts.picking = Some(accounts[k].name.clone());
                self.file_dialog(kind);
                return;
            }
            AccAct::Unfolder(r) => {
                self.settings.claude_account_folders.remove(r);
                self.persist("claude_account_folders", config::toml_str_array(&self.settings.claude_account_folders));
                return;
            }
            AccAct::Login(k) => {
                let (argv, env) = claude_account::login_command(accounts[k].dir.as_deref());
                if let Some(d) = &accounts[k].dir {
                    let _ = std::fs::create_dir_all(d);
                }
                let ctx = self.egui_ctx.clone();
                match self.term.open_once_panel(&ctx, &argv, None, &env) {
                    Ok(id) => {
                        self.term.set_name(id, tf!("Connexion Claude : {}", "Claude login: {}", accounts[k].name));
                        self.bottom = Some(BottomTab::Terminal);
                        self.prefs.accounts.login = Some((id, accounts[k].name.clone()));
                    }
                    Err(e) => self.set_error(tf!("Terminal impossible à ouvrir : {e}", "Cannot open a terminal: {e}")),
                }
                return;
            }
            AccAct::Default(k) => {
                self.settings.claude_account = accounts[k].name.clone();
                self.persist("claude_account", config::toml_str(&self.settings.claude_account));
                crate::claude_account::set_default(accounts[k].dir.clone());
                return;
            }
            AccAct::Remove(k) => {
                specs.remove(k);
                let gone = accounts[k].name.clone();
                self.settings.claude_account_folders.retain(|r| r.rsplit_once('=').is_none_or(|(_, a)| a.trim() != gone));
                self.persist("claude_account_folders", config::toml_str_array(&self.settings.claude_account_folders));
            }
            AccAct::Rename(k, name) => {
                self.prefs.accounts.rename = None;
                let name = name.trim().to_string();
                if name.is_empty() || name.contains('=') || accounts.iter().enumerate().any(|(i, a)| i != k && a.name == name) {
                    return;
                }
                let old = accounts[k].name.clone();
                specs[k] = claude_account::spec(&name, accounts[k].dir.as_deref());
                // the conversations open and the default follow the new name
                for c in std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()) {
                    for n in [&mut c.account, &mut c.session_account] {
                        if *n == old {
                            *n = name.clone();
                        }
                    }
                }
                if self.settings.claude_account == old {
                    self.settings.claude_account = name.clone();
                    self.persist("claude_account", config::toml_str(&name));
                }
                // its folders follow
                for r in self.settings.claude_account_folders.iter_mut() {
                    if let Some((f, _)) = r.rsplit_once('=').filter(|(_, a)| a.trim() == old) {
                        *r = format!("{f}={name}");
                    }
                }
                self.persist("claude_account_folders", config::toml_str_array(&self.settings.claude_account_folders));
                if let Some(s) = self.prefs.accounts.status.remove(&old) {
                    self.prefs.accounts.status.insert(name, s);
                }
            }
            AccAct::Add(name) => {
                self.prefs.accounts.new_name.clear();
                specs.push(claude_account::spec(&name, Some(&claude_account::folder_for(&name))));
            }
        }
        self.settings.claude_accounts = specs;
        self.persist("claude_accounts", config::toml_str_array(&self.settings.claude_accounts));
        crate::claude_account::set_default(super::claude_ui::default_account_dir(&self.settings));
    }
}

impl App {
    /// A folder chosen for an account: one whose conversations use it, or its configuration.
    pub(in crate::app) fn claude_account_picked(&mut self, kind: crate::os::Pick, path: PathBuf) {
        let Some(name) = self.prefs.accounts.picking.take() else { return };
        if kind == crate::os::Pick::AccountProject {
            // one account per folder: its rule of before goes
            let rules = &mut self.settings.claude_account_folders;
            rules.retain(|r| r.rsplit_once('=').is_none_or(|(f, _)| Path::new(f.trim()) != path));
            rules.push(claude_account::folder_rule(&path, &name));
            self.persist("claude_account_folders", config::toml_str_array(&self.settings.claude_account_folders));
            self.set_status(tf!("Les conversations de {} utilisent le compte {name}", "The conversations of {} use the account {name}", path.display()));
            return;
        }
        let accounts = self.claude_accounts();
        let specs: Vec<String> = accounts
            .iter()
            .map(|a| if a.name == name { claude_account::spec(&a.name, Some(&path)) } else { claude_account::spec(&a.name, a.dir.as_deref()) })
            .collect();
        self.settings.claude_accounts = specs;
        self.persist("claude_accounts", config::toml_str_array(&self.settings.claude_accounts));
        crate::claude_account::set_default(super::claude_ui::default_account_dir(&self.settings));
        self.claude_accounts_refresh();
    }
}
