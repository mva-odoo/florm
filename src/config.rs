use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    /// Language of the interface: "fr", "en"; empty: the system's.
    pub language: String,
    pub font_size: f32,
    pub tab_size: usize,
    pub translate_tabs_to_spaces: bool,
    pub theme: String,
    /// Interface accent "#RRGGBB" instead of the theme's; empty: the theme's.
    pub accent_color: String,
    /// 0.0 – 0.4: text colors mixed with the background (lighter, thinner-looking text).
    pub text_softness: f32,
    /// Editor font (a monospaced font installed on the system); empty: the default one.
    pub editor_font: String,
    /// Interface font; empty: the default one.
    pub ui_font: String,
    pub show_minimap: bool,
    /// Lines longer than the editor shown on several rows (Alt+Z).
    pub word_wrap: bool,
    /// The class / function lines around the top of the editor stay pinned there while scrolling.
    pub sticky_scroll: bool,
    /// Columns of vertical rulers, e.g. [79, 120].
    pub rulers: Vec<usize>,
    /// Path and enclosing class / function above the editor.
    pub breadcrumbs: bool,
    /// Menu bar (Fichier, Édition…) at the top; hidden, the menus are under ☰ in the status bar.
    pub show_menu_bar: bool,
    /// Florm's own title bar (menus, title, ─ □ ✕ in the window, as VS Code) instead of the
    /// system's, on every system.
    pub custom_title_bar: bool,
    /// "Find in files" in an editor tab instead of the bottom panel.
    pub search_in_tab: bool,
    /// Claude in an editor tab instead of the bottom panel.
    pub claude_in_tab: bool,
    /// The review of Claude's changes on the right of the conversation.
    pub claude_review: bool,
    /// What Claude may do: "edit", "bash", "web", "agents", and "sandbox" (commands isolated),
    /// "confine" (files of the project only).
    pub claude_perms: Vec<String>,
    /// Most dollars a conversation may spend (0: no limit).
    pub claude_budget: f64,
    /// Branch and upstream of git repositories on their folder in the tree.
    pub tree_git_info: bool,
    /// Show git changes inside the editor (deleted lines in red, added in green).
    pub inline_diff: bool,
    pub auto_pair: bool,
    pub lint_on_save: bool,
    /// Lint Python while typing (not only on save).
    pub lint_live: bool,
    /// "auto" (flake8 → ruff → pyflakes), "flake8", "ruff", "pyflakes" or "pylint".
    pub python_linter: String,
    /// Extra arguments for the Python linter, e.g. ["--max-line-length=120"].
    pub python_lint_args: Vec<String>,
    /// Codes not shown, per linter (a code, a prefix like "E5", or a pylint / eslint rule name).
    pub lint_ignore_flake8: Vec<String>,
    pub lint_ignore_ruff: Vec<String>,
    pub lint_ignore_pylint: Vec<String>,
    pub lint_ignore_eslint: Vec<String>,
    pub trim_trailing_whitespace_on_save: bool,
    /// Python files formatted with ruff when saved.
    pub format_on_save: bool,
    /// Keep unsaved changes when closing (restored at the next start), without asking.
    pub hot_exit: bool,
    /// Shell used by the terminal panel (plugins always use `sh` / `cmd`).
    pub shell: Option<String>,
    /// "ctrl+shift+r" = "git_push", chords as "ctrl+k ctrl+b".
    pub keys: HashMap<String, String>,
    /// Language servers: completion, go to definition, rename.
    pub lsp: bool,
    /// Server command per language family ("python", "typescript", "rust", "go", "c"); else detected.
    pub lsp_servers: HashMap<String, String>,
    /// Left out of go to file, the symbol index and find in files (gitignore syntax).
    pub exclude: Vec<String>,
    /// Most source files in the symbol index (the repositories of the open files come first).
    pub index_max_files: usize,
    /// Look for a newer release of Florm at startup (GitHub).
    pub check_updates: bool,
    /// Odoo tests: command run in the container (`{tags}`: the tags), empty: the default one.
    pub odoo_test_command: String,
    /// Last tags run, newest first.
    pub odoo_test_history: Vec<String>,
    /// Container the tests run in.
    pub odoo_test_container: String,
    /// Order of the services of each compose project in the Docker view (▶ tout starts them in
    /// it, ■ tout stops them the other way): project → services.
    pub docker_order: HashMap<String, Vec<String>>,
    /// Icons hidden at the bottom of the sidebar: "settings", "search", "terminal", "claude"…
    pub sidebar_hidden: Vec<String>,
    /// The icons of the views at the top of the sidebar rather than at its bottom.
    pub sidebar_tools_top: bool,
    /// Debugger: the container the program runs in, empty for this computer.
    pub debug_target: String,
    /// "launch" (Florm starts the program) or "attach" (it listens with debugpy).
    pub debug_mode: String,
    /// Program launched here and its arguments; empty: the odoo-bin of the folder.
    pub debug_command: String,
    /// Program launched in the container; empty: `debug::DOCKER_COMMAND`.
    pub debug_docker_command: String,
    /// Python of this computer (with debugpy).
    pub debug_python: String,
    /// Attach: the port debugpy listens on (in the container, for Docker).
    pub debug_port: u16,
    /// Exceptions the debugger stops on: "raised", "uncaught", "userUnhandled".
    pub debug_exceptions: Vec<String>,
    /// Step through the code of the project only, not Odoo's nor the libraries'.
    pub debug_just_my_code: bool,
    /// Shared configuration (settings.toml, templates/, themes/, plugins/) in a git repository:
    /// "login" (login/florm-config on GitHub), "owner/repo" or a git address; empty: none.
    pub shared_config: String,
}

impl Settings {
    /// The codes ignored for `linter` (one of `lint::WITH_CODES`).
    pub fn lint_ignore_mut(&mut self, linter: &str) -> &mut Vec<String> {
        match linter {
            "flake8" => &mut self.lint_ignore_flake8,
            "ruff" => &mut self.lint_ignore_ruff,
            "pylint" => &mut self.lint_ignore_pylint,
            _ => &mut self.lint_ignore_eslint,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: "fr".into(),
            font_size: 14.0,
            tab_size: 4,
            translate_tabs_to_spaces: true,
            theme: "Florm".into(),
            accent_color: String::new(),
            text_softness: 0.0,
            editor_font: String::new(),
            ui_font: String::new(),
            show_minimap: true,
            word_wrap: false,
            sticky_scroll: true,
            rulers: vec![80],
            breadcrumbs: true,
            show_menu_bar: true,
            custom_title_bar: true,
            search_in_tab: false,
            claude_in_tab: false,
            claude_review: true,
            claude_perms: ["edit", "agents", "sandbox"].map(String::from).to_vec(),
            claude_budget: 0.0,
            tree_git_info: true,
            inline_diff: false,
            auto_pair: true,
            lint_on_save: true,
            lint_live: true,
            python_linter: "auto".into(),
            python_lint_args: Vec::new(),
            lint_ignore_flake8: Vec::new(),
            lint_ignore_ruff: Vec::new(),
            lint_ignore_pylint: Vec::new(),
            lint_ignore_eslint: Vec::new(),
            trim_trailing_whitespace_on_save: false,
            format_on_save: false,
            hot_exit: true,
            shell: None,
            keys: HashMap::new(),
            lsp: true,
            lsp_servers: HashMap::new(),
            exclude: ["__pycache__/", "*.pyc", "node_modules/", ".venv/", "*.egg-info/"].map(String::from).to_vec(),
            index_max_files: 150_000,
            check_updates: true,
            odoo_test_command: String::new(),
            odoo_test_history: Vec::new(),
            odoo_test_container: String::new(),
            docker_order: HashMap::new(),
            sidebar_hidden: Vec::new(),
            sidebar_tools_top: false,
            debug_target: String::new(),
            debug_mode: "launch".into(),
            debug_command: String::new(),
            debug_docker_command: String::new(),
            debug_python: if cfg!(windows) { "python".into() } else { "python3".into() },
            debug_port: 5678,
            debug_exceptions: vec!["uncaught".into()],
            debug_just_my_code: false,
            shared_config: String::new(),
        }
    }
}

const DEFAULT_SETTINGS: &str = r#"# Préférences de Florm (enregistrer ce fichier les applique immédiatement)
# langue de l'interface : "fr" (français), "en" (English) ; vide = celle du système
language = "fr"
font_size = 14.0
tab_size = 4
translate_tabs_to_spaces = true
# Thèmes intégrés : Florm, Florm Light, Light+ (VS Code), base16-ocean.dark, base16-eighties.dark, base16-mocha.dark,
# Solarized (dark), Solarized (light), InspiredGitHub.
# Tout fichier .tmTheme déposé dans le dossier "themes" de la config est aussi chargé.
theme = "Florm"
# couleur d'accentuation de l'interface, ex. #4F8CFF ; vide = celle du thème
accent_color = ""
# texte adouci (0.0 à 0.4) : couleurs du texte mélangées au fond, pour un texte moins « gras »
text_softness = 0.0
# polices installées sur le système ; vide = par défaut (Consolas / Segoe UI sous Windows)
editor_font = ""
ui_font = ""
show_minimap = true
# retour à la ligne automatique des lignes trop longues pour l'éditeur (Alt+Z)
word_wrap = false
# classe / fonction courantes figées en haut de l'éditeur pendant le défilement
sticky_scroll = true
# règles verticales aux colonnes données, ex. [79, 120]
rulers = [80]
# fil d'Ariane (dossier › fichier › classe › méthode) au-dessus de l'éditeur
breadcrumbs = true
# barre de menus en haut (masquée : menus sous ☰ en bas à gauche)
show_menu_bar = true
# barre de titre de Florm (menus et boutons ─ □ ✕ dans la fenêtre) plutôt que celle du système
custom_title_bar = true
# recherche dans le dossier (Ctrl+Shift+F) dans un onglet plutôt que dans le panneau du bas
search_in_tab = false
# Claude (Ctrl+Alt+C) dans un onglet plutôt que dans le panneau du bas
claude_in_tab = false
# revue des changements de Claude à droite de la conversation (bouton « Revue »)
claude_review = true
# Ce que Claude a le droit de faire : "edit" (modifier les fichiers), "bash" (lancer des commandes),
# "web" (internet), "agents" (sous-agents), "sandbox" (commandes isolées), "confine" (fichiers du projet seulement)
claude_perms = ["edit", "agents", "sandbox"]
# dépense maximale d'une conversation en dollars (0 = sans limite)
claude_budget = 0.0
# branche et branche distante des dépôts git sur leur dossier dans l'arborescence
tree_git_info = true
# diff git dans l'éditeur : lignes supprimées en rouge, ajoutées en vert (Ctrl+Alt+D)
inline_diff = false
auto_pair = true
lint_on_save = true
# Lint Python pendant la frappe
lint_live = true
# Linter Python : "auto" (flake8 → ruff → pyflakes), "flake8", "ruff", "pyflakes", "pylint"
python_linter = "auto"
# Arguments en plus, ex. ["--max-line-length=120"] (la config du projet, setup.cfg / .flake8 / tox.ini, est lue aussi)
python_lint_args = []
# Codes à ne pas afficher, par linter : un code ("E501"), un préfixe ("E5" : tous les E5xx)
# ou, pour pylint et eslint, le nom d'une règle ("missing-docstring", "no-unused-vars")
lint_ignore_flake8 = []
lint_ignore_ruff = []
lint_ignore_pylint = []
lint_ignore_eslint = []
trim_trailing_whitespace_on_save = false
# formater les fichiers Python avec ruff à l'enregistrement (Ctrl+Alt+F pour le faire à la main)
format_on_save = false
# garder les modifications non enregistrées à la fermeture (rouvertes au démarrage suivant)
hot_exit = true
# shell = "/bin/bash"
# Serveurs de langage (complétion Ctrl+Espace, définition F12, renommer F2)
lsp = true
# Exclus de « aller au fichier » (Ctrl+P), de l'index des symboles et de la recherche (syntaxe .gitignore)
exclude = ["__pycache__/", "*.pyc", "node_modules/", ".venv/", "*.egg-info/"]
# Fichiers source au plus dans l'index des symboles (les dépôts des fichiers ouverts d'abord)
index_max_files = 150000
# chercher une nouvelle version de Florm au démarrage (releases GitHub)
check_updates = true
# tests Odoo dans un conteneur (vue Docker → 🧪 Tests) : commande lancée, {tags} = les tags
# vide = "odoo --stop-after-init --test-enable --test-tags {tags}"
odoo_test_command = ""
# débogueur (F5, vue Débogage) : programme lancé sur cet ordinateur (vide = l'odoo-bin du dossier)
# et dans un conteneur (vide = "odoo --workers 0 --http-port 8070")
debug_command = ""
debug_docker_command = ""
# configuration partagée entre ordinateurs : un dépôt GitHub avec settings.toml, templates/,
# themes/ et plugins/ ; "login" (= login/florm-config), "owner/repo" ou une adresse git
shared_config = ""

[keys]
# Raccourcis personnalisés : "combinaison" = "id_de_commande"
# "ctrl+shift+r" = "git_push"
# "ctrl+k ctrl+t" = "toggle_terminal"

[lsp_servers]
# Commande du serveur par langage (sinon détectée : pyright ou pylsp, typescript-language-server,
# rust-analyzer, gopls, clangd)
# python = "pyright-langserver --stdio"
"#;

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Held by the tests that change `FLORM_CONFIG` (one at a time).
#[cfg(test)]
pub static TEST_CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("FLORM_CONFIG") {
        return PathBuf::from(d);
    }
    #[cfg(windows)]
    if let Some(a) = std::env::var_os("APPDATA") {
        return PathBuf::from(a).join("florm");
    }
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(x).join("florm");
    }
    home().join(".config").join("florm")
}

/// Data folder (tools installed by Florm, X11 libraries on WSL…).
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(a) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(a).join("florm");
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(x).join("florm");
    }
    home().join(".local").join("share").join("florm")
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.toml")
}

pub fn plugins_dir() -> PathBuf {
    config_dir().join("plugins")
}

/// Where new themes are saved: the shared configuration when there is one.
pub fn themes_dir() -> PathBuf {
    crate::shared::sub("themes").unwrap_or_else(|| config_dir().join("themes"))
}

/// Folders the .tmTheme files are loaded from (the shared ones last: they win).
pub fn theme_dirs() -> Vec<PathBuf> {
    std::iter::once(config_dir().join("themes")).chain(crate::shared::sub("themes")).collect()
}

/// settings.toml, local or shared.
pub fn is_settings_file(path: &std::path::Path) -> bool {
    let canon = |p: &std::path::Path| crate::os::canon(p);
    canon(path) == canon(&settings_path()) || crate::shared::settings_path().is_some_and(|s| canon(path) == canon(&s))
}

/// `over` written on `base`; tables ([keys]…) merged key by key.
fn merge(base: &mut toml::Table, over: toml::Table) {
    for (k, v) in over {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

/// Loads settings, creating a commented default file on first run. The shared settings.toml
/// (see `shared`) goes over the local one.
pub fn load() -> (Settings, Option<String>) {
    let p = settings_path();
    if !p.exists() {
        let _ = std::fs::create_dir_all(config_dir());
        let _ = std::fs::write(&p, DEFAULT_SETTINGS);
    }
    let Ok(text) = std::fs::read_to_string(&p) else { return (Settings::default(), None) };
    let mut table = match toml::from_str::<toml::Table>(&text) {
        Ok(t) => t,
        Err(e) => return (Settings::default(), Some(format!("settings.toml : {e}"))),
    };
    crate::shared::set_spec(table.get("shared_config").and_then(|v| v.as_str()).unwrap_or(""));
    let mut err = None;
    if let Some(sp) = crate::shared::settings_path().filter(|sp| sp.is_file()) {
        match std::fs::read_to_string(&sp).map_err(|e| e.to_string()).and_then(|t| toml::from_str::<toml::Table>(&t).map_err(|e| e.to_string())) {
            Ok(mut shared) => {
                for k in crate::shared::LOCAL_KEYS {
                    shared.remove(*k);
                }
                merge(&mut table, shared);
            }
            Err(e) => err = Some(format!("{} : {e}", sp.display())),
        }
    }
    match toml::Value::Table(table).try_into::<Settings>() {
        Ok(v) => {
            crate::i18n::set(crate::i18n::from_code(&v.language));
            (v, err)
        }
        Err(e) => (Settings::default(), Some(format!("settings.toml : {e}"))),
    }
}

/// TOML string literal.
pub fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Sets `key = value` (value already in TOML syntax) in settings.toml, keeping comments
/// and the rest of the file as is.
/// Sets (`Some`) or removes (`None`) the entry `"key" = "value"` of the [keys] section.
pub fn set_key(key: &str, value: Option<&str>) -> Result<(), String> {
    // the shared file has shortcuts: they go there too (it goes over the local ones)
    if let Some(sp) = crate::shared::settings_path() {
        if std::fs::read_to_string(&sp).is_ok_and(|t| t.lines().any(|l| l.trim() == "[keys]")) {
            set_key_in(&sp, key, value)?;
        }
    }
    set_key_in(&settings_path(), key, value)
}

fn set_key_in(p: &std::path::Path, key: &str, value: Option<&str>) -> Result<(), String> {
    let text = std::fs::read_to_string(p).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let header = match lines.iter().position(|l| l.trim() == "[keys]") {
        Some(i) => i,
        None if value.is_none() => return Ok(()),
        None => {
            lines.push(String::new());
            lines.push("[keys]".into());
            lines.len() - 1
        }
    };
    let end = lines.iter().enumerate().skip(header + 1).find(|(_, l)| l.trim_start().starts_with('[')).map_or(lines.len(), |(i, _)| i);
    let quoted = toml_str(key);
    let existing = (header + 1..end).find(|&i| {
        let t = lines[i].trim_start();
        t.strip_prefix(quoted.as_str()).is_some_and(|rest| rest.trim_start().starts_with('='))
    });
    match (existing, value) {
        (Some(i), Some(v)) => lines[i] = format!("{quoted} = {}", toml_str(v)),
        (Some(i), None) => {
            lines.remove(i);
        }
        (None, Some(v)) => {
            // after the last non-empty line of the section
            let mut at = end;
            while at > header + 1 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            lines.insert(at, format!("{quoted} = {}", toml_str(v)));
        }
        (None, None) => {}
    }
    std::fs::write(p, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

/// TOML array of strings.
pub fn toml_str_array(v: &[String]) -> String {
    format!("[{}]", v.iter().map(|s| toml_str(s)).collect::<Vec<_>>().join(", "))
}

/// Sets `key = value` (value already in TOML syntax) in settings.toml, keeping comments and the
/// rest of the file as is; in the shared settings.toml too when it sets this key.
pub fn set_setting(key: &str, value: &str) -> Result<(), String> {
    if let Some(sp) = crate::shared::settings_path().filter(|_| !crate::shared::LOCAL_KEYS.contains(&key)) {
        if let Ok(text) = std::fs::read_to_string(&sp) {
            if top_level_index(&text, key).is_some() {
                let _ = set_setting_in(&sp, key, value);
            }
        }
    }
    let _ = std::fs::create_dir_all(config_dir());
    set_setting_in(&settings_path(), key, value)
}

/// Line of `key = …` before the first [section].
fn top_level_index(text: &str, key: &str) -> Option<usize> {
    text.lines().take_while(|l| !l.trim_start().starts_with('[')).position(|l| {
        let t = l.trim_start();
        t.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

fn set_setting_in(p: &std::path::Path, key: &str, value: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(p).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let section = lines.iter().position(|l| l.trim_start().starts_with('[')).unwrap_or(lines.len());
    let line = format!("{key} = {value}");
    match top_level_index(&text, key) {
        Some(i) => lines[i] = line,
        None => lines.insert(section, line),
    }
    std::fs::write(p, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_order_table() {
        let order = vec!["postgresql".to_string(), "dns".into()];
        let line = format!("docker_order = {{ {} = {} }}", toml_str("dockerized-saas"), toml_str_array(&order));
        let s: Settings = toml::from_str(&line).unwrap();
        assert_eq!(s.docker_order.get("dockerized-saas"), Some(&order));
    }

    #[test]
    fn set_setting_keeps_comments() {
        let dir = std::env::temp_dir().join(format!("florm_cfg_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("FLORM_CONFIG", &dir);
        std::fs::write(settings_path(), "# hello\ntheme = \"base16-ocean.dark\"\n\n[keys]\n\"ctrl+r\" = \"git_push\"\n").unwrap();
        set_setting("theme", &toml_str("Solarized (dark)")).unwrap();
        set_setting("show_minimap", "false").unwrap();
        let text = std::fs::read_to_string(settings_path()).unwrap();
        assert!(text.starts_with("# hello\ntheme = \"Solarized (dark)\"\n"));
        let s: Settings = toml::from_str(&text).unwrap();
        assert_eq!(s.theme, "Solarized (dark)");
        assert!(!s.show_minimap);
        assert_eq!(s.keys.get("ctrl+r").map(String::as_str), Some("git_push"));
        assert_eq!(toml_str("a\"b\\c"), "\"a\\\"b\\\\c\"");
        set_key("ctrl+shift+x", Some("git_push")).unwrap();
        set_key("ctrl+r", None).unwrap();
        set_key("ctrl+d", Some("none")).unwrap();
        let s: Settings = toml::from_str(&std::fs::read_to_string(settings_path()).unwrap()).unwrap();
        assert_eq!(s.keys.get("ctrl+shift+x").map(String::as_str), Some("git_push"));
        assert_eq!(s.keys.get("ctrl+d").map(String::as_str), Some("none"));
        assert!(!s.keys.contains_key("ctrl+r"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
