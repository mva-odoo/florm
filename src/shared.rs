//! Shared configuration: a git repository (on GitHub) cloned in the configuration folder, so that
//! every computer gets the same preferences, templates, themes and skills. Its layout is fixed,
//! every part optional:
//!
//! ```text
//! settings.toml   preferences applied over the local ones (only the keys it sets)
//! templates/      templates, as the local "templates" folder
//! themes/         .tmTheme files
//! ```
//!
//! The `shared_config` setting names it: "login" (the repository login/florm-config),
//! "owner/repo", or any git address.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Name of the repository when only a GitHub login is given.
pub const DEFAULT_REPO: &str = "florm-config";

/// Settings that stay on each computer (never taken from the shared settings.toml, never written
/// to it): the commands run, the programs and folders used as configuration (Claude's folder
/// holds its hooks), what is trusted or let out of the sandbox. A shared repository must not set
/// them: it could run any command on every computer that syncs it. A new setting is put here or
/// in `tests::SHARED` (a test checks that each one is in one of the two).
pub const LOCAL_KEYS: &[&str] = &[
    "shared_config", "skills_dir", "test_history", "shell",
    "lsp_servers", "linters", "python_linter", "python_lint_args", "trusted_folders", "claude_accounts", "claude_account", "claude_account_folders",
    "claude_sandbox",
];

/// `shared_config` of the local settings (set by `config::load`).
static SPEC: Mutex<String> = Mutex::new(String::new());

pub fn set_spec(spec: &str) {
    *SPEC.lock().unwrap_or_else(|e| e.into_inner()) = spec.trim().to_string();
}

pub fn spec() -> String {
    SPEC.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Address to clone and "owner/repo" when it is on GitHub.
pub fn resolve(spec: &str) -> Option<(String, Option<String>)> {
    let s = spec.trim().trim_end_matches('/');
    if s.is_empty() {
        return None;
    }
    if s.contains("://") || s.starts_with("git@") {
        let slug = ["https://github.com/", "http://github.com/", "git@github.com:", "ssh://git@github.com/"]
            .iter()
            .find_map(|p| s.strip_prefix(p))
            .map(|r| r.trim_end_matches(".git").to_string());
        return Some((s.to_string(), slug));
    }
    let slug = if s.contains('/') { s.trim_end_matches(".git").to_string() } else { format!("{s}/{DEFAULT_REPO}") };
    Some((format!("https://github.com/{slug}.git"), Some(slug)))
}

/// Short name of the repository, for the messages.
pub fn label(spec: &str) -> String {
    resolve(spec).map(|(url, slug)| slug.unwrap_or(url)).unwrap_or_default()
}

/// Folder the repository of `spec` is cloned in (one per repository: changing it keeps the other).
pub fn clone_dir(spec: &str) -> Option<PathBuf> {
    let name: String = label(spec).chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect();
    (!name.is_empty()).then(|| crate::config::config_dir().join("shared").join(name))
}

/// Folder of the shared configuration, when one is set and cloned.
pub fn dir() -> Option<PathBuf> {
    clone_dir(&spec()).filter(|d| d.join(".git").exists())
}

/// `sub` ("templates", "themes"…) of the shared configuration.
pub fn sub(name: &str) -> Option<PathBuf> {
    dir().map(|d| d.join(name))
}

pub fn settings_path() -> Option<PathBuf> {
    sub("settings.toml")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Sync,
    Publish,
    Create,
}

/// Does `op` (in the background): the `shared_config` it ends with, and the message.
pub fn run(op: Op, spec: &str) -> Result<(String, String), String> {
    match op {
        Op::Sync => sync(spec).map(|m| (spec.to_string(), m)),
        Op::Publish => publish(spec).map(|m| (spec.to_string(), m)),
        Op::Create => create(spec),
    }
}

/// Clones the repository, or brings it up to date (changes not published yet are kept on top).
pub fn sync(spec: &str) -> Result<String, String> {
    let (url, slug) = resolve(spec).ok_or_else(|| t!("Aucun dépôt de configuration partagée", "No shared configuration repository").to_string())?;
    let dir = clone_dir(spec).unwrap_or_default();
    let name = label(spec);
    if dir.join(".git").exists() {
        if remote_is_empty(&dir) {
            return Ok(tf!("Configuration partagée {name} : dépôt vide", "Shared configuration {name}: empty repository"));
        }
        crate::git::run(&dir, &["pull", "--rebase", "--autostash"])?;
        return Ok(tf!("Configuration partagée {name} à jour", "Shared configuration {name} up to date"));
    }
    clone(&url, slug.as_deref(), &dir)?;
    Ok(tf!("Configuration partagée {name} récupérée", "Shared configuration {name} fetched"))
}

/// Commits the changes of the shared folder and pushes them.
pub fn publish(spec: &str) -> Result<String, String> {
    let dir = clone_dir(spec).filter(|d| d.join(".git").exists()).ok_or_else(|| t!("Configuration partagée pas encore récupérée : synchronisez d'abord", "Shared configuration not fetched yet: synchronize first").to_string())?;
    crate::git::run(&dir, &["add", "-A"])?;
    let changed = !crate::git::stdout(&dir, &["status", "--porcelain"])?.trim().is_empty();
    if changed {
        crate::git::run(&dir, &["commit", "-m", &format!("Florm : configuration depuis {}", hostname())])?;
    }
    if !remote_is_empty(&dir) {
        crate::git::run(&dir, &["pull", "--rebase", "--autostash"])?;
    }
    crate::git::run(&dir, &["push", "-u", "origin", "HEAD"])?;
    let name = label(spec);
    Ok(if changed {
        tf!("Configuration partagée publiée sur {name}", "Shared configuration published to {name}")
    } else {
        tf!("Rien de nouveau à publier sur {name}", "Nothing new to publish to {name}")
    })
}

/// Creates the repository on GitHub (private, with `gh`), filled with the local configuration,
/// and pushes it. `spec` empty: login/florm-config of the connected account. Returns the
/// `shared_config` to keep and the message.
pub fn create(spec: &str) -> Result<(String, String), String> {
    if crate::lsp::in_path("gh").is_none() {
        return Err(t!("Créer le dépôt demande gh (GitHub CLI), connecté", "Creating the repository needs gh (GitHub CLI), logged in").into());
    }
    let spec = if spec.trim().is_empty() { gh(&["api", "user", "--jq", ".login"])?.trim().to_string() } else { spec.trim().to_string() };
    let slug = resolve(&spec).and_then(|(_, s)| s).ok_or_else(|| t!("Seul un dépôt GitHub peut être créé", "Only a GitHub repository can be created").to_string())?;
    gh(&["repo", "create", &slug, "--private", "--description", "Configuration de Florm (préférences, modèles, thèmes)"])?;
    let dir = clone_dir(&spec).unwrap_or_default();
    if !dir.join(".git").exists() {
        clone(&format!("https://github.com/{slug}.git"), Some(&slug), &dir)?;
    }
    fill_from_local(&dir).map_err(|e| e.to_string())?;
    publish(&spec)?;
    Ok((spec, tf!("Dépôt {slug} créé avec la configuration de cet ordinateur", "Repository {slug} created with the configuration of this computer")))
}

/// Copies the local settings (without the ones of this computer), templates, themes and skills.
fn fill_from_local(dir: &Path) -> std::io::Result<()> {
    let cfg = crate::config::config_dir();
    let text = std::fs::read_to_string(crate::config::settings_path()).unwrap_or_default();
    std::fs::write(dir.join("settings.toml"), portable_settings(&text))?;
    std::fs::write(dir.join("README.md"), README)?;
    for name in ["templates", "themes", "skills"] {
        copy_dir(&cfg.join(name), &dir.join(name))?;
    }
    Ok(())
}

/// settings.toml without the keys that belong to one computer.
pub fn portable_settings(text: &str) -> String {
    // the section the line is in ([lsp_servers] is local, [keys] is not)
    let mut section: Option<String> = None;
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            if t.starts_with('[') {
                section = Some(t.trim_start_matches('[').split(']').next().unwrap_or("").trim().trim_matches('"').to_string());
            }
            match &section {
                // [linters.shellcheck] is in [linters]
                Some(name) => !LOCAL_KEYS.contains(&name.split('.').next().unwrap_or("").trim().trim_matches('"')),
                None => !LOCAL_KEYS.iter().any(|k| t.strip_prefix(k).is_some_and(|r| r.trim_start().starts_with('='))),
            }
        })
        .collect();
    kept.join("\n") + "\n"
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    let Ok(rd) = std::fs::read_dir(from) else { return Ok(()) };
    for e in rd.flatten() {
        let (p, name) = (e.path(), e.file_name());
        if name == ".git" {
            continue;
        }
        if p.is_dir() {
            copy_dir(&p, &to.join(&name))?;
        } else {
            std::fs::create_dir_all(to)?;
            std::fs::copy(&p, to.join(&name))?;
        }
    }
    Ok(())
}

fn clone(url: &str, slug: Option<&str>, dir: &Path) -> Result<(), String> {
    let parent = dir.parent().unwrap_or(dir);
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let target = dir.to_string_lossy().into_owned();
    // gh knows the account: private repositories, https or ssh as chosen in gh
    if let (Some(slug), Some(_)) = (slug, crate::lsp::in_path("gh")) {
        if gh(&["repo", "clone", slug, &target]).is_ok() {
            return Ok(());
        }
        let _ = std::fs::remove_dir_all(dir);
    }
    crate::git::run(parent, &["clone", url, &target]).map(|_| ())
}

fn remote_is_empty(dir: &Path) -> bool {
    crate::git::stdout(dir, &["ls-remote", "--heads", "origin"]).is_ok_and(|o| o.trim().is_empty())
}

fn gh(args: &[&str]) -> Result<String, String> {
    let out = crate::os::command("gh")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| tf!("gh introuvable : {e}", "gh not found: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("gh {} : {}", args.first().copied().unwrap_or(""), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| crate::os::command("hostname").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()))
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "un ordinateur".into())
}

const README: &str = "# Configuration Florm partagée\n\n\
Florm récupère ce dépôt sur chaque ordinateur où il est indiqué (Préférences → Avancé →\n\
« Configuration partagée »), et l'utilise en plus de la configuration locale :\n\n\
- `settings.toml` : préférences, appliquées par-dessus celles de l'ordinateur (seulement les\n\
  clés écrites ici ; les tables comme `[keys]` sont fusionnées) ;\n\
- `templates/` : modèles (un fichier par modèle, comme le dossier local) ;\n\
- `themes/` : thèmes `.tmTheme` ;\n\
- `skills/` : skills de Claude (`skills/nom/SKILL.md`), donnés à chaque conversation.\n\n\
Les préférences modifiées dans Florm et présentes ici sont écrites ici aussi ; les nouveaux\n\
modèles, thèmes et skills sont créés ici. « Publier » enregistre et pousse les changements.\n";

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings a shared repository may set: appearance, behaviour, toggles. Nothing that
    /// names a command, a program, a folder used as configuration or that grants a permission.
    const SHARED: &[&str] = &[
        "language", "font_size", "tab_size", "translate_tabs_to_spaces", "theme", "accent_color", "text_softness", "editor_font", "ui_font", "show_minimap",
        "spell_check", "rainbow_brackets", "show_whitespace", "word_wrap", "sticky_scroll", "rulers", "breadcrumbs", "show_menu_bar", "search_in_tab", "diff_side_by_side",
        "git", "github", "claude", "claude_done_blink", "claude_done_sound", "markdown_toolbar", "tree_git_info", "inline_diff", "auto_pair",
        "keyboard_hints", "ai_complete", "lint_on_save", "lint_live", "lint_ignore_flake8", "lint_ignore_ruff", "lint_ignore_pylint", "lint_ignore_eslint",
        "trim_trailing_whitespace_on_save", "format_on_save", "hot_exit", "auto_save", "keys", "lsp", "exclude", "index_max_files", "check_updates",
        "pr_default_state", "pr_default_who", "pr_show_resolved", "pr_draft",
    ];

    /// Each setting is either shared or local: a new one cannot be taken from a shared
    /// repository without a choice made here.
    #[test]
    fn each_setting_is_shared_or_local() {
        let v = serde_json::to_value(crate::config::Settings::default()).unwrap();
        let fields: Vec<&String> = v.as_object().unwrap().keys().collect();
        for f in &fields {
            let (shared, local) = (SHARED.contains(&f.as_str()), LOCAL_KEYS.contains(&f.as_str()));
            assert!(shared != local, "{f}: to put in LOCAL_KEYS or in SHARED (not both)");
        }
        for k in SHARED.iter().chain(LOCAL_KEYS) {
            assert!(fields.iter().any(|f| f == k), "{k}: not a setting");
        }
    }

    #[test]
    fn local_settings_are_not_exported() {
        let text = "theme = \"x\"\nclaude_accounts = [\"a=/tmp\"]\nclaude_sandbox = false\n[lsp_servers]\npython = \"evil\"\n[keys]\n\"ctrl+k\" = \"save\"\n";
        assert_eq!(portable_settings(text), "theme = \"x\"\n[keys]\n\"ctrl+k\" = \"save\"\n");
        // the linters and servers of this computer, in sections of their own, stay here too
        let text = "theme = \"x\"\n[linters.evil]\ncmd = \"rm -rf ~\"\n[lsp_servers.nim]\ncmd = \"x\"\n[keys]\n\"ctrl+k\" = \"save\"\n";
        assert_eq!(portable_settings(text), "theme = \"x\"\n[keys]\n\"ctrl+k\" = \"save\"\n");
    }

    #[test]
    fn resolves_the_repository() {
        assert_eq!(resolve("mva"), Some(("https://github.com/mva/florm-config.git".into(), Some("mva/florm-config".into()))));
        assert_eq!(resolve(" mva/dotfiles/ "), Some(("https://github.com/mva/dotfiles.git".into(), Some("mva/dotfiles".into()))));
        assert_eq!(resolve("git@github.com:mva/cfg.git").unwrap().1.as_deref(), Some("mva/cfg"));
        assert_eq!(resolve("https://gitlab.com/mva/cfg.git").unwrap().1, None);
        assert_eq!(resolve("  "), None);
        assert!(clone_dir("mva").unwrap().ends_with("shared/mva-florm-config"));
    }

    #[test]
    fn local_keys_stay_local() {
        let text = "theme = \"Florm\"\nshell = \"fish\"\nshared_config = \"mva\"\n\n[keys]\n\"ctrl+r\" = \"git_push\"\n";
        assert_eq!(portable_settings(text), "theme = \"Florm\"\n\n[keys]\n\"ctrl+r\" = \"git_push\"\n");
    }
}

#[cfg(test)]
mod round_trip {
    use super::*;

    /// Changes FLORM_CONFIG: run alone (`cargo test round_trip -- --ignored`).
    #[test]
    #[ignore]
    fn sync_merge_and_publish() {
        let base = std::env::temp_dir().join(format!("florm_shared_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (remote, work, cfg) = (base.join("remote.git"), base.join("work"), base.join("cfg"));
        std::fs::create_dir_all(&cfg).unwrap();
        crate::git::run(&base, &["init", "--bare", "-b", "main", &remote.to_string_lossy()]).unwrap();
        crate::git::run(&base, &["clone", &remote.to_string_lossy(), &work.to_string_lossy()]).unwrap();
        std::fs::create_dir_all(work.join("templates")).unwrap();
        std::fs::write(work.join("templates/brk.py"), "breakpoint()").unwrap();
        std::fs::write(work.join("settings.toml"), "tab_size = 2\nshell = \"zsh\"\n[keys]\n\"ctrl+r\" = \"git_push\"\n").unwrap();
        for a in [&["add", "-A"][..], &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "init"], &["push", "origin", "HEAD"]] {
            crate::git::run(&work, a).unwrap();
        }
        std::env::set_var("FLORM_CONFIG", &cfg);
        let url = format!("file://{}", remote.display());
        std::fs::write(cfg.join("settings.toml"), format!("tab_size = 4\nfont_size = 15.0\nshell = \"fish\"\nshared_config = {}\n", crate::config::toml_str(&url))).unwrap();
        crate::config::load();
        sync(&url).unwrap();
        let (s, err) = crate::config::load();
        assert_eq!(err, None);
        assert_eq!((s.tab_size, s.font_size, s.shell.as_deref()), (2, 15.0, Some("fish")));
        assert_eq!(s.keys.get("ctrl+r").map(String::as_str), Some("git_push"));
        assert!(crate::templates::load_all().iter().any(|t| t.trigger == "brk"));
        // a setting of the shared file changed in Florm goes there, and is published
        crate::config::set_setting("tab_size", "3").unwrap();
        let shared = dir().unwrap();
        assert!(std::fs::read_to_string(shared.join("settings.toml")).unwrap().contains("tab_size = 3"));
        crate::git::run(&shared, &["config", "user.name", "t"]).unwrap();
        crate::git::run(&shared, &["config", "user.email", "t@t"]).unwrap();
        publish(&url).unwrap();
        crate::git::run(&work, &["pull"]).unwrap();
        assert!(std::fs::read_to_string(work.join("settings.toml")).unwrap().contains("tab_size = 3"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
