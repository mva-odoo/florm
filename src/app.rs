use crate::commands::{self, Cmd};
use crate::config::{self, Settings};
use crate::document::{self, Document, Edit, EditKind, Sel};
use crate::editor_view::{self, editor_id};
use crate::fuzzy;
use crate::git;
use crate::highlight::{self, Highlighting};
use crate::history;
use crate::icons;
use crate::jobs::{JobResult, Jobs};
use crate::keymap::{self, Combo, Keymap, Seq, UNBOUND};
use crate::lint::{self, Severity};
use crate::logo;
use crate::lsp;
use crate::plugins::{self, Input, Output, Plugin};
use crate::project::{self, Project, Session, SessionFile, UiState};
use crate::search::{self, FileHits};
use crate::sidebar::{self, Node, TreeAction};
use crate::split;
use crate::symbols;
use crate::terminal::Terminal;
use eframe::egui::{self, text::LayoutJob, Align2, Color32, Event, Key, RichText, Sense, TextFormat};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod git_view;
mod ai_review;
mod inline_ai;
mod md;
mod quick_fix;
mod pulls;
mod docker_view;
mod debugger;
mod view_tabs;
mod remote_ui;
mod install_guide;
mod odoo_tests;
mod pr_links;
mod claude_ui;
mod claude_queue;
mod claude_model;
mod claude_turns;
mod md_toolbar;
mod claude_ask;
mod theme_edit;
mod template_edit;
mod split_commit;
mod commit_plan;
mod panes;
mod lsp_client;
mod maximize;
mod navigation;
mod projects;
mod search_panel;
mod save_lint;
mod settings;
mod git_actions;
mod job_results;
mod input;
mod find;
mod overlay;
mod panels;
mod prefs;
mod title_bar;

/// The same colors whatever egui thinks the system theme is.
fn apply_visuals(ctx: &egui::Context, v: egui::Visuals) {
    ctx.set_visuals_of(egui::Theme::Dark, v.clone());
    ctx.set_visuals_of(egui::Theme::Light, v);
}

/// egui's fonts, plus Hack (the monospace font) as a fallback for the interface text: it has the
/// arrows and triangles (→ ↑ ▾ ● …) that Ubuntu-Light lacks.
pub fn font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.families.entry(egui::FontFamily::Proportional).or_default().push("Hack".to_owned());
    fonts
}

fn err() -> Color32 {
    highlight::red()
}

fn warn() -> Color32 {
    highlight::yellow()
}

fn ok() -> Color32 {
    highlight::green()
}

/// Text color readable on a filled button (accent, status color): dark on light fills, white on dark ones.
fn text_on(fill: Color32) -> Color32 {
    let l = 0.2126 * fill.r() as f32 + 0.7152 * fill.g() as f32 + 0.0722 * fill.b() as f32;
    if l > 150.0 {
        Color32::from_rgb(0x14, 0x18, 0x20)
    } else {
        Color32::WHITE
    }
}

const MIN_FRAME: Duration = Duration::from_millis(8);
/// Between two images while the window is not in front (behind another window, in the background):
/// nothing moves on screen that anyone looks at, and the processor rests.
const BACKGROUND_FRAME: Duration = Duration::from_millis(250);

/// Loading spinner like egui's, which repaints the whole window at every frame (hundreds per
/// second without vsync): this one asks for the next frame at ~30 per second.
fn spinner(ui: &mut egui::Ui) -> egui::Response {
    spinner_sized(ui, ui.style().spacing.interact_size.y)
}

fn spinner_sized(ui: &mut egui::Ui, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.ctx().request_repaint_after(Duration::from_millis(33));
        let radius = rect.height() / 2.0 - 2.0;
        let time = ui.input(|i| i.time);
        let start = time * std::f64::consts::TAU;
        let end = start + 240f64.to_radians() * time.sin();
        let points: Vec<egui::Pos2> = (0..20)
            .map(|i| {
                let (sin, cos) = egui::emath::lerp(start..=end, i as f64 / 20.0).sin_cos();
                rect.center() + radius * egui::vec2(cos as f32, sin as f32)
            })
            .collect();
        ui.painter().add(egui::Shape::line(points, egui::Stroke::new(3.0_f32, ui.visuals().strong_text_color())));
    }
    resp
}

/// Main action button, filled with `fill` (the theme accent, or a status color).
/// Field of a replacement: Maj+Entrée puts a line break in it (Entrée stays for the action).
fn replace_field<'t>(text: &'t mut String, hint: &str) -> egui::TextEdit<'t> {
    egui::TextEdit::multiline(text)
        .desired_rows(1)
        .return_key(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter))
        .hint_text(hint)
        .font(egui::TextStyle::Monospace)
}

/// Layout of a multiline field: a word longer than the width (a path, a URL) is cut at the edge
/// instead of running past it, under the panel next to it.
fn layout_cutting(ui: &egui::Ui, text: &str, wrap_width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_string(), egui::TextStyle::Body.resolve(ui.style()), ui.visuals().widgets.inactive.text_color(), wrap_width);
    job.wrap.break_anywhere = true;
    ui.fonts(|f| f.layout_job(job))
}

fn replace_tip() -> &'static str {
    t!("Maj+Entrée : retour à la ligne dans le remplacement.\nEn mode regex (.*) : \\n retour à la ligne, \\t tabulation, \\\\ barre oblique inverse, $1 groupe.\nEx. : chercher \\\\n (regex), remplacer par \\n, pour changer les « \\n » écrits en vrais retours à la ligne.", "Shift+Enter: new line in the replacement.\nIn regex mode (.*): \\n new line, \\t tab, \\\\ backslash, $1 group.\nE.g.: find \\\\n (regex), replace with \\n, to turn written “\\n” into real new lines.")
}

fn filled_button(text: impl Into<String>, fill: Color32) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(text_on(fill)).strong()).fill(fill)
}

const TAB_STRIP_H: f32 = 32.0;

#[derive(PartialEq, Clone, Copy, Debug)]
enum BottomTab {
    Search,
    Problems,
    Terminal,
    Output,
    Claude,
    Git,
    Pulls,
    Docker,
    Debug,
}

impl BottomTab {
    /// The views of the bottom panel: its tabs, and the buttons at the bottom of the sidebar.
    const ALL: [(BottomTab, icons::ViewIcon, &'static str); 9] = [
        (BottomTab::Search, icons::ViewIcon::Search, "Recherche"),
        (BottomTab::Problems, icons::ViewIcon::Problems, "Problèmes"),
        (BottomTab::Terminal, icons::ViewIcon::Terminal, "Terminal"),
        (BottomTab::Output, icons::ViewIcon::Output, "Sortie"),
        (BottomTab::Claude, icons::ViewIcon::Claude, "Claude"),
        (BottomTab::Git, icons::ViewIcon::Git, "Git"),
        (BottomTab::Pulls, icons::ViewIcon::PullRequests, "GitHub"),
        (BottomTab::Docker, icons::ViewIcon::Docker, "Docker"),
        (BottomTab::Debug, icons::ViewIcon::Debug, "Débogage"),
    ];

    /// The views offered. Docker is always there: without the docker command, its view explains
    /// how to install it.
    fn shown() -> impl Iterator<Item = (BottomTab, icons::ViewIcon, &'static str)> {
        Self::ALL.into_iter().map(|(t, i, l)| {
            let l = match l {
                "Recherche" => t!(l, "Search"),
                "Problèmes" => t!(l, "Problems"),
                "Sortie" => t!(l, "Output"),
                "Débogage" => t!(l, "Debug"),
                _ => l,
            };
            (t, i, l)
        })
    }

    /// Its name in the settings (`sidebar_hidden`).
    fn key(self) -> String {
        format!("{self:?}").to_lowercase()
    }

    /// Problems and output only exist in the bottom panel.
    fn bottom_only(self) -> bool {
        matches!(self, BottomTab::Problems | BottomTab::Output)
    }

    /// Has an icon at the bottom of the sidebar. Not problems, output (in the status bar) nor the
    /// debugger (its view opens by F5, a breakpoint's run or the palette's « Débogage »).
    fn in_sidebar(self) -> bool {
        !matches!(self, BottomTab::Problems | BottomTab::Output | BottomTab::Debug)
    }

    fn hint(tab: BottomTab) -> &'static str {
        if tab.bottom_only() {
            t!("Clic : dans le panneau du bas", "Click: in the bottom panel")
        } else {
            t!("Clic : en grand dans l'éditeur · clic droit : dans le panneau du bas", "Click: large in the editor · right click: in the bottom panel")
        }
    }
}

#[derive(Clone)]
enum Prompt {
    OpenFolder,
    OpenFile,
    SaveAs,
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
    RenameProject(PathBuf),
    BranchAt(String),
    NewBranch,
    /// New name of the symbol under the cursor (LSP rename).
    RenameSymbol,
    /// Trigger of a new template, with its body (the selection) and the extension of the file.
    NewTemplate { body: String, ext: String },
}

#[derive(Clone)]
enum Mode {
    Commands,
    Files,
    GotoLine,
    Syntax,
    Themes,
    Branches,
    Projects,
    Symbols,
    /// Templates to insert at the cursor.
    Templates,
    /// Several definitions of a word: choose one.
    Definitions(String),
    /// Where a symbol is used (language server).
    References(String),
    Bookmarks,
    /// Machines to connect to (SSH hosts, WSL distributions).
    Remotes,
    Prompt(Prompt),
}

#[derive(Clone)]
enum Action {
    Cmd(Cmd),
    /// Open entry n of `App::file_index` (go to file: no full path kept per entry, there may be a million).
    OpenIndexed(usize),
    Syntax(String),
    Theme(String),
    Branch(String),
    Project(PathBuf),
    /// Go to a line of the current file (symbol list).
    Line(usize),
    /// Open a file at a line (definitions).
    Location(PathBuf, usize),
    /// Insert template n (index in `App::templates`).
    Template(usize),
    /// Connect to another machine, then choose a folder there.
    Remote(crate::remote::Target),
}

struct Item {
    label: String,
    detail: String,
    action: Action,
}

struct Overlay {
    mode: Mode,
    query: String,
    selected: usize,
    focus: bool,
    /// Shared: go to file reuses the list of the whole project (up to a million files).
    items: Arc<[Item]>,
    filtered: Vec<usize>,
    last_query: Option<String>,
    /// Added to the score of some items (go to file: the recent files), empty for the other lists.
    boost: HashMap<usize, i32>,
    /// Every item matching `.0` (not only the shown ones): a longer query only filters these.
    matched: Option<(String, Arc<Vec<usize>>)>,
    /// Number of the last query scored in the background (big lists): an older job stops, an
    /// older result is ignored.
    gen: Arc<AtomicU64>,
    /// A query is being scored in the background: the results shown are the previous ones.
    pending: bool,
    /// Path prompts: entries of the folder being typed, for the query they were listed for.
    paths: Option<(String, Vec<(String, bool)>)>,
}

/// Completion of a path typed in a prompt, like Tab in a shell: the entries of the folder being
/// typed that start with the last component, and the input completed as far as they agree
/// (with a separator after a single folder). `base` resolves relative paths.
fn complete_path(input: &str, base: &Path, dirs_only: bool) -> (String, Vec<(String, bool)>) {
    let cut = input.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let (dir_part, prefix) = input.split_at(cut);
    let dir = match dir_part.strip_prefix("~/") {
        Some(rest) => config::home().join(rest),
        None if dir_part == "~" => config::home(),
        None if dir_part.is_empty() => base.to_path_buf(),
        None if Path::new(dir_part).is_absolute() => PathBuf::from(dir_part),
        None => base.join(dir_part),
    };
    let entries: Vec<(String, bool)> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir())).collect())
        .unwrap_or_default();
    let pick = |exact: bool| -> Vec<(String, bool)> {
        let lower = prefix.to_lowercase();
        entries
            .iter()
            .filter(|(n, d)| (!dirs_only || *d) && if exact { n.starts_with(prefix) } else { n.to_lowercase().starts_with(&lower) })
            // hidden entries only when asked for with a dot
            .filter(|(n, _)| !n.starts_with('.') || prefix.starts_with('.'))
            .cloned()
            .collect()
    };
    let mut found = pick(true);
    if found.is_empty() {
        found = pick(false);
    }
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    let completed = match found.as_slice() {
        [] => input.to_string(),
        [(n, true)] => format!("{dir_part}{n}{}", std::path::MAIN_SEPARATOR),
        [(n, false)] => format!("{dir_part}{n}"),
        many => {
            let first = &many[0].0;
            let mut len = first.len();
            for (n, _) in &many[1..] {
                len = first.char_indices().zip(n.chars()).take_while(|((_, a), b)| a == b).last().map_or(0, |((i, a), _)| i + a.len_utf8()).min(len);
            }
            if len >= prefix.len() { format!("{dir_part}{}", &first[..len]) } else { input.to_string() }
        }
    };
    (completed, found)
}

fn recent_cmds_path() -> PathBuf {
    config::config_dir().join("recent_commands")
}

/// Ids of the commands run from the palette lately (one per line), the unknown ones left out.
fn load_recent_cmds() -> Vec<&'static str> {
    let text = std::fs::read_to_string(recent_cmds_path()).unwrap_or_default();
    text.lines().filter_map(|l| commands::ALL.iter().find(|(id, ..)| *id == l.trim()).map(|(id, ..)| *id)).take(20).collect()
}

/// Message of the status bar.
#[derive(Clone)]
struct StatusMsg {
    text: String,
    at: Instant,
    error: bool,
}

impl StatusMsg {
    /// Still shown in the status bar: an error stays longer.
    fn shown(&self) -> bool {
        self.at.elapsed() < Duration::from_secs(if self.error { 15 } else { 6 })
    }
}

enum Confirm {
    CloseTab(u64),
    Quit,
    Delete(PathBuf),
    SwitchProject(PathBuf),
    ReplaceInFiles,
    /// Irreversible change requested from a diff (discarding uncommitted changes).
    DiffOp { msg: String, label: String, op: DiffOp },
    /// Deleting a stash: (its commit, what it is).
    DropStash(String, String),
    /// Saving a document whose file was changed by another program meanwhile.
    Overwrite(u64),
}

/// Change to the files or the index requested from a diff.
enum DiffOp {
    /// `git apply <flags> --recount -` with this patch.
    Apply { patch: String, flags: Vec<&'static str> },
    Git(Vec<String>),
    Delete(PathBuf),
}

/// What a diff shows: unstaged changes, staged changes, or a commit.
#[derive(Clone, Copy, PartialEq)]
enum DiffSource {
    Worktree,
    Index,
    Commit,
    /// Difference between two commits: read only.
    Compare,
}

#[derive(Default)]
struct FindState {
    open: bool,
    replace_mode: bool,
    query: String,
    replacement: String,
    case: bool,
    regex: bool,
    whole: bool,
    matches: Vec<(usize, usize)>,
    key: Option<(u64, u64, String, bool, bool, bool, Option<(usize, usize)>)>,
    error: Option<String>,
    focus: bool,
    /// Search only inside the range selected when the option was turned on (`Document::find_scope`).
    in_sel: bool,
    /// Replace keeping the case of each match.
    preserve: bool,
}

enum FindAction {
    Incremental,
    Next,
    Prev,
    All,
    ReplaceOne,
    ReplaceAll,
}

#[derive(Default)]
struct SearchUi {
    q: search::Query,
    replacement: String,
    show_replace: bool,
    files: Vec<FileHits>,
    /// (file index, hit index or None for the file header)
    rows: Vec<(usize, Option<usize>)>,
    /// Query that produced `files` (used to replace, even if the fields were edited since).
    done: search::Query,
    /// Files scanned, duration (ms), hit limit reached.
    stats: Option<(usize, u128, bool)>,
    error: Option<String>,
    running: bool,
    gen: Arc<AtomicU64>,
    focus: bool,
    cursor: Option<(usize, usize)>,
    /// Position in the search history while browsing it with ↑ / ↓.
    hist_pos: Option<usize>,
    /// Files whose hits are folded (indices in `files`).
    collapsed: HashSet<usize>,
    /// Scroll the results so that `cursor` is visible (set when it changes, e.g. with F4).
    scroll_to_cursor: bool,
    /// Syntax colors of the result lines, computed when first shown: (file, hit) -> spans.
    colors: HashMap<(usize, usize), Vec<(Color32, std::ops::Range<usize>)>>,
    /// Syntax of each result file.
    syntaxes: HashMap<usize, String>,
}

/// Appends `text[a..b]` to `job` with the syntax colors of `spans` (byte ranges of `text`).
fn append_colored(job: &mut LayoutJob, text: &str, a: usize, b: usize, spans: &[(Color32, std::ops::Range<usize>)], font: &egui::FontId, default: Color32) {
    let mut pos = a;
    for (c, r) in spans {
        let (x, y) = (r.start.max(pos), r.end.min(b));
        if x >= y {
            continue;
        }
        if x > pos {
            job.append(&text[pos..x], 0.0, TextFormat::simple(font.clone(), default));
        }
        job.append(&text[x..y], 0.0, TextFormat::simple(font.clone(), *c));
        pos = y;
    }
    if pos < b {
        job.append(&text[pos..b], 0.0, TextFormat::simple(font.clone(), default));
    }
}

/// Something clicked in the search results, applied once the panel is drawn.
enum SearchAct {
    Open(usize, usize),
    Fold(usize),
    Dismiss(usize, Option<usize>),
    Replace(usize, Option<usize>),
    ReplaceAll,
    Reveal(usize),
    Copy(String),
    /// Into an editor tab (true) or back into the bottom panel.
    Move(bool),
}

impl SearchUi {
    fn rebuild_rows(&mut self) {
        let collapsed = &self.collapsed;
        self.rows = self
            .files
            .iter()
            .enumerate()
            .flat_map(|(fi, f)| {
                let n = if collapsed.contains(&fi) { 0 } else { f.hits.len() };
                std::iter::once((fi, None)).chain((0..n).map(move |h| (fi, Some(h))))
            })
            .collect();
    }

    fn hit_count(&self) -> usize {
        self.files.iter().map(|f| f.hits.len()).sum()
    }

    /// Removes a hit (or a whole file with `hit == None`) from the results: it will not be replaced.
    fn dismiss(&mut self, fi: usize, hit: Option<usize>) {
        let Some(f) = self.files.get_mut(fi) else { return };
        let drop_file = match hit {
            Some(h) if h < f.hits.len() => {
                f.hits.remove(h);
                f.partial = true;
                f.hits.is_empty()
            }
            Some(_) => false,
            None => true,
        };
        if drop_file {
            self.files.remove(fi);
            self.collapsed = self.collapsed.iter().filter(|&&c| c != fi).map(|&c| if c > fi { c - 1 } else { c }).collect();
        }
        self.cursor = None;
        self.colors.clear();
        self.syntaxes.clear();
        self.rebuild_rows();
    }
}

/// Commits loaded in the Git view; up to `HISTORY_MAX` to find an older one asked for.
const HISTORY_LIMIT: usize = 3000;
const HISTORY_MAX: usize = 100_000;

#[derive(Default)]
struct HistoryUi {
    /// Number of the last load: results of older ones are dropped.
    gen: u64,
    /// Commits loaded (0: the default); raised to find a commit asked for that is older.
    limit: usize,
    /// Scroll the commit list to the selected commit at the next frame.
    scroll_to_sel: bool,
    /// `select_hash` was asked for ("voir dans le graphe"): more commits are loaded to find it.
    seek: bool,
    repo: Option<PathBuf>,
    graph: Option<history::Graph>,
    error: Option<String>,
    loading: bool,
    all: bool,
    filter: String,
    selected: Option<usize>,
    details: Option<history::Details>,
    /// Diff shown under the commit list.
    diff: Option<HistoryDiff>,
    diff_loading: bool,
    /// Commit to select once the graph is loaded ("voir dans le graphe").
    select_hash: Option<String>,
    /// Message shown above the graph (e.g. why a split was refused).
    notice: Option<(String, Color32)>,
    /// Staged (true) / unstaged (false) changes selected instead of a commit.
    wip_sel: Option<bool>,
    /// Comparison shown instead of a commit: (from, to, title).
    compare: Option<(String, String, String)>,
    /// Interactive rebase being prepared.
    rebase: Option<RebaseUi>,
    /// Folded files of the diff (index of their "diff --git" line).
    collapsed: HashSet<usize>,
    /// Scroll the diff to a file / by a page at the next frame (arrow keys, file list).
    jump: Option<history::FileJump>,
    /// File to scroll to once the diff is loaded (a file clicked in the Git panel).
    jump_path: Option<String>,
}

/// A place in a document, for the cursor history (Alt+- / Alt++).
#[derive(Clone)]
struct NavPos {
    uid: u64,
    path: Option<PathBuf>,
    pos: usize,
}

/// Suggestions of the language server, filtered while typing.
struct CompletionUi {
    uid: u64,
    /// Start of the word being completed.
    start: usize,
    items: Vec<lsp::CompletionItem>,
    /// Indices in `items` matching the typed word, best first.
    shown: Vec<usize>,
    selected: usize,
    #[allow(dead_code)]
    incomplete: bool,
    client: usize,
}

/// What a pending LSP request was for.
enum LspReq {
    Completion { uid: u64, start: usize },
    /// The word is searched in the project index when the server finds nothing.
    Definition { word: String },
    Rename { name: String },
    /// Tooltip of the symbol at `range` in document `uid`.
    Hover { uid: u64, range: (usize, usize) },
    References { word: String },
    /// Code actions for the corrections of document `uid`.
    CodeActions { uid: u64 },
    /// A code action sent without its edit, completed by the server.
    ResolveAction,
    /// workspace/executeCommand: its edit comes as a workspace/applyEdit.
    Command,
}

/// Interactive rebase prepared in the graph (see rebase.rs).
struct RebaseUi {
    from: String,
    steps: Vec<crate::rebase::Step>,
}

/// Commit split assistant (see split.rs).
struct SplitUi {
    repo: PathBuf,
    info: split::Info,
    files: Vec<split::FileChange>,
    /// Chosen lines for the current part: per file, per hunk, per line of the hunk (only +/- lines count).
    sel: Vec<Vec<Vec<bool>>>,
    folded: HashSet<String>,
    message: String,
    /// Commits made so far: (short hash, subject).
    parts: Vec<(String, String)>,
    busy: bool,
    error: Option<String>,
    confirm_abort: bool,
}

impl SplitUi {
    fn set_files(&mut self, files: Vec<split::FileChange>) {
        self.sel = files.iter().map(|f| f.empty_selection()).collect();
        self.files = files;
    }
}

struct LineHistoryUi {
    title: String,
    loading: bool,
    error: Option<String>,
    commits: Vec<git::LineCommit>,
    diffs: Vec<Vec<history::DiffLine>>,
    selected: usize,
}

struct HistoryDiff {
    hash: String,
    path: Option<String>,
    text: String,
    lines: Vec<history::DiffLine>,
    /// Files of the diff, in order.
    files: Vec<history::DiffFile>,
}

#[derive(Default)]
struct GitUi {
    status: Option<git::Status>,
    /// Claude is writing the commit message.
    suggesting: bool,
    /// Claude is splitting the changes into commits by theme, and the plan it made (to read
    /// over before committing).
    planning: bool,
    plan: Option<commit_plan::CommitPlan>,
    error: Option<String>,
    commit_msg: String,
    busy: usize,
    /// Last failed git command (label, message): shown until dismissed or a later command succeeds.
    op_error: Option<(String, String)>,
    focus_commit: bool,
    /// AI review of the changes not committed yet.
    ai: ai_review::AiReview,
    /// Stashes of the repository (read when the Stash menu opens and after each stash command).
    stashes: Vec<git::Stash>,
    /// Message of the next stash, and whether the untracked files stay out of it.
    stash_msg: String,
    stash_tracked_only: bool,
    /// The stash menu was open at the last frame (the stashes are read when it opens).
    stash_shown: bool,
    /// absolute path -> status letter (directories inherit from their files)
    map: HashMap<PathBuf, char>,
}

/// Cursor history (back / forward between places).
#[derive(Default)]
struct NavState {
    /// Cursor history: previous / next places, and the place seen at the last frame.
    back: Vec<NavPos>,
    fwd: Vec<NavPos>,
    last: Option<(NavPos, usize)>,
    /// The cursor moved because of Alt+- / Alt++: do not record it.
    jumping: bool,
}

/// Indexes of the project: its files (go to file) and its symbols.
#[derive(Default)]
struct Indexes {
    /// Files of the project, relative and sorted (recent files are found by binary search);
    /// shared with the job that lists them again, which compares without the interface.
    files: Arc<Vec<PathBuf>>,
    /// Go to file entries made from `files`, built once for each new list.
    file_items: Option<Arc<[Item]>>,
    /// When the file list was last asked for (Ctrl+P rebuilds it, at most every few seconds).
    files_at: Option<Instant>,
    /// Project symbols (go to definition / completion without a language server).
    symbols: Option<crate::index::Index>,
    /// The project index is being built for this folder.
    symbols_for: Option<PathBuf>,
    /// Files of the project opened lately, most recent first (go to file puts them first).
    recent: Vec<PathBuf>,
}

/// Language servers and what is waiting for them.
#[derive(Default)]
struct LspUi {
    manager: lsp::Manager,
    requests: HashMap<(usize, i64), LspReq>,
    completion: Option<CompletionUi>,
    /// Ask the server for suggestions at this moment (debounce while typing).
    complete_at: Option<Instant>,
    /// Why a language has no server (status bar).
    notes: HashMap<String, String>,
    /// Hovered symbol whose tooltip was asked to the server (document uid, range).
    hover_pending: Option<(u64, (usize, usize))>,
}

pub struct App {
    settings: Settings,
    hl: Highlighting,
    docs: Vec<Document>,
    active: usize,
    uid_counter: u64,
    root: Option<PathBuf>,
    /// Selected repository (git panel, history, status bar).
    repo: Option<PathBuf>,
    /// All repositories found in the opened folder.
    repos: Vec<PathBuf>,
    history: HistoryUi,
    split: Option<SplitUi>,
    last_active_path: Option<PathBuf>,
    tree: Option<Node>,
    show_sidebar: bool,
    bottom: Option<BottomTab>,
    /// The exclusions of the settings for the open folder (excluded folders are dimmed in the tree).
    tree_ex: Option<(PathBuf, Vec<String>, crate::walk::Exclusions)>,
    /// Tab of the bottom panel shown last: the panel button reopens it.
    last_bottom: BottomTab,
    overlay: Option<Overlay>,
    find: FindState,
    search: SearchUi,
    projects: Vec<Project>,
    last_session_save: Instant,
    jobs: Jobs,
    git: GitUi,
    term: Terminal,
    /// Terminal the installation guides type their commands in (`install_guide`).
    install_term: Option<u64>,
    plugins: Vec<Plugin>,
    plugin_cmds: Vec<(usize, usize)>,
    keymap: Keymap,
    chord: bool,
    confirm: Option<Confirm>,
    /// Commands run from the palette lately, most recent first (shown first there).
    recent_cmds: Vec<&'static str>,
    output: Vec<String>,
    status: Option<StatusMsg>,
    /// The last messages of the status bar, newest last: a message gone too fast can be read again.
    status_log: std::collections::VecDeque<StatusMsg>,
    /// Newer release of Florm, shown in the status bar until dismissed.
    update: Option<crate::update::Update>,
    /// The update is being downloaded and installed.
    update_installing: bool,
    /// The shared configuration is being synchronized / published.
    shared_busy: bool,
    /// Asked from the preferences, done once the setting is saved.
    shared_op: Option<crate::shared::Op>,
    /// Installed update: the program to start for the restart.
    update_ready: Option<PathBuf>,
    /// Program started when Florm quits ("Redémarrer" after an update).
    restart: Option<PathBuf>,
    /// Start of the last frame, when there is no vsync to pace the frames (Wayland).
    paced_frame: Option<Instant>,
    pending: Vec<Cmd>,
    last_disk_check: Instant,
    /// Last command run in a terminal: git and the files are checked more often for a while.
    term_enter: Option<Instant>,
    last_line_copy: Option<String>,
    quit_confirmed: bool,
    closed: Vec<PathBuf>,
    last_title: String,
    visuals_dirty: bool,
    /// Tab last scrolled into view (to follow the active tab once).
    tab_scrolled: Option<usize>,
    /// Tab being dragged to another position.
    tab_drag: Option<usize>,
    /// The tab shown before the current one (Échap on Git or the preferences goes back to it).
    prev_tab: Option<u64>,
    /// Split editor: orientation, panel with the keyboard focus, document shown by each panel.
    split_dir: SplitDir,
    focused_pane: u8,
    pane_active: [Option<u64>; 2],
    drop_zones: DropZones,
    /// Give the keyboard focus to the editor at the next frame (after a tab change).
    pending_focus: bool,
    md_cache: egui_commonmark::CommonMarkCache,
    /// Claude on the selection, in a small window over the editor (Ctrl+I).
    inline: Option<inline_ai::InlineAsk>,
    /// Corrections rapides (Ctrl+.) of the cursor, while their list is open.
    quick_fix: Option<quick_fix::QuickFix>,
    /// GitHub pull requests tab.
    pr: pulls::PrUi,
    docker: docker_view::DockerUi,
    tests: odoo_tests::Tests,
    /// The debugger (F5): its session, the breakpoints of the project.
    debug: debugger::DebugUi,
    /// Pull requests of branches and commits, creation of one.
    gh: pr_links::GhUi,
    /// Conversation with Claude Code and review of its changes: the one shown last.
    claude: claude_ui::ClaudeUi,
    /// The other conversations (each in its editor tab, or one in the bottom panel).
    claude_others: Vec<claude_ui::ClaudeUi>,
    /// The conversations kept between sessions (newest first), and whether the last one was
    /// brought back at the first showing of Claude.
    claude_saved: Vec<claude_ui::SavedConv>,
    claude_loaded: bool,
    /// Use of the plan's quotas (5 hours, 7 days), shared by the conversations and kept between
    /// sessions.
    claude_limits: Vec<(String, f32, i64)>,
    /// Slash commands of Claude Code (the last known, kept between sessions): sent as a message,
    /// and those of its terminal interface only.
    claude_commands: claude_ui::Commands,
    /// For the places without a context at hand (prompt submission).
    egui_ctx: egui::Context,
    /// The window has no system title bar: Florm draws its own (`title_bar`).
    title_bar_on: bool,
    /// File to bring into view in the tree ("afficher dans l'arborescence").
    tree_scroll: Option<PathBuf>,
    /// Keyboard cursor of the tree.
    tree_nav: sidebar::Nav,
    /// Branch / upstream (label) and remotes (tooltip) of each repository, shown on its folder in the tree.
    repo_info: HashMap<PathBuf, (String, String)>,
    /// The "go to line" prompt was opened by Alt+G: select the line(s).
    goto_select: bool,
    /// Ignore the letter of an Alt shortcut if it arrives just after it.
    drop_alt_text: Option<Instant>,
    /// A native file dialog is open (do not open a second one).
    dialog_open: bool,
    /// Ctrl+B: blame annotation following the cursor line.
    blame_mode: bool,
    blame_req: Option<(u64, usize, u64)>,
    line_history: Option<LineHistoryUi>,
    /// Search history and last filters, shared by all projects.
    state: UiState,
    /// Documents with a live lint running.
    lint_inflight: HashSet<u64>,
    /// Python linter actually used for the last lint ("flake8", "ruff"… or "syntaxe").
    python_linter: Option<String>,
    prefs: PrefsUi,
    /// Templates (snippets) offered by "insérer un modèle", loaded when the list opens.
    templates: Vec<crate::templates::Template>,
    /// Last `git::stamp` of each repository: a change made outside Florm (commit in a terminal…)
    /// refreshes the git panel and the gutter marks.
    git_stamps: HashMap<PathBuf, Vec<Option<std::time::SystemTime>>>,
    /// The window had the focus at the last frame (coming back to Florm checks the files at once).
    was_focused: bool,
    nav: NavState,
    index: Indexes,
    lsp: LspUi,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrefSection {
    Appearance,
    Editor,
    Lint,
    Git,
    PullRequests,
    Claude,
    Docker,
    Debug,
    Terminal,
    Keys,
    Templates,
    Advanced,
}

/// (section, French name, English name).
const PREF_SECTIONS: [(PrefSection, &str, &str); 12] = [
    (PrefSection::Appearance, "Apparence", "Appearance"),
    (PrefSection::Editor, "Éditeur", "Editor"),
    (PrefSection::Lint, "Lint", "Lint"),
    (PrefSection::Git, "Git", "Git"),
    (PrefSection::PullRequests, "Pull requests", "Pull requests"),
    (PrefSection::Claude, "Claude", "Claude"),
    (PrefSection::Docker, "Docker", "Docker"),
    (PrefSection::Debug, "Débogueur", "Debugger"),
    (PrefSection::Terminal, "Terminal", "Terminal"),
    (PrefSection::Keys, "Raccourcis", "Shortcuts"),
    (PrefSection::Templates, "Modèles", "Templates"),
    (PrefSection::Advanced, "Avancé", "Advanced"),
];

#[derive(Clone, Copy)]
struct Capture {
    cmd: Cmd,
    /// first key of a `ctrl+k` chord
    first: Option<Combo>,
}

#[derive(Clone, Copy)]
enum KeyAction {
    Capture(Cmd),
    Unbind(Cmd),
    Reset(Cmd),
    Confirm,
    Cancel,
}

struct PrefsUi {
    /// shortcut being recorded
    capture: Option<Capture>,
    /// shortcut waiting for confirmation because it is already used
    proposed: Option<(Cmd, Seq)>,
    key_action: Option<KeyAction>,
    section: PrefSection,
    filter: String,
    /// text buffers edited before being saved
    lint_args: String,
    /// Codes ignored by each linter of `lint::WITH_CODES` (comma separated).
    lint_ignore: [String; 4],
    /// The theme being made, shown live (the one chosen before stays in the settings).
    theme_draft: Option<theme_edit::ThemeDraft>,
    /// The template being edited in Préférences → Modèles.
    template: Option<template_edit::TemplateDraft>,
    shell: String,
    /// `exclude` setting being edited (comma separated).
    exclude: String,
    /// `shared_config` setting being edited.
    shared: String,
    /// `rulers` setting being edited (comma separated columns).
    rulers: String,
    /// Fonts of the system, for the preferences (None until listed).
    font_list: Option<Vec<crate::fonts::FontInfo>>,
    font_list_loading: bool,
}

impl Default for PrefsUi {
    fn default() -> Self {
        PrefsUi { capture: None, proposed: None, key_action: None, section: PrefSection::Appearance, filter: String::new(), lint_args: String::new(), lint_ignore: Default::default(), theme_draft: None, template: None, shell: String::new(), exclude: String::new(), shared: String::new(), rulers: String::new(), font_list: None, font_list_loading: false }
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, arg: Option<PathBuf>) -> Self {
        Self::with_ctx(&cc.egui_ctx, arg)
    }

    /// `new` with only the egui context (the tests make an app without a window).
    fn with_ctx(egui_ctx: &egui::Context, arg: Option<PathBuf>) -> Self {
        let (settings, settings_err) = config::load();
        // Florm has its own palette: egui must not switch to its light / dark style with the system
        // theme (Windows), and both styles get the same settings
        egui_ctx.set_theme(egui::ThemePreference::Dark);
        egui_ctx.all_styles_mut(|s| {
            use egui::{FontId, TextStyle};
            s.spacing.item_spacing = egui::vec2(8.0, 5.0);
            s.spacing.button_padding = egui::vec2(8.0, 3.0);
            s.spacing.interact_size.y = 22.0;
            s.spacing.menu_margin = egui::Margin::same(8.0);
            s.spacing.window_margin = egui::Margin::same(12.0);
            s.spacing.scroll = egui::style::ScrollStyle::floating();
            s.text_styles.insert(TextStyle::Body, FontId::proportional(14.0));
            s.text_styles.insert(TextStyle::Button, FontId::proportional(14.0));
            s.text_styles.insert(TextStyle::Small, FontId::proportional(11.5));
            s.text_styles.insert(TextStyle::Heading, FontId::proportional(19.0));
            s.text_styles.insert(TextStyle::Monospace, FontId::monospace(13.5));
        });
        plugins::install_examples(&config::plugins_dir());
        let mut hl = Highlighting::new(&settings.theme, &config::config_dir().join("themes"));
        if let Some(d) = crate::shared::sub("themes") {
            hl.add_folder(&d);
            hl.set_theme(&settings.theme);
        }
        hl.set_overrides(highlight::parse_hex(&settings.accent_color), settings.text_softness);
        apply_visuals(egui_ctx, hl.visuals());
        egui_ctx.set_fonts(crate::fonts::definitions(&settings.editor_font, &settings.ui_font));
        egui_extras::install_image_loaders(egui_ctx);
        let (keymap, _) = Keymap::new(&settings.keys, &[]);
        let mut app = App {
            hl,
            docs: Vec::new(),
            active: 0,
            uid_counter: 0,
            root: None,
            repo: None,
            repos: Vec::new(),
            history: HistoryUi::default(),
            split: None,
            last_active_path: None,
            tree: None,
            show_sidebar: true,
            bottom: None,
            tree_ex: None,
            last_bottom: BottomTab::Terminal,
            overlay: None,
            find: FindState::default(),
            search: SearchUi::default(),
            projects: project::load_list(),
            last_session_save: Instant::now(),
            jobs: Jobs::new(egui_ctx.clone()),
            git: GitUi::default(),
            term: Terminal::new(),
            install_term: None,
            plugins: Vec::new(),
            plugin_cmds: Vec::new(),
            keymap,
            chord: false,
            confirm: None,
            recent_cmds: load_recent_cmds(),
            output: Vec::new(),
            status: None,
            status_log: Default::default(),
            update: None,
            update_installing: false,
            shared_busy: false,
            shared_op: None,
            update_ready: None,
            restart: None,
            paced_frame: std::env::var_os("WAYLAND_DISPLAY").is_some().then(Instant::now),
            pending: Vec::new(),
            last_disk_check: Instant::now(),
            term_enter: None,
            last_line_copy: None,
            quit_confirmed: false,
            closed: Vec::new(),
            last_title: String::new(),
            visuals_dirty: false,
            tab_scrolled: None,
            tab_drag: None,
            prev_tab: None,
            split_dir: SplitDir::Right,
            focused_pane: 0,
            pane_active: [None; 2],
            drop_zones: DropZones::default(),
            pending_focus: false,
            md_cache: egui_commonmark::CommonMarkCache::default(),
            inline: None,
            quick_fix: None,
            pr: {
                let mut pr = pulls::PrUi::default();
                pr.apply_settings(&settings.pr_default_state, settings.pr_show_resolved);
                pr
            },
            docker: Default::default(),
            tests: Default::default(),
            debug: Default::default(),
            gh: Default::default(),
            claude: Default::default(),
            claude_others: Vec::new(),
            claude_saved: claude_ui::load_saved(),
            claude_loaded: false,
            claude_limits: claude_ui::load_limits(),
            claude_commands: claude_ui::load_commands(),
            egui_ctx: egui_ctx.clone(),
            title_bar_on: crate::os::draws_title_bar(settings.show_menu_bar),
            tree_scroll: None,
            tree_nav: sidebar::Nav::default(),
            repo_info: HashMap::new(),
            goto_select: false,
            drop_alt_text: None,
            dialog_open: false,
            blame_mode: false,
            blame_req: None,
            line_history: None,
            state: project::load_state(),
            lint_inflight: HashSet::new(),
            python_linter: None,
            prefs: PrefsUi::default(),
            nav: NavState::default(),
            index: Indexes::default(),
            lsp: LspUi::default(),
            templates: Vec::new(),
            git_stamps: HashMap::new(),
            was_focused: true,
            settings,
        };
        if let Some(e) = settings_err {
            app.log(e);
        }
        app.search = app.fresh_search();
        crate::update::cleanup();
        if app.settings.check_updates {
            app.check_update(false);
        }
        if !app.settings.shared_config.trim().is_empty() {
            app.shared_job(crate::shared::Op::Sync);
        }
        let cwd = std::env::current_dir().unwrap_or_default();
        let last_project = app.projects.iter().find(|p| p.path.is_dir() || crate::remote::ssh_location(&p.path).is_some()).map(|p| p.path.clone());
        match arg {
            Some(p) if p.is_dir() => app.open_project(p),
            Some(p) => {
                if cwd != config::home() {
                    app.open_project(cwd);
                } else {
                    app.reload_plugins();
                }
                let p = if p.is_absolute() { p } else { std::env::current_dir().unwrap_or_default().join(p) };
                app.open_path(p);
            }
            // Without arguments: reopen the last project, like Sublime.
            None => match last_project {
                Some(p) => app.open_project(p),
                None if cwd != config::home() && cwd.parent().is_some() => app.open_project(cwd),
                None => app.reload_plugins(),
            },
        }
        app
    }

    // ------------------------------------------------------------------ helpers

    fn next_uid(&mut self) -> u64 {
        self.uid_counter += 1;
        self.uid_counter
    }

    fn log(&mut self, s: impl Into<String>) {
        let s: String = s.into();
        self.output.extend(s.lines().map(String::from));
        let n = self.output.len();
        if n > 5000 {
            self.output.drain(..n - 5000);
        }
    }

    fn set_status(&mut self, s: impl Into<String>) {
        self.push_status(s.into(), false);
    }

    /// A failure: shown in red, and longer than the other messages.
    fn set_error(&mut self, s: impl Into<String>) {
        self.push_status(s.into(), true);
    }

    fn push_status(&mut self, text: String, error: bool) {
        let m = StatusMsg { text, at: Instant::now(), error };
        if self.status_log.len() >= 50 {
            self.status_log.pop_front();
        }
        self.status_log.push_back(m.clone());
        self.status = Some(m);
    }

    /// Looks for a newer release in the background; `manual` also reports "up to date" and errors.
    fn check_update(&mut self, manual: bool) {
        if manual {
            self.set_status(t!("Recherche d'une mise à jour…", "Checking for an update…"));
        }
        self.jobs.spawn(move || JobResult::Update { manual, result: crate::update::check() });
    }

    /// Downloads and installs the update in the background; "Redémarrer" is offered when done.
    fn install_update(&mut self) {
        let Some(u) = self.update.clone() else { return };
        if self.update_installing || self.update_ready.is_some() {
            return;
        }
        self.update_installing = true;
        self.set_status(tf!("Téléchargement de Florm {}…", "Downloading Florm {}…", u.version));
        self.jobs.spawn(move || JobResult::UpdateInstalled { version: u.version.clone(), result: crate::update::install(&u) });
    }

    /// Quits (with the usual questions for unsaved files) and starts the installed version.
    fn restart_for_update(&mut self, ctx: &egui::Context) {
        if let Some(exe) = self.update_ready.clone() {
            self.restart = Some(exe);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn focus_editor(&self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(editor_id()));
    }

    fn new_doc(&mut self) {
        let uid = self.next_uid();
        self.docs.push(Document::new(uid, self.settings.tab_size, self.settings.translate_tabs_to_spaces));
        self.active = self.docs.len() - 1;
    }

    fn new_scratch(&mut self, title: String, text: String, syntax: &str, read_only: bool) {
        let uid = self.next_uid();
        let mut d = Document::from_text(uid, &text, self.settings.tab_size, self.settings.translate_tabs_to_spaces);
        d.title_override = Some(title);
        d.syntax = syntax.to_string();
        d.read_only = read_only;
        self.docs.push(d);
        self.active = self.docs.len() - 1;
    }

    fn resolve(&self, p: PathBuf) -> PathBuf {
        let p = match p.to_str().and_then(|s| s.strip_prefix("~/")) {
            Some(rest) => config::home().join(rest),
            None => p,
        };
        if p.is_absolute() {
            p
        } else {
            self.root.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default()).join(p)
        }
    }

    fn open_path(&mut self, path: PathBuf) -> bool {
        let path = crate::os::canon(&path);
        if path.is_dir() {
            self.switch_project(path);
            return true;
        }
        // the file is shown even if the graph, the preferences or a commit split was on screen
        if let Some(i) = self.docs.iter().position(|d| d.path.as_ref() == Some(&path)) {
            self.activate_tab(i);
            return true;
        }
        let uid = self.next_uid();
        match Document::open(uid, &path, self.settings.tab_size, self.settings.translate_tabs_to_spaces, &self.hl) {
            Ok(d) => {
                // an empty document, or the welcome page, gives its place to the file
                if self.docs.len() == 1 && (self.docs[0].is_pristine_empty() || self.docs[0].view == document::View::Welcome) {
                    self.docs.clear();
                }
                self.docs.push(d);
                self.activate_tab(self.docs.len() - 1);
                self.analyze(self.active, false);
                true
            }
            Err(e) => {
                self.set_error(tf!("Ouverture impossible : {e}", "Cannot open: {e}"));
                false
            }
        }
    }

    fn set_root(&mut self, path: PathBuf) {
        let path = crate::os::canon(&path);
        self.tree = Some(Node::new(path.clone(), true));
        self.term.cwd = Some(path.clone());
        self.root = Some(path.clone());
        project::touch(&mut self.projects, &path);
        project::save_list(&self.projects);
        // looking for the repositories reads many folders (slow on a WSL folder seen from
        // Windows): in the background, the window shows up at once
        self.repos.clear();
        self.repo = None;
        self.git = GitUi::default();
        self.history = HistoryUi::default();
        let r = path.clone();
        self.jobs.spawn(move || {
            let repos = git::find_repos(&r, 3);
            JobResult::Repos(r, repos)
        });
        self.index.files = Default::default();
        self.index.file_items = None;
        self.refresh_index();
        // the symbol index starts once the file list is done (they would compete for the disk)
        self.index.symbols = None;
        self.index.symbols_for = None;
        self.reload_plugins();
    }

    /// The innermost known repository containing `path`.
    fn repo_for(&self, path: &Path) -> Option<PathBuf> {
        self.repos.iter().filter(|r| path.starts_with(r)).max_by_key(|r| r.as_os_str().len()).cloned()
    }

    fn repo_label(&self, repo: &Path) -> String {
        match self.root.as_ref().and_then(|root| repo.strip_prefix(root).ok()) {
            Some(rel) if !rel.as_os_str().is_empty() => rel.display().to_string(),
            _ => repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }

    fn select_repo(&mut self, repo: PathBuf) {
        if self.repo.as_ref() == Some(&repo) {
            return;
        }
        let map = std::mem::take(&mut self.git.map);
        self.git = GitUi { map, ..Default::default() };
        self.repo = Some(repo);
        self.git_refresh();
        if self.git_shown() {
            self.load_history();
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(exe) = self.restart.take() {
            if let Err(e) = crate::os::command(&exe).stdin(std::process::Stdio::null()).spawn() {
                eprintln!("{}", tf!("Redémarrage de Florm impossible ({}) : {e}", "Cannot restart Florm ({}): {e}", exe.display()));
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Without vsync, an animation (scrolling, panel opening) would redraw as fast as the processor
        // allows: at most ~120 frames per second, and never later than an input (key, mouse).
        if let Some(last) = self.paced_frame {
            let el = last.elapsed();
            let gap = if ctx.input(|i| i.focused) { MIN_FRAME } else { BACKGROUND_FRAME };
            if el < gap && ctx.input(|i| i.raw.events.is_empty()) {
                std::thread::sleep(gap - el);
            }
            self.paced_frame = Some(Instant::now());
        }
        if std::mem::take(&mut self.visuals_dirty) {
            apply_visuals(ctx, self.hl.visuals());
        }
        self.process_jobs();
        // the update is installed and nothing is unsaved: Florm restarts by itself, without asking
        if self.update_ready.is_some() && self.restart.is_none() && !self.docs.iter().any(|d| d.is_dirty()) {
            self.restart_for_update(ctx);
        }
        self.sync_decorations(ctx);
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for p in dropped {
            self.open_path(p);
        }
        // The git panel follows the repository of the active file.
        let cur = self.docs.get(self.active).and_then(|d| d.path.clone());
        if cur != self.last_active_path {
            if let Some(r) = cur.as_deref().and_then(|p| self.repo_for(p)) {
                self.select_repo(r);
            }
            if let Some(p) = cur.clone().filter(|p| self.root.as_ref().is_some_and(|r| p.starts_with(r))) {
                self.index.recent.retain(|q| *q != p);
                self.index.recent.insert(0, p);
                self.index.recent.truncate(50);
            }
            self.last_active_path = cur;
        }
        self.lsp_frame(ctx);
        self.debug_frame();
        self.update_hover(ctx);
        self.completion_keys(ctx);
        self.quick_fix_keys(ctx);
        self.handle_keys(ctx);
        self.editor_input(ctx);
        self.update_blame();
        self.update_live_lint(ctx);
        self.update_find();
        self.periodic(ctx);

        // Order matters: the sidebar takes the full height, the bottom panels and the
        // tab strip only span the editor area.
        self.menu_bar(ctx);
        self.status_bar(ctx);
        if self.show_sidebar {
            self.sidebar(ctx);
        } else {
            self.sidebar_strip(ctx);
        }
        if self.bottom.is_some() {
            self.bottom_panel(ctx);
        }
        if self.find.open {
            self.find_bar(ctx);
        }
        self.central(ctx);
        self.sync_terminal_tabs();
        // Ctrl+clic on a traceback in a terminal
        if let Some((p, line)) = self.term.open_request.take() {
            if p.is_file() {
                self.open_at(p, line.saturating_sub(1));
            } else {
                self.set_error(tf!("Fichier introuvable ici : {}", "File not found here: {}", p.display()));
            }
        }
        self.claude_poll_all();
        self.claude_take_picks();
        self.completion_popup(ctx);
        self.quick_fix_ui(ctx);
        let (mb, mf) = ctx.input(|i| (i.pointer.button_pressed(egui::PointerButton::Extra1), i.pointer.button_pressed(egui::PointerButton::Extra2)));
        if mb || mf {
            self.nav_go(mb);
        }
        self.track_nav();
        self.line_history_ui(ctx);
        self.pr_create_ui(ctx);
        self.inline_ai_ui(ctx);
        self.overlay_ui(ctx);
        self.confirm_ui(ctx);

        if let Some(d) = self.docs.get_mut(self.active) {
            if std::mem::take(&mut d.request_inline_diff) {
                self.pending.push(Cmd::ToggleInlineDiff);
            }
        }
        for c in std::mem::take(&mut self.pending) {
            self.exec(ctx, c);
        }
        if self.overlay.is_none()
            && self.confirm.is_none()
            && !self.showing(document::View::Git)
            && !self.showing(document::View::Settings)
            && ctx.memory(|m| m.focused().is_none())
        {
            self.focus_editor(ctx);
        }
        if self.title_bar_on {
            self.resize_edges(ctx);
        }
    }
}

/// What is drawn before the title of a tab.
#[derive(Clone, Copy)]
enum TabIcon<'a> {
    /// The badge of this file name.
    File(&'a str),
    View(icons::ViewIcon),
}

/// The icon of a document tab, and its title without the symbol older sessions put in front
/// of the views ("🔍 Recherche", ">_ bash").
fn doc_tab(d: &Document) -> (TabIcon<'_>, String) {
    use document::View;
    use icons::ViewIcon;
    let view = match d.view {
        View::Search => Some(ViewIcon::Search),
        View::Terminal(_) => Some(ViewIcon::Terminal),
        View::Help => Some(ViewIcon::Help),
        View::Welcome => Some(ViewIcon::Welcome),
        View::PullRequests => Some(ViewIcon::PullRequests),
        View::Docker => Some(ViewIcon::Docker),
        View::Debug => Some(ViewIcon::Debug),
        View::Git => Some(ViewIcon::Git),
        View::Settings => Some(ViewIcon::Settings),
        View::Split => Some(ViewIcon::Split),
        View::Claude => Some(ViewIcon::Claude),
        View::Text | View::Image | View::Preview(_) => None,
    };
    let title = d.title();
    match view {
        Some(v) => {
            let bare = ["🔍 ", ">_ ", "✳ ", "⇄ ", "❓ "].iter().find_map(|s| title.strip_prefix(s)).map(str::to_string);
            (TabIcon::View(v), bare.unwrap_or(title))
        }
        None => {
            let name = match (&d.path, d.view) {
                (_, View::Preview(_)) => "x.md",
                (Some(p), _) => p.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                (None, _) if d.syntax == "Diff" => "x.diff",
                (None, _) => "",
            };
            (TabIcon::File(name), title)
        }
    }
}

/// One editor tab: rounded top, active tab merged with the editor and underlined with
/// the theme accent gradient, close button (or "modified" dot).
fn tab_widget(
    ui: &mut egui::Ui,
    title: &str,
    icon: TabIcon,
    dirty: bool,
    active: bool,
    // color of the title instead of the usual one (errors, Claude working or finished)
    tint: Option<Color32>,
    tc: &crate::highlight::TabColors,
) -> (egui::Response, bool) {
    let font = egui::FontId::proportional(13.5);
    let text_color = tint.unwrap_or(if active { tc.text_active } else { tc.text_inactive });
    let galley = ui.painter().layout_no_wrap(title.to_string(), font, text_color);
    let icon_w = 22.0;
    let w = galley.size().x + 52.0 + icon_w;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, TAB_STRIP_H), Sense::click_and_drag());
    let tab = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + 4.0), rect.right_bottom());
    let close_rect = egui::Rect::from_center_size(egui::pos2(tab.right() - 17.0, tab.center().y), egui::vec2(18.0, 18.0));
    let close = ui.interact(close_rect, resp.id.with("close"), Sense::click());
    let hovered = resp.hovered() || close.hovered();
    let p = ui.painter();
    let rounding = egui::Rounding { nw: 8.0, ne: 8.0, sw: 0.0, se: 0.0 };
    if active {
        p.rect_filled(tab, rounding, tc.active);
        // thin accent gradient under the active tab
        let line = egui::Rect::from_min_max(egui::pos2(tab.left() + 10.0, tab.bottom() - 2.0), egui::pos2(tab.right() - 10.0, tab.bottom()));
        let mut mesh = egui::epaint::Mesh::default();
        mesh.colored_vertex(line.left_top(), tc.accent);
        mesh.colored_vertex(line.right_top(), tc.accent2);
        mesh.colored_vertex(line.right_bottom(), tc.accent2);
        mesh.colored_vertex(line.left_bottom(), tc.accent);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        p.add(egui::Shape::mesh(mesh));
    } else if hovered {
        p.rect_filled(tab, rounding, tc.hover);
    }
    let icon_at = egui::pos2(tab.left() + 20.0, tab.center().y);
    match icon {
        TabIcon::File(name) => {
            let bg = if active { tc.active } else { tc.bar };
            let light = bg.r() as u32 + bg.g() as u32 + bg.b() as u32 > 384;
            icons::paint_file(p, egui::Rect::from_center_size(icon_at, egui::vec2(15.0, 15.0)), name, bg, light);
        }
        TabIcon::View(v) => icons::paint_view(p, icon_at, v, text_color),
    }
    p.galley(egui::pos2(tab.left() + 13.0 + icon_w, tab.center().y - galley.size().y / 2.0), galley, text_color);
    if close.hovered() {
        p.rect_filled(close_rect, 4.0, tc.close_hover);
    }
    if dirty && !hovered {
        p.circle_filled(close_rect.center(), 4.0, text_color);
    } else if active || hovered || dirty {
        p.text(close_rect.center(), Align2::CENTER_CENTER, "×", egui::FontId::proportional(15.0), text_color);
    }
    // a thin line between two tabs (not around the active or the hovered one)
    if !active && !hovered {
        let x = rect.right() - 0.5;
        let c = tc.text_inactive.gamma_multiply(0.45);
        ui.painter().line_segment([egui::pos2(x, tab.top() + 7.0), egui::pos2(x, tab.bottom() - 5.0)], egui::Stroke::new(1.0, c));
    }
    let closed = close.on_hover_text(t!("Fermer (Ctrl+W)", "Close (Ctrl+W)")).clicked();
    (resp, closed)
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SplitDir {
    #[default]
    Right,
    Down,
}

/// Where a dragged tab would land.
enum TabDrop {
    /// In a panel's tab strip, before this index of `docs` (or at the end).
    Strip(u8, usize),
    /// Into the other panel.
    Pane(u8),
    /// Into a new panel.
    Split(SplitDir),
}

/// Drop targets of the current frame: tab strips (panel, strip, tabs with their index) and editor areas.
#[derive(Default)]
struct DropZones {
    strips: Vec<(u8, egui::Rect, Vec<(egui::Rect, usize)>)>,
    panes: Vec<(u8, egui::Rect)>,
}

/// Right-click menu of a tab.
enum TabAct {
    Split(usize, SplitDir),
    MoveOther(usize),
    Unsplit,
    Flip,
    Close(usize),
    CloseOthers(usize),
    CopyPath(String),
    Reveal(PathBuf),
    Locate(usize),
}

fn tab_menu(ui: &mut egui::Ui, i: usize, d: &Document, split: bool, alone: bool, act: &mut Option<TabAct>) {
    ui.set_min_width(240.0);
    let mut item = |ui: &mut egui::Ui, label: &str, a: TabAct, enabled: bool, tip: &str| {
        let r = ui.add_enabled(enabled, egui::Button::new(label));
        let r = if tip.is_empty() { r } else { r.on_disabled_hover_text(tip) };
        if r.clicked() {
            *act = Some(a);
            ui.close_menu();
        }
    };
    if split {
        item(ui, t!("Déplacer dans l'autre panneau", "Move to the other pane"), TabAct::MoveOther(i), true, "");
        item(ui, t!("Fusionner les panneaux", "Merge the panes"), TabAct::Unsplit, true, "");
        item(ui, t!("Changer l'orientation (côte à côte / l'un sous l'autre)", "Change the orientation (side by side / one below the other)"), TabAct::Flip, true, "");
    } else {
        let tip = t!("Ouvre un deuxième fichier d'abord : un fichier ne peut être que dans un panneau.", "Open a second file first: a file can only be in one pane.");
        item(ui, t!("Diviser à droite", "Split right"), TabAct::Split(i, SplitDir::Right), !alone, tip);
        item(ui, t!("Diviser en bas", "Split down"), TabAct::Split(i, SplitDir::Down), !alone, tip);
    }
    ui.separator();
    item(ui, t!("Fermer", "Close"), TabAct::Close(i), true, "");
    item(ui, t!("Fermer les autres onglets", "Close the other tabs"), TabAct::CloseOthers(i), !alone, "");
    if let Some(p) = &d.path {
        ui.separator();
        item(ui, t!("Afficher dans l'arborescence (Alt+L)", "Show in the tree (Alt+L)"), TabAct::Locate(i), true, "");
        item(ui, t!("Copier le chemin", "Copy the path"), TabAct::CopyPath(p.display().to_string()), true, "");
        item(ui, t!("Afficher dans le gestionnaire de fichiers", "Show in the file manager"), TabAct::Reveal(p.clone()), true, "");
    }
}


/// Action chosen in the commit graph (right click or details buttons).
enum HistAct {
    /// `git checkout <target>`: a branch name, or a hash (detached HEAD).
    Checkout(String),
    BranchAt(String),
    Copy(String),
    Diff(String),
    Split(String),
    CherryPick(String),
    Revert(String),
    /// Compare two revisions: (from, to, title).
    Compare(String, String, String),
    Rebase(String),
    /// Given to Claude with the next message.
    Claude(String),
}

/// Right-click menu of a commit in the graph.
fn commit_menu(ui: &mut egui::Ui, c: &history::Commit, local: &[String], selected: Option<&str>, act: &mut Option<HistAct>) {
    ui.set_min_width(260.0);
    fn item(ui: &mut egui::Ui, label: &str, a: HistAct, act: &mut Option<HistAct>) {
        if ui.button(label).clicked() {
            *act = Some(a);
            ui.close_menu();
        }
    }
    let short = &c.hash[..c.hash.len().min(8)];
    ui.label(RichText::new(format!("{short}  {}", c.subject)).small().weak());
    // branches pointing here: local ones, and remote ones without a local branch of the same name
    let mut branches: Vec<(String, String)> = Vec::new();
    for r in &c.refs {
        let r = r.strip_prefix("HEAD -> ").unwrap_or(r);
        if r == "HEAD" || r.starts_with("tag: ") || r.ends_with("/HEAD") {
            continue;
        }
        if local.iter().any(|l| l == r) {
            branches.push((r.to_string(), tf!("Checkout de la branche « {r} »", "Checkout of the branch “{r}”")));
        } else if let Some((_, name)) = r.split_once('/') {
            if !local.iter().any(|l| l == name) && !c.refs.iter().any(|x| x == name) {
                branches.push((name.to_string(), tf!("Checkout de « {name} » (suit {r})", "Checkout of “{name}” (tracks {r})")));
            }
        }
    }
    for (target, label) in branches {
        item(ui, &label, HistAct::Checkout(target), act);
    }
    item(ui, t!("Checkout de ce commit (HEAD détachée)", "Checkout of this commit (detached HEAD)"), HistAct::Checkout(c.hash.clone()), act);
    item(ui, t!("Créer une branche ici…", "Create a branch here…"), HistAct::BranchAt(c.hash.clone()), act);
    ui.separator();
    item(ui, t!("Voir le diff complet", "See the full diff"), HistAct::Diff(c.hash.clone()), act);
    item(ui, t!("✳ Donner ce commit à Claude (contexte)", "✳ Give this commit to Claude (context)"), HistAct::Claude(c.hash.clone()), act);
    if let Some(s) = selected {
        let title = format!("{} → {}", &s[..s.len().min(8)], short);
        item(ui, t!("Comparer avec le commit sélectionné", "Compare with the selected commit"), HistAct::Compare(s.to_string(), c.hash.clone(), title), act);
    }
    ui.menu_button(t!("Comparer avec la branche", "Compare with the branch"), |ui| {
        item(ui, t!("HEAD (branche courante)", "HEAD (current branch)"), HistAct::Compare(c.hash.clone(), "HEAD".into(), format!("{short} → HEAD")), act);
        for b in local {
            item(ui, b, HistAct::Compare(c.hash.clone(), b.clone(), format!("{short} → {b}")), act);
        }
    });
    ui.separator();
    item(ui, t!("Cherry-pick sur la branche courante", "Cherry-pick on the current branch"), HistAct::CherryPick(c.hash.clone()), act);
    item(ui, t!("Revert : annuler ce commit par un nouveau commit", "Revert: undo this commit with a new commit"), HistAct::Revert(c.hash.clone()), act);
    if c.parents.len() == 1 {
        item(ui, t!("Rebase interactif à partir d'ici…", "Interactive rebase from here…"), HistAct::Rebase(c.hash.clone()), act);
        item(ui, t!("✂ Découper ce commit en plusieurs…", "✂ Split this commit into several…"), HistAct::Split(c.hash.clone()), act);
    }
    ui.separator();
    item(ui, t!("Copier le hash", "Copy the hash"), HistAct::Copy(c.hash.clone()), act);
    item(ui, t!("Copier le message", "Copy the message"), HistAct::Copy(c.subject.clone()), act);
}

/// Colors of the welcome page, on the navy (dark theme) or paper (light theme) backdrop.
#[derive(Clone, Copy)]
struct WelcomeColors {
    text: Color32,
    dim: Color32,
    strong: Color32,
    card: Color32,
    card_stroke: Color32,
    hover: Color32,
    key: Color32,
    key_stroke: Color32,
}

fn welcome_colors(light: bool) -> WelcomeColors {
    if light {
        WelcomeColors {
            text: Color32::from_rgb(0x1f, 0x2a, 0x3e),
            dim: Color32::from_rgb(0x5f, 0x6e, 0x88),
            strong: Color32::from_rgb(0x0d, 0x16, 0x26),
            card: Color32::from_rgb(0xff, 0xff, 0xff),
            card_stroke: Color32::from_rgb(0xd5, 0xde, 0xec),
            hover: Color32::from_rgb(0xe8, 0xef, 0xfb),
            key: Color32::from_rgb(0xf1, 0xf4, 0xf9),
            key_stroke: Color32::from_rgb(0xc8, 0xd3, 0xe4),
        }
    } else {
        WelcomeColors {
            text: Color32::from_rgb(0xe8, 0xee, 0xf8),
            dim: Color32::from_rgb(0xa3, 0xb3, 0xcc),
            strong: Color32::WHITE,
            card: Color32::from_rgb(0x16, 0x22, 0x38),
            card_stroke: Color32::from_rgb(0x26, 0x36, 0x52),
            hover: Color32::from_rgb(0x22, 0x33, 0x52),
            key: Color32::from_rgb(0x0f, 0x19, 0x2b),
            key_stroke: Color32::from_rgb(0x34, 0x47, 0x68),
        }
    }
}

/// Rounded panel on the welcome backdrop, with a colored title.
fn welcome_card(ui: &mut egui::Ui, title: &str, color: Color32, wc: &WelcomeColors, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(wc.card)
        .stroke(egui::Stroke::new(1.0_f32, wc.card_stroke))
        .rounding(10.0)
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).size(12.0).strong().color(color));
            ui.add_space(6.0);
            add(ui);
        });
}

/// Clickable line of the welcome page: label, then a shortcut (keycap) or a dimmed detail.
fn welcome_row(ui: &mut egui::Ui, label: &str, detail: &str, keycap: bool, color: Color32, wc: &WelcomeColors) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), Sense::click());
    let hovered = resp.hovered();
    let p = ui.painter().with_clip_rect(rect);
    if hovered {
        p.rect_filled(rect, 6.0, wc.hover);
        p.rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 6.0), egui::vec2(3.0, 16.0)), 1.5, color);
    }
    let cy = rect.center().y;
    let text = if hovered { wc.strong } else { wc.text };
    let lr = p.text(egui::pos2(rect.left() + 10.0, cy), Align2::LEFT_CENTER, label, egui::FontId::proportional(14.0), text);
    if !detail.is_empty() {
        let font = egui::FontId::proportional(if keycap { 11.5 } else { 12.0 });
        let galley = p.layout_no_wrap(detail.to_string(), font, wc.dim);
        let w = galley.size().x;
        if keycap {
            let key = egui::Rect::from_min_size(egui::pos2(rect.right() - w - 18.0, cy - 10.0), egui::vec2(w + 12.0, 20.0));
            p.rect_filled(key, 4.0, wc.key);
            p.rect_stroke(key, 4.0, egui::Stroke::new(1.0_f32, wc.key_stroke));
            p.galley(key.center() - galley.size() / 2.0, galley, wc.dim);
        } else {
            // path after the name, cut on the left when too long
            let x = lr.right() + 10.0;
            let room = rect.right() - 8.0 - x;
            if room > 30.0 {
                let mut d = detail.to_string();
                let mut g = galley;
                while g.size().x > room && d.chars().count() > 2 {
                    d = format!("…{}", d.chars().skip(4).collect::<String>());
                    g = p.layout_no_wrap(d.clone(), egui::FontId::proportional(12.0), wc.dim);
                }
                p.galley(egui::pos2(x, cy - g.size().y / 2.0), g, wc.dim);
            }
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// "▾" button listing previous values; returns true when one was picked.
fn history_menu(ui: &mut egui::Ui, list: &[String], target: &mut String, title: &str) -> bool {
    let mut picked = false;
    ui.menu_button("⏷", |ui| {
        ui.label(RichText::new(title).small().strong());
        if list.is_empty() {
            ui.label(RichText::new(t!("(vide)", "(empty)")).weak());
        }
        for h in list.iter().take(20) {
            if ui.button(RichText::new(h).monospace()).clicked() {
                *target = h.clone();
                picked = true;
                ui.close_menu();
            }
        }
    })
    .response
    .on_hover_text(title);
    picked
}

/// ↑ / ↓ in a focused field browse the history (most recent first).
fn history_arrows(ui: &egui::Ui, list: &[String], pos: &mut Option<usize>) -> Option<String> {
    if list.is_empty() {
        return None;
    }
    let (up, down) = ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
    if up {
        let next = pos.map_or(0, |p| (p + 1).min(list.len() - 1));
        *pos = Some(next);
        return Some(list[next].clone());
    }
    if down {
        if let Some(p) = *pos {
            if p == 0 {
                *pos = None;
                return Some(String::new());
            }
            *pos = Some(p - 1);
            return Some(list[p - 1].clone());
        }
    }
    None
}

/// Minimal text tab with an accent underline when selected (sidebar / bottom panel headers).
fn nav_tab(ui: &mut egui::Ui, icon: icons::ViewIcon, label: &str, active: bool) -> egui::Response {
    let font = egui::FontId::proportional(13.0);
    let size = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size();
    let (rect, resp) = ui.allocate_exact_size(size + egui::vec2(38.0, 14.0), Sense::click());
    let v = ui.visuals();
    let color = if active {
        v.strong_text_color()
    } else if resp.hovered() {
        v.text_color()
    } else {
        v.weak_text_color()
    };
    icons::paint_view(ui.painter(), egui::pos2(rect.left() + 17.0, rect.center().y - 1.0), icon, color);
    ui.painter().text(egui::pos2(rect.left() + 30.0, rect.center().y - 1.0), Align2::LEFT_CENTER, label, font, color);
    if active {
        let y = rect.bottom() - 1.5;
        ui.painter().line_segment(
            [egui::pos2(rect.left() + 8.0, y), egui::pos2(rect.right() - 8.0, y)],
            egui::Stroke::new(2.0_f32, v.selection.stroke.color),
        );
    }
    resp
}

/// flake8 / ruff codes reported on `line` (the ones "# noqa" understands).
fn noqa_codes(d: &Document, line: usize) -> Vec<String> {
    let mut codes: Vec<String> = d
        .diagnostics
        .iter()
        .filter(|g| g.line == line && matches!(g.source.as_str(), "flake8" | "ruff" | "pyflakes"))
        .filter_map(|g| g.code.clone())
        .collect();
    let mut seen = HashSet::new();
    codes.retain(|c| seen.insert(c.clone()));
    codes
}

#[cfg(test)]
mod font_tests {
    use super::*;

    /// Every symbol written in the sources must exist in the fonts (otherwise it shows as an empty box).
    #[test]
    fn all_symbols_have_glyphs() {
        let ctx = egui::Context::default();
        ctx.set_fonts(font_definitions());
        let _ = ctx.run(Default::default(), |_| {});
        let sources = [
            include_str!("app.rs"),
            include_str!("app/find.rs"),
            include_str!("app/git_actions.rs"),
            include_str!("app/git_view.rs"),
            include_str!("app/input.rs"),
            include_str!("app/job_results.rs"),
            include_str!("app/lsp_client.rs"),
            include_str!("app/navigation.rs"),
            include_str!("app/overlay.rs"),
            include_str!("app/panels.rs"),
            include_str!("app/panes.rs"),
            include_str!("app/prefs.rs"),
            include_str!("app/projects.rs"),
            include_str!("app/claude_queue.rs"),
            include_str!("app/claude_ask.rs"),
            include_str!("app/claude_ui.rs"),
            include_str!("app/claude_turns.rs"),
            include_str!("app/theme_edit.rs"),
            include_str!("app/template_edit.rs"),
            include_str!("app/quick_fix.rs"),
            include_str!("app/save_lint.rs"),
            include_str!("app/search_panel.rs"),
            include_str!("app/settings.rs"),
            include_str!("app/split_commit.rs"),
            include_str!("app/commit_plan.rs"),
            include_str!("app/debugger.rs"),
            include_str!("view_bar.rs"),
            include_str!("editor_view.rs"),
            include_str!("history.rs"),
            include_str!("sidebar.rs"),
            include_str!("terminal.rs"),
            include_str!("search.rs"),
            include_str!("commands.rs"),
            include_str!("icons.rs"),
            include_str!("logo.rs"),
            include_str!("symbols.rs"),
        ];
        let mut missing = std::collections::BTreeSet::new();
        for src in sources {
            for line in src.lines().filter(|l| !l.trim_start().starts_with("//")) {
                for ch in line.chars().filter(|c| !c.is_ascii()) {
                    for font in [egui::FontId::proportional(14.0), egui::FontId::monospace(14.0)] {
                        if !ctx.fonts(|f| f.has_glyphs(&font, &ch.to_string())) {
                            missing.insert(format!("{ch} U+{:04X} ({:?})", ch as u32, font.family));
                        }
                    }
                }
            }
        }
        assert!(missing.is_empty(), "caractères sans glyphe : {missing:#?}");
        // the check does see missing glyphs, and the Hack fallback is what brings the arrows
        let prop = egui::FontId::proportional(14.0);
        assert!(!ctx.fonts(|f| f.has_glyphs(&prop, "\u{2460}")));
        assert!(ctx.fonts(|f| f.has_glyphs(&prop, "\u{2192}\u{25BE}\u{25CF}")));
        let plain = egui::Context::default();
        let _ = plain.run(Default::default(), |_| {});
        assert!(!plain.fonts(|f| f.has_glyphs(&prop, "\u{2192}")));
    }

    /// The image tabs rely on egui_extras' loaders: PNG and SVG files must decode.
    #[test]
    fn images_decode() {
        let dir = std::env::temp_dir().join(format!("florm_img_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("a.png");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 255])).save(&png).unwrap();
        let svg = dir.join("b.svg");
        std::fs::write(&svg, r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="blue"/></svg>"#).unwrap();
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        for (path, want) in [(&png, egui::vec2(3.0, 2.0)), (&svg, egui::vec2(40.0, 20.0))] {
            assert!(document::is_image(path));
            let uri = format!("file://{}", path.display());
            let mut size = None;
            for _ in 0..200 {
                let _ = ctx.run(Default::default(), |ctx| {
                    if let Ok(egui::load::TexturePoll::Ready { texture }) =
                        egui::Image::new(uri.as_str()).load_for_size(ctx, egui::Vec2::splat(512.0))
                    {
                        size = Some(texture.size);
                    }
                });
                if size.is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // a vector image is rasterized at the requested size: compare the proportions
            let got = size.unwrap_or_else(|| panic!("{} non décodé", path.display()));
            assert!((got.x / got.y - want.x / want.y).abs() < 0.01, "{}: {got:?}", path.display());
            if path == &png {
                assert_eq!(got, want);
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn definition_word_and_word_start() {
        let at = |text: &str, pos: usize| {
            let mut d = Document::from_text(1, text, 4, true);
            d.sels = vec![Sel::cursor(pos)];
            d.primary = 0;
            (App::definition_word(&d), App::word_start(&d))
        };
        assert_eq!(at("self.compute_total()", 8).0, "compute_total");
        assert_eq!(at("_inherit = 'sale.order'", 15).0, "sale.order");
        assert_eq!(at("env.ref(\"syndic.view_form\")", 16).0, "syndic.view_form");
        // a quoted word without a dot stays a plain word
        assert_eq!(at("x = 'hello'", 7).0, "hello");
        assert_eq!(at("    record.amo", 14).1, 11);
    }

    /// The view tabs come back with the project. It changes the configuration folder of the
    /// process: `FLORM_TEST_APP=1 cargo test view_tabs`.
    #[test]
    fn view_tabs_come_back() {
        if std::env::var_os("FLORM_TEST_APP").is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("florm_views_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        std::fs::write(proj.join("a.py"), "x = 1\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        app.open_path(proj.join("a.py"));
        app.claude_restore_tab(0);
        app.open_debug();
        app.open_view_tab(document::View::Help, "Guide".into());
        app.save_session();
        let views = |app: &App| app.docs.iter().map(|d| d.view.clone()).collect::<Vec<_>>();
        let before = views(&app);
        let again = App::with_ctx(&ctx, Some(proj.clone()));
        assert_eq!(views(&again), before);
        assert_eq!(again.docs[again.active].view, document::View::Help);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Too many tabs: the arrows appear, the wheel moves the strip. `FLORM_TEST_APP=1`.
    #[test]
    fn many_tabs_scroll() {
        if std::env::var_os("FLORM_TEST_APP").is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("florm_tabs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        for k in 0..40 {
            let p = proj.join(format!("fichier_numero_{k}.py"));
            std::fs::write(&p, "").unwrap();
            app.open_path(p);
        }
        app.activate_tab(0);
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0)));
        let id = egui::Id::new("tabs").with(None::<u8>);
        let frame = |app: &mut App, events: Vec<egui::Event>| {
            let _ = ctx.run(egui::RawInput { screen_rect: screen, events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.tabs_strip(ui, None));
            });
        };
        for _ in 0..3 {
            frame(&mut app, Vec::new());
        }
        assert_eq!(ctx.data(|d| d.get_temp::<bool>(id.with("over"))), Some(true));
        let offset = || ctx.data(|d| d.get_temp::<(f32, f32)>(id.with("offset"))).unwrap().0;
        assert_eq!(offset(), 0.0);
        let at = egui::pos2(300.0, 15.0);
        frame(&mut app, vec![egui::Event::PointerMoved(at)]);
        for _ in 0..30 {
            frame(&mut app, vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -60.0), modifiers: Default::default() }]);
        }
        for _ in 0..10 {
            frame(&mut app, Vec::new());
        }
        assert!(offset() > 300.0, "{}", offset());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The screens drawn without a window: the welcome tab when nothing is open, the advanced
    /// preferences, the sidebar with icons hidden. `FLORM_TEST_APP=1`.
    #[test]
    fn screens_draw() {
        if std::env::var_os("FLORM_TEST_APP").is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("florm_screens_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\nsidebar_hidden = [\"settings\", \"docker\"]\nshow_menu_bar = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        let mut frame = |app: &mut App| {
            let _ = ctx.run(egui::RawInput { screen_rect: screen, ..Default::default() }, |ctx| {
                app.sidebar(ctx);
                app.central(ctx);
            });
        };
        for _ in 0..2 {
            frame(&mut app);
        }
        // nothing open: the welcome page is a tab
        assert_eq!(app.docs.iter().map(|d| d.view.clone()).collect::<Vec<_>>(), [document::View::Welcome]);
        // a file takes its place
        std::fs::write(proj.join("a.py"), "").unwrap();
        app.open_path(proj.join("a.py"));
        assert_eq!(app.docs.len(), 1);
        assert_eq!(app.docs[0].view, document::View::Text);
        app.prefs.section = PrefSection::Advanced;
        app.open_settings();
        for _ in 0..2 {
            frame(&mut app);
        }
        // the preferences and Git are tabs: Échap goes back to the file, they stay open
        let views = |app: &App| app.docs.iter().map(|d| d.view).collect::<Vec<_>>();
        assert_eq!(views(&app), [document::View::Text, document::View::Settings]);
        app.exec(&ctx, Cmd::Escape);
        assert_eq!(app.docs[app.active].view, document::View::Text);
        app.open_git_view();
        frame(&mut app);
        assert_eq!(views(&app), [document::View::Text, document::View::Settings, document::View::Git]);
        assert!(app.showing(document::View::Git));
        // Ctrl+W closes it like any tab
        app.exec(&ctx, Cmd::CloseTab);
        assert_eq!(views(&app), [document::View::Text, document::View::Settings]);
        // kept with the session
        app.save_session();
        let again = App::with_ctx(&ctx, Some(proj.clone()));
        assert_eq!(views(&again), views(&app));
        // every view, large in a tab then in the bottom panel
        let mut all = |app: &mut App| {
            let _ = ctx.run(egui::RawInput { screen_rect: screen, ..Default::default() }, |ctx| {
                app.sidebar(ctx);
                if app.bottom.is_some() {
                    app.bottom_panel(ctx);
                }
                app.central(ctx);
            });
        };
        for (tab, ..) in BottomTab::ALL {
            app.show_big(tab);
            all(&mut app);
            all(&mut app);
            app.show_bottom(tab, false);
            all(&mut app);
            all(&mut app);
            assert_eq!(app.bottom, Some(tab));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn readable_text_on_filled_buttons() {
        // white on dark fills (Light+ accent), dark on light ones (yellow)
        assert_eq!(text_on(Color32::from_rgb(0x00, 0x7a, 0xcc)), Color32::WHITE);
        assert_ne!(text_on(Color32::from_rgb(0xe5, 0xc0, 0x7b)), Color32::WHITE);
        assert_ne!(text_on(Color32::from_rgb(0x98, 0xc3, 0x79)), Color32::WHITE);
    }

    #[test]
    fn path_completion_like_a_shell() {
        let dir = std::env::temp_dir().join(format!("florm_complete_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("addons/sale")).unwrap();
        std::fs::create_dir_all(dir.join("addons/stock")).unwrap();
        std::fs::write(dir.join("addons/readme.md"), "").unwrap();
        std::fs::write(dir.join(".hidden"), "").unwrap();
        let sep = std::path::MAIN_SEPARATOR;
        // a single folder: completed with its separator
        assert_eq!(complete_path("add", &dir, false).0, format!("addons{sep}"));
        // several: as far as they agree
        let (done, found) = complete_path("addons/s", &dir, false);
        assert_eq!(done, "addons/s");
        assert_eq!(found.len(), 2);
        assert_eq!(complete_path("addons/sa", &dir, false).0, format!("addons/sale{sep}"));
        // folders only (open a folder), case-insensitive when nothing matches exactly
        assert_eq!(complete_path("addons/", &dir, true).1.len(), 2);
        assert_eq!(complete_path("addons/README", &dir, false).0, "addons/readme.md");
        // absolute paths, hidden files only with a dot
        let abs = format!("{}/", dir.display());
        assert!(complete_path(&abs, Path::new("/"), false).1.iter().all(|(n, _)| !n.starts_with('.')));
        assert_eq!(complete_path(&format!("{abs}.h"), Path::new("/"), false).0, format!("{abs}.hidden"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
