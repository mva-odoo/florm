//! Language packs in the app: the page « Langages » of the preferences (each language, its tools
//! there or not, « Installer »), the tools installed by themselves when a file of a language opens
//! (or a bar proposing them, when that is turned off or failed), the installs on a thread.

use super::*;
use crate::packs::{self, Pack, Tool};

#[derive(Default)]
pub(super) struct PacksUi {
    pub list: Vec<Pack>,
    /// Per pack, per tool: there or not (None: not looked yet).
    pub status: Option<Vec<Vec<bool>>>,
    pub checking: bool,
    /// The pack being installed.
    pub busy: Option<String>,
    /// Packs whose bar was closed (until Florm is closed).
    pub later: HashSet<String>,
    /// Packs installed by themselves in this session (tried once: a failure shows the bar).
    pub auto_tried: HashSet<String>,
    /// F5 waits for the debugger being installed: it starts once the install is done.
    pub then_debug: bool,
}

enum PackAct {
    Install(String, Vec<Tool>),
    Type(String),
    Copy(String),
    Folder,
    New,
    Check,
    /// The tree-sitter grammars of a pack turned on or off (the pack's name, its grammars).
    TreeSitter(String, Vec<String>, bool),
}

impl App {
    /// Looks again which tools are there (on a thread).
    pub(super) fn packs_check(&mut self) {
        if self.packs.checking {
            return;
        }
        self.packs.checking = true;
        self.jobs.spawn(|| {
            packs::reload();
            let list = packs::all();
            let status = packs::status(&list);
            JobResult::PackStatus(list, status)
        });
    }

    pub(super) fn on_pack_status(&mut self, list: Vec<Pack>, status: Vec<Vec<bool>>) {
        self.packs.checking = false;
        self.packs.list = list;
        self.packs.status = Some(status);
    }

    fn pack_install(&mut self, name: String, tools: Vec<Tool>) {
        if self.packs.busy.is_some() || tools.is_empty() {
            return;
        }
        let what: Vec<String> = tools.iter().map(|t| t.bin.clone()).collect();
        self.set_status(tf!("{name} : installation de {}…", "{name}: installing {}…", what.join(", ")));
        self.log(tf!("Pack {name} : installation de {}", "Pack {name}: installing {}", what.join(", ")));
        self.packs.busy = Some(name);
        self.jobs.spawn(move || JobResult::PackInstalled(packs::install(&tools)));
    }

    pub(super) fn on_pack_installed(&mut self, r: Result<String, String>) {
        let name = self.packs.busy.take().unwrap_or_default();
        let ok = r.is_ok();
        let r = r.map(|m| tf!("{name} prêt — {m}", "{name} ready — {m}"));
        self.on_tool_installed(r);
        // the debugger installed for an F5: the session starts now
        if std::mem::take(&mut self.packs.then_debug) && ok {
            self.pending.push(Cmd::DebugStart);
        }
        self.packs_check();
    }

    fn pack_act(&mut self, act: PackAct) {
        match act {
            PackAct::Install(name, tools) => self.pack_install(name, tools),
            PackAct::Type(cmd) => self.install_type(&cmd, false),
            PackAct::Copy(cmd) => {
                self.egui_ctx.copy_text(cmd);
                self.set_status(t!("Commande copiée", "Command copied"));
            }
            PackAct::Folder => {
                let dir = packs::user_dir();
                let _ = std::fs::create_dir_all(&dir);
                self.reveal(&dir);
            }
            PackAct::New => {
                let dir = packs::user_dir();
                let _ = std::fs::create_dir_all(&dir);
                let mut p = dir.join("mon-langage.toml");
                let mut n = 2;
                while p.exists() {
                    p = dir.join(format!("mon-langage-{n}.toml"));
                    n += 1;
                }
                if let Err(e) = std::fs::write(&p, PACK_EXAMPLE) {
                    self.set_error(e.to_string());
                    return;
                }
                self.open_path(p);
                self.set_status(t!("Nouveau pack : enregistre-le, puis ⟳ dans Préférences → Langages", "New pack: save it, then ⟳ in Preferences → Languages"));
            }
            PackAct::Check => self.packs_check(),
            PackAct::TreeSitter(syntax, grammars, on) => {
                self.settings.tree_sitter_off.retain(|s| !grammars.contains(s));
                if !on {
                    self.settings.tree_sitter_off.extend(grammars);
                }
                crate::syntax_tree::set_off(&self.settings.tree_sitter_off);
                let list = serde_json::to_string(&self.settings.tree_sitter_off).unwrap_or_else(|_| "[]".into());
                self.persist("tree_sitter_off", list);
                self.set_status(if on {
                    tf!("{syntax} : coloration par l'arbre de syntaxe (tree-sitter)", "{syntax}: highlighting from the syntax tree (tree-sitter)")
                } else {
                    tf!("{syntax} : coloration par expressions régulières", "{syntax}: highlighting by regular expressions")
                });
            }
        }
    }

    /// F5 with the debugger of the file's language missing (and Florm able to install it): it is
    /// installed, then the session starts by itself. True when that happens (F5 waits).
    pub(super) fn debug_needs_install(&mut self) -> bool {
        let Some((name, missing)) = self.debug_missing() else { return false };
        if self.packs.busy.is_some() {
            return false;
        }
        self.packs.then_debug = true;
        self.pack_install(name, missing);
        true
    }

    /// The debugger tools missing for what F5 would debug: (its pack, the tools to install).
    fn debug_missing(&self) -> Option<(String, Vec<Tool>)> {
        let status = self.packs.status.as_ref()?;
        // the language: the command's program (python…), else the open file's
        let by_cmd = self
            .debug_command_line()
            .and_then(|(cmd, dir)| crate::dap::from_command(&cmd, &dir).ok())
            .and_then(|c| match crate::dap::family_of(&c.adapter) {
                "python" => Some("Python"),
                "go" => Some("Go"),
                _ => None,
            });
        let k = match by_cmd {
            Some(name) => self.packs.list.iter().position(|p| p.name == name),
            None => self.docs.get(self.active).and_then(|d| d.path.clone()).and_then(|p| self.packs.list.iter().position(|pk| pk.matches(&p))),
        };
        let pack = &self.packs.list[k?];
        let missing = packs::to_install_kinds(pack, status.get(k?).map(Vec::as_slice).unwrap_or(&[]), &["debug".to_string()]);
        (!missing.is_empty()).then(|| (pack.name.clone(), missing))
    }

    /// Bar above a file whose language pack misses tools Florm can install.
    pub(super) fn pack_bar(&mut self, ui: &mut egui::Ui, i: usize, pane: u8) {
        let Some(path) = self.docs.get(i).filter(|d| d.view == document::View::Text).and_then(|d| d.path.clone()) else { return };
        if self.packs.status.is_none() {
            self.packs_check();
            return;
        }
        let Some(status) = &self.packs.status else { return };
        let Some(k) = self.packs.list.iter().position(|p| p.matches(&path)) else { return };
        let pack = &self.packs.list[k];
        if self.packs.later.contains(&pack.name) || self.settings.packs_ignored.iter().any(|n| n.eq_ignore_ascii_case(&pack.name)) {
            return;
        }
        let missing = packs::to_install_kinds(pack, status.get(k).map(Vec::as_slice).unwrap_or(&[]), &self.settings.packs_kinds);
        if missing.is_empty() {
            return;
        }
        let name = pack.name.clone();
        let busy = self.packs.busy.as_deref() == Some(name.as_str());
        // installed by itself, once per session (nothing asked); the bar only if that failed
        let tried = self.packs.auto_tried.contains(&name);
        if self.settings.packs_auto && !tried {
            if self.packs.busy.is_none() {
                self.packs.auto_tried.insert(name.clone());
                self.pack_install(name, missing);
            }
            return;
        }
        if self.settings.packs_auto && busy {
            return;
        }
        let failed = self.settings.packs_auto && tried;
        let what: Vec<String> = missing.iter().map(|t| format!("{} ({})", t.bin, t.kind_label())).collect();
        let (mut install, mut later, mut never) = (false, false, false);
        let bg = icons::mix(ui.visuals().panel_fill, self.hl.colors.accent, 0.10);
        egui::Panel::top(egui::Id::unique("pack_bar").with(pane))
            .frame(egui::Frame::NONE.fill(bg).inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tf!("⬇ {name}", "⬇ {name}")).strong());
                    if failed {
                        ui.label(RichText::new(t!("L'installation automatique n'a pas marché (voir Sortie) —", "The automatic install did not work (see Output) —")).color(warn()));
                    }
                    ui.label(tf!("Il manque : {}", "Missing: {}", what.join(", ")));
                    if busy {
                        ui.spinner();
                        ui.label(RichText::new(t!("installation…", "installing…")).weak());
                    } else {
                        install = ui
                            .button(t!("Installer", "Install"))
                            .on_hover_text(t!("Dans le dossier des outils de Florm, sans toucher au système (pip, npm, go install, rustup)", "In Florm's tools folder, without touching the system (pip, npm, go install, rustup)"))
                            .clicked();
                    }
                    later = ui.button(t!("Plus tard", "Later")).on_hover_text(t!("Jusqu'à la fermeture de Florm", "Until Florm is closed")).clicked();
                    never = ui.button(t!("Ne plus proposer", "Don't ask again")).on_hover_text(t!("Toujours dans Préférences → Langages", "Still in Preferences → Languages")).clicked();
                });
            });
        if install {
            self.pack_install(name, missing);
        } else if later {
            self.packs.later.insert(name);
        } else if never {
            self.settings.packs_ignored.push(name);
            let list = serde_json::to_string(&self.settings.packs_ignored).unwrap_or_else(|_| "[]".into());
            self.persist("packs_ignored", list);
        }
    }

    /// The top of the page « Langages »: what is installed and how (for all the languages).
    pub(super) fn packs_header(&mut self, ui: &mut egui::Ui) {
        if self.packs.status.is_none() {
            self.packs_check();
        }
        let mut act: Option<PackAct> = None;
        let mut auto = self.settings.packs_auto;
        if ui
            .checkbox(&mut auto, t!("Installer tout seul les outils d'un langage quand un de ses fichiers s'ouvre", "Install the tools of a language by themselves when one of its files opens"))
            .on_hover_text(t!("Sans rien demander, en arrière-plan ; une barre au-dessus du fichier seulement si ça échoue", "Without asking, in the background; a bar above the file only if it fails"))
            .changed()
        {
            self.settings.packs_auto = auto;
            self.persist("packs_auto", auto.to_string());
        }
        // which tools: the language server and the linter are not always both wanted
        let mut kinds_changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(t!("Ce qui s'installe (tout seul ou par « Installer ») :", "What is installed (by itself or by « Install »):")).weak());
            for (kind, label, tip) in [
                ("lsp", t!("Serveur de langage", "Language server"), t!("Complétion, F12, renommer, documentation au survol", "Completion, F12, rename, documentation on hover")),
                ("lint", t!("Linter", "Linter"), t!("Les erreurs soulignées pendant la frappe", "The errors underlined while typing")),
                ("format", t!("Formateur", "Formatter"), t!("Formater le fichier (palette), ou à l'enregistrement", "Format the file (palette), or on saving")),
                ("debug", t!("Débogueur", "Debugger"), t!("Sinon installé au premier F5, quand il sert", "Else installed at the first F5, when it is needed")),
            ] {
                let mut on = self.settings.packs_kinds.iter().any(|k| k == kind);
                if ui.checkbox(&mut on, label).on_hover_text(tip).changed() {
                    self.settings.packs_kinds.retain(|k| k != kind);
                    if on {
                        self.settings.packs_kinds.push(kind.to_string());
                    }
                    kinds_changed = true;
                }
            }
        });
        if kinds_changed {
            let list = serde_json::to_string(&self.settings.packs_kinds).unwrap_or_else(|_| "[]".into());
            self.persist("packs_kinds", list);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button(t!("⟳ Vérifier", "⟳ Check")).on_hover_text(t!("Cherche à nouveau les outils installés, et relit les packs perso", "Looks again for the installed tools, and reads the own packs again")).clicked() {
                act = Some(PackAct::Check);
            }
            if ui.button(t!("+ Nouveau pack…", "+ New pack…")).on_hover_text(t!("Un langage que Florm ne connaît pas, ou d'autres outils pour un langage : un fichier TOML", "A language Florm does not know, or other tools for a language: a TOML file")).clicked() {
                act = Some(PackAct::New);
            }
            if ui.button(t!("📁 Dossier des packs", "📁 Packs folder")).on_hover_text(packs::user_dir().display().to_string()).clicked() {
                act = Some(PackAct::Folder);
            }
            if self.packs.checking {
                ui.spinner();
            }
        });
        for e in packs::errors() {
            ui.label(RichText::new(tf!("⚠ Pack illisible : {e}", "⚠ Unreadable pack: {e}")).color(warn()));
        }
        if let Some(a) = act {
            self.pack_act(a);
        }
    }

    /// The languages, one row each: there or what to install, its own settings; a click opens its
    /// page. Returns the language clicked.
    pub(super) fn packs_page(&mut self, ui: &mut egui::Ui) -> Option<String> {
        if self.packs.status.is_none() {
            self.packs_check();
        }
        let Some(status) = self.packs.status.clone() else {
            ui.spinner();
            return None;
        };
        let mut act: Option<PackAct> = None;
        let mut open = None;
        let code_bg = ui.visuals().extreme_bg_color;
        let accent = self.hl.colors.accent;
        for (k, pack) in self.packs.list.iter().enumerate() {
            let st = status.get(k).cloned().unwrap_or_default();
            let missing = packs::to_install_kinds(pack, &st, &self.settings.packs_kinds);
            let busy = self.packs.busy.as_deref() == Some(pack.name.as_str());
            let own = lang_settings_count(self.settings.languages.get(&pack.name));
            let r = egui::Frame::NONE.fill(code_bg).corner_radius(6.0).inner_margin(egui::Margin::symmetric(12, 7)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&pack.name).strong().size(14.5));
                    if pack.own {
                        ui.label(RichText::new(t!("perso", "own")).small().color(accent));
                    }
                    ui.label(RichText::new(pack.ext.iter().map(|e| if e.chars().next().is_some_and(char::is_uppercase) { e.clone() } else { format!(".{e}") }).collect::<Vec<_>>().join(" ")).small().weak());
                    if own > 0 {
                        ui.label(RichText::new(tf!("{own} réglage(s) à lui", "{own} setting(s) of its own")).small().color(accent));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("›").size(16.0).weak());
                        if busy {
                            ui.spinner();
                        } else if !missing.is_empty() {
                            let names: Vec<&str> = missing.iter().map(|t| t.bin.as_str()).collect();
                            if ui.button(tf!("Installer ({})", "Install ({})", missing.len())).on_hover_text(names.join(", ")).clicked() {
                                act = Some(PackAct::Install(pack.name.clone(), missing.clone()));
                            }
                        } else if st.iter().zip(&pack.tools).all(|(ok, t)| *ok || t.optional) {
                            ui.label(RichText::new(t!("✓ Prêt", "✓ Ready")).color(ok()));
                        }
                    });
                });
            });
            let r = ui.interact(r.response.rect, ui.scope_id().with(("pack_row", k)), Sense::click());
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if r.on_hover_text(t!("Ses outils et ses réglages", "Its tools and its settings")).clicked() && act.is_none() {
                open = Some(pack.name.clone());
            }
            ui.add_space(4.0);
        }
        if let Some(a) = act {
            self.pack_act(a);
        }
        open
    }

    /// The tools of language `name` (on its page): ✓ there, ✖ missing (a click installs one),
    /// the syntax tree, the commands of what only the system installs.
    pub(super) fn pack_tools(&mut self, ui: &mut egui::Ui, name: &str) {
        if self.packs.status.is_none() {
            self.packs_check();
        }
        let Some(status) = self.packs.status.clone() else {
            ui.spinner();
            return;
        };
        let Some(k) = self.packs.list.iter().position(|p| p.name == name) else { return };
        let pack = &self.packs.list[k];
        let mut act: Option<PackAct> = None;
        let st = status.get(k).cloned().unwrap_or_default();
        let missing = packs::to_install_kinds(pack, &st, &self.settings.packs_kinds);
        let busy = self.packs.busy.as_deref() == Some(pack.name.as_str());
        let code_bg = ui.visuals().extreme_bg_color;
        egui::Frame::NONE.fill(code_bg).corner_radius(6.0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("Outils", "Tools")).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if busy {
                        ui.spinner();
                    } else if !missing.is_empty() {
                        let names: Vec<&str> = missing.iter().map(|t| t.bin.as_str()).collect();
                        if ui.button(tf!("Installer ({})", "Install ({})", missing.len())).on_hover_text(names.join(", ")).clicked() {
                            act = Some(PackAct::Install(pack.name.clone(), missing.clone()));
                        }
                    } else if st.iter().zip(&pack.tools).all(|(ok, t)| *ok || t.optional) {
                        ui.label(RichText::new(t!("✓ Prêt", "✓ Ready")).color(ok()));
                    }
                });
            });
            // its tools: ✓ there, ✖ missing (a click installs one Florm can install)
            ui.horizontal_wrapped(|ui| {
                // the syntax tree (tree-sitter), compiled in: on or off
                if !pack.tree_sitter.is_empty() {
                    let on = !pack.tree_sitter.iter().any(|g| self.settings.tree_sitter_off.contains(g));
                    let text = format!("{} {}", if on { "✓" } else { "○" }, t!("Syntaxe · tree-sitter", "Syntax · tree-sitter"));
                    let rt = if on { RichText::new(text).small().color(ok()) } else { RichText::new(text).small().weak() };
                    if ui
                        .add(egui::Button::new(rt).small())
                        .on_hover_text(t!(
                            "Coloration précise et sélection par bloc logique (Alt+Maj+→), d'après l'arbre de syntaxe du fichier (intégré à Florm). Clic : activer / désactiver.",
                            "Precise highlighting and selection by logical block (Alt+Shift+→), from the syntax tree of the file (built into Florm). Click: turn on / off."
                        ))
                        .clicked()
                    {
                        act = Some(PackAct::TreeSitter(pack.name.clone(), pack.tree_sitter.clone(), !on));
                    }
                }
                for (t, there) in pack.tools.iter().zip(st.iter().copied().chain(std::iter::repeat(false))) {
                    let text = format!("{} {} · {}", if there { "✓" } else { "✖" }, t.kind_label(), if t.builtin { t!("intégré", "built in") } else { t.bin.as_str() });
                    let rt = if there { RichText::new(text).small().color(ok()) } else { RichText::new(text).small().weak() };
                    let how = if t.builtin {
                        t!("Dans Florm, rien à installer", "Inside Florm, nothing to install").to_string()
                    } else if let Some((m, p)) = t.method() {
                        tf!("{m} : {p}{}", "{m}: {p}{}", if t.optional { t!(" (facultatif)", " (optional)") } else { "" })
                    } else {
                        t.hint().to_string()
                    };
                    if !there && t.method().is_some() && !busy && self.packs.busy.is_none() {
                        if ui.add(egui::Button::new(rt).small()).on_hover_text(tf!("Installer — {how}", "Install — {how}")).clicked() {
                            act = Some(PackAct::Install(pack.name.clone(), vec![t.clone()]));
                        }
                    } else {
                        ui.label(rt).on_hover_text(how);
                    }
                }
            });
            // what only the system installs: its commands, to copy or type in a terminal
            for (t, _) in pack.tools.iter().zip(st.iter()).filter(|(t, ok)| !**ok && t.method().is_none() && !t.hint().is_empty()) {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{} :", t.bin)).small().weak());
                    for cmd in t.hint().split(" · ").map(str::trim).filter(|c| !c.is_empty()) {
                        let is_url = cmd.contains("://") || cmd.starts_with("github.com");
                        let r = ui.add(egui::Label::new(RichText::new(cmd).small().monospace()).sense(Sense::click()));
                        if is_url {
                            if r.on_hover_text(t!("Clic : copier le lien", "Click: copy the link")).clicked() {
                                act = Some(PackAct::Copy(cmd.to_string()));
                            }
                        } else {
                            let r = r.on_hover_text(t!("Clic : taper dans un terminal (Entrée pour lancer) ; clic droit : copier", "Click: type in a terminal (Enter to run); right click: copy"));
                            if r.clicked() {
                                act = Some(PackAct::Type(cmd.to_string()));
                            } else if r.secondary_clicked() {
                                act = Some(PackAct::Copy(cmd.to_string()));
                            }
                        }
                    }
                });
            }
        });
        if let Some(a) = act {
            self.pack_act(a);
        }
    }
}

/// How many settings language `o` has of its own.
pub(super) fn lang_settings_count(o: Option<&config::LangOverride>) -> usize {
    let Some(o) = o else { return 0 };
    let v = toml::Value::try_from(o).ok();
    v.as_ref().and_then(|v| v.as_table()).map_or(0, |t| t.len())
}

/// The file « + Nouveau pack » writes.
const PACK_EXAMPLE: &str = r#"# A language pack: its files, its tools, how to install them.
# Saved, then ⟳ in Preferences → Languages. Same name as a pack of Florm: replaces it.
name = "Nim"
ext = ["nim", "nims"]          # extensions, or whole file names (Dockerfile)

# A language server Florm does not know: cmd = its command.
[[tool]]
kind = "lsp"                   # lsp, lint, format, debug
bin = "nimlangserver"          # the program looked for
cmd = "nimlangserver"          # (lsp, lint) the command Florm runs
hint = "nimble install nimlangserver"   # the command to copy, when Florm cannot install it

# A linter: its output "file:line:col: message", {file} replaced by the path of the file.
# Another format: pattern = 'regex with (?P<line>…) (?P<col>…) (?P<msg>…)'.
[[tool]]
kind = "lint"
bin = "nim"
cmd = "nim check --hints:off {file}"
hint = "pacman -S nim · brew install nim"

# What Florm can install itself, in its tools folder: one of
#   pip = "package"      npm = "package"      go = "module@latest"      rustup = "component"
# optional = true: not installed by « Installer » (another tool does the same).
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// The page lists the languages with their tools and an « Installer » for what is missing;
    /// the bar above a Python file proposes it, « Plus tard » hides it.
    #[test]
    fn the_page_and_the_bar_show_what_is_missing() {
        let dir = std::env::temp_dir().join(format!("florm_packs_ui_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\npacks_auto = false\n").unwrap();
        std::fs::write(dir.join("a.py"), "x = 1\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(dir.clone()));
        assert!(app.open_path(dir.join("a.py")));
        // as if nothing were installed (XML: built in, there)
        let list = packs::shipped();
        let status: Vec<Vec<bool>> = list.iter().map(|p| p.tools.iter().map(|t| t.builtin).collect()).collect();
        app.on_pack_status(list, status);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 4000.0));
        // "": the page of all the languages; "bar": the bar of the file; else a language's tools
        let texts = |app: &mut App, page: &str, events: Vec<egui::Event>| {
            let out = crate::run_ui(&ctx, egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| match page {
                    "" => {
                        app.packs_header(ui);
                        app.packs_page(ui);
                    }
                    "bar" => {
                        let i = app.active;
                        app.pack_bar(ui, i, 0);
                    }
                    name => app.pack_tools(ui, name),
                });
            });
            let mut v = Vec::new();
            fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
                match s {
                    egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()))),
                    egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                    _ => {}
                }
            }
            out.shapes.iter().for_each(|c| walk(&c.shape, &mut v));
            v
        };
        texts(&mut app, "", Vec::new());
        let page = texts(&mut app, "", Vec::new());
        let names: Vec<&str> = page.iter().map(|(t, _)| t.as_str()).collect();
        for want in ["Python", "Go", "XML", "Installer (2)", "+ Nouveau pack…"] {
            assert!(names.contains(&want), "{want}: {names:?}");
        }
        // the page of a language: its tools, what Florm installs and what is to type
        let mut tools: Vec<String> = Vec::new();
        for name in ["Python", "XML", "Rust"] {
            texts(&mut app, name, Vec::new());
            tools.extend(texts(&mut app, name, Vec::new()).into_iter().map(|(t, _)| t));
        }
        for want in ["✖ LSP · pylsp", "✓ Lint · intégré", "gdb :", "pacman -S gdb", "✓ Syntaxe · tree-sitter"] {
            assert!(tools.iter().any(|t| t == want), "{want}: {tools:?}");
        }
        // a click on a language's row: its page
        let at = page.iter().find(|(t, _)| t == "Go").unwrap().1.center();
        let click = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let mut opened = None;
        for events in [vec![], vec![egui::Event::PointerMoved(at)], vec![click(true)], vec![click(false)]] {
            let _ = crate::run_ui(&ctx, egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    app.packs_header(ui);
                    opened = opened.take().or(app.packs_page(ui));
                });
            });
        }
        assert_eq!(opened.as_deref(), Some("Go"));
        // the bar of the Python file
        texts(&mut app, "bar", Vec::new());
        let bar = texts(&mut app, "bar", Vec::new());
        let joined: Vec<&str> = bar.iter().map(|(t, _)| t.as_str()).collect();
        assert!(joined.contains(&"⬇ Python") && joined.contains(&"Installer"), "{joined:?}");
        // the language server and the linter; the debugger only at the first F5
        assert!(joined.iter().any(|t| t.contains("pylsp (LSP)") && t.contains("ruff (Lint)") && !t.contains("debugpy")), "{joined:?}");
        let at = bar.iter().find(|(t, _)| t == "Plus tard").unwrap().1.center();
        let click = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        texts(&mut app, "bar", vec![egui::Event::PointerMoved(at), click(true)]);
        texts(&mut app, "bar", vec![click(false)]);
        assert!(app.packs.later.contains("Python"));
        assert!(texts(&mut app, "bar", Vec::new()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Installed by itself when a file opens: no bar while it installs (or waits for another
    /// install); the bar, with a warning, only when it failed.
    #[test]
    fn the_tools_install_by_themselves() {
        let dir = std::env::temp_dir().join(format!("florm_packs_auto_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\npacks_auto = true\n").unwrap();
        std::fs::write(dir.join("a.py"), "x = 1\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(dir.clone()));
        assert!(app.settings.packs_auto);
        assert!(app.open_path(dir.join("a.py")));
        let list = packs::shipped();
        let status: Vec<Vec<bool>> = list.iter().map(|p| p.tools.iter().map(|t| t.builtin).collect()).collect();
        app.on_pack_status(list, status);
        let texts = |app: &mut App| {
            let out = crate::run_ui(&ctx, Default::default(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let i = app.active;
                    app.pack_bar(ui, i, 0);
                });
            });
            let mut v = Vec::new();
            fn walk(s: &egui::Shape, out: &mut Vec<String>) {
                match s {
                    egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                    egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                    _ => {}
                }
            }
            out.shapes.iter().for_each(|c| walk(&c.shape, &mut v));
            v
        };
        // another pack installing: Python waits its turn, without a bar
        app.packs.busy = Some("Go".into());
        assert!(texts(&mut app).is_empty() && app.packs.auto_tried.is_empty());
        // tried and failed: the bar, with why
        app.packs.busy = None;
        app.packs.auto_tried.insert("Python".into());
        let t = texts(&mut app);
        assert!(t.iter().any(|x| x.contains("automatique")) && t.iter().any(|x| x == "Installer"), "{t:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F5 without the debugger of the language: the tools to install first (by the open file, or
    /// by the command typed); nothing when they are there.
    #[test]
    fn f5_finds_the_debugger_to_install() {
        let dir = std::env::temp_dir().join(format!("florm_packs_f5_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        std::fs::write(dir.join("a.py"), "x = 1\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "x\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(dir.clone()));
        let list = packs::shipped();
        let none: Vec<Vec<bool>> = list.iter().map(|p| p.tools.iter().map(|t| t.builtin).collect()).collect();
        app.on_pack_status(list.clone(), none);
        assert!(app.open_path(dir.join("a.py")));
        app.debug.choice = String::new();
        let (pack, tools) = app.debug_missing().expect("debugpy missing");
        assert_eq!((pack.as_str(), tools.iter().map(|t| t.bin.as_str()).collect::<Vec<_>>()), ("Python", vec!["debugpy"]));
        // a command typed: its language, whatever file is open
        assert!(app.open_path(dir.join("notes.txt")));
        assert!(app.debug_missing().is_none());
        app.debug.choice = "cmd".into();
        app.debug.cmd = "python3 a.py".into();
        assert_eq!(app.debug_missing().map(|(p, _)| p).as_deref(), Some("Python"));
        // installed: nothing to do
        let all: Vec<Vec<bool>> = list.iter().map(|p| vec![true; p.tools.len()]).collect();
        app.on_pack_status(list, all);
        assert!(app.debug_missing().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
