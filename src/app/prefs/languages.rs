//! Préférences → Langages, as in Zed: the page of all the languages (installing their tools, the
//! language servers and the linters in general), then one page per language (its tools, and its
//! settings over the general ones: indentation, wrapping, display, saving, lint).

use super::*;
use crate::config::LangOverride;

/// The program is in the PATH: looked for again every few seconds, not at every frame (several
/// file lookups per folder of the PATH, slow on a network disk).
fn on_path(name: &str) -> bool {
    thread_local! {
        static SEEN: std::cell::RefCell<HashMap<String, (Instant, bool)>> = Default::default();
    }
    SEEN.with(|m| {
        let mut m = m.borrow_mut();
        match m.get(name) {
            Some((t, v)) if t.elapsed() < Duration::from_secs(5) => *v,
            _ => {
                let v = crate::lsp::in_path(name).is_some();
                m.insert(name.to_string(), (Instant::now(), v));
                v
            }
        }
    })
}

/// « oui » / « non ».
fn yes_no(v: bool) -> &'static str {
    if v {
        t!("oui", "yes")
    } else {
        t!("non", "no")
    }
}

/// A switch of a language: its own value, else the general one (said in the text); ↺ goes back
/// to the general one.
fn over_bool(ui: &mut egui::Ui, rc: &mut RowCtx, accent: Color32, title: &str, desc: &str, general: bool, o: &mut Option<bool>) {
    let desc = format!("{desc} {}", tf!("Tous les langages : {}.", "All languages: {}.", yes_no(general)));
    pref_row(ui, rc, title, &desc, |ui| {
        let mut v = o.unwrap_or(general);
        if toggle_switch(ui, &mut v).changed() {
            *o = Some(v);
        }
        own_mark(ui, accent, o.is_some(), || *o = None);
    });
}

/// « à lui » and ↺ when the language has its own value.
fn own_mark(ui: &mut egui::Ui, accent: Color32, own: bool, reset: impl FnOnce()) {
    if !own {
        return;
    }
    if ui.small_button("↺").on_hover_text(t!("Comme tous les langages", "Like all the languages")).clicked() {
        reset();
    }
    ui.label(RichText::new(t!("à lui", "its own")).small().color(accent));
}

impl App {
    /// The page of all the languages.
    pub(super) fn languages_overview(&mut self, ui: &mut egui::Ui, rc: &mut RowCtx, actions: &mut Vec<Cmd>) {
        let accent = self.hl.colors.accent;
        if rc.filter.is_empty() {
            ui.label(
                RichText::new(t!(
                    "Chaque langage a sa page (dans la liste ou à gauche) : ses outils (serveur de langage, linter, formateur, débogueur) et ses réglages, qui remplacent pour ses fichiers ceux de Éditeur et Apparence (indentation, retour à la ligne, formatage à l'enregistrement…), comme dans Zed.",
                    "Each language has its page (in the list or on the left): its tools (language server, linter, formatter, debugger) and its settings, which replace for its files those of Editor and Appearance (indentation, wrapping, format on save…), as in Zed."
                ))
                .weak(),
            );
            // the language of the open file, one click away
            let cur = self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(crate::packs::name_for);
            if let Some(name) = cur {
                ui.add_space(4.0);
                if ui.link(tf!("Langage du fichier ouvert : {name} ›", "Language of the open file: {name} ›")).clicked() {
                    self.prefs.lang = Some(name);
                }
            }
        }
        let install_words = [t!("Installer", "Install"), "pack", t!("outils", "tools"), t!("automatique", "automatic"), t!("Langages", "Languages")];
        if rc.matches(&install_words) {
            pref_group(ui, rc, t!("Installation des outils", "Installing the tools"), accent);
            rc.header(ui);
            ui.add_space(8.0);
            self.packs_header(ui);
        }
        if rc.matches(&[t!("Langages", "Languages"), "Python", "JavaScript", "Rust", "Go", "tree-sitter", "LSP", "linter"]) {
            pref_group(ui, rc, t!("Langages", "Languages"), accent);
            rc.header(ui);
            ui.add_space(8.0);
            if let Some(name) = self.packs_page(ui) {
                self.prefs.lang = Some(name);
                self.prefs.filter.clear();
            }
        }
        let s = &mut self.settings;
        pref_group(ui, rc, t!("Serveurs de langage et lint", "Language servers and lint"), accent);
        pref_row(ui, rc, t!("Serveurs de langage (LSP)", "Language servers (LSP)"), t!("Complétion (Ctrl+Espace), aller à la définition (F12), renommer (F2), leurs erreurs soulignées, et le formatage des fichiers autres que Python. Un langage peut s'en passer (sa page). Un autre langage : [lsp_servers] dans settings.toml.", "Completion (Ctrl+Space), go to definition (F12), rename (F2), their errors underlined, and the formatting of files other than Python. A language can do without (its page). Another language: [lsp_servers] in settings.toml."), |ui| {
            toggle_switch(ui, &mut s.lsp);
        });
        pref_row(ui, rc, t!("Vérifier pendant la frappe", "Check while typing"), t!("Les fichiers Python et ceux des autres langages qui ont un linter (shell, YAML, Markdown…) sont vérifiés une demi-seconde après la dernière touche, sans enregistrer.", "Python files and those of the other languages with a linter (shell, YAML, Markdown…) are checked half a second after the last key, without saving."), |ui| {
            toggle_switch(ui, &mut s.lint_live);
        });
        pref_row(ui, rc, t!("Vérifier à l'enregistrement", "Check on save"), t!("Lance le linter du fichier (Python, JavaScript, les autres langages) à chaque enregistrement.", "Runs the linter of the file (Python, JavaScript, the other languages) on every save."), |ui| {
            toggle_switch(ui, &mut s.lint_on_save);
        });
        // the linters declared in settings.toml (those of the languages are on their page)
        let files = |f: &[&str]| f.iter().map(|x| if x.chars().next().is_some_and(char::is_uppercase) { x.to_string() } else { format!(".{x}") }).collect::<Vec<_>>().join(", ");
        let mut others: Vec<(&String, &lint::LinterCfg)> = s.linters.iter().filter(|(n, c)| !c.cmd.trim().is_empty() && !lint::KNOWN_LINTERS.iter().any(|(k, ..)| k == n)).collect();
        others.sort_by_key(|(n, _)| n.as_str());
        for (name, c) in others {
            let ext: Vec<&str> = c.ext.iter().map(String::as_str).collect();
            let desc = tf!("Déclaré dans settings.toml. Fichiers {} : `{}`.", "Declared in settings.toml. Files {}: `{}`.", files(&ext), c.cmd);
            pref_row(ui, rc, name, &desc, |ui| {
                install_state(ui, c.cmd.split_whitespace().next().is_some_and(on_path));
            });
        }
        pref_row(ui, rc, t!("Un autre linter", "Another linter"), t!("Dans settings.toml, [linters.nom] avec cmd (une commande qui écrit fichier:ligne:col: message ; le texte arrive sur son entrée standard, {file} est son chemin) et ext (les extensions). La même clé remplace un linter de Florm, cmd = \"\" le coupe.", "In settings.toml, [linters.name] with cmd (a command writing file:line:col: message; the text comes on its standard input, {file} is its path) and ext (the extensions). The same key replaces a linter of Florm, cmd = \"\" turns it off."), |ui| {
            if ui.button(t!("Ouvrir settings.toml", "Open settings.toml")).clicked() {
                actions.push(Cmd::OpenSettingsFile);
            }
        });
        self.trusted_folders_prefs(ui, rc);
    }

    /// The page of language `name`: its tools, then its own settings.
    pub(super) fn language_page(&mut self, ui: &mut egui::Ui, rc: &mut RowCtx, name: &str, actions: &mut Vec<Cmd>) {
        let accent = self.hl.colors.accent;
        let exts: Vec<String> = self.packs.list.iter().find(|p| p.name == name).map(|p| p.ext.clone()).unwrap_or_default();
        ui.label(RichText::new(exts.iter().map(|e| if e.chars().next().is_some_and(char::is_uppercase) { e.clone() } else { format!(".{e}") }).collect::<Vec<_>>().join(" ")).weak());
        ui.add_space(8.0);
        self.pack_tools(ui, name);
        let s = &mut self.settings;
        let g = s.lang(None);
        let mut o: LangOverride = s.languages.get(name).cloned().unwrap_or_default();

        pref_group(ui, rc, t!("Serveur de langage et lint", "Language server and lint"), accent);
        let lsp_desc = if s.lsp {
            t!("Complétion, F12, renommer, documentation au survol, formatage, avec le serveur de langage installé (Outils).", "Completion, F12, rename, documentation on hover, formatting, with the installed language server (Tools).")
        } else {
            t!("Coupés pour tous les langages (Préférences → Langages) : rien ne tourne, même activé ici.", "Off for all the languages (Preferences → Languages): nothing runs, even turned on here.")
        };
        over_bool(ui, rc, accent, t!("Serveur de langage", "Language server"), lsp_desc, true, &mut o.lsp);
        over_bool(ui, rc, accent, t!("Lint", "Lint"), t!("Les erreurs de ses linters soulignées, pendant la frappe ou à l'enregistrement.", "The errors of its linters underlined, while typing or on save."), true, &mut o.lint);
        let installed = |n: &str| lint::is_installed(n);
        if name == "Python" {
            pref_row(ui, rc, t!("Linter Python", "Python linter"), t!("« Automatique » prend flake8, sinon ruff, sinon pyflakes ; un linter qui plante est signalé en ligne 1 et le suivant prend le relais. ruff est le plus rapide. La configuration du projet (setup.cfg, .flake8, tox.ini, pyproject.toml) est lue.", "“Automatic” takes flake8, else ruff, else pyflakes; a linter that crashes is reported on line 1 and the next one takes over. ruff is the fastest. The project configuration (setup.cfg, .flake8, tox.ini, pyproject.toml) is read."), |ui| {
                let label = |n: &str| match n {
                    "auto" => t!("Automatique", "Automatic").to_string(),
                    n => format!("{n}{}", if installed(n) { t!(" — installé", " — installed") } else { t!(" — non installé", " — not installed") }),
                };
                egui::ComboBox::from_id_salt("pref_linter").selected_text(label(&s.python_linter)).width(220.0).show_ui(ui, |ui| {
                    for n in ["auto", "flake8", "ruff", "pyflakes", "pylint"] {
                        ui.selectable_value(&mut s.python_linter, n.to_string(), label(n));
                    }
                });
            });
            let args = &mut self.prefs.lint_args;
            pref_row(ui, rc, t!("Arguments du linter", "Linter arguments"), t!("Ajoutés à la ligne de commande, séparés par des espaces. Ex. : --max-line-length=120 (en automatique, ruff ne reçoit que la longueur de ligne).", "Added to the command line, separated by spaces. E.g.: --max-line-length=120 (in automatic mode, ruff only gets the line length)."), |ui| {
                let r = crate::input::add(ui, egui::TextEdit::singleline(args).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text("--max-line-length=120"));
                if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    s.python_lint_args = args.split_whitespace().map(String::from).collect();
                }
            });
            pref_row(ui, rc, "flake8", &tf!("pycodestyle + pyflakes, codes E / W / F. Installé par Florm dans {} (sans toucher au Python du système).", "pycodestyle + pyflakes, codes E / W / F. Installed by Florm in {} (without touching the system's Python).", lint::tools_dir().display()), |ui| {
                if ui.button(if installed("flake8") { t!("Mettre à jour", "Update") } else { t!("Installer flake8", "Install flake8") }).clicked() {
                    actions.push(Cmd::InstallFlake8);
                }
                install_state(ui, installed("flake8"));
            });
        }
        // the codes each linter of the language does not report
        let mine: &[&str] = match name {
            "Python" => &["flake8", "ruff", "pylint"],
            "JavaScript / TypeScript" => &["eslint"],
            _ => &[],
        };
        for (k, (lname, example)) in lint::WITH_CODES.iter().enumerate().filter(|(_, (n, _))| mine.contains(n)) {
            if *lname == "eslint" {
                pref_row(ui, rc, "ESLint", t!("À l'enregistrement. Celui du projet (node_modules/.bin) d'abord, sinon celui du PATH ; sa configuration est lue.", "On save. The project's one (node_modules/.bin) first, else the one in the PATH; its configuration is read."), |ui| {
                    install_state(ui, on_path("eslint"));
                });
            }
            let buf = &mut self.prefs.lint_ignore[k];
            let desc = tf!("Problèmes de {lname} à ne pas afficher, séparés par des virgules : un code, un préfixe (E5 : tous les E5xx){}.", "Problems of {lname} not to show, separated by commas: a code, a prefix (E5: all the E5xx){}.", if matches!(*lname, "pylint" | "eslint") { t!(" ou le nom d'une règle", " or the name of a rule") } else { "" });
            pref_row(ui, rc, &tf!("Codes ignorés par {lname}", "Codes ignored by {lname}"), &desc, |ui| {
                let r = crate::input::add(ui, egui::TextEdit::singleline(buf).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text(*example));
                if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    *s.lint_ignore_mut(lname) = codes_of(buf);
                }
            });
        }
        // the other linters Florm knows for its files
        let has = |files: &[&str]| files.iter().any(|f| exts.iter().any(|e| e.eq_ignore_ascii_case(f)));
        for (lname, files, cmd) in lint::KNOWN_LINTERS.iter().filter(|(_, f, _)| has(f)) {
            let set = s.linters.get(*lname);
            let desc = tf!("`{}`, pendant la frappe, dans un dossier de confiance. [linters.{lname}] dans settings.toml le remplace (cmd = \"\" le coupe).", "`{}`, while typing, in a trusted folder. [linters.{lname}] in settings.toml replaces it (cmd = \"\" turns it off).", set.filter(|c| !c.cmd.trim().is_empty()).map_or(*cmd, |c| c.cmd.as_str()));
            let _ = files;
            pref_row(ui, rc, lname, &desc, |ui| match set {
                Some(c) if c.cmd.trim().is_empty() => {
                    ui.label(RichText::new(t!("Coupé (settings.toml)", "Off (settings.toml)")).weak());
                }
                _ => {
                    let prog = set.map_or(*cmd, |c| c.cmd.as_str()).split_whitespace().next().unwrap_or("");
                    install_state(ui, on_path(prog));
                }
            });
        }

        pref_group(ui, rc, t!("Indentation", "Indentation"), accent);
        pref_row(ui, rc, t!("Largeur des tabulations", "Tab width"), &tf!("Colonnes d'une tabulation et d'un niveau d'indentation. Tous les langages : {}.", "Columns of a tab and of an indentation level. All languages: {}.", g.tab_size), |ui| {
            let mut v = o.tab_size.unwrap_or(g.tab_size);
            if ui.add(egui::Slider::new(&mut v, 1..=8)).changed() {
                o.tab_size = Some(v);
            }
            own_mark(ui, accent, o.tab_size.is_some(), || o.tab_size = None);
        });
        over_bool(ui, rc, accent, t!("Indenter avec des espaces", "Indent with spaces"), t!("Tab insère des espaces plutôt qu'une tabulation (les fichiers existants gardent leur style).", "Tab inserts spaces rather than a tab (existing files keep their style)."), g.translate_tabs_to_spaces, &mut o.translate_tabs_to_spaces);

        pref_group(ui, rc, t!("Retour à la ligne et règles", "Wrapping and rulers"), accent);
        over_bool(ui, rc, accent, t!("Retour à la ligne automatique", "Word wrap"), t!("Les lignes plus longues que l'éditeur continuent sur les rangées suivantes (Alt+Z).", "Lines longer than the editor go on over the next rows (Alt+Z)."), g.word_wrap, &mut o.word_wrap);
        let shown = |v: &[usize]| v.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ");
        if self.prefs.lang_rulers.as_ref().is_none_or(|(n, _)| n != name) {
            self.prefs.lang_rulers = Some((name.to_string(), shown(o.rulers.as_deref().unwrap_or(&g.rulers))));
        }
        let buf = &mut self.prefs.lang_rulers.as_mut().unwrap().1;
        let general_rulers = if g.rulers.is_empty() { t!("aucune", "none").to_string() } else { shown(&g.rulers) };
        pref_row(ui, rc, t!("Règles verticales", "Vertical rulers"), &tf!("Colonnes séparées par des virgules (ex. 79, 120). Tous les langages : {general_rulers}.", "Columns separated by commas (e.g. 79, 120). All languages: {general_rulers}."), |ui| {
            let r = crate::input::add(ui, egui::TextEdit::singleline(buf).desired_width(120.0).font(egui::TextStyle::Monospace).hint_text("79, 120"));
            if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                o.rulers = Some(buf.split(',').filter_map(|c| c.trim().parse().ok()).filter(|c| *c > 0).collect());
            }
            let mut reset = false;
            own_mark(ui, accent, o.rulers.is_some(), || reset = true);
            if reset {
                o.rulers = None;
                *buf = shown(&g.rulers);
            }
        });

        pref_group(ui, rc, t!("Affichage", "Display"), accent);
        over_bool(ui, rc, accent, t!("Espaces visibles", "Visible whitespace"), t!("Un point pour chaque espace, une flèche pour chaque tabulation.", "A dot for each space, an arrow for each tab."), g.show_whitespace, &mut o.show_whitespace);
        over_bool(ui, rc, accent, t!("Guides d'indentation", "Indent guides"), t!("Des traits verticaux à chaque niveau d'indentation, plus marqué pour le bloc du curseur.", "Vertical lines at each indentation level, stronger for the block of the cursor."), g.indent_guides, &mut o.indent_guides);
        over_bool(ui, rc, accent, t!("Crochets colorés", "Colored brackets"), t!("Une couleur par niveau d'imbrication des parenthèses, crochets et accolades.", "A color per level of nesting of the parentheses, brackets and braces."), g.rainbow_brackets, &mut o.rainbow_brackets);
        over_bool(ui, rc, accent, t!("Orthographe", "Spelling"), t!("Les mots inconnus de ses commentaires (et du texte, en Markdown) soulignés.", "The unknown words of its comments (and of the text, in Markdown) underlined."), g.spell_check, &mut o.spell_check);

        pref_group(ui, rc, t!("Saisie", "Typing"), accent);
        over_bool(ui, rc, accent, t!("Fermeture automatique", "Auto-close brackets"), t!("Ferme ( [ { \" ' ` en les tapant et entoure la sélection.", "Closes ( [ { \" ' ` as they are typed and surrounds the selection."), g.auto_pair, &mut o.auto_pair);

        pref_group(ui, rc, t!("Enregistrement", "Saving"), accent);
        let format_desc = if name == "Python" {
            t!("Avec ruff (comme Ctrl+Alt+F), avant d'enregistrer.", "With ruff (like Ctrl+Alt+F), before saving.")
        } else {
            t!("Par son serveur de langage, juste après l'enregistrement (puis enregistré à nouveau).", "By its language server, right after saving (then saved again).")
        };
        over_bool(ui, rc, accent, t!("Formater à l'enregistrement", "Format on save"), format_desc, g.format_on_save, &mut o.format_on_save);
        over_bool(ui, rc, accent, t!("Supprimer les espaces en fin de ligne", "Trim trailing whitespace"), t!("À chaque enregistrement.", "On every save."), g.trim_trailing_whitespace_on_save, &mut o.trim_trailing_whitespace_on_save);
        over_bool(ui, rc, accent, t!("Retour à la ligne final", "Final newline"), t!("Ajouté à l'enregistrement quand le fichier n'en a pas.", "Added on saving when the file has none."), g.ensure_final_newline_on_save, &mut o.ensure_final_newline_on_save);

        if o.is_empty() {
            s.languages.remove(name);
        } else {
            s.languages.insert(name.to_string(), o);
        }
    }

    /// Alt+Z and the visible whitespace toggled for the active file: its language's own value when
    /// it has one, else the general one.
    pub(in crate::app) fn toggle_lang_bool(&mut self, key: &str) -> bool {
        let name = self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(crate::packs::name_for);
        let before = self.settings.clone();
        let own = name.as_ref().and_then(|n| self.settings.languages.get_mut(n)).and_then(|o| match key {
            "word_wrap" => o.word_wrap.as_mut(),
            _ => o.show_whitespace.as_mut(),
        });
        let now = match own {
            Some(v) => {
                *v = !*v;
                *v
            }
            None => {
                let v = if key == "word_wrap" { &mut self.settings.word_wrap } else { &mut self.settings.show_whitespace };
                *v = !*v;
                *v
            }
        };
        self.apply_settings_change(&before);
        now
    }
}
