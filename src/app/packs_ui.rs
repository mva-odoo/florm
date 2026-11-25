//! Language packs in the app: the page « Langages » of the preferences (each language, its tools
//! there or not, « Installer »), the bar proposing them when a file of a language opens, the
//! installs on a thread.

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
}

enum PackAct {
    Install(String, Vec<Tool>),
    Type(String),
    Copy(String),
    Folder,
    New,
    Check,
    /// The tree-sitter grammar of a syntax turned on or off.
    TreeSitter(String, bool),
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
        self.packs.busy = None;
        self.on_tool_installed(r);
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
            PackAct::TreeSitter(syntax, on) => {
                self.settings.tree_sitter_off.retain(|s| *s != syntax);
                if !on {
                    self.settings.tree_sitter_off.push(syntax.clone());
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
        let missing = packs::to_install(pack, status.get(k).map(Vec::as_slice).unwrap_or(&[]));
        if missing.is_empty() {
            return;
        }
        let name = pack.name.clone();
        let busy = self.packs.busy.as_deref() == Some(name.as_str());
        let what: Vec<String> = missing.iter().map(|t| format!("{} ({})", t.bin, t.kind_label())).collect();
        let (mut install, mut later, mut never) = (false, false, false);
        let bg = icons::mix(ui.visuals().panel_fill, self.hl.colors.accent, 0.10);
        egui::TopBottomPanel::top(egui::Id::new("pack_bar").with(pane))
            .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(10.0, 4.0)))
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tf!("⬇ {name}", "⬇ {name}")).strong());
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

    /// The page « Langages » of the preferences.
    pub(super) fn packs_page(&mut self, ui: &mut egui::Ui) {
        if self.packs.status.is_none() {
            self.packs_check();
        }
        let mut act: Option<PackAct> = None;
        ui.label(
            RichText::new(t!(
                "Les outils de chaque langage : serveur de langage (complétion, F12, renommer), linter, formateur, débogueur. « Installer » les met dans le dossier des outils de Florm (pip dans un environnement virtuel, npm, go install, rustup), sans toucher au système ; les autres se copient ou se tapent dans un terminal.",
                "The tools of each language: language server (completion, F12, rename), linter, formatter, debugger. « Install » puts them in Florm's tools folder (pip in a virtual environment, npm, go install, rustup), without touching the system; the others are copied or typed in a terminal."
            ))
            .weak(),
        );
        ui.add_space(6.0);
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
        ui.add_space(8.0);
        let Some(status) = self.packs.status.clone() else {
            ui.spinner();
            return;
        };
        let code_bg = ui.visuals().extreme_bg_color;
        let accent = self.hl.colors.accent;
        for (k, pack) in self.packs.list.iter().enumerate() {
            let st = status.get(k).cloned().unwrap_or_default();
            let missing = packs::to_install(pack, &st);
            let busy = self.packs.busy.as_deref() == Some(pack.name.as_str());
            egui::Frame::none().fill(code_bg).rounding(6.0).inner_margin(egui::Margin::symmetric(12.0, 8.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&pack.name).strong().size(15.0));
                    if pack.own {
                        ui.label(RichText::new(t!("perso", "own")).small().color(accent));
                    }
                    ui.label(RichText::new(pack.ext.iter().map(|e| if e.chars().next().is_some_and(char::is_uppercase) { e.clone() } else { format!(".{e}") }).collect::<Vec<_>>().join(" ")).small().weak());
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
                        let on = !self.settings.tree_sitter_off.iter().any(|s| *s == pack.tree_sitter);
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
                            act = Some(PackAct::TreeSitter(pack.tree_sitter.clone(), !on));
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
            ui.add_space(6.0);
        }
        if let Some(a) = act {
            self.pack_act(a);
        }
    }
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
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
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
        let texts = |app: &mut App, page: bool, events: Vec<egui::Event>| {
            let out = ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if page {
                        app.packs_page(ui);
                    } else {
                        let i = app.active;
                        app.pack_bar(ui, i, 0);
                    }
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
        texts(&mut app, true, Vec::new());
        let page = texts(&mut app, true, Vec::new());
        let names: Vec<&str> = page.iter().map(|(t, _)| t.as_str()).collect();
        for want in ["Python", "Go", "XML", "Installer (3)", "✖ LSP · pylsp", "✓ Lint · intégré", "gdb :", "pacman -S gdb", "+ Nouveau pack…", "✓ Syntaxe · tree-sitter"] {
            assert!(names.contains(&want), "{want}: {names:?}");
        }
        // the bar of the Python file
        texts(&mut app, false, Vec::new());
        let bar = texts(&mut app, false, Vec::new());
        let joined: Vec<&str> = bar.iter().map(|(t, _)| t.as_str()).collect();
        assert!(joined.contains(&"⬇ Python") && joined.contains(&"Installer"), "{joined:?}");
        assert!(joined.iter().any(|t| t.contains("pylsp (LSP)") && t.contains("debugpy (Débogueur)")), "{joined:?}");
        let at = bar.iter().find(|(t, _)| t == "Plus tard").unwrap().1.center();
        let click = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        texts(&mut app, false, vec![egui::Event::PointerMoved(at), click(true)]);
        texts(&mut app, false, vec![click(false)]);
        assert!(app.packs.later.contains("Python"));
        assert!(texts(&mut app, false, Vec::new()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
