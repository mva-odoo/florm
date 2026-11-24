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
    /// Interface font; empty: the default one (the editor's font with the "square" style).
    pub ui_font: String,
    /// Look of the interface: "rounded" (cards, rounded corners, shadows) or "square" (straight
    /// corners, no shadows, the interface in the editor's monospaced font, like a terminal).
    pub ui_style: String,
    pub show_minimap: bool,
    /// Spell checking of the Markdown, the comments and the commit messages (French and English).
    pub spell_check: bool,
    /// Brackets colored by level of nesting.
    pub rainbow_brackets: bool,
    /// Spaces and tabs shown in the editor (· and →).
    pub show_whitespace: bool,
    /// Lines longer than the editor shown on several rows (Alt+Z).
    pub word_wrap: bool,
    /// The class / function lines around the top of the editor stay pinned there while scrolling.
    pub sticky_scroll: bool,
    /// Columns of vertical rulers, e.g. [79, 120].
    pub rulers: Vec<usize>,
    /// Path and enclosing class / function above the editor.
    pub breadcrumbs: bool,
    /// Menu bar (Fichier, Édition…) at the top; hidden, its commands are in the palette.
    pub show_menu_bar: bool,
    /// "Find in files" in an editor tab instead of the bottom panel.
    pub search_in_tab: bool,
    /// Diffs side by side (old on the left, new on the right) instead of in one column.
    pub diff_side_by_side: bool,
    /// Claude's commands run in the sandbox of Claude Code (bubblewrap on Linux, Seatbelt on macOS).
    pub claude_sandbox: bool,
    /// Parts that can be turned off: Git (off by itself without `git`), GitHub (pull requests),
    /// the debugger, Claude.
    pub git: bool,
    pub github: bool,
    pub debugger: bool,
    pub claude: bool,
    /// The panel of the session (context, cost, quotas) on the right of Claude.
    pub claude_review: bool,
    /// Claude is done and its conversation is not shown: its tab blinks a few seconds.
    pub claude_done_blink: bool,
    /// Sound played when Claude is done (`sound::SOUNDS`); empty: none.
    pub claude_done_sound: String,
    /// Claude accounts: "name=configuration folder" (empty folder: the usual one of Claude Code).
    pub claude_accounts: Vec<String>,
    /// The account of new conversations and of the other calls (commit messages…); empty: the first.
    pub claude_account: String,
    /// Folders whose conversations use an account: "folder=account name" (the longest that
    /// holds the folder of the conversation).
    pub claude_account_folders: Vec<String>,
    /// Branch and upstream of git repositories on their folder in the tree.
    /// The toolbar of formatting buttons above a Markdown source (the source stays editable).
    pub markdown_toolbar: bool,
    pub tree_git_info: bool,
    /// Show git changes inside the editor (deleted lines in red, added in green).
    pub inline_diff: bool,
    pub auto_pair: bool,
    /// Alt alone puts letters on everything clickable: typing them clicks it.
    pub keyboard_hints: bool,
    /// AI completion: Haiku suggests the rest of the line after a pause in typing (Tab takes it).
    pub ai_complete: bool,
    /// The Claude account (its subscription) the AI completion uses; empty: the default one.
    pub ai_complete_account: String,
    /// Who suggests the AI completion: "claude" (Haiku) or "copilot" (GitHub Copilot).
    pub ai_complete_source: String,
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
    /// Files saved by themselves: "off", "focus" (when Florm loses the focus or another tab is
    /// shown), "delay" (a second after the last keystroke).
    pub auto_save: String,
    /// Shell used by the terminal panel.
    pub shell: Option<String>,
    /// "ctrl+shift+r" = "git_push", chords as "ctrl+k ctrl+b".
    pub keys: HashMap<String, String>,
    /// Language servers: completion, go to definition, rename.
    pub lsp: bool,
    /// Server command per language family ("python", "typescript", "rust", "go", "c"); else detected.
    pub lsp_servers: HashMap<String, String>,
    /// Folders trusted (with those inside): their language servers and the linters set up by the
    /// project run there (they may run its code). Elsewhere Florm is in restricted mode.
    pub trusted_folders: Vec<String>,
    /// Left out of go to file, the symbol index and find in files (gitignore syntax).
    pub exclude: Vec<String>,
    /// Most source files in the symbol index (the repositories of the open files come first).
    pub index_max_files: usize,
    /// Look for a newer release of Florm at startup (GitHub).
    pub check_updates: bool,
    /// Last test commands run, newest first.
    pub odoo_test_history: Vec<String>,
    /// Views opened in an editor tab rather than in the bottom panel: "debug" (search has its own
    /// setting; Git, the pull requests and Claude are always in a tab). ⇱ / ⇲ in a view change it.
    pub views_in_tab: Vec<String>,
    /// Debugger: the container the program runs in, empty for this computer.
    pub debug_target: String,
    /// "launch" (Florm starts the program) or "attach" (it listens with debugpy).
    pub debug_mode: String,
    /// Python program launched here and its arguments; empty: the odoo-bin of the folder.
    pub debug_command: String,
    /// The same for Node, Rust / C and Go; empty: `npm start`, `cargo run`, `go run .`…
    pub debug_command_node: String,
    pub debug_command_native: String,
    pub debug_command_go: String,
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
    /// State of the pull requests shown at the start: "open", "merged", "closed" or "all".
    pub pr_default_state: String,
    /// Whose pull requests are shown at the start: "mine", "all", "to_review" or "reviewed".
    pub pr_default_who: String,
    /// The resolved conversations of a pull request shown (otherwise hidden).
    pub pr_show_resolved: bool,
    /// A new pull request is created as a draft.
    pub pr_draft: bool,
    /// Shared configuration (settings.toml, templates/, themes/) in a git repository:
    /// "login" (login/florm-config on GitHub), "owner/repo" or a git address; empty: none.
    pub shared_config: String,
    /// The folder of the skills of Claude (empty: `skills/` of the shared configuration).
    pub skills_dir: String,
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
            ui_style: "rounded".into(),
            show_minimap: true,
            spell_check: true,
            rainbow_brackets: true,
            show_whitespace: false,
            word_wrap: false,
            sticky_scroll: true,
            rulers: vec![80],
            breadcrumbs: true,
            show_menu_bar: true,
            search_in_tab: false,
            diff_side_by_side: false,
            claude_sandbox: true,
            git: true,
            github: true,
            debugger: true,
            claude: true,
            claude_review: true,
            claude_done_blink: true,
            claude_done_sound: "chime".into(),
            claude_accounts: Vec::new(),
            claude_account: String::new(),
            claude_account_folders: Vec::new(),
            markdown_toolbar: true,
            tree_git_info: true,
            inline_diff: false,
            auto_pair: true,
            keyboard_hints: true,
            ai_complete: false,
            ai_complete_account: String::new(),
            ai_complete_source: "claude".into(),
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
            auto_save: "off".into(),
            shell: None,
            keys: HashMap::new(),
            lsp: true,
            lsp_servers: HashMap::new(),
            trusted_folders: Vec::new(),
            exclude: ["__pycache__/", "*.pyc", "node_modules/", ".venv/", "*.egg-info/"].map(String::from).to_vec(),
            index_max_files: 150_000,
            check_updates: true,
            odoo_test_history: Vec::new(),
            views_in_tab: vec!["debug".into()],
            debug_target: String::new(),
            debug_mode: "launch".into(),
            debug_command: String::new(),
            debug_command_node: String::new(),
            debug_command_native: String::new(),
            debug_command_go: String::new(),
            debug_docker_command: String::new(),
            debug_python: if cfg!(windows) { "python".into() } else { "python3".into() },
            debug_port: 5678,
            debug_exceptions: vec!["uncaught".into()],
            debug_just_my_code: false,
            pr_default_state: "open".into(),
            pr_default_who: "mine".into(),
            pr_show_resolved: false,
            pr_draft: false,
            shared_config: String::new(),
            skills_dir: String::new(),
        }
    }
}

const DEFAULT_SETTINGS: &str = r#"# Préférences de Florm (enregistrer ce fichier les applique immédiatement)
# langue de l'interface : "fr" (français), "en" (English) ; vide = celle du système
language = "fr"
font_size = 14.0
tab_size = 4
translate_tabs_to_spaces = true
# Thèmes intégrés : Florm, Florm Light, Florm Monokai, Florm One Dark, Florm Dracula, Florm GitHub Light.
# Tout fichier .tmTheme déposé dans le dossier "themes" de la config est aussi chargé.
theme = "Florm"
# couleur d'accentuation de l'interface, ex. #4F8CFF ; vide = celle du thème
accent_color = ""
# texte adouci (0.0 à 0.4) : couleurs du texte mélangées au fond, pour un texte moins « gras »
text_softness = 0.0
# polices installées sur le système ; vide = par défaut (Consolas / Segoe UI sous Windows)
editor_font = ""
ui_font = ""
# style de l'interface : "rounded" (coins arrondis, ombres) ou "square" (coins droits, sans ombres, police de l'éditeur partout, comme un terminal)
ui_style = "rounded"
show_minimap = true
# orthographe (français et anglais) du Markdown, des commentaires et des messages de commit ;
# les dictionnaires sont téléchargés la première fois
spell_check = true
# crochets colorés selon leur niveau d'imbrication
rainbow_brackets = true
# espaces et tabulations visibles dans l'éditeur (· et →)
show_whitespace = false
# retour à la ligne automatique des lignes trop longues pour l'éditeur (Alt+Z)
word_wrap = false
# classe / fonction courantes figées en haut de l'éditeur pendant le défilement
sticky_scroll = true
# règles verticales aux colonnes données, ex. [79, 120]
rulers = [80]
# fil d'Ariane (dossier › fichier › classe › méthode) au-dessus de l'éditeur
breadcrumbs = true
# barre de menus en haut (masquée : ses commandes restent dans la palette, Ctrl+Shift+P)
show_menu_bar = true
# recherche dans le dossier (Ctrl+Shift+F) dans un onglet plutôt que dans le panneau du bas
search_in_tab = false
# diffs côte à côte (l'ancienne version à gauche, la nouvelle à droite) plutôt qu'en une colonne
diff_side_by_side = false
# Fonctions à activer ou désactiver (false : leurs vues, commandes, menus et boutons disparaissent).
# Désactivées à l'installation : Préférences → leur section dit ce qu'il faut installer pour les activer.
# Git est actif dès que la commande git est installée
git = true
github = false
debugger = false
claude = false
# les commandes de Claude tournent dans le bac à sable de Claude Code : écriture dans le projet seulement,
# réseau filtré (Linux : paquets bubblewrap et socat ; macOS : Seatbelt, intégré ; pas sous Windows hors WSL).
# false : aucun bac à sable, même si les réglages de Claude Code en activent un. Pris en compte sans redémarrer Florm
claude_sandbox = true
# session de Claude (contexte, coût, quotas) à droite de son terminal
claude_review = true
# comptes Claude ("nom=dossier de configuration", dossier vide = celui habituel de Claude Code), par ex.
# ["Pro=", "Perso=~/.claude-perso"] ; compte des nouvelles conversations et des messages de commit…
claude_accounts = []
claude_account = ""
# dossiers dont les conversations utilisent un compte : "dossier=nom du compte"
claude_account_folders = []
# le débogage s'ouvre dans un onglet ("debug") plutôt que dans le panneau du bas ; ⇱ / ⇲ dans son
# en-tête changent ce choix (Git, Pull requests et Claude sont toujours dans un onglet)
views_in_tab = ["debug"]
# barre de mise en forme au-dessus du Markdown (le texte reste modifiable à la main)
markdown_toolbar = true
# branche et branche distante des dépôts git sur leur dossier dans l'arborescence
tree_git_info = true
# diff git dans l'éditeur : lignes supprimées en rouge, ajoutées en vert (Ctrl+Alt+D)
inline_diff = false
auto_pair = true
# Alt seul met des lettres sur tout ce qui se clique : les taper le clique
keyboard_hints = true
# Complétion par IA : Haiku propose la suite de la ligne après une pause dans la frappe (Tab la prend)
ai_complete = false
# Le compte Claude (son abonnement) de la complétion par IA ; vide : le compte par défaut
ai_complete_account = ""
# Qui propose la complétion par IA : "claude" (Haiku) ou "copilot" (GitHub Copilot)
ai_complete_source = "claude"
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
# enregistrement automatique : "off", "focus" (quand Florm perd le focus ou qu'on change d'onglet), "delay" (une seconde après la dernière frappe)
auto_save = "off"
# shell = "/bin/bash"
# Serveurs de langage (complétion Ctrl+Espace, définition F12, renommer F2)
lsp = true
# dossiers de confiance (et ceux qu'ils contiennent) : les serveurs de langage et les linters configurés
# par le projet (flake8, eslint) n'y tournent qu'une fois le dossier approuvé, car ils peuvent exécuter son code
trusted_folders = []
# Exclus de « aller au fichier » (Ctrl+P), de l'index des symboles et de la recherche (syntaxe .gitignore)
exclude = ["__pycache__/", "*.pyc", "node_modules/", ".venv/", "*.egg-info/"]
# Fichiers source au plus dans l'index des symboles (les dépôts des fichiers ouverts d'abord)
index_max_files = 150000
# chercher une nouvelle version de Florm au démarrage (releases GitHub)
check_updates = true
# débogueur (F5, vue Débogage) : programme Python lancé sur cet ordinateur (vide = l'odoo-bin du
# dossier) et dans un conteneur (vide = "odoo --workers 0 --http-port 8070") ; ceux de Node, Rust /
# C et Go (vides = npm start ou le "main" de package.json, cargo run, go run .)
debug_command = ""
debug_docker_command = ""
debug_command_node = ""
debug_command_native = ""
debug_command_go = ""
# pull requests : état affiché au départ ("open", "merged", "closed", "all"), auteur ("mine",
# "all", "to_review", "reviewed"), conversations résolues affichées, nouvelle PR en brouillon
pr_default_state = "open"
pr_default_who = "mine"
pr_show_resolved = false
pr_draft = false
# configuration partagée entre ordinateurs : un dépôt GitHub avec settings.toml, templates/,
# et themes/ ; "login" (= login/florm-config), "owner/repo" ou une adresse git
shared_config = ""
# dossier des skills de Claude, donnés à chaque conversation (vide = skills/ de la configuration
# partagée) ; n'importe quel rangement, chaque SKILL.md trouvé est un skill
skills_dir = ""

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

/// The folders of Florm only readable by the user: they hold the Claude conversations (code,
/// pasted texts), the sessions (the SQL connection), the copies of the local history. On Windows,
/// the folders of the user profile already are.
pub fn make_private() {
    #[cfg(unix)]
    for d in [config_dir(), data_dir()] {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::create_dir_all(&d).is_ok() {
            let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
        }
    }
}

/// Data folder (tools installed by Florm, X11 libraries on WSL…).
#[cfg(test)]
pub fn data_dir() -> PathBuf {
    // the tests never read nor write the user's data (the Claude conversations…): next to their
    // configuration, else in a folder of their own
    match std::env::var_os("FLORM_CONFIG") {
        Some(d) => PathBuf::from(d).join("data"),
        None => std::env::temp_dir().join(format!("florm_test_data_{}", std::process::id())),
    }
}

#[cfg(not(test))]
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
    crate::skills::set_dir(table.get("skills_dir").and_then(|v| v.as_str()).unwrap_or(""));
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

/// The line after the value starting at line `i`: the first that makes `lines[i..end]` valid
/// TOML. Line `i` alone when none does (a value already broken is not made worse).
fn value_end(lines: &[String], i: usize) -> usize {
    let mut text = String::new();
    for (j, l) in lines.iter().enumerate().skip(i) {
        text.push_str(l);
        text.push('\n');
        if text.parse::<toml::Table>().is_ok() {
            return j + 1;
        }
    }
    i + 1
}

fn set_setting_in(p: &std::path::Path, key: &str, value: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(p).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let section = lines.iter().position(|l| l.trim_start().starts_with('[')).unwrap_or(lines.len());
    let line = format!("{key} = {value}");
    match top_level_index(&text, key) {
        // the whole old value is replaced, even written over several lines (a list, a long text)
        Some(i) => {
            let end = value_end(&lines, i);
            lines.splice(i..end, [line]);
        }
        None => lines.insert(section, line),
    }
    std::fs::write(p, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value written over several lines by hand is replaced whole: the file stays valid.
    #[test]
    fn set_setting_replaces_a_value_over_several_lines() {
        let p = std::env::temp_dir().join(format!("florm_cfg_multi_{}.toml", std::process::id()));
        let text = "exclude = [\n  \"a\",\n  # b aussi\n  \"b\",\n]\nnote = \"\"\"\nun\ndeux = 2\n\"\"\"\nlsp = true\n\n[keys]\n\"ctrl+r\" = \"git_push\"\n";
        std::fs::write(&p, text).unwrap();
        set_setting_in(&p, "exclude", "[\"c\"]").unwrap();
        set_setting_in(&p, "note", "\"court\"").unwrap();
        let out = std::fs::read_to_string(&p).unwrap();
        assert_eq!(out, "exclude = [\"c\"]\nnote = \"court\"\nlsp = true\n\n[keys]\n\"ctrl+r\" = \"git_push\"\n");
        let t: toml::Table = out.parse().unwrap();
        assert_eq!(t["keys"]["ctrl+r"].as_str(), Some("git_push"));
        // a broken value: only its line is replaced
        std::fs::write(&p, "exclude = [\nlsp = true\n").unwrap();
        set_setting_in(&p, "exclude", "[]").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "exclude = []\nlsp = true\n");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn set_setting_keeps_comments() {
        let _lock = TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
