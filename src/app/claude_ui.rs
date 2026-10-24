//! The Claude tab: a conversation with Claude Code on the left, and on the right the review of
//! what it changed in the files since the review started, block by block (kept or rejected).
//!
//! The review compares the files with a private index (`.git/florm-review-index`) holding them as
//! they were: keeping a block adds it to that index, rejecting it takes it out of the file.

use super::*;
use crate::claude::{self, Event};
use serde::{Deserialize, Serialize};
use history::{DiffAct, DiffClick};
use super::claude_queue::{self, Queue, QueueAct, TaskState};

/// Something given to Claude with the next message: lines of a file, a file / folder, or a text
/// (the changes not committed, the problems of a file…).
#[derive(Clone)]
pub(super) struct Attach {
    path: PathBuf,
    /// Lines (1-based, first and last) and their text; None: the whole file or folder.
    lines: Option<(usize, usize, String)>,
    /// A text and what it is (instead of a file).
    note: Option<(String, String)>,
}

impl Attach {
    fn label(&self, dir: Option<&Path>) -> String {
        if let Some((label, _)) = &self.note {
            return label.clone();
        }
        let rel = dir.and_then(|d| self.path.strip_prefix(d).ok()).unwrap_or(&self.path).display().to_string();
        match &self.lines {
            Some((a, b, _)) if a == b => format!("{rel}:{a}"),
            Some((a, b, _)) => format!("{rel}:{a}-{b}"),
            None => rel,
        }
    }

    /// How it is said to Claude.
    fn prompt(&self, dir: Option<&Path>) -> String {
        let label = self.label(dir);
        if let Some((_, text)) = &self.note {
            return format!("{label} :\n```\n{text}\n```");
        }
        match &self.lines {
            Some((_, _, text)) => {
                let lang = self.path.extension().and_then(|e| e.to_str()).unwrap_or("");
                tf!("Extrait de {label} :\n```{lang}\n{text}\n```", "Excerpt of {label}:\n```{lang}\n{text}\n```")
            }
            None if self.path.is_dir() => tf!("Dossier à prendre en compte : {label}", "Folder to take into account: {label}"),
            None => tf!("Fichier à prendre en compte : {label}", "File to take into account: {label}"),
        }
    }
}

/// Most characters of a diff given to Claude as context (the rest: Claude reads it with git itself).
const MAX_DIFF_CHARS: usize = 20_000;

/// What can be added to the context from the chat (« + Contexte »).
#[derive(Clone)]
enum CtxAdd {
    /// A commit (its hash): its message and its diff.
    Commit(String),
    File,
    Selection,
    Tabs,
    Diff,
    Problems,
}

/// A line of the conversation.
#[derive(Clone, Serialize, Deserialize)]
enum Item {
    /// A message, with what was given with it.
    User(String, Vec<String>),
    /// The answer, being written (pieces) or whole.
    Text(String),
    Streaming(String),
    Tool { id: String, name: String, summary: String, result: Option<(String, bool)> },
    Info(String),
    Error(String),
    /// What the last answer cost: tokens in (`cached` of them read from the cache), out, dollars.
    Usage { input: u64, cached: u64, output: u64, cost: f64 },
}

#[derive(Default)]
pub(super) struct ClaudeUi {
    /// Number of the conversation (0: the first one), and the editor tab showing it (document
    /// uid; None: the bottom panel).
    id: u64,
    tab: Option<u64>,
    /// List items of the answers ticked (or unticked) by a click: (answer, item's text). Not kept
    /// with the conversation.
    checks: HashSet<(usize, String)>,
    /// It finished answering while it was not shown: its tab is colored until seen (and blinks
    /// a few seconds after `done_at`).
    unseen: bool,
    done_at: Option<Instant>,
    /// Key of the conversation in the saved ones (0: not saved yet).
    saved_key: u64,
    /// Session of Claude Code, continued (`--resume`) when the process is started again (after
    /// a restart of Florm), and whether the running process confirmed it.
    session_id: Option<String>,
    started: bool,
    /// Folder of the conversation (the selected repository, else the project).
    pub(super) dir: Option<PathBuf>,
    session: Option<claude::Session>,
    items: Vec<Item>,
    input: String,
    busy: bool,
    model: usize,
    mode: usize,
    cost: f64,
    /// Tokens of the conversation read by the model for its last message, and its window.
    context: (u64, u64),
    /// The file being edited and its selection are given with the message.
    with_context: bool,
    /// Lines and files given with the next message.
    pub(super) attach: Vec<Attach>,
    /// First message of the conversation: the wish behind the changes (for the commit message).
    pub(super) wish: Option<String>,
    /// The review: its index and the changes since.
    review_index: Option<PathBuf>,
    review: Option<HistoryDiff>,
    review_loading: bool,
    collapsed: HashSet<usize>,
    jump: Option<history::FileJump>,
    /// "Tout rejeter" clicked once: a second click does it.
    confirm_reject: bool,
    pub(super) focus: bool,
    /// The to-do list: tasks sent one by one.
    queue: Queue,
    /// The definitions of the names cited are not given with the messages.
    no_defs: bool,
    /// Claude asks its questions (with options) before coding when the request leaves choices open.
    ask_first: bool,
    /// The choices made for the questions of the last answer.
    answers: Option<claude_ask::Answers>,
    /// Last commits of its folder for « + Contexte → Un commit »: (hash, short hash, subject, when).
    recent_commits: Vec<(String, String, String, String)>,
    /// Height of each item when it was last drawn, with the key of what it showed (`item_key`),
    /// and whether it was the same the time before (only then is the item left undrawn out of sight).
    heights: Vec<(u64, f32, bool)>,
    /// `/btw` or `/status`: what was asked and the answer (None: on its way), shown above the
    /// field until closed; not part of the conversation.
    side: Option<(String, Option<Result<String, String>>)>,
}

/// What the height of a message depends on: its content, its place and the width.
fn item_key(it: &Item, claude_said: bool, busy: bool, last_turn: bool, width: f32, ppp: f32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::mem::discriminant(it).hash(&mut h);
    match it {
        Item::User(t, given) => (t, given).hash(&mut h),
        Item::Text(t) | Item::Streaming(t) | Item::Info(t) | Item::Error(t) => t.hash(&mut h),
        Item::Tool { name, summary, result, .. } => (name, summary, result).hash(&mut h),
        Item::Usage { input, cached, output, cost } => (input, cached, output, cost.to_bits()).hash(&mut h),
    }
    (claude_said, busy, last_turn, width.to_bits(), ppp.to_bits()).hash(&mut h);
    h.finish()
}

/// A conversation kept between sessions of Florm: what was said, and the Claude Code session to
/// continue it with its context.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct SavedConv {
    key: u64,
    title: String,
    dir: Option<PathBuf>,
    model: usize,
    mode: usize,
    session_id: Option<String>,
    wish: Option<String>,
    cost: f64,
    /// Last change, in unix seconds.
    updated: i64,
    items: Vec<Item>,
    #[serde(default)]
    queue: Queue,
}

/// Most conversations kept.
const SAVED_MAX: usize = 50;

fn saved_path() -> PathBuf {
    config::data_dir().join("claude_conversations.json")
}

/// The saved conversations, newest first.
pub(super) fn load_saved() -> Vec<SavedConv> {
    std::fs::read_to_string(saved_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_saved(list: &[SavedConv]) -> Result<(), String> {
    let p = saved_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let text = serde_json::to_string(list).map_err(|e| e.to_string())?;
    // written aside then renamed: a crash never leaves half a file
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &p)).map_err(|e| e.to_string())
}

/// Slash commands of Claude Code, as its last session listed them.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Commands {
    /// Sent as a message (`/usage`, `/compact`, the skills…).
    message: Vec<String>,
    /// Of its terminal interface only (`/doctor`…): run in a terminal.
    terminal: Vec<String>,
    /// Version of Claude Code.
    version: String,
}

fn commands_path() -> PathBuf {
    config::data_dir().join("claude_commands.json")
}

pub(super) fn load_commands() -> Commands {
    std::fs::read_to_string(commands_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_commands(c: &Commands) {
    let p = commands_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(text) = serde_json::to_string(c) {
        let _ = std::fs::write(p, text);
    }
}

/// What a slash command does, for the list shown while typing it (those Florm knows).
fn command_hint(name: &str) -> &'static str {
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
        _ => "",
    }
}

/// The slash commands for the list shown while typing one: (name, what it does). Before any
/// session told them, the usual ones.
fn command_list(known: &Commands) -> Vec<(String, String)> {
    let usual = ["clear", "compact", "context", "init", "model", "usage"].map(String::from);
    let message: &[String] = if known.message.is_empty() { &usual } else { &known.message };
    let mut list: Vec<(String, String)> = ["btw", "status"].iter().map(|n| (n.to_string(), command_hint(n).to_string())).collect();
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

fn limits_path() -> PathBuf {
    config::data_dir().join("claude_limits.json")
}

/// Use of the plan's quotas last known, kept between sessions: shown before the first message.
pub(super) fn load_limits() -> Vec<(String, f32, i64)> {
    std::fs::read_to_string(limits_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_limits(limits: &[(String, f32, i64)]) {
    let p = limits_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(text) = serde_json::to_string(limits) {
        let _ = std::fs::write(p, text);
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Review index of a repository for conversation `conv` (inside .git: not seen by git status,
/// not committed); each conversation has its own.
fn review_index_of(repo: &Path, conv: u64) -> PathBuf {
    let name = if conv == 0 { "florm-review-index".to_string() } else { format!("florm-review-index-{conv}") };
    let p = git::stdout(repo, &["rev-parse", "--git-path", &name]).unwrap_or_default();
    let p = PathBuf::from(p.trim());
    if p.is_absolute() {
        p
    } else {
        repo.join(p)
    }
}

/// git with the review index instead of the real one.
fn rgit(repo: &Path, index: &Path, args: &[&str], input: Option<&str>) -> Result<String, String> {
    git::run_with(repo, args, &[("GIT_INDEX_FILE", &index.to_string_lossy())], input)
}

/// The changes since the review started: modified files, then new ones.
fn review_diff(repo: &Path, index: &Path) -> Result<String, String> {
    let mut text = rgit(repo, index, &["-c", "core.quotePath=false", "diff"], None)?;
    let others = rgit(repo, index, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"], None).unwrap_or_default();
    for p in others.lines().filter(|p| !p.is_empty()).take(200) {
        if std::fs::metadata(repo.join(p)).is_ok_and(|m| m.len() > 2_000_000) {
            continue;
        }
        let d = match git::run(repo, &["diff", "--no-index", "--", "/dev/null", p]) {
            Err(e) if e.starts_with("diff --git") => e,
            Ok(o) => o,
            Err(_) => continue,
        };
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&d);
    }
    Ok(text)
}

impl App {
    /// Shows Claude in the bottom panel, or in an editor tab (`claude_in_tab`).
    pub(super) fn open_claude(&mut self) {
        if self.settings.claude_in_tab {
            if self.bottom == Some(BottomTab::Claude) {
                self.bottom = None;
            }
            // the tab of the current conversation, else the first one (or a new one)
            match self.claude.tab.and_then(|u| self.docs.iter().position(|d| d.uid == u)) {
                Some(i) => self.activate_tab(i),
                None => self.open_view_tab(document::View::Claude, "Claude".into()),
            }
        } else {
            self.bottom = Some(BottomTab::Claude);
        }
        self.claude_prepare();
    }

    /// Shows Claude with `text` written in its field (not sent: to read and send), in `dir`
    /// when no conversation runs elsewhere.
    pub(super) fn claude_prefill(&mut self, dir: PathBuf, text: String) {
        if self.claude.session.is_none() && self.claude.dir.as_ref() != Some(&dir) {
            self.claude.dir = Some(dir);
            self.claude.review_index = None;
            self.claude.review = None;
        }
        if !self.claude.input.trim().is_empty() {
            self.claude.input.push_str("\n\n");
        }
        self.claude.input.push_str(&text);
        self.open_claude();
    }

    /// Its folder, the keyboard and the review, wherever Claude is shown.
    pub(super) fn claude_prepare(&mut self) {
        self.claude_restore_last();
        if self.claude.dir.is_none() {
            self.claude.dir = self.repo.clone().or_else(|| self.root.clone());
        }
        self.claude.focus = true;
        self.claude_reload_review();
    }

    /// What Claude sent since the last frame.
    pub(super) fn claude_poll(&mut self) {
        let Some(s) = &self.claude.session else { return };
        let events = s.poll();
        if events.is_empty() {
            return;
        }
        let (mut reload, mut save) = (false, false);
        // the answer ended: the task of the queue being done is finished (or failed)
        let mut task_done: Option<Result<(), String>> = None;
        for e in events {
            let items = &mut self.claude.items;
            match e {
                // its session: continued after a restart of Florm
                Event::Init { session, version, commands, terminal, .. } => {
                    if !session.is_empty() {
                        self.claude.session_id = Some(session);
                    }
                    self.claude.started = true;
                    let known = Commands { message: commands, terminal, version };
                    if !known.message.is_empty() && known != self.claude_commands {
                        write_commands(&known);
                        self.claude_commands = known;
                    }
                }
                Event::Session(id) => self.claude.session_id = Some(id),
                Event::Reset => {
                    items.clear();
                    items.push(Item::Info(t!("— /clear : Claude repart sans la conversation —", "— /clear: Claude starts again without the conversation —").into()));
                    self.claude.context.0 = 0;
                    self.claude.wish = None;
                    save = true;
                }
                Event::Delta(t) => match items.last_mut() {
                    Some(Item::Streaming(s)) => s.push_str(&t),
                    _ => items.push(Item::Streaming(t)),
                },
                Event::Text(t) => match items.last_mut() {
                    Some(it @ Item::Streaming(_)) => *it = Item::Text(t),
                    _ => items.push(Item::Text(t)),
                },
                Event::Tool { id, name, summary } => {
                    if let Some(Item::Streaming(s)) = items.last() {
                        let s = s.clone();
                        *items.last_mut().unwrap() = Item::Text(s);
                    }
                    items.push(Item::Tool { id, name, summary, result: None });
                }
                Event::ToolResult { id, text, error } => {
                    for it in items.iter_mut().rev() {
                        if let Item::Tool { id: i, name, result, .. } = it {
                            if *i == id {
                                reload |= claude::changes_files(name);
                                *result = Some((text, error));
                                break;
                            }
                        }
                    }
                }
                Event::Context(tokens) => self.claude.context.0 = tokens,
                Event::Limits(l) => {
                    if l != self.claude_limits {
                        write_limits(&l);
                        self.claude_limits = l;
                    }
                }
                Event::Done { error, text, cost, window, tokens } => {
                    self.claude.busy = false;
                    self.claude.unseen = true;
                    // a real answer (not a /usage…): its tab blinks, the sound chosen plays
                    if error || tokens.input + tokens.output > 0 {
                        self.claude.done_at = Some(Instant::now());
                        crate::sound::play(&self.settings.claude_done_sound);
                    }
                    self.claude.cost += cost;
                    if window > 0 {
                        self.claude.context.1 = window;
                    }
                    if let Some(Item::Streaming(s)) = items.last() {
                        let s = s.clone();
                        *items.last_mut().unwrap() = Item::Text(s);
                    }
                    if tokens.input + tokens.output > 0 {
                        items.push(Item::Usage { input: tokens.input, cached: tokens.cached, output: tokens.output, cost });
                    }
                    if error && !text.is_empty() {
                        items.push(Item::Error(text.clone()));
                    }
                    reload = true;
                    save = true;
                    task_done = Some(if error { Err(if text.is_empty() { t!("Claude s'est arrêté sur une erreur", "Claude stopped on an error").to_string() } else { text }) } else { Ok(()) });
                }
                Event::Exited(err) => {
                    self.claude.busy = false;
                    self.claude.unseen = true;
                    self.claude.session = None;
                    if !self.claude.started && self.claude.session_id.is_some() {
                        // the session to continue could not be found: the next message starts afresh
                        self.claude.session_id = None;
                        items.push(Item::Info(t!("La session d'avant n'a pas pu être reprise : le prochain message repart sans son contexte.", "The previous session could not be resumed: the next message starts without its context.").into()));
                    }
                    let msg = match (err.is_empty(), self.claude.session_id.is_some()) {
                        (false, _) => err,
                        (true, true) => t!("Claude s'est arrêté : le prochain message reprend la conversation.", "Claude stopped: the next message resumes the conversation.").to_string(),
                        (true, false) => t!("Claude s'est arrêté : un nouveau message démarre une nouvelle conversation.", "Claude stopped: a new message starts a new conversation.").to_string(),
                    };
                    items.push(Item::Error(msg.clone()));
                    save = true;
                    task_done = Some(Err(msg));
                    break;
                }
            }
        }
        if let Some(r) = task_done {
            self.claude_task_finished(r);
        }
        if save {
            self.claude_save();
        }
        if reload {
            self.claude_reload_review();
            // the files changed: read again at once
            self.last_disk_check = Instant::now() - Duration::from_secs(10);
            self.git_refresh();
        }
    }

    /// The message written in the field of the chat.
    fn claude_send(&mut self) {
        let text = self.claude.input.trim().to_string();
        if let Some(clear) = text.starts_with('/').then(|| self.claude_command(&text)).flatten() {
            if clear {
                self.claude.input.clear();
            }
            return;
        }
        let ask = self.claude.ask_first;
        if self.claude_send_text(text, ask) {
            self.claude.input.clear();
        }
    }

    /// The slash commands Florm answers itself (`/btw`, `/status`) or runs in a terminal (those
    /// of the terminal interface of Claude Code), and those that wait for Claude to be free.
    /// None: to send as a message; else whether the field is emptied.
    fn claude_command(&mut self, text: &str) -> Option<bool> {
        let (name, rest) = text[1..].split_once(char::is_whitespace).map_or((&text[1..], ""), |(n, r)| (n, r.trim()));
        let dir = self.claude.dir.clone()?;
        let c = &self.claude;
        match name {
            "btw" if rest.is_empty() => {
                self.set_status(t!("/btw suivi de la question", "/btw followed by the question"));
                Some(false)
            }
            "btw" => {
                let (session, model, id, question) = (c.session_id.clone(), claude::MODELS[c.model].0.to_string(), c.id, rest.to_string());
                self.claude.side = Some((format!("/btw {rest}"), None));
                self.jobs.spawn(move || JobResult::ClaudeBtw(id, claude::btw(&dir, session.as_deref(), &model, &question)));
                Some(true)
            }
            "status" => {
                let version = &self.claude_commands.version;
                let line = |k: &str, v: &str| format!("- **{k}** : {v}\n");
                let mut s = String::new();
                s += &line("Claude Code", if version.is_empty() { "?" } else { version });
                s += &line(t!("Modèle", "Model"), claude::MODELS[c.model].1);
                s += &line(t!("Mode", "Mode"), claude::MODES[c.mode].1);
                s += &line(t!("Permissions", "Permissions"), &self.settings.claude_perms.join(", "));
                s += &line(t!("Dossier", "Folder"), &dir.display().to_string());
                s += &line("Session", c.session_id.as_deref().unwrap_or(t!("pas encore commencée", "not started yet")));
                s += &line(t!("Processus", "Process"), if c.session.is_some() { t!("en cours", "running") } else { t!("arrêté (le prochain message le relance)", "stopped (the next message starts it)") });
                if c.context.1 > 0 {
                    s += &line(t!("Contexte", "Context"), &format!("{} / {} tokens", thousands(c.context.0), thousands(c.context.1)));
                }
                s += &line(t!("Coût de la conversation", "Cost of the conversation"), &format!("{:.2} $", c.cost));
                self.claude.side = Some(("/status".into(), Some(Ok(s))));
                Some(true)
            }
            _ if self.claude_commands.terminal.iter().any(|t| t == name) => {
                let ctx = self.egui_ctx.clone();
                match self.term.open_command_panel(&ctx, &["claude".to_string(), text.to_string()], Some(dir)) {
                    Ok(id) => {
                        self.term.set_name(id, format!("claude /{name}"));
                        self.bottom = Some(BottomTab::Terminal);
                    }
                    Err(e) => self.set_error(tf!("Terminal impossible à ouvrir : {e}", "Cannot open a terminal: {e}")),
                }
                Some(true)
            }
            _ if c.busy => {
                self.set_status(t!("Claude travaille : la commande partira quand il aura fini (/btw répond tout de suite)", "Claude is working: send the command once it is done (/btw answers at once)"));
                Some(false)
            }
            _ => None,
        }
    }

    /// The answer of a `/btw` of conversation `id`.
    pub(super) fn claude_btw_loaded(&mut self, id: u64, r: Result<String, String>) {
        let Some(c) = std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).find(|c| c.id == id) else { return };
        if let Some((_, answer @ None)) = &mut c.side {
            *answer = Some(r);
        }
    }

    /// Sends `text` (with the context given) to Claude, asking it for its questions first when
    /// `ask_first`; false when it could not be sent. A slash command (`/usage`…) goes alone: Claude
    /// Code reads it only at the start of the message.
    fn claude_send_text(&mut self, text: String, ask_first: bool) -> bool {
        let Some(dir) = self.claude.dir.clone() else {
            self.set_status(t!("Aucun dossier ouvert", "No folder open"));
            return false;
        };
        if text.is_empty() || self.claude.busy {
            return false;
        }
        let command = text.starts_with('/');
        // the review starts with the first message: everything changed since is Claude's
        if self.claude.review_index.is_none() && !command {
            self.claude_start_review();
        }
        let mut message = text.clone();
        // a command leaves the files given for the next message
        let attach = if command { Vec::new() } else { std::mem::take(&mut self.claude.attach) };
        let mut given: Vec<String> = attach.iter().map(|a| a.label(Some(&dir))).collect();
        if !attach.is_empty() {
            let parts: Vec<String> = attach.iter().map(|a| a.prompt(Some(&dir))).collect();
            message = format!("{}\n\n{message}", parts.join("\n\n"));
        }
        if self.claude.with_context && !command {
            if let Some((ctx, rel)) = self.editor_context(&dir) {
                message = format!("{ctx}\n\n{message}");
                // the file is given too: « reprendre la conversation de ce fichier » finds it
                if !given.contains(&rel) {
                    given.insert(0, rel);
                }
            }
        }
        // the classes and functions named in the message, found in the index: Claude does not
        // have to search for them
        if !self.claude.no_defs && !command {
            if let Some((defs, names)) = self.cited_definitions(&text, &dir) {
                message = format!("{message}\n\n{defs}");
                given.push(tf!("définitions : {}", "definitions: {}", names.join(", ")));
            }
        }
        if ask_first && !command {
            message = format!("{message}\n\n{}", claude_ask::instruction());
        }
        if self.claude.session.is_none() {
            let model = claude::MODELS[self.claude.model].0;
            let perms = claude::permission_args(self.claude.mode, &self.settings.claude_perms, self.settings.claude_budget);
            let ctx = self.egui_ctx.clone();
            // a conversation of before (restart of Florm, process stopped): continued with its context
            let resume = self.claude.session_id.clone();
            self.claude.started = false;
            match claude::Session::start(&dir, model, &perms, resume.as_deref(), move || ctx.request_repaint()) {
                Ok(s) => self.claude.session = Some(s),
                Err(e) => {
                    self.claude.items.push(Item::Error(e));
                    self.claude.attach = attach;
                    return false;
                }
            }
        }
        let Some(s) = self.claude.session.as_mut() else { return false };
        match s.send(&message) {
            Ok(()) => {
                self.claude.items.push(Item::User(text.clone(), given));
                let first = self.claude.wish.is_none() && !command;
                if !command {
                    self.claude.wish.get_or_insert(text);
                }
                self.claude.busy = true;
                // its tab is named after the first message
                if let (true, Some(uid)) = (first, self.claude.tab) {
                    let title = self.claude_title();
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                        d.title_override = Some(title);
                    }
                }
                self.claude_save();
                true
            }
            Err(e) => {
                self.claude.session = None;
                self.claude.items.push(Item::Error(e));
                self.claude.attach = attach;
                false
            }
        }
    }

    /// The task of the queue being done is finished: committed if asked, then the next one.
    fn claude_task_finished(&mut self, r: Result<(), String>) {
        let q = &mut self.claude.queue;
        let Some(k) = q.current().filter(|k| q.tasks[*k].state == TaskState::Running) else { return };
        // Claude asks questions: the task goes on once they are answered
        let asks = {
            let items = &self.claude.items;
            let last_user = items.iter().rposition(|i| matches!(i, Item::User(..))).unwrap_or(0);
            items[last_user..].iter().any(|i| matches!(i, Item::Text(t) if !claude_ask::parse(t).1.is_empty()))
        };
        match r {
            Err(e) => {
                q.tasks[k].state = TaskState::Failed;
                q.tasks[k].note = e;
                q.running = false;
                self.set_error(t!("File de tâches en pause : une tâche a échoué", "Task queue paused: a task failed"));
            }
            Ok(()) if asks => self.set_status(t!("Claude pose des questions : la file reprend après tes réponses", "Claude is asking questions: the queue resumes after your answers")),
            Ok(()) if q.commit_each => {
                q.tasks[k].state = TaskState::Committing;
                let (dir, wish, id) = (self.claude.dir.clone(), q.tasks[k].text.clone(), self.claude.id);
                let Some(dir) = dir else { return };
                self.jobs.spawn(move || JobResult::QueueCommit(id, claude_queue::commit_task(&dir, &wish)));
            }
            Ok(()) => {
                q.tasks[k].state = TaskState::Done;
                self.claude_queue_next();
            }
        }
    }

    /// The commit after a task of conversation `id` is made: the next task goes.
    pub(super) fn queue_commit_loaded(&mut self, id: u64, r: Result<String, String>) {
        let Some(k) = self.claude_others.iter().position(|c| c.id == id) else {
            if self.claude.id == id {
                self.queue_commit_done(r);
            }
            return;
        };
        // a conversation not shown: made the current one the time to send its next task
        std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
        self.queue_commit_done(r);
        std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
    }

    fn queue_commit_done(&mut self, r: Result<String, String>) {
        let q = &mut self.claude.queue;
        let Some(k) = q.current().filter(|k| q.tasks[*k].state == TaskState::Committing) else { return };
        match r {
            Ok(first) => {
                q.tasks[k].state = TaskState::Done;
                q.tasks[k].note = tf!("Commit : {first}", "Commit: {first}");
                self.claude.items.push(Item::Info(format!("— commit : {first} —")));
                self.git_refresh();
                self.claude_queue_next();
            }
            Err(e) => {
                q.tasks[k].state = TaskState::Failed;
                q.tasks[k].note = tf!("Le commit a échoué : {e}", "The commit failed: {e}");
                q.running = false;
                self.claude.items.push(Item::Error(tf!("Commit après la tâche : {e}", "Commit after the task: {e}")));
            }
        }
        self.claude_save();
    }

    /// The next waiting task, if the queue runs and Claude is free.
    fn claude_queue_next(&mut self) {
        let q = &self.claude.queue;
        if !q.running || self.claude.busy || q.current().is_some() {
            return;
        }
        let Some(k) = q.next_waiting() else {
            self.claude.queue.running = false;
            self.set_status(t!("File de tâches terminée", "Task queue done"));
            return;
        };
        let mut text = q.tasks[k].text.trim().to_string();
        let model = q.model_of(k);
        let previous = q.tasks[..k].iter().rev().find(|t| t.state == TaskState::Done).map(|t| t.text.lines().next().unwrap_or("").to_string());
        let fresh = q.fresh_each;
        let c = &mut self.claude;
        // a conversation of its own: the history is not sent again, only what was just done
        if fresh && c.items.iter().any(|i| matches!(i, Item::User(..))) {
            let last: Option<String> = c.items.iter().rev().find_map(|i| if let Item::Text(t) = i { Some(t.clone()) } else { None });
            c.session = None;
            c.session_id = None;
            c.context.0 = 0;
            c.items.push(Item::Info(t!("— conversation neuve pour cette tâche —", "— new conversation for this task —").into()));
            if let (Some(prev), Some(last)) = (previous, last) {
                let mut said: String = last.chars().take(1200).collect();
                if said.len() < last.len() {
                    said.push('…');
                }
                text = tf!("Pour info, la tâche d'avant (« {prev} ») vient d'être faite ; sa conclusion :\n{said}\n\nTâche :\n{text}", "For information, the previous task (“{prev}”) has just been done; its conclusion:\n{said}\n\nTask:\n{text}");
            }
        }
        // another model: the process starts again, the conversation goes on (--resume)
        if let Some(m) = model.filter(|m| *m != c.model && *m < claude::MODELS.len()) {
            c.model = m;
            c.session = None;
        }
        self.claude.queue.tasks[k].state = TaskState::Running;
        let ask = self.claude.ask_first;
        if !self.claude_send_text(text, ask) {
            let q = &mut self.claude.queue;
            q.tasks[k].state = TaskState::Failed;
            q.tasks[k].note = t!("Pas pu être envoyée", "Could not be sent").into();
            q.running = false;
        }
    }

    fn claude_queue_act(&mut self, act: QueueAct) {
        let q = &mut self.claude.queue;
        match act {
            QueueAct::Start => {
                q.running = true;
                q.base_model = Some(self.claude.model);
                self.claude_queue_next();
            }
            QueueAct::Pause => q.running = false,
            QueueAct::Remove(k) => {
                q.tasks.remove(k);
            }
            QueueAct::Up(k) => q.tasks.swap(k, k - 1),
            QueueAct::Retry(k) => {
                q.tasks[k].state = TaskState::Waiting;
                q.tasks[k].note.clear();
            }
            QueueAct::ClearDone => q.tasks.retain(|t| t.state != TaskState::Done),
            QueueAct::Close => q.tasks.clear(),
        }
        self.claude_save();
    }

    /// Lines `a..=b` (0-based) of document `i` given to Claude with the next message.
    pub(super) fn claude_attach_lines(&mut self, i: usize, a: usize, b: usize) {
        let Some(d) = self.docs.get(i) else { return };
        let Some(path) = d.path.clone() else {
            self.set_status(t!("Enregistre d'abord le fichier pour le donner à Claude", "Save the file first to give it to Claude"));
            return;
        };
        let b = b.min(d.rope.len_lines().saturating_sub(1));
        let text: String = (a..=b).map(|l| d.line_text(l)).collect::<Vec<_>>().join("\n");
        self.claude_attach(Attach { path, lines: Some((a + 1, b + 1, text)), note: None });
    }

    pub(super) fn claude_attach_path(&mut self, path: PathBuf) {
        self.claude_attach(Attach { path, lines: None, note: None });
    }

    /// A text given to Claude with the next message (`label`: what it is).
    fn claude_attach_note(&mut self, label: String, text: String) {
        let path = self.claude.dir.clone().unwrap_or_default();
        self.claude_attach(Attach { path, lines: None, note: Some((label, text)) });
    }

    /// The text document Claude's context is about: the active one, else the last one opened.
    fn claude_editor_doc(&self) -> Option<usize> {
        let ok = |d: &Document| d.view == document::View::Text && d.path.is_some();
        self.docs.get(self.active).filter(|d| ok(*d)).map(|_| self.active).or_else(|| self.docs.iter().rposition(ok))
    }

    /// What the « + Contexte » menu of the chat asked for.
    /// A commit given to Claude with the next message: its message, the files it touched and its
    /// diff (cut when very large), to start from or to compare with.
    pub(super) fn claude_attach_commit(&mut self, repo: &Path, hash: &str) {
        match git::run(repo, &["show", "--stat", "--patch", "--format=commit %H%nAuteur : %an, %ad%n%n%B", "--date=short", hash]) {
            Ok(text) => {
                let subject = git::stdout(repo, &["log", "-1", "--format=%s", hash]).unwrap_or_default();
                let mut text: String = text.chars().take(MAX_DIFF_CHARS).collect();
                if text.chars().count() >= MAX_DIFF_CHARS {
                    text.push_str(t!("\n[… diff coupé, trop long]", "\n[… diff cut, too long]"));
                }
                self.claude.dir.get_or_insert_with(|| repo.to_path_buf());
                let short = &hash[..hash.len().min(8)];
                self.claude_attach_note(tf!("Commit {short} : {}", "Commit {short}: {}", subject.trim()), text);
            }
            Err(e) => self.set_error(format!("git show : {}", e.lines().next().unwrap_or(""))),
        }
    }

    fn claude_add_context(&mut self, what: CtxAdd) {
        let doc = self.claude_editor_doc();
        match what {
            CtxAdd::Commit(hash) => {
                if let Some(dir) = self.claude.dir.clone() {
                    self.claude_attach_commit(&dir, &hash);
                }
            }
            CtxAdd::File => match doc.and_then(|i| self.docs[i].path.clone()) {
                Some(p) => self.claude_attach_path(p),
                None => self.set_status(t!("Aucun fichier ouvert", "No file open")),
            },
            CtxAdd::Selection => match doc {
                Some(i) => {
                    let d = &self.docs[i];
                    let s = d.primary_sel();
                    let a = d.rope.char_to_line(s.start());
                    let b = d.rope.char_to_line(s.end().saturating_sub(usize::from(s.end() > s.start())));
                    self.claude_attach_lines(i, a, b.max(a));
                }
                None => self.set_status(t!("Aucun fichier ouvert", "No file open")),
            },
            CtxAdd::Tabs => {
                let paths: Vec<PathBuf> = self.docs.iter().filter(|d| d.view == document::View::Text).filter_map(|d| d.path.clone()).collect();
                for p in paths {
                    self.claude_attach_path(p);
                }
            }
            CtxAdd::Diff => {
                let Some(dir) = self.claude.dir.clone() else { return };
                match super::git_view::worktree_diff(&dir, &["diff", "HEAD"]) {
                    Ok(d) if !d.trim().is_empty() => {
                        let d: String = d.chars().take(MAX_DIFF_CHARS).collect();
                        self.claude_attach_note(t!("Changements non commités", "Uncommitted changes").into(), d);
                    }
                    Ok(_) => self.set_status(t!("Aucun changement non commité", "No uncommitted change")),
                    Err(e) => self.set_error(format!("git diff : {}", e.lines().next().unwrap_or(""))),
                }
            }
            CtxAdd::Problems => {
                let Some(i) = doc else {
                    self.set_status(t!("Aucun fichier ouvert", "No file open"));
                    return;
                };
                let d = &self.docs[i];
                let name = d.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let list: Vec<String> = d
                    .diagnostics
                    .iter()
                    .map(|g| tf!("ligne {} : {}{} ({})", "line {}: {}{} ({})", g.line + 1, g.code.as_ref().map(|c| format!("{c} ")).unwrap_or_default(), g.message, g.source))
                    .collect();
                if list.is_empty() {
                    self.set_status(tf!("Aucun problème dans {name}", "No problem in {name}"));
                } else {
                    self.claude_attach_note(tf!("Problèmes de {name}", "Problems of {name}"), list.join("\n"));
                }
            }
        }
    }

    fn claude_attach(&mut self, a: Attach) {
        if self.claude.dir.is_none() {
            self.claude.dir = self.repo_for(&a.path).or_else(|| self.repo.clone()).or_else(|| self.root.clone());
        }
        let label = a.label(self.claude.dir.as_deref());
        let same = |x: &Attach| {
            x.path == a.path && x.lines.as_ref().map(|l| (l.0, l.1)) == a.lines.as_ref().map(|l| (l.0, l.1)) && x.note.as_ref().map(|n| &n.0) == a.note.as_ref().map(|n| &n.0)
        };
        if let Some(x) = self.claude.attach.iter_mut().find(|x| same(x)) {
            // the same diff or problems again: the newest text
            x.note = a.note;
        } else {
            self.claude.attach.push(a);
        }
        self.set_status(tf!("✳ {label} ajouté au contexte de Claude ({} élément(s)) — Ctrl+Alt+C pour lui écrire", "✳ {label} added to Claude's context ({} item(s)) — Ctrl+Alt+C to write to it", self.claude.attach.len()));
    }

    /// The selection (else the line of the cursor) of the current file for Claude; `ask`: and
    /// the Claude tab opened to write the question.
    pub(super) fn claude_attach_selection(&mut self, ask: bool) {
        let Some(d) = self.docs.get(self.active).filter(|d| d.view == document::View::Text) else { return };
        let s = d.primary_sel();
        let a = d.rope.char_to_line(s.start());
        let b = d.rope.char_to_line(s.end().saturating_sub(usize::from(s.end() > s.start())));
        self.claude_attach_lines(self.active, a, b.max(a));
        if ask {
            self.open_claude();
        }
    }

    /// Lines picked with ✳ in the gutter of an editor.
    pub(super) fn claude_take_picks(&mut self) {
        let picks: Vec<(usize, (usize, usize))> = self.docs.iter_mut().enumerate().filter_map(|(i, d)| d.claude_pick.take().map(|p| (i, p))).collect();
        for (i, (a, b)) in picks {
            self.claude_attach_lines(i, a, b);
        }
    }

    /// The file being edited, the line of the cursor and the selection, said to Claude.
    /// The definitions in the project of the names `text` cites (at most 4 names, each defined in
    /// at most 2 places: a name found everywhere says nothing), with their first lines; and the
    /// names given.
    fn cited_definitions(&self, text: &str, dir: &Path) -> Option<(String, Vec<String>)> {
        const MAX_NAMES: usize = 4;
        const MAX_LINES: usize = 40;
        let ix = self.index.symbols.as_ref()?;
        let (mut parts, mut names) = (Vec::new(), Vec::new());
        for name in cited_names(text) {
            if names.len() >= MAX_NAMES {
                break;
            }
            let found: Vec<(PathBuf, crate::index::Entry)> = ix
                .lookup(&name)
                .into_iter()
                .filter(|(_, e)| e.name == name && matches!(e.kind, crate::symbols::Kind::Class | crate::symbols::Kind::Function | crate::symbols::Kind::Method))
                .collect();
            if found.is_empty() || found.len() > 2 {
                continue;
            }
            for (p, e) in &found {
                let Ok(src) = std::fs::read_to_string(p) else { continue };
                let rel = p.strip_prefix(dir).unwrap_or(p).display().to_string();
                let lang = p.extension().and_then(|x| x.to_str()).unwrap_or("");
                parts.push(format!("{rel}:{} :\n```{lang}\n{}\n```", e.line + 1, definition_lines(&src, e.line, MAX_LINES)));
            }
            names.push(name);
        }
        (!parts.is_empty()).then(|| (tf!("(Définitions trouvées dans le projet pour les noms cités :\n{})", "(Definitions found in the project for the names mentioned:\n{})", parts.join("\n")), names))
    }

    /// The file open in the editor, said to Claude: the text, and the path of the file.
    fn editor_context(&self, dir: &Path) -> Option<(String, String)> {
        let d = self.docs.iter().rev().find(|d| d.view == document::View::Text && d.path.is_some())?;
        let path = d.path.as_ref()?;
        let rel = path.strip_prefix(dir).unwrap_or(path).display().to_string();
        let sel = d.primary_sel();
        let (line, _) = d.line_col(sel.head);
        let mut s = tf!("(Fichier ouvert dans l'éditeur : {rel}, ligne {}.", "(File open in the editor: {rel}, line {}.", line + 1);
        if !sel.is_empty() {
            let text: String = d.slice(sel.start(), sel.end()).chars().take(8000).collect();
            s.push_str(&tf!(" Texte sélectionné :\n```\n{text}\n```", " Selected text:\n```\n{text}\n```"));
        }
        s.push(')');
        Some((s, rel))
    }

    /// The review starts from the files as they are now.
    fn claude_start_review(&mut self) {
        let Some(repo) = self.claude.dir.clone().filter(|d| d.join(".git").exists()) else { return };
        let index = review_index_of(&repo, self.claude.id);
        let r = rgit(&repo, &index, &["read-tree", "HEAD"], None).and_then(|_| rgit(&repo, &index, &["add", "-A"], None));
        match r {
            Ok(_) => {
                self.claude.review_index = Some(index);
                self.claude.review = None;
                self.claude.collapsed.clear();
            }
            Err(e) => self.claude.items.push(Item::Error(tf!("Revue impossible : {e}", "Review impossible: {e}"))),
        }
    }

    fn claude_reload_review(&mut self) {
        let (Some(repo), Some(index)) = (self.claude.dir.clone(), self.claude.review_index.clone()) else { return };
        self.claude.review_loading = true;
        let id = self.claude.id;
        self.jobs.spawn(move || JobResult::ClaudeReview(id, review_diff(&repo, &index)));
    }

    /// The review of conversation `id` read again (wherever the conversation is now).
    pub(super) fn claude_review_loaded(&mut self, id: u64, r: Result<String, String>) {
        let Some(c) = std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).find(|c| c.id == id) else { return };
        c.review_loading = false;
        let text = r.unwrap_or_else(|e| e);
        let lines = history::parse_diff(&text);
        let files = history::diff_files(&lines);
        c.review = Some(HistoryDiff { hash: String::new(), path: None, text, lines, files });
    }

    /// Keeps or rejects a block or a file of the review.
    fn claude_review_action(&mut self, click: DiffClick) {
        let (Some(repo), Some(index), Some(d)) = (self.claude.dir.clone(), self.claude.review_index.clone(), self.claude.review.as_ref()) else { return };
        let idx = match click {
            DiffClick::Line(i) | DiffClick::Hunk(i, _) | DiffClick::File(i, _) => i,
        };
        let Some(path) = history::file_at(&d.text, idx) else { return };
        let new_file = history::file_patch(&d.text, idx).is_some_and(|f| f.contains("\n--- /dev/null"));
        let text = d.text.clone();
        let r = match click {
            DiffClick::Line(_) | DiffClick::File(_, DiffAct::Open) => {
                let line = history::target_line(&d.lines, idx);
                self.pr_open_path(&repo.join(&path), line);
                return;
            }
            DiffClick::File(_, DiffAct::Stage) => rgit(&repo, &index, &["add", "-A", "--", &path], None),
            DiffClick::Hunk(_, DiffAct::Stage) if new_file => rgit(&repo, &index, &["add", "--", &path], None),
            DiffClick::Hunk(i, DiffAct::Stage) => match history::hunk_patch(&text, i) {
                Some(p) => rgit(&repo, &index, &["apply", "--cached", "--recount", "-"], Some(&p)),
                None => return,
            },
            DiffClick::File(..) | DiffClick::Hunk(..) if new_file => std::fs::remove_file(repo.join(&path)).map(|_| String::new()).map_err(|e| e.to_string()),
            DiffClick::File(..) => {
                // the file as it was (or removed again when Claude deleted it… restored)
                rgit(&repo, &index, &["checkout-index", "-f", "--", &path], None)
            }
            DiffClick::Hunk(i, _) => match history::hunk_patch(&text, i) {
                Some(p) => git::run_with(&repo, &["apply", "-R", "--recount", "-"], &[], Some(&p)),
                None => return,
            },
        };
        // shown at once without it, git's new diff follows
        if let Some(d) = self.claude.review.as_mut() {
            d.text = history::without(&d.text, idx, matches!(click, DiffClick::File(..)));
            d.lines = history::parse_diff(&d.text);
            d.files = history::diff_files(&d.lines);
        }
        if let Err(e) = r {
            self.claude.items.push(Item::Error(format!("{path} : {e}")));
        }
        self.last_disk_check = Instant::now() - Duration::from_secs(10);
        self.claude_reload_review();
        self.git_refresh();
    }

    /// Keeps everything (the review starts again from here) or rejects everything.
    fn claude_review_all(&mut self, keep: bool) {
        let (Some(repo), Some(index)) = (self.claude.dir.clone(), self.claude.review_index.clone()) else { return };
        let r = if keep {
            rgit(&repo, &index, &["add", "-A"], None)
        } else {
            let news = rgit(&repo, &index, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"], None).unwrap_or_default();
            for p in news.lines().filter(|p| !p.is_empty()) {
                let _ = std::fs::remove_file(repo.join(p));
            }
            rgit(&repo, &index, &["checkout-index", "-a", "-f"], None)
        };
        if let Err(e) = r {
            self.claude.items.push(Item::Error(e));
        }
        self.claude.confirm_reject = false;
        self.last_disk_check = Instant::now() - Duration::from_secs(10);
        self.claude_reload_review();
        self.git_refresh();
    }

    fn pr_open_path(&mut self, path: &Path, line: usize) {
        if self.open_path(path.to_path_buf()) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.goto_line(line.saturating_sub(1), 0);
            }
            self.pending_focus = true;
        }
    }

    /// Claude into an editor tab (⇱, more room) or back into the bottom panel (⇲), remembered.
    pub(super) fn move_claude(&mut self, to_tab: bool) {
        if to_tab {
            if self.claude.tab.is_none() {
                let uid = self.new_claude_tab(self.claude_title());
                self.claude.tab = Some(uid);
            }
        } else if let Some(uid) = self.claude.tab.take() {
            // the conversation of the bottom panel until now takes its tab
            match self.claude_others.iter_mut().find(|o| o.tab.is_none()) {
                Some(o) => o.tab = Some(uid),
                None => {
                    if let Some(i) = self.docs.iter().position(|d| d.uid == uid) {
                        self.close_tab(i, true);
                    }
                }
            }
        }
        self.settings.claude_in_tab = to_tab;
        self.persist("claude_in_tab", to_tab.to_string());
        self.open_claude();
    }

    /// A new editor tab for a conversation; returns its uid.
    fn new_claude_tab(&mut self, title: String) -> u64 {
        let uid = self.next_uid();
        let mut d = Document::new(uid, self.settings.tab_size, self.settings.translate_tabs_to_spaces);
        d.view = document::View::Claude;
        d.read_only = true;
        d.title_override = Some(title);
        self.docs.push(d);
        self.activate_tab(self.docs.len() - 1);
        uid
    }

    /// Title of the tab of the current conversation: its first message, short.
    fn claude_title(&self) -> String {
        match self.claude.wish.as_deref().and_then(|w| w.lines().find(|l| !l.trim().is_empty())) {
            Some(l) if l.chars().count() > 28 => format!("{}…", l.trim().chars().take(27).collect::<String>()),
            Some(l) => l.trim().to_string(),
            None => "Claude".into(),
        }
    }

    /// An empty conversation with the settings of the current one, shown in `tab`.
    fn claude_fresh(&self, tab: Option<u64>) -> ClaudeUi {
        static N: AtomicU64 = AtomicU64::new(1);
        let c = &self.claude;
        ClaudeUi {
            id: N.fetch_add(1, Ordering::Relaxed),
            tab,
            checks: HashSet::new(),
            dir: c.dir.clone(),
            model: c.model,
            mode: c.mode,
            with_context: c.with_context,
            focus: true,
            ..Default::default()
        }
    }

    /// « Nouvelle conversation »: in a new tab, the current one goes on in its place.
    fn claude_new_conversation(&mut self) {
        let empty = self.claude.session.is_none() && self.claude.items.iter().all(|i| matches!(i, Item::Info(_)));
        if empty {
            self.claude.focus = true;
            return;
        }
        let uid = self.new_claude_tab("Claude".into());
        let fresh = self.claude_fresh(Some(uid));
        let old = std::mem::replace(&mut self.claude, fresh);
        self.claude_others.push(old);
        if self.bottom == Some(BottomTab::Claude) && !self.claude_has_bottom() {
            self.bottom = None;
        }
        self.claude_prepare();
    }

    /// A conversation is meant for the bottom panel.
    pub(super) fn claude_has_bottom(&self) -> bool {
        self.claude.tab.is_none() || self.claude_others.iter().any(|o| o.tab.is_none())
    }

    /// The conversation of `tab` (None: the bottom panel), made the current one, and shown.
    pub(super) fn claude_show(&mut self, ui: &mut egui::Ui, tab: Option<u64>) {
        self.claude_restore_last();
        if self.claude.tab != tab {
            if let Some(k) = self.claude_others.iter().position(|o| o.tab == tab) {
                std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
            } else if tab.is_some() && self.claude.tab.is_none() && self.bottom != Some(BottomTab::Claude) {
                // a Claude tab without a conversation (from the menu, a restored session): the
                // current one, shown nowhere, goes there
                self.claude.tab = tab;
            } else {
                let fresh = self.claude_fresh(tab);
                let old = std::mem::replace(&mut self.claude, fresh);
                self.claude_others.push(old);
                self.claude_prepare();
            }
        }
        self.claude.unseen = false;
        self.claude_ui(ui, tab.is_some());
    }

    /// What every conversation received; those whose tab was closed are over.
    pub(super) fn claude_poll_all(&mut self) {
        self.claude_poll();
        for k in 0..self.claude_others.len() {
            if self.claude_others[k].session.is_some() {
                std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
                self.claude_poll();
                std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
            }
        }
        let docs = &self.docs;
        let alive = |c: &ClaudeUi| c.tab.map_or(true, |u| docs.iter().any(|d| d.uid == u));
        self.claude_others.retain(alive);
        if !alive(&self.claude) {
            self.claude = match self.claude_others.iter().position(|o| o.tab.is_none()) {
                Some(k) => self.claude_others.remove(k),
                None => self.claude_fresh(None),
            };
        }
    }

    /// The current conversation written to disk (after each message and each answer).
    fn claude_save(&mut self) {
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
            model: c.model,
            mode: c.mode,
            session_id: c.session_id.clone(),
            wish: c.wish.clone(),
            cost: c.cost,
            updated: now_secs(),
            items,
            queue: c.queue.clone(),
        };
        self.claude.saved_key = key;
        self.claude_saved.retain(|s| s.key != key);
        self.claude_saved.insert(0, saved);
        self.claude_saved.truncate(SAVED_MAX);
        if let Err(e) = write_saved(&self.claude_saved) {
            self.log(tf!("Conversations Claude non enregistrées : {e}", "Claude conversations not saved: {e}"));
        }
    }

    /// A saved conversation, ready to go on (shown in `tab`).
    fn claude_from_saved(&self, s: &SavedConv, tab: Option<u64>) -> ClaudeUi {
        let mut c = self.claude_fresh(tab);
        c.saved_key = s.key;
        c.dir = s.dir.clone().or(c.dir);
        c.model = s.model.min(claude::MODELS.len() - 1);
        c.mode = s.mode.min(claude::MODES.len() - 1);
        c.session_id = s.session_id.clone();
        c.wish = s.wish.clone();
        c.cost = s.cost;
        c.items = s.items.clone();
        c.queue = s.queue.clone();
        c.queue.settle();
        c.items.push(Item::Info(t!("— conversation reprise : le prochain message continue avec son contexte —", "— conversation resumed: the next message goes on with its context —").into()));
        c
    }

    /// At the first showing of Claude after a start: the last conversation is back.
    fn claude_restore_last(&mut self) {
        if std::mem::replace(&mut self.claude_loaded, true) {
            return;
        }
        if self.claude.items.is_empty() && self.claude.session.is_none() {
            if let Some(s) = self.claude_saved.first().cloned() {
                let tab = self.claude.tab;
                self.claude = self.claude_from_saved(&s, tab);
            }
        }
    }

    /// The saved conversation shown in the tab `uid` (0: not saved yet), for the session.
    pub(super) fn claude_tab_key(&self, uid: u64) -> u64 {
        std::iter::once(&self.claude).chain(&self.claude_others).find(|c| c.tab == Some(uid)).map_or(0, |c| c.saved_key)
    }

    /// The tab of the conversation `key` at the start (0: a new conversation).
    pub(super) fn claude_restore_tab(&mut self, key: u64) {
        let saved = self.claude_saved.iter().find(|s| s.key == key).cloned();
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
    fn claude_open_saved(&mut self, key: u64) {
        if self.claude.saved_key == key {
            self.open_claude();
            return;
        }
        if let Some(o) = self.claude_others.iter().find(|o| o.saved_key == key) {
            match o.tab.and_then(|u| self.docs.iter().position(|d| d.uid == u)) {
                Some(i) => self.activate_tab(i),
                None => self.bottom = Some(BottomTab::Claude),
            }
            return;
        }
        let Some(s) = self.claude_saved.iter().find(|s| s.key == key).cloned() else { return };
        let uid = self.new_claude_tab(s.title.clone());
        let conv = self.claude_from_saved(&s, Some(uid));
        let old = std::mem::replace(&mut self.claude, conv);
        self.claude_others.push(old);
    }

    /// « Reprendre la conversation de ce fichier » : the most recent conversation that gave this
    /// file to Claude (attached, or open in the editor when a message was sent).
    pub(super) fn claude_resume_file(&mut self) {
        let Some(path) = self.claude_editor_doc().and_then(|i| self.docs[i].path.clone()) else {
            self.set_status(t!("Aucun fichier ouvert", "No file open"));
            return;
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if rel_path(self.claude.dir.as_deref(), &path).is_some_and(|rel| gives_file(&self.claude.items, &rel)) {
            self.open_claude();
            return;
        }
        let found = self.claude_saved.iter().find(|s| rel_path(s.dir.as_deref(), &path).is_some_and(|rel| gives_file(&s.items, &rel))).map(|s| s.key);
        match found {
            Some(key) => self.claude_open_saved(key),
            None => self.set_status(tf!("Aucune conversation Claude ne concerne {name}", "No Claude conversation is about {name}")),
        }
    }

    /// A conversation taken out of the history (not the open ones).
    fn claude_forget_saved(&mut self, key: u64) {
        self.claude_saved.retain(|s| s.key != key);
        if let Err(e) = write_saved(&self.claude_saved) {
            self.log(tf!("Conversations Claude non enregistrées : {e}", "Claude conversations not saved: {e}"));
        }
    }

    /// Color of the title of a Claude tab: working (accent), finished and not seen yet (green,
    /// blinking a few seconds first).
    pub(super) fn claude_tab_tint(&self, uid: u64) -> Option<Color32> {
        let c = std::iter::once(&self.claude).chain(self.claude_others.iter()).find(|c| c.tab == Some(uid))?;
        if c.busy {
            Some(self.hl.colors.accent)
        } else if c.unseen {
            self.claude_lit(c).then(ok)
        } else {
            None
        }
    }

    /// The conversation of the bottom panel finished and is not seen yet: its tab and its icon
    /// light up (blinking a few seconds first).
    pub(super) fn claude_bottom_done(&self) -> bool {
        std::iter::once(&self.claude).chain(self.claude_others.iter()).any(|c| c.tab.is_none() && c.unseen && !c.busy && self.claude_lit(c))
    }

    /// On or off: blinking for a few seconds after the end of the answer, then on.
    fn claude_lit(&self, c: &ClaudeUi) -> bool {
        match c.done_at.map(|t| t.elapsed()) {
            Some(e) if self.settings.claude_done_blink && e < Duration::from_secs(8) => {
                self.egui_ctx.request_repaint_after(Duration::from_millis(100));
                e.as_millis() / 400 % 2 == 0
            }
            _ => true,
        }
    }

    pub(super) fn claude_ui(&mut self, ui: &mut egui::Ui, in_tab: bool) {
        if self.install_guide(ui, crate::tools::Tool::Claude, in_tab) {
            return;
        }
        let accent = self.hl.colors.accent;
        let fill = ui.visuals().panel_fill;
        let dir_name = self.claude.dir.as_deref().map(|d| self.repo_label(d)).unwrap_or_default();
        let dirs: Vec<(PathBuf, String)> = self.repos.iter().map(|r| (r.clone(), self.repo_label(r))).collect();
        let (mut send, mut stop, mut new_conv, mut review_click, mut all) = (false, false, false, None, None);
        let (mut chosen_dir, mut move_to, mut ctx_add) = (None, None, None);
        let (mut queue_act, mut queued, mut read_commits) = (None, None, false);
        let mut answer: Option<String> = None;
        // the kept conversations: (key, title, folder and date)
        let history: Vec<(u64, String, String)> = self
            .claude_saved
            .iter()
            .map(|s| {
                let place = s.dir.as_deref().map(|d| self.repo_label(d)).unwrap_or_default();
                (s.key, s.title.clone(), format!("{place} · {}", git::ago(s.updated)))
            })
            .collect();
        let (mut open_saved, mut forget_saved) = (None, None);
        let (perms, budget, mut perms_changed) = (&mut self.settings.claude_perms, &mut self.settings.claude_budget, false);
        let (mut show_review, was_shown) = (self.settings.claude_review, self.settings.claude_review);
        let limits = &self.claude_limits;
        let commands = command_list(&self.claude_commands);
        let c = &mut self.claude;
        let cache = &mut self.md_cache;
        // ids of their own: two conversations can be shown at once (bottom panel and a tab)
        let cid = |name: &str| egui::Id::new(name).with(c.id);
        let (bar_id, review_id, input_id, chat_id) = (cid("claude_bar"), cid("claude_review"), cid("claude_input"), cid("claude_chat"));
        let queue_id = cid("claude_queue");
        let conv = c.id;

        // ------------------------------------------------ settings of the conversation
        egui::TopBottomPanel::top(bar_id)
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 8.0, bottom: 6.0 }))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("✳ Claude").strong().size(15.0).color(accent));
                    let running = c.session.is_some();
                    ui.add_enabled_ui(!running, |ui| {
                        if dirs.len() > 1 {
                            let labels: Vec<String> = dirs.iter().map(|(_, l)| l.clone()).collect();
                            let current = dirs.iter().position(|(d, _)| c.dir.as_ref() == Some(d));
                            if let Some(k) = crate::pick::combo(ui, ("claude_dir", conv), &dir_name, &labels, current) {
                                chosen_dir = Some(dirs[k].0.clone());
                            }
                        } else {
                            ui.label(RichText::new(&dir_name).weak());
                        }
                        egui::ComboBox::from_id_salt(("claude_model", conv)).selected_text(claude::label(claude::MODELS[c.model].1)).show_ui(ui, |ui| {
                            for (k, (_, label)) in claude::MODELS.iter().enumerate() {
                                ui.selectable_value(&mut c.model, k, claude::label(label));
                            }
                        });
                        egui::ComboBox::from_id_salt(("claude_mode", conv)).selected_text(claude::label(claude::MODES[c.mode].1)).show_ui(ui, |ui| {
                            for (k, (_, label, tip)) in claude::MODES.iter().enumerate() {
                                ui.selectable_value(&mut c.mode, k, claude::label(label)).on_hover_text(claude::label(tip));
                            }
                        });
                    })
                    .response
                    .on_disabled_hover_text(t!("Pour changer : « Nouvelle conversation »", "To change: “New conversation”"));
                    let n_perms = perms.iter().filter(|p| claude::PERMS.iter().any(|(k, ..)| k == p)).count();
                    ui.menu_button(format!("🔒 Permissions ({n_perms})"), |ui| {
                        ui.set_min_width(330.0);
                        if running {
                            ui.label(RichText::new(t!("S'applique à la prochaine conversation", "Applies to the next conversation")).small().color(warn()));
                        }
                        if c.mode != 0 {
                            ui.label(RichText::new(tf!("Mode « {} » : seules les deux dernières cases comptent", "“{}” mode: only the last two boxes count", claude::label(claude::MODES[c.mode].1))).small().weak());
                        }
                        ui.add_enabled(false, egui::Checkbox::new(&mut true, t!("Lire les fichiers du projet", "Read the project files"))).on_disabled_hover_text(t!("Toujours : Claude doit lire pour aider", "Always: Claude has to read to help"));
                        for (key, label, tip) in claude::PERMS {
                            let mut on = perms.iter().any(|p| p == key);
                            if key == "sandbox" {
                                ui.separator();
                            }
                            if ui.checkbox(&mut on, claude::label(label)).on_hover_text(claude::label(tip)).changed() {
                                perms.retain(|p| p != key);
                                if on {
                                    perms.push(key.to_string());
                                }
                                perms_changed = true;
                            }
                        }
                        if perms.iter().any(|p| p == "sandbox") && crate::lsp::in_path("bwrap").is_none() {
                            ui.label(RichText::new(t!("⚠ Le bac à sable demande bubblewrap (paquet bubblewrap) et socat", "⚠ The sandbox needs bubblewrap (bubblewrap package) and socat")).small().color(warn()));
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(t!("Budget max par conversation", "Max budget per conversation"));
                            let r = ui.add(egui::DragValue::new(budget).speed(0.1).range(0.0..=100.0).prefix("$ ").max_decimals(2));
                            if r.changed() {
                                perms_changed = true;
                            }
                            r.on_hover_text(t!("La conversation s'arrête au-delà (0 : sans limite)", "The conversation stops beyond it (0: no limit)"));
                        });
                    })
                    .response
                    .on_hover_text(t!("Ce que Claude a le droit de faire", "What Claude is allowed to do"));
                    if ui.button(t!("Nouvelle conversation", "New conversation")).on_hover_text(t!("Dans un nouvel onglet : celle-ci continue dans le sien (l'onglet devient vert quand Claude a fini)", "In a new tab: this one goes on in its own (the tab turns green when Claude is done)")).clicked() {
                        new_conv = true;
                    }
                    ui.menu_button(tf!("Historique ({})", "History ({})", history.len()), |ui| {
                        ui.set_min_width(420.0);
                        if history.is_empty() {
                            ui.label(RichText::new(t!("Les conversations sont gardées ici, même après avoir fermé Florm (50 au plus).", "Conversations are kept here, even after closing Florm (50 at most).")).weak());
                        }
                        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                            for (key, title, info) in &history {
                                ui.horizontal(|ui| {
                                    if ui.button(title).on_hover_text(t!("La rouvrir : la suite continue avec son contexte", "Reopen it: what follows goes on with its context")).clicked() {
                                        open_saved = Some(*key);
                                        ui.close_menu();
                                    }
                                    ui.label(RichText::new(info).small().weak());
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("×").on_hover_text(t!("L'oublier", "Forget it")).clicked() {
                                            forget_saved = Some(*key);
                                        }
                                    });
                                });
                            }
                        });
                    })
                    .response
                    .on_hover_text(t!("Les conversations d'avant, gardées entre les sessions", "The earlier conversations, kept between sessions"));
                    if c.cost > 0.0 {
                        ui.label(RichText::new(format!("{:.2} $", c.cost)).weak().small()).on_hover_text(t!("Coût estimé de la conversation", "Estimated cost of the conversation"));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(crate::view_bar::BarAct::Move(b)) = crate::view_bar::controls(ui, in_tab, None, None) {
                            move_to = Some(b);
                        }
                        // the review of the changes on the right, or the whole width for the conversation
                        let n = c.review.as_ref().map_or(0, |d| d.files.len());
                        let label = match (show_review, n) {
                            (true, _) => t!("Revue ▶", "Review ▶").to_string(),
                            (false, 0) => t!("◀ Revue", "◀ Review").to_string(),
                            (false, n) => tf!("◀ Revue ({n})", "◀ Review ({n})"),
                        };
                        let text = if !show_review && n > 0 { RichText::new(label).color(accent).strong() } else { RichText::new(label) };
                        let tip = if show_review { t!("Masquer la revue des changements : la conversation prend toute la largeur", "Hide the review of the changes: the conversation takes the whole width") } else { t!("Afficher à droite la revue de ce que Claude a changé", "Show on the right the review of what Claude changed") };
                        if ui.small_button(text).on_hover_text(tip).clicked() {
                            show_review = !show_review;
                        }
                        ui.add_space(8.0);
                        // the plan's quota over 5 hours always; over 7 days and the context only
                        // past 70 % (right to left: they come after the 5 hours)
                        let mut gauges: Vec<(String, f32, String)> = Vec::new();
                        for window in ["five_hour", "seven_day"] {
                            let (label, what) = match window {
                                "five_hour" => ("5 h", t!("sur 5 heures", "over 5 hours")),
                                _ => ("7 j", t!("sur 7 jours", "over 7 days")),
                            };
                            match limits.iter().find(|l| l.0 == window) {
                                // its window is over since: back to zero
                                Some((_, _, reset)) if *reset > 0 && *reset <= now_secs() => {
                                    gauges.push((label.into(), 0.0, tf!("Quota de ton abonnement {what} : remis à zéro depuis la dernière réponse de Claude", "Quota of your subscription {what}: reset since Claude's last answer")));
                                }
                                Some((_, used, reset)) => {
                                    gauges.push((label.into(), *used, tf!("Quota de ton abonnement {what} : {:.0} % utilisé, remis à zéro {}", "Quota of your subscription {what}: {:.0} % used, reset {}", used * 100.0, until(*reset))));
                                }
                                None => gauges.push((label.into(), -1.0, tf!("Quota de ton abonnement {what} : connu à la première réponse de Claude", "Quota of your subscription {what}: known at Claude's first answer"))),
                            }
                        }
                        let (tokens, window) = c.context;
                        if tokens > 0 && window > 0 {
                            let used = (tokens as f32 / window as f32).min(1.0);
                            gauges.push((
                                t!("Contexte", "Context").into(),
                                used,
                                tf!("Contexte de la conversation : {} / {} tokens.\nPlein, Claude résume le début de la conversation ; « Nouvelle conversation » repart de zéro.", "Context of the conversation: {} / {} tokens.\nWhen full, Claude summarizes the start of the conversation; “New conversation” starts from scratch.", thousands(tokens), thousands(window)),
                            ));
                        }
                        for (k, (label, used, tip)) in gauges.into_iter().enumerate().rev() {
                            match k {
                                0 if used < 0.0 => {
                                    ui.label(RichText::new("5 h – %").small().weak()).on_hover_text(tip);
                                    ui.add_space(6.0);
                                }
                                0 => gauge(ui, &label, used, accent, tip),
                                _ if used > 0.7 => gauge(ui, &label, used, accent, tip),
                                _ => {}
                            }
                        }
                    });
                });
            });

        // ------------------------------------------------ the review, on the right (or taking all the room)
        let view = cid("claude_view");
        let review_max = maximize::maxed(ui.ctx(), view) == Some("review");
        let review_ui = |ui: &mut egui::Ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("CE QUE CLAUDE A CHANGÉ", "WHAT CLAUDE CHANGED")).small().strong().color(accent));
                let n = c.review.as_ref().map_or(0, |d| d.files.len());
                if n > 0 {
                    ui.label(RichText::new(tf!("{n} fichier(s)", "{n} file(s)")).small().weak());
                }
                if c.review_loading {
                    spinner(ui);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    maximize::button(ui, view, "review");
                    if n > 0 {
                        let label = if c.confirm_reject { t!("Confirmer : tout rejeter", "Confirm: reject all") } else { t!("↺ Tout rejeter", "↺ Reject all") };
                        if ui.add(egui::Button::new(RichText::new(label).color(err())).small()).on_hover_text(t!("Remettre les fichiers comme au début de la revue (les nouveaux fichiers sont supprimés)", "Put the files back as at the start of the review (the new files are deleted)")).clicked() {
                            if c.confirm_reject {
                                all = Some(false);
                            } else {
                                c.confirm_reject = true;
                            }
                        }
                        if ui.small_button(t!("✔ Tout garder", "✔ Keep all")).on_hover_text(t!("Tout accepter : la revue repart de là", "Accept all: the review starts again from there")).clicked() {
                            all = Some(true);
                        }
                    }
                });
            });
            ui.separator();
            match &c.review {
                _ if c.review_index.is_none() => {
                    ui.label(RichText::new(t!("Les fichiers modifiés par Claude apparaîtront ici, bloc par bloc : tu gardes ou tu rejettes chaque partie.", "The files changed by Claude will show here, block by block: you keep or reject each part.")).weak());
                }
                Some(d) if d.files.is_empty() => {
                    ui.label(RichText::new(t!("Aucun changement à revoir.", "No change to review.")).weak());
                }
                Some(d) => {
                    let hunk_buttons: &[history::DiffButton] = &[
                        (t!("✔ Garder", "✔ Keep"), t!("Accepter ce bloc (il quitte la revue, il reste dans le fichier)", "Accept this block (it leaves the review, it stays in the file)"), DiffAct::Stage),
                        (t!("↺ Rejeter", "↺ Reject"), t!("Retirer ce bloc du fichier", "Remove this block from the file"), DiffAct::Discard),
                    ];
                    let file_buttons: &[history::DiffButton] = &[
                        (t!("Ouvrir", "Open"), t!("Ouvrir le fichier", "Open the file"), DiffAct::Open),
                        (t!("✔ Garder", "✔ Keep"), t!("Accepter tout le fichier", "Accept the whole file"), DiffAct::Stage),
                        (t!("↺ Rejeter", "↺ Reject"), t!("Remettre le fichier comme avant (supprimé s'il est nouveau)", "Put the file back as before (deleted if it is new)"), DiffAct::Discard),
                    ];
                    let jump = c.jump.take();
                    review_click = history::stacked_diff_view(ui, &d.lines, &d.files, &mut c.collapsed, "claude_review", hunk_buttons, file_buttons, jump, None);
                }
                None => {
                    spinner(ui);
                }
            }
        };
        if review_max {
            egui::CentralPanel::default().show_inside(ui, review_ui);
        } else if show_review {
            egui::SidePanel::right(review_id).resizable(true).default_width(ui.available_width() * 0.5).width_range(320.0..=1400.0).show_inside(ui, review_ui);
        }

        // ------------------------------------------------ the conversation
        if !review_max {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                egui::TopBottomPanel::bottom(input_id).show_inside(ui, |ui| {
                    ui.add_space(6.0);
                    if !c.queue.tasks.is_empty() {
                        queue_act = claude_queue::queue_ui(ui, &mut c.queue, c.busy, accent, queue_id);
                    }
                    if !c.attach.is_empty() {
                        let mut remove = None;
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(t!("Contexte :", "Context:")).small().weak());
                            for (k, a) in c.attach.iter().enumerate() {
                                let chip = egui::Button::new(RichText::new(format!("✳ {}  ×", a.label(c.dir.as_deref()))).small().color(accent))
                                    .fill(icons::mix(fill, accent, 0.12))
                                    .rounding(10.0);
                                let tip = match (&a.note, &a.lines) {
                                    (Some((_, t)), _) | (None, Some((_, _, t))) => t.lines().take(20).collect::<Vec<_>>().join("\n"),
                                    (None, None) => a.path.display().to_string(),
                                };
                                if ui.add(chip).on_hover_text(format!("{tip}\n\nClic : retirer")).clicked() {
                                    remove = Some(k);
                                }
                            }
                        });
                        if let Some(k) = remove {
                            c.attach.remove(k);
                        }
                    }
                    let rows = match (c.input.lines().count() > 2, in_tab) {
                        (true, true) => 6,
                        (true, false) => 4,
                        (false, true) => 3,
                        (false, false) => 2,
                    };
                    // /btw, /status: the answer above the field, until closed
                    let mut close_side = false;
                    if let Some((asked, answer)) = &c.side {
                        egui::Frame::none().fill(icons::mix(fill, accent, 0.08)).rounding(6.0).inner_margin(8.0).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(asked).strong());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("×").on_hover_text(t!("Fermer (ce n'est pas dans la conversation)", "Close (it is not in the conversation)")).clicked() {
                                        close_side = true;
                                    }
                                });
                            });
                            match answer {
                                None => {
                                    ui.horizontal(|ui| {
                                        spinner(ui);
                                        ui.label(RichText::new(t!("Claude répond…", "Claude is answering…")).weak());
                                    });
                                }
                                Some(Ok(t)) => {
                                    egui::ScrollArea::vertical().id_salt(input_id.with("side")).max_height(260.0).show(ui, |ui| super::md::markdown(ui, cache, t));
                                }
                                Some(Err(e)) => {
                                    ui.label(RichText::new(format!("⚠ {e}")).color(err()));
                                }
                            }
                        });
                    }
                    if close_side {
                        c.side = None;
                    }
                    // a slash command being typed: the matching ones; Tab or a click completes it
                    let field_id = input_id.with("field");
                    let mut to_end = false;
                    if let Some(typed) = c.input.strip_prefix('/').filter(|t| !t.contains(char::is_whitespace)) {
                        let typed = typed.to_lowercase();
                        let first = commands.iter().filter(|(n, _)| n.starts_with(&typed));
                        let inside = commands.iter().filter(|(n, _)| !n.starts_with(&typed) && n.contains(&typed));
                        let matching: Vec<&(String, String)> = first.chain(inside).take(8).collect();
                        if !matching.is_empty() && !(matching.len() == 1 && matching[0].0 == typed) {
                            let mut pick = None;
                            if ui.memory(|m| m.has_focus(field_id)) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab)) {
                                pick = Some(matching[0].0.clone());
                            }
                            egui::Frame::none().fill(icons::mix(fill, accent, 0.05)).rounding(6.0).inner_margin(4.0).show(ui, |ui| {
                                for (k, (name, hint)) in matching.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        if ui.selectable_label(k == 0, format!("/{name}")).on_hover_text(t!("Clic ou Tab : compléter", "Click or Tab: complete")).clicked() {
                                            pick = Some(name.clone());
                                        }
                                        if !hint.is_empty() {
                                            ui.label(RichText::new(hint.as_str()).weak().small());
                                        }
                                    });
                                }
                            });
                            if let Some(name) = pick {
                                c.input = format!("/{name} ");
                                c.focus = true;
                                to_end = true;
                            }
                        }
                    }
                    let resp = ui.add(
                        egui::TextEdit::multiline(&mut c.input)
                            .id(field_id)
                            .hint_text(t!("Demande à Claude… (Ctrl+Entrée pour envoyer, / pour les commandes)", "Ask Claude… (Ctrl+Enter to send, / for the commands)"))
                            .desired_rows(rows)
                            .desired_width(f32::INFINITY)
                            .layouter(&mut layout_cutting),
                    );
                    if to_end {
                        if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), resp.id) {
                            let end = egui::text::CCursor::new(c.input.chars().count());
                            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                            st.store(ui.ctx(), resp.id);
                        }
                    }
                    if std::mem::take(&mut c.focus) {
                        resp.request_focus();
                    }
                    if resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter)) {
                        send = true;
                    }
                    ui.horizontal(|ui| {
                        if c.busy {
                            if ui.button(t!("■ Arrêter", "■ Stop")).on_hover_text(t!("Arrêter la réponse en cours (la conversation continue)", "Stop the current answer (the conversation goes on)")).clicked() {
                                stop = true;
                            }
                            spinner(ui);
                            ui.label(RichText::new(t!("Claude travaille…", "Claude is working…")).weak());
                        } else if ui.add_enabled(!c.input.trim().is_empty(), filled_button(t!("Envoyer", "Send"), accent)).clicked() {
                            send = true;
                        }
                        let tip = t!("Ajoute la demande à la file de tâches au lieu de l'envoyer.\nUne liste (- …, * …, 1. …) donne une tâche par élément ; la phrase avant la liste va avec chacune.\n« Lancer » les envoie une à une, chacune quand Claude a fini la précédente.", "Adds the request to the task queue instead of sending it.\nA list (- …, * …, 1. …) gives one task per item; the sentence before the list goes with each.\n“Run” sends them one by one, each when Claude is done with the previous one.");
                        if ui.add_enabled(!c.input.trim().is_empty(), egui::Button::new(t!("+ File", "+ Queue"))).on_hover_text(tip).clicked() {
                            let n = c.queue.add(&c.input);
                            c.input.clear();
                            queued = Some(n);
                        }
                        let ctx_menu = ui.menu_button(t!("+ Contexte", "+ Context"), |ui| {
                            ui.set_min_width(260.0);
                            for (label, tip, what) in [
                                (t!("Fichier ouvert", "Open file"), t!("Le fichier en entier (Claude le lit)", "The whole file (Claude reads it)"), CtxAdd::File),
                                (t!("Sélection (ou ligne du curseur)", "Selection (or cursor line)"), t!("Les lignes sélectionnées dans l'éditeur, avec leur texte", "The lines selected in the editor, with their text"), CtxAdd::Selection),
                                (t!("Tous les onglets ouverts", "All open tabs"), t!("Chaque fichier ouvert dans l'éditeur", "Each file open in the editor"), CtxAdd::Tabs),
                                (t!("Changements non commités", "Uncommitted changes"), t!("Le diff de ce qui n'est pas commité (git diff HEAD, nouveaux fichiers compris)", "The diff of what is not committed (git diff HEAD, new files included)"), CtxAdd::Diff),
                                (t!("Problèmes du fichier", "Problems of the file"), t!("Les erreurs et avertissements du linter dans le fichier ouvert", "The linter errors and warnings in the open file"), CtxAdd::Problems),
                            ] {
                                if ui.button(label).on_hover_text(tip).clicked() {
                                    ctx_add = Some(what);
                                    ui.close_menu();
                                }
                            }
                            // the last commits of the folder of the conversation, read when the menu opens
                            ui.menu_button(t!("Un commit", "A commit"), |ui| {
                                ui.set_min_width(420.0);
                                if c.recent_commits.is_empty() {
                                    ui.label(RichText::new(t!("Aucun commit", "No commit")).weak());
                                }
                                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                                    for (hash, short, subject, ago) in &c.recent_commits {
                                        let text = format!("{short}  {subject}");
                                        if ui.button(text).on_hover_text(tf!("{ago} — son message et son diff partent avec le prochain message\n(aussi : clic droit sur un commit dans la vue Git)", "{ago} — its message and diff go with the next message\n(also: right click on a commit in the Git view)")).clicked() {
                                            ctx_add = Some(CtxAdd::Commit(hash.clone()));
                                            ui.close_menu();
                                        }
                                    }
                                });
                            });
                            ui.separator();
                            ui.label(RichText::new(t!("Aussi : clic droit dans l'éditeur, ou ✳ dans la marge pour des lignes", "Also: right click in the editor, or ✳ in the margin for lines")).small().weak());
                        });
                        if ctx_menu.response.clicked() {
                            read_commits = true;
                        }
                        ctx_menu.response.on_hover_text(t!("Donner à Claude un fichier, la sélection, le diff, un commit, les problèmes…", "Give Claude a file, the selection, the diff, a commit, the problems…"));
                        // what goes with each message: in a menu, the options stay one click away
                        let on = [c.with_context, c.ask_first, !c.no_defs].iter().filter(|b| **b).count();
                        ui.menu_button(tf!("Options ({on}/3) ▾", "Options ({on}/3) ▾"), |ui| {
                            ui.set_min_width(300.0);
                            ui.checkbox(&mut c.with_context, t!("Fichier ouvert et sélection", "Open file and selection")).on_hover_text(t!("Dit à Claude quel fichier tu édites, la ligne du curseur et le texte sélectionné", "Tells Claude which file you are editing, the cursor line and the selected text"));
                            ui.checkbox(&mut c.ask_first, t!("Questions d'abord", "Questions first")).on_hover_text(t!("Si la demande laisse des choix ouverts, Claude pose d'abord 1 à 3 questions avec des options à cocher, au lieu de partir dans une direction à refaire (moins de tokens)", "If the request leaves choices open, Claude first asks 1 to 3 questions with options to tick, instead of going in a direction to redo (fewer tokens)"));
                            let mut defs = !c.no_defs;
                            if ui.checkbox(&mut defs, t!("Définitions citées", "Mentioned definitions")).on_hover_text(t!("Les classes et fonctions du projet nommées dans le message (SaleOrder, _compute_total, `foo`…) partent avec lui, leurs premières lignes comprises : Claude n'a pas à les chercher (moins de tokens)", "The classes and functions of the project named in the message (SaleOrder, _compute_total, `foo`…) go with it, their first lines included: Claude does not have to look for them (fewer tokens)")).changed() {
                                c.no_defs = !defs;
                            }
                        })
                        .response
                        .on_hover_text(t!("Ce qui part avec chaque message : le fichier ouvert, les questions d'abord, les définitions citées", "What goes with each message: the open file, questions first, the mentioned definitions"));
                    });
                    ui.add_space(4.0);
                });
                // "⬇ Dernier message" asked at the previous frame
                let to_end_id = egui::Id::new(("claude_to_end", chat_id));
                let to_end = ui.data_mut(|d| d.remove_temp::<bool>(to_end_id)).unwrap_or(false);
                let chat = egui::ScrollArea::vertical().id_salt(chat_id).stick_to_bottom(true).auto_shrink([false, false]);
                let out = chat.show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(900.0));
                    if c.items.is_empty() {
                        ui.add_space(20.0);
                        ui.label(RichText::new(t!("Demande quelque chose à Claude : il lit le projet et modifie les fichiers. Tu revois ses changements à droite, bloc par bloc.", "Ask Claude something: it reads the project and changes the files. You review its changes on the right, block by block.")).weak());
                        ui.add_space(6.0);
                        ui.label(RichText::new(t!("Florm utilise la commande claude (Claude Code) et ta connexion : pas de clé à configurer.", "Florm uses the claude command (Claude Code) and your login: no key to set up.")).weak().small());
                    }
                    // who speaks is said once, at the start of their turn
                    let mut claude_said = true;
                    // the questions of the last answer can be answered (not those of before)
                    let last_user = c.items.iter().rposition(|i| matches!(i, Item::User(..)));
                    // only the messages in sight are drawn (a long conversation would be laid out
                    // at every frame): the others take the height they had when last drawn
                    let ppp = ui.ctx().pixels_per_point();
                    let sight = ui.clip_rect().expand(300.0);
                    c.heights.resize(c.items.len(), (0, 0.0, false));
                    for (k, it) in c.items.iter().enumerate() {
                        ui.add_space(6.0);
                        let key = item_key(it, claude_said, c.busy, last_user.is_none_or(|u| k > u), ui.available_width(), ppp);
                        let top = ui.cursor().top();
                        let (known, h, stable) = c.heights[k];
                        // only a height measured twice the same stands for the message: one still
                        // changing (an output unfolding, an image loading) would make the
                        // conversation jump each time the message goes in and out of sight
                        if known == key && stable && (top + h < sight.top() || top > sight.bottom()) {
                            ui.allocate_space(egui::vec2(ui.available_width(), h));
                            match it {
                                Item::User(..) => claude_said = false,
                                Item::Text(_) | Item::Streaming(_) => claude_said = true,
                                _ => {}
                            }
                            continue;
                        }
                        ui.push_id(k, |ui| match it {
                            Item::User(t, given) => {
                                claude_said = false;
                                ui.add_space(6.0);
                                // on the left like the answers, set apart by its color and its edge
                                egui::Frame::none()
                                    .fill(icons::mix(fill, accent, 0.10))
                                    .stroke(egui::Stroke::new(1.0_f32, icons::mix(fill, accent, 0.35)))
                                    .rounding(8.0)
                                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(t!("Toi", "You")).small().strong().color(accent));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                super::md::copy_button(ui, t, t!("Copier", "Copy"));
                                            });
                                        });
                                        ui.add(egui::Label::new(t.as_str()).wrap());
                                        if !given.is_empty() {
                                            ui.label(RichText::new(format!("✳ {}", given.join(", "))).small().weak());
                                        }
                                    });
                            }
                            Item::Text(t) | Item::Streaming(t) => {
                                if !claude_said {
                                    claude_said = true;
                                    ui.label(RichText::new("✳ Claude").small().strong().color(accent));
                                }
                                let (rest, questions) = claude_ask::parse(t);
                                if questions.is_empty() {
                                    super::md::markdown_checks(ui, cache, t, &mut c.checks, k);
                                    return;
                                }
                                super::md::markdown_checks(ui, cache, &rest, &mut c.checks, k);
                                let open = matches!(it, Item::Text(_)) && !c.busy && last_user.is_none_or(|u| k > u);
                                let answers = if open {
                                    if c.answers.as_ref().map(|a| a.item) != Some(k) {
                                        c.answers = Some(claude_ask::Answers::for_item(k, questions.len()));
                                    }
                                    c.answers.as_mut()
                                } else {
                                    None
                                };
                                if claude_ask::questions_ui(ui, &questions, answers, accent) {
                                    answer = c.answers.as_ref().map(|a| a.message(&questions));
                                }
                            }
                            Item::Tool { name, summary, result, .. } => {
                                let (icon, color) = match name.as_str() {
                                    "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => ("✏", warn()),
                                    "Bash" => ("$", accent),
                                    _ => ("·", ui.visuals().weak_text_color()),
                                };
                                let error = result.as_ref().is_some_and(|(_, e)| *e);
                                let head = RichText::new(format!("{icon} {name}  {summary}")).monospace().size(12.0).color(if error { err() } else { color });
                                // a command: copied to run it again in a terminal
                                let command = (name == "Bash" && !summary.is_empty()).then_some(summary.as_str());
                                // lines that wrap: one too wide (a long command, its output) would widen the
                                // conversation, and every answer after it would go past the right edge
                                match result.as_ref().filter(|(t, _)| !t.trim().is_empty() && (name == "Bash" || error)) {
                                    Some((t, _)) => {
                                        let id = ui.next_auto_id();
                                        let (_, header, _) = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                                            .show_header(ui, |ui| {
                                                ui.horizontal_wrapped(|ui| {
                                                    let clicked = ui.add(egui::Label::new(head).sense(egui::Sense::click())).clicked();
                                                    if let Some(cmd) = command {
                                                        super::md::copy_button(ui, cmd, t!("Copier", "Copy"));
                                                    }
                                                    clicked
                                                })
                                                .inner
                                            })
                                            .body(|ui| {
                                                let t: String = t.lines().take(80).collect::<Vec<_>>().join("\n");
                                                ui.add(egui::Label::new(RichText::new(t).monospace().size(11.5).weak()).wrap());
                                            });
                                        if header.inner {
                                            if let Some(mut s) = egui::collapsing_header::CollapsingState::load(ui.ctx(), id) {
                                                s.toggle(ui);
                                                s.store(ui.ctx());
                                            }
                                        }
                                    }
                                    None => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(head);
                                            if result.is_none() {
                                                spinner(ui);
                                            }
                                            if let Some(cmd) = command {
                                                super::md::copy_button(ui, cmd, t!("Copier", "Copy"));
                                            }
                                        });
                                    }
                                }
                            }
                            Item::Info(t) => {
                                ui.label(RichText::new(t).weak().italics());
                            }
                            Item::Error(t) => {
                                ui.label(RichText::new(format!("⚠ {t}")).color(err()));
                            }
                            Item::Usage { input, cached, output, cost } => {
                                let mut line = tf!(
                                    "{} tokens en entrée ({} lus du cache), {} en sortie",
                                    "{} input tokens ({} read from the cache), {} output",
                                    thousands(*input),
                                    thousands(*cached),
                                    thousands(*output)
                                );
                                if *cost > 0.0 {
                                    line += &format!(" · {cost:.3} $");
                                }
                                ui.label(RichText::new(line).weak().small()).on_hover_text(t!(
                                    "Ce que cette réponse a coûté : les tokens lus et écrits pour elle, et son coût estimé.",
                                    "What this answer cost: the tokens read and written for it, and its estimated cost."
                                ));
                            }
                        });
                        // allocate_space adds the spacing after it again
                        let new_h = ui.cursor().top() - top - ui.spacing().item_spacing.y;
                        c.heights[k] = (key, new_h, known == key && (new_h - h).abs() < 0.5);
                    }
                    ui.add_space(10.0);
                    // the end of the conversation: scrolled to here, not to an offset past the end
                    // (the messages out of sight are not laid out, so that offset broke the view)
                    let end = ui.allocate_exact_size(egui::vec2(1.0, 1.0), Sense::hover()).0;
                    if to_end {
                        ui.scroll_to_rect(end, Some(egui::Align::BOTTOM));
                    }
                });
                // scrolled up: a button to go back down to the last message
                let below = out.content_size.y - out.state.offset.y - out.inner_rect.height();
                if below > 40.0 {
                    let r = out.inner_rect;
                    let at = egui::Rect::from_min_size(egui::pos2(r.right() - 170.0, r.bottom() - 36.0), egui::vec2(150.0, 26.0));
                    let b = egui::Button::new(RichText::new(t!("⬇ Dernier message", "⬇ Last message")).strong()).fill(ui.visuals().widgets.inactive.weak_bg_fill).rounding(13.0);
                    if ui.put(at, b).on_hover_text(t!("Descendre à la fin de la conversation", "Go down to the end of the conversation")).clicked() {
                        ui.data_mut(|d| d.insert_temp(to_end_id, true));
                        ui.ctx().request_repaint();
                    }
                }
            });
        }

        if let Some(t) = move_to {
            self.move_claude(t);
        }
        if read_commits {
            let dir = self.claude.dir.clone();
            let out = dir.and_then(|d| git::stdout(&d, &["log", "-40", "--format=%H%x1f%h%x1f%s%x1f%cr"]).ok()).unwrap_or_default();
            self.claude.recent_commits = out
                .lines()
                .filter_map(|l| {
                    let mut f = l.split('\x1f');
                    Some((f.next()?.to_string(), f.next()?.to_string(), f.next()?.to_string(), f.next()?.to_string()))
                })
                .collect();
        }
        if let Some(w) = ctx_add {
            self.claude_add_context(w);
        }
        if let Some(k) = open_saved {
            self.claude_open_saved(k);
        }
        if let Some(k) = forget_saved {
            self.claude_forget_saved(k);
        }
        if show_review != was_shown {
            self.settings.claude_review = show_review;
            self.persist("claude_review", show_review.to_string());
        }
        if perms_changed {
            let (p, b) = (config::toml_str_array(&self.settings.claude_perms), format!("{:?}", self.settings.claude_budget));
            self.persist("claude_perms", p);
            self.persist("claude_budget", b);
        }
        if let Some(d) = chosen_dir {
            self.claude.dir = Some(d);
            self.claude.review_index = None;
            self.claude.review = None;
        }
        if new_conv {
            self.claude_new_conversation();
        }
        if stop {
            if let Some(s) = self.claude.session.as_mut() {
                let _ = s.interrupt();
            }
            // stopped by hand: the queue waits (the task can be tried again)
            let q = &mut self.claude.queue;
            if let Some(k) = q.current().filter(|k| q.tasks[*k].state == TaskState::Running) {
                q.tasks[k].state = TaskState::Failed;
                q.tasks[k].note = t!("Arrêtée", "Stopped").into();
            }
            q.running = false;
        }
        if send {
            self.claude_send();
        }
        if let Some(m) = answer {
            self.claude.answers = None;
            self.claude_send_text(m, false);
        }
        if let Some(n) = queued {
            self.set_status(tf!("{n} tâche(s) ajoutée(s) à la file — « Lancer » pour les envoyer une à une", "{n} task(s) added to the queue — “Run” to send them one by one"));
            self.claude_save();
        }
        if let Some(a) = queue_act {
            self.claude_queue_act(a);
        }
        if let Some(c) = review_click {
            self.claude_review_action(c);
        }
        if let Some(keep) = all {
            self.claude_review_all(keep);
        }
    }

    /// A commit message proposed by Claude for what is staged (else for all the changes).
    pub(super) fn suggest_commit_message(&mut self) {
        let Some(repo) = self.repo.clone() else { return };
        let staged = self.git.status.as_ref().is_some_and(|s| s.entries.iter().any(|e| e.staged()));
        let wish = self.claude.wish.clone().filter(|_| self.claude.dir.as_ref() == Some(&repo)).unwrap_or_default();
        self.git.suggesting = true;
        self.jobs.spawn(move || {
            let diff = if staged { git::run(&repo, &["diff", "--cached"]) } else { git::run(&repo, &["diff", "HEAD"]) }.unwrap_or_default();
            let recent = git::stdout(&repo, &["log", "-12", "--format=%s"]).unwrap_or_default();
            let r = if diff.trim().is_empty() { Err(t!("Rien à décrire : aucun changement", "Nothing to describe: no change").to_string()) } else { claude::commit_message(&repo, &diff, &recent, &wish) };
            JobResult::CommitMessage(r)
        });
    }

    pub(super) fn commit_message_ready(&mut self, r: Result<String, String>) {
        self.git.suggesting = false;
        match r {
            Ok(m) => {
                self.git.commit_msg = m;
                self.set_status(t!("Message de commit proposé par Claude — à relire", "Commit message suggested by Claude — to read over"));
            }
            Err(e) => self.set_error(tf!("Pas de message proposé : {}", "No message suggested: {}", e.lines().next().unwrap_or(""))),
        }
    }
}

/// The names of a message that look like code: in backquotes, followed by "(", CamelCase, or
/// with an underscore — not the words of the sentence.
/// The path of `path` relative to the folder `dir` of a conversation (None outside it).
fn rel_path(dir: Option<&Path>, path: &Path) -> Option<String> {
    dir.and_then(|d| path.strip_prefix(d).ok()).map(|r| r.display().to_string())
}

/// Whether a message of a conversation gave the file `rel`, or a part of it (`rel:line`).
fn gives_file(items: &[Item], rel: &str) -> bool {
    let prefix = format!("{rel}:");
    items.iter().any(|i| matches!(i, Item::User(_, given) if given.iter().any(|g| g == rel || g.starts_with(&prefix))))
}

fn cited_names(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |w: &str| {
        let w = w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_'));
        if w.chars().count() >= 3 && w.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && !out.iter().any(|o| o == w) {
            out.push(w.to_string());
        }
    };
    let quoted: Vec<&str> = text.split('`').skip(1).step_by(2).collect();
    for q in &quoted {
        for w in q.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            push(w);
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphanumeric() || chars[i] == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let w: String = chars[start..i].iter().collect();
            let call = chars.get(i) == Some(&'(');
            let camel = w.chars().next().is_some_and(|c| c.is_uppercase()) && w.chars().skip(1).any(|c| c.is_uppercase()) && w.chars().any(|c| c.is_lowercase());
            let snake = w.contains('_') && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if call || camel || snake {
                push(&w);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The lines of a definition starting at `line`: until the code comes back to its indentation
/// (the next definition), at most `max`.
fn definition_lines(src: &str, line: usize, max: usize) -> String {
    let lines: Vec<&str> = src.lines().skip(line).collect();
    let Some(first) = lines.first() else { return String::new() };
    let indent = first.len() - first.trim_start().len();
    let mut out = vec![*first];
    for l in lines.iter().skip(1).take(max - 1) {
        let ind = l.len() - l.trim_start().len();
        // decorators and blank lines do not end it; code at its indentation or less does
        if !l.trim().is_empty() && ind <= indent && !l.trim_start().starts_with(')') {
            break;
        }
        out.push(l);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        git::run(dir, args).unwrap();
    }

    #[test]
    fn conversations_are_kept() {
        let s = SavedConv {
            key: 7,
            title: "Corrige le bug".into(),
            dir: Some("/p".into()),
            model: 1,
            mode: 0,
            session_id: Some("abc".into()),
            wish: None,
            cost: 0.5,
            updated: 1,
            items: vec![
                Item::User("Corrige".into(), vec!["a.py".into()]),
                Item::Tool { id: "t".into(), name: "Bash".into(), summary: "ls".into(), result: Some(("ok".into(), false)) },
                Item::Text("Fait.".into()),
            ],
            queue: {
                let mut q = Queue::default();
                q.add("- a\n- b");
                q.running = true;
                q
            },
        };
        let back: Vec<SavedConv> = serde_json::from_str(&serde_json::to_string(&vec![s]).unwrap()).unwrap();
        assert_eq!((back[0].key, back[0].session_id.as_deref(), back[0].items.len()), (7, Some("abc"), 3));
        assert!(matches!(&back[0].items[1], Item::Tool { result: Some((r, false)), .. } if r == "ok"));
        // the tasks are kept, not the run (it is started again by hand)
        assert_eq!((back[0].queue.tasks.len(), back[0].queue.running), (2, false));
        // a conversation saved before the queue existed
        let old = r#"[{"key":1,"title":"t","dir":null,"model":0,"mode":0,"session_id":null,"wish":null,"cost":0.0,"updated":1,"items":[]}]"#;
        let old: Vec<SavedConv> = serde_json::from_str(old).unwrap();
        assert!(old[0].queue.tasks.is_empty());
    }

    #[test]
    fn names_and_definitions_given() {
        assert_eq!(cited_names("Corrige SaleOrder et _compute_total, puis appelle foo() ; voir `bar.baz`. La vue est fausse."), ["bar", "baz", "SaleOrder", "_compute_total", "foo"]);
        assert!(cited_names("Ajoute un bouton dans la vue Git").is_empty());
        let src = "import os\n\nclass A:\n    x = 1\n\n    def f(self):\n        return 2\n\n\nclass B:\n    pass\n";
        assert_eq!(definition_lines(src, 2, 40), "class A:\n    x = 1\n\n    def f(self):\n        return 2");
        assert_eq!(definition_lines(src, 5, 40), "    def f(self):\n        return 2");
        assert_eq!(definition_lines(src, 2, 2), "class A:\n    x = 1");
    }

    #[test]
    fn what_is_given_to_claude() {
        let dir = Path::new("/p");
        let a = Attach { path: "/p/src/m.py".into(), lines: Some((3, 4, "a\nb".into())), note: None };
        assert_eq!(a.label(Some(dir)), "src/m.py:3-4");
        assert_eq!(a.prompt(Some(dir)), "Extrait de src/m.py:3-4 :\n```py\na\nb\n```");
        let f = Attach { path: "/p/src/m.py".into(), lines: None, note: None };
        assert_eq!(f.prompt(Some(dir)), "Fichier à prendre en compte : src/m.py");
        let n = Attach { path: "/p".into(), lines: None, note: Some(("Changements non commités".into(), "diff".into())) };
        assert_eq!(n.label(Some(dir)), "Changements non commités");
        assert_eq!(n.prompt(Some(dir)), "Changements non commités :\n```\ndiff\n```");
    }

    #[test]
    fn review_keeps_and_rejects() {
        let dir = std::env::temp_dir().join(format!("florm_review_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["init", "-q"]);
        let lines: String = (1..=30).map(|i| format!("l{i}\n")).collect();
        std::fs::write(dir.join("a.txt"), &lines).unwrap();
        sh(&dir, &["add", "."]);
        sh(&dir, &["-c", "user.email=a@b", "-c", "user.name=a", "commit", "-qm", "init"]);
        // a change of the user before the review: not Claude's
        std::fs::write(dir.join("mine.txt"), "à moi\n").unwrap();
        let index = review_index_of(&dir, 0);
        assert!(review_index_of(&dir, 3).ends_with("florm-review-index-3"));
        rgit(&dir, &index, &["read-tree", "HEAD"], None).unwrap();
        rgit(&dir, &index, &["add", "-A"], None).unwrap();
        assert_eq!(review_diff(&dir, &index).unwrap(), "");
        // Claude changes two places and adds a file
        std::fs::write(dir.join("a.txt"), lines.replace("l2\n", "DEUX\n").replace("l28\n", "VINGT-HUIT\n")).unwrap();
        std::fs::write(dir.join("new.txt"), "nouveau\n").unwrap();
        let d = review_diff(&dir, &index).unwrap();
        assert!(d.contains("+DEUX") && d.contains("+VINGT-HUIT") && d.contains("+nouveau") && !d.contains("à moi"), "{d}");
        // keep the first block, reject the second
        let hunks: Vec<usize> = d.split('\n').enumerate().filter(|(_, l)| l.starts_with("@@")).map(|(i, _)| i).collect();
        rgit(&dir, &index, &["apply", "--cached", "--recount", "-"], Some(&history::hunk_patch(&d, hunks[0]).unwrap())).unwrap();
        git::run_with(&dir, &["apply", "-R", "--recount", "-"], &[], Some(&history::hunk_patch(&d, hunks[1]).unwrap())).unwrap();
        let a = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(a.contains("DEUX") && a.contains("l28") && !a.contains("VINGT"));
        let d = review_diff(&dir, &index).unwrap();
        assert!(!d.contains("a.txt") && d.contains("new.txt"), "{d}");
        // the real index was not touched
        assert_eq!(git::run(&dir, &["diff", "--cached", "--name-only"]).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A small gauge (right to left: the bar, then its label), red when nearly full.
fn gauge(ui: &mut egui::Ui, label: &str, used: f32, accent: Color32, tip: String) {
    let color = if used >= 0.9 { err() } else if used >= 0.7 { warn() } else { accent };
    let (rect, r) = ui.allocate_exact_size(egui::vec2(46.0, 14.0), Sense::hover());
    let bar = egui::Rect::from_center_size(rect.center(), egui::vec2(46.0, 6.0));
    ui.painter().rect_filled(bar, 3.0, ui.visuals().widgets.inactive.bg_fill);
    ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * used, bar.height())), 3.0, color);
    let text = ui.label(RichText::new(format!("{label} {:.0} %", used * 100.0)).small().weak());
    r.union(text).on_hover_text(tip);
    ui.add_space(6.0);
}

/// "dans 2 h 05", "dans 3 j" until a unix time.
fn until(ts: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let s = (ts - now).max(0);
    match s {
        0..=59 => t!("maintenant", "now").into(),
        60..=3599 => tf!("dans {} min", "in {} min", s / 60),
        3600..=86_399 => tf!("dans {} h {:02}", "in {} h {:02}", s / 3600, s % 3600 / 60),
        _ => tf!("dans {} j {} h", "in {} d {} h", s / 86_400, s % 86_400 / 3600),
    }
}

/// 1234567 → "1 234 567".
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}
