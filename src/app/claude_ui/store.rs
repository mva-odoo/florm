//! Conversations kept between sessions, the slash commands of Claude Code and the limits of
//! the plan, on disk.

use super::*;

/// A conversation kept between sessions of Florm: what was said, and the Claude Code session to
/// continue it with its context.
#[derive(Clone, Serialize, Deserialize)]
pub(in crate::app) struct SavedConv {
    pub(super) key: u64,
    pub(super) title: String,
    pub(super) dir: Option<PathBuf>,
    /// Index in `claude::MODELS` (conversations of before `model_value`).
    pub(super) model: usize,
    /// The --model value, and the --effort one.
    #[serde(default)]
    pub(super) model_value: Option<String>,
    #[serde(default)]
    pub(super) effort: String,
    pub(super) mode: usize,
    pub(super) session_id: Option<String>,
    pub(super) wish: Option<String>,
    pub(super) cost: f64,
    /// Last change, in unix seconds.
    pub(super) updated: i64,
    pub(super) items: Vec<Item>,
    #[serde(default)]
    pub(super) queue: Queue,
    /// The options of the messages (the menu Options), kept with the conversation.
    #[serde(default)]
    pub(super) options: Options,
    #[serde(default)]
    pub(super) checkpoints: Vec<Checkpoint>,
    #[serde(default)]
    pub(super) last_uuid: Option<String>,
    #[serde(default)]
    pub(super) add_dirs: Vec<PathBuf>,
    #[serde(default)]
    pub(super) worktree: Option<Worktree>,
    /// The Claude account of the conversation, and the one of its session.
    #[serde(default)]
    pub(super) account: String,
    #[serde(default)]
    pub(super) session_account: String,
    #[serde(default)]
    pub(super) account_chosen: bool,
    /// Florm was closed while Claude was at work in it: at the next start, offered to go on.
    #[serde(default)]
    pub(super) interrupted: bool,
}

/// The options of the messages: what goes with each one.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub(in crate::app) struct Options {
    pub(super) with_context: bool,
    pub(super) no_defs: bool,
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
        let mut given: Vec<String> = Vec::new();
        for i in &s.items {
            if let Item::User(_, g, ..) = i {
                for x in g {
                    if !given.contains(x) {
                        given.push(x.clone());
                    }
                }
            }
        }
        SavedMeta { key: s.key, title: s.title.clone(), dir: s.dir.clone(), updated: s.updated, given }
    }
}

/// The folder of the conversations: `list.json` and one `<key>.json` per conversation.
fn saved_dir() -> PathBuf {
    config::data_dir().join("claude")
}

fn conv_path(key: u64) -> PathBuf {
    saved_dir().join(format!("{key}.json"))
}

/// The conversations kept, newest first. The first time, those of the single file of before.
pub(in crate::app) fn load_saved() -> Vec<SavedMeta> {
    let list = saved_dir().join("list.json");
    if let Some(l) = std::fs::read_to_string(&list).ok().and_then(|t| serde_json::from_str(&t).ok()) {
        return l;
    }
    let old = config::data_dir().join("claude_conversations.json");
    let Some(convs) = std::fs::read_to_string(&old).ok().and_then(|t| serde_json::from_str::<Vec<SavedConv>>(&t).ok()) else { return Vec::new() };
    let metas: Vec<SavedMeta> = convs.iter().map(SavedMeta::of).collect();
    let written = convs.iter().all(|c| write_json(&conv_path(c.key), c).is_ok()) && write_json(&list, &metas).is_ok();
    // kept aside rather than removed, in case
    if written {
        let _ = std::fs::rename(&old, old.with_extension("json.old"));
    }
    metas
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

/// Slash commands of Claude Code, as its last session listed them.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub(in crate::app) struct Commands {
    /// Sent as a message (`/usage`, `/compact`, the skills…).
    pub(super) message: Vec<String>,
    /// Of its terminal interface only (`/doctor`…): run in a terminal.
    pub(super) terminal: Vec<String>,
    /// Version of Claude Code.
    pub(super) version: String,
    /// The models it offers, with their effort levels (`claude::list_models`).
    #[serde(default)]
    pub(in crate::app) models: Vec<claude::ModelInfo>,
}

/// The name of the model `value` (a --model value): the one Claude Code gives, else the alias of
/// before, else made from the value ("claude-opus-4-8" → "Opus 4.8").
pub(in crate::app) fn model_label(models: &[claude::ModelInfo], value: &str) -> String {
    if let Some(m) = models.iter().find(|m| m.value == value) {
        return if value.is_empty() {
            tf!("Défaut ({})", "Default ({})", claude::model_name(&m.resolved))
        } else {
            m.name.clone()
        };
    }
    match claude::MODELS.iter().find(|(v, _)| *v == value) {
        Some((_, label)) => claude::label(label).to_string(),
        None => claude::model_name(value),
    }
}

/// The models to choose from, (value, name): the ones that follow the latest version first, then
/// the pinned versions; the aliases of before when Claude Code has not given its list yet.
pub(in crate::app) fn model_choices(models: &[claude::ModelInfo]) -> Vec<(String, String)> {
    if models.is_empty() {
        return claude::MODELS.iter().map(|(v, l)| (v.to_string(), claude::label(l).to_string())).collect();
    }
    let latest = models.iter().filter(|m| m.is_latest());
    let pinned = models.iter().filter(|m| !m.is_latest());
    latest.chain(pinned).map(|m| (m.value.clone(), model_label(models, &m.value))).collect()
}

/// The effort levels the model `value` takes; all of them when it is not known.
pub(in crate::app) fn model_efforts(models: &[claude::ModelInfo], value: &str) -> Vec<String> {
    match models.iter().find(|m| m.value == value) {
        Some(m) => m.efforts.clone(),
        None => claude::EFFORTS.iter().map(|(v, _)| v.to_string()).collect(),
    }
}

pub(super) fn commands_path() -> PathBuf {
    config::data_dir().join("claude_commands.json")
}

pub(in crate::app) fn load_commands() -> Commands {
    std::fs::read_to_string(commands_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub(in crate::app) fn write_commands(c: &Commands) {
    let p = commands_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(text) = serde_json::to_string(c) {
        let _ = std::fs::write(p, text);
    }
}

/// What a slash command does, for the list shown while typing it (those Florm knows).
pub(super) fn command_hint(name: &str) -> &'static str {
    match name {
        "btw" => t!("Question à côté, sur la conversation, même pendant que Claude travaille ; elle n'y est pas ajoutée", "A question on the side, about the conversation, even while Claude is working; not added to it"),
        "status" => t!("Version, modèle, session, dossier, permissions", "Version, model, session, folder, permissions"),
        "usage" => t!("Quotas du forfait (5 h, semaine) et ce qui les consomme", "The plan's quotas (5 h, week) and what uses them"),
        "context" => t!("Ce qui remplit le contexte", "What fills the context"),
        "compact" => t!("Résumer la conversation pour libérer du contexte", "Summarize the conversation to free context"),
        "clear" => t!("Repartir sans la conversation", "Start again without the conversation"),
        "model" => t!("Modèle de cette session", "Model of this session"),
        "init" => t!("Écrire le CLAUDE.md du projet", "Write the CLAUDE.md of the project"),
        "effort" => t!("Effort de réflexion", "Thinking effort"),
        "review" | "code-review" => t!("Relire les changements", "Review the changes"),
        "security-review" => t!("Relire la sécurité des changements", "Review the security of the changes"),
        "rewind" => t!("Revenir avant le dernier message (fichiers et conversation) ; ↶ sur un message pour un autre", "Go back to before the last message (files and conversation); ↶ on a message for another one"),
        "mcp" => t!("Les serveurs MCP et leur état (⋯ pour en ajouter)", "The MCP servers and their state (⋯ to add one)"),
        "agents" => t!("Les sous-agents du projet et les tiens (⋯ pour en créer)", "The subagents of the project and yours (⋯ to create one)"),
        "permissions" => t!("Les permissions et les règles (🔒 pour les changer)", "The permissions and the rules (🔒 to change them)"),
        "memory" => t!("Ouvrir le CLAUDE.md du projet (# au début d'un message y ajoute une ligne)", "Open the CLAUDE.md of the project (# at the start of a message adds a line to it)"),
        "add-dir" => t!("/add-dir chemin : Claude peut aussi utiliser ce dossier", "/add-dir path: Claude may also use this folder"),
        _ => "",
    }
}

/// The slash commands for the list shown while typing one: (name, what it does). Before any
/// session told them, the usual ones.
pub(super) fn command_list(known: &Commands) -> Vec<(String, String)> {
    let usual = ["clear", "compact", "context", "init", "model", "usage"].map(String::from);
    let message: &[String] = if known.message.is_empty() { &usual } else { &known.message };
    let mut list: Vec<(String, String)> = ["btw", "status", "rewind", "mcp", "agents", "permissions", "memory", "add-dir"].iter().map(|n| (n.to_string(), command_hint(n).to_string())).collect();
    for n in message {
        if !list.iter().any(|(m, _)| m == n) {
            list.push((n.clone(), command_hint(n).to_string()));
        }
    }
    for n in &known.terminal {
        if !list.iter().any(|(m, _)| m == n) {
            list.push((n.clone(), t!("dans un terminal", "in a terminal").to_string()));
        }
    }
    list
}

pub(super) fn limits_path() -> PathBuf {
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
    /// The current conversation written to disk (after each message and each answer).
    pub(in crate::app) fn claude_save(&mut self) {
        let c = &self.claude;
        if !c.items.iter().any(|i| matches!(i, Item::User(..))) {
            return;
        }
        let key = match c.saved_key {
            0 => std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_millis() as u64),
            k => k,
        };
        let items = c.items.iter().map(|i| if let Item::Streaming(t) = i { Item::Text(t.clone()) } else { i.clone() }).collect();
        let saved = SavedConv {
            key,
            title: self.claude_title(),
            dir: c.dir.clone(),
            model: claude::MODELS.iter().position(|(v, _)| *v == c.model).unwrap_or(0),
            model_value: Some(c.model.clone()),
            effort: c.effort.clone(),
            mode: c.mode,
            session_id: c.session_id.clone(),
            wish: c.wish.clone(),
            cost: c.cost,
            updated: now_secs(),
            items,
            queue: c.queue.clone(),
            options: Options { with_context: c.with_context, no_defs: c.no_defs },
            checkpoints: c.checkpoints.clone(),
            last_uuid: c.last_uuid.clone(),
            add_dirs: c.add_dirs.clone(),
            worktree: c.worktree.clone(),
            account: c.account.clone(),
            session_account: c.session_account.clone(),
            account_chosen: c.account_chosen,
            interrupted: c.interrupted,
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
        c.model = s.model_value.clone().unwrap_or_else(|| claude::MODELS[s.model.min(claude::MODELS.len() - 1)].0.to_string());
        c.effort = s.effort.clone();
        c.mode = s.mode.min(claude::MODES.len() - 1);
        c.session_id = s.session_id.clone();
        c.wish = s.wish.clone();
        c.cost = s.cost;
        c.items = s.items.clone();
        c.queue = s.queue.clone();
        c.queue.settle();
        c.checkpoints = s.checkpoints.clone();
        c.last_uuid = s.last_uuid.clone();
        c.add_dirs = s.add_dirs.clone();
        c.worktree = s.worktree.clone();
        (c.account, c.session_account, c.account_chosen) = (s.account.clone(), s.session_account.clone(), s.account_chosen);
        c.interrupted = s.interrupted;
        (c.with_context, c.no_defs) = (s.options.with_context, s.options.no_defs);
        c.items.push(Item::Info(t!("— conversation reprise : le prochain message continue avec son contexte —", "— conversation resumed: the next message goes on with its context —").into()));
        c
    }

    /// The conversations where Claude is at work (closing Florm would cut them).
    pub(in crate::app) fn claude_working(&self) -> usize {
        std::iter::once(&self.claude).chain(&self.claude_others).filter(|c| c.busy && c.session.is_some()).count()
    }

    /// Florm closes: the conversations where Claude was at work are kept as cut, so that the
    /// next start offers to go on.
    pub(in crate::app) fn claude_mark_interrupted(&mut self) {
        let working = |c: &ClaudeUi| c.busy && c.session.is_some();
        if working(&self.claude) {
            self.claude.interrupted = true;
            self.claude_save();
        }
        for k in 0..self.claude_others.len() {
            if working(&self.claude_others[k]) {
                std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
                self.claude.interrupted = true;
                self.claude_save();
                std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
            }
        }
    }

    /// « Florm se fermera quand Claude aura fini », with a button to stay.
    pub(in crate::app) fn quit_when_idle_ui(&mut self, ctx: &egui::Context) {
        let accent = self.hl.colors.accent;
        egui::Area::new(egui::Id::new("quit_when_idle")).order(egui::Order::Foreground).anchor(Align2::RIGHT_BOTTOM, [-16.0, -36.0]).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    spinner(ui);
                    ui.label(RichText::new(tf!("Florm se fermera quand Claude aura fini ({})", "Florm will close when Claude is done ({})", self.claude_working())).color(accent));
                    if ui.button(t!("Ne pas fermer", "Don't close")).clicked() {
                        self.quit_when_idle = false;
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
        if self.claude.items.is_empty() && self.claude.session.is_none() {
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
        if self.claude.tab.is_none() && self.claude.items.is_empty() && self.claude.session.is_none() {
            // the first one is the current conversation (not the last one of the history)
            self.claude_loaded = true;
            if let Some(s) = &saved {
                self.claude = self.claude_from_saved(s, None);
            }
            if self.claude.dir.is_none() {
                self.claude.dir = self.repo.clone().or_else(|| self.root.clone());
            }
            let uid = self.new_claude_tab(self.claude_title());
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
        if self.claude_one_tab_at(self.claude.tab) {
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
    /// their process starts again with the next message).
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

    /// « Reprendre la conversation de ce fichier » : the most recent conversation that gave this
    /// file to Claude (attached, or open in the editor when a message was sent).
    pub(in crate::app) fn claude_resume_file(&mut self) {
        let Some(path) = self.claude_editor_doc().and_then(|i| self.docs[i].path.clone()) else {
            self.set_status(t!("Aucun fichier ouvert", "No file open"));
            return;
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if rel_path(self.claude.dir.as_deref(), &path).is_some_and(|rel| gives_file(&self.claude.items, &rel)) {
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

    /// A conversation as an older Florm wrote it: only the fields it had then.
    const OLD_CONV: &str = r#"{"key":7,"title":"Ancienne","dir":"/p","model":1,"mode":0,"session_id":"s1","wish":null,"cost":0.5,"updated":100,
        "items":[{"User":["question",["a.py"]]},{"Text":"réponse"},{"User":["encore",["a.py","b.py"]]}]}"#;

    #[test]
    fn old_conversations_still_load() {
        let c: SavedConv = serde_json::from_str(OLD_CONV).unwrap();
        assert_eq!((c.key, c.model, c.model_value.as_deref()), (7, 1, None));
        assert!(c.effort.is_empty() && c.checkpoints.is_empty() && c.account.is_empty() && !c.interrupted);
        assert!(matches!(&c.items[0], Item::User(t, given, texts, images) if t == "question" && given == &["a.py"] && texts.is_empty() && images.is_empty()));
    }

    #[test]
    fn meta_lists_what_was_given_once() {
        let c: SavedConv = serde_json::from_str(OLD_CONV).unwrap();
        let m = SavedMeta::of(&c);
        assert_eq!((m.key, m.title.as_str(), m.updated), (7, "Ancienne", 100));
        assert_eq!(m.given, ["a.py", "b.py"]);
    }

    /// The single file of before becomes one file per conversation and a list, once; the old
    /// file is kept aside.
    #[test]
    fn single_file_of_before_is_split() {
        let (_lock, dir) = in_temp("split");
        let data = config::data_dir();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("claude_conversations.json"), format!("[{OLD_CONV}]")).unwrap();
        let metas = load_saved();
        assert_eq!(metas.len(), 1);
        assert!(data.join("claude_conversations.json.old").is_file() && !data.join("claude_conversations.json").exists());
        assert_eq!(load_conv(7).map(|c| c.title), Some("Ancienne".to_string()));
        // read from the list now, and no file left half written
        assert_eq!(load_saved().len(), 1);
        let left: Vec<_> = std::fs::read_dir(saved_dir()).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "tmp")).collect();
        assert!(left.is_empty());
        assert!(load_conv(8).is_none());
        std::env::remove_var("FLORM_CONFIG");
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn commands_listed_once() {
        // before any session: Florm's own, then the usual ones
        let list = command_list(&Commands::default());
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(&names[..2], ["btw", "status"]);
        assert!(names.contains(&"compact") && names.contains(&"usage"));
        // told by a session: no name twice, those of the terminal said so
        let known = Commands { message: vec!["compact".into(), "btw".into(), "mon-skill".into()], terminal: vec!["doctor".into(), "status".into()], ..Default::default() };
        let list = command_list(&known);
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
        assert!(names.contains(&"mon-skill") && names.contains(&"doctor") && !names.contains(&"usage"));
        assert!(list.iter().any(|(n, hint)| n == "doctor" && !hint.is_empty()));
    }

    #[test]
    fn models_named_and_ordered() {
        let m = |value: &str, name: &str, efforts: &[&str]| claude::ModelInfo { value: value.into(), name: name.into(), efforts: efforts.iter().map(|e| e.to_string()).collect(), ..Default::default() };
        let models = vec![m("claude-opus-5-1", "Opus 5.1", &[]), m("opus", "Opus", &["low", "max"]), m("sonnet", "Sonnet", &[])];
        // the latest ones first, then the pinned versions
        let values: Vec<String> = model_choices(&models).into_iter().map(|(v, _)| v).collect();
        assert_eq!(values, ["opus", "sonnet", "claude-opus-5-1"]);
        assert_eq!(model_label(&models, "opus"), "Opus");
        // not offered: made from its value
        assert_eq!(model_label(&models, "claude-haiku-5-5"), claude::model_name("claude-haiku-5-5"));
        assert_eq!(model_efforts(&models, "opus"), ["low", "max"]);
        assert_eq!(model_efforts(&models, "inconnu").len(), claude::EFFORTS.len());
        // Claude Code has not told its models yet: the aliases of before
        assert_eq!(model_choices(&[]).len(), claude::MODELS.len());
    }
}
