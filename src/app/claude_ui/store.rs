//! Conversations kept between sessions (their Claude Code session, to go on with it) and the
//! quotas of the plan, on disk.

use super::*;
use serde::{Deserialize, Serialize};

/// A conversation kept between sessions of Florm: its folder, its title, and the Claude Code
/// session to go on with. (The files of the chat of before have more: left aside when read.)
#[derive(Clone, Serialize, Deserialize)]
pub(in crate::app) struct SavedConv {
    pub(super) key: u64,
    pub(super) title: String,
    pub(super) dir: Option<PathBuf>,
    pub(super) session_id: Option<String>,
    pub(super) wish: Option<String>,
    pub(super) cost: f64,
    /// Last change, in unix seconds.
    pub(super) updated: i64,
    /// What was given to it (files, lines…).
    #[serde(default)]
    pub(super) given: Vec<String>,
    /// The Claude account of the conversation, and the one of its session.
    #[serde(default)]
    pub(super) account: String,
    #[serde(default)]
    pub(super) session_account: String,
    #[serde(default)]
    pub(super) account_chosen: bool,
}

/// Most conversations kept.
pub(super) const SAVED_MAX: usize = 50;

/// A conversation of the history, without what was said: the list read at the start (the
/// conversations themselves are read when opened).
#[derive(Clone, Serialize, Deserialize)]
pub(in crate::app) struct SavedMeta {
    pub(super) key: u64,
    pub(super) title: String,
    pub(super) dir: Option<PathBuf>,
    /// Last change, in unix seconds.
    pub(super) updated: i64,
    /// What its messages gave (files, lines…): « Reprendre la conversation de ce fichier ».
    #[serde(default)]
    pub(super) given: Vec<String>,
}

impl SavedMeta {
    pub(super) fn of(s: &SavedConv) -> SavedMeta {
        SavedMeta { key: s.key, title: s.title.clone(), dir: s.dir.clone(), updated: s.updated, given: s.given.clone() }
    }
}

/// The folder of the conversations: `list.json` and one `<key>.json` per conversation.
fn saved_dir() -> PathBuf {
    config::data_dir().join("claude")
}

fn conv_path(key: u64) -> PathBuf {
    saved_dir().join(format!("{key}.json"))
}

/// The conversations kept, newest first.
pub(in crate::app) fn load_saved() -> Vec<SavedMeta> {
    std::fs::read_to_string(saved_dir().join("list.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// The conversation `key` of the history.
pub(super) fn load_conv(key: u64) -> Option<SavedConv> {
    std::fs::read_to_string(conv_path(key)).ok().and_then(|t| serde_json::from_str(&t).ok())
}

/// Written aside then renamed: a crash never leaves half a file.
fn write_json(path: &Path, v: &impl Serialize) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string(v).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, path)).map_err(|e| e.to_string())
}

/// What the writing thread does, in the order asked.
enum Write {
    Conv(Box<SavedConv>),
    List(Vec<SavedMeta>),
    Remove(u64),
    /// Told back once what was asked before is written.
    Done(std::sync::mpsc::Sender<()>),
}

/// Waits (2 s at most) for the conversations being written: before Florm stops.
pub(in crate::app) fn wait_saved() {
    let (tx, rx) = std::sync::mpsc::channel();
    write_later(Write::Done(tx));
    let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
}

/// Writes the conversations out of the interface's thread, one after the other: a conversation
/// of tens of messages takes a moment to write, and an older state never comes after a newer one.
fn write_later(w: Write) {
    static WRITER: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<Write>>> = std::sync::OnceLock::new();
    let tx = WRITER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Write>();
        std::thread::spawn(move || {
            for w in rx {
                let r = match w {
                    Write::Conv(c) => write_json(&conv_path(c.key), &*c),
                    Write::List(l) => write_json(&saved_dir().join("list.json"), &l),
                    Write::Remove(key) => std::fs::remove_file(conv_path(key)).or(Ok(())),
                    Write::Done(tx) => {
                        let _ = tx.send(());
                        Ok(())
                    }
                };
                if let Err(e) = r {
                    eprintln!("florm: conversation Claude non enregistrée : {e}");
                }
            }
        });
        std::sync::Mutex::new(tx)
    });
    if let Ok(tx) = tx.lock() {
        let _ = tx.send(w);
    }
}

fn limits_path() -> PathBuf {
    config::data_dir().join("claude_limits.json")
}

type Limits = Vec<(String, f32, i64)>;

/// Use of the plan's quotas last known, of each account, kept between sessions: shown before
/// the first message. (Those of before the accounts: the first account's.)
pub(in crate::app) fn load_limits() -> HashMap<String, Limits> {
    let text = std::fs::read_to_string(limits_path()).unwrap_or_default();
    serde_json::from_str(&text)
        .ok()
        .or_else(|| serde_json::from_str::<Limits>(&text).ok().map(|l| HashMap::from([(String::new(), l)])))
        .unwrap_or_default()
}

pub(super) fn write_limits(limits: &HashMap<String, Limits>) {
    let p = limits_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(text) = serde_json::to_string(limits) {
        let _ = std::fs::write(p, text);
    }
}

pub(super) fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl App {
    /// The current conversation written to disk (when its session, title or account changes).
    pub(in crate::app) fn claude_save(&mut self) {
        let c = &self.claude;
        if c.is_empty() {
            return;
        }
        let key = match c.saved_key {
            0 => std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_millis() as u64),
            k => k,
        };
        let saved = SavedConv {
            key,
            title: conv_title(c),
            dir: c.dir.clone(),
            session_id: c.session_id.clone(),
            wish: c.wish.clone(),
            cost: c.cost,
            updated: now_secs(),
            given: c.given.clone(),
            account: c.account.clone(),
            session_account: c.session_account.clone(),
            account_chosen: c.account_chosen,
        };
        self.claude.saved_key = key;
        self.claude_saved.retain(|s| s.key != key);
        self.claude_saved.insert(0, SavedMeta::of(&saved));
        for old in self.claude_saved.split_off(self.claude_saved.len().min(SAVED_MAX)) {
            write_later(Write::Remove(old.key));
        }
        write_later(Write::Conv(Box::new(saved)));
        write_later(Write::List(self.claude_saved.clone()));
    }

    /// A saved conversation, ready to go on (shown in `tab`).
    pub(super) fn claude_from_saved(&self, s: &SavedConv, tab: Option<u64>) -> ClaudeUi {
        let mut c = self.claude_fresh(tab);
        c.saved_key = s.key;
        c.dir = s.dir.clone().or(c.dir);
        c.session_id = s.session_id.clone();
        c.wish = s.wish.clone().or_else(|| Some(s.title.clone()).filter(|t| !t.is_empty()));
        c.cost = s.cost;
        // a conversation of the chat of before: what it gave is in the list of the history
        c.given = if s.given.is_empty() { self.claude_saved.iter().find(|m| m.key == s.key).map(|m| m.given.clone()).unwrap_or_default() } else { s.given.clone() };
        (c.account, c.session_account, c.account_chosen) = (s.account.clone(), s.session_account.clone(), s.account_chosen);
        c
    }

    /// The conversations where Claude is at work (closing Florm would cut them).
    pub(in crate::app) fn claude_working(&self) -> usize {
        std::iter::once(&self.claude).chain(&self.claude_others).filter(|c| c.busy && c.term.is_some()).count()
    }

    /// « Florm se fermera quand Claude aura fini », with a button to stay.
    pub(in crate::app) fn quit_when_idle_ui(&mut self, ctx: &egui::Context) {
        let accent = self.hl.colors.accent;
        egui::Area::new(egui::Id::unique("quit_when_idle")).order(egui::Order::Foreground).anchor(Align2::RIGHT_BOTTOM, [-16.0, -36.0]).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    spinner(ui);
                    ui.label(RichText::new(tf!("Florm se fermera quand Claude aura fini ({})", "Florm will close when Claude is done ({})", self.claude_working())).color(accent));
                    if ui.button(t!("Ne pas fermer", "Don't close")).clicked() {
                        self.quit.when_idle = false;
                    }
                });
            });
        });
    }

    /// At the first showing of Claude after a start: the last conversation is back.
    pub(super) fn claude_restore_last(&mut self) {
        if std::mem::replace(&mut self.claude_loaded, true) {
            return;
        }
        if self.claude.is_empty() && self.claude.term.is_none() {
            if let Some(s) = self.claude_saved.first().and_then(|m| load_conv(m.key)) {
                let tab = self.claude.tab;
                self.claude = self.claude_from_saved(&s, tab);
            }
        }
    }

    /// The saved conversation shown in the tab `uid` (0: not saved yet), for the session.
    pub(in crate::app) fn claude_tab_key(&self, uid: u64) -> u64 {
        std::iter::once(&self.claude).chain(&self.claude_others).find(|c| c.tab == Some(uid)).map_or(0, |c| c.saved_key)
    }

    /// The tab of the conversation `key` at the start (0: a new conversation).
    pub(in crate::app) fn claude_restore_tab(&mut self, key: u64) {
        let saved = self.claude_saved.iter().any(|s| s.key == key).then(|| load_conv(key)).flatten();
        let taken = key != 0 && (self.claude.saved_key == key || self.claude_others.iter().any(|o| o.saved_key == key));
        if taken {
            return;
        }
        if self.claude.tab.is_none() && self.claude.is_empty() && self.claude.term.is_none() {
            // the first one is the current conversation (not the last one of the history)
            self.claude_loaded = true;
            if let Some(s) = &saved {
                self.claude = self.claude_from_saved(s, None);
            }
            if self.claude.dir.is_none() {
                self.claude.dir = self.repo.clone().or_else(|| self.root.clone());
            }
            let uid = self.new_claude_tab(conv_title(&self.claude));
            self.claude.tab = Some(uid);
        } else if saved.is_some() {
            self.claude_open_saved(key);
        }
    }

    /// A conversation of the history: shown where it is open, else in a new tab.
    pub(super) fn claude_open_saved(&mut self, key: u64) {
        if self.claude.saved_key == key {
            self.open_claude();
            return;
        }
        if let Some(id) = self.claude_others.iter().find(|o| o.saved_key == key).map(|o| o.id) {
            self.claude_go_to(id);
            return;
        }
        let Some(s) = self.claude_saved.iter().any(|s| s.key == key).then(|| load_conv(key)).flatten() else {
            self.set_error(t!("Cette conversation n'a pas pu être relue", "This conversation could not be read again"));
            return;
        };
        // one Claude tab: it comes in place of the current one, which goes on in the list
        if self.claude_in_open_tab(self.claude.tab) {
            let conv = self.claude_from_saved(&s, None);
            self.claude_in_place(conv);
            self.open_claude();
            return;
        }
        let uid = self.new_claude_tab(s.title.clone());
        let conv = self.claude_from_saved(&s, Some(uid));
        let old = std::mem::replace(&mut self.claude, conv);
        self.claude_others.push(old);
    }

    /// The conversations of the list of the Claude tab (their key in the history), kept with the
    /// session of the project.
    pub(in crate::app) fn claude_listed_keys(&self) -> Vec<u64> {
        std::iter::once(&self.claude).chain(&self.claude_others).filter(|c| c.tab == Some(LISTED) && c.saved_key != 0).map(|c| c.saved_key).collect()
    }

    /// The list of the Claude tab at the start: the conversations it had (read from the history,
    /// their `claude` starts again when shown).
    pub(in crate::app) fn claude_restore_listed(&mut self, keys: &[u64]) {
        for &key in keys {
            let taken = self.claude.saved_key == key || self.claude_others.iter().any(|o| o.saved_key == key);
            if taken || !self.claude_saved.iter().any(|s| s.key == key) {
                continue;
            }
            if let Some(s) = load_conv(key) {
                let conv = self.claude_from_saved(&s, Some(LISTED));
                self.claude_others.push(conv);
            }
        }
    }

    /// « Reprendre la conversation de ce fichier » : the most recent conversation that was given
    /// this file.
    pub(in crate::app) fn claude_resume_file(&mut self) {
        let Some(path) = self.claude_editor_doc().and_then(|i| self.docs[i].path.clone()) else {
            self.set_status(t!("Aucun fichier ouvert", "No file open"));
            return;
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if rel_path(self.claude.dir.as_deref(), &path).is_some_and(|rel| gives(&self.claude.given, &rel)) {
            self.open_claude();
            return;
        }
        let found = self.claude_saved.iter().find(|s| rel_path(s.dir.as_deref(), &path).is_some_and(|rel| gives(&s.given, &rel))).map(|s| s.key);
        match found {
            Some(key) => self.claude_open_saved(key),
            None => self.set_status(tf!("Aucune conversation Claude ne concerne {name}", "No Claude conversation is about {name}")),
        }
    }

    /// A conversation taken out of the history (not the open ones).
    pub(super) fn claude_forget_saved(&mut self, key: u64) {
        self.claude_saved.retain(|s| s.key != key);
        write_later(Write::Remove(key));
        write_later(Write::List(self.claude_saved.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The configuration of the test in a folder of its own (the user's conversations untouched).
    fn in_temp(name: &str) -> (std::sync::MutexGuard<'static, ()>, PathBuf) {
        let lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("florm_store_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FLORM_CONFIG", &dir);
        (lock, dir)
    }

    /// A conversation as the chat of before wrote it: what is left of it is read.
    const OLD_CONV: &str = r#"{"key":7,"title":"Ancienne","dir":"/p","model":1,"mode":0,"session_id":"s1","wish":null,"cost":0.5,"updated":100,
        "items":[{"User":["question",["a.py"]]},{"Text":"réponse"}],"queue":{"tasks":[]},"account":"Perso"}"#;

    #[test]
    fn old_conversations_still_load() {
        let c: SavedConv = serde_json::from_str(OLD_CONV).unwrap();
        assert_eq!((c.key, c.title.as_str(), c.session_id.as_deref(), c.account.as_str()), (7, "Ancienne", Some("s1"), "Perso"));
        assert!(c.given.is_empty() && !c.account_chosen);
        let m = SavedMeta::of(&c);
        assert_eq!((m.key, m.title.as_str(), m.updated), (7, "Ancienne", 100));
        // written again: only what is kept now
        let back = serde_json::to_string(&c).unwrap();
        assert!(!back.contains("items") && back.contains("\"session_id\":\"s1\""), "{back}");
    }

    /// The writes leave in the order asked: removing after writing leaves nothing.
    #[test]
    fn writes_keep_their_order() {
        let (_lock, dir) = in_temp("order");
        let c: SavedConv = serde_json::from_str(OLD_CONV).unwrap();
        write_later(Write::Conv(Box::new(c)));
        write_later(Write::List(Vec::new()));
        wait_saved();
        assert!(load_conv(7).is_some());
        write_later(Write::Remove(7));
        wait_saved();
        assert!(load_conv(7).is_none());
        assert!(load_saved().is_empty());
        std::env::remove_var("FLORM_CONFIG");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The quotas of before the accounts (a plain list) are those of the first account.
    #[test]
    fn limits_of_before_the_accounts() {
        let (_lock, dir) = in_temp("limits");
        std::fs::create_dir_all(config::data_dir()).unwrap();
        std::fs::write(limits_path(), r#"[["5 h",0.25,1000]]"#).unwrap();
        let l = load_limits();
        assert_eq!(l.get("").map(|v| v[0].0.as_str()), Some("5 h"));
        let mut both = l.clone();
        both.insert("pro".into(), vec![("7 j".into(), 0.5, 2000)]);
        write_limits(&both);
        assert_eq!(load_limits().len(), 2);
        std::fs::write(limits_path(), "pas du json").unwrap();
        assert!(load_limits().is_empty());
        std::env::remove_var("FLORM_CONFIG");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
