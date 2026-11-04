//! Several Claude accounts (work, personal…): each one is a configuration folder of Claude Code
//! (`CLAUDE_CONFIG_DIR`) with its own login, quotas and history of sessions. A conversation
//! runs with the account chosen for it; the other calls of Florm (commit messages, PR
//! descriptions…) with the account chosen by default.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::RwLock;

/// An account: its name, and its configuration folder (None: the usual one of Claude Code).
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub name: String,
    pub dir: Option<PathBuf>,
}

/// The name of the usual account, while it has none.
pub fn main_name() -> &'static str {
    t!("Principal", "Main")
}

/// The accounts of the settings ("name=folder", folder empty: the usual one); with none, the
/// usual account alone.
pub fn accounts(list: &[String]) -> Vec<Account> {
    let mut out: Vec<Account> = list
        .iter()
        .filter_map(|s| {
            let (name, dir) = s.split_once('=').unwrap_or((s.as_str(), ""));
            let name = name.trim();
            let dir = dir.trim();
            (!name.is_empty()).then(|| Account { name: name.to_string(), dir: (!dir.is_empty()).then(|| expand(dir)) })
        })
        .collect();
    if out.is_empty() {
        out.push(Account { name: main_name().to_string(), dir: None });
    }
    out
}

/// "name=folder" of the settings.
pub fn spec(name: &str, dir: Option<&Path>) -> String {
    format!("{}={}", name.trim(), dir.map(|d| d.to_string_lossy().into_owned()).unwrap_or_default())
}

/// The account named `name`, else the first one.
pub fn find<'a>(accounts: &'a [Account], name: &str) -> &'a Account {
    accounts.iter().find(|a| a.name == name).unwrap_or(&accounts[0])
}

/// The folder proposed for a new account: `~/.claude-<name>`.
pub fn folder_for(name: &str) -> PathBuf {
    let slug: String = name.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    crate::config::home().join(format!(".claude-{}", slug.trim_matches('-')))
}

fn expand(dir: &str) -> PathBuf {
    match dir.strip_prefix("~/") {
        Some(rest) => crate::config::home().join(rest),
        None => PathBuf::from(dir),
    }
}

/// The account of the conversations of folder `dir`, from the rules "folder=account" of the
/// settings: the rule of the longest folder that holds it.
pub fn for_folder(rules: &[String], dir: &Path) -> Option<String> {
    rules
        .iter()
        .filter_map(|r| r.rsplit_once('='))
        .map(|(f, a)| (expand(f.trim()), a.trim()))
        .filter(|(f, a)| !a.is_empty() && dir.starts_with(f))
        .max_by_key(|(f, _)| f.as_os_str().len())
        .map(|(_, a)| a.to_string())
}

/// "folder=account" of the settings.
pub fn folder_rule(dir: &Path, account: &str) -> String {
    format!("{}={}", dir.display(), account.trim())
}

/// The configuration folder Claude Code uses for `dir` (None: the usual one).
pub fn config_dir(dir: Option<&Path>) -> PathBuf {
    dir.map(Path::to_path_buf)
        .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
        .unwrap_or_else(|| crate::config::home().join(".claude"))
}

static DEFAULT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// The account of the calls of Florm that belong to no conversation.
pub fn set_default(dir: Option<PathBuf>) {
    if let Ok(mut d) = DEFAULT.write() {
        *d = dir;
    }
}

/// `claude`, for the account of folder `dir` (None: the usual one).
pub fn command_for(dir: Option<&Path>) -> Command {
    let mut c = crate::os::command("claude");
    if let Some(d) = dir {
        c.env("CLAUDE_CONFIG_DIR", d);
    }
    c
}

/// `claude`, for the account chosen by default.
pub fn command() -> Command {
    let dir = DEFAULT.read().ok().and_then(|d| d.clone());
    command_for(dir.as_deref())
}

/// What `claude auth status` says of an account.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub logged_in: bool,
    pub email: String,
    /// "pro", "max", "team"… (empty: unknown, or an API key).
    pub plan: String,
}

pub fn parse_status(json: &str) -> Option<Status> {
    let v: Value = serde_json::from_str(json).ok()?;
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    Some(Status { logged_in: v["loggedIn"].as_bool().unwrap_or(false), email: s("email"), plan: s("subscriptionType") })
}

/// Whether the account of folder `dir` is logged in, and as whom.
pub fn status(dir: Option<&Path>) -> Result<Status, String> {
    let out = command_for(dir).args(["auth", "status", "--json"]).output().map_err(|_| crate::claude::not_installed())?;
    parse_status(&String::from_utf8_lossy(&out.stdout)).ok_or_else(|| String::from_utf8_lossy(&out.stderr).trim().to_string())
}

/// What runs the login of an account in a terminal.
pub fn login_command(dir: Option<&Path>) -> (Vec<String>, Vec<(String, String)>) {
    let env = dir.map(|d| vec![("CLAUDE_CONFIG_DIR".to_string(), d.to_string_lossy().into_owned())]).unwrap_or_default();
    (["claude", "auth", "login"].map(String::from).to_vec(), env)
}

/// The session `session` of an account copied to another one: a conversation goes on with
/// the other account (Claude Code keeps the sessions in `<config>/projects/<folder>/<id>.jsonl`).
pub fn copy_session(from: Option<&Path>, to: Option<&Path>, session: &str) -> Result<(), String> {
    let (from, to) = (config_dir(from), config_dir(to));
    if from == to {
        return Ok(());
    }
    let file = format!("{session}.jsonl");
    for project in std::fs::read_dir(from.join("projects")).map_err(|e| e.to_string())?.flatten() {
        let src = project.path().join(&file);
        if src.is_file() {
            let dest = to.join("projects").join(project.file_name());
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            std::fs::copy(&src, dest.join(&file)).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err(t!("session introuvable dans l'autre compte", "session not found in the other account").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_of_the_settings() {
        assert_eq!(accounts(&[]), vec![Account { name: main_name().into(), dir: None }]);
        let home = crate::config::home();
        let a = accounts(&["Pro=".into(), "Perso=~/.claude-perso".into(), "=x".into()]);
        assert_eq!(a, vec![Account { name: "Pro".into(), dir: None }, Account { name: "Perso".into(), dir: Some(home.join(".claude-perso")) }]);
        assert_eq!(find(&a, "Perso").name, "Perso");
        assert_eq!(find(&a, "?").name, "Pro");
        assert_eq!(spec("Perso", Some(Path::new("/h/.c"))), "Perso=/h/.c");
        assert_eq!(folder_for("Mon Perso"), home.join(".claude-mon-perso"));
    }

    #[test]
    fn the_account_of_a_folder() {
        let rules = vec!["/src=Pro".to_string(), "/src/florm=Perso".to_string(), "/x=".to_string()];
        assert_eq!(for_folder(&rules, Path::new("/src/florm/src")).as_deref(), Some("Perso"));
        assert_eq!(for_folder(&rules, Path::new("/src/internal")).as_deref(), Some("Pro"));
        // a folder of the same beginning is not inside
        assert_eq!(for_folder(&rules, Path::new("/src/florm2")).as_deref(), Some("Pro"));
        assert_eq!(for_folder(&rules, Path::new("/x/y")), None);
        assert_eq!(for_folder(&rules, Path::new("/home")), None);
        assert_eq!(folder_rule(Path::new("/src/florm"), "Perso"), "/src/florm=Perso");
    }

    #[test]
    fn status_of_an_account() {
        let s = parse_status(r#"{"loggedIn":true,"authMethod":"claude.ai","email":"a@b.c","subscriptionType":"max"}"#).unwrap();
        assert_eq!(s, Status { logged_in: true, email: "a@b.c".into(), plan: "max".into() });
        assert!(!parse_status(r#"{"loggedIn":false,"authMethod":"none"}"#).unwrap().logged_in);
    }

    #[test]
    fn a_session_goes_to_another_account() {
        let t = std::env::temp_dir().join(format!("florm-acc-{}", std::process::id()));
        let (a, b) = (t.join("a"), t.join("b"));
        std::fs::create_dir_all(a.join("projects/-home-x")).unwrap();
        std::fs::write(a.join("projects/-home-x/s1.jsonl"), "{}").unwrap();
        copy_session(Some(&a), Some(&b), "s1").unwrap();
        assert_eq!(std::fs::read_to_string(b.join("projects/-home-x/s1.jsonl")).unwrap(), "{}");
        assert!(copy_session(Some(&a), Some(&b), "s2").is_err());
        let _ = std::fs::remove_dir_all(t);
    }
}
