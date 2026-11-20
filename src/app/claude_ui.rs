//! The Claude tab: a conversation with Claude Code on the left, and on the right the review of
//! what it changed in the files since the review started, block by block (kept or rejected).
//!
//! The review compares the files with a private index (`.git/florm-review-index`) holding them as
//! they were: keeping a block adds it to that index, rejecting it takes it out of the file.

use super::*;
use crate::claude::{self, Event};
use crate::claude_account;
use serde::{Deserialize, Serialize};
use history::{DiffAct, DiffClick};
use super::claude_queue::{self, Queue, QueueAct, TaskState};
use super::claude_turns::{Checkpoint, Rewind, TurnDiff};
use super::claude_tools::{ToolAct, ToolsUi, Worktree};

mod asks;
mod bar;
mod chat;
mod context;
mod hub;
mod input;
mod live;
mod review;
mod send;
mod session;
mod side;
mod store;
mod term;
pub(in crate::app) use context::*;
pub(in crate::app) use hub::*;
pub(in crate::app) use review::*;
pub(in crate::app) use store::*;

/// A line of the conversation.
#[derive(Clone, Serialize, Deserialize)]
pub(super) enum Item {
    /// A message, with what was given with it, the texts given (a pasted text…: what it is,
    /// the text), shown folded under the message, and the images given (what it is, its PNG).
    User(String, Vec<String>, #[serde(default)] Vec<(String, String)>, #[serde(default)] Vec<(String, PathBuf)>),
    /// The answer, being written (pieces) or whole.
    Text(String),
    Streaming(String),
    /// Claude's thinking before it answers (shown folded).
    Thinking(String),
    /// A tool used, its result, and for an agent the tools it used in turn (name, what about).
    Tool {
        id: String,
        name: String,
        summary: String,
        result: Option<(String, bool)>,
        #[serde(default)]
        steps: Vec<(String, String)>,
        /// When it started and ended (unix milliseconds; end 0: still running, start 0: unknown).
        #[serde(default)]
        time: (u64, u64),
        /// A subagent run in the background: its state and report (None: another tool, or an
        /// agent of before).
        #[serde(default)]
        agent: Option<AgentRun>,
    },
    Info(String),
    Error(String),
    /// What the last answer cost: tokens in (`cached` of them read from the cache), out, dollars.
    Usage { input: u64, cached: u64, output: u64, cost: f64 },
}

/// A subagent at work (Claude Code runs them in the background: the turn may end before it).
#[derive(Clone, Default, Serialize, Deserialize, Hash)]
pub(super) struct AgentRun {
    /// Its kind (`general-purpose`, `Explore`…), its state ("running", "completed", "failed"…),
    /// what it is doing, and its report (what it wrote, or the summary given at its end).
    pub(super) kind: String,
    pub(super) status: String,
    pub(super) doing: String,
    pub(super) report: String,
}

#[derive(Default)]
pub(super) struct ClaudeUi {
    /// A list of completions (`/` commands, `@` files) was shown at the last frame: Escape closes it.
    pub(super) completing: bool,
    /// The field as it was when Escape closed the list: no list until it changes.
    pub(super) dismissed: Option<String>,
    /// Number of the conversation (0: the first one), and the editor tab showing it (document
    /// uid; None: the bottom panel).
    pub(super) id: u64,
    tab: Option<u64>,
    /// List items of the answers ticked (or unticked) by a click: (answer, item's text). Not kept
    /// with the conversation.
    pub(super) checks: HashSet<(usize, String)>,
    /// It finished answering while it was not shown: its tab is colored until seen (and blinks
    /// a few seconds after `done_at`).
    unseen: bool,
    done_at: Option<Instant>,
    /// Key of the conversation in the saved ones (0: not saved yet).
    saved_key: u64,
    /// Session of Claude Code, continued (`--resume`) when the process is started again (after
    /// a restart of Florm), and whether the running process confirmed it.
    pub(super) session_id: Option<String>,
    started: bool,
    /// Folder of the conversation (the selected repository, else the project).
    pub(super) dir: Option<PathBuf>,
    pub(super) session: Option<claude::Session>,
    pub(super) items: Vec<Item>,
    pub(super) input: String,
    pub(super) busy: bool,
    /// While it works: what it is doing when nothing of it shows (thinking, a subagent…), and
    /// when it last sent something.
    activity: Option<String>,
    news: Option<Instant>,
    /// The message waiting for its files to be noted before it leaves.
    pub(super) outgoing: Option<send::Outgoing>,
    /// Value of --model ("opus", "claude-opus-4-8"…; empty: the default of Claude Code).
    pub(super) model: String,
    /// Value of --effort (empty: Claude Code chooses).
    pub(super) effort: String,
    /// The model really at work, as Claude Code names it ("claude-opus-5-5"; empty: not known yet).
    pub(super) model_id: String,
    mode: usize,
    cost: f64,
    /// Tokens of the conversation read by the model for its last message, and its window.
    pub(super) context: (u64, u64),
    /// The context is over 60 % of its window (the warning is given when it gets there).
    pub(super) context_high: bool,
    /// The bar of the full context was closed: shown again when it gets there once more.
    context_hidden: bool,
    /// The file being edited and its selection are given with the message.
    with_context: bool,
    /// Lines and files given with the next message.
    pub(super) attach: Vec<Attach>,
    /// First message of the conversation: the wish behind the changes (for the commit message).
    pub(super) wish: Option<String>,
    /// The review: its index and the changes since.
    pub(super) review_index: Option<PathBuf>,
    pub(super) review: Option<HistoryDiff>,
    review_loading: bool,
    collapsed: HashSet<usize>,
    jump: Option<history::FileJump>,
    /// "Tout rejeter" clicked once: a second click does it.
    confirm_reject: bool,
    pub(super) focus: bool,
    /// The to-do list: tasks sent one by one.
    pub(super) queue: Queue,
    /// The definitions of the names cited are not given with the messages.
    no_defs: bool,
    /// Mode « Demander avant »: the changes and commands waiting for the user.
    pub(super) asks: Vec<asks::Ask>,
    /// Florm was closed while Claude was at work here: a band offers to go on.
    pub(super) interrupted: bool,
    /// Last commits of its folder for « + Contexte → Un commit »: (hash, short hash, subject, when).
    recent_commits: Vec<(String, String, String, String)>,
    /// The clipboard held an image when « + Contexte » was opened (« Image du presse-papiers »).
    clip_image: bool,
    /// Height of each item when it was last drawn, with the key of what it showed (`item_key`),
    /// and whether it was the same the time before (only then is the item left undrawn out of sight).
    pub(super) heights: Vec<(u64, f32, bool)>,
    /// `/btw` or `/status`: what was asked and the answer (None: on its way), shown above the
    /// field until closed; not part of the conversation.
    pub(super) side: Option<(String, Option<Result<String, String>>)>,
    /// The state before each message of the user (to go back to it), and the id of the last
    /// message of Claude's answers (where the conversation can be cut).
    pub(super) checkpoints: Vec<Checkpoint>,
    pub(super) last_uuid: Option<String>,
    /// After going back: the session starts again from that message (`--resume-session-at`).
    pub(super) resume_at: Option<String>,
    /// The messages of the user whose turn is folded (their index).
    pub(super) folded: HashSet<usize>,
    /// What Florm told Claude of the project when its process started (`florm_context`).
    pub(super) florm_ctx: String,
    /// The groups of steps (tools, thinking) unfolded: the index of their first item.
    pub(super) open_steps: HashSet<usize>,
    /// The changes of one question, shown instead of the review.
    pub(super) turn_diff: Option<TurnDiff>,
    /// ↶ clicked on a message: what to take back is asked.
    pub(super) confirm_rewind: Option<usize>,
    /// The model, the mode, the permissions… changed while Claude ran: the next message starts
    /// it again with them (`--resume`: the conversation goes on).
    pub(super) restart: bool,
    /// Folders Claude may use besides its own (`--add-dir`).
    pub(super) add_dirs: Vec<PathBuf>,
    /// The worktree the conversation was started in (« 🌿 »).
    pub(super) worktree: Option<Worktree>,
    /// Since when it has been working (shown in the list of the conversations).
    pub(super) busy_since: Option<Instant>,
    /// A copy of another conversation (« ⇉ Dupliquer »).
    pub(super) forked: bool,
    /// `#` at the start of a message: the note to remember, until where is chosen.
    memory: Option<String>,
    /// The fields and lists of the menus (⋯, rules, worktree, `@`).
    pub(super) tools: ToolsUi,
    /// Where it was drawn, and at which frame: an image dropped there is given to it.
    pub(super) drop_area: Option<(u64, egui::Rect)>,
    /// The outputs of its commands shown while they run.
    live: live::Live,
    /// The Claude account it runs with (its name; empty: the first one), and the one its session
    /// (`session_id`) was made with: another one, it is copied there before going on.
    pub(super) account: String,
    pub(super) session_account: String,
    /// The account was chosen by hand (👤): the folders of the accounts no longer choose it.
    pub(super) account_chosen: bool,
    /// Claude Code in a terminal (Préférences → Claude dans un terminal): its session in the
    /// terminals, the files where its hooks write (see `crate::claude_term`), what to start it
    /// with, what waits to be pasted once it is ready (and then Enter), whether it waits for you,
    /// and what its status line said last.
    pub(super) term: Option<u64>,
    pub(super) term_file: Option<PathBuf>,
    pub(super) term_prompt: Option<String>,
    pub(super) term_pending: String,
    pub(super) term_submit: bool,
    pub(super) term_waiting: bool,
    pub(super) term_state: crate::claude_term::State,
}

/// The configuration folder of the account chosen by default in `s`.
pub(in crate::app) fn default_account_dir(s: &config::Settings) -> Option<PathBuf> {
    let accounts = claude_account::accounts(&s.claude_accounts);
    claude_account::find(&accounts, &s.claude_account).dir.clone()
}

/// What the height of a message depends on: its content, its place and the width.
/// Colours and ids shared by the parts of the panel of a conversation.
#[derive(Clone, Copy)]
struct Shared {
    accent: Color32,
    fill: Color32,
    in_tab: bool,
    /// The conversation is Claude Code in a terminal: no review of Florm (git does it).
    term: bool,
    /// The id of the conversation: two can be shown at once (bottom panel and a tab).
    conv: u64,
}

/// What the parts of the panel ask for while drawn, done once they are (when `self` is free).
#[derive(Default)]
struct Acts {
    send: bool,
    stop: bool,
    new_conv: bool,
    review_click: Option<DiffClick>,
    all: Option<bool>,
    later: bool,
    compact: bool,
    ask_act: Option<asks::AskAct>,
    resume: Option<bool>,
    chosen_dir: Option<PathBuf>,
    move_to: Option<bool>,
    ctx_add: Option<CtxAdd>,
    pick_dir: bool,
    queue_act: Option<QueueAct>,
    read_commits: bool,
    image_add: Option<(Vec<u8>, String)>,
    open_image: Option<PathBuf>,
    answer: Option<String>,
    shot: Option<Duration>,
    tool_act: Option<ToolAct>,
    mem_act: Option<Option<bool>>,
    mode_cycle: bool,
    fold: Option<usize>,
    fold_all: Option<bool>,
    turn_req: Option<usize>,
    rewind: Option<(usize, Option<Rewind>)>,
    close_turn: bool,
    turn_click: Option<DiffClick>,
    open_saved: Option<u64>,
    forget_saved: Option<u64>,
    chosen_account: Option<String>,
    perms_changed: bool,
    show_review: bool,
    side_review: bool,
}

impl App {
    /// The last commits of the folder of conversation `id`, for « + Contexte → Un commit ».
    pub(super) fn claude_commits_loaded(&mut self, id: u64, out: String) {
        let Some(c) = std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).find(|c| c.id == id) else { return };
        c.recent_commits = out
            .lines()
            .filter_map(|l| {
                let mut f = l.split('\x1f');
                Some((f.next()?.to_string(), f.next()?.to_string(), f.next()?.to_string(), f.next()?.to_string()))
            })
            .collect();
    }
}

/// What a tool did, in a word: « Commande », « Lu », « Modifié »… (an MCP tool: its server and name).
fn tool_verb(name: &str) -> String {
    let word = match name {
        "Bash" => t!("Commande", "Ran"),
        "Read" => t!("Lu", "Read"),
        "Edit" | "MultiEdit" | "NotebookEdit" => t!("Modifié", "Edited"),
        "Write" => t!("Écrit", "Wrote"),
        "Grep" => t!("Cherché", "Searched"),
        "Glob" => t!("Fichiers", "Files"),
        "WebFetch" => t!("Page web", "Fetched"),
        "WebSearch" => t!("Recherche web", "Searched the web"),
        "TodoWrite" => t!("Tâches", "Tasks"),
        _ => {
            return match name.strip_prefix("mcp__") {
                Some(rest) => format!("MCP {}", rest.replace("__", " › ")),
                None => name.to_string(),
            }
        }
    };
    word.to_string()
}

/// The message saying what was ticked in the tasks of answer `k` (`- [ ]`), and how many: None
/// when nothing is ticked. The tasks left unticked are said too, so that Claude does not do them.
fn ticked_reply(text: &str, checks: &HashSet<(usize, String)>, k: usize) -> Option<(String, usize)> {
    let tasks = super::md::tasks(text);
    let state: Vec<(&str, bool)> = tasks.iter().map(|(t, on)| (t.as_str(), *on ^ checks.contains(&(k, t.clone())))).collect();
    let yes: Vec<&str> = state.iter().filter(|s| s.1).map(|s| s.0).collect();
    if yes.is_empty() {
        return None;
    }
    let no: Vec<&str> = state.iter().filter(|s| !s.1).map(|s| s.0).collect();
    let list = |v: &[&str]| v.iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n");
    let mut msg = tf!("Ok, tu peux faire :\n{}", "Ok, go ahead with:\n{}", list(&yes));
    if !no.is_empty() {
        msg += &tf!("\n\nPas pour l'instant :\n{}", "\n\nNot for now:\n{}", list(&no));
    }
    Some((msg, yes.len()))
}

/// A step of an answer, folded with the others around it: a tool (not a subagent, shown as a card)
/// or a thinking with something to read.
fn is_step(it: &Item) -> bool {
    match it {
        Item::Tool { name, .. } => !matches!(name.as_str(), "Agent" | "Task"),
        Item::Thinking(t) => !t.trim().is_empty(),
        _ => false,
    }
}

/// The runs of at least two steps (first, last item), what costs and empty thinkings left aside.
fn step_groups(items: &[Item]) -> Vec<(usize, usize)> {
    let mut groups = Vec::new();
    let mut run: Option<(usize, usize, usize)> = None;
    for (k, it) in items.iter().enumerate() {
        let transparent = matches!(it, Item::Usage { .. }) || matches!(it, Item::Thinking(t) if t.trim().is_empty());
        if is_step(it) {
            run = Some(match run {
                Some((a, _, n)) => (a, k, n + 1),
                None => (k, k, 1),
            });
        } else if !transparent {
            if let Some((a, b, n)) = run.take() {
                if n >= 2 {
                    groups.push((a, b));
                }
            }
        }
    }
    if let Some((a, b, n)) = run {
        if n >= 2 {
            groups.push((a, b));
        }
    }
    groups
}

/// The line of a group of steps: how many, of which kinds (« Cherché ×2, Lu, Commande »), and
/// while it runs, the step in progress.
fn steps_summary(items: &[Item], open: bool, running: bool) -> String {
    let mut kinds: Vec<(String, usize)> = Vec::new();
    let mut n = 0;
    for it in items.iter().filter(|i| is_step(i)) {
        n += 1;
        let word = match it {
            Item::Tool { name, .. } => tool_verb(name),
            _ => t!("Réflexion", "Thinking").to_string(),
        };
        match kinds.iter_mut().find(|(w, _)| *w == word) {
            Some(k) => k.1 += 1,
            None => kinds.push((word, 1)),
        }
    }
    let list = kinds.iter().map(|(w, c)| if *c > 1 { format!("{w} ×{c}") } else { w.clone() }).collect::<Vec<_>>().join(", ");
    let arrow = if open { "▾" } else { "▸" };
    let mut text = tf!("{arrow} {n} étapes · {list}", "{arrow} {n} steps · {list}");
    if running {
        if let Some(Item::Tool { name, summary, .. }) = items.iter().rev().find(|i| matches!(i, Item::Tool { .. })) {
            let what: String = shown_summary(name, summary).chars().take(80).collect();
            text += &tf!(" — en cours : {} {what}", " — now: {} {what}", tool_verb(name));
        }
    }
    text
}

/// « Claude is thinking… », with the time since its last news when it is long (it is still there).
fn working_label(activity: Option<&str>, news: Option<Instant>) -> String {
    let what = match activity {
        Some(a) => tf!("Claude {a}…", "Claude {a}…"),
        None => t!("Claude travaille…", "Claude is working…").to_string(),
    };
    match news.map(|t| t.elapsed().as_secs()) {
        Some(s) if s >= 15 => tf!("{what} ({s} s sans nouvelles)", "{what} (no news for {s} s)"),
        _ => what,
    }
}

/// The color of the subagents' cards, apart from the accent of the questions.
fn agent_color(dark: bool) -> Color32 {
    let (s, v) = if dark { (0.45, 0.90) } else { (0.60, 0.60) };
    egui::ecolor::Hsva::new(0.78, s, v, 1.0).into()
}

/// A subagent: what it was asked, its state (it runs in the background, the turn may end
/// before it), the last tools it used, and its report folded. `alive`: the conversation still
/// runs (else an agent never ended was stopped with it).
#[allow(clippy::too_many_arguments)]
fn agent_card(
    ui: &mut egui::Ui,
    k: usize,
    what: &str,
    result: Option<&(String, bool)>,
    steps: &[(String, String)],
    time: (u64, u64),
    agent: Option<&AgentRun>,
    alive: bool,
    cache: &mut egui_commonmark::CommonMarkCache,
    hl: &crate::highlight::Highlighting,
    fill: Color32,
) {
    let color = agent_color(ui.visuals().dark_mode);
    let status = match (agent.map(|a| a.status.as_str()), result) {
        (Some(st), _) if !st.is_empty() => st.to_string(),
        (_, None) => "running".into(),
        (_, Some((_, true))) => "failed".into(),
        _ => "completed".into(),
    };
    let running = status == "running" && alive;
    egui::Frame::none()
        .fill(icons::mix(fill, color, 0.07))
        .stroke(egui::Stroke::new(1.0_f32, icons::mix(fill, color, 0.45)))
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(t!("⇉ Sous-agent", "⇉ Subagent")).small().strong().color(color))
                    .on_hover_text(t!("Une autre instance de Claude, lancée par Claude pour une recherche ou une tâche à part ; elle travaille en arrière-plan et lui rend un rapport", "Another instance of Claude, started by Claude for a search or a task apart; it works in the background and hands it a report"));
                if let Some(kind) = agent.map(|a| a.kind.as_str()).filter(|k| !k.is_empty()) {
                    ui.label(RichText::new(kind).small().weak());
                }
                match status.as_str() {
                    _ if running => {
                        spinner(ui);
                        ui.label(RichText::new(t!("en cours", "running")).small().color(color));
                    }
                    "running" => {
                        ui.label(RichText::new(t!("arrêté", "stopped")).small().weak());
                    }
                    "completed" => {
                        ui.label(RichText::new(t!("✔ terminé", "✔ done")).small().color(ok()));
                    }
                    "failed" => {
                        ui.label(RichText::new(t!("✖ échoué", "✖ failed")).small().color(err()));
                    }
                    other => {
                        ui.label(RichText::new(other).small().weak());
                    }
                }
                let ms = match time {
                    (0, _) => None,
                    (a, 0) if running => Some(now_ms().saturating_sub(a)),
                    (a, b) if b >= a && b > 0 => Some(b - a),
                    _ => None,
                };
                if let Some(ms) = ms {
                    ui.label(RichText::new(duration(ms)).small().weak());
                }
            });
            if !what.is_empty() {
                ui.add(egui::Label::new(RichText::new(what).strong()).wrap());
            }
            if let Some(doing) = agent.map(|a| a.doing.as_str()).filter(|d| running && !d.is_empty()) {
                ui.add(egui::Label::new(RichText::new(doing).small().italics().weak()).wrap());
            }
            // the last steps in sight, all of them folded
            let step = |ui: &mut egui::Ui, (name, what): &(String, String)| {
                ui.add(egui::Label::new(RichText::new(format!("· {name}  {what}")).monospace().size(11.5).weak()).wrap());
            };
            let shown = 3;
            if steps.len() > shown {
                egui::CollapsingHeader::new(RichText::new(tf!("{} étapes", "{} steps", steps.len())).small().weak()).id_salt(("agent_steps", k)).show(ui, |ui| {
                    for s in &steps[..steps.len() - shown] {
                        step(ui, s);
                    }
                });
            }
            for s in &steps[steps.len().saturating_sub(shown)..] {
                step(ui, s);
            }
            let report = agent.map(|a| a.report.as_str()).filter(|r| !r.trim().is_empty());
            match (report, result) {
                (Some(r), _) => {
                    egui::CollapsingHeader::new(RichText::new(t!("Rapport de l'agent", "Report of the agent")).small().color(color)).id_salt(("agent_report", k)).show(ui, |ui| {
                        super::md::markdown(ui, cache, Some(hl), r);
                    });
                }
                (None, Some((e, true))) if !e.trim().is_empty() => {
                    ui.add(egui::Label::new(RichText::new(e.lines().take(20).collect::<Vec<_>>().join("\n")).monospace().size(11.5).color(err())).wrap());
                }
                _ => {}
            }
        });
}

fn item_key(it: &Item, claude_said: bool, busy: bool, last_turn: bool, extra: u8, width: f32, ppp: f32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::mem::discriminant(it).hash(&mut h);
    match it {
        Item::User(t, given, notes, images) => (t, given, notes, images).hash(&mut h),
        Item::Text(t) | Item::Streaming(t) | Item::Thinking(t) | Item::Info(t) | Item::Error(t) => t.hash(&mut h),
        Item::Tool { name, summary, result, steps, agent, .. } => (name, summary, result, steps, agent).hash(&mut h),
        Item::Usage { input, cached, output, cost } => (input, cached, output, cost.to_bits()).hash(&mut h),
    }
    (claude_said, busy, last_turn, extra, width.to_bits(), ppp.to_bits()).hash(&mut h);
    h.finish()
}

/// The image in the clipboard as PNG, if there is one.
/// The clipboard holds an image (its types only are read: nothing is decoded).
fn clipboard_has_image() -> bool {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let types = crate::os::command("wl-paste").arg("--list-types").stderr(std::process::Stdio::null()).output();
        if types.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).lines().any(|t| t.starts_with("image/"))) {
            return true;
        }
    }
    arboard::Clipboard::new().ok().is_some_and(|mut c| c.get_image().is_ok())
}

fn clipboard_image() -> Option<Vec<u8>> {
    if let Some(img) = arboard::Clipboard::new().ok().and_then(|mut c| c.get_image().ok()) {
        return claude::png_of(img.width, img.height, img.bytes.into_owned()).ok();
    }
    // Wayland: the clipboard of the X11 side may not have it, wl-paste reads the Wayland one
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let types = crate::os::command("wl-paste").arg("--list-types").stderr(std::process::Stdio::null()).output().ok()?;
        let types = String::from_utf8_lossy(&types.stdout).into_owned();
        let kind = types.lines().find(|t| t.starts_with("image/"))?;
        let o = crate::os::command("wl-paste").args(["--no-newline", "--type", kind]).stderr(std::process::Stdio::null()).output().ok()?;
        return o.status.success().then(|| claude::png_of_file_bytes(&o.stdout).ok()).flatten();
    }
    None
}

/// Whether `path` is a file Claude can be given with a message: an image or a PDF.
pub(super) fn is_image_file(path: &Path) -> bool {
    claude::is_pdf(path) || path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&e.to_lowercase().as_str()))
}

/// The thumbnail of an image given to Claude (`max`: its tallest), its file opened by a click;
/// a PDF: a 📄 label.
/// What the line of a step says of its target: a path read kept whole (an image) is cut there.
fn shown_summary(name: &str, summary: &str) -> String {
    if name == "Read" && Path::new(summary).is_absolute() {
        claude::short_path(summary)
    } else {
        summary.to_string()
    }
}

/// `input` shows a list of completions: a slash command being typed, or a file after `@`.
fn opens_completion(input: &str) -> bool {
    let command = input.starts_with('/') && !input.contains(char::is_whitespace);
    let mention = input.rsplit(char::is_whitespace).next().is_some_and(|w| w.starts_with('@'));
    command || mention
}

fn image_thumb(ui: &mut egui::Ui, path: &Path, max: f32) -> egui::Response {
    if claude::is_pdf(path) {
        let size = if max > 40.0 { 15.0 } else { 12.5 };
        return ui.add(egui::Button::new(RichText::new("📄 PDF").size(size)).rounding(4.0)).on_hover_text(t!("Clic : ouvrir le PDF", "Click: open the PDF"));
    }
    let uri = format!("file://{}", path.display());
    ui.add(egui::Image::new(uri.clone()).max_height(max).max_width(max * 3.0).rounding(4.0).sense(Sense::click())).on_hover_ui(|ui| {
        ui.add(egui::Image::new(uri).max_width(520.0).max_height(520.0));
        ui.label(RichText::new(t!("Clic : ouvrir l'image", "Click: open the image")).small().weak());
    })
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
        self.claude.news = Some(Instant::now());
        for e in events {
            let items = &mut self.claude.items;
            // what it does shows by itself from now on (text, a tool), else the last activity stays
            if !matches!(e, Event::Activity(_) | Event::SubTool { .. } | Event::Context(_) | Event::Limits(_) | Event::Uuid(_)) {
                self.claude.activity = None;
            }
            // it works, even without a message sent from here: a turn it starts by itself (a
            // command of the background finished), or a late end of the turn before
            if matches!(e, Event::Activity(_) | Event::ThinkingDelta(_) | Event::Thinking(_) | Event::Delta(_) | Event::Tool { .. } | Event::SubTool { .. }) {
                self.claude.busy = true;
            }
            // a thinking that stayed empty is not kept once something else comes
            if !matches!(e, Event::ThinkingDelta(_) | Event::Thinking(_) | Event::Activity(_) | Event::Context(_) | Event::Limits(_) | Event::Uuid(_)) && matches!(items.last(), Some(Item::Thinking(t)) if t.trim().is_empty()) {
                items.pop();
            }
            match e {
                Event::Activity(a) => self.claude.activity = Some(a),
                Event::ThinkingDelta(t) => match items.last_mut() {
                    Some(Item::Thinking(s)) => s.push_str(&t),
                    _ if t.trim().is_empty() => {}
                    _ => items.push(Item::Thinking(t)),
                },
                Event::Thinking(t) => match items.last_mut() {
                    Some(Item::Thinking(s)) => *s = t,
                    _ => items.push(Item::Thinking(t)),
                },
                Event::SubTool { parent, name, summary } => {
                    for it in items.iter_mut().rev() {
                        if let Item::Tool { id, steps, .. } = it {
                            if *id == parent {
                                steps.push((name, summary));
                                break;
                            }
                        }
                    }
                }
                Event::SubText { parent, text } => {
                    if let Some(Item::Tool { agent, .. }) = items.iter_mut().rev().find(|i| matches!(i, Item::Tool { id, .. } if *id == parent)) {
                        agent.get_or_insert_with(Default::default).report = text;
                    }
                }
                Event::Task { call, kind, status, text } => {
                    if let Some(Item::Tool { name, agent, time, .. }) = items.iter_mut().rev().find(|i| matches!(i, Item::Tool { id, .. } if *id == call)) {
                        // only an agent (a command sent to the background is a task too)
                        if matches!(name.as_str(), "Agent" | "Task") {
                            let a = agent.get_or_insert_with(Default::default);
                            if !kind.is_empty() {
                                a.kind = kind;
                            }
                            if status == "running" {
                                a.doing = text;
                            } else {
                                time.1 = now_ms();
                                a.doing.clear();
                                if a.report.is_empty() {
                                    a.report = text;
                                }
                            }
                            a.status = status;
                        }
                    }
                }
                Event::Note(t) => items.push(Item::Info(t)),
                // summarized: the context is small again, the bar « over 60 % » goes (it came back,
                // the old size kept until the next message)
                Event::Compacted(after) => {
                    self.claude.context.0 = after.unwrap_or(0);
                    self.claude.context_high = false;
                    self.claude.context_hidden = false;
                    save = true;
                }
                // its session: continued after a restart of Florm
                Event::Init { session, model, version, commands, terminal } => {
                    if !session.is_empty() {
                        self.claude.session_id = Some(session);
                    }
                    if !model.is_empty() {
                        self.claude.model_id = model;
                    }
                    self.claude.started = true;
                    let known = Commands { message: commands, terminal, version, models: self.claude_commands.models.clone() };
                    if !known.message.is_empty() && known != self.claude_commands {
                        write_commands(&known);
                        self.claude_commands = known;
                    }
                }
                Event::Model(m) => self.claude.model_id = m,
                Event::Session(id) => self.claude.session_id = Some(id),
                Event::Uuid(id) => self.claude.last_uuid = Some(id),
                Event::Reset => {
                    items.clear();
                    self.claude.asks.clear();
                    self.claude.checkpoints.clear();
                    self.claude.folded.clear();
                    self.claude.open_steps.clear();
                    self.claude.turn_diff = None;
                    self.claude.last_uuid = None;
                    items.push(Item::Info(t!("— /clear : Claude repart sans la conversation —", "— /clear: Claude starts again without the conversation —").into()));
                    self.claude.context.0 = 0;
                    self.claude.context_high = false;
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
                    // an image costs many tokens, and stays in the conversation: said where it happens
                    if name == "Read" && claude::is_image(&summary) {
                        let file = claude::short_path(&summary);
                        items.push(Item::Info(tf!("— Claude a ouvert une image ({file}) : elle coûte beaucoup de tokens, à chaque message suivant —", "— Claude opened an image ({file}): it costs many tokens, with every next message —")));
                    }
                    items.push(Item::Tool { id, name, summary, result: None, steps: Vec::new(), time: (now_ms(), 0), agent: None });
                }
                Event::ToolResult { id, text, error, .. } => {
                    for it in items.iter_mut().rev() {
                        if let Item::Tool { id: i, name, result, time, agent, .. } = it {
                            if *i == id {
                                reload |= claude::changes_files(name);
                                *result = Some((text, error));
                                // an agent in the background goes on after it: its end comes later
                                if !agent.as_ref().is_some_and(|a| a.status == "running") {
                                    time.1 = now_ms();
                                }
                                break;
                            }
                        }
                    }
                }
                Event::Ask { request, tool, input, call: _ } => {
                    if claude::MODES[self.claude.mode].0 == "ask" {
                        self.claude.asks.push(asks::Ask::new(request, tool, input));
                        self.claude.activity = Some(t!("attend ta réponse", "is waiting for your answer").into());
                        // not shown: its tab blinks, as at the end of an answer
                        self.claude.unseen = true;
                        self.claude.done_at = Some(Instant::now());
                    } else if let Some(s) = self.claude.session.as_mut() {
                        // started in « Demander avant », switched since: nothing is asked any more
                        let _ = s.answer(&request, Err(t!("Refusé : ce mode ne le permet pas.", "Refused: this mode does not allow it.").into()));
                    }
                }
                Event::Context(tokens) => self.claude.context.0 = tokens,
                Event::Limits(l) => {
                    let key = self.claude_limits_key(&self.claude.account);
                    if self.claude_limits.get(&key) != Some(&l) {
                        self.claude_limits.insert(key, l);
                        write_limits(&self.claude_limits);
                    }
                }
                Event::Done { error, text, cost, window, tokens } => {
                    self.claude.busy = false;
                    self.claude.asks.clear();
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
                    // once when the context gets over 60 % of its window (again after a /compact)
                    let (used, size) = self.claude.context;
                    let high = claude::over_context(used, size);
                    if high && !self.claude.context_high {
                        items.push(Item::Info(t!("— la conversation remplit plus de 60 % du contexte : /compact la résume, et une nouvelle conversation repart de zéro —", "— the conversation fills over 60% of the context: /compact summarizes it, and a new conversation starts again from zero —").into()));
                    }
                    if !high {
                        self.claude.context_hidden = false;
                    }
                    self.claude.context_high = high;
                    if error && !text.is_empty() {
                        items.push(Item::Error(text.clone()));
                    }
                    reload = true;
                    save = true;
                    task_done = Some(if error { Err(if text.is_empty() { t!("Claude s'est arrêté sur une erreur", "Claude stopped on an error").to_string() } else { text }) } else { Ok(()) });
                }
                Event::Exited(err) => {
                    self.claude.busy = false;
                    self.claude.asks.clear();
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

    /// Every conversation, for the ▾ menu of the bottom panel and the palette: its id, its tab
    /// (None: the panel's), its title, working, finished and not seen yet. The panel's first,
    /// then the tabs in their order, then the list of the Claude tab.
    pub(super) fn claude_conversations(&self) -> Vec<(u64, Option<u64>, String, bool, bool)> {
        let mut all: Vec<&ClaudeUi> = std::iter::once(&self.claude).chain(self.claude_others.iter()).collect();
        let order = |c: &&ClaudeUi| c.tab.map_or(0, |u| 1 + self.docs.iter().position(|d| d.uid == u).unwrap_or(usize::MAX - 1));
        all.sort_by_key(order);
        all.into_iter()
            .map(|c| {
                let title = c.tab.and_then(|u| self.docs.iter().find(|d| d.uid == u)).and_then(|d| d.title_override.clone()).unwrap_or_else(|| conv_title(c));
                (c.id, c.tab, title, c.busy, c.unseen && !c.busy)
            })
            .collect()
    }

    /// Title of the tab of the current conversation: its first message, short.
    fn claude_title(&self) -> String {
        conv_title(&self.claude)
    }
}

/// Title of a conversation: its first message, short ("Claude" before it).
fn conv_title(c: &ClaudeUi) -> String {
    match c.wish.as_deref().and_then(|w| w.lines().find(|l| !l.trim().is_empty())) {
        Some(l) if l.chars().count() > 28 => format!("{}…", l.trim().chars().take(27).collect::<String>()),
        Some(l) => l.trim().to_string(),
        None => "Claude".into(),
    }
}

impl App {
    /// The folder Claude works in, for the conversation not started yet.
    pub(super) fn claude_set_dir(&mut self, dir: PathBuf) {
        if self.claude.session.is_some() {
            self.set_error(t!("Pour changer de dossier : « Nouvelle conversation »", "To change the folder: “New conversation”"));
            return;
        }
        if !dir.is_dir() {
            self.set_error(tf!("Dossier introuvable : {}", "Folder not found: {}", dir.display()));
            return;
        }
        self.claude.dir = Some(crate::os::canon(&dir));
        self.claude.review_index = None;
        self.claude.review = None;
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
            model: c.model.clone(),
            effort: c.effort.clone(),
            mode: c.mode,
            with_context: c.with_context,
            add_dirs: c.add_dirs.clone(),
            // the default one; its folder may choose another (`claude_sync_account`)
            account: self.settings.claude_account.clone(),
            focus: true,
            ..Default::default()
        }
    }

    /// The Claude accounts of the settings.
    pub(in crate::app) fn claude_accounts(&self) -> Vec<claude_account::Account> {
        claude_account::accounts(&self.settings.claude_accounts)
    }

    /// A conversation not started yet takes the account of its folder (Préférences → Comptes
    /// Claude → its folders), unless one was chosen by hand.
    pub(in crate::app) fn claude_sync_account(&mut self) {
        let c = &self.claude;
        if c.account_chosen || c.session_id.is_some() || c.session.is_some() {
            return;
        }
        let Some(dir) = c.dir.as_deref() else { return };
        if let Some(a) = claude_account::for_folder(&self.settings.claude_account_folders, dir) {
            if self.claude_accounts().iter().any(|x| x.name == a) {
                self.claude.account = a;
            }
        }
    }

    /// The key of the quotas of account `name`: empty for the first one.
    pub(in crate::app) fn claude_limits_key(&self, name: &str) -> String {
        let accounts = self.claude_accounts();
        let a = claude_account::find(&accounts, name);
        if a.name == accounts[0].name { String::new() } else { a.name.clone() }
    }

    /// The configuration folder of the account named `name` (None: the usual one).
    pub(super) fn claude_account_dir(&self, name: &str) -> Option<PathBuf> {
        claude_account::find(&self.claude_accounts(), name).dir.clone()
    }

    /// The current conversation goes on with the account `name`, from the next message (its
    /// session is copied there then: `claude_session_to_account`).
    pub(in crate::app) fn claude_set_account(&mut self, name: String) {
        if self.claude.account == name {
            return;
        }
        self.claude.account_chosen = true;
        if self.claude.session.is_some() {
            self.claude.restart = true;
        }
        self.claude.account = name;
        self.claude_save();
    }

    /// A new conversation in `dir` (a worktree), in a new tab; the current one goes on in its place.
    pub(super) fn claude_new_in(&mut self, dir: PathBuf) {
        if self.claude_one_tab_at(self.claude.tab) {
            let mut fresh = self.claude_fresh(None);
            fresh.dir = Some(dir);
            self.claude_in_place(fresh);
            self.claude_prepare();
            return;
        }
        let uid = self.new_claude_tab("Claude".into());
        let mut fresh = self.claude_fresh(Some(uid));
        fresh.dir = Some(dir);
        let old = std::mem::replace(&mut self.claude, fresh);
        self.claude_others.push(old);
        if self.bottom == Some(BottomTab::Claude) && !self.claude_has_bottom() {
            self.bottom = None;
        }
        self.claude_prepare();
    }

    /// « Nouvelle conversation »: in a new tab, the current one goes on in its place.
    fn claude_new_conversation(&mut self) {
        let empty = self.claude.session.is_none() && self.claude.items.iter().all(|i| matches!(i, Item::Info(_)));
        if empty {
            self.claude.focus = true;
            return;
        }
        // one Claude tab: the new one takes its place, this one goes on in the list
        if self.claude_one_tab_at(self.claude.tab) {
            let fresh = self.claude_fresh(None);
            self.claude_in_place(fresh);
            self.claude_prepare();
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
        self.claude_sync_account();
        if self.claude.tab != tab {
            if let Some(k) = self.claude_others.iter().position(|o| o.tab == tab) {
                std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
            } else if tab.is_some() && self.settings.claude_one_tab && self.claude.tab == Some(LISTED) {
                // one Claude tab (opened again): the current one of the list comes back in it
                self.claude.tab = tab;
            } else if let Some(k) = self.claude_others.iter().enumerate().filter(|(_, o)| tab.is_some() && self.settings.claude_one_tab && o.tab == Some(LISTED)).max_by_key(|(_, o)| o.id).map(|(k, _)| k) {
                self.claude_others[k].tab = tab;
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
        // one tab for every conversation: it is named after the one shown
        if let (true, Some(uid)) = (self.settings.claude_one_tab, tab) {
            let title = self.claude_title();
            if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid && d.title_override.as_deref() != Some(title.as_str())) {
                d.title_override = Some(title);
            }
        }
        self.claude_ui(ui, tab.is_some());
    }

    /// What every conversation received; those whose tab was closed are over.
    pub(super) fn claude_poll_all(&mut self) {
        self.claude_term_tick();
        self.claude_poll();
        for k in 0..self.claude_others.len() {
            if self.claude_others[k].session.is_some() {
                std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
                self.claude_poll();
                std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
            }
        }
        self.claude_track_busy();
        let docs = &self.docs;
        let alive = |c: &ClaudeUi| c.tab.map_or(true, |u| u == LISTED || docs.iter().any(|d| d.uid == u));
        // one Claude tab: a closed tab leaves its conversation in the list (× in the list stops it),
        // unless it is empty
        if self.settings.claude_one_tab {
            let empty = |c: &ClaudeUi| c.session.is_none() && c.items.iter().all(|i| matches!(i, Item::Info(_)));
            for c in std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()) {
                if !alive(c) && !empty(c) {
                    c.tab = Some(LISTED);
                }
            }
        }
        self.claude_others.retain(alive);
        if !alive(&self.claude) {
            self.claude = match self.claude_others.iter().position(|o| o.tab.is_none()) {
                Some(k) => self.claude_others.remove(k),
                None => self.claude_fresh(None),
            };
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

    /// The whole tab of a conversation lit while it blinks, just after it finished.
    pub(super) fn claude_tab_flash(&self, uid: u64) -> Option<Color32> {
        let c = std::iter::once(&self.claude).chain(self.claude_others.iter()).find(|c| c.tab == Some(uid))?;
        let blinking = c.unseen && !c.busy && self.settings.claude_done_blink && c.done_at.is_some_and(|t| t.elapsed() < Duration::from_secs(8));
        (blinking && self.claude_lit(c)).then(ok)
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
        // Claude Code itself in a terminal, started the first time its conversation is shown
        let term_mode = self.settings.claude_terminal;
        if term_mode {
            self.claude_term_start();
        }
        let accent = self.hl.colors.accent;
        let fill = ui.visuals().panel_fill;
        let dir_name = self.claude.dir.as_deref().map(|d| self.repo_label(d)).unwrap_or_default();
        // the repositories of the opened folder, then the recent projects: any of them, not only a repository
        let mut dirs: Vec<(PathBuf, String)> = self.repos.iter().map(|r| (r.clone(), self.repo_label(r))).collect();
        for p in self.projects.iter().filter(|p| p.path.is_dir()) {
            if !dirs.iter().any(|(d, _)| *d == p.path) {
                dirs.push((p.path.clone(), format!("{}  ·  {}", p.name, p.path.display())));
            }
        }
        if let Some(d) = self.claude.dir.as_ref().filter(|d| !dirs.iter().any(|(x, _)| x == *d)) {
            dirs.insert(0, (d.clone(), d.display().to_string()));
        }
        // an image file dropped on it goes with the next message (seen at the next frame)
        self.claude.drop_area = Some((ui.ctx().cumulative_pass_nr(), ui.max_rect()));
        // `@` being typed in the field: the files of the project matching it
        let field_id = egui::Id::new("claude_input").with(self.claude.id).with("field");
        let cursor = egui::TextEdit::load_state(&self.egui_ctx, field_id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index);
        let mention = cursor.and_then(|k| super::claude_tools::mention_at(&self.claude.input, k));
        if let (Some((_, q)), Some(root), Some(dir)) = (&mention, self.root.as_deref(), self.claude.dir.as_deref()) {
            if self.claude.tools.mention.0.as_ref() != Some(q) {
                self.claude.tools.mention = (Some(q.clone()), super::claude_tools::mention_matches(&self.index.files, root, dir, q));
            }
        }
        // the kept conversations: (key, title, folder and date)
        let history: Vec<(u64, String, String)> = self
            .claude_saved
            .iter()
            .map(|s| {
                let place = s.dir.as_deref().map(|d| self.repo_label(d)).unwrap_or_default();
                (s.key, s.title.clone(), format!("{place} · {}", git::ago(s.updated)))
            })
            .collect();
        self.claude_ask_models();
        let accounts = self.claude_accounts();
        let account_name = claude_account::find(&accounts, &self.claude.account).name.clone();
        let limits_key = self.claude_limits_key(&self.claude.account);
        let (perms, budget) = (&mut self.settings.claude_perms, &mut self.settings.claude_budget);
        let (allow, deny) = (&self.settings.claude_allow, &self.settings.claude_deny);
        let no_limits = Vec::new();
        let limits = self.claude_limits.get(&limits_key).unwrap_or(&no_limits);
        let commands = command_list(&self.claude_commands);
        let models = &self.claude_commands.models;
        let session_info = session::SessionInfo {
            project: dir_name.clone(),
            branch: self.claude.dir.as_deref().and_then(git::head_branch),
            limits,
            commands: commands.len(),
            allow,
            models: &self.claude_commands.models,
        };
        let (term, term_colors, light) = (&mut self.term, self.hl.colors, self.hl.is_light());
        let c = &mut self.claude;
        let cache = &mut self.md_cache;
        let hl = &self.hl;
        let conv = c.id;
        let (model_before, effort_before, mode_before) = (c.model.clone(), c.effort.clone(), c.mode);
        let mut a = Acts { show_review: self.settings.claude_review, ..Default::default() };
        let was_shown = a.show_review;
        // the panel on the right shows the review of the changes (else the session)
        let side_id = egui::Id::new("claude_side_review").with(conv);
        a.side_review = ui.data(|d| d.get_temp::<bool>(side_id)).unwrap_or(false);
        let s = Shared { accent, fill, in_tab, term: term_mode, conv };
        let d = bar::BarData { dirs: &dirs, dir_name: &dir_name, history: &history, accounts: &accounts, account_name: &account_name, limits, allow, deny, perms, budget };
        bar::bar(ui, c, &s, d, &mut a);
        let review_max = side::side(ui, c, &s, &session_info, &mut a);
        if !review_max && term_mode {
            // the conversation is the terminal where `claude` runs
            egui::CentralPanel::default().frame(egui::Frame::none().fill(term_colors.bg)).show_inside(ui, |ui| match c.term {
                Some(id) => term.embedded_ui(ui, id, std::mem::take(&mut c.focus), term_colors, light),
                None => {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| ui.label(RichText::new(t!("Choisis un dossier en haut pour lancer Claude.", "Choose a folder at the top to start Claude.")).weak()));
                }
            });
        } else if !review_max {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                let d = input::InputData { cache: &mut *cache, hl, commands: &commands, models, cursor, field_id, mention };
                input::input(ui, c, &s, d, &mut a);
                chat::chat(ui, c, &s, cache, hl, &mut a);
            });
        }
        self.claude_apply(a, (model_before, effort_before, mode_before), was_shown);
    }

    /// What the panel of the conversation asked for, done now that `self` is free.
    fn claude_apply(&mut self, a: Acts, before: (String, String, usize), was_shown: bool) {
        let Acts { send, stop, new_conv, review_click, all, later, compact, ask_act, resume, chosen_dir, move_to, ctx_add, pick_dir, queue_act, read_commits, image_add, open_image, answer, shot, tool_act, mem_act, mode_cycle, fold, fold_all, turn_req, rewind, close_turn, turn_click, open_saved, forget_saved, chosen_account, perms_changed, show_review, side_review: _ } = a;
        let (model_before, effort_before, mode_before) = before;
        if let Some(t) = move_to {
            self.move_claude(t);
        }
        if let Some(k) = fold {
            if !self.claude.folded.remove(&k) {
                self.claude.folded.insert(k);
            }
        }
        if let Some(all) = fold_all {
            let c = &mut self.claude;
            c.folded.clear();
            if all {
                let users: Vec<usize> = c.items.iter().enumerate().filter(|(_, i)| matches!(i, Item::User(..))).map(|(k, _)| k).collect();
                c.folded.extend(users.iter().take(users.len().saturating_sub(1)));
            }
        }
        if let Some(k) = turn_req {
            self.claude_show_turn_diff(k);
        }
        if close_turn {
            self.claude.turn_diff = None;
        }
        if let Some(click) = turn_click {
            let idx = match click {
                DiffClick::Line(i) | DiffClick::Hunk(i, _) | DiffClick::File(i, _) => i,
            };
            let found = self.claude.turn_diff.as_ref().and_then(|t| t.diff.as_ref()).and_then(|d| Some((history::file_at(&d.text, idx)?, history::target_line(&d.lines, idx))));
            if let (Some((path, line)), Some(dir)) = (found, self.claude.dir.clone()) {
                self.pr_open_path(&dir.join(path), line);
            }
        }
        match rewind {
            Some((usize::MAX, _)) => self.claude.confirm_rewind = None,
            Some((k, None)) => self.claude.confirm_rewind = Some(k),
            Some((k, Some(how))) => self.claude_rewind(k, how),
            None => {}
        }
        if let Some(a) = tool_act {
            self.claude_tool_act(a);
        }
        match mem_act {
            Some(Some(personal)) => {
                if let Some(note) = self.claude.memory.take() {
                    self.claude_remember(note, personal);
                }
            }
            Some(None) => {
                if let Some(note) = self.claude.memory.take() {
                    self.claude.input = format!("# {note}");
                }
            }
            None => {}
        }
        if mode_cycle {
            self.claude.mode = (self.claude.mode + 1) % claude::MODES.len();
            self.set_status(tf!("Mode : {}", "Mode: {}", claude::label(claude::MODES[self.claude.mode].1)));
        }
        let model_changed = self.claude.model != model_before || self.claude.effort != effort_before;
        let mode_changed = self.claude.mode != mode_before;
        if model_changed || mode_changed {
            self.claude_switch(model_changed, mode_changed);
        }
        if let Some(a) = chosen_account {
            self.set_status(tf!("Compte Claude : {a} (dès le prochain message)", "Claude account: {a} (from the next message on)"));
            self.claude_set_account(a);
        }
        if read_commits {
            if let Some(dir) = self.claude.dir.clone() {
                let id = self.claude.id;
                self.jobs.spawn(move || JobResult::ClaudeCommits(id, git::stdout(&dir, &["log", "-40", "--format=%H%x1f%h%x1f%s%x1f%cr"]).unwrap_or_default()));
            }
        }
        if let Some(delay) = shot {
            self.screenshot_for_claude(delay);
        }
        if let Some(w) = ctx_add {
            self.claude_add_context(w);
        }
        if let Some((png, label)) = image_add {
            self.claude_attach_image(png, label);
        }
        if let Some(p) = open_image {
            // a PDF in the viewer of the system, an image in Florm
            if claude::is_pdf(&p) {
                self.egui_ctx.open_url(egui::OpenUrl::new_tab(format!("file://{}", p.display())));
            } else {
                self.open_path(p);
            }
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
            self.claude_restart_all();
        }
        if let Some(d) = chosen_dir {
            self.claude_set_dir(d);
        }
        if pick_dir {
            self.file_dialog(crate::os::Pick::ClaudeDir);
        }
        if new_conv {
            self.claude_new_conversation();
        }
        if stop {
            // still noting the files: the message does not leave, it goes back to the field
            if let Some(out) = self.claude.outgoing.take() {
                out.give_back(&mut self.claude);
                self.claude.busy = false;
                self.claude.activity = None;
            }
            // what waited for an answer is dropped with the answer stopped
            self.claude.asks.clear();
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
            self.claude_send(later);
        }
        if let Some(m) = answer {
            self.claude_send_text(m);
        }
        if let Some(a) = ask_act {
            self.claude_ask_act(a);
        }
        if let Some(go_on) = resume {
            self.claude.interrupted = false;
            if go_on {
                let msg = t!(
                    "Florm a été fermé pendant que tu travaillais, et ta dernière étape a été coupée. Reprends là où tu t'es arrêté (vérifie d'abord l'état des fichiers si besoin).",
                    "Florm was closed while you were working, and your last step was cut. Go on where you stopped (check the state of the files first if needed)."
                );
                self.claude_send_text(msg.into());
            } else {
                self.claude_save();
            }
        }
        if compact {
            // a command of Claude Code: it goes alone, as the other commands
            self.claude_send_text("/compact".into());
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

    /// The model or the mode changed during the conversation: while Claude works it switches at
    /// once (and starts again at the next message, so that the arguments match); otherwise the
    /// next message starts it again with them, the conversation kept.
    fn claude_switch(&mut self, model: bool, mode: bool) {
        let c = &mut self.claude;
        let Some(s) = c.session.as_mut() else { return };
        if c.busy {
            // « Demander avant » needs Claude started for it: from the next message
            if mode && claude::MODES[c.mode].0 != "ask" {
                let _ = s.set_mode(claude::mode_value(c.mode));
            }
            if model {
                let _ = s.set_model(&c.model);
            }
        }
        c.restart = true;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        git::run(dir, args).unwrap();
    }

    #[test]
    fn what_is_ticked_is_sent_back() {
        let text = "Voilà.\n\n- [ ] Essayer\n- [ ] Committer\n\n```\n- [ ] pas une tâche\n```";
        let mut checks = HashSet::new();
        assert!(ticked_reply(text, &checks, 3).is_none());
        checks.insert((3, "Committer".to_string()));
        let (msg, n) = ticked_reply(text, &checks, 3).unwrap();
        assert_eq!(n, 1);
        assert!(msg.contains("- Committer") && msg.contains("- Essayer") && !msg.contains("pas une tâche"));
        // the boxes of another answer do not count
        assert!(ticked_reply(text, &checks, 4).is_none());
    }

    #[test]
    fn the_steps_between_two_texts_are_grouped() {
        let tool = |name: &str| Item::Tool { id: String::new(), name: name.into(), summary: "x".into(), result: None, steps: Vec::new(), time: (0, 0), agent: None };
        let items = vec![
            Item::Text("a".into()),
            tool("Grep"),
            Item::Thinking(String::new()),
            tool("Grep"),
            tool("Read"),
            Item::Text("b".into()),
            tool("Bash"),
            Item::Text("c".into()),
            tool("Agent"),
            tool("Edit"),
        ];
        // a lone step stays a line of its own, a subagent is not a step
        assert_eq!(step_groups(&items), [(1, 4)]);
        assert_eq!(steps_summary(&items[1..=4], false, false), tf!("▸ 3 étapes · Cherché ×2, Lu", "▸ 3 steps · Searched ×2, Read"));
    }

    #[test]
    fn conversations_are_kept() {
        let s = SavedConv {
            key: 7,
            title: "Corrige le bug".into(),
            dir: Some("/p".into()),
            model: 1,
            model_value: None,
            effort: String::new(),
            mode: 0,
            session_id: Some("abc".into()),
            wish: None,
            cost: 0.5,
            updated: 1,
            items: vec![
                Item::User("Corrige".into(), vec!["a.py".into()], vec![], vec![]),
                Item::Tool { id: "t".into(), name: "Bash".into(), summary: "ls".into(), result: Some(("ok".into(), false)), steps: Vec::new(), time: (0, 0), agent: None },
                Item::Text("Fait.".into()),
            ],
            queue: {
                let mut q = Queue::default();
                q.push_message("a", Vec::new());
                q.push_message("b", Vec::new());
                q.running = true;
                q
            },
            options: Options { with_context: true, ..Options::default() },
            checkpoints: vec![Checkpoint { item: 0, tree: Some("abc".into()), session: None, after: None }],
            last_uuid: Some("u".into()),
            add_dirs: Vec::new(),
            worktree: None,
            account: "Perso".into(),
            session_account: "Pro".into(),
            account_chosen: true,
            interrupted: true,
        };
        let back: Vec<SavedConv> = serde_json::from_str(&serde_json::to_string(&vec![s]).unwrap()).unwrap();
        assert_eq!((back[0].key, back[0].session_id.as_deref(), back[0].items.len()), (7, Some("abc"), 3));
        assert!(back[0].options.with_context && !back[0].options.no_defs);
        // cut by the closing of Florm: said again at the next start; an older file has no such field
        assert!(back[0].interrupted);
        let old: SavedConv = serde_json::from_str(&serde_json::to_string(&back[0]).unwrap().replace(",\"interrupted\":true", "")).unwrap();
        assert!(!old.interrupted);
        assert_eq!((back[0].account.as_str(), back[0].session_account.as_str()), ("Perso", "Pro"));
        assert_eq!((back[0].checkpoints[0].tree.as_deref(), back[0].last_uuid.as_deref()), (Some("abc"), Some("u")));
        assert!(matches!(&back[0].items[1], Item::Tool { result: Some((r, false)), .. } if r == "ok"));
        // the list of the history: what the messages gave, for « Reprendre la conversation de ce fichier »
        let meta = SavedMeta::of(&back[0]);
        assert_eq!((meta.key, meta.title.as_str(), meta.given.clone()), (7, "Corrige le bug", vec!["a.py".to_string()]));
        assert!(gives(&meta.given, "a.py") && !gives(&meta.given, "b.py"));
        // the tasks are kept, not the run (it is started again by hand)
        assert_eq!((back[0].queue.tasks.len(), back[0].queue.running), (2, false));
        // a conversation saved before the queue existed
        let old = r#"[{"key":1,"title":"t","dir":null,"model":0,"mode":0,"session_id":null,"wish":null,"cost":0.0,"updated":1,"items":[]}]"#;
        let old: Vec<SavedConv> = serde_json::from_str(old).unwrap();
        assert!(old[0].queue.tasks.is_empty());
        // a message saved before the texts given were kept with it, and one with a pasted text
        let items: Vec<Item> = serde_json::from_str(r#"[{"User":["a",["b"]]}]"#).unwrap();
        assert!(matches!(&items[0], Item::User(t, _, notes, images) if t == "a" && notes.is_empty() && images.is_empty()));
        let pasted = Item::User(String::new(), vec!["Texte collé (2 lignes)".into()], vec![("Texte collé (2 lignes)".into(), "x\ny".into())], vec![]);
        let back: Item = serde_json::from_str(&serde_json::to_string(&pasted).unwrap()).unwrap();
        assert!(matches!(back, Item::User(_, _, notes, _) if notes[0].1 == "x\ny"));
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
        let a = Attach { path: "/p/src/m.py".into(), lines: Some((3, 4, "a\nb".into())), note: None, image: None };
        assert_eq!(a.label(Some(dir)), "src/m.py:3-4");
        assert_eq!(a.prompt(Some(dir)), "Extrait de src/m.py:3-4 :\n```py\na\nb\n```");
        let f = Attach { path: "/p/src/m.py".into(), lines: None, note: None, image: None };
        assert_eq!(f.prompt(Some(dir)), "Fichier à prendre en compte : src/m.py");
        let n = Attach { path: "/p".into(), lines: None, note: Some(("Changements non commités".into(), "diff".into())), image: None };
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
        // from a copy of the real index, as when the review starts
        crate::app::claude_turns::seed_index(&dir, &index).unwrap();
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

    /// On a small screen the conversation stays in the window: the bars wrap instead of
    /// pushing the messages past its right edge (where nothing scrolls to them), and so do the
    /// changes and commands waiting for an answer.
    /// « /co » typed (several commands match), Enter: the command is completed and the field keeps
    /// the focus, the next letters go on in it.
    #[test]
    fn tab_completes_a_command_and_keeps_the_focus() {
        let dir = std::env::temp_dir().join(format!("florm_claude_tab_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        // the field of Florm's own chat (not Claude in a terminal)
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\nclaude_terminal = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        crate::tools::Tool::Claude.pretend_installed();
        let mut field = egui::Id::NULL;
        let key = |pressed| egui::Event::Key { key: Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
        let mut focused_after_tab = false;
        for frame in 0..8 {
            // after the first frame: the conversation is loaded then
            if frame == 1 {
                field = egui::Id::new("claude_input").with(app.claude.id).with("field");
                app.claude.input = "/co".into();
                app.claude.focus = true;
            }
            let events = match frame {
                4 => vec![key(true)],
                5 => vec![key(false)],
                6 => vec![egui::Event::Text("x".into())],
                _ => Vec::new(),
            };
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.claude_show(ui, None));
            });
            if frame == 5 {
                focused_after_tab = ctx.memory(|m| m.has_focus(field));
            }
        }
        assert!(focused_after_tab, "the field lost the focus with Enter");
        assert!(app.claude.input.starts_with("/co") && app.claude.input.ends_with(" x") && !app.claude.input.contains('\t'), "{:?}", app.claude.input);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// « / » typed in the field, then Tab at once: the field keeps the focus (the list of the
    /// commands takes the key as soon as it shows).
    #[test]
    fn slash_then_tab_keeps_the_focus() {
        let dir = std::env::temp_dir().join(format!("florm_claude_slash_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        // the field of Florm's own chat (not Claude in a terminal)
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\nclaude_terminal = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        crate::tools::Tool::Claude.pretend_installed();
        let mut field = egui::Id::NULL;
        let key = |k, pressed| egui::Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
        let mut focus = Vec::new();
        for frame in 0..10 {
            if frame == 1 {
                field = egui::Id::new("claude_input").with(app.claude.id).with("field");
                app.claude.focus = true;
            }
            let events = match frame {
                3 => vec![egui::Event::Text("/".into())],
                4 => vec![key(Key::Tab, true)],
                5 => vec![key(Key::Tab, false)],
                6 => vec![key(Key::Tab, true)],
                7 => vec![key(Key::Tab, false)],
                _ => Vec::new(),
            };
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.claude_show(ui, None));
            });
            focus.push((frame, ctx.memory(|m| m.has_focus(field)), app.claude.input.clone()));
        }
        assert!(focus.iter().skip(3).all(|(_, f, _)| *f), "{focus:?}");
        // Tab goes through the choices: nothing completed yet, the second one chosen after two
        assert_eq!(app.claude.input, "/");
        let field_choice = field.with(("command_choice", String::new()));
        assert_eq!(ctx.data(|d| d.get_temp::<usize>(field_choice)), Some(2));
        // Escape: the list goes, the field keeps « / » and the focus; a letter brings it back
        assert!(app.claude.completing);
        app.exec(&ctx, Cmd::Escape);
        let frame = |app: &mut App, events: Vec<egui::Event>| {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.claude_show(ui, None));
            });
        };
        frame(&mut app, Vec::new());
        assert!(!app.claude.completing && app.claude.input == "/" && ctx.memory(|m| m.has_focus(field)));
        frame(&mut app, vec![egui::Event::Text("c".into())]);
        frame(&mut app, Vec::new());
        assert!(app.claude.completing, "{:?}", app.claude.input);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The pills of the bottom row (here the mode) stay at the same place when Claude starts working.
    #[test]
    fn the_buttons_do_not_move_while_claude_works() {
        let dir = std::env::temp_dir().join(format!("florm_claude_still_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        crate::tools::Tool::Claude.pretend_installed();
        let mut mode = String::new();
        // where the text of the mode's pill is drawn, after a few frames in the state set
        let mut at = |app: &mut App| {
            let mut pos = None;
            for _ in 0..3 {
                mode = claude::label(claude::MODES[app.claude.mode].1).to_string();
                let out = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 700.0))), ..Default::default() }, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.claude_show(ui, None));
                });
                // the last one drawn: the same text is in the Session card above
                fn find(shape: &egui::Shape, text: &str) -> Option<egui::Pos2> {
                    match shape {
                        egui::Shape::Text(t) if t.galley.text() == text => Some(t.pos),
                        egui::Shape::Vec(v) => v.iter().rev().find_map(|s| find(s, text)),
                        _ => None,
                    }
                }
                pos = out.shapes.iter().rev().find_map(|c| find(&c.shape, &mode));
            }
            pos.expect("the mode's pill is drawn")
        };
        let idle = at(&mut app);
        app.claude.busy = true;
        app.claude.activity = Some("Lecture de src/app/claude_ui.rs".into());
        let working = at(&mut app);
        assert_eq!(idle, working, "the mode's pill moved when Claude started working");
        // on a whole pixel: on a half one, a tiny change between two frames moved it a pixel up or down
        assert!(idle.x.fract() == 0.0 && idle.y.fract() == 0.0, "the text of the pill is at {idle:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn conversation_fits_a_small_window() {
        let dir = std::env::temp_dir().join(format!("florm_claude_narrow_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        crate::tools::Tool::Claude.pretend_installed();
        let width = 560.0;
        let mut right = 0.0_f32;
        for frame in 0..4 {
            if frame == 1 {
                // working, with a long request typed: the bottom row has all its buttons
                app.claude.items.push(Item::User("Corrige le bug".into(), Vec::new(), Vec::new(), Vec::new()));
                app.claude.items.push(Item::Thinking(String::new()));
                app.claude.items.push(Item::Text("Un texte assez long pour passer à la ligne plusieurs fois dans une fenêtre étroite. ".repeat(4)));
                app.claude.items.push(Item::Text(format!("Voici :\n```python\ndef f(x):\n    return {}\n```\n", "x + ".repeat(60))));
                app.claude.items.push(Item::Tool {
                    id: "toolu_a".into(),
                    name: "Agent".into(),
                    summary: "Trouver où la fonction b est définie dans tout le projet, avec ses appels".into(),
                    result: Some(("Async agent launched successfully".into(), false)),
                    steps: (0..5).map(|n| ("Bash".to_string(), format!("grep -rn 'def b' . --include=*.py # {n} {}", "x".repeat(80)))).collect(),
                    time: (now_ms() - 4000, 0),
                    agent: Some(AgentRun { kind: "general-purpose".into(), status: "running".into(), doing: "Running Search for function b".into(), report: "b est dans `b.py`.".into() }),
                });
                app.claude.items.push(Item::Thinking("Je regarde le fichier".into()));
                app.claude.busy = true;
                app.claude.activity = Some("Lecture de src/app/claude_ui.rs".into());
                app.claude.input = "Une demande".into();
                // « Demander avant »: a change with a long line, and a command, waiting
                app.claude.mode = claude::MODES.iter().position(|m| m.0 == "ask").unwrap();
                let long = format!("y = {}\n", "1 + ".repeat(80));
                app.claude.asks.push(asks::Ask::new("r1".into(), "Edit".into(), serde_json::json!({"file_path": proj.join("a.py"), "old_string": "x = 1\n", "new_string": long})));
                app.claude.asks.push(asks::Ask::new("r2".into(), "Bash".into(), serde_json::json!({"command": "cargo test --all-features -- --nocapture", "description": "Lancer les tests"})));
            }
            if frame == 3 {
                // cut by the closing of Florm, at the next start: the band to go on
                app.claude.busy = false;
                app.claude.interrupted = true;
            }
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 700.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.claude_show(ui, None);
                    right = ui.min_rect().right();
                });
            });
        }
        assert!(right <= width + 0.5, "the conversation goes to x = {right}, past the window ({width})");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A small gauge (right to left: the bar, then its label), red when nearly full.
fn gauge(ui: &mut egui::Ui, label: &str, used: f32, accent: Color32, tip: impl FnOnce(&mut egui::Ui)) {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(46.0, 14.0), Sense::hover());
    gauge_bar(ui, rect, used, accent);
    let text = ui.label(RichText::new(format!("{label} {:.0} %", used * 100.0)).small().weak());
    r.union(text).on_hover_ui(tip);
    ui.add_space(6.0);
}

/// The bar of a gauge in `rect`: `used` (0 to 1) filled, orange then red when nearly full.
fn gauge_bar(ui: &egui::Ui, rect: egui::Rect, used: f32, accent: Color32) {
    let color = if used >= 0.9 { err() } else if used >= 0.7 { warn() } else { accent };
    let bar = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 6.0));
    ui.painter().rect_filled(bar, 3.0, ui.visuals().widgets.inactive.bg_fill);
    ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * used.max(0.0), bar.height())), 3.0, color);
}

/// Every gauge (label, used or -1 when unknown, details) one under the other, for a tooltip.
fn gauges_tip(ui: &mut egui::Ui, gauges: &[(String, f32, String)], accent: Color32) {
    ui.set_max_width(420.0);
    for (k, (label, used, tip)) in gauges.iter().enumerate() {
        if k > 0 {
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            ui.add_sized([64.0, 14.0], egui::Label::new(RichText::new(label).strong()));
            let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 14.0), Sense::hover());
            gauge_bar(ui, rect, *used, accent);
            let pct = if *used < 0.0 { "– %".to_string() } else { format!("{:.0} %", used * 100.0) };
            ui.label(RichText::new(pct).small());
        });
        ui.label(RichText::new(tip).small().weak());
    }
}

/// Now, in unix milliseconds.
/// The button that shows (or hides) what command `id` prints.
fn live_toggle(ui: &mut egui::Ui, live: &mut live::Live, id: &str) {
    let open = live.open.contains(id);
    if ui.small_button(live_label(open)).on_hover_text(t!("Voir ce que la commande affiche, pendant qu'elle tourne", "See what the command prints, while it runs")).clicked() {
        if open {
            live.open.remove(id);
        } else {
            live.open.insert(id.to_string());
        }
    }
}

/// The label of `live_toggle`: a verb, not an arrow (the line has one already, for its details).
fn live_label(open: bool) -> &'static str {
    if open { t!("Masquer la sortie", "Hide the output") } else { t!("Voir la sortie", "Show the output") }
}

/// What command `id` (started at `start`, unix ms) prints, its end, read again while it runs.
fn live_output(ui: &mut egui::Ui, live: &mut live::Live, id: &str, start: u64, result: Option<&str>, session: Option<&str>, running: bool) {
    let frame = egui::Frame::none().fill(ui.visuals().extreme_bg_color).rounding(4.0).inner_margin(egui::Margin::symmetric(8.0, 6.0));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let Some(path) = live.file(id, start, result, session) else {
            ui.horizontal(|ui| {
                if running {
                    spinner(ui);
                }
                ui.label(RichText::new(t!("Pas encore de sortie", "No output yet")).small().weak());
            });
            return;
        };
        let (text, quiet) = live.text(&path);
        ui.horizontal(|ui| {
            let when = match quiet {
                Some(s) if s < 2 => t!("s'écrit", "being written").to_string(),
                Some(s) => tf!("rien de nouveau depuis {}", "nothing new for {}", duration(s * 1000)),
                None => String::new(),
            };
            ui.label(RichText::new(when).small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                super::md::copy_button(ui, &text, t!("Copier", "Copy"));
            });
        });
        if text.trim().is_empty() {
            ui.label(RichText::new(t!("Pas encore de sortie", "No output yet")).small().weak());
        } else {
            egui::ScrollArea::vertical().id_salt(("live_output", id)).max_height(260.0).stick_to_bottom(true).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(text).monospace().size(11.5)).wrap());
            });
        }
    });
    if running {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// How long a command took: "0,4 s", "12 s", "2 min 05".
fn duration(ms: u64) -> String {
    match ms {
        0..=9_999 => {
            let s = format!("{:.1} s", ms as f64 / 1000.0);
            if crate::i18n::en() { s } else { s.replace('.', ",") }
        }
        10_000..=59_999 => format!("{} s", ms / 1000),
        _ => format!("{} min {:02}", ms / 60_000, ms / 1000 % 60),
    }
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
