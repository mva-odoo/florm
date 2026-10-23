//! Projects: every opened folder is remembered, with its session
//! (open files, cursors, expanded folders, search filters).

use crate::config;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub last_opened: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct ProjectList {
    #[serde(default)]
    projects: Vec<Project>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Session {
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub search_include: String,
    #[serde(default)]
    pub search_exclude: String,
    #[serde(default)]
    pub expanded: Vec<PathBuf>,
    #[serde(default)]
    pub files: Vec<SessionFile>,
    /// Files opened lately, most recent first (go to file shows them first).
    #[serde(default)]
    pub recent: Vec<PathBuf>,
    /// Breakpoints of the debugger.
    #[serde(default)]
    pub breakpoints: Vec<SessionBreakpoint>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionBreakpoint {
    pub path: PathBuf,
    /// 0-based.
    pub line: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub condition: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SessionFile {
    /// Empty for an untitled document.
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    /// Unsaved content kept at exit: file name in `backup_dir` (hot exit).
    #[serde(default)]
    pub backup: Option<String>,
    /// Bookmarked lines.
    #[serde(default)]
    pub bookmarks: Vec<usize>,
    /// A view tab instead of a file: "claude:<conversation>", "docker", "pulls", "debug",
    /// "search", "help", "welcome".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
}

/// State remembered across projects and restarts.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct UiState {
    #[serde(default)]
    pub last_include: String,
    #[serde(default)]
    pub last_exclude: String,
    #[serde(default)]
    pub search_history: Vec<String>,
    #[serde(default)]
    pub include_history: Vec<String>,
    #[serde(default)]
    pub exclude_history: Vec<String>,
}

impl UiState {
    /// Most recent first, without duplicates, bounded.
    pub fn remember(list: &mut Vec<String>, value: &str) {
        let v = value.trim();
        if v.is_empty() {
            return;
        }
        list.retain(|x| x != v);
        list.insert(0, v.to_string());
        list.truncate(30);
    }
}

fn state_path() -> PathBuf {
    config::config_dir().join("state.toml")
}

pub fn load_state() -> UiState {
    std::fs::read_to_string(state_path()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_state(s: &UiState) {
    if let Ok(text) = toml::to_string(s) {
        let _ = std::fs::create_dir_all(config::config_dir());
        let _ = std::fs::write(state_path(), text);
    }
}

fn list_path() -> PathBuf {
    config::config_dir().join("projects.toml")
}

pub fn load_list() -> Vec<Project> {
    std::fs::read_to_string(list_path())
        .ok()
        .and_then(|s| toml::from_str::<ProjectList>(&s).ok())
        .map(|l| l.projects)
        .unwrap_or_default()
}

pub fn save_list(list: &[Project]) {
    if let Ok(s) = toml::to_string(&ProjectList { projects: list.to_vec() }) {
        let _ = std::fs::create_dir_all(config::config_dir());
        let _ = std::fs::write(list_path(), s);
    }
}

/// Records that `path` was opened now (adds it if new) and keeps most recent first.
pub fn touch(list: &mut Vec<Project>, path: &Path) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    match list.iter_mut().find(|p| p.path == path) {
        Some(p) => p.last_opened = now,
        None => list.push(Project {
            name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string()),
            path: path.to_path_buf(),
            last_opened: now,
        }),
    }
    list.sort_by(|a, b| b.last_opened.cmp(&a.last_opened));
}

/// Stable FNV-1a hash (std's DefaultHasher may change between Rust releases).
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn session_path(root: &Path) -> PathBuf {
    config::config_dir().join("sessions").join(format!("{:016x}.toml", fnv(&root.to_string_lossy())))
}

/// Unsaved contents of the project's documents (hot exit).
pub fn backup_dir(root: &Path) -> PathBuf {
    config::config_dir().join("backups").join(format!("{:016x}", fnv(&root.to_string_lossy())))
}

/// Writes the backups of a session and removes the old ones.
pub fn save_backups(root: &Path, backups: &[(String, String)]) {
    let dir = backup_dir(root);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !backups.iter().any(|(n, _)| *n == name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    if backups.is_empty() {
        let _ = std::fs::remove_dir(&dir);
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    for (name, text) in backups {
        let _ = std::fs::write(dir.join(name), text);
    }
}

pub fn load_backup(root: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(backup_dir(root).join(name)).ok()
}

pub fn load_session(root: &Path) -> Option<Session> {
    toml::from_str(&std::fs::read_to_string(session_path(root)).ok()?).ok()
}

pub fn save_session(root: &Path, s: &Session) {
    let p = session_path(root);
    if let (Some(dir), Ok(text)) = (p.parent(), toml::to_string(s)) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(p, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_history() {
        let mut h = vec!["a".to_string(), "b".to_string()];
        UiState::remember(&mut h, "b");
        UiState::remember(&mut h, "  ");
        assert_eq!(h, vec!["b", "a"]);
    }

    #[test]
    fn session_roundtrip() {
        let s = Session {
            active: 1,
            search_include: "*.py".into(),
            files: vec![
                SessionFile { path: "/a/b.py".into(), line: 3, col: 4, backup: Some("doc0.txt".into()), bookmarks: vec![2, 7], view: None },
                SessionFile { path: PathBuf::new(), line: 0, col: 0, backup: None, bookmarks: Vec::new(), view: Some("claude:12".into()) },
            ],
            recent: vec!["/a/b.py".into()],
            breakpoints: vec![SessionBreakpoint { path: "/a/b.py".into(), line: 4, condition: "x > 1".into() }, SessionBreakpoint { path: "/a/b.py".into(), line: 9, condition: String::new() }],
            ..Default::default()
        };
        let back: Session = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.files[0].line, 3);
        assert_eq!(back.files[0].backup.as_deref(), Some("doc0.txt"));
        assert_eq!(back.files[0].bookmarks, [2, 7]);
        assert_eq!(back.recent.len(), 1);
        assert_eq!(back.breakpoints, s.breakpoints);
        assert_eq!((back.files[0].view.as_deref(), back.files[1].view.as_deref()), (None, Some("claude:12")));
        // sessions written before hot exit still load
        let old: Session = toml::from_str("active = 0\n[[files]]\npath = \"/x\"\nline = 1\ncol = 2\n").unwrap();
        assert!(old.files[0].backup.is_none() && old.files[0].bookmarks.is_empty() && old.recent.is_empty());
        assert_eq!(back.search_include, "*.py");
    }
}
