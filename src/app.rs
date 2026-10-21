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

/// Main action button, filled with `fill` (the theme accent, or a status color).
fn filled_button(text: impl Into<String>, fill: Color32) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(text_on(fill)).strong()).fill(fill)
}

const TAB_STRIP_H: f32 = 36.0;

#[derive(PartialEq, Clone, Copy)]
enum SideTab {
    Files,
    Git,
    Projects,
}

#[derive(PartialEq, Clone, Copy)]
enum BottomTab {
    Search,
    Problems,
    Terminal,
    Output,
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
    items: Vec<Item>,
    filtered: Vec<usize>,
    last_query: Option<String>,
    /// Every item matching `.0` (not only the shown ones): a longer query only filters these.
    matched: Option<(String, Vec<usize>)>,
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

enum Confirm {
    CloseTab(u64),
    Quit,
    Delete(PathBuf),
    SwitchProject(PathBuf),
    ReplaceInFiles,
    /// Irreversible change requested from a diff (discarding uncommitted changes).
    DiffOp { msg: String, label: String, op: DiffOp },
    /// Destructive git command of the Git panel (e.g. dropping a stash).
    Git { title: String, msg: String, label: String, args: Vec<String> },
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

#[derive(Default)]
struct HistoryUi {
    open: bool,
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
    /// Shown in the central area (it stays in progress while the user looks at other tabs).
    shown: bool,
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
    error: Option<String>,
    log: Vec<git::LogEntry>,
    commit_msg: String,
    busy: usize,
    /// Last failed git command (label, message): shown until dismissed or a later command succeeds.
    op_error: Option<(String, String)>,
    focus_commit: bool,
    /// absolute path -> status letter (directories inherit from their files)
    map: HashMap<PathBuf, char>,
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
    side_tab: SideTab,
    bottom: Option<BottomTab>,
    overlay: Option<Overlay>,
    find: FindState,
    search: SearchUi,
    projects: Vec<Project>,
    last_session_save: Instant,
    jobs: Jobs,
    file_index: Vec<PathBuf>,
    /// When the file list was last asked for (Ctrl+P rebuilds it, at most every few seconds).
    file_index_at: Option<Instant>,
    git: GitUi,
    term: Terminal,
    plugins: Vec<Plugin>,
    plugin_cmds: Vec<(usize, usize)>,
    keymap: Keymap,
    chord: bool,
    confirm: Option<Confirm>,
    output: Vec<String>,
    status: Option<(String, Instant)>,
    pending: Vec<Cmd>,
    last_disk_check: Instant,
    last_line_copy: Option<String>,
    quit_confirmed: bool,
    closed: Vec<PathBuf>,
    last_title: String,
    visuals_dirty: bool,
    /// Tab last scrolled into view (to follow the active tab once).
    tab_scrolled: Option<usize>,
    /// Tab being dragged to another position.
    tab_drag: Option<usize>,
    /// Split editor: orientation, panel with the keyboard focus, document shown by each panel.
    split_dir: SplitDir,
    focused_pane: u8,
    pane_active: [Option<u64>; 2],
    drop_zones: DropZones,
    /// Give the keyboard focus to the editor at the next frame (after a tab change).
    pending_focus: bool,
    md_cache: egui_commonmark::CommonMarkCache,
    lsp: lsp::Manager,
    lsp_req: HashMap<(usize, i64), LspReq>,
    /// For the places without a context at hand (prompt submission).
    egui_ctx: egui::Context,
    completion: Option<CompletionUi>,
    /// Ask the server for suggestions at this moment (debounce while typing).
    complete_at: Option<Instant>,
    /// Why a language has no server (status bar).
    lsp_note: HashMap<String, String>,
    /// Project symbols (go to definition / completion without a language server).
    sym_index: Option<crate::index::Index>,
    /// Cursor history: previous / next places, and the place seen at the last frame.
    nav_back: Vec<NavPos>,
    nav_fwd: Vec<NavPos>,
    nav_last: Option<(NavPos, usize)>,
    /// The cursor moved because of Alt+- / Alt++: do not record it.
    nav_jumping: bool,
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
    /// Fonts of the system, for the preferences (None until listed).
    font_list: Option<Vec<crate::fonts::FontInfo>>,
    font_list_loading: bool,
    /// The project index is being built for this folder.
    sym_index_for: Option<PathBuf>,
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
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrefSection {
    Appearance,
    Editor,
    Lint,
    Git,
    Terminal,
    Keys,
    Advanced,
}

const PREF_SECTIONS: [(PrefSection, &str); 7] = [
    (PrefSection::Appearance, "Apparence"),
    (PrefSection::Editor, "Éditeur"),
    (PrefSection::Lint, "Lint"),
    (PrefSection::Git, "Git"),
    (PrefSection::Terminal, "Terminal"),
    (PrefSection::Keys, "Raccourcis"),
    (PrefSection::Advanced, "Avancé"),
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
    open: bool,
    section: PrefSection,
    filter: String,
    /// text buffers edited before being saved
    lint_args: String,
    shell: String,
}

impl Default for PrefsUi {
    fn default() -> Self {
        PrefsUi { capture: None, proposed: None, key_action: None, open: false, section: PrefSection::Appearance, filter: String::new(), lint_args: String::new(), shell: String::new() }
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, arg: Option<PathBuf>) -> Self {
        let (settings, settings_err) = config::load();
        // Florm has its own palette: egui must not switch to its light / dark style with the system
        // theme (Windows), and both styles get the same settings
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        cc.egui_ctx.all_styles_mut(|s| {
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
        let mut hl = Highlighting::new(&settings.theme, &config::themes_dir());
        hl.set_overrides(highlight::parse_hex(&settings.accent_color), settings.text_softness);
        apply_visuals(&cc.egui_ctx, hl.visuals());
        cc.egui_ctx.set_fonts(crate::fonts::definitions(&settings.editor_font, &settings.ui_font));
        egui_extras::install_image_loaders(&cc.egui_ctx);
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
            side_tab: SideTab::Files,
            bottom: None,
            overlay: None,
            find: FindState::default(),
            search: SearchUi::default(),
            projects: project::load_list(),
            last_session_save: Instant::now(),
            jobs: Jobs::new(cc.egui_ctx.clone()),
            file_index: Vec::new(),
            file_index_at: None,
            git: GitUi::default(),
            term: Terminal::new(),
            plugins: Vec::new(),
            plugin_cmds: Vec::new(),
            keymap,
            chord: false,
            confirm: None,
            output: Vec::new(),
            status: None,
            pending: Vec::new(),
            last_disk_check: Instant::now(),
            last_line_copy: None,
            quit_confirmed: false,
            closed: Vec::new(),
            last_title: String::new(),
            visuals_dirty: false,
            tab_scrolled: None,
            tab_drag: None,
            split_dir: SplitDir::Right,
            focused_pane: 0,
            pane_active: [None; 2],
            drop_zones: DropZones::default(),
            pending_focus: false,
            md_cache: egui_commonmark::CommonMarkCache::default(),
            lsp: lsp::Manager::default(),
            egui_ctx: cc.egui_ctx.clone(),
            lsp_req: HashMap::new(),
            completion: None,
            complete_at: None,
            lsp_note: HashMap::new(),
            sym_index: None,
            nav_back: Vec::new(),
            nav_fwd: Vec::new(),
            nav_last: None,
            nav_jumping: false,
            tree_scroll: None,
            tree_nav: sidebar::Nav::default(),
            repo_info: HashMap::new(),
            goto_select: false,
            drop_alt_text: None,
            dialog_open: false,
            font_list: None,
            font_list_loading: false,
            sym_index_for: None,
            blame_mode: false,
            blame_req: None,
            line_history: None,
            state: project::load_state(),
            lint_inflight: HashSet::new(),
            python_linter: None,
            prefs: PrefsUi::default(),
            templates: Vec::new(),
            git_stamps: HashMap::new(),
            was_focused: true,
            settings,
        };
        if let Some(e) = settings_err {
            app.log(e);
        }
        app.search = app.fresh_search();
        let cwd = std::env::current_dir().unwrap_or_default();
        let last_project = app.projects.iter().find(|p| p.path.is_dir()).map(|p| p.path.clone());
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
        self.status = Some((s.into(), Instant::now()));
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
                if self.docs.len() == 1 && self.docs[0].is_pristine_empty() {
                    self.docs.clear();
                }
                self.docs.push(d);
                self.activate_tab(self.docs.len() - 1);
                self.analyze(self.active, false);
                true
            }
            Err(e) => {
                self.set_status(e);
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
        self.file_index.clear();
        self.refresh_index();
        // the symbol index starts once the file list is done (they would compete for the disk)
        self.sym_index = None;
        self.sym_index_for = None;
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
        if self.history.open {
            self.load_history();
        }
    }

    // ------------------------------------------------------------------ history (commit graph)

    fn load_history(&mut self) {
        let Some(repo) = self.repo.clone() else {
            self.history.error = Some("Aucun dépôt git dans ce dossier.".into());
            return;
        };
        if self.history.repo.as_ref() != Some(&repo) {
            self.history.graph = None;
        }
        self.history.repo = Some(repo.clone());
        self.history.loading = true;
        // a reload (refresh, change made in a terminal…) keeps the selected commit
        if self.history.select_hash.is_none() && self.history.wip_sel.is_none() && self.history.compare.is_none() {
            self.history.select_hash = self.history.selected.and_then(|i| self.history.graph.as_ref().and_then(|g| g.commits.get(i)).map(|c| c.hash.clone()));
        }
        self.history.selected = None;
        self.history.details = None;
        let all = self.history.all;
        self.jobs.spawn(move || {
            let r = history::load(&repo, all, 3000);
            JobResult::History(repo, r)
        });
    }

    fn select_commit(&mut self, i: usize) {
        let (Some(g), Some(repo)) = (&self.history.graph, self.history.repo.clone()) else { return };
        let Some(c) = g.commits.get(i) else { return };
        let hash = c.hash.clone();
        self.history.selected = Some(i);
        self.history.details = None;
        let h2 = hash.clone();
        self.jobs.spawn(move || {
            let r = history::details(&repo, &h2);
            JobResult::HistoryDetails(h2, r)
        });
        // its changes under the list
        self.load_commit_diff(hash, None);
    }

    /// Loads a commit diff (whole commit or one file) into the pane under the commit list.
    fn load_commit_diff(&mut self, hash: String, path: Option<String>) {
        let Some(repo) = self.history.repo.clone() else { return };
        self.history.diff_loading = true;
        self.jobs.spawn(move || {
            let result = match &path {
                // -m --first-parent: merges are shown against their first parent (what the merge brought)
                Some(p) => git::run(&repo, &["show", "--format=", "-m", "--first-parent", &hash, "--", p]),
                None => git::run(&repo, &["show", "--format=", "--patch", "-m", "--first-parent", &hash]),
            };
            JobResult::HistoryDiff { hash, path, result }
        });
    }

    fn lint_config(&self) -> lint::LintConfig {
        lint::LintConfig { python_linter: self.settings.python_linter.clone(), python_args: self.settings.python_lint_args.clone() }
    }

    /// Lints the active Python buffer while typing (debounced, one run at a time per document).
    fn update_live_lint(&mut self, ctx: &egui::Context) {
        if !self.settings.lint_live {
            return;
        }
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(path) = d.path.clone() else { return };
        if lint::lang_of(&path) != Some("python") || d.lint_version == d.version || self.lint_inflight.contains(&d.uid) {
            return;
        }
        let wait = Duration::from_millis(500);
        let el = d.last_change.elapsed();
        if el < wait && d.lint_version != u64::MAX {
            ctx.request_repaint_after(wait - el);
            return;
        }
        let (uid, version, text) = (d.uid, d.version, d.rope.to_string());
        let root = self.repo_for(&path).or_else(|| self.root.clone());
        let cfg = self.lint_config();
        self.lint_inflight.insert(uid);
        self.jobs.spawn(move || {
            let (diags, linter) = lint::lint_python(&path, &text, root.as_deref(), &cfg);
            JobResult::Lint { uid, version, diags, linter }
        });
    }

    /// Requests the blame of the cursor line when it changed (blame mode).
    fn update_blame(&mut self) {
        if !self.blame_mode {
            return;
        }
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(path) = d.path.clone() else { return };
        let line = d.rope.char_to_line(d.primary_sel().head);
        let key = (d.uid, line, d.version);
        if self.blame_req == Some(key) || d.last_change.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.blame_req = Some(key);
        let (uid, version, contents) = (d.uid, d.version, d.rope.to_string());
        self.jobs.spawn(move || JobResult::Blame { uid, line, version, result: git::blame_line(&path, line + 1, &contents) });
    }

    fn open_line_history(&mut self) {
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(path) = d.path.clone() else {
            self.set_status("Le fichier n'est pas enregistré");
            return;
        };
        let line = d.rope.char_to_line(d.primary_sel().head);
        let name = d.title();
        let Some(head_line) = d.head_line(line) else {
            self.set_status("Ligne ajoutée depuis le dernier commit : pas encore d'historique");
            return;
        };
        self.line_history = Some(LineHistoryUi {
            title: format!("Historique de la ligne {} — {name}", line + 1),
            loading: true,
            error: None,
            commits: Vec::new(),
            diffs: Vec::new(),
            selected: 0,
        });
        self.jobs.spawn(move || JobResult::LineHistory(git::line_history(&path, head_line + 1)));
    }

    fn line_history_ui(&mut self, ctx: &egui::Context) {
        let Some(lh) = self.line_history.as_mut() else { return };
        let mut open = true;
        let (mut show_graph, mut copy) = (None, None);
        egui::Window::new(lh.title.clone())
            .id(egui::Id::new("line_history"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([960.0, 540.0])
            .min_size([480.0, 260.0])
            .show(ctx, |ui| {
                if lh.loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Recherche des commits qui ont modifié cette ligne…");
                    });
                    return;
                }
                if let Some(e) = &lh.error {
                    ui.label(RichText::new(e).color(err()));
                    return;
                }
                if lh.commits.is_empty() {
                    ui.label(RichText::new("Aucun commit trouvé pour cette ligne.").weak());
                    return;
                }
                ui.set_min_size(ui.available_size());
                let height = ui.available_height();
                ui.horizontal_top(|ui| {
                    // commit list
                    ui.vertical(|ui| {
                        ui.set_width(320.0);
                        ui.label(RichText::new(format!("{} COMMIT(S) ONT MODIFIÉ CETTE LIGNE", lh.commits.len())).small().strong());
                        egui::ScrollArea::vertical().id_salt("lh_list").max_height(height - 24.0).auto_shrink([false, false]).show(ui, |ui| {
                            for (i, c) in lh.commits.iter().enumerate() {
                                let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), Sense::click());
                                let v = ui.visuals();
                                if i == lh.selected {
                                    ui.painter().rect_filled(rect, 5.0, v.selection.bg_fill);
                                } else if r.hovered() {
                                    ui.painter().rect_filled(rect, 5.0, v.widgets.hovered.weak_bg_fill);
                                }
                                let p = ui.painter().with_clip_rect(rect.shrink(4.0));
                                p.text(rect.left_top() + egui::vec2(8.0, 6.0), Align2::LEFT_TOP, &c.subject, egui::FontId::proportional(14.0), v.text_color());
                                p.text(
                                    rect.left_top() + egui::vec2(8.0, 25.0),
                                    Align2::LEFT_TOP,
                                    format!("{} · {} · {}", c.author, c.date, &c.hash[..c.hash.len().min(7)]),
                                    egui::FontId::proportional(12.0),
                                    v.weak_text_color(),
                                );
                                if r.clicked() {
                                    lh.selected = i;
                                }
                            }
                        });
                    });
                    ui.separator();
                    // selected commit: header + diff
                    ui.vertical(|ui| {
                        let c = &lh.commits[lh.selected];
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&c.subject).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Voir dans le graphe").clicked() {
                                    show_graph = Some(c.hash.clone());
                                }
                                if ui.button("Copier le hash").clicked() {
                                    copy = Some(c.hash.clone());
                                }
                            });
                        });
                        ui.label(RichText::new(format!("{} · {} · {}", c.author, c.date, &c.hash[..c.hash.len().min(10)])).weak());
                        ui.separator();
                        let _ = history::diff_view(ui, &lh.diffs[lh.selected], "line_history_diff", &[]);
                    });
                });
            });
        if !open {
            self.line_history = None;
        }
        if let Some(h) = copy {
            ctx.output_mut(|o| o.copied_text = h);
            self.set_status("Hash copié");
        }
        if let Some(h) = show_graph {
            self.line_history = None;
            if let Some(r) = self.docs.get(self.active).and_then(|d| d.path.clone()).and_then(|p| self.repo_for(&p)) {
                self.select_repo(r);
            }
            self.history.open = true;
            self.history.all = true;
            self.history.select_hash = Some(h);
            self.load_history();
        }
    }

    /// Key of what is selected in the graph: a commit hash, or "INDEX" / "WORKTREE" for the
    /// staged / unstaged changes (rows at the top, like tig).
    fn history_key(&self) -> Option<String> {
        if let Some((a, b, _)) = &self.history.compare {
            return Some(format!("{a}..{b}"));
        }
        match self.history.wip_sel {
            Some(true) => Some("INDEX".into()),
            Some(false) => Some("WORKTREE".into()),
            None => self.history.selected.and_then(|i| self.history.graph.as_ref().map(|g| g.commits[i].hash.clone())),
        }
    }

    fn start_compare(&mut self, a: String, b: String, title: String) {
        let Some(repo) = self.history.repo.clone() else { return };
        let h = &mut self.history;
        h.compare = Some((a.clone(), b.clone(), title));
        h.wip_sel = None;
        h.rebase = None;
        h.diff = None;
        h.diff_loading = false;
        h.details = None;
        let key = format!("{a}..{b}");
        self.jobs.spawn(move || JobResult::HistoryDetails(key, history::compare(&repo, &a, &b)));
    }

    /// Diff between two commits (all files, or one).
    fn load_range_diff(&mut self, a: String, b: String, path: Option<String>) {
        let Some(repo) = self.history.repo.clone() else { return };
        self.history.diff_loading = true;
        self.jobs.spawn(move || {
            let result = match &path {
                Some(p) => git::run(&repo, &["diff", &a, &b, "--", p]),
                None => git::run(&repo, &["diff", &a, &b]),
            };
            JobResult::HistoryDiff { hash: format!("{a}..{b}"), path, result }
        });
    }

    /// Diff of all the staged (`staged`) or unstaged changes, new files included.
    fn load_wip_all(&mut self, staged: bool) {
        let Some(repo) = self.history.repo.clone() else { return };
        self.history.diff_loading = true;
        self.jobs.spawn(move || {
            let result = if staged {
                git::run(&repo, &["diff", "--cached"])
            } else {
                git::run(&repo, &["diff"]).map(|mut text| {
                    // new files are not in `git diff`: compared with nothing, one by one
                    let others = git::stdout(&repo, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
                    for p in others.lines().filter(|p| !p.is_empty()).take(200) {
                        if std::fs::metadata(repo.join(p)).is_ok_and(|m| m.len() > 2_000_000) {
                            continue;
                        }
                        let d = match git::run(&repo, &["diff", "--no-index", "--", "/dev/null", p]) {
                            Err(e) if e.starts_with("diff --git") => e,
                            Ok(o) => o,
                            Err(_) => continue,
                        };
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(&d);
                    }
                    text
                })
            };
            let hash = if staged { "INDEX" } else { "WORKTREE" }.to_string();
            JobResult::HistoryDiff { hash, path: None, result }
        });
    }

    /// Button or double click in the diff of the graph view.
    fn diff_action(&mut self, source: DiffSource, click: Option<history::DiffClick>, file_act: Option<&str>) {
        let Some(d) = &self.history.diff else { return };
        let Some(repo) = self.history.repo.clone() else { return };
        let (text, lines) = (d.text.clone(), &d.lines);
        let first = lines.iter().position(|l| l.kind == history::DiffKind::Hunk).unwrap_or(0);
        let (idx, hunk_button) = match click {
            Some(history::DiffClick::Line(i)) => (i, None),
            Some(history::DiffClick::Hunk(i, b)) => (i, Some(b)),
            Some(history::DiffClick::File(i, _)) => (i, None),
            None => (first, None),
        };
        // a new file not yet known to git (only in the unstaged changes)
        let untracked = source == DiffSource::Worktree && history::file_patch(&text, idx).is_some_and(|f| f.contains("\n--- /dev/null"));
        let Some(path) = history::file_at(&text, idx).or_else(|| d.path.clone()) else { return };
        let line = history::target_line(lines, idx);
        let lose = |what: &str| format!("{what}\n\nCe changement n'est pas encore dans un commit : il sera perdu.");
        let stage_file = DiffOp::Git(vec!["add".into(), "--".into(), path.clone()]);
        let (label, op, confirm) = match (click, file_act, hunk_button) {
            (Some(history::DiffClick::Line(_)), _, _) | (_, Some("open"), _) => {
                self.jump_to_file(&repo, &path, line);
                return;
            }
            (_, Some("stage"), _) => ("add", stage_file, None),
            (_, Some("unstage"), _) => ("restore", DiffOp::Git(vec!["restore".into(), "--staged".into(), "--".into(), path.clone()]), None),
            (_, Some("discard"), _) if untracked => {
                ("rm", DiffOp::Delete(repo.join(&path)), Some(lose(&format!("Supprimer le nouveau fichier « {path} » ?"))))
            }
            (_, Some("discard"), _) => (
                "restore",
                DiffOp::Git(vec!["restore".into(), "--".into(), path.clone()]),
                Some(lose(&format!("Annuler toutes les modifications non indexées de « {path} » ?"))),
            ),
            (_, Some("revert"), _) => {
                let Some(patch) = history::file_patch(&text, idx) else { return };
                ("apply", DiffOp::Apply { patch, flags: vec!["-R"] }, None)
            }
            (_, _, Some(b)) => {
                let Some(patch) = history::hunk_patch(&text, idx) else { return };
                match (source, b) {
                    (DiffSource::Worktree, 0) if untracked => ("add", stage_file, None),
                    (DiffSource::Worktree, 0) => ("apply", DiffOp::Apply { patch, flags: vec!["--cached"] }, None),
                    (DiffSource::Worktree, _) if untracked => {
                        ("rm", DiffOp::Delete(repo.join(&path)), Some(lose(&format!("Supprimer le nouveau fichier « {path} » ?"))))
                    }
                    (DiffSource::Worktree, _) => (
                        "apply",
                        DiffOp::Apply { patch, flags: vec!["-R"] },
                        Some(lose(&format!("Annuler ce bloc de modifications dans « {path} » (vers la ligne {line}) ?"))),
                    ),
                    (DiffSource::Index, _) => ("apply", DiffOp::Apply { patch, flags: vec!["--cached", "-R"] }, None),
                    (DiffSource::Commit, _) => ("apply", DiffOp::Apply { patch, flags: vec!["-R"] }, None),
                    (DiffSource::Compare, _) => return,
                }
            }
            _ => return,
        };
        match confirm {
            Some(msg) => self.confirm = Some(Confirm::DiffOp { msg, label: label.into(), op }),
            None => self.run_diff_op(label.into(), op),
        }
    }

    fn run_diff_op(&mut self, label: String, op: DiffOp) {
        let Some(repo) = self.history.repo.clone() else { return };
        self.git.busy += 1;
        self.jobs.spawn(move || {
            let result = match op {
                DiffOp::Apply { patch, flags } => {
                    let mut a = vec!["apply"];
                    a.extend(flags);
                    a.extend(["--recount", "-"]);
                    git::run_with(&repo, &a, &[], Some(&patch))
                }
                DiffOp::Git(args) => {
                    let a: Vec<&str> = args.iter().map(String::as_str).collect();
                    git::run(&repo, &a)
                }
                DiffOp::Delete(p) => std::fs::remove_file(&p).map(|()| format!("{} supprimé", p.display())).map_err(|e| e.to_string()),
            };
            JobResult::GitOp { label, result }
        });
    }

    /// Opens a file of the repository at a line (from a diff).
    fn jump_to_file(&mut self, repo: &Path, rel: &str, line: usize) {
        let path = repo.join(rel);
        if !path.is_file() {
            self.history.notice = Some((format!("« {rel} » n'existe plus dans tes fichiers."), err()));
            return;
        }
        if self.open_path(path) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.goto_line(line.saturating_sub(1), 0);
            }
            self.history.open = false;
            self.pending_focus = true;
        }
    }

    fn history_ui(&mut self, ui: &mut egui::Ui) {
        let (mut reload, mut close, mut select, mut diff_to_tab) = (false, false, None, false);
        let (mut diff_click, mut file_act): (Option<history::DiffClick>, Option<&str>) = (None, None);
        let source = match (self.history.wip_sel, &self.history.compare) {
            (_, Some(_)) => DiffSource::Compare,
            (Some(false), _) => DiffSource::Worktree,
            (Some(true), _) => DiffSource::Index,
            (None, _) => DiffSource::Commit,
        };
        let (mut rb_run, mut rb_cancel) = (false, false);
        let op_banner = self
            .git
            .status
            .as_ref()
            .filter(|_| self.repo == self.history.repo)
            .and_then(|st| st.operation.map(|o| (o, st.entries.iter().filter(|e| e.conflicted()).count())));
        let mut op_cmd: Option<Vec<String>> = None;
        let (mut menu_act, mut clear_notice, mut resume, mut abort_rebase) = (None, false, false, false);
        let (mut wip_pick, mut wip_ops): (Option<bool>, Vec<Vec<String>>) = (None, Vec::new());
        let split_hidden = self.split.as_ref().map(|s| s.info.short.clone());
        let repo_name = self.history.repo.as_deref().map(|r| self.repo_label(r)).unwrap_or_default();
        let repo_choices: Vec<(PathBuf, String)> = self.repos.iter().map(|r| (r.clone(), self.repo_label(r))).collect();
        let mut chosen_repo = None;
        let accent = self.hl.colors.accent;
        // working tree status (same repository as the graph)
        let wip = self.git.status.clone().filter(|_| self.repo == self.history.repo);
        let h = &mut self.history;
        egui::TopBottomPanel::top("history_top").show_inside(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Historique Git").strong().size(15.0));
                if repo_choices.len() > 1 {
                    egui::ComboBox::from_id_salt("history_repo").selected_text(&repo_name).show_ui(ui, |ui| {
                        for (r, label) in &repo_choices {
                            if ui.selectable_label(h.repo.as_ref() == Some(r), label).clicked() {
                                chosen_repo = Some(r.clone());
                            }
                        }
                    });
                } else {
                    ui.label(RichText::new(&repo_name).weak());
                }
                ui.add_space(8.0);
                ui.add(egui::TextEdit::singleline(&mut h.filter).hint_text("🔍 Filtrer : message, auteur, hash, branche…").desired_width(240.0));
                if ui.checkbox(&mut h.all, "Toutes les branches").changed() {
                    reload = true;
                }
                if ui.button("⟳").on_hover_text("Rafraîchir").clicked() {
                    reload = true;
                }
                if h.loading {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("×").on_hover_text("Fermer (Échap)").clicked() {
                        close = true;
                    }
                    ui.label(RichText::new("↑ ↓ commits · ← → fichiers · Page ↑ ↓ défiler").small().weak());
                });
            });
            if let Some((n, color)) = &h.notice {
                ui.horizontal(|ui| {
                    let icon = if *color == err() { "⚠" } else { "✔" };
                    ui.label(RichText::new(format!("{icon} {n}")).color(*color).strong());
                    if ui.small_button("×").clicked() {
                        clear_notice = true;
                    }
                });
            }
            if let Some(short) = &split_hidden {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("✂ Le découpage de {short} est en cours.")).color(warn()).strong());
                    if ui.button("Revenir au découpage").clicked() {
                        resume = true;
                    }
                });
            } else if let Some((op, n)) = op_banner {
                ui.horizontal_wrapped(|ui| {
                    let what = if n > 0 { format!(" — {n} fichier(s) en conflit à résoudre") } else { String::new() };
                    ui.label(RichText::new(format!("⚠ {} en cours{what}", op.label())).color(warn()).strong());
                    if ui.add_enabled(n == 0, egui::Button::new("Continuer")).clicked() {
                        let mut a = vec!["-c".to_string(), "core.editor=true".to_string()];
                        a.extend(op.continue_args().iter().map(|s| s.to_string()));
                        op_cmd = Some(a);
                    }
                    if ui.button("Abandonner").on_hover_text("Remet la branche comme avant l'opération").clicked() {
                        op_cmd = Some(op.abort_args().iter().map(|s| s.to_string()).collect());
                    }
                    if op == git::Operation::Rebase && ui.button("Reprendre le découpage").clicked() {
                        resume = true;
                    }
                });
            } else if h.graph.as_ref().is_some_and(|g| g.rebasing) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠ Un rebase est en cours dans ce dépôt (découpage pas terminé ?)").color(warn()));
                    if ui.button("Reprendre le découpage").clicked() {
                        resume = true;
                    }
                    if ui.button("Annuler le rebase").on_hover_text("git rebase --abort : remet la branche comme avant").clicked() {
                        abort_rebase = true;
                    }
                });
            }
            ui.add_space(2.0);
        });

        // keyboard (when no text field has it): ↑ ↓ commits, ← → files of the diff, Page ↑ ↓ scroll the diff
        let free = ui.ctx().memory(|m| m.focused().is_none());
        let keys = if free {
            ui.input(|i| {
                (
                    i.key_pressed(Key::ArrowDown) as i32 - i.key_pressed(Key::ArrowUp) as i32,
                    i.key_pressed(Key::ArrowRight) as i32 - i.key_pressed(Key::ArrowLeft) as i32,
                    i.key_pressed(Key::PageDown) as i32 - i.key_pressed(Key::PageUp) as i32,
                )
            })
        } else {
            (0, 0, 0)
        };
        match keys {
            (_, 1, _) => h.jump = Some(history::FileJump::Next),
            (_, -1, _) => h.jump = Some(history::FileJump::Prev),
            (_, _, 1) => h.jump = Some(history::FileJump::PageDown),
            (_, _, -1) => h.jump = Some(history::FileJump::PageUp),
            _ => {}
        }
        let selected_commit = h.selected.zip(h.graph.as_ref()).map(|(i, g)| g.commits[i].clone());
        let has_detail = selected_commit.is_some() || h.wip_sel.is_some() || h.compare.is_some() || h.rebase.is_some();

        // ------------------------------------------------ commit list, with the working tree on top
        let list = |ui: &mut egui::Ui| {
            if let Some(st) = &wip {
                let staged: Vec<&git::Entry> = st.entries.iter().filter(|e| e.staged()).collect();
                let unstaged: Vec<&git::Entry> = st.entries.iter().filter(|e| e.unstaged()).collect();
                for (is_staged, list, color, label, stats) in [
                    (false, &unstaged, warn(), "Modifications non indexées", &st.unstaged_stats),
                    (true, &staged, ok(), "Modifications indexées", &st.staged_stats),
                ] {
                    if list.is_empty() {
                        continue;
                    }
                    let (a, d) = stats.values().fold((0, 0), |(a, d), s| (a + s.0.unwrap_or(0), d + s.1.unwrap_or(0)));
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
                    let sel = h.wip_sel == Some(is_staged);
                    if sel {
                        ui.painter().rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
                    }
                    let p = ui.painter();
                    let cx = rect.left() + history::LANE_W / 2.0 + 2.0;
                    if is_staged {
                        p.circle_filled(egui::pos2(cx, rect.center().y), 4.5, color);
                    } else {
                        p.circle_stroke(egui::pos2(cx, rect.center().y), 4.5, egui::Stroke::new(1.5_f32, color));
                    }
                    let r = p.text(egui::pos2(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, label, egui::FontId::proportional(13.5), color);
                    let info = format!("{} fichier(s)", list.len());
                    let r = p.text(egui::pos2(r.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, info, egui::FontId::proportional(12.0), ui.visuals().weak_text_color());
                    let r = p.text(egui::pos2(r.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, format!("+{a}"), egui::FontId::monospace(11.5), ok());
                    p.text(egui::pos2(r.right() + 6.0, rect.center().y), Align2::LEFT_CENTER, format!("−{d}"), egui::FontId::monospace(11.5), err());
                    if resp.on_hover_text("Voir les changements, indexer / désindexer").clicked() {
                        wip_pick = Some(is_staged);
                    }
                }
                if !st.entries.is_empty() {
                    ui.separator();
                }
            }
            let Some(g) = &h.graph else {
                match &h.error {
                    Some(e) => ui.label(RichText::new(e).color(err())),
                    None => ui.label(RichText::new("Chargement…").weak()),
                };
                return;
            };
            let f = h.filter.to_lowercase();
            let idx: Vec<usize> = if f.is_empty() {
                (0..g.commits.len()).collect()
            } else {
                (0..g.commits.len())
                    .filter(|&i| {
                        let c = &g.commits[i];
                        c.subject.to_lowercase().contains(&f)
                            || c.author.to_lowercase().contains(&f)
                            || c.hash.starts_with(&f)
                            || c.refs.iter().any(|r| r.to_lowercase().contains(&f))
                    })
                    .collect()
            };
            if idx.is_empty() {
                ui.label(RichText::new("Aucun commit").weak());
                return;
            }
            // ↑ / ↓ move the selection (and keep it in view)
            let pos = h.selected.filter(|_| h.wip_sel.is_none()).and_then(|s| idx.iter().position(|&i| i == s));
            let moved = match keys.0 {
                1 => Some(pos.map_or(0, |p| (p + 1).min(idx.len() - 1))),
                -1 => Some(pos.map_or(0, |p| p.saturating_sub(1))),
                _ => None,
            };
            if let Some(k) = moved {
                select = Some(idx[k]);
            }
            let graph_w = g.lanes.clamp(1, 24) as f32 * history::LANE_W + 6.0;
            let row_h = 22.0;
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut area = egui::ScrollArea::vertical().id_salt("history_list").auto_shrink([false, false]);
            if let Some(k) = moved {
                let id = ui.make_persistent_id("history_list");
                let top = egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |st| st.offset.y);
                let (y, view_h) = (k as f32 * row_h, ui.available_height());
                if y < top {
                    area = area.vertical_scroll_offset(y);
                } else if y + row_h > top + view_h {
                    area = area.vertical_scroll_offset(y + row_h - view_h);
                }
            }
            area.show_rows(ui, row_h, idx.len(), |ui, range| {
                for k in range {
                    let i = idx[k];
                    let c = &g.commits[i];
                    let r = history::row_ui(ui, c, &g.rows[i], graph_w, h.selected == Some(i) && h.wip_sel.is_none() && h.compare.is_none(), f.is_empty());
                    if r.clicked() || r.secondary_clicked() {
                        select = Some(i);
                    }
                    if r.double_clicked() {
                        diff_to_tab = true;
                    }
                    let sel = h.selected.filter(|&s| s != i).map(|s| g.commits[s].hash.as_str());
                    r.context_menu(|ui| {
                        commit_menu(ui, c, &g.local, sel, &mut menu_act);
                    });
                }
            });
        };
        if !has_detail {
            egui::CentralPanel::default().show_inside(ui, list);
        } else {
            let avail = ui.available_height();
            egui::TopBottomPanel::top("history_list_panel")
                .resizable(true)
                .default_height((avail * 0.36).max(140.0))
                .height_range(100.0..=(avail - 160.0).max(120.0))
                .show_inside(ui, list);

            // ------------------------------------------------ below: what is selected, then its changes (like a pull request)
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.add_space(4.0);
                if let Some(rb) = h.rebase.as_mut() {
                    ui.label(RichText::new("Rebase interactif").size(16.0).strong());
                    ui.label(
                        RichText::new(
                            "Du plus ancien (en haut) au plus récent. Change l'ordre avec ↑ ↓ et choisis quoi faire de chaque commit. \
                             « Découper » s'arrête sur le commit et ouvre l'assistant de découpage. \
                             Les commits sont réécrits : si la branche est déjà poussée, il faudra un push --force.",
                        )
                        .weak()
                        .small(),
                    );
                    ui.add_space(4.0);
                    let n = rb.steps.len();
                    let mut swap = None;
                    egui::ScrollArea::vertical().id_salt("rebase_steps").max_height(ui.available_height() - 70.0).show(ui, |ui| {
                        for (k, st) in rb.steps.iter_mut().enumerate() {
                            egui::Frame::none()
                                .fill(ui.visuals().faint_bg_color)
                                .rounding(6.0)
                                .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                                .outer_margin(egui::Margin { bottom: 3.0, ..Default::default() })
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        if ui.add_enabled(k > 0, egui::Button::new("↑").small()).on_hover_text("Plus tôt").clicked() {
                                            swap = Some((k, k - 1));
                                        }
                                        if ui.add_enabled(k + 1 < n, egui::Button::new("↓").small()).on_hover_text("Plus tard").clicked() {
                                            swap = Some((k, k + 1));
                                        }
                                        egui::ComboBox::from_id_salt(("rebase_action", k)).selected_text(st.action.label()).width(220.0).show_ui(ui, |ui| {
                                            for a in crate::rebase::Action::ALL {
                                                ui.selectable_value(&mut st.action, a, a.label());
                                            }
                                        });
                                        ui.label(RichText::new(&st.short).monospace().color(accent));
                                        let mut subj = RichText::new(&st.subject);
                                        if st.action == crate::rebase::Action::Drop {
                                            subj = subj.strikethrough().weak();
                                        }
                                        ui.add(egui::Label::new(subj).truncate());
                                    });
                                    match st.action {
                                        crate::rebase::Action::Reword => {
                                            ui.add(egui::TextEdit::multiline(&mut st.message).desired_rows(2).desired_width(f32::INFINITY));
                                        }
                                        crate::rebase::Action::Squash => {
                                            ui.label(RichText::new("↳ fusionné dans le commit gardé au-dessus (messages mis bout à bout)").small().weak());
                                        }
                                        crate::rebase::Action::Fixup => {
                                            ui.label(RichText::new("↳ fusionné dans le commit gardé au-dessus (son message est oublié)").small().weak());
                                        }
                                        crate::rebase::Action::Split => {
                                            ui.label(RichText::new("↳ le rebase s'arrête ici : tu répartis ses changements (blocs ou lignes) en plusieurs commits").small().weak());
                                        }
                                        _ => {}
                                    }
                                });
                        }
                    });
                    if let Some((a, b)) = swap {
                        rb.steps.swap(a, b);
                    }
                    ui.separator();
                    let check = crate::rebase::check(&rb.steps);
                    if let Err(e) = &check {
                        ui.label(RichText::new(e).color(err()));
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(check.is_ok(), filled_button("Lancer le rebase", accent)).clicked() {
                            rb_run = true;
                        }
                        if ui.button("Annuler").clicked() {
                            rb_cancel = true;
                        }
                    });
                    return;
                }
                // --- header of what is selected
                let (mut chips, mut file_buttons): (Vec<(char, String, Option<(Option<u32>, Option<u32>)>)>, &[(&str, &str)]) = (Vec::new(), &[]);
                if let Some((_, _, title)) = &h.compare {
                    ui.label(RichText::new(title).size(16.0).strong());
                    if let Some(d) = &h.details {
                        if !d.message.is_empty() {
                            ui.label(RichText::new(&d.message).weak().small());
                        }
                        chips = d.files.clone();
                    }
                    file_buttons = &[("Ouvrir", "Ouvrir le fichier (tel qu'il est dans tes fichiers)")];
                } else if let Some(staged) = h.wip_sel {
                    let (title, color) = if staged { ("Modifications indexées", ok()) } else { ("Modifications non indexées", warn()) };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).size(16.0).strong().color(color));
                        ui.label(
                            RichText::new(if staged {
                                "elles iront dans le prochain commit"
                            } else {
                                "pas encore indexées : « + Indexer » pour les ajouter au prochain commit"
                            })
                            .weak(),
                        );
                    });
                    if let Some(st) = &wip {
                        ui.horizontal(|ui| {
                            if staged {
                                if ui.button("Tout désindexer").clicked() {
                                    wip_ops.push(vec!["restore".into(), "--staged".into(), ".".into()]);
                                }
                            } else if ui.button("Tout indexer").clicked() {
                                wip_ops.push(vec!["add".into(), "-A".into()]);
                            }
                        });
                        chips = st
                            .entries
                            .iter()
                            .filter(|e| if staged { e.staged() } else { e.unstaged() })
                            .map(|e| {
                                let stats = if staged { st.staged_stats.get(&e.path) } else { st.unstaged_stats.get(&e.path) };
                                (if staged { e.x } else { e.label() }, e.path.clone(), stats.copied())
                            })
                            .collect();
                    }
                    file_buttons = if staged {
                        &[("Ouvrir", "Ouvrir le fichier"), ("− Désindexer", "Le retirer du prochain commit (les modifications restent dans le fichier)")]
                    } else {
                        &[
                            ("Ouvrir", "Ouvrir le fichier"),
                            ("+ Indexer", "Ajouter tout le fichier au prochain commit"),
                            ("↺ Annuler", "Remettre le fichier comme au dernier commit (demande confirmation)"),
                        ]
                    };
                } else if let Some(c) = &selected_commit {
                    match &h.details {
                        None => {
                            ui.spinner();
                        }
                        Some(d) => {
                            let (subject, body) = d.message.split_once('\n').unwrap_or((&d.message, ""));
                            ui.horizontal(|ui| {
                                ui.add(egui::Label::new(RichText::new(subject).size(16.0).strong()).truncate());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.menu_button("Actions ▾", |ui| {
                                        commit_menu(ui, c, h.graph.as_ref().map_or(&[][..], |g| &g.local[..]), None, &mut menu_act);
                                    });
                                    if ui.button("Diff dans un onglet").on_hover_text("Tout le diff du commit dans un onglet de l'éditeur (double-clic sur le commit)").clicked() {
                                        diff_to_tab = true;
                                    }
                                });
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label(RichText::new(&c.hash[..c.hash.len().min(8)]).monospace().color(accent));
                                ui.label(RichText::new(format!("· {} · {}", d.author.split(" <").next().unwrap_or(&d.author), d.date)).weak());
                                for r in &c.refs {
                                    ui.label(RichText::new(r.strip_prefix("HEAD -> ").unwrap_or(r)).small().color(accent).background_color(icons::mix(ui.visuals().panel_fill, accent, 0.15)));
                                }
                            });
                            let body = body.trim();
                            if !body.is_empty() {
                                egui::CollapsingHeader::new(RichText::new("Message complet").small().weak())
                                    .default_open(body.lines().count() <= 6)
                                    .show(ui, |ui| {
                                        egui::ScrollArea::vertical().id_salt("commit_msg").max_height(120.0).auto_shrink([false, true]).show(ui, |ui| {
                                            ui.add(egui::Label::new(RichText::new(history::reflow(body)).weak()).wrap());
                                        });
                                    });
                            }
                            chips = d.files.clone();
                        }
                    }
                    file_buttons = &[
                        ("Ouvrir", "Ouvrir le fichier (tel qu'il est dans tes fichiers)"),
                        ("↺ Annuler dans mes fichiers", "Défaire ce que ce commit a changé dans ce fichier, dans tes fichiers actuels (à valider ensuite)"),
                    ];
                }
                // --- files: summary, then a chip per file (click: go to its diff)
                let (adds, dels) = chips.iter().fold((0u32, 0u32), |(a, r), (_, _, s)| match s {
                    Some((x, y)) => (a + x.unwrap_or(0), r + y.unwrap_or(0)),
                    _ => (a, r),
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("FICHIERS MODIFIÉS · {}", chips.len())).small().strong().color(accent));
                    ui.label(RichText::new(format!("+{adds}")).small().monospace().color(ok()));
                    ui.label(RichText::new(format!("−{dels}")).small().monospace().color(err()));
                    if h.diff_loading {
                        ui.spinner();
                    }
                    if let Some(d) = &h.diff {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !d.files.is_empty() {
                                if ui.small_button("Tout déplier").clicked() {
                                    h.collapsed.clear();
                                }
                                if ui.small_button("Tout replier").clicked() {
                                    h.collapsed = d.files.iter().map(|f| f.start).collect();
                                }
                            }
                        });
                    }
                });
                if !chips.is_empty() {
                    egui::ScrollArea::vertical().id_salt("diff_chips").max_height(64.0).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                            for (letter, path, _) in &chips {
                                let name = path.rsplit('/').next().unwrap_or(path);
                                let c = match letter {
                                    'A' | '?' => ok(),
                                    'D' => err(),
                                    _ => warn(),
                                };
                                let text = RichText::new(format!("{letter} {name}")).small().color(c);
                                if ui.add(egui::Button::new(text).small()).on_hover_text(path.as_str()).clicked() {
                                    if let Some(k) = h.diff.as_ref().and_then(|d| d.files.iter().position(|f| f.path == *path)) {
                                        if let Some(start) = h.diff.as_ref().map(|d| d.files[k].start) {
                                            h.collapsed.remove(&start);
                                        }
                                        h.jump = Some(history::FileJump::To(k));
                                    }
                                }
                            }
                        });
                    });
                }
                ui.separator();
                // --- every file with its diff, one under the other
                match &h.diff {
                    Some(d) => {
                        let hunk_buttons: &[(&str, &str)] = match source {
                            DiffSource::Worktree => &[
                                ("+ Indexer ce bloc", "Ajouter ce bloc au prochain commit"),
                                ("↺ Annuler ce bloc", "Supprimer ce changement du fichier (demande confirmation)"),
                            ],
                            DiffSource::Index => &[("− Désindexer ce bloc", "Retirer ce bloc du prochain commit (le changement reste dans le fichier)")],
                            DiffSource::Commit => &[("↺ Annuler ce changement", "Défaire ce bloc dans tes fichiers actuels (à valider ensuite)")],
                            DiffSource::Compare => &[],
                        };
                        let jump = h.jump.take();
                        if let Some(c) = history::stacked_diff_view(ui, &d.lines, &d.files, &mut h.collapsed, "history_stacked", hunk_buttons, file_buttons, jump) {
                            if let history::DiffClick::File(_, b) = c {
                                file_act = match (source, b) {
                                    (_, 0) => Some("open"),
                                    (DiffSource::Worktree, 1) => Some("stage"),
                                    (DiffSource::Worktree, _) => Some("discard"),
                                    (DiffSource::Index, _) => Some("unstage"),
                                    (DiffSource::Commit, _) => Some("revert"),
                                    (DiffSource::Compare, _) => None,
                                };
                            }
                            diff_click = Some(c);
                        }
                    }
                    None if h.diff_loading => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Chargement du diff…").weak());
                        });
                    }
                    None => {}
                }
            });
        }

        if close {
            self.history.open = false;
        }
        if let Some(r) = chosen_repo {
            self.select_repo(r.clone());
            // the panel may already be on it while the view still shows another one
            if self.history.repo.as_ref() != Some(&r) {
                self.load_history();
            }
        }
        if clear_notice {
            self.history.notice = None;
        }
        if diff_click.is_some() || file_act.is_some() {
            self.diff_action(source, diff_click, file_act);
        }
        if diff_to_tab {
            match &self.history.diff {
                Some(d) => {
                    let short = &d.hash[..d.hash.len().min(8)];
                    let title = match &d.path {
                        Some(p) => format!("{short} : {p}"),
                        None => format!("diff {short}"),
                    };
                    let text = d.text.clone();
                    self.history.open = false;
                    self.new_scratch(title, text, "Diff", true);
                }
                None => self.set_status("Le diff est en cours de chargement"),
            }
        }
        if resume {
            match self.split.as_mut() {
                Some(s) => s.shown = true,
                None => self.resume_split(),
            }
        }
        if abort_rebase {
            if let Some(repo) = self.history.repo.clone() {
                self.git_job_at(repo, "rebase --abort", &["rebase", "--abort"]);
            }
        }
        if let Some(a) = menu_act {
            let repo = self.history.repo.clone();
            match (a, repo) {
                (HistAct::Checkout(target), Some(repo)) => self.git_job_at(repo, "checkout", &["checkout", &target]),
                (HistAct::BranchAt(hash), _) => {
                    self.open_overlay_with(Mode::Prompt(Prompt::BranchAt(hash)), Vec::new(), String::new())
                }
                (HistAct::Copy(text), _) => {
                    ui.ctx().output_mut(|o| o.copied_text = text.clone());
                    self.set_status(format!("Copié : {text}"));
                }
                (HistAct::Diff(hash), _) => {
                    if let Some(i) = self.history.graph.as_ref().and_then(|g| g.commits.iter().position(|c| c.hash == hash)) {
                        select = Some(i);
                    }
                }
                (HistAct::Split(hash), _) => self.start_split(hash),
                (HistAct::CherryPick(hash), Some(repo)) => self.git_job_at(repo, "cherry-pick", &["-c", "core.editor=true", "cherry-pick", &hash]),
                (HistAct::Revert(hash), Some(repo)) => self.git_job_at(repo, "revert", &["-c", "core.editor=true", "revert", "--no-edit", &hash]),
                (HistAct::Compare(a, b, title), _) => {
                    self.start_compare(a.clone(), b.clone(), title);
                    self.load_range_diff(a, b, None);
                }
                (HistAct::Rebase(hash), Some(repo)) => match crate::rebase::load(&repo, &hash) {
                    Ok(steps) => {
                        let h = &mut self.history;
                        h.rebase = Some(RebaseUi { from: hash, steps });
                        h.compare = None;
                        h.wip_sel = None;
                        h.diff = None;
                    }
                    Err(e) => self.history.notice = Some((e, err())),
                },
                _ => {}
            }
        }
        if reload {
            self.load_history();
            self.git_refresh();
        }
        if let Some(args) = op_cmd {
            if let Some(repo) = self.history.repo.clone() {
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                self.git_job_at(repo, "rebase/merge", &a);
            }
        }
        if rb_cancel {
            self.history.rebase = None;
        }
        if rb_run {
            if let (Some(rb), Some(repo)) = (self.history.rebase.take(), self.history.repo.clone()) {
                self.git.busy += 1;
                self.set_status("Rebase en cours…");
                self.jobs.spawn(move || JobResult::GitOp {
                    label: if crate::rebase::splits(&rb.steps) { "rebase (découpage)".into() } else { "rebase".into() },
                    result: crate::rebase::run(&repo, &rb.from, &rb.steps, crate::rebase::self_editor),
                });
            }
        }
        if let Some(staged) = wip_pick {
            let h = &mut self.history;
            h.compare = None;
            h.rebase = None;
            h.wip_sel = Some(staged);
            h.diff = None;
            h.diff_loading = false;
            self.load_wip_all(staged);
        }
        if !wip_ops.is_empty() {
            if let Some(repo) = self.history.repo.clone() {
                for args in wip_ops {
                    let a: Vec<&str> = args.iter().map(String::as_str).collect();
                    let label = if args[0] == "add" { "add" } else { "restore" };
                    self.git_job_at(repo.clone(), label, &a);
                }
            }
        }
        if let Some(i) = select {
            if Some(i) != self.history.selected || self.history.wip_sel.is_some() || self.history.compare.is_some() || self.history.rebase.is_some() {
                let h = &mut self.history;
                h.wip_sel = None;
                h.compare = None;
                h.rebase = None;
                h.diff = None;
                h.diff_loading = false;
                self.select_commit(i);
            }
        }
    }

    // ------------------------------------------------------------------ commit split assistant

    fn start_split(&mut self, hash: String) {
        let Some(repo) = self.history.repo.clone() else { return };
        if self.docs.iter().any(|d| d.is_dirty()) {
            self.history.notice = Some(("Enregistre d'abord les fichiers modifiés avant de découper un commit.".into(), err()));
            return;
        }
        let editor = match split::self_editor() {
            Ok(e) => e,
            Err(e) => {
                self.history.notice = Some((e, err()));
                return;
            }
        };
        self.set_status("Préparation du découpage…");
        self.history.loading = true;
        self.jobs.spawn(move || {
            let r = split::start(&repo, &hash, &editor).and_then(|info| {
                let files = split::changes(&repo, &info.added)?;
                Ok((info, files))
            });
            JobResult::SplitStarted(repo, r)
        });
    }

    /// Reopens the assistant on a rebase left stopped (e.g. Florm was closed during a split).
    fn resume_split(&mut self) {
        let Some(repo) = self.history.repo.clone() else { return };
        self.jobs.spawn(move || {
            let r = split::stopped_commit(&repo)
                .ok_or_else(|| "Le rebase en cours n'est pas arrêté sur un commit (conflit ?) : utilise le terminal.".to_string())
                .and_then(|hash| split::info(&repo, &hash))
                .and_then(|info| {
                    let files = split::changes(&repo, &info.added)?;
                    Ok((info, files))
                });
            JobResult::SplitStarted(repo, r)
        });
    }

    fn split_commit_part(&mut self) {
        let Some(s) = self.split.as_mut() else { return };
        let (repo, files, sel, msg, added) =
            (s.repo.clone(), s.files.clone(), s.sel.clone(), s.message.trim().to_string(), s.info.added.clone());
        s.busy = true;
        s.error = None;
        self.jobs.spawn(move || match split::commit_part(&repo, &files, &sel, &msg) {
            Ok(c) => JobResult::SplitChanges { result: split::changes(&repo, &added), committed: Some(c) },
            Err(e) => JobResult::SplitChanges { result: Err(e), committed: None },
        });
    }

    fn split_end(&mut self, aborted: bool) {
        let Some(s) = self.split.as_mut() else { return };
        let repo = s.repo.clone();
        s.busy = true;
        s.error = None;
        self.jobs.spawn(move || JobResult::SplitEnded {
            aborted,
            result: if aborted { split::abort(&repo) } else { split::finish(&repo) },
        });
    }

    fn after_split_change(&mut self) {
        self.git_refresh();
        if let Some(t) = self.tree.as_mut() {
            t.refresh();
        }
        // files rewritten by git: reload the unmodified ones right away
        self.last_disk_check = Instant::now() - Duration::from_secs(10);
    }

    fn split_ui(&mut self, ui: &mut egui::Ui) {
        let accent = self.hl.colors.accent;
        let Some(s) = self.split.as_mut() else { return };
        let part = s.parts.len() + 1;
        // hunks with at least one chosen line
        let n_sel: usize = s
            .files
            .iter()
            .zip(&s.sel)
            .map(|(f, fs)| f.hunks.iter().zip(fs).filter(|(h, hs)| split::hunk_flags(h, hs).iter().any(|&b| b)).count())
            .sum();
        let n_hunks: usize = s.sel.iter().map(Vec::len).sum();
        let (mut commit, mut finish, mut abort) = (false, false, false);
        let bg = ui.visuals().panel_fill;

        egui::TopBottomPanel::top("split_top").show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("✂ Découper le commit").size(17.0).strong());
                ui.label(RichText::new(&s.info.short).monospace().color(accent).size(15.0));
                ui.label(RichText::new(&s.info.subject).size(15.0));
                if s.busy {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if s.confirm_abort {
                        if ui.button("Non, continuer").clicked() {
                            s.confirm_abort = false;
                        }
                        let b = filled_button("Oui, tout annuler", err());
                        if ui.add_enabled(!s.busy, b).clicked() {
                            abort = true;
                        }
                        ui.label(RichText::new("Remettre le commit d'origine et supprimer les parties ?").color(err()));
                    } else if ui.add_enabled(!s.busy, egui::Button::new("Annuler le découpage")).clicked() {
                        s.confirm_abort = true;
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let step = |ui: &mut egui::Ui, n: &str, t: &str, on: bool| {
                    let c = if on { accent } else { ui.visuals().weak_text_color() };
                    ui.label(RichText::new(n).strong().color(c));
                    ui.label(RichText::new(t).color(if on { ui.visuals().strong_text_color() } else { c }));
                };
                let left = !s.files.is_empty();
                step(ui, "1.", &format!("Coche les blocs (ou les lignes + / −) de la partie {part}"), left && n_sel == 0);
                ui.label(RichText::new("›").weak());
                step(ui, "2.", "Écris son message", left && n_sel > 0 && s.message.trim().is_empty());
                ui.label(RichText::new("›").weak());
                step(ui, "3.", &format!("« Valider la partie {part} »"), left && n_sel > 0 && !s.message.trim().is_empty());
                ui.label(RichText::new("›").weak());
                step(ui, "4.", "Recommence, puis « Terminer » quand tout est validé", !left);
            });
            if let Some(e) = &s.error {
                ui.label(RichText::new(format!("⚠ {e}")).color(err()));
            }
            ui.add_space(4.0);
        });

        egui::SidePanel::left("split_parts").resizable(true).default_width(260.0).show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("PARTIES").small().strong().color(accent));
            ui.add_space(4.0);
            for (i, (h, subj)) in s.parts.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("✔ {}", i + 1)).color(ok()).strong());
                    ui.label(RichText::new(h).monospace().weak());
                    ui.label(subj);
                });
            }
            if !s.files.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("● {part}")).color(accent).strong());
                    ui.label(RichText::new("en cours").italics());
                });
            }
            ui.add_space(10.0);
            ui.label(RichText::new("COMMIT D'ORIGINE").small().strong().weak());
            egui::ScrollArea::vertical().id_salt("split_orig_msg").show(ui, |ui| {
                ui.label(RichText::new(&s.info.message).weak());
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Les commits qui suivaient seront rejoués automatiquement à la fin. \
                         « Annuler le découpage » remet tout exactement comme avant.",
                    )
                    .small()
                    .weak(),
                );
            });
        });

        egui::TopBottomPanel::bottom("split_commit").show_inside(ui, |ui| {
            ui.add_space(6.0);
            if s.files.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("✔ Tout est validé").color(ok()).size(16.0).strong());
                    ui.label(format!("— le commit d'origine sera remplacé par {} commit(s).", s.parts.len()));
                });
                ui.add_space(4.0);
                let b = filled_button("Terminer le découpage", ok())
                    .min_size(egui::vec2(220.0, 30.0));
                if ui.add_enabled(!s.busy, b).clicked() {
                    finish = true;
                }
            } else {
                ui.add(
                    egui::TextEdit::multiline(&mut s.message)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text(format!("Message du commit de la partie {part}"))
                        .font(egui::TextStyle::Monospace),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let ready = n_sel > 0 && !s.message.trim().is_empty() && !s.busy;
                    let label = format!("Valider la partie {part}  ({n_sel} / {n_hunks} bloc(s))");
                    let b = filled_button(label, accent).min_size(egui::vec2(0.0, 28.0));
                    let r = ui.add_enabled(ready, b);
                    if r.clicked() {
                        commit = true;
                    }
                    if !ready && !s.busy {
                        let why = if n_sel == 0 { "coche au moins un bloc" } else { "écris un message" };
                        ui.label(RichText::new(format!("({why})")).weak());
                    }
                    ui.add_space(12.0);
                    if ui.button("Tout cocher").on_hover_text("Le reste va dans cette partie (la dernière)").clicked() {
                        s.sel.iter_mut().flatten().flatten().for_each(|b| *b = true);
                    }
                    if ui.button("Rien").clicked() {
                        s.sel.iter_mut().flatten().flatten().for_each(|b| *b = false);
                    }
                });
            }
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if s.files.is_empty() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Il ne reste aucun changement à répartir.").size(15.0));
                });
                return;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("CHANGEMENTS RESTANTS").small().strong().color(accent));
                ui.label(RichText::new(format!("{} fichier(s), {n_hunks} bloc(s)", s.files.len())).small().weak());
            });
            ui.add_space(4.0);
            let mono = egui::TextStyle::Monospace.resolve(ui.style());
            let fg = ui.visuals().text_color();
            egui::ScrollArea::vertical().id_salt("split_changes").auto_shrink([false, false]).show(ui, |ui| {
                for (fi, f) in s.files.iter().enumerate() {
                    let sel = &mut s.sel[fi];
                    let chosen = split::hunk_flags;
                    let flags: Vec<bool> = f.hunks.iter().zip(sel.iter()).flat_map(|(h, hs)| chosen(h, hs)).collect();
                    let all = !flags.is_empty() && flags.iter().all(|&b| b);
                    let any = flags.iter().any(|&b| b);
                    let folded = s.folded.contains(&f.path);
                    let partial = f.partial_ok();
                    ui.horizontal(|ui| {
                        let mut v = all;
                        if ui.add(egui::Checkbox::new(&mut v, "").indeterminate(any && !all)).changed() {
                            sel.iter_mut().flatten().for_each(|b| *b = v);
                        }
                        let arrow = if folded { "⏵" } else { "⏷" };
                        if ui.add(egui::Button::new(arrow).frame(false)).clicked() {
                            if folded {
                                s.folded.remove(&f.path);
                            } else {
                                s.folded.insert(f.path.clone());
                            }
                        }
                        let (kc, kt) = match f.kind {
                            'A' => (ok(), "nouveau"),
                            'D' => (err(), "supprimé"),
                            _ => (warn(), "modifié"),
                        };
                        ui.label(RichText::new(&f.path).strong());
                        ui.label(RichText::new(kt).small().color(kc));
                        if f.binary {
                            ui.label(RichText::new("binaire").small().weak());
                        }
                        if !partial {
                            ui.label(RichText::new("(se valide en entier)").small().weak());
                        } else if f.hunks.len() > 1 {
                            ui.label(RichText::new(format!("{} blocs", f.hunks.len())).small().weak());
                        }
                    });
                    if folded {
                        continue;
                    }
                    for (hi, h) in f.hunks.iter().enumerate() {
                        let hflags = chosen(h, &sel[hi]);
                        let on = hflags.iter().any(|&b| b);
                        let whole = !hflags.is_empty() && hflags.iter().all(|&b| b);
                        let stroke = if on { egui::Stroke::new(2.0_f32, accent) } else { ui.visuals().widgets.noninteractive.bg_stroke };
                        let fill = if on { icons::mix(bg, accent, 0.10) } else { ui.visuals().faint_bg_color };
                        let mut set_all: Option<bool> = None;
                        let mut toggle_line: Option<usize> = None;
                        let resp = egui::Frame::none()
                            .fill(fill)
                            .stroke(stroke)
                            .rounding(6.0)
                            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                            .outer_margin(egui::Margin { left: 26.0, right: 4.0, top: 2.0, bottom: 4.0 })
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let mut v = whole;
                                    let label = match (whole, on) {
                                        (true, _) => "tout le bloc dans cette partie",
                                        (false, true) => "une partie des lignes dans cette partie",
                                        _ => "pas dans cette partie",
                                    };
                                    if ui.add(egui::Checkbox::new(&mut v, RichText::new(label).small()).indeterminate(on && !whole)).changed() {
                                        set_all = Some(v);
                                    }
                                    ui.label(RichText::new(&h.lines[0]).monospace().small().color(accent));
                                    if partial {
                                        ui.label(RichText::new("· clic sur une ligne + / − pour la prendre ou la laisser").small().weak());
                                    }
                                });
                                let shown = h.lines.len().min(300);
                                for (li, l) in h.lines.iter().enumerate().take(shown).skip(1) {
                                    let change = h.is_change(li);
                                    let line_on = sel[hi][li];
                                    let (c, lbg, mark) = match l.chars().next() {
                                        Some('+') if line_on => (fg, Some(icons::mix(fill, ok(), 0.22)), "☑ "),
                                        Some('-') if line_on => (fg, Some(icons::mix(fill, err(), 0.22)), "☑ "),
                                        // not chosen: an added line is left out, a removed one stays as it is
                                        Some('+') | Some('-') => (ui.visuals().weak_text_color(), None, "☐ "),
                                        _ => (ui.visuals().weak_text_color(), None, "   "),
                                    };
                                    let mut fmt = TextFormat::simple(mono.clone(), c);
                                    if let Some(b) = lbg {
                                        fmt.background = b;
                                    }
                                    if change && !line_on && l.starts_with('+') {
                                        fmt.strikethrough = egui::Stroke::new(1.0_f32, c);
                                    }
                                    let text = if partial && change { format!("{mark}{}", l.replace('\t', "    ")) } else { l.replace('\t', "    ") };
                                    let mut job = LayoutJob::single_section(text, fmt);
                                    job.wrap.max_rows = 1;
                                    let sense = if partial && change { Sense::click() } else { Sense::hover() };
                                    let r = ui.add(egui::Label::new(job).sense(sense));
                                    if partial && change && r.clicked() {
                                        toggle_line = Some(li);
                                    }
                                }
                                if h.lines.len() > shown {
                                    ui.label(RichText::new(format!("… {} ligne(s) de plus", h.lines.len() - shown)).weak().small());
                                }
                            })
                            .response;
                        // clicking the block elsewhere than on a line toggles it whole
                        if toggle_line.is_none() && set_all.is_none() && resp.interact(Sense::click()).clicked() {
                            set_all = Some(!on);
                        }
                        if let Some(li) = toggle_line {
                            sel[hi][li] = !sel[hi][li];
                        }
                        if let Some(v) = set_all {
                            if partial {
                                sel[hi].iter_mut().for_each(|b| *b = v);
                            } else {
                                sel.iter_mut().flatten().for_each(|b| *b = v);
                            }
                        }
                    }
                    ui.add_space(6.0);
                }
            });
        });

        if commit {
            self.split_commit_part();
        }
        if finish {
            self.split_end(false);
        }
        if abort {
            self.split_end(true);
        }
    }

    // ------------------------------------------------------------------ editor panels (split)

    fn is_split(&self) -> bool {
        self.docs.iter().any(|d| d.pane == 1)
    }

    /// Places new documents in the active panel, closes a panel left empty, and makes the
    /// active panel follow the active document (however it was activated).
    fn normalize_panes(&mut self) {
        let focused = self.focused_pane;
        for d in self.docs.iter_mut().filter(|d| d.pane == document::NO_PANE) {
            d.pane = focused;
        }
        if !self.docs.iter().any(|d| d.pane == 0) {
            self.docs.iter_mut().for_each(|d| d.pane = 0);
        }
        self.focused_pane = self.docs.get(self.active).map_or(0, |d| d.pane);
        if let Some(d) = self.docs.get(self.active) {
            self.pane_active[self.focused_pane as usize] = Some(d.uid);
        }
        let other = 1 - self.focused_pane as usize;
        let ok = self.pane_active[other].is_some_and(|u| self.docs.iter().any(|d| d.uid == u && d.pane as usize == other));
        if !ok {
            self.pane_active[other] = self.docs.iter().rev().find(|d| d.pane as usize == other).map(|d| d.uid);
        }
    }

    /// Document shown in `pane`.
    fn pane_doc(&self, pane: u8) -> Option<usize> {
        if pane == self.focused_pane {
            return (self.active < self.docs.len()).then_some(self.active);
        }
        let uid = self.pane_active[pane as usize]?;
        self.docs.iter().position(|d| d.uid == uid)
    }

    /// Sends tab `i` to the other panel (creating the split in `dir` if needed) and focuses it.
    fn split_tab(&mut self, i: usize, dir: Option<SplitDir>) {
        let Some(d) = self.docs.get(i) else { return };
        let pane = d.pane;
        let alone = self.docs.iter().filter(|x| x.pane == pane).count() == 1;
        if alone && !self.is_split() {
            self.set_status("Ouvre un deuxième fichier pour diviser l'éditeur (un fichier ne peut être que dans un panneau).");
            return;
        }
        if let Some(dir) = dir {
            self.split_dir = dir;
        }
        self.docs[i].pane = 1 - pane;
        self.activate_tab(i);
    }

    fn unsplit(&mut self) {
        self.docs.iter_mut().for_each(|d| d.pane = 0);
        self.focused_pane = 0;
    }

    fn activate_tab(&mut self, i: usize) {
        self.active = i;
        self.history.open = false;
        self.prefs.open = false;
        if let Some(sp) = self.split.as_mut() {
            sp.shown = false;
        }
        self.pending_focus = true;
    }

    /// One panel of a split editor: its tabs and its editor.
    fn pane_ui(&mut self, ui: &mut egui::Ui, pane: u8) -> Option<egui::Response> {
        self.tabs_strip(ui, Some(pane));
        let rect = ui.available_rect_before_wrap();
        self.drop_zones.panes.push((pane, rect));
        let i = self.pane_doc(pane)?;
        self.breadcrumbs_ui(ui, i, pane);
        self.conflict_bar(ui, i, pane);
        let focused = pane == self.focused_pane;
        let id = if focused { editor_id() } else { editor_view::other_editor_id() };
        let r = self.doc_body(ui, i, id, focused)?;
        focused.then_some(r)
    }

    /// Body of tab `i`: the editor, an image, or a Markdown preview. Returns the editor's response.
    fn doc_body(&mut self, ui: &mut egui::Ui, i: usize, id: egui::Id, focused: bool) -> Option<egui::Response> {
        match self.docs.get(i)?.view {
            document::View::Text => {
                let matches: &[(usize, usize)] = if focused && self.find.open { &self.find.matches } else { &[] };
                let doc = self.docs.get_mut(i)?;
                Some(editor_view::show(ui, doc, &self.hl, self.settings.font_size, self.settings.show_minimap, self.settings.inline_diff, matches, id))
            }
            document::View::Image => {
                self.image_view(ui, i);
                None
            }
            document::View::Preview(src) => {
                self.markdown_view(ui, src);
                None
            }
            document::View::Help => {
                self.render_markdown(ui, "guide", include_str!("../assets/guide.md"), "file://".into());
                None
            }
            document::View::Search => {
                let bg = ui.visuals().panel_fill;
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin { left: 12.0, right: 12.0, top: 10.0, bottom: 4.0 }))
                    .show_inside(ui, |ui| self.search_ui(ui, true));
                None
            }
            document::View::Terminal(id) => {
                if self.term.cwd.is_none() {
                    self.term.cwd = self.root.clone();
                }
                let shell = self.term_shell();
                self.term.tab_ui(ui, id, shell.as_deref(), self.hl.colors, self.hl.is_light());
                None
            }
        }
    }

    /// Adds a read-only tab showing `view` (search, terminal) in the focused panel and shows it.
    fn open_view_tab(&mut self, view: document::View, title: String) {
        if let Some(i) = self.docs.iter().position(|d| d.view == view) {
            self.activate_tab(i);
            return;
        }
        let uid = self.next_uid();
        let mut d = Document::new(uid, self.settings.tab_size, self.settings.translate_tabs_to_spaces);
        d.view = view;
        d.read_only = true;
        d.title_override = Some(title);
        self.docs.push(d);
        self.activate_tab(self.docs.len() - 1);
    }

    /// The "find in files" panel in an editor tab (instead of the bottom panel).
    fn open_search_tab(&mut self) {
        if self.bottom == Some(BottomTab::Search) {
            self.bottom = None;
        }
        self.open_view_tab(document::View::Search, "🔍 Recherche".into());
        self.search.focus = true;
    }

    /// A new terminal in an editor tab (it can be split beside a file like any tab).
    fn open_terminal_tab(&mut self) {
        if self.term.cwd.is_none() {
            self.term.cwd = self.root.clone();
        }
        let shell = self.term_shell();
        let ctx = self.egui_ctx.clone();
        match self.term.open_tab(&ctx, shell.as_deref(), None) {
            Ok((id, title)) => self.open_view_tab(document::View::Terminal(id), format!(">_ {title}")),
            Err(e) => self.set_status(e),
        }
    }

    /// Terminals moved into the editor ("⇱ Onglet"), or started from a terminal tab, get their tab;
    /// terminal tabs follow the title of their shell.
    fn sync_terminal_tabs(&mut self) {
        if let Some(id) = self.term.dock_request.take() {
            self.term.set_docked(id, true);
            if self.bottom == Some(BottomTab::Terminal) && !self.term.has_panel_sessions() {
                // the panel would open a new terminal at once: close it instead
                self.bottom = None;
            }
        }
        for id in self.term.docked_ids() {
            if !self.docs.iter().any(|d| d.view == document::View::Terminal(id)) {
                let title = self.term.title_of(id).unwrap_or_default();
                self.open_view_tab(document::View::Terminal(id), format!(">_ {title}"));
            }
        }
        for d in self.docs.iter_mut() {
            if let document::View::Terminal(id) = d.view {
                if let Some(t) = self.term.title_of(id) {
                    let t = format!(">_ {t}");
                    if d.title_override.as_ref() != Some(&t) {
                        d.title_override = Some(t);
                    }
                }
            }
        }
    }

    fn image_view(&mut self, ui: &mut egui::Ui, i: usize) {
        let Some(path) = self.docs[i].path.clone() else { return };
        let uri = format!("file://{}", path.display());
        let bg = self.hl.colors.bg;
        let mut zoom = self.docs[i].zoom;
        egui::TopBottomPanel::top(egui::Id::new("image_bar").with(i)).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let size = egui::Image::new(uri.as_str()).load_for_size(ui.ctx(), egui::Vec2::splat(4096.0)).ok().and_then(|t| t.size());
                if let Some(s) = size {
                    ui.label(RichText::new(format!("{} × {} px", s.x as u32, s.y as u32)).strong());
                }
                if let Ok(m) = std::fs::metadata(&path) {
                    let kb = m.len() as f64 / 1024.0;
                    ui.label(RichText::new(if kb < 1024.0 { format!("{kb:.1} Ko") } else { format!("{:.1} Mo", kb / 1024.0) }).weak());
                }
                ui.separator();
                if ui.selectable_label(zoom == 0.0, "Ajuster").clicked() {
                    zoom = 0.0;
                }
                for (label, z) in [("50 %", 0.5), ("100 %", 1.0), ("200 %", 2.0)] {
                    if ui.selectable_label(zoom == z, label).clicked() {
                        zoom = z;
                    }
                }
                if ui.button("Afficher dans le gestionnaire de fichiers").clicked() {
                    let _ = crate::os::reveal(&path);
                }
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::none().fill(bg)).show_inside(ui, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                let avail = ui.available_size();
                let img = egui::Image::new(uri.as_str()).show_loading_spinner(true);
                let img = match zoom {
                    0.0 => img.max_size(avail - egui::vec2(24.0, 24.0)).fit_to_original_size(1.0).maintain_aspect_ratio(true),
                    z => img.fit_to_original_size(z),
                };
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    ui.add(img);
                });
            });
        });
        self.docs[i].zoom = zoom;
    }

    fn markdown_view(&mut self, ui: &mut egui::Ui, src: u64) {
        let Some(d) = self.docs.iter().find(|d| d.uid == src) else {
            ui.label(RichText::new("Le fichier Markdown de cet aperçu a été fermé.").weak());
            return;
        };
        let text = d.rope.to_string();
        let base = d.path.as_deref().and_then(Path::parent).map(|p| format!("file://{}/", p.display())).unwrap_or_else(|| "file://".into());
        self.render_markdown(ui, ("md_preview", src), &text, base);
    }

    fn render_markdown(&mut self, ui: &mut egui::Ui, id: impl std::hash::Hash, text: &str, base: String) {
        let bg = self.hl.colors.bg;
        let cache = &mut self.md_cache;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(bg)).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
                let w = ui.available_width().min(880.0);
                let pad = ((ui.available_width() - w) / 2.0).max(16.0);
                egui::Frame::none().inner_margin(egui::Margin { left: pad, right: pad, top: 16.0, bottom: 40.0 }).show(ui, |ui| {
                    ui.set_max_width(w);
                    egui_commonmark::CommonMarkViewer::new()
                        .default_implicit_uri_scheme(base)
                        .max_image_width(Some(w as usize))
                        .show(ui, cache, text);
                });
            });
        });
    }

    /// Opens (or shows) the rendered preview of the active Markdown file, in its panel or beside it.
    fn markdown_preview(&mut self, side: bool) {
        let Some(d) = self.docs.get(self.active) else { return };
        let is_md = d.syntax.contains("Markdown") || d.path.as_deref().is_some_and(|p| p.extension().is_some_and(|e| e == "md" || e == "markdown"));
        if !is_md {
            self.set_status("L'aperçu est pour les fichiers Markdown (.md)");
            return;
        }
        let (src, title, src_pane) = (d.uid, d.title(), d.pane);
        let src_idx = self.active;
        if let Some(i) = self.docs.iter().position(|x| x.view == document::View::Preview(src)) {
            if side {
                self.docs[i].pane = 1 - src_pane;
                self.active = src_idx;
            } else {
                self.activate_tab(i);
            }
            return;
        }
        let uid = self.next_uid();
        let mut p = Document::new(uid, self.settings.tab_size, self.settings.translate_tabs_to_spaces);
        p.view = document::View::Preview(src);
        p.read_only = true;
        p.title_override = Some(format!("Aperçu : {title}"));
        p.pane = if side { 1 - src_pane } else { src_pane };
        self.docs.push(p);
        if side {
            // keep typing in the source, the preview follows beside it
            self.split_dir = SplitDir::Right;
            self.active = src_idx;
        } else {
            self.active = self.docs.len() - 1;
        }
    }

    /// Bar above a file with merge conflicts: navigation and "keep mine / theirs / both".
    fn conflict_bar(&mut self, ui: &mut egui::Ui, i: usize, pane: u8) {
        let path = self.docs.get(i).and_then(|d| d.path.clone());
        let in_repo = path.as_deref().and_then(|p| self.repo_for(p)).is_some();
        let Some(d) = self.docs.get_mut(i) else { return };
        let conflicts = d.conflicts();
        if conflicts.is_empty() && !d.had_conflicts {
            return;
        }
        let line = d.rope.char_to_line(d.primary_sel().head);
        let k = conflicts.iter().position(|c| line <= c.end).unwrap_or(0);
        let (mut act, mut goto, mut resolved) = (None, None, false);
        let bg = icons::mix(ui.visuals().panel_fill, if conflicts.is_empty() { ok() } else { warn() }, 0.15);
        egui::TopBottomPanel::top(egui::Id::new("conflicts").with(pane))
            .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(10.0, 4.0)))
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if conflicts.is_empty() {
                        ui.label(RichText::new("✔ Tous les conflits de ce fichier sont résolus").color(ok()).strong());
                        if in_repo && ui.button("Enregistrer et marquer comme résolu (git add)").clicked() {
                            resolved = true;
                        }
                        return;
                    }
                    let c = &conflicts[k];
                    ui.label(RichText::new(format!("⚠ Conflit de merge {}/{}", k + 1, conflicts.len())).color(warn()).strong());
                    if ui.small_button("◀").on_hover_text("Conflit précédent").clicked() {
                        goto = Some(conflicts[(k + conflicts.len() - 1) % conflicts.len()].start);
                    }
                    if ui.small_button("▶").on_hover_text("Conflit suivant").clicked() {
                        goto = Some(conflicts[(k + 1) % conflicts.len()].start);
                    }
                    ui.add_space(8.0);
                    let mine = if c.ours.is_empty() { "Garder le mien".to_string() } else { format!("Garder le mien ({})", c.ours) };
                    let theirs = if c.theirs.is_empty() { "Garder le leur".to_string() } else { format!("Garder le leur ({})", c.theirs) };
                    if ui.button(RichText::new(mine).color(highlight::green())).on_hover_text("Ta version (en vert)").clicked() {
                        act = Some((c.clone(), document::Resolution::Ours));
                    }
                    if ui.button(RichText::new(theirs).color(highlight::blue())).on_hover_text("La version qui arrive (en bleu)").clicked() {
                        act = Some((c.clone(), document::Resolution::Theirs));
                    }
                    if ui.button("Garder les deux").on_hover_text("La tienne puis l'autre").clicked() {
                        act = Some((c.clone(), document::Resolution::Both));
                    }
                    ui.label(RichText::new("(Ctrl+Z pour revenir)").weak().small());
                });
            });
        let d = &mut self.docs[i];
        if let Some((c, how)) = act {
            d.resolve_conflict(&c, how);
            self.active = i;
            self.pending_focus = true;
        }
        if let Some(l) = goto {
            d.goto_line(l, 0);
            self.active = i;
        }
        if resolved {
            d.had_conflicts = false;
            if self.save(i) {
                if let Some(p) = self.docs[i].path.clone() {
                    if let Some(repo) = self.repo_for(&p) {
                        let rel = p.strip_prefix(&repo).unwrap_or(&p).to_string_lossy().into_owned();
                        self.git_job_at(repo, "add", &["add", "--", &rel]);
                    }
                }
            }
        }
    }

    /// "dossier › fichier › Classe › méthode" above the editor; a click goes to the symbol.
    fn breadcrumbs_ui(&mut self, ui: &mut egui::Ui, i: usize, pane: u8) {
        if !self.settings.breadcrumbs {
            return;
        }
        let root = self.root.clone();
        let accent = self.hl.colors.accent;
        let Some(d) = self.docs.get_mut(i) else { return };
        let line = d.rope.char_to_line(d.primary_sel().head);
        // folders of the path (relative to the project), with their absolute path for the menus
        let mut dirs: Vec<(String, Option<PathBuf>)> = Vec::new();
        let file = match &d.path {
            Some(p) => {
                let base = root.as_ref().filter(|r| p.starts_with(r)).cloned().unwrap_or_default();
                let rel = p.strip_prefix(&base).unwrap_or(p);
                let mut acc = base.clone();
                let mut comps: Vec<(String, Option<PathBuf>)> = rel
                    .components()
                    .map(|c| {
                        acc.push(c.as_os_str());
                        (c.as_os_str().to_string_lossy().into_owned(), Some(acc.clone()))
                    })
                    .collect();
                let f = comps.pop().map(|c| c.0).unwrap_or_default();
                if comps.len() > 3 {
                    comps.drain(..comps.len() - 3);
                    comps.insert(0, ("…".into(), None));
                }
                dirs = comps;
                f
            }
            None => d.title(),
        };
        let chain: Vec<(String, symbols::Kind, usize)> =
            symbols::path_at(d.symbols(), line).iter().map(|s| (s.name.clone(), s.kind, s.line)).collect();
        let is_md = d.view == document::View::Text && d.syntax.contains("Markdown");
        let (mut goto, mut list, mut preview, mut open, mut reveal) = (None, false, false, None, None);
        let bg = self.hl.colors.bg;
        egui::TopBottomPanel::top(egui::Id::new("crumbs").with(pane))
            .exact_height(22.0)
            .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(10.0, 0.0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let weak = ui.visuals().weak_text_color();
                    let sep = |ui: &mut egui::Ui| {
                        ui.label(RichText::new("›").size(12.0).color(weak));
                    };
                    for (dname, dir) in &dirs {
                        match dir {
                            // a folder: its files and folders in a menu
                            Some(dir) => {
                                let r = ui.menu_button(RichText::new(dname).size(12.0).color(weak), |ui| {
                                    ui.set_max_height(420.0);
                                    if ui.button("Afficher dans l'arborescence").clicked() {
                                        reveal = Some(dir.clone());
                                        ui.close_menu();
                                    }
                                    ui.separator();
                                    let mut entries: Vec<(String, PathBuf, bool)> = std::fs::read_dir(dir)
                                        .map(|rd| {
                                            rd.flatten()
                                                .filter(|e| e.file_name() != ".git")
                                                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path(), e.path().is_dir()))
                                                .collect()
                                        })
                                        .unwrap_or_default();
                                    entries.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
                                    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                                        for (name, path, is_dir) in entries {
                                            let label = if is_dir { format!("📁 {name}") } else { name };
                                            if ui.button(label).clicked() {
                                                if is_dir {
                                                    reveal = Some(path);
                                                } else {
                                                    open = Some(path);
                                                }
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                });
                                r.response.on_hover_text(dir.display().to_string());
                            }
                            None => {
                                ui.label(RichText::new(dname).size(12.0).color(weak));
                            }
                        }
                        sep(ui);
                    }
                    let r = ui.add(egui::Label::new(RichText::new(&file).size(12.0)).sense(Sense::click()));
                    if r.on_hover_text("Symboles du fichier (Ctrl+R)").clicked() {
                        list = true;
                    }
                    for (name, kind, l) in &chain {
                        sep(ui);
                        let text = RichText::new(format!("{} {name}", kind.icon())).size(12.0).color(accent);
                        if ui.add(egui::Label::new(text).sense(Sense::click())).on_hover_text(format!("Aller à la ligne {}", l + 1)).clicked() {
                            goto = Some(*l);
                        }
                    }
                    if is_md {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Aperçu").on_hover_text("Aperçu Markdown à côté (Ctrl+K V)").clicked() {
                                preview = true;
                            }
                        });
                    }
                });
            });
        if let Some(l) = goto {
            if let Some(d) = self.docs.get_mut(i) {
                let ind = d.indent_len(l);
                d.goto_line(l, ind);
            }
            self.active = i;
            self.pending_focus = true;
        }
        if list {
            self.active = i;
            self.pending.push(Cmd::GotoSymbol);
        }
        if preview {
            self.active = i;
            self.markdown_preview(true);
        }
        if let Some(p) = open {
            self.open_path(p);
        }
        if let Some(p) = reveal {
            self.reveal_in_tree(&p);
            self.tree_nav.focus = true;
        }
    }

    /// Ends a tab drag: reorder in a tab strip, move to the other panel, or split.
    fn resolve_tab_drag(&mut self, ctx: &egui::Context) {
        let Some(from) = self.tab_drag else { return };
        let Some(p) = ctx.pointer_latest_pos() else { return };
        if from >= self.docs.len() {
            self.tab_drag = None;
            return;
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        let accent = self.hl.colors.accent;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tab_drop")));
        let src = self.docs[from].pane;
        let split = self.is_split();
        let alone = self.docs.iter().filter(|d| d.pane == src).count() == 1;
        let zones = std::mem::take(&mut self.drop_zones);
        let mut target = None;
        let hint = |r: egui::Rect, text: &str| {
            painter.rect_filled(r.shrink(4.0), 8.0, accent.gamma_multiply(0.18));
            painter.rect_stroke(r.shrink(4.0), 8.0, egui::Stroke::new(2.0_f32, accent));
            painter.text(r.center(), Align2::CENTER_CENTER, text, egui::FontId::proportional(16.0), accent);
        };
        if let Some((pane, _, tabs)) = zones.strips.iter().find(|(_, r, _)| r.contains(p)) {
            let k = tabs.iter().filter(|(r, _)| r.center().x < p.x).count();
            let before = tabs.get(k).map(|(_, g)| *g);
            let x = match tabs.get(k) {
                Some((r, _)) => r.left() - 1.0,
                None => tabs.last().map_or(p.x, |(r, _)| r.right() + 1.0),
            };
            let top = tabs.first().map_or(p.y - 10.0, |(r, _)| r.top() + 6.0);
            painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x - 1.5, top), egui::pos2(x + 1.5, top + TAB_STRIP_H - 8.0)), 1.5, accent);
            // insertion index in the whole list: before a tab, or after the last one of the panel
            let to = before.unwrap_or_else(|| tabs.last().map_or(self.docs.len(), |(_, g)| g + 1));
            target = Some(TabDrop::Strip(*pane, to));
        } else if let Some((pane, r)) = zones.panes.iter().find(|(_, r)| r.contains(p)) {
            if *pane != src {
                hint(*r, "Déplacer dans ce panneau");
                target = Some(TabDrop::Pane(*pane));
            } else if !split && !alone {
                if p.x > r.left() + r.width() * 0.6 {
                    hint(egui::Rect::from_min_max(egui::pos2(r.center().x, r.top()), r.max), "Diviser à droite");
                    target = Some(TabDrop::Split(SplitDir::Right));
                } else if p.y > r.top() + r.height() * 0.6 {
                    hint(egui::Rect::from_min_max(egui::pos2(r.left(), r.center().y), r.max), "Diviser en bas");
                    target = Some(TabDrop::Split(SplitDir::Down));
                }
            }
        }
        if !ctx.input(|i| i.pointer.primary_down()) {
            self.tab_drag = None;
            match target {
                Some(TabDrop::Strip(pane, to)) => {
                    self.docs[from].pane = pane;
                    self.move_tab(from, to);
                }
                Some(TabDrop::Pane(pane)) => {
                    self.docs[from].pane = pane;
                    self.activate_tab(from);
                }
                Some(TabDrop::Split(dir)) => self.split_tab(from, Some(dir)),
                None => {}
            }
        }
    }

    // ------------------------------------------------------------------ language servers (LSP)

    /// Server for document `i` (started if needed) and its language id; sends the latest text.
    fn lsp_for(&mut self, i: usize, ctx: &egui::Context, force_sync: bool) -> Option<(usize, &'static str)> {
        if !self.settings.lsp {
            return None;
        }
        let d = self.docs.get(i)?;
        if d.view != document::View::Text {
            return None;
        }
        let path = d.path.clone()?;
        let (family, lang) = lsp::language_of(&path)?;
        let root = self.repo_for(&path).or_else(|| self.root.clone()).or_else(|| path.parent().map(Path::to_path_buf))?;
        let ci = match self.lsp.ensure(family, &root, &self.settings.lsp_servers, ctx) {
            Ok(ci) => ci,
            Err(e) => {
                if !e.is_empty() {
                    self.lsp_note.insert(family.to_string(), e);
                }
                return None;
            }
        };
        let d = &self.docs[i];
        let sent = self.lsp.client(ci).and_then(|c| c.opened.get(&d.uid).map(|o| o.0));
        // while typing, the whole text is sent at most every 200 ms
        if force_sync || sent.is_none() || (sent != Some(d.version) && d.last_change.elapsed() > Duration::from_millis(200)) {
            let (uid, version) = (d.uid, d.version);
            let rope = &self.docs[i].rope;
            if let Some(c) = self.lsp.client(ci) {
                c.sync(uid, &path, lang, version, || rope.to_string());
            }
        }
        Some((ci, lang))
    }

    fn lsp_frame(&mut self, ctx: &egui::Context) {
        for (ci, msg) in self.lsp.poll() {
            match msg {
                lsp::Incoming::Response { id, result } => {
                    let Some(req) = self.lsp_req.remove(&(ci, id)) else { continue };
                    match result {
                        Ok(v) => self.lsp_response(ci, req, v, ctx),
                        Err(e) => self.set_status(format!("Serveur de langage : {e}")),
                    }
                }
                lsp::Incoming::Exited(name) => {
                    self.set_status(format!("Le serveur de langage {name} s'est arrêté (LSP : redémarrer pour réessayer)"));
                }
            }
        }
        if self.active < self.docs.len() {
            self.lsp_for(self.active, ctx, false);
        }
        // closed tabs: didClose
        let alive: HashSet<u64> = self.docs.iter().map(|d| d.uid).collect();
        for c in self.lsp.clients.iter_mut().flatten() {
            c.close_missing(&alive);
        }
        if self.complete_at.is_some_and(|t| Instant::now() >= t) {
            self.complete_at = None;
            if self.completion.is_none() {
                self.request_completion(ctx);
            }
        } else if let Some(t) = self.complete_at {
            ctx.request_repaint_after(t.saturating_duration_since(Instant::now()));
        }
        self.completion_filter();
    }

    fn lsp_text_position(&self, i: usize) -> Option<(serde_json::Value, serde_json::Value)> {
        let d = self.docs.get(i)?;
        let uri = lsp::path_to_uri(d.path.as_ref()?);
        Some((serde_json::json!({ "uri": uri }), lsp::to_position(&d.rope, d.primary_sel().head)))
    }

    /// Start of the identifier before the cursor.
    fn word_start(d: &Document) -> usize {
        let head = d.primary_sel().head;
        let mut p = head;
        while p > 0 && d.char_at(p - 1).is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$') {
            p -= 1;
        }
        p
    }

    fn request_completion(&mut self, ctx: &egui::Context) {
        let i = self.active;
        let Some((ci, _)) = self.lsp_for(i, ctx, true) else {
            self.local_completion(true);
            return;
        };
        let Some((doc, pos)) = self.lsp_text_position(i) else { return };
        let d = &self.docs[i];
        let (uid, start) = (d.uid, Self::word_start(d));
        if let Some(c) = self.lsp.client(ci) {
            let id = c.request("textDocument/completion", serde_json::json!({ "textDocument": doc, "position": pos }));
            self.lsp_req.insert((ci, id), LspReq::Completion { uid, start });
        }
    }

    /// Word to look up for "go to definition": the identifier at the cursor, or a quoted dotted
    /// name ('sale.order', 'module.xml_id') when the cursor is in one.
    fn definition_word(d: &Document) -> String {
        let head = d.primary_sel().head;
        let line = d.rope.char_to_line(head);
        let text = d.line_text(line);
        let col = head - d.rope.line_to_char(line);
        let chars: Vec<char> = text.chars().collect();
        let quote = |c: char| c == '\'' || c == '"';
        let (mut a, mut b) = (col.min(chars.len()), col.min(chars.len()));
        while a > 0 && !quote(chars[a - 1]) && !chars[a - 1].is_whitespace() {
            a -= 1;
        }
        while b < chars.len() && !quote(chars[b]) && !chars[b].is_whitespace() {
            b += 1;
        }
        let quoted: String = chars[a..b].iter().collect();
        if a > 0 && b < chars.len() && quote(chars[a - 1]) && quote(chars[b]) && quoted.contains('.')
            && quoted.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            return quoted;
        }
        let (ws, we) = d.word_at(head);
        d.slice(ws, we)
    }

    fn goto_definition(&mut self, ctx: &egui::Context) {
        let i = self.active;
        let Some(d) = self.docs.get(i) else { return };
        let word = Self::definition_word(d);
        if word.trim().is_empty() {
            return;
        }
        match self.lsp_for(i, ctx, true) {
            Some((ci, _)) => {
                let Some((doc, pos)) = self.lsp_text_position(i) else { return };
                if let Some(cl) = self.lsp.client(ci) {
                    let id = cl.request("textDocument/definition", serde_json::json!({ "textDocument": doc, "position": pos }));
                    self.lsp_req.insert((ci, id), LspReq::Definition { word });
                }
            }
            None => self.index_definition(&word),
        }
    }

    /// Native dialog of the system (Windows Explorer, macOS Finder) to choose a folder (0), a file
    /// to open (1) or where to save (2). Returns false where Florm uses its own path entry (Linux).
    #[cfg(any(windows, target_os = "macos"))]
    fn native_dialog(&mut self, kind: u8) -> bool {
        self.file_dialog(kind);
        true
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn native_dialog(&mut self, _kind: u8) -> bool {
        false
    }

    /// File chooser of the system (Explorer, Finder, the desktop's one through the xdg portal on
    /// Linux) for a folder (0), a file to open (1) or where to save (2).
    fn file_dialog(&mut self, kind: u8) {
        if self.dialog_open {
            return;
        }
        self.dialog_open = true;
        let current = self.docs.get(self.active).and_then(|d| d.path.clone());
        let dir = current
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| self.root.clone())
            .unwrap_or_else(config::home);
        let name = current.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
        self.jobs.spawn(move || {
            #[cfg(target_os = "linux")]
            let picked = crate::os::portal_pick(kind, &dir, name.as_deref().filter(|_| kind == 2));
            #[cfg(not(target_os = "linux"))]
            let picked = {
                let d = rfd::FileDialog::new().set_directory(&dir);
                Ok(match kind {
                    0 => d.set_title("Ouvrir un dossier (les projets WSL : \\\\wsl.localhost\\…)").pick_folder(),
                    1 => d.set_title("Ouvrir un fichier").pick_file(),
                    _ => {
                        let d = d.set_title("Enregistrer sous");
                        match name {
                            Some(n) => d.set_file_name(n).save_file(),
                            None => d.save_file(),
                        }
                    }
                })
            };
            JobResult::Picked(kind, picked)
        });
    }

    /// Shows `path` in the file tree: sidebar opened, folders expanded, entry scrolled into view.
    fn reveal_in_tree(&mut self, path: &Path) {
        let Some(tree) = self.tree.as_mut() else {
            self.set_status("Aucun dossier ouvert");
            return;
        };
        let path = crate::os::canon(path);
        if !tree.reveal(&path) {
            self.set_status("Ce fichier n'est pas dans le dossier ouvert");
            return;
        }
        self.show_sidebar = true;
        self.side_tab = SideTab::Files;
        self.tree_nav.cursor = Some(path.clone());
        self.tree_scroll = Some(path);
    }

    // ------------------------------------------------------------------ cursor history

    /// Records the previous place when the cursor jumps (another file, or 8+ lines away).
    fn track_nav(&mut self) {
        let Some(d) = self.docs.get(self.active).filter(|d| d.view == document::View::Text) else { return };
        let pos = d.primary_sel().head;
        let line = d.rope.char_to_line(pos);
        let cur = NavPos { uid: d.uid, path: d.path.clone(), pos };
        if let Some((last, last_line)) = self.nav_last.take() {
            let jumped = last.uid != cur.uid || line.abs_diff(last_line) >= 8;
            if jumped && !std::mem::take(&mut self.nav_jumping) {
                let same = self.nav_back.last().is_some_and(|b| b.uid == last.uid && b.pos == last.pos);
                if !same {
                    self.nav_back.push(last);
                    if self.nav_back.len() > 100 {
                        self.nav_back.remove(0);
                    }
                }
                self.nav_fwd.clear();
            }
        }
        self.nav_jumping = false;
        self.nav_last = Some((cur, line));
    }

    /// Alt+- (back) / Alt++ (forward).
    fn nav_go(&mut self, back: bool) {
        let here = self.docs.get(self.active).map(|d| NavPos { uid: d.uid, path: d.path.clone(), pos: d.primary_sel().head });
        loop {
            let target = if back { self.nav_back.pop() } else { self.nav_fwd.pop() };
            let Some(t) = target else {
                self.set_status(if back { "Pas de position précédente" } else { "Pas de position suivante" });
                return;
            };
            // the document may have been closed: reopen it from its path
            let idx = match self.docs.iter().position(|d| d.uid == t.uid) {
                Some(i) => Some(i),
                None => t.path.clone().filter(|p| p.is_file()).and_then(|p| self.open_path(p).then_some(self.active)),
            };
            let Some(i) = idx else { continue };
            if let Some(h) = here.clone() {
                if back { self.nav_fwd.push(h) } else { self.nav_back.push(h) }
            }
            self.activate_tab(i);
            let d = &mut self.docs[i];
            let p = t.pos.min(d.len());
            let (l, c) = d.line_col(p);
            d.goto_line(l, c);
            self.nav_jumping = true;
            return;
        }
    }

    /// Like Sublime Text: definitions of `word` in the project index; one opens, several are listed.
    fn index_definition(&mut self, word: &str) {
        let mut found: Vec<(PathBuf, crate::index::Entry)> = match &self.sym_index {
            Some(ix) => ix.lookup(word),
            None => Vec::new(),
        };
        if found.is_empty() {
            // not indexed yet (or unsaved): the open documents
            for d in self.docs.iter_mut() {
                let Some(p) = d.path.clone() else { continue };
                for s in d.symbols().iter().filter(|s| s.name == word) {
                    found.push((p.clone(), crate::index::Entry { name: s.name.clone(), kind: s.kind, line: s.line, detail: s.detail.clone() }));
                }
            }
        }
        match found.len() {
            0 => {
                let more = if self.sym_index.is_none() { " (indexation du projet en cours…)" } else { "" };
                self.set_status(format!("Aucune définition de « {word} » trouvée dans le projet{more}"));
            }
            1 => {
                let (p, e) = found.remove(0);
                self.open_at(p, e.line);
            }
            _ => {
                let root = self.root.clone();
                let items = found
                    .into_iter()
                    .map(|(p, e)| {
                        let rel = root.as_ref().and_then(|r| p.strip_prefix(r).ok()).unwrap_or(&p).display().to_string();
                        let detail = if e.detail.is_empty() { format!("{} {}", e.kind.icon(), e.name) } else { format!("{} {} · {}", e.kind.icon(), e.name, e.detail) };
                        Item { label: format!("{rel}:{}", e.line + 1), detail, action: Action::Location(p, e.line) }
                    })
                    .collect();
                self.open_overlay_with(Mode::Definitions(word.to_string()), items, String::new());
            }
        }
    }

    fn open_at(&mut self, p: PathBuf, line: usize) {
        if self.open_path(p) {
            if let Some(d) = self.docs.get_mut(self.active) {
                let ind = d.indent_len(line.min(d.rope.len_lines().saturating_sub(1)));
                d.goto_line(line, ind);
            }
            self.history.open = false;
            self.pending_focus = true;
        }
    }

    /// Suggestions without a language server, like Sublime Text: words of the open files and
    /// the project's symbols starting like the word being typed.
    fn local_completion(&mut self, manual: bool) {
        let Some(d) = self.docs.get(self.active) else { return };
        if d.read_only || d.view != document::View::Text {
            return;
        }
        let (uid, start, head) = (d.uid, Self::word_start(d), d.primary_sel().head);
        let prefix = d.slice(start, head);
        if prefix.chars().count() < if manual { 1 } else { 2 } {
            return;
        }
        let lower = prefix.to_lowercase();
        let mut seen: HashSet<String> = HashSet::new();
        let mut items = Vec::new();
        let mut push = |w: &str, detail: &str, kind: u64| {
            if w != prefix && w.to_lowercase().starts_with(&lower) && seen.insert(w.to_string()) {
                items.push(lsp::CompletionItem {
                    label: w.to_string(),
                    detail: detail.to_string(),
                    kind,
                    filter: w.to_string(),
                    sort: w.to_string(),
                    insert: w.to_string(),
                    range: None,
                });
            }
        };
        static WORD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = WORD.get_or_init(|| regex::Regex::new(r"[A-Za-z_][A-Za-z0-9_]{2,}").unwrap());
        // the current file first, then the other open files
        let order: Vec<usize> = std::iter::once(self.active).chain((0..self.docs.len()).filter(|&k| k != self.active)).collect();
        for k in order {
            let doc = &self.docs[k];
            if doc.view != document::View::Text || doc.rope.len_bytes() > 2_000_000 {
                continue;
            }
            let text = doc.rope.to_string();
            let label = if k == self.active { "" } else { "autre fichier" };
            for m in re.find_iter(&text) {
                push(m.as_str(), label, 1);
            }
        }
        if let Some(ix) = &self.sym_index {
            for n in ix.names_with_prefix(&prefix, 200) {
                push(n, "symbole du projet", 6);
            }
            if prefix.chars().next().is_some_and(|c| c.is_lowercase()) {
                let cap: String = prefix.chars().take(1).flat_map(char::to_uppercase).chain(prefix.chars().skip(1)).collect();
                for n in ix.names_with_prefix(&cap, 100) {
                    push(n, "symbole du projet", 7);
                }
            }
        }
        if items.is_empty() {
            return;
        }
        self.completion = Some(CompletionUi { uid, start, items, shown: Vec::new(), selected: 0, incomplete: false, client: usize::MAX });
        self.completion_filter();
    }

    fn lsp_response(&mut self, ci: usize, req: LspReq, v: serde_json::Value, ctx: &egui::Context) {
        match req {
            LspReq::Completion { uid, start } => {
                let Some(d) = self.docs.get(self.active).filter(|d| d.uid == uid) else { return };
                if d.primary_sel().head < start {
                    return;
                }
                let (items, incomplete) = lsp::parse_completion(&v);
                if items.is_empty() {
                    self.local_completion(false);
                    return;
                }
                self.completion = Some(CompletionUi { uid, start, items, shown: Vec::new(), selected: 0, incomplete, client: ci });
                self.completion_filter();
            }
            LspReq::Definition { word } => match lsp::parse_location(&v) {
                Some((path, pos)) => {
                    if self.open_path(path) {
                        if let Some(d) = self.docs.get_mut(self.active) {
                            let p = lsp::from_position(&d.rope, &pos);
                            let (l, c) = d.line_col(p);
                            d.goto_line(l, c);
                        }
                        self.history.open = false;
                        self.pending_focus = true;
                    }
                }
                None => self.index_definition(&word),
            },
            LspReq::Rename { name } => {
                let edits = lsp::parse_workspace_edit(&v);
                if edits.is_empty() {
                    self.set_status("Rien à renommer ici");
                    return;
                }
                let back = self.docs.get(self.active).map(|d| d.uid);
                let (mut n, files) = (0, edits.len());
                for (path, list) in edits {
                    let i = match self.docs.iter().position(|d| d.path.as_ref() == Some(&path)) {
                        Some(i) => i,
                        None if self.open_path(path.clone()) => self.active,
                        None => continue,
                    };
                    let d = &mut self.docs[i];
                    let changes: Vec<(usize, usize, String)> = list
                        .iter()
                        .map(|(range, t)| (lsp::from_position(&d.rope, &range["start"]), lsp::from_position(&d.rope, &range["end"]), t.clone()))
                        .collect();
                    n += changes.len();
                    d.replace_keep(EditKind::Other, changes);
                }
                if let Some(i) = back.and_then(|u| self.docs.iter().position(|d| d.uid == u)) {
                    self.active = i;
                }
                self.set_status(format!("Renommé en « {name} » : {n} endroit(s) dans {files} fichier(s) — à enregistrer (Ctrl+Alt+S : tout enregistrer)"));
                let _ = ctx;
            }
        }
    }

    /// Keeps the suggestions matching the word being typed; closes the list when it no longer applies.
    fn completion_filter(&mut self) {
        let Some(c) = self.completion.as_mut() else { return };
        let Some(d) = self.docs.get(self.active).filter(|d| d.uid == c.uid && d.sels.len() == 1) else {
            self.completion = None;
            return;
        };
        let head = d.primary_sel().head;
        if head < c.start || d.rope.char_to_line(head) != d.rope.char_to_line(c.start) {
            self.completion = None;
            return;
        }
        let prefix = d.slice(c.start, head);
        if prefix.chars().any(|ch| !(ch.is_alphanumeric() || ch == '_' || ch == '$')) {
            self.completion = None;
            return;
        }
        let lower = prefix.to_lowercase();
        let mut scored: Vec<(i32, usize)> = c
            .items
            .iter()
            .enumerate()
            .filter_map(|(k, it)| {
                if prefix.is_empty() {
                    return Some((0, k));
                }
                let f = it.filter.to_lowercase();
                let bonus = if f.starts_with(&lower) { 1000 } else { 0 };
                fuzzy::score(&prefix, &it.filter).map(|s| (s + bonus, k))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| c.items[a.1].sort.cmp(&c.items[b.1].sort)));
        c.shown = scored.into_iter().map(|(_, k)| k).take(200).collect();
        if c.shown.is_empty() {
            self.completion = None;
            return;
        }
        c.selected = c.selected.min(c.shown.len() - 1);
    }

    fn accept_completion(&mut self, k: usize) {
        let Some(c) = self.completion.take() else { return };
        let Some(&item) = c.shown.get(k).as_ref() else { return };
        let it = &c.items[*item];
        let Some(d) = self.docs.get_mut(self.active).filter(|d| d.uid == c.uid) else { return };
        let head = d.primary_sel().head;
        let a = match &it.range {
            Some(r) => lsp::from_position(&d.rope, &r["start"]).min(c.start),
            None => c.start,
        };
        let n = it.insert.chars().count();
        d.replace_range(a, head, &it.insert);
        d.sels = vec![Sel::cursor(a + n)];
        d.primary = 0;
        let _ = c.client;
    }

    /// Up / Down / Enter / Tab / Escape go to the suggestion list while it is open.
    fn completion_keys(&mut self, ctx: &egui::Context) {
        if self.completion.is_none() || !ctx.memory(|m| m.has_focus(editor_id())) {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let mut used = vec![false; events.len()];
        let mut accept = None;
        for (k, ev) in events.iter().enumerate() {
            let Event::Key { key, pressed: true, modifiers, .. } = ev else { continue };
            if !modifiers.is_none() && !(modifiers.shift_only() && *key == Key::Tab) {
                continue;
            }
            let Some(c) = self.completion.as_mut() else { break };
            let n = c.shown.len();
            match key {
                Key::ArrowDown => c.selected = (c.selected + 1) % n,
                Key::ArrowUp => c.selected = (c.selected + n - 1) % n,
                Key::PageDown => c.selected = (c.selected + 8).min(n - 1),
                Key::PageUp => c.selected = c.selected.saturating_sub(8),
                Key::Enter | Key::Tab => accept = Some(c.selected),
                Key::Escape => self.completion = None,
                _ => continue,
            }
            used[k] = true;
        }
        if used.iter().any(|u| *u) {
            ctx.input_mut(|inp| {
                let mut k = 0;
                inp.events.retain(|_| {
                    let keep = !used.get(k).copied().unwrap_or(false);
                    k += 1;
                    keep
                });
            });
        }
        if let Some(k) = accept {
            self.accept_completion(k);
        }
    }

    fn completion_popup(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.completion else { return };
        let Some(d) = self.docs.get(self.active).filter(|d| d.uid == c.uid) else { return };
        let Some((x, y)) = d.caret_screen else { return };
        let mut clicked = None;
        let font = egui::TextStyle::Monospace.resolve(&ctx.style());
        egui::Area::new(egui::Id::new("completion"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(x - 22.0, y + 2.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(4.0)).show(ui, |ui| {
                    ui.set_width(440.0);
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (row, &k) in c.shown.iter().enumerate() {
                            let it = &c.items[k];
                            let sel = row == c.selected;
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), Sense::click());
                            if sel {
                                ui.painter().rect_filled(rect, 4.0, ui.visuals().selection.bg_fill);
                                resp.scroll_to_me(None);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
                            }
                            let p = ui.painter().with_clip_rect(rect);
                            let cy = rect.center().y;
                            p.text(egui::pos2(rect.left() + 10.0, cy), Align2::CENTER_CENTER, it.icon(), font.clone(), self.hl.colors.accent);
                            let r = p.text(egui::pos2(rect.left() + 22.0, cy), Align2::LEFT_CENTER, &it.label, font.clone(), ui.visuals().strong_text_color());
                            if !it.detail.is_empty() {
                                // the detail on the right, without covering the label
                                let p = p.with_clip_rect(egui::Rect::from_min_max(egui::pos2(r.right() + 12.0, rect.top()), rect.max));
                                p.text(
                                    egui::pos2(rect.right() - 6.0, cy),
                                    Align2::RIGHT_CENTER,
                                    it.detail.lines().next().unwrap_or(""),
                                    egui::FontId::proportional(11.5),
                                    ui.visuals().weak_text_color(),
                                );
                            }
                            if resp.clicked() {
                                clicked = Some(row);
                            }
                        }
                    });
                    ui.label(RichText::new("↑ ↓ choisir · Entrée / Tab insérer · Échap fermer").small().weak());
                });
            });
        if let Some(k) = clicked {
            self.accept_completion(k);
            self.pending_focus = true;
        }
    }

    // ------------------------------------------------------------------ projects

    fn open_project(&mut self, path: PathBuf) {
        self.set_root(path);
        self.restore_session();
    }

    fn switch_project(&mut self, path: PathBuf) {
        let path = crate::os::canon(&path);
        if self.root.as_ref() == Some(&path) {
            return;
        }
        if !path.is_dir() {
            self.set_status(format!("Dossier introuvable : {}", path.display()));
            return;
        }
        // with hot exit, the unsaved changes stay in the session of the project we leave
        if !self.settings.hot_exit && self.docs.iter().any(|d| d.is_dirty()) {
            self.confirm = Some(Confirm::SwitchProject(path));
            return;
        }
        self.do_switch(path);
    }

    /// Empty search state that keeps the last used filters.
    fn fresh_search(&self) -> SearchUi {
        let mut s = SearchUi::default();
        s.q.include = self.state.last_include.clone();
        s.q.exclude = self.state.last_exclude.clone();
        s
    }

    /// Writes one value into settings.toml so UI choices survive a restart.
    fn persist(&mut self, key: &str, value: String) {
        if let Err(e) = config::set_setting(key, &value) {
            self.log(format!("Préférences non enregistrées : {e}"));
        }
    }

    fn do_switch(&mut self, path: PathBuf) {
        self.save_session();
        self.docs.clear();
        self.active = 0;
        self.search.gen.store(u64::MAX, Ordering::SeqCst); // cancel a running search
        self.search = self.fresh_search();
        self.find = FindState::default();
        self.open_project(path);
    }

    fn close_folder(&mut self) {
        if self.docs.iter().any(|d| d.is_dirty()) {
            self.set_status("Enregistre ou ferme d'abord les fichiers modifiés");
            return;
        }
        self.save_session();
        self.docs.clear();
        self.root = None;
        self.repo = None;
        self.repos.clear();
        self.history = HistoryUi::default();
        self.tree = None;
        self.file_index.clear();
        self.git = GitUi::default();
        self.search.gen.store(u64::MAX, Ordering::SeqCst);
        self.search = self.fresh_search();
        self.reload_plugins();
    }

    /// Session of the project; with hot exit, unsaved contents go in `backups` (file name, text).
    fn current_session(&self) -> (Session, Vec<(String, String)>) {
        let mut files = Vec::new();
        let mut backups = Vec::new();
        let mut active = 0;
        for (i, d) in self.docs.iter().enumerate() {
            if d.read_only {
                continue;
            }
            let dirty = self.settings.hot_exit && d.is_dirty();
            let path = match &d.path {
                Some(p) => p.clone(),
                None if dirty && !d.is_pristine_empty() => PathBuf::new(),
                None => continue,
            };
            if i == self.active {
                active = files.len();
            }
            let (line, col) = d.line_col(d.primary_sel().head);
            let backup = dirty.then(|| format!("doc{}.txt", files.len()));
            if let Some(name) = &backup {
                backups.push((name.clone(), d.rope.to_string()));
            }
            files.push(SessionFile { path, line, col, backup });
        }
        let mut expanded = Vec::new();
        if let Some(t) = &self.tree {
            t.expanded_paths(&mut expanded);
        }
        let session = Session {
            active,
            files,
            expanded,
            search_include: self.search.q.include.clone(),
            search_exclude: self.search.q.exclude.clone(),
        };
        (session, backups)
    }

    fn save_session(&mut self) {
        if let Some(r) = &self.root {
            let (session, backups) = self.current_session();
            project::save_session(r, &session);
            project::save_backups(r, &backups);
        }
        self.last_session_save = Instant::now();
    }

    fn restore_session(&mut self) {
        let Some(root) = self.root.clone() else { return };
        let Some(s) = project::load_session(&root) else { return };
        if !s.search_include.is_empty() {
            self.search.q.include = s.search_include;
        }
        if !s.search_exclude.is_empty() {
            self.search.q.exclude = s.search_exclude;
        }
        if let Some(t) = self.tree.as_mut() {
            let set: HashSet<PathBuf> = s.expanded.into_iter().collect();
            t.expand_paths(&set);
        }
        let mut opened = Vec::new();
        let mut restored = 0;
        for f in &s.files {
            let backup = f.backup.as_deref().and_then(|b| project::load_backup(&root, b));
            let ok = if f.path.as_os_str().is_empty() || (!f.path.is_file() && backup.is_some()) {
                // untitled document, or a file deleted since: recreate it from its backup
                match &backup {
                    Some(_) => {
                        self.new_doc();
                        if !f.path.as_os_str().is_empty() {
                            let d = &mut self.docs[self.active];
                            d.path = Some(f.path.clone());
                            d.syntax = self.hl.detect(Some(&f.path), "");
                        }
                        true
                    }
                    None => false,
                }
            } else {
                f.path.is_file() && self.open_path(f.path.clone())
            };
            if ok {
                let i = self.active;
                if let Some(text) = backup.as_ref().filter(|t| **t != self.docs[i].rope.to_string()) {
                    self.docs[i].set_text(text);
                    restored += 1;
                }
                self.docs[i].goto_line(f.line, f.col);
                opened.push(Some(i));
            } else {
                opened.push(None);
            }
        }
        if let Some(Some(i)) = opened.get(s.active) {
            self.active = *i;
        }
        if restored > 0 {
            self.set_status(format!("{restored} fichier(s) avec des modifications non enregistrées restauré(s)"));
        }
    }

    fn projects_ui(&mut self, ui: &mut egui::Ui) {
        let (mut switch, mut remove, mut rename) = (None, None, None);
        ui.horizontal(|ui| {
            if ui.button("Ouvrir un dossier…").clicked() {
                self.pending.push(Cmd::OpenFolder);
            }
            if self.root.is_some() && ui.button("Fermer").on_hover_text("Fermer le dossier courant").clicked() {
                self.pending.push(Cmd::CloseFolder);
            }
        });
        ui.separator();
        if self.projects.is_empty() {
            ui.label(RichText::new("Aucun projet : ouvre un dossier.").weak());
        }
        egui::ScrollArea::vertical().id_salt("projects").auto_shrink([false, false]).show(ui, |ui| {
            for p in &self.projects {
                let current = self.root.as_ref() == Some(&p.path);
                let exists = p.path.is_dir();
                let mut name = RichText::new(&p.name).strong();
                if !exists {
                    name = name.strikethrough().weak();
                }
                let r = ui.selectable_label(current, name).on_hover_text("Clic : ouvrir · Clic droit : options");
                ui.label(RichText::new(p.path.display().to_string()).weak().small());
                if r.clicked() && !current && exists {
                    switch = Some(p.path.clone());
                }
                r.context_menu(|ui| {
                    if ui.button("Renommer…").clicked() {
                        rename = Some(p.clone());
                        ui.close_menu();
                    }
                    if ui.button("Retirer de la liste").clicked() {
                        remove = Some(p.path.clone());
                        ui.close_menu();
                    }
                    if ui.button("Copier le chemin").clicked() {
                        ui.ctx().output_mut(|o| o.copied_text = p.path.display().to_string());
                        ui.close_menu();
                    }
                });
                ui.add_space(4.0);
            }
        });
        if let Some(p) = switch {
            self.switch_project(p);
        }
        if let Some(p) = remove {
            self.projects.retain(|x| x.path != p);
            project::save_list(&self.projects);
        }
        if let Some(p) = rename {
            self.open_overlay_with(Mode::Prompt(Prompt::RenameProject(p.path)), Vec::new(), p.name);
        }
    }

    fn relocate_docs(&mut self, old: &Path, new: &Path) {
        for d in self.docs.iter_mut() {
            if let Some(p) = d.path.clone() {
                if let Ok(rest) = p.strip_prefix(old) {
                    d.path = Some(if rest.as_os_str().is_empty() { new.to_path_buf() } else { new.join(rest) });
                }
            }
        }
    }

    fn move_entry(&mut self, src: PathBuf, dir: PathBuf) {
        let Some(name) = src.file_name() else { return };
        let dest = dir.join(name);
        if dest.exists() {
            self.set_status(format!("{} existe déjà", dest.display()));
            return;
        }
        match std::fs::rename(&src, &dest) {
            Ok(()) => {
                self.relocate_docs(&src, &dest);
                self.log(format!("Déplacé : {} → {}", src.display(), dest.display()));
                self.set_status(format!("Déplacé vers {}", dir.display()));
                self.exec_refresh_tree();
            }
            Err(e) => self.set_status(format!("Déplacement impossible : {e}")),
        }
    }

    // ------------------------------------------------------------------ find in files

    fn start_search(&mut self) {
        let Some(root) = self.root.clone() else {
            self.set_status("Ouvre d'abord un dossier");
            return;
        };
        if self.search.q.pattern.is_empty() {
            return;
        }
        let q = &self.search.q;
        UiState::remember(&mut self.state.search_history, &q.pattern);
        UiState::remember(&mut self.state.include_history, &q.include);
        UiState::remember(&mut self.state.exclude_history, &q.exclude);
        self.state.last_include = q.include.trim().to_string();
        self.state.last_exclude = q.exclude.trim().to_string();
        project::save_state(&self.state);
        self.search.hist_pos = None;
        let gen = self.search.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let (q, current) = (self.search.q.clone(), self.search.gen.clone());
        self.search.done = q.clone();
        self.search.running = true;
        self.jobs.spawn(move || JobResult::Search(gen, search::run(&root, &q, gen, &current)));
    }

    fn open_hit(&mut self, fi: usize, hi: usize) {
        let Some(f) = self.search.files.get(fi) else { return };
        let Some(h) = f.hits.get(hi) else { return };
        let (path, line, col, len) = (f.path.clone(), h.line, h.col, h.len);
        self.search.cursor = Some((fi, hi));
        if self.open_path(path) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.goto_line(line, col);
                let p = d.primary_sel().head;
                let end = (p + len).min(d.len());
                d.sels = vec![Sel::range(p, end)];
                d.primary = 0;
                d.flash = Some((p, end, Instant::now()));
            }
        }
        self.search.scroll_to_cursor = true;
    }

    fn step_result(&mut self, forward: bool) {
        let s = &self.search;
        let hits: Vec<(usize, usize)> =
            s.files.iter().enumerate().flat_map(|(fi, f)| (0..f.hits.len()).map(move |h| (fi, h))).collect();
        let n = hits.len();
        if n == 0 {
            return;
        }
        let idx = match s.cursor.and_then(|c| hits.iter().position(|&x| x == c)) {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            None if forward => 0,
            None => n - 1,
        };
        let (f, h) = hits[idx];
        if self.search.collapsed.remove(&f) {
            self.search.rebuild_rows();
        }
        self.open_hit(f, h);
    }

    /// Replaces every listed hit, those of one file (`Some((file, None))`) or a single one.
    fn replace_in_files(&mut self, only: Option<(usize, Option<usize>)>) {
        let q = &self.search.done;
        let Ok(re) = document::build_regex(&q.pattern, q.case, q.regex, q.whole) else { return };
        let (rep, regex_mode, preserve) = (self.search.replacement.clone(), q.regex, self.search.q.preserve);
        let keep_all = |f: &FileHits| f.partial.then(|| f.hits.iter().map(|h| (h.line, h.col)).collect::<HashSet<_>>());
        let targets: Vec<(PathBuf, Option<HashSet<(usize, usize)>>)> = match only {
            None => self.search.files.iter().map(|f| (f.path.clone(), keep_all(f))).collect(),
            Some((fi, hit)) => {
                let Some(f) = self.search.files.get(fi) else { return };
                let keep = match hit.and_then(|h| f.hits.get(h)) {
                    Some(h) => Some([(h.line, h.col)].into()),
                    None => keep_all(f),
                };
                vec![(f.path.clone(), keep)]
            }
        };
        let (mut files, mut count, mut in_editor) = (0, 0, 0);
        let mut errors = Vec::new();
        for (p, keep) in targets {
            // Open files are changed in the editor (undoable, saved by the user).
            if let Some(d) = self.docs.iter_mut().find(|d| d.path.as_ref() == Some(&p)) {
                if let Some((new, n)) = search::replace_text(&re, &d.rope.to_string(), &rep, regex_mode, preserve, keep.as_ref()) {
                    d.set_text(&new);
                    (files, count, in_editor) = (files + 1, count + n, in_editor + 1);
                }
                continue;
            }
            match std::fs::read_to_string(&p) {
                Ok(text) => {
                    if let Some((new, n)) = search::replace_text(&re, &text, &rep, regex_mode, preserve, keep.as_ref()) {
                        match std::fs::write(&p, new) {
                            Ok(()) => (files, count) = (files + 1, count + n),
                            Err(e) => errors.push(format!("{} : {e}", p.display())),
                        }
                    }
                }
                Err(e) => errors.push(format!("{} : {e}", p.display())),
            }
        }
        for e in errors {
            self.log(e);
        }
        let open = if in_editor > 0 { format!(" — {in_editor} fichier(s) ouvert(s) à enregistrer") } else { String::new() };
        self.set_status(format!("{count} remplacement(s) dans {files} fichier(s){open}"));
        self.start_search();
        self.git_refresh();
    }

    /// Shell chosen in the preferences (None: $SHELL, or cmd on Windows).
    fn term_shell(&self) -> Option<String> {
        self.settings.shell.clone().filter(|s| !s.trim().is_empty())
    }

    fn reveal(&mut self, path: &Path) {
        if let Err(e) = crate::os::reveal(path) {
            self.set_status(e);
        }
    }

    fn search_ui(&mut self, ui: &mut egui::Ui, in_tab: bool) {
        let (mut go, mut act) = (false, None);
        let accent = self.hl.colors.accent;
        let st = &self.state;
        let hl = &self.hl;
        let s = &mut self.search;
        let enter = |r: &egui::Response, ui: &egui::Ui| r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let field_w = (ui.available_width() * 0.45).clamp(220.0, 520.0);

        // --- search line: [▸ replace] [pattern] [Aa .* « »] [⏷] [Rechercher]
        ui.horizontal(|ui| {
            let arrow = if s.show_replace { "⏷" } else { "⏵" };
            if ui
                .add(egui::Button::new(RichText::new(arrow).color(accent)).frame(false).min_size(egui::vec2(18.0, 0.0)))
                .on_hover_text("Afficher / masquer le remplacement")
                .clicked()
            {
                s.show_replace = !s.show_replace;
            }
            let te = ui.add(
                egui::TextEdit::singleline(&mut s.q.pattern)
                    .desired_width(field_w)
                    .hint_text("🔍  Rechercher dans le dossier… (Entrée)")
                    .font(egui::TextStyle::Monospace),
            );
            if s.focus {
                te.request_focus();
                s.focus = false;
            }
            if enter(&te, ui) {
                go = true;
                te.request_focus();
            }
            if te.has_focus() {
                if let Some(t) = history_arrows(ui, &st.search_history, &mut s.hist_pos) {
                    s.q.pattern = t;
                }
            }
            go |= history_menu(ui, &st.search_history, &mut s.q.pattern, "Recherches récentes");
            ui.toggle_value(&mut s.q.case, "Aa").on_hover_text("Respecter la casse");
            ui.toggle_value(&mut s.q.regex, ".*").on_hover_text("Expression régulière");
            ui.toggle_value(&mut s.q.whole, "« »").on_hover_text("Mot entier");
            let search_btn = filled_button("Rechercher", accent);
            if ui.add(search_btn).clicked() {
                go = true;
            }
            if s.running {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if in_tab {
                    if ui.small_button("⇲ Panneau du bas").on_hover_text("Afficher la recherche dans le panneau du bas").clicked() {
                        act = Some(SearchAct::Move(false));
                    }
                } else if ui.small_button("⇱ Onglet").on_hover_text("Afficher la recherche dans un onglet de l'éditeur (plus de place pour les résultats)").clicked() {
                    act = Some(SearchAct::Move(true));
                }
            });
        });

        // --- replace line
        if s.show_replace {
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                ui.add(
                    egui::TextEdit::singleline(&mut s.replacement)
                        .desired_width(field_w)
                        .hint_text(if s.q.regex { "Remplacer par… ($1, $2 = groupes)" } else { "Remplacer par… (vide = supprimer)" })
                        .font(egui::TextStyle::Monospace),
                );
                let n = s.hit_count();
                if ui.add_enabled(n > 0 && !s.running, filled_button(format!("Tout remplacer ({n})"), warn())).clicked() {
                    act = Some(SearchAct::ReplaceAll);
                }
                ui.toggle_value(&mut s.q.preserve, "Garder la casse").on_hover_text("chat → chien, Chat → Chien, CHAT → CHIEN");
            });
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                ui.spacing_mut().item_spacing.x = 4.0;
                let small = |t: &str| RichText::new(t).small();
                ui.label(small("Aperçu :").weak());
                ui.label(small(" ancien ").strikethrough().background_color(icons::mix(ui.visuals().panel_fill, err(), 0.4)));
                ui.label(small("→").weak());
                ui.label(small(" nouveau ").background_color(icons::mix(ui.visuals().panel_fill, ok(), 0.4)));
                ui.label(
                    small("· survol ou clic droit sur un résultat pour le remplacer seul ou l'écarter (× = ne pas remplacer)")
                        .weak(),
                );
            });
        }

        // --- filters line
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            ui.label(RichText::new("Fichiers").color(ok()));
            let inc = ui
                .add(egui::TextEdit::singleline(&mut s.q.include).desired_width(220.0).hint_text("*.py, models/*.py, tests"))
                .on_hover_text(
                    "Séparés par des virgules :\n\
                     *.py — tous les fichiers Python\n\
                     models/*.py — les fichiers Python des dossiers « models » (où qu'ils soient) et de leurs sous-dossiers\n\
                     tests — tout dossier (ou fichier) nommé « tests »\n\
                     ./src/*.rs ou /chemin/absolu/*.py — à partir de la racine du projet\n\
                     Aide → Guide de Florm pour plus d'exemples",
                );
            go |= history_menu(ui, &st.include_history, &mut s.q.include, "Filtres récents");
            ui.add_space(8.0);
            ui.label(RichText::new("Exclure").color(err()));
            let exc = ui.add(
                egui::TextEdit::singleline(&mut s.q.exclude).desired_width(170.0).hint_text("node_modules, *.min.js"),
            );
            go |= history_menu(ui, &st.exclude_history, &mut s.q.exclude, "Exclusions récentes");
            go |= enter(&inc, ui) || enter(&exc, ui);
        });

        // --- summary
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            ui.spacing_mut().item_spacing.x = 0.0;
            if let Some(e) = &s.error {
                ui.label(RichText::new(format!("⚠ {e}")).color(err()));
            } else if let Some((scanned, millis, truncated)) = s.stats {
                let n = s.hit_count();
                if scanned == 0 && !s.done.include.trim().is_empty() {
                    ui.label(
                        RichText::new(format!("Aucun fichier ne correspond au filtre « {} »", s.done.include.trim()))
                            .color(warn()),
                    );
                } else if n == 0 {
                    ui.label(RichText::new(format!("Aucun résultat pour « {} »", s.done.pattern)).color(warn()));
                } else {
                    ui.label(RichText::new(n.to_string()).color(accent).strong());
                    ui.label(" résultat(s) dans ");
                    ui.label(RichText::new(s.files.len().to_string()).color(accent).strong());
                    ui.label(" fichier(s)");
                }
                ui.label(RichText::new(format!("   ·   {scanned} fichiers parcourus en {millis} ms")).weak());
                if truncated {
                    ui.label(RichText::new("   ·   limite atteinte").color(warn()));
                }
                if n > 0 {
                    ui.label(RichText::new("   ·   F4 / Maj+F4 pour naviguer   ").weak());
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if ui.small_button("Tout replier").clicked() {
                        s.collapsed = (0..s.files.len()).collect();
                        s.rebuild_rows();
                    }
                    if ui.small_button("Tout déplier").clicked() {
                        s.collapsed.clear();
                        s.rebuild_rows();
                    }
                }
            } else {
                ui.label(
                    RichText::new("Tape un texte puis Entrée. Filtre les fichiers avec « *.py », exclus des dossiers avec « tests ».")
                        .weak(),
                );
            }
        });
        ui.add_space(2.0);

        // --- results
        let v = ui.visuals().clone();
        let bg = v.panel_fill;
        let light = bg.r() as u32 + bg.g() as u32 + bg.b() as u32 > 384;
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let fg = v.text_color();
        let preview_re = if s.show_replace {
            let q = &s.done;
            document::build_regex(&q.pattern, q.case, q.regex, q.whole).ok()
        } else {
            None
        };
        let match_fmt = TextFormat {
            font_id: font.clone(),
            color: if light { Color32::BLACK } else { v.strong_text_color() },
            background: icons::mix(bg, warn(), if light { 0.65 } else { 0.45 }),
            ..Default::default()
        };
        let old_fmt = TextFormat {
            font_id: font.clone(),
            color: fg,
            background: icons::mix(bg, err(), 0.4),
            strikethrough: egui::Stroke::new(1.0_f32, if light { Color32::BLACK } else { fg }),
            ..Default::default()
        };
        let new_fmt = TextFormat {
            font_id: font.clone(),
            color: if light { Color32::BLACK } else { v.strong_text_color() },
            background: icons::mix(bg, ok(), 0.4),
            ..Default::default()
        };
        let row_h = 22.0;
        ui.spacing_mut().item_spacing.y = 0.0; // show_rows positions rows with this spacing
        let mut area = egui::ScrollArea::both().id_salt("search_results").auto_shrink([false, false]);
        if std::mem::take(&mut s.scroll_to_cursor) {
            if let Some(i) = s.cursor.and_then(|(f, h)| s.rows.iter().position(|&r| r == (f, Some(h)))) {
                let (y, view_h) = (i as f32 * row_h, ui.available_height());
                let id = ui.make_persistent_id("search_results");
                let top = egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |st| st.offset.y);
                if y < top || y + row_h > top + view_h {
                    area = area.vertical_scroll_offset((y - view_h / 2.0).max(0.0));
                }
            }
        }
        area.show_rows(
            ui,
            row_h,
            s.rows.len(),
            |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let visible_right = ui.clip_rect().right();
                for r in range {
                    let (fi, hi) = s.rows[r];
                    let f = &s.files[fi];
                    let width = ui.available_width();
                    match hi {
                        None => {
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::click());
                            let hovered = ui.rect_contains_pointer(rect);
                            let fill = if hovered { v.widgets.hovered.weak_bg_fill } else { v.faint_bg_color };
                            ui.painter().rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), 4.0, fill);
                            let p = ui.painter().with_clip_rect(rect);
                            let cy = rect.center().y;
                            let collapsed = s.collapsed.contains(&fi);
                            icons::paint_chevron(&p, egui::pos2(rect.left() + 9.0, cy), !collapsed, v.weak_text_color());
                            let name = f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 28.0, cy), egui::vec2(16.0, 16.0));
                            icons::paint_file(&p, icon, &name, fill, light);
                            let mut x = rect.left() + 42.0;
                            let name_r = p.text(
                                egui::pos2(x, cy),
                                Align2::LEFT_CENTER,
                                &name,
                                egui::FontId::proportional(13.5),
                                v.strong_text_color(),
                            );
                            x = name_r.right() + 8.0;
                            let dir = f.rel.strip_suffix(&name).unwrap_or("").trim_end_matches(['/', '\\']);
                            if !dir.is_empty() {
                                let r = p.text(
                                    egui::pos2(x, cy),
                                    Align2::LEFT_CENTER,
                                    dir,
                                    egui::FontId::proportional(12.0),
                                    v.weak_text_color(),
                                );
                                x = r.right() + 8.0;
                            }
                            // hit count pill
                            let count = f.hits.len().to_string();
                            let galley = p.layout_no_wrap(count, egui::FontId::proportional(11.5), accent);
                            let pill = egui::Rect::from_min_size(
                                egui::pos2(x, cy - 8.0),
                                egui::vec2(galley.size().x + 12.0, 16.0),
                            );
                            p.rect_filled(pill, 8.0, icons::mix(fill, accent, 0.22));
                            p.galley(pill.center() - galley.size() / 2.0, galley, accent);
                            let mut clicked_button = false;
                            if hovered {
                                let mut right = rect.right().min(visible_right) - 4.0;
                                let mut btn = |ui: &mut egui::Ui, text: &str, tip: &str| {
                                    let w = 10.0 + 7.5 * text.chars().count() as f32;
                                    let r = egui::Rect::from_min_max(egui::pos2(right - w, cy - 9.0), egui::pos2(right, cy + 9.0));
                                    right -= w + 4.0;
                                    ui.put(r, egui::Button::new(RichText::new(text).small())).on_hover_text(tip).clicked()
                                };
                                if btn(ui, "×", "Écarter ce fichier des résultats (il ne sera pas modifié)") {
                                    act = Some(SearchAct::Dismiss(fi, None));
                                    clicked_button = true;
                                }
                                if s.show_replace && btn(ui, "Remplacer", "Remplacer dans ce fichier") {
                                    act = Some(SearchAct::Replace(fi, None));
                                    clicked_button = true;
                                }
                                if btn(ui, "📂", "Afficher dans le gestionnaire de fichiers") {
                                    act = Some(SearchAct::Reveal(fi));
                                    clicked_button = true;
                                }
                            }
                            let resp = resp.on_hover_text_at_pointer(f.path.display().to_string());
                            if resp.clicked() && !clicked_button {
                                act = Some(SearchAct::Fold(fi));
                            }
                            if resp.double_clicked() {
                                act = Some(SearchAct::Open(fi, 0));
                            }
                            resp.context_menu(|ui| {
                                let mut item = |ui: &mut egui::Ui, label: &str, a: SearchAct| {
                                    if ui.button(label).clicked() {
                                        act = Some(a);
                                        ui.close_menu();
                                    }
                                };
                                item(ui, "Ouvrir le fichier", SearchAct::Open(fi, 0));
                                item(ui, if collapsed { "Déplier" } else { "Replier" }, SearchAct::Fold(fi));
                                ui.separator();
                                if s.show_replace {
                                    item(ui, "Remplacer dans ce fichier", SearchAct::Replace(fi, None));
                                }
                                item(ui, "Écarter ce fichier des résultats", SearchAct::Dismiss(fi, None));
                                ui.separator();
                                item(ui, "Afficher dans le gestionnaire de fichiers", SearchAct::Reveal(fi));
                                item(ui, "Copier le chemin", SearchAct::Copy(f.path.display().to_string()));
                                item(ui, "Copier le chemin relatif", SearchAct::Copy(f.rel.clone()));
                            });
                        }
                        Some(h) => {
                            let hit = &f.hits[h];
                            let syntax = s.syntaxes.entry(fi).or_insert_with(|| hl.detect(Some(&f.path), ""));
                            let spans = s.colors.entry((fi, h)).or_insert_with(|| hl.line_spans(syntax, &hit.preview));
                            let mut job = LayoutJob::default();
                            append_colored(&mut job, &hit.preview, 0, hit.start, spans, &font, fg);
                            let matched = &hit.preview[hit.start..hit.end];
                            match &preview_re {
                                Some(re) => {
                                    job.append(matched, 0.0, old_fmt.clone());
                                    let new = search::replacement_preview(re, matched, &s.replacement, s.done.regex, s.q.preserve);
                                    job.append(&new, 0.0, new_fmt.clone());
                                }
                                None => job.append(matched, 0.0, match_fmt.clone()),
                            }
                            append_colored(&mut job, &hit.preview, hit.end, hit.preview.len(), spans, &font, fg);
                            let galley = ui.fonts(|fo| fo.layout_job(job));
                            let text_x = 92.0;
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width.max(text_x + galley.size().x + 16.0), row_h),
                                Sense::click(),
                            );
                            let hovered = ui.rect_contains_pointer(rect);
                            let current = s.cursor == Some((fi, h));
                            if current {
                                ui.painter().rect_filled(rect, 4.0, v.selection.bg_fill);
                            } else if hovered {
                                ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
                            }
                            let p = ui.painter().with_clip_rect(rect);
                            let cy = rect.center().y;
                            if current {
                                p.rect_filled(
                                    egui::Rect::from_min_size(egui::pos2(rect.left() + 2.0, rect.top() + 3.0), egui::vec2(3.0, row_h - 6.0)),
                                    1.5,
                                    accent,
                                );
                            }
                            p.text(
                                egui::pos2(rect.left() + text_x - 12.0, cy),
                                Align2::RIGHT_CENTER,
                                (hit.line + 1).to_string(),
                                font.clone(),
                                icons::mix(bg, accent, 0.75),
                            );
                            p.galley(egui::pos2(rect.left() + text_x, cy - galley.size().y / 2.0), galley, fg);
                            let mut clicked_button = false;
                            if hovered {
                                let mut right = rect.right().min(visible_right) - 4.0;
                                let mut btn = |ui: &mut egui::Ui, text: &str, tip: &str| {
                                    let w = 10.0 + 7.5 * text.chars().count() as f32;
                                    let r = egui::Rect::from_min_max(egui::pos2(right - w, cy - 9.0), egui::pos2(right, cy + 9.0));
                                    right -= w + 4.0;
                                    ui.put(r, egui::Button::new(RichText::new(text).small())).on_hover_text(tip).clicked()
                                };
                                if btn(ui, "×", "Écarter ce résultat (il ne sera pas remplacé)") {
                                    act = Some(SearchAct::Dismiss(fi, Some(h)));
                                    clicked_button = true;
                                }
                                if s.show_replace && btn(ui, "Remplacer", "Remplacer cette occurrence") {
                                    act = Some(SearchAct::Replace(fi, Some(h)));
                                    clicked_button = true;
                                }
                            }
                            if resp.clicked() && !clicked_button {
                                act = Some(SearchAct::Open(fi, h));
                            }
                            let ref_text = format!("{}:{}", f.rel, hit.line + 1);
                            resp.context_menu(|ui| {
                                let mut item = |ui: &mut egui::Ui, label: &str, a: SearchAct| {
                                    if ui.button(label).clicked() {
                                        act = Some(a);
                                        ui.close_menu();
                                    }
                                };
                                item(ui, "Ouvrir", SearchAct::Open(fi, h));
                                if s.show_replace {
                                    item(ui, "Remplacer cette occurrence", SearchAct::Replace(fi, Some(h)));
                                }
                                item(ui, "Écarter ce résultat", SearchAct::Dismiss(fi, Some(h)));
                                ui.separator();
                                item(ui, "Afficher dans le gestionnaire de fichiers", SearchAct::Reveal(fi));
                                item(ui, "Copier la référence fichier:ligne", SearchAct::Copy(ref_text.clone()));
                            });
                        }
                    }
                }
            },
        );
        if go {
            self.start_search();
        }
        match act {
            Some(SearchAct::Open(f, h)) => {
                self.open_hit(f, h);
                ui.ctx().memory_mut(|m| m.request_focus(editor_id()));
            }
            Some(SearchAct::Fold(f)) => {
                let s = &mut self.search;
                if !s.collapsed.remove(&f) {
                    s.collapsed.insert(f);
                }
                s.rebuild_rows();
            }
            Some(SearchAct::Dismiss(f, h)) => self.search.dismiss(f, h),
            Some(SearchAct::Replace(f, h)) => self.replace_in_files(Some((f, h))),
            Some(SearchAct::ReplaceAll) => self.confirm = Some(Confirm::ReplaceInFiles),
            Some(SearchAct::Reveal(f)) => {
                if let Some(p) = self.search.files.get(f).map(|f| f.path.clone()) {
                    self.reveal(&p);
                }
            }
            Some(SearchAct::Copy(t)) => {
                ui.ctx().output_mut(|o| o.copied_text = t.clone());
                self.set_status(format!("Copié : {t}"));
            }
            Some(SearchAct::Move(true)) => {
                self.open_search_tab();
                self.settings.search_in_tab = true;
                self.persist("search_in_tab", "true".into());
            }
            Some(SearchAct::Move(false)) => {
                if let Some(i) = self.docs.iter().position(|d| d.view == document::View::Search) {
                    self.close_tab(i, true);
                }
                self.bottom = Some(BottomTab::Search);
                self.search.focus = true;
                self.settings.search_in_tab = false;
                self.persist("search_in_tab", "false".into());
            }
            None => {}
        }
    }

    fn refresh_index(&mut self) {
        let Some(root) = self.root.clone() else { return };
        self.file_index_at = Some(Instant::now());
        self.jobs.spawn(move || {
            let mut v = Vec::new();
            let walker = ignore::WalkBuilder::new(&root).hidden(false).filter_entry(|e| e.file_name() != ".git").build();
            for e in walker.flatten() {
                if e.file_type().is_some_and(|t| t.is_file()) {
                    if let Ok(rel) = e.path().strip_prefix(&root) {
                        v.push(rel.to_path_buf());
                    }
                    if v.len() >= 1_500_000 {
                        break;
                    }
                }
            }
            JobResult::FileIndex(root, v)
        });
    }

    fn plugin_env(&self, idx: usize) -> Vec<(String, String)> {
        let mut env = Vec::new();
        if let Some(r) = &self.root {
            env.push(("FLORM_PROJECT".into(), r.display().to_string()));
        }
        if let Some(d) = self.docs.get(idx) {
            if let Some(p) = &d.path {
                env.push(("FLORM_FILE".into(), p.display().to_string()));
            }
            let (l, c) = d.line_col(d.primary_sel().head);
            env.push(("FLORM_LINE".into(), (l + 1).to_string()));
            env.push(("FLORM_COLUMN".into(), (c + 1).to_string()));
            env.push(("FLORM_SYNTAX".into(), d.syntax.clone()));
        }
        env
    }

    // ------------------------------------------------------------------ save / lint

    fn save(&mut self, idx: usize) -> bool {
        let trim = self.settings.trim_trailing_whitespace_on_save;
        let Some(doc) = self.docs.get_mut(idx) else { return false };
        if doc.read_only {
            return false;
        }
        if doc.path.is_none() {
            self.active = idx;
            self.open_prompt(Prompt::SaveAs);
            return false;
        }
        match doc.save(trim) {
            Ok(()) => {
                let path = doc.path.clone().unwrap();
                self.set_status(format!("Enregistré : {}", path.display()));
                self.after_save(idx, &path);
                true
            }
            Err(e) => {
                self.set_status(format!("Erreur : {e}"));
                false
            }
        }
    }

    fn after_save(&mut self, idx: usize, path: &Path) {
        if let (Some(ix), Some(d)) = (self.sym_index.as_mut(), self.docs.get(idx)) {
            ix.update(path, &d.rope.to_string());
        }
        let canon = |p: &Path| crate::os::canon(p);
        if canon(path) == canon(&config::settings_path()) {
            self.reload_settings();
        }
        if path.file_name().is_some_and(|n| n == "plugin.toml") {
            self.reload_plugins();
        }
        self.analyze(idx, true);
        self.git_refresh();
        if let Some(t) = self.tree.as_mut() {
            t.refresh();
        }
    }

    /// Lints (in-process now, external in background), runs on_save hooks, updates git gutter marks.
    fn analyze(&mut self, idx: usize, saving: bool) {
        let env = self.plugin_env(idx);
        let Some(doc) = self.docs.get_mut(idx) else { return };
        let Some(path) = doc.path.clone() else { return };
        if doc.view != document::View::Text {
            return;
        }
        if let Some(lang) = lint::lang_of(&path) {
            if lint::is_in_process(lang) {
                doc.diagnostics = lint::lint_in_process(lang, &doc.rope.to_string());
                doc.lint_version = doc.version;
            }
        }
        let (uid, version) = (doc.uid, doc.version);
        let hooks: Vec<(PathBuf, String, bool)> = if saving {
            self.plugins
                .iter()
                .flat_map(|p| p.hooks_for("on_save", &path).map(move |h| (p.dir.clone(), h.run.clone(), h.reload)))
                .collect()
        } else {
            Vec::new()
        };
        let lint_on = self.settings.lint_on_save || !saving;
        let root = self.repo_for(&path).or_else(|| self.root.clone());
        let cfg = self.lint_config();
        let text = self.docs[idx].rope.to_string();
        self.jobs.spawn(move || {
            let mut log = Vec::new();
            let mut reload = false;
            for (dir, run, rl) in hooks {
                match plugins::exec(&dir, &run, "", &env) {
                    Ok(o) if !o.trim().is_empty() => log.push(o),
                    Ok(_) => {}
                    Err(e) => log.push(e),
                }
                reload |= rl;
            }
            // a formatting hook rewrote the file: lint what is on disk now
            let text = if reload { std::fs::read_to_string(&path).unwrap_or(text) } else { text };
            let diags = if lint_on { lint::lint_external(&path, &text, root.as_deref(), &cfg) } else { None };
            JobResult::Analyzed { uid, version, reload, diags, base: git::head_text(&path), log }
        });
    }

    /// Moves tab `from` before tab `to` (`to == len`: at the end); the moved tab becomes active.
    fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.docs.len() {
            return;
        }
        let d = self.docs.remove(from);
        let to = if to > from { to - 1 } else { to }.min(self.docs.len());
        self.docs.insert(to, d);
        self.active = to;
        self.tab_scrolled = None;
        self.history.open = false;
        self.prefs.open = false;
        if let Some(sp) = self.split.as_mut() {
            sp.shown = false;
        }
    }

    fn close_tab(&mut self, idx: usize, force: bool) {
        let Some(doc) = self.docs.get(idx) else { return };
        if doc.is_dirty() && !force {
            self.confirm = Some(Confirm::CloseTab(doc.uid));
            return;
        }
        let was_active = idx == self.active;
        let d = self.docs.remove(idx);
        if let document::View::Terminal(id) = d.view {
            self.term.close(id);
        }
        if let Some(p) = d.path {
            self.closed.push(p);
        }
        if was_active {
            // next tab of the same panel (the one on the right, else on the left)
            let same = self.docs.iter().enumerate().filter(|(_, x)| x.pane == d.pane).map(|(j, _)| j);
            self.active = same.min_by_key(|&j| (j < idx, j.abs_diff(idx))).unwrap_or(idx.min(self.docs.len().saturating_sub(1)));
        } else if idx < self.active {
            self.active -= 1;
        }
    }

    // ------------------------------------------------------------------ settings / plugins

    fn reload_settings(&mut self) {
        let (s, err) = config::load();
        if let Some(e) = err {
            self.log(e.clone());
            self.set_status(e);
            return;
        }
        if s.theme != self.hl.theme_name {
            if self.hl.set_theme(&s.theme) {
                self.visuals_dirty = true;
            } else {
                self.log(format!("Thème inconnu : {}", s.theme));
            }
        }
        if s.editor_font != self.settings.editor_font || s.ui_font != self.settings.ui_font {
            self.egui_ctx.set_fonts(crate::fonts::definitions(&s.editor_font, &s.ui_font));
        }
        if s.accent_color != self.settings.accent_color || s.text_softness != self.settings.text_softness {
            self.hl.set_overrides(highlight::parse_hex(&s.accent_color), s.text_softness);
            self.visuals_dirty = true;
        }
        self.settings = s;
        self.rebuild_keymap();
        self.set_status("Préférences rechargées");
    }

    fn reload_plugins(&mut self) {
        let mut dirs = vec![config::plugins_dir()];
        if let Some(r) = &self.root {
            dirs.push(r.join(".florm").join("plugins"));
        }
        let (p, errs) = plugins::load_all(&dirs);
        for e in errs {
            self.log(e);
        }
        self.plugins = p;
        self.plugin_cmds = self
            .plugins
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| (0..p.commands.len()).map(move |ci| (pi, ci)))
            .collect();
        self.rebuild_keymap();
    }

    fn rebuild_keymap(&mut self) {
        let pk: Vec<(String, Cmd)> = self
            .plugin_cmds
            .iter()
            .enumerate()
            .filter_map(|(k, &(pi, ci))| self.plugins[pi].commands[ci].keys.clone().map(|key| (key, Cmd::Plugin(k))))
            .collect();
        let (km, errs) = Keymap::new(&self.settings.keys, &pk);
        self.keymap = km;
        for e in errs {
            self.log(e);
        }
    }

    fn run_plugin(&mut self, k: usize) {
        let Some(&(pi, ci)) = self.plugin_cmds.get(k) else { return };
        let def = self.plugins[pi].commands[ci].clone();
        let dir = self.plugins[pi].dir.clone();
        let env = self.plugin_env(self.active);
        let (uid, version, inputs, output) = match self.docs.get(self.active) {
            Some(d) => {
                let all_empty = d.sels.iter().all(|s| s.is_empty());
                let (inputs, whole) = match def.input {
                    Input::None => (vec![String::new()], false),
                    Input::Buffer => (vec![d.rope.to_string()], true),
                    Input::Selection if all_empty => (vec![d.rope.to_string()], true),
                    Input::Selection => (d.sels.iter().map(|s| d.slice(s.start(), s.end())).collect(), false),
                };
                let output =
                    if whole && def.output == Output::ReplaceSelection { Output::ReplaceBuffer } else { def.output };
                (d.uid, d.version, inputs, output)
            }
            None => (0, 0, vec![String::new()], Output::Panel),
        };
        self.set_status(format!("Plugin : {}…", def.title));
        let title = def.title.clone();
        self.jobs.spawn(move || {
            let mut outs = Vec::new();
            for inp in &inputs {
                match plugins::exec(&dir, &def.run, inp, &env) {
                    Ok(o) => outs.push(o),
                    Err(e) => return JobResult::Plugin { uid, version, title, output, result: Err(e) },
                }
            }
            JobResult::Plugin { uid, version, title, output, result: Ok(outs) }
        });
    }

    // ------------------------------------------------------------------ git

    fn git_refresh(&mut self) {
        let Some(selected) = self.repo.clone() else { return };
        let repos = self.repos.clone();
        self.jobs.spawn(move || {
            let statuses = repos.iter().map(|r| (r.clone(), git::status(r))).collect();
            let remotes = repos.iter().map(|r| (r.clone(), git::stdout(r, &["remote", "-v"]).unwrap_or_default())).collect();
            JobResult::GitStatus { log: git::log(&selected), selected, statuses, remotes }
        });
    }

    fn git_job(&mut self, label: &str, args: &[&str]) {
        let Some(repo) = self.repo.clone() else {
            self.set_status("Pas de dépôt git");
            return;
        };
        self.git_job_at(repo, label, args);
    }

    fn git_job_at(&mut self, repo: PathBuf, label: &str, args: &[&str]) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.log(format!("$ git {}", args.join(" ")));
        self.set_status(format!("git {label}…"));
        self.git.busy += 1;
        let label = label.to_string();
        self.jobs.spawn(move || {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            JobResult::GitOp { label, result: git::run(&repo, &a) }
        });
    }

    /// The graph view on the staged / unstaged changes, scrolled to the diff of `rel`.
    fn show_wip_file(&mut self, rel: String, staged: bool) {
        let Some(repo) = self.repo.clone() else { return };
        self.prefs.open = false;
        if let Some(sp) = self.split.as_mut() {
            sp.shown = false;
        }
        let reload = !self.history.open || self.history.repo.as_ref() != Some(&repo) || self.history.graph.is_none();
        self.history.open = true;
        let h = &mut self.history;
        h.compare = None;
        h.rebase = None;
        let key = if staged { "INDEX" } else { "WORKTREE" };
        let shown = h.wip_sel == Some(staged) && h.diff.as_ref().is_some_and(|d| d.hash == key);
        h.wip_sel = Some(staged);
        match h.diff.as_ref().filter(|_| shown).and_then(|d| d.files.iter().position(|f| f.path == rel)) {
            Some(k) => h.jump = Some(history::FileJump::To(k)),
            None => {
                h.diff = None;
                h.jump_path = Some(rel);
                self.load_wip_all(staged);
            }
        }
        if reload {
            self.load_history();
        }
    }

    fn show_diff(&mut self, rel: String, untracked: bool) {
        let Some(repo) = self.repo.clone() else { return };
        if untracked {
            self.open_path(repo.join(&rel));
            return;
        }
        self.jobs.spawn(move || {
            let text = git::run(&repo, &["diff", "HEAD", "--", &rel])
                .or_else(|_| git::run(&repo, &["diff", "--cached", "--", &rel]))
                .unwrap_or_else(|e| e);
            let text = if text.is_empty() { "(aucune différence)".into() } else { text };
            JobResult::NewTab { title: format!("diff : {rel}"), text, syntax: "Diff".into() }
        });
    }

    // ------------------------------------------------------------------ jobs

    fn process_jobs(&mut self) {
        while let Ok(r) = self.jobs.rx.try_recv() {
            match r {
                JobResult::Analyzed { uid, version, reload, diags, base, log } => {
                    for l in log {
                        self.log(l);
                    }
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                        if reload && d.version == version {
                            let _ = d.reload_from_disk();
                        }
                        if let Some((dg, linter)) = diags {
                            d.diagnostics = dg;
                            d.lint_version = d.version;
                            if lint::lang_of(d.path.as_deref().unwrap_or(Path::new(""))) == Some("python") {
                                self.python_linter = Some(linter);
                            }
                        }
                        d.git_base = base;
                        d.refresh_diff();
                    }
                }
                JobResult::Lint { uid, version, diags, linter } => {
                    self.lint_inflight.remove(&uid);
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                        if d.version == version {
                            d.diagnostics = diags;
                            d.lint_version = version;
                        }
                    }
                    self.python_linter = Some(linter);
                }
                JobResult::ToolInstalled(r) => {
                    // a language server may have been installed: try again
                    self.lsp.reset();
                    self.lsp_note.clear();
                    match r {
                        Ok(msg) => {
                            self.log(msg.clone());
                            self.set_status(msg);
                        }
                        Err(e) => {
                            self.log(e);
                            self.set_status("Installation échouée (voir Sortie)");
                            self.bottom = Some(BottomTab::Output);
                        }
                    }
                    for d in self.docs.iter_mut() {
                        d.lint_version = u64::MAX; // re-lint with the new tool
                    }
                }
                JobResult::FileIndex(root, v) => {
                    if self.root.as_ref() == Some(&root) && self.sym_index_for.as_ref() != Some(&root) {
                        self.sym_index_for = Some(root.clone());
                        let r = root.clone();
                        self.jobs.spawn(move || {
                            let idx = crate::index::Index::build(&r);
                            JobResult::SymbolIndex(r, idx)
                        });
                    }
                    if self.root.as_ref() == Some(&root) && self.file_index != v {
                        self.file_index = v;
                        // "go to file" opened while the list was being made: show the new list
                        if self.overlay.as_ref().is_some_and(|o| matches!(o.mode, Mode::Files)) {
                            let items = self.file_items();
                            if let Some(o) = self.overlay.as_mut() {
                                o.items = items;
                                o.last_query = None;
                                o.matched = None;
                            }
                        }
                    }
                }
                JobResult::GitStatus { selected, statuses, log, remotes } => {
                    if self.repo.as_ref() != Some(&selected) {
                        continue;
                    }
                    // branch / remote of every repository, for the tree
                    self.repo_info.clear();
                    for (repo, st) in &statuses {
                        let Ok(st) = st else { continue };
                        let mut label = st.branch.clone();
                        if let Some((up, ahead, behind)) = st.upstream() {
                            label.push_str(&format!(" → {up}"));
                            if ahead > 0 {
                                label.push_str(&format!(" ↑{ahead}"));
                            }
                            if behind > 0 {
                                label.push_str(&format!(" ↓{behind}"));
                            }
                        }
                        let raw = remotes.iter().find(|(r, _)| r == repo).map(|(_, t)| t.as_str()).unwrap_or("");
                        let mut urls: Vec<String> = raw
                            .lines()
                            .filter(|l| l.ends_with("(fetch)"))
                            .map(|l| l.trim_end_matches("(fetch)").trim().replacen('\t', " : ", 1))
                            .collect();
                        urls.dedup();
                        let mut tip = format!("Branche : {}\n", st.branch);
                        match st.upstream() {
                            Some((up, a, b)) => tip.push_str(&format!("Suit : {up} ({a} à pousser, {b} à récupérer)\n")),
                            None => tip.push_str("Pas de branche distante suivie\n"),
                        }
                        if urls.is_empty() {
                            tip.push_str("Aucun remote");
                        } else {
                            tip.push_str(&format!("Remote(s) :\n{}", urls.join("\n")));
                        }
                        let n = st.entries.len();
                        if n > 0 {
                            label.push_str(&format!(" · {n} modif."));
                        }
                        self.repo_info.insert(repo.clone(), (label, tip));
                    }
                    self.git.log = log;
                    self.git.map.clear();
                    self.git.status = None;
                    self.git.error = None;
                    let root = self.root.clone();
                    for (repo, st) in statuses {
                        match st {
                            Ok(st) => {
                                for e in &st.entries {
                                    let abs = repo.join(&e.path);
                                    // Color parent folders up to the opened folder (repos may be nested in it).
                                    for a in abs.ancestors().skip(1) {
                                        if root.as_ref().is_some_and(|r| !a.starts_with(r) || a == r.as_path()) {
                                            break;
                                        }
                                        self.git.map.entry(a.to_path_buf()).or_insert('M');
                                    }
                                    self.git.map.insert(abs, e.label());
                                }
                                if repo == selected {
                                    self.git.status = Some(st);
                                }
                            }
                            Err(e) if repo == selected => self.git.error = Some(e),
                            Err(_) => {}
                        }
                    }
                }
                JobResult::History(repo, r) => {
                    if self.history.repo.as_ref() == Some(&repo) {
                        self.history.loading = false;
                        match r {
                            Ok(g) => {
                                let pick = self.history.select_hash.take().and_then(|h| g.commits.iter().position(|c| c.hash == h));
                                self.history.graph = Some(g);
                                self.history.error = None;
                                if let Some(i) = pick {
                                    self.history.wip_sel = None;
                                    self.history.diff = None;
                                    self.select_commit(i);
                                }
                            }
                            Err(e) => self.history.error = Some(e),
                        }
                    }
                }
                JobResult::Blame { uid, line, version, result } => {
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                        let text = match result {
                            Ok(b) if b.uncommitted() => "Vous · pas encore commité".to_string(),
                            Ok(b) => format!(
                                "{} · {} · {} ({})",
                                b.author,
                                git::ago(b.time),
                                b.summary,
                                &b.hash[..b.hash.len().min(7)]
                            ),
                            Err(_) => "pas de blame (fichier hors git ou non suivi)".to_string(),
                        };
                        d.blame = Some((line, version, text));
                    }
                }
                JobResult::LineHistory(result) => {
                    if let Some(lh) = self.line_history.as_mut() {
                        lh.loading = false;
                        match result {
                            Ok(commits) => {
                                lh.diffs = commits.iter().map(|c| history::parse_diff(&c.diff)).collect();
                                lh.commits = commits;
                            }
                            Err(e) => lh.error = Some(e),
                        }
                    }
                }
                JobResult::HistoryDiff { hash, path, result } => {
                    if self.history_key().as_deref() == Some(hash.as_str()) {
                        self.history.diff_loading = false;
                        let text = result.unwrap_or_else(|e| e);
                        let text = if text.trim().is_empty() { "(aucune différence)".to_string() } else { text };
                        let lines = history::parse_diff(&text);
                        let files = history::diff_files(&lines);
                        // a reload of the same diff (after staging…) keeps the folded files
                        let same = self.history.diff.as_ref().is_some_and(|d| d.hash == hash);
                        if !same {
                            self.history.collapsed.clear();
                        }
                        if let Some(p) = self.history.jump_path.take() {
                            if let Some(k) = files.iter().position(|f| f.path == p) {
                                self.history.jump = Some(history::FileJump::To(k));
                            }
                        }
                        self.history.diff = Some(HistoryDiff { hash, path, text, lines, files });
                    }
                }
                JobResult::HistoryDetails(hash, r) => {
                    if self.history_key().as_deref() == Some(hash.as_str()) {
                        match r {
                            Ok(d) => self.history.details = Some(d),
                            Err(e) => self.log(e),
                        }
                    }
                }
                JobResult::GitBranches(list) => {
                    let items =
                        list.into_iter().map(|b| Item { label: b.clone(), detail: String::new(), action: Action::Branch(b) }).collect();
                    self.open_overlay_with(Mode::Branches, items, String::new());
                }
                JobResult::GitOp { label, result } => {
                    self.git.busy = self.git.busy.saturating_sub(1);
                    match result {
                        Ok(out) => {
                            self.log(out);
                            self.git.op_error = None;
                            self.set_status(format!("git {label} : OK"));
                        }
                        Err(e) => {
                            self.log(e.clone());
                            if self.history.open {
                                let first = e.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
                                self.history.notice = Some((format!("git {label} a échoué : {first}"), err()));
                            }
                            self.git.op_error = Some((label.clone(), e));
                            self.set_status(format!("git {label} : échec (détail dans le panneau Git)"));
                        }
                    }
                    // the rebase stopped on the commit to split: open the assistant on it
                    if label == "rebase (découpage)" && self.split.is_none() && self.history.repo.as_deref().is_some_and(crate::split::rebase_in_progress) {
                        self.resume_split();
                    }
                    self.git_refresh();
                    // staging, unstaging or discarding does not move branches: keep the selection
                    let refs = !matches!(label.as_str(), "add" | "restore" | "apply" | "rm");
                    if self.history.open && refs {
                        self.load_history();
                    }
                    if let (true, Some(staged)) = (self.history.open, self.history.wip_sel) {
                        self.load_wip_all(staged);
                    }
                    if let Some(t) = self.tree.as_mut() {
                        t.refresh();
                    }
                    for i in 0..self.docs.len() {
                        if self.docs[i].path.is_some() {
                            self.analyze(i, false);
                        }
                    }
                }
                JobResult::NewTab { title, text, syntax } => {
                    self.history.open = false;
                    self.new_scratch(title, text, &syntax, true);
                }
                JobResult::SplitStarted(repo, r) => {
                    self.history.loading = false;
                    match r {
                        Ok((info, files)) => {
                            self.set_status(format!("Découpage de {} : choisis le contenu de chaque partie", info.short));
                            let mut s = SplitUi {
                                repo,
                                message: info.message.clone(),
                                info,
                                files: Vec::new(),
                                sel: Vec::new(),
                                folded: HashSet::new(),
                                parts: Vec::new(),
                                busy: false,
                                error: None,
                                confirm_abort: false,
                                shown: true,
                            };
                            s.set_files(files);
                            self.split = Some(s);
                            self.history.notice = None;
                        }
                        Err(e) => {
                            self.log(e.clone());
                            self.history.notice = Some((e, err()));
                        }
                    }
                    self.after_split_change();
                }
                JobResult::SplitChanges { result, committed } => {
                    if let Some(s) = self.split.as_mut() {
                        s.busy = false;
                        if let Some(c) = committed {
                            s.parts.push(c);
                            s.message.clear();
                        }
                        match result {
                            Ok(files) => s.set_files(files),
                            Err(e) => s.error = Some(e),
                        }
                    }
                    self.after_split_change();
                }
                JobResult::SplitEnded { aborted, result } => {
                    match result {
                        Ok(_) => {
                            let n = self.split.take().map_or(0, |s| s.parts.len());
                            let msg = if aborted {
                                "Découpage annulé : tout est revenu comme avant (commit d'origine rétabli).".to_string()
                            } else {
                                format!("Commit découpé en {n} commits.")
                            };
                            self.set_status(msg.clone());
                            self.history.notice = Some((msg, ok()));
                            self.history.open = true;
                            self.prefs.open = false;
                            self.load_history();
                        }
                        Err(e) => {
                            self.log(e.clone());
                            if let Some(s) = self.split.as_mut() {
                                s.busy = false;
                                s.error = Some(e);
                            }
                        }
                    }
                    self.after_split_change();
                }
                JobResult::Picked(kind, path) => {
                    self.dialog_open = false;
                    if let Err(e) = &path {
                        self.set_status(e.clone());
                    }
                    if let Ok(Some(p)) = path {
                        let prompt = match kind {
                            0 => Prompt::OpenFolder,
                            1 => Prompt::OpenFile,
                            _ => Prompt::SaveAs,
                        };
                        self.submit_prompt(prompt, p.display().to_string());
                    }
                }
                JobResult::Repos(root, repos) => {
                    if self.root.as_ref() == Some(&root) {
                        self.repos = repos;
                        let active = self.docs.get(self.active).and_then(|d| d.path.clone());
                        let pick = active.as_deref().and_then(|p| self.repo_for(p)).or_else(|| self.repos.first().cloned());
                        if let Some(r) = pick {
                            self.select_repo(r);
                        }
                        // git marks of the files already open
                        for i in 0..self.docs.len() {
                            if self.docs[i].path.is_some() {
                                self.analyze(i, false);
                            }
                        }
                    }
                }
                JobResult::Fonts(list) => {
                    self.font_list = Some(list);
                    self.font_list_loading = false;
                }
                JobResult::SymbolIndex(root, idx) => {
                    if self.root.as_ref() == Some(&root) {
                        self.log(format!("Projet indexé : {} symboles dans {} fichiers", idx.len(), idx.files.len()));
                        self.sym_index = Some(idx);
                    }
                }
                JobResult::Search(gen, out) => {
                    if gen == self.search.gen.load(Ordering::SeqCst) {
                        let s = &mut self.search;
                        s.running = false;
                        s.error = out.error;
                        s.stats = Some((out.scanned, out.millis, out.truncated));
                        s.files = out.files;
                        s.colors.clear();
                        s.syntaxes.clear();
                        s.collapsed.clear();
                        s.rebuild_rows();
                        s.cursor = None;
                    }
                }
                JobResult::Plugin { uid, version, title, output, result } => {
                    self.apply_plugin_result(uid, version, title, output, result)
                }
            }
        }
    }

    fn apply_plugin_result(&mut self, uid: u64, version: u64, title: String, output: Output, result: Result<Vec<String>, String>) {
        let outs = match result {
            Ok(o) => o,
            Err(e) => {
                self.log(format!("[{title}] {e}"));
                self.bottom = Some(BottomTab::Output);
                self.set_status(format!("Plugin « {title} » : erreur"));
                return;
            }
        };
        match output {
            Output::None => {}
            Output::Panel => {
                self.log(format!("── {title} ──"));
                for o in outs {
                    self.log(o);
                }
                self.bottom = Some(BottomTab::Output);
            }
            Output::NewTab => self.new_scratch(title, outs.join("\n"), "Plain Text", false),
            _ => {
                let Some(doc) = self.docs.iter_mut().find(|d| d.uid == uid) else { return };
                if doc.version != version {
                    self.set_status("Le document a changé pendant le plugin : résultat ignoré");
                    return;
                }
                // Keep the trailing newline only if the input had one.
                let fit = |input: &str, out: String| -> String {
                    if !input.ends_with('\n') {
                        out.strip_suffix('\n').map(String::from).unwrap_or(out)
                    } else {
                        out
                    }
                };
                match output {
                    Output::ReplaceBuffer => {
                        let cur = doc.rope.to_string();
                        let new = fit(&cur, outs.concat());
                        doc.set_text(&new);
                    }
                    Output::ReplaceSelection if outs.len() == doc.sels.len() => {
                        let items: Vec<Edit> = doc
                            .sels
                            .iter()
                            .zip(outs)
                            .map(|(s, o)| {
                                let t = fit(&doc.slice(s.start(), s.end()), o);
                                let n = t.chars().count();
                                Edit { start: s.start(), end: s.end(), text: t, anchor: 0, head: n }
                            })
                            .collect();
                        doc.edit(EditKind::Other, items);
                    }
                    Output::Insert => {
                        let t = fit("", outs.concat());
                        doc.insert_text(&t, false);
                    }
                    _ => {}
                }
                self.set_status(format!("Plugin « {title} » : OK"));
            }
        }
    }

    // ------------------------------------------------------------------ input

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if let Some(cap) = self.prefs.capture {
            self.capture_shortcut(ctx, cap);
            return;
        }
        let editor_focused = !self.history.open && !self.prefs.open && ctx.memory(|m| m.has_focus(editor_id()));
        let term_focused = self.term.has_focus(ctx);
        if let Some(t) = self.drop_alt_text {
            if Instant::now() < t {
                ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(t) if t.chars().count() == 1)));
            } else {
                self.drop_alt_text = None;
            }
        }
        let events = ctx.input(|i| i.events.clone());
        let mut consumed = vec![false; events.len()];
        let mut cmds = Vec::new();
        for (i, ev) in events.iter().enumerate() {
            let Event::Key { key, pressed: true, modifiers, .. } = ev else { continue };
            let combo = Combo::from_event(*key, *modifiers);
            if !self.chord && self.keymap.is_chord_start(&combo) {
                self.chord = true;
                consumed[i] = true;
                continue;
            }
            let in_chord = std::mem::take(&mut self.chord);
            match self.keymap.lookup(combo, in_chord) {
                Some(cmd) => {
                    if cmd.is_editor() && (!editor_focused || self.overlay.is_some()) {
                        continue;
                    }
                    // in the terminal, keys go to the shell, except a few global shortcuts
                    if term_focused
                        && !matches!(cmd, Cmd::ToggleTerminal | Cmd::CommandPalette | Cmd::QuickOpen | Cmd::ToggleSidebar)
                    {
                        continue;
                    }
                    consumed[i] = true;
                    cmds.push(cmd);
                }
                None if in_chord => consumed[i] = true,
                None => {}
            }
        }
        if consumed.iter().any(|c| *c) {
            ctx.input_mut(|inp| {
                let mut k = 0;
                inp.events.retain(|_| {
                    let keep = !consumed.get(k).copied().unwrap_or(false);
                    k += 1;
                    keep
                });
            });
        }
        // Alt+letter also arrives as typed text ("l" for Alt+L): drop the text of the key press
        // when it triggered a shortcut
        let alt_shortcut = events
            .iter()
            .zip(&consumed)
            .any(|(e, used)| *used && matches!(e, Event::Key { modifiers, .. } if modifiers.alt && !modifiers.ctrl && !modifiers.command));
        if alt_shortcut {
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(t) if t.chars().count() == 1)));
            // in case the text comes with the next frame
            self.drop_alt_text = Some(Instant::now() + Duration::from_millis(80));
        }
        for c in cmds {
            self.exec(ctx, c);
        }
    }

    /// Records the next key combination for the shortcut editor.
    fn capture_shortcut(&mut self, ctx: &egui::Context, cap: Capture) {
        let events = ctx.input(|i| i.events.clone());
        let mut first = cap.first;
        let mut result: Option<Option<Seq>> = None; // Some(None) = cancelled
        for ev in &events {
            let Event::Key { key, pressed: true, modifiers, .. } = ev else { continue };
            let combo = Combo::from_event(*key, *modifiers);
            if *key == Key::Escape && first.is_none() {
                result = Some(None);
                break;
            }
            if first.is_none() && self.keymap.is_chord_start(&combo) {
                first = Some(combo);
                continue;
            }
            result = Some(Some(first.into_iter().chain([combo]).collect()));
            break;
        }
        // the keys being recorded must not reach shortcuts or text fields
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_))));
        self.chord = false;
        match result {
            Some(None) => self.prefs.capture = None,
            Some(Some(seq)) => {
                self.prefs.capture = None;
                self.propose_shortcut(cap.cmd, seq);
            }
            None => self.prefs.capture = Some(Capture { first, ..cap }),
        }
    }

    fn editor_input(&mut self, ctx: &egui::Context) {
        if self.overlay.is_some() || self.history.open || self.prefs.open || !ctx.memory(|m| m.has_focus(editor_id())) {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let auto = self.settings.auto_pair;
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        for ev in events {
            match ev {
                Event::Text(t) => {
                    doc.insert_text(&t, auto);
                    // suggestions after "." or two letters of a word (if a server handles the file)
                    let last = t.chars().last();
                    let word = |c: char| c.is_alphanumeric() || c == '_';
                    let head = doc.primary_sel().head;
                    let typed = (head.saturating_sub(2)..head).all(|p| doc.char_at(p).is_some_and(word));
                    let has_server = self.settings.lsp && doc.path.as_deref().and_then(lsp::language_of).is_some();
                    if self.completion.is_none() && ((has_server && last == Some('.')) || (last.is_some_and(word) && typed)) {
                        self.complete_at = Some(Instant::now() + Duration::from_millis(120));
                    }
                }
                Event::Copy => {
                    let (t, lw) = doc.copy_text();
                    self.last_line_copy = lw.then(|| t.clone());
                    ctx.output_mut(|o| o.copied_text = t);
                }
                Event::Cut => {
                    let (t, lw) = doc.cut();
                    self.last_line_copy = lw.then(|| t.clone());
                    ctx.output_mut(|o| o.copied_text = t);
                }
                Event::Paste(t) => {
                    let lw = self.last_line_copy.as_deref() == Some(t.replace("\r\n", "\n").as_str());
                    doc.paste(&t, lw);
                }
                _ => {}
            }
        }
    }

    fn exec(&mut self, ctx: &egui::Context, cmd: Cmd) {
        use Cmd::*;
        if matches!(cmd, GitRevertChange | GitRevertFile) {
            if let Some(d) = self.docs.get_mut(self.active) {
                if d.diff_version != d.version {
                    d.refresh_diff();
                }
                let line = d.rope.char_to_line(d.primary_sel().head);
                let msg = if cmd == GitRevertChange && !d.hunks.iter().any(|h| h.contains(line)) {
                    "Pas de changement git sur cette ligne"
                } else if cmd == GitRevertFile && d.git_base.is_none() {
                    "Ce fichier n'est pas suivi par git"
                } else if cmd == GitRevertChange {
                    "Changement git annulé (Ctrl+Z pour le récupérer) — à enregistrer"
                } else {
                    "Fichier remis comme au dernier commit (Ctrl+Z pour revenir) — à enregistrer"
                };
                self.set_status(msg);
            }
        }
        if cmd.is_editor() {
            let auto = self.settings.auto_pair;
            if let Some(d) = self.docs.get_mut(self.active) {
                d.exec(cmd, auto);
            }
            return;
        }
        match cmd {
            Escape => {
                let peek_open = self.docs.get(self.active).is_some_and(|d| d.peek.is_some());
                if self.overlay.take().is_some() || self.confirm.take().is_some() || self.line_history.take().is_some() {
                } else if let Some(sp) = self.split.as_mut().filter(|sp| sp.shown && !self.prefs.open) {
                    sp.shown = false;
                } else if self.history.open {
                    self.history.open = false;
                } else if self.prefs.open {
                    self.prefs.open = false;
                } else if peek_open {
                    self.docs[self.active].peek = None;
                } else if self.find.open {
                    self.find.open = false;
                } else if let Some(d) = self.docs.get_mut(self.active) {
                    if d.sels.len() > 1 {
                        let p = d.primary_sel();
                        d.sels = vec![p];
                        d.primary = 0;
                    }
                }
                self.focus_editor(ctx);
            }
            CommandPalette => self.open_overlay(Mode::Commands),
            QuickOpen => {
                // the list is rebuilt in the background (new files), the last one is shown meanwhile
                if self.file_index.is_empty() || self.file_index_at.map_or(true, |t| t.elapsed() > Duration::from_secs(5)) {
                    self.refresh_index();
                }
                self.open_overlay(Mode::Files);
            }
            GotoLine | GotoLineSelect => {
                self.goto_select = cmd == GotoLineSelect;
                self.open_overlay(Mode::GotoLine);
            }
            Find | Replace => {
                self.find.open = true;
                self.find.replace_mode = cmd == Replace;
                self.find.focus = true;
                if let Some(d) = self.docs.get(self.active) {
                    let p = d.primary_sel();
                    if !p.is_empty() {
                        let t = d.slice(p.start(), p.end());
                        if !t.contains('\n') {
                            self.find.query = t;
                        }
                    }
                }
            }
            FindInFiles | FindInFilesTab => {
                if cmd == FindInFilesTab || self.settings.search_in_tab {
                    self.open_search_tab();
                } else {
                    self.bottom = Some(BottomTab::Search);
                }
                self.search.focus = true;
                if let Some(d) = self.docs.get(self.active) {
                    let p = d.primary_sel();
                    let t = d.slice(p.start(), p.end());
                    if !t.is_empty() && !t.contains('\n') {
                        self.search.q.pattern = t;
                    }
                }
            }
            NextResult => self.step_result(true),
            PrevResult => self.step_result(false),
            SwitchProject => self.open_overlay(Mode::Projects),
            ShowProjects => {
                self.show_sidebar = true;
                self.side_tab = SideTab::Projects;
            }
            CloseFolder => self.close_folder(),
            FindNext => self.find_step(true, false),
            FindPrev => self.find_step(false, false),
            FindAll => self.find_select_all(ctx),
            Save => {
                self.save(self.active);
            }
            SaveAs => {
                if !self.native_dialog(2) {
                    self.open_prompt(Prompt::SaveAs);
                }
            }
            SaveAll => {
                for i in 0..self.docs.len() {
                    if self.docs[i].is_dirty() && self.docs[i].path.is_some() {
                        self.save(i);
                    }
                }
            }
            NewFile => self.new_doc(),
            OpenFile => {
                if !self.native_dialog(1) {
                    self.open_prompt(Prompt::OpenFile);
                }
            }
            OpenFolder => {
                if !self.native_dialog(0) {
                    self.open_prompt(Prompt::OpenFolder);
                }
            }
            OpenFolderPath => self.open_prompt(Prompt::OpenFolder),
            CloseTab => self.close_tab(self.active, false),
            ReopenClosed => {
                if let Some(p) = self.closed.pop() {
                    self.open_path(p);
                }
            }
            NextTab if !self.docs.is_empty() => self.active = (self.active + 1) % self.docs.len(),
            PrevTab if !self.docs.is_empty() => self.active = (self.active + self.docs.len() - 1) % self.docs.len(),
            ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            SplitRight | SplitDown => {
                let dir = if cmd == SplitRight { SplitDir::Right } else { SplitDir::Down };
                if self.is_split() {
                    self.split_dir = dir;
                } else if self.active < self.docs.len() {
                    self.split_tab(self.active, Some(dir));
                }
            }
            Unsplit => self.unsplit(),
            FocusOtherPane => {
                if let Some(i) = self.pane_doc(1 - self.focused_pane) {
                    self.activate_tab(i);
                }
            }
            NewTerminalTab => self.open_terminal_tab(),
            ToggleTerminal => {
                if self.bottom == Some(BottomTab::Terminal) {
                    self.bottom = None;
                    self.focus_editor(ctx);
                } else {
                    self.bottom = Some(BottomTab::Terminal);
                    self.term.want_focus = true;
                }
            }
            ShowProblems => self.bottom = Some(BottomTab::Problems),
            ShowOutput => self.bottom = Some(BottomTab::Output),
            ShowGit => {
                self.show_sidebar = true;
                self.side_tab = SideTab::Git;
                self.git_refresh();
            }
            ShowFiles => {
                // a second time: back to the editor
                if self.show_sidebar && self.side_tab == SideTab::Files && ctx.memory(|m| m.has_focus(sidebar::tree_id())) {
                    self.focus_editor(ctx);
                } else {
                    self.show_sidebar = true;
                    self.side_tab = SideTab::Files;
                    // the keyboard goes to the tree, on the current file
                    if let Some(p) = self.docs.get(self.active).and_then(|d| d.path.clone()) {
                        if self.tree_nav.cursor.is_none() || !ctx.memory(|m| m.has_focus(sidebar::tree_id())) {
                            self.reveal_in_tree(&p);
                        }
                    }
                    self.tree_nav.focus = true;
                }
            }
            ToggleMenuBar => {
                self.settings.show_menu_bar = !self.settings.show_menu_bar;
                self.persist("show_menu_bar", self.settings.show_menu_bar.to_string());
                if !self.settings.show_menu_bar {
                    self.set_status("Barre de menus masquée : les menus restent dans ☰ en bas à gauche");
                }
            }
            RevealInTree => {
                if let Some(p) = self.docs.get(self.active).and_then(|d| d.path.clone()) {
                    self.reveal_in_tree(&p);
                }
            }
            NavBack => self.nav_go(true),
            NavForward => self.nav_go(false),
            Complete => self.request_completion(ctx),
            GotoDefinition => self.goto_definition(ctx),
            RenameSymbol => {
                let i = self.active;
                match self.lsp_for(i, ctx, true) {
                    None => {
                        self.set_status("Renommer dans tout le projet demande un serveur de langage (pour Python : « installer pylsp » dans la barre d'état)");
                    }
                    Some(_) => {
                        let w = self.word_under_cursor();
                        self.open_overlay_with(Mode::Prompt(Prompt::RenameSymbol), Vec::new(), w);
                    }
                }
            }
            InstallPylsp => {
                self.set_status("Installation de pylsp (pip)…");
                self.log(format!("pip install --target {} python-lsp-server", lint::tools_dir().display()));
                self.jobs.spawn(|| JobResult::ToolInstalled(lint::install_python_tool("python-lsp-server")));
            }
            LspRestart => {
                self.lsp.reset();
                self.lsp_req.clear();
                self.completion = None;
                self.lsp_note.clear();
                self.set_status("Serveurs de langage redémarrés");
            }
            MarkdownPreview => self.markdown_preview(false),
            MarkdownPreviewSide => self.markdown_preview(true),
            GotoSymbol => {
                if self.docs.get(self.active).is_some() {
                    self.open_overlay(Mode::Symbols);
                    if self.overlay.as_ref().is_some_and(|o| o.items.is_empty()) {
                        self.overlay = None;
                        self.set_status("Aucun symbole reconnu dans ce fichier");
                    }
                }
            }
            Fold | Unfold | FoldAll | UnfoldAll => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    match cmd {
                        Fold => d.fold_at_cursor(),
                        Unfold => d.unfold_at_cursor(),
                        FoldAll => d.fold_all(),
                        _ => d.unfold_all(),
                    }
                }
            }
            ToggleBreadcrumbs => {
                self.settings.breadcrumbs = !self.settings.breadcrumbs;
                self.persist("breadcrumbs", self.settings.breadcrumbs.to_string());
            }
            ToggleMinimap => {
                self.settings.show_minimap = !self.settings.show_minimap;
                self.persist("show_minimap", self.settings.show_minimap.to_string());
            }
            Copy | Cut => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    let (t, linewise) = if cmd == Cut && !d.read_only { d.cut() } else { d.copy_text() };
                    self.last_line_copy = linewise.then(|| t.clone());
                    ctx.output_mut(|o| o.copied_text = t);
                }
            }
            Paste => match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                Ok(t) => {
                    let linewise = self.last_line_copy.as_deref() == Some(t.replace("\r\n", "\n").as_str());
                    if let Some(d) = self.docs.get_mut(self.active) {
                        d.paste(&t, linewise);
                    }
                }
                Err(e) => self.set_status(format!("Presse-papiers illisible : {e}")),
            },
            CopyFilePath | CopyFileRef => {
                if let Some(d) = self.docs.get(self.active) {
                    if let Some(p) = &d.path {
                        let text = if cmd == CopyFilePath {
                            p.display().to_string()
                        } else {
                            let rel = self.root.as_ref().and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p);
                            format!("{}:{}", rel.display(), d.rope.char_to_line(d.primary_sel().head) + 1)
                        };
                        ctx.output_mut(|o| o.copied_text = text.clone());
                        self.set_status(format!("Copié : {text}"));
                    }
                }
            }
            CopyGithubLink => self.copy_github_link(ctx),
            RevealInOs => {
                if let Some(p) = self.docs.get(self.active).and_then(|d| d.path.clone()) {
                    self.reveal(&p);
                }
            }
            FindWordInFiles => {
                let w = self.word_under_cursor();
                if !w.is_empty() {
                    self.search.q.pattern = w;
                    self.bottom = Some(BottomTab::Search);
                    self.start_search();
                }
            }
            NoqaLine => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    let line = d.rope.char_to_line(d.primary_sel().head);
                    let codes = noqa_codes(d, line);
                    if !codes.is_empty() && !d.read_only {
                        d.add_noqa(line, &codes);
                    }
                }
            }
            InstallFlake8 => {
                self.set_status("Installation de flake8 (pip)…");
                self.log(format!("pip install --target {} flake8", lint::tools_dir().display()));
                self.jobs.spawn(|| JobResult::ToolInstalled(lint::install_python_tool("flake8")));
            }
            ShowChange => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    let line = d.rope.char_to_line(d.primary_sel().head);
                    d.peek = d.hunks.iter().find(|h| h.contains(line)).map(|h| h.mark_lines().start);
                }
            }
            ClearSearchHistory => {
                self.state.search_history.clear();
                self.state.include_history.clear();
                self.state.exclude_history.clear();
                project::save_state(&self.state);
                self.set_status("Historique de recherche effacé");
            }
            GitBlame => {
                self.blame_mode = !self.blame_mode;
                self.blame_req = None;
                if !self.blame_mode {
                    for d in self.docs.iter_mut() {
                        d.blame = None;
                    }
                }
                self.set_status(if self.blame_mode {
                    "Blame activé (Ctrl+B pour masquer, Ctrl+Alt+B : historique de la ligne)"
                } else {
                    "Blame masqué"
                });
            }
            GitLineHistory => self.open_line_history(),
            ToggleInlineDiff => {
                self.settings.inline_diff = !self.settings.inline_diff;
                self.persist("inline_diff", self.settings.inline_diff.to_string());
                self.set_status(if self.settings.inline_diff {
                    "Diff dans l'éditeur : activé (Ctrl+Alt+D pour masquer)"
                } else {
                    "Diff dans l'éditeur : désactivé"
                });
            }
            ZoomIn | ZoomOut => {
                let d = if cmd == ZoomIn { 1.0 } else { -1.0 };
                self.settings.font_size = (self.settings.font_size + d).clamp(7.0, 48.0);
                self.persist("font_size", format!("{:.1}", self.settings.font_size));
            }
            SetSyntax => self.open_overlay(Mode::Syntax),
            SelectTheme => self.open_overlay(Mode::Themes),
            Lint => {
                self.analyze(self.active, false);
                self.bottom = Some(BottomTab::Problems);
            }
            OpenSettings => {
                self.prefs.open = true;
                self.history.open = false;
                self.prefs.lint_args = self.settings.python_lint_args.join(" ");
                self.prefs.shell = self.settings.shell.clone().unwrap_or_default();
            }
            OpenSettingsFile => {
                self.prefs.open = false;
                self.open_path(config::settings_path());
            }
            ReloadPlugins => {
                self.reload_plugins();
                self.set_status(format!("{} plugin(s) chargé(s)", self.plugins.len()));
            }
            ShowGuide => self.open_view_tab(document::View::Help, "❓ Guide de Florm".into()),
            InsertTemplate => self.open_overlay(Mode::Templates),
            OpenTemplatesFolder => {
                let dir = crate::templates::dir();
                crate::templates::install_readme(&dir);
                self.open_path(dir.join("README.md"));
                self.set_status(format!("Dossier des modèles : {} — un fichier par modèle", dir.display()));
            }
            OpenPluginsFolder => {
                let dir = config::plugins_dir();
                let _ = std::fs::create_dir_all(&dir);
                self.log(format!("Dossier des plugins : {}", dir.display()));
                self.open_path(dir.join("README.md"));
            }
            RefreshTree => {
                if let Some(t) = self.tree.as_mut() {
                    t.refresh();
                }
                self.refresh_index();
            }
            Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            GitRefresh => self.git_refresh(),
            GitPull => self.git_job("pull", &["pull"]),
            GitPush => self.git_job("push", &["push", "-u", "origin", "HEAD"]),
            GitFetch => self.git_job("fetch", &["fetch", "--all", "--prune"]),
            GitStageAll => self.git_job("add", &["add", "-A"]),
            GitCommit => {
                self.show_sidebar = true;
                self.side_tab = SideTab::Git;
                self.git.focus_commit = true;
                self.git_refresh();
            }
            GitCheckout => {
                if let Some(repo) = self.repo.clone() {
                    self.jobs.spawn(move || JobResult::GitBranches(git::branches(&repo)));
                }
            }
            GitNewBranch => self.open_prompt(Prompt::NewBranch),
            GitDiffFile => {
                let path = self.docs.get(self.active).and_then(|d| d.path.clone());
                match path.as_deref().and_then(|p| Some((p, self.repo_for(p)?))) {
                    Some((p, repo)) => {
                        let rel = p.strip_prefix(&repo).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
                        self.select_repo(repo);
                        self.show_diff(rel, false);
                    }
                    None => self.set_status("Fichier hors d'un dépôt git"),
                }
            }
            GitHistory => {
                let split_shown = self.split.as_mut().map(|sp| std::mem::replace(&mut sp.shown, false)).unwrap_or(false);
                if self.history.open && !split_shown {
                    self.history.open = false;
                } else {
                    self.history.open = true;
                    self.prefs.open = false;
                    if self.history.graph.is_none() || self.history.repo != self.repo {
                        self.load_history();
                    }
                }
            }
            Plugin(k) => self.run_plugin(k),
            _ => {}
        }
    }

    // ------------------------------------------------------------------ find

    fn update_find(&mut self) {
        if !self.find.open {
            if self.find.in_sel {
                self.find.in_sel = false;
                self.docs.iter_mut().for_each(|d| d.find_scope = None);
            }
            return;
        }
        let Some(doc) = self.docs.get(self.active) else { return };
        let f = &self.find;
        let scope = if f.in_sel { doc.find_scope } else { None };
        let key = (doc.uid, doc.version, f.query.clone(), f.case, f.regex, f.whole, scope);
        if self.find.key.as_ref() == Some(&key) {
            return;
        }
        if f.query.is_empty() {
            self.find.matches.clear();
            self.find.error = None;
        } else {
            match document::build_regex(&f.query, f.case, f.regex, f.whole) {
                Ok(re) => {
                    let mut m = doc.find_all(&re);
                    if let Some((s, e)) = scope {
                        m.retain(|&(a, b)| a >= s && b <= e);
                    }
                    self.find.matches = m;
                    self.find.error = None;
                }
                Err(e) => {
                    self.find.matches.clear();
                    self.find.error = Some(e);
                }
            }
        }
        self.find.key = Some(key);
    }

    fn find_step(&mut self, forward: bool, from_start: bool) {
        self.find.open = true;
        self.update_find();
        let m = &self.find.matches;
        if m.is_empty() {
            return;
        }
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        let p = doc.primary_sel();
        let idx = if from_start {
            m.iter().position(|&(a, _)| a >= p.start()).unwrap_or(0)
        } else if forward {
            m.iter().position(|&(a, _)| a >= p.end() && (a, a) != (p.start(), p.end())).unwrap_or(0)
        } else {
            m.iter().rposition(|&(a, _)| a < p.start()).unwrap_or(m.len() - 1)
        };
        let (a, b) = m[idx];
        doc.sels = vec![Sel::range(a, b)];
        doc.primary = 0;
        doc.scroll_to_cursor = true;
        doc.center_cursor = true;
    }

    fn find_select_all(&mut self, ctx: &egui::Context) {
        self.update_find();
        if self.find.matches.is_empty() {
            return;
        }
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.sels = self.find.matches.iter().map(|&(a, b)| Sel::range(a, b)).collect();
            doc.primary = 0;
            doc.normalize();
            doc.scroll_to_cursor = true;
        }
        self.find.open = false;
        self.focus_editor(ctx);
    }

    fn replace_one(&mut self) {
        self.update_find();
        let f = &self.find;
        let Ok(re) = document::build_regex(&f.query, f.case, f.regex, f.whole) else { return };
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        if doc.read_only {
            return;
        }
        let p = doc.primary_sel();
        if f.matches.iter().any(|&(a, b)| a == p.start() && b == p.end()) {
            let edits = document::replacements(&doc.rope, &re, &f.replacement, f.regex, f.preserve, Some((p.start(), p.end())));
            if let Some((a, b, new)) = edits.into_iter().next() {
                let delta = new.chars().count() as isize - (b - a) as isize;
                doc.replace_range(a, b, &new);
                if let Some((s, e)) = doc.find_scope.as_mut() {
                    *e = (*e as isize + delta).max(*s as isize) as usize;
                }
                let end = doc.primary_sel().end();
                doc.sels = vec![Sel::cursor(end)];
                doc.primary = 0;
            }
        }
        self.find_step(true, true);
    }

    fn replace_all(&mut self) {
        let f = &self.find;
        let Ok(re) = document::build_regex(&f.query, f.case, f.regex, f.whole) else { return };
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        if doc.read_only || f.query.is_empty() {
            return;
        }
        let scope = if f.in_sel { doc.find_scope } else { None };
        let edits = document::replacements(&doc.rope, &re, &f.replacement, f.regex, f.preserve, scope);
        let n = edits.len();
        if n == 0 {
            return;
        }
        let delta: isize = edits.iter().map(|(a, b, t)| t.chars().count() as isize - (b - a) as isize).sum();
        doc.replace_keep(EditKind::Other, edits);
        if let Some((s, e)) = doc.find_scope.as_mut() {
            *e = (*e as isize + delta).max(*s as isize) as usize;
        }
        let where_ = if scope.is_some() { " dans la sélection" } else { "" };
        self.set_status(format!("{n} remplacement(s){where_}"));
    }

    // ------------------------------------------------------------------ overlay (palette, quick open, prompts)

    fn open_overlay(&mut self, mode: Mode) {
        let items: Vec<Item> = match &mode {
            Mode::Commands => {
                let mut v: Vec<Item> = commands::ALL
                    .iter()
                    .filter(|(_, l, _)| !l.is_empty())
                    .map(|(_, l, c)| Item { label: l.to_string(), detail: self.keymap.label(*c), action: Action::Cmd(*c) })
                    .collect();
                for (k, &(pi, ci)) in self.plugin_cmds.iter().enumerate() {
                    let p = &self.plugins[pi];
                    v.push(Item {
                        label: format!("{} : {}", p.name, p.commands[ci].title),
                        detail: self.keymap.label(Cmd::Plugin(k)),
                        action: Action::Cmd(Cmd::Plugin(k)),
                    });
                }
                v
            }
            Mode::Files => self.file_items(),
            Mode::Projects => self
                .projects
                .iter()
                .filter(|p| p.path.is_dir())
                .map(|p| Item { label: p.name.clone(), detail: p.path.display().to_string(), action: Action::Project(p.path.clone()) })
                .collect(),
            Mode::Syntax => self
                .hl
                .syntax_names()
                .into_iter()
                .map(|n| Item { label: n.clone(), detail: String::new(), action: Action::Syntax(n) })
                .collect(),
            Mode::Symbols => match self.docs.get_mut(self.active) {
                Some(d) => {
                    let syms = d.symbols().to_vec();
                    syms.iter()
                        .map(|sy| {
                            let parents: Vec<&str> =
                                symbols::path_at(&syms, sy.line).iter().filter(|p| p.line != sy.line).map(|p| p.name.as_str()).collect();
                            let mut detail = parents.join(" › ");
                            if !sy.detail.is_empty() {
                                detail = if detail.is_empty() { sy.detail.clone() } else { format!("{detail} · {}", sy.detail) };
                            }
                            let detail = if detail.is_empty() { format!("ligne {}", sy.line + 1) } else { format!("{detail} · ligne {}", sy.line + 1) };
                            Item { label: format!("{}  {}", sy.kind.icon(), sy.name), detail, action: Action::Line(sy.line) }
                        })
                        .collect()
                }
                None => Vec::new(),
            },
            Mode::Templates => {
                self.templates = crate::templates::load(&crate::templates::dir());
                let ext = self
                    .docs
                    .get(self.active)
                    .and_then(|d| d.path.as_deref())
                    .and_then(|p| p.extension())
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                // the templates for the current kind of file first
                let mut order: Vec<usize> = (0..self.templates.len()).collect();
                order.sort_by_key(|&i| self.templates[i].ext != ext);
                order
                    .into_iter()
                    .map(|i| {
                        let t = &self.templates[i];
                        let detail = match (&t.path, t.ext.is_empty()) {
                            (Some(_), _) => format!(".{} · perso", t.ext),
                            (None, false) => format!(".{}", t.ext),
                            (None, true) => String::new(),
                        };
                        Item { label: t.name.clone(), detail, action: Action::Template(i) }
                    })
                    .collect()
            }
            Mode::Themes => self
                .hl
                .theme_names()
                .into_iter()
                .map(|n| Item { label: n.clone(), detail: String::new(), action: Action::Theme(n) })
                .collect(),
            _ => Vec::new(),
        };
        self.open_overlay_with(mode, items, String::new());
    }

    fn file_items(&self) -> Vec<Item> {
        if self.root.is_none() {
            return Vec::new();
        }
        self.file_index
            .iter()
            .enumerate()
            .map(|(i, p)| Item { label: p.to_string_lossy().into_owned(), detail: String::new(), action: Action::OpenIndexed(i) })
            .collect()
    }

    fn open_overlay_with(&mut self, mode: Mode, items: Vec<Item>, query: String) {
        self.overlay = Some(Overlay { mode, query, selected: 0, focus: true, items, filtered: Vec::new(), last_query: None, matched: None, paths: None });
    }

    fn open_prompt(&mut self, p: Prompt) {
        let root = self.root.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let with_sep = |p: &Path| format!("{}{}", p.display(), std::path::MAIN_SEPARATOR);
        let query = match &p {
            Prompt::OpenFolder => with_sep(&root),
            Prompt::OpenFile => with_sep(&root),
            Prompt::SaveAs => match self.docs.get(self.active).and_then(|d| d.path.clone()) {
                Some(path) => path.display().to_string(),
                None => with_sep(&root),
            },
            Prompt::Rename(path) => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            _ => String::new(),
        };
        self.open_overlay_with(Mode::Prompt(p), Vec::new(), query);
    }

    fn overlay_ui(&mut self, ctx: &egui::Context) {
        let Some(mut ov) = self.overlay.take() else { return };
        let title = match &ov.mode {
            Mode::Commands => "Palette de commandes".to_string(),
            Mode::Files => "Aller au fichier   (fichier:ligne, ou :ligne dans le fichier courant)".into(),
            Mode::GotoLine if self.goto_select => {
                "Sélectionner la ligne   (42, ou 10-20 pour plusieurs lignes ; vide + Entrée : la ligne courante)".into()
            }
            Mode::GotoLine => "Aller à la ligne   (ligne ou ligne:colonne)".into(),
            Mode::Syntax => "Syntaxe du fichier".into(),
            Mode::Themes => "Thème".into(),
            Mode::Templates => "Insérer un modèle   (${model}, ${Model}, ${module} remplis d'après le fichier courant)".into(),
            Mode::Branches => "Changer de branche".into(),
            Mode::Projects => "Changer de projet".into(),
            Mode::Symbols => "Aller au symbole   (classe, fonction, méthode, record Odoo, titre…)".into(),
            Mode::Definitions(w) => format!("Définitions de « {w} »"),
            Mode::Prompt(p) => match p {
                Prompt::OpenFolder => "Ouvrir le dossier".into(),
                Prompt::OpenFile => "Ouvrir le fichier".into(),
                Prompt::SaveAs => "Enregistrer sous".into(),
                Prompt::NewFile(d) => format!("Nouveau fichier dans {}", d.display()),
                Prompt::NewFolder(d) => format!("Nouveau dossier dans {}", d.display()),
                Prompt::Rename(p) => format!("Renommer {}", p.display()),
                Prompt::NewBranch => "Nom de la nouvelle branche".into(),
                Prompt::RenameSymbol => "Nouveau nom (renommé partout dans le projet)".into(),
                Prompt::RenameProject(_) => "Nom du projet".into(),
                Prompt::BranchAt(h) => format!("Nouvelle branche sur le commit {}", &h[..h.len().min(8)]),
            },
        };
        let is_list = !matches!(ov.mode, Mode::Prompt(_) | Mode::GotoLine);
        // prompts asking for a path complete it with Tab, like a shell
        let path_base = match &ov.mode {
            Mode::Prompt(Prompt::OpenFolder | Prompt::OpenFile | Prompt::SaveAs) => {
                Some((self.root.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default()), matches!(ov.mode, Mode::Prompt(Prompt::OpenFolder))))
            }
            Mode::Prompt(Prompt::NewFile(d) | Prompt::NewFolder(d)) => Some((d.clone(), matches!(ov.mode, Mode::Prompt(Prompt::NewFolder(_))))),
            _ => None,
        };
        let mut cursor_to_end = false;
        // the same prompts can go through the system's file chooser
        let dialog_kind = match &ov.mode {
            Mode::Prompt(Prompt::OpenFolder) => Some(0),
            Mode::Prompt(Prompt::OpenFile) => Some(1),
            Mode::Prompt(Prompt::SaveAs) => Some(2),
            _ => None,
        };
        let mut browse = false;
        if let Some((base, dirs_only)) = &path_base {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab)) {
                let (done, _) = complete_path(&ov.query, base, *dirs_only);
                ov.query = done;
                cursor_to_end = true;
            }
            if ov.paths.as_ref().map(|(q, _)| q) != Some(&ov.query) {
                let (_, found) = complete_path(&ov.query, base, *dirs_only);
                ov.paths = Some((ov.query.clone(), found));
            }
        }
        let files_mode = matches!(ov.mode, Mode::Files);
        let mut chosen: Option<Action> = None;
        let mut submit = false;
        let mut goto_only: Option<String> = None;
        let indexing = files_mode && self.file_index.is_empty() && self.root.is_some();
        let area = egui::Area::new(egui::Id::new("overlay"))
            .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 40.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(12.0)).rounding(14.0).show(ui, |ui| {
                    ui.set_width(640.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&title).small().weak());
                        if dialog_kind.is_some() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                browse = ui.small_button("📂 Parcourir…").on_hover_text("Choisir dans le gestionnaire de fichiers").clicked();
                            });
                        }
                    });
                    ui.add_space(2.0);
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut ov.query)
                            .desired_width(f32::INFINITY)
                            .frame(false)
                            .hint_text("Tapez pour filtrer…")
                            .font(egui::FontId::proportional(17.0))
                            // Tab completes the path: it must not also move the focus to the next widget
                            .lock_focus(path_base.is_some()),
                    );
                    ui.add_space(4.0);
                    ui.separator();
                    if ov.focus {
                        te.request_focus();
                        ov.focus = false;
                        // the text field keeps its cursor from the previous prompt: put it at the end,
                        // or select the name (without extension) of the file to save
                        {
                            let mut st = egui::TextEdit::load_state(ui.ctx(), te.id).unwrap_or_default();
                            let chars: Vec<char> = ov.query.chars().collect();
                            let end = chars.len();
                            let name_start = chars.iter().rposition(|c| matches!(c, '/' | '\\')).map_or(0, |i| i + 1);
                            let range = match chars[name_start..].iter().rposition(|c| *c == '.') {
                                Some(dot) if matches!(ov.mode, Mode::Prompt(Prompt::SaveAs)) && dot > 0 => egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(name_start),
                                    egui::text::CCursor::new(name_start + dot),
                                ),
                                _ => egui::text::CCursorRange::one(egui::text::CCursor::new(end)),
                            };
                            st.cursor.set_char_range(Some(range));
                            st.store(ui.ctx(), te.id);
                        }
                    }
                    if cursor_to_end {
                        if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), te.id) {
                            let end = egui::text::CCursor::new(ov.query.chars().count());
                            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                            st.store(ui.ctx(), te.id);
                        }
                        te.request_focus();
                    }
                    let (down, up, enter) =
                        ui.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::Enter)));
                    if let Some((_, found)) = ov.paths.as_ref().filter(|_| path_base.is_some()) {
                        let cut = ov.query.rfind(['/', '\\']).map_or(0, |i| i + 1);
                        let mut fill = None;
                        if found.is_empty() {
                            ui.label(RichText::new("Tab : compléter le chemin").small().weak());
                        } else {
                            ui.label(RichText::new(format!("Tab : compléter · {} entrée(s)", found.len())).small().weak());
                            egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                                for (name, is_dir) in found.iter().take(200) {
                                    let label = if *is_dir { format!("{name}{}", std::path::MAIN_SEPARATOR) } else { name.clone() };
                                    let text = RichText::new(label.clone()).monospace();
                                    let text = if *is_dir { text.color(ui.visuals().hyperlink_color) } else { text };
                                    if ui.add(egui::Label::new(text).sense(Sense::click())).clicked() {
                                        fill = Some(format!("{}{label}", &ov.query[..cut]));
                                    }
                                }
                            });
                        }
                        if let Some(f) = fill {
                            ov.query = f;
                            ov.focus = true;
                            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), te.id) {
                                let end = egui::text::CCursor::new(ov.query.chars().count());
                                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                                st.store(ui.ctx(), te.id);
                            }
                        }
                    }
                    if !is_list {
                        submit = enter;
                        return;
                    }
                    if matches!(ov.mode, Mode::Files) && ov.query.starts_with(':') {
                        ui.label(RichText::new("Entrée : aller à cette ligne du fichier courant").weak());
                        if enter {
                            goto_only = Some(ov.query[1..].to_string());
                        }
                        return;
                    }
                    if ov.last_query.as_ref() != Some(&ov.query) {
                        ov.last_query = Some(ov.query.clone());
                        ov.selected = 0;
                        let q = ov.query.split(':').next().unwrap_or("").to_string();
                        // typing one more letter: only the items that matched before can match
                        let narrowing = ov.matched.as_ref().filter(|(prev, _)| !prev.is_empty() && q.starts_with(prev.as_str()));
                        let pool: Box<dyn Iterator<Item = usize>> = match narrowing {
                            Some((_, m)) => Box::new(m.clone().into_iter()),
                            None => Box::new(0..ov.items.len()),
                        };
                        let mut scored: Vec<(i32, usize)> =
                            pool.filter_map(|i| fuzzy::score(&q, &ov.items[i].label).map(|s| (s, i))).collect();
                        ov.matched = Some((q.clone(), scored.iter().map(|(_, i)| *i).collect()));
                        if !q.is_empty() && scored.len() > 300 {
                            // only the best 300 are shown: no need to sort everything
                            scored.select_nth_unstable_by(300, |a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                            scored.truncate(300);
                        }
                        if !q.is_empty() {
                            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                        }
                        ov.filtered = scored.into_iter().take(300).map(|(_, i)| i).collect();
                    }
                    let n = ov.filtered.len();
                    if n > 0 && down {
                        ov.selected = (ov.selected + 1) % n;
                    }
                    if n > 0 && up {
                        ov.selected = (ov.selected + n - 1) % n;
                    }
                    // same height whatever the number of results: the window does not jump while typing
                    let fixed = matches!(ov.mode, Mode::Files | Mode::Commands);
                    egui::ScrollArea::vertical()
                        .max_height(440.0)
                        .min_scrolled_height(if fixed { 440.0 } else { 0.0 })
                        .auto_shrink([false, !fixed])
                        .show(ui, |ui| {
                        for (row, &i) in ov.filtered.iter().enumerate() {
                            let it = &ov.items[i];
                            // full-width row: label on the left, shortcut / detail on the right
                            let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::click());
                            let v = ui.visuals();
                            if row == ov.selected {
                                ui.painter().rect_filled(rect, 7.0, v.selection.bg_fill);
                            } else if r.hovered() {
                                ui.painter().rect_filled(rect, 7.0, v.widgets.hovered.weak_bg_fill);
                            }
                            let text_color = if row == ov.selected { v.strong_text_color() } else { v.text_color() };
                            let detail_w = if it.detail.is_empty() {
                                0.0
                            } else {
                                let g = ui.painter().layout_no_wrap(it.detail.clone(), egui::FontId::proportional(12.0), v.weak_text_color());
                                let w = g.size().x;
                                ui.painter().galley(egui::pos2(rect.right() - w - 12.0, rect.center().y - g.size().y / 2.0), g, v.weak_text_color());
                                w + 24.0
                            };
                            let clip = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - detail_w, rect.bottom()));
                            let mut text_x = rect.left() + 12.0;
                            if files_mode {
                                let name = it.label.rsplit(['/', '\\']).next().unwrap_or(&it.label);
                                let bg = if row == ov.selected { v.selection.bg_fill } else { v.window_fill };
                                let light = bg.r() as u32 + bg.g() as u32 + bg.b() as u32 > 384;
                                let r = egui::Rect::from_center_size(egui::pos2(rect.left() + 20.0, rect.center().y), egui::vec2(16.0, 16.0));
                                icons::paint_file(ui.painter(), r, name, bg, light);
                                text_x += 22.0;
                            }
                            ui.painter().with_clip_rect(clip).text(
                                egui::pos2(text_x, rect.center().y),
                                Align2::LEFT_CENTER,
                                &it.label,
                                egui::FontId::proportional(14.0),
                                text_color,
                            );
                            if row == ov.selected && (up || down) {
                                r.scroll_to_me(None);
                            }
                            if r.clicked() {
                                chosen = Some(it.action.clone());
                            }
                        }
                        if n == 0 && indexing {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("Liste des fichiers du projet en cours…").weak());
                            });
                        } else if n == 0 {
                            ui.label(RichText::new("Aucun résultat").weak());
                        }
                    });
                    if enter {
                        if let Some(&i) = ov.filtered.get(ov.selected) {
                            chosen = Some(ov.items[i].action.clone());
                        }
                    }
                });
            });
        // A click elsewhere closes the overlay: otherwise it would stay open, holding the keyboard
        // (the editor ignores typing while an overlay is open).
        if let Some(kind) = dialog_kind.filter(|_| browse) {
            self.file_dialog(kind);
            return;
        }
        let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p)));
        if outside && chosen.is_none() && goto_only.is_none() && !submit {
            return;
        }

        if let Some(spec) = goto_only {
            self.goto_spec(&spec);
            self.focus_editor(ctx);
        } else if let Some(action) = chosen {
            let line_spec = match ov.mode {
                Mode::Files => ov.query.split_once(':').map(|(_, l)| l.to_string()),
                _ => None,
            };
            self.choose(ctx, action);
            if let Some(spec) = line_spec {
                self.goto_spec(&spec);
            }
            if self.overlay.is_none() {
                self.focus_editor(ctx);
            }
        } else if submit {
            match ov.mode {
                Mode::GotoLine if self.goto_select => {
                    if let Some(d) = self.docs.get_mut(self.active) {
                        let cur = d.rope.char_to_line(d.primary_sel().head);
                        match document::parse_line_range(&ov.query, d.rope.len_lines(), cur) {
                            Some((a, b)) => d.select_lines(a, b),
                            None => self.set_status("Numéro de ligne attendu : 42, ou 10-20"),
                        }
                    }
                }
                Mode::GotoLine => self.goto_spec(&ov.query),
                Mode::Prompt(p) => self.submit_prompt(p, ov.query.clone()),
                _ => {}
            }
            if self.overlay.is_none() {
                self.focus_editor(ctx);
            }
        } else {
            self.overlay = Some(ov);
        }
    }

    fn goto_spec(&mut self, spec: &str) {
        let mut it = spec.split(':').map(|s| s.trim().parse::<usize>().ok());
        let line = it.next().flatten();
        let col = it.next().flatten().unwrap_or(1);
        if let (Some(l), Some(d)) = (line, self.docs.get_mut(self.active)) {
            d.goto_line(l.saturating_sub(1), col.saturating_sub(1));
        }
    }

    fn choose(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Cmd(c) => self.exec(ctx, c),
            Action::OpenIndexed(i) => {
                if let (Some(root), Some(p)) = (self.root.clone(), self.file_index.get(i)) {
                    self.open_path(root.join(p));
                }
            }
            Action::Location(p, l) => self.open_at(p, l),
            Action::Line(l) => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    let ind = d.indent_len(l);
                    d.goto_line(l, ind);
                }
            }
            Action::Syntax(n) => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    d.syntax = n;
                }
            }
            Action::Theme(n) => {
                self.hl.set_theme(&n);
                apply_visuals(ctx, self.hl.visuals());
                self.settings.theme = n.clone();
                self.persist("theme", config::toml_str(&n));
                self.set_status(format!("Thème « {n} » enregistré"));
            }
            Action::Branch(b) => {
                let name = b.split_once('/').filter(|(r, _)| *r == "origin").map(|(_, n)| n.to_string()).unwrap_or(b);
                self.git_job("checkout", &["checkout", &name]);
            }
            Action::Project(p) => self.switch_project(p),
            Action::Template(k) => self.insert_template(k),
        }
    }

    /// Inserts template `k` at every cursor, indented like the line, with the cursor at its `$0`.
    fn insert_template(&mut self, k: usize) {
        let Some(t) = self.templates.get(k).cloned() else { return };
        if self.docs.get(self.active).map_or(true, |d| d.read_only || d.view != document::View::Text) {
            self.new_doc();
            let first = t.body.lines().next().unwrap_or("").to_string();
            let syntax = self.hl.detect(Some(Path::new(&format!("x.{}", t.ext))), &first);
            if let Some(d) = self.docs.get_mut(self.active) {
                d.syntax = syntax;
            }
        }
        let Some(d) = self.docs.get_mut(self.active) else { return };
        let vars = crate::templates::variables(d.path.as_deref());
        let items: Vec<Edit> = d
            .sels
            .clone()
            .iter()
            .map(|s| {
                let line = d.rope.char_to_line(s.start());
                let line_start = d.rope.line_to_char(line);
                let indent: String = d.line_text(line).chars().take(s.start() - line_start).take_while(|c| *c == ' ' || *c == '\t').collect();
                let (text, cursor) = crate::templates::expand(&t.body, &vars, &indent);
                Edit { start: s.start(), end: s.end(), text, anchor: cursor, head: cursor }
            })
            .collect();
        d.edit(EditKind::Other, items);
        d.scroll_to_cursor = true;
        self.set_status(format!("Modèle « {} » inséré", t.name));
    }

    fn submit_prompt(&mut self, p: Prompt, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        match p {
            Prompt::OpenFolder => {
                let path = self.resolve(PathBuf::from(&text));
                if path.is_dir() {
                    self.switch_project(path);
                } else {
                    self.set_status(format!("Dossier introuvable : {}", path.display()));
                }
            }
            Prompt::OpenFile => {
                let path = self.resolve(PathBuf::from(&text));
                self.open_path(path);
            }
            Prompt::SaveAs => {
                let path = self.resolve(PathBuf::from(&text));
                if let Some(d) = self.docs.get_mut(self.active) {
                    let first = d.line_text(0);
                    d.path = Some(path.clone());
                    d.title_override = None;
                    d.read_only = false;
                    d.syntax = self.hl.detect(Some(&path), &first);
                }
                if self.save(self.active) {
                    self.refresh_index();
                }
            }
            Prompt::NewFile(dir) => {
                let path = dir.join(&text);
                if !path.exists() {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(&path, "") {
                        self.set_status(e.to_string());
                        return;
                    }
                }
                self.open_path(path);
                self.exec_refresh_tree();
            }
            Prompt::NewFolder(dir) => {
                if let Err(e) = std::fs::create_dir_all(dir.join(&text)) {
                    self.set_status(e.to_string());
                }
                self.exec_refresh_tree();
            }
            Prompt::Rename(old) => {
                let new = old.with_file_name(&text);
                match std::fs::rename(&old, &new) {
                    Ok(()) => {
                        self.relocate_docs(&old, &new);
                        self.exec_refresh_tree();
                    }
                    Err(e) => self.set_status(e.to_string()),
                }
            }
            Prompt::NewBranch => self.git_job("checkout -b", &["checkout", "-b", &text]),
            Prompt::BranchAt(hash) => self.git_job("branch", &["branch", &text, &hash]),
            Prompt::RenameSymbol => {
                let ctx = self.egui_ctx.clone();
                let i = self.active;
                if let (Some((ci, _)), Some((doc, pos))) = (self.lsp_for(i, &ctx, true), self.lsp_text_position(i)) {
                    if let Some(c) = self.lsp.client(ci) {
                        let id = c.request("textDocument/rename", serde_json::json!({ "textDocument": doc, "position": pos, "newName": text }));
                        self.lsp_req.insert((ci, id), LspReq::Rename { name: text });
                    }
                }
            }
            Prompt::RenameProject(path) => {
                if let Some(p) = self.projects.iter_mut().find(|p| p.path == path) {
                    p.name = text;
                }
                project::save_list(&self.projects);
            }
        }
    }

    fn exec_refresh_tree(&mut self) {
        if let Some(t) = self.tree.as_mut() {
            t.refresh();
        }
        self.refresh_index();
        self.git_refresh();
    }

    fn confirm_ui(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        let (title, msg) = match c {
            Confirm::CloseTab(uid) => {
                let name = self.docs.iter().find(|d| d.uid == *uid).map(|d| d.title()).unwrap_or_default();
                ("Fichier modifié", format!("Enregistrer les modifications de « {name} » ?"))
            }
            Confirm::Quit => ("Quitter", "Des fichiers ne sont pas enregistrés.".to_string()),
            Confirm::Delete(p) => ("Supprimer", format!("Supprimer définitivement « {} » ?", p.display())),
            Confirm::SwitchProject(p) => {
                ("Changer de projet", format!("Des fichiers ne sont pas enregistrés. Ouvrir « {} » ?", p.display()))
            }
            Confirm::DiffOp { msg, .. } => ("Annuler le changement", msg.clone()),
            Confirm::Git { title, msg, .. } => (title.as_str(), msg.clone()),
            Confirm::ReplaceInFiles => {
                let n = self.search.hit_count();
                (
                    "Remplacer dans les fichiers",
                    format!(
                        "Remplacer {n} occurrence(s) dans {} fichier(s) ?\nLes fichiers non ouverts sont modifiés directement sur le disque.",
                        self.search.files.len()
                    ),
                )
            }
        };
        let mut choice = None;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(msg);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let (yes, no) = match c {
                        Confirm::CloseTab(_) => ("Enregistrer", Some("Ne pas enregistrer")),
                        Confirm::Quit => ("Tout enregistrer et quitter", Some("Quitter sans enregistrer")),
                        Confirm::Delete(_) => ("Supprimer", None),
                        Confirm::SwitchProject(_) => ("Tout enregistrer et changer", Some("Changer sans enregistrer")),
                        Confirm::ReplaceInFiles => ("Remplacer", None),
                        Confirm::DiffOp { .. } => ("Oui, annuler le changement", None),
                        Confirm::Git { .. } => ("Oui", None),
                    };
                    if ui.button(yes).clicked() {
                        choice = Some(1);
                    }
                    if let Some(no) = no {
                        if ui.button(no).clicked() {
                            choice = Some(2);
                        }
                    }
                    if ui.button("Annuler").clicked() {
                        choice = Some(0);
                    }
                });
            });
        let Some(choice) = choice else { return };
        let c = self.confirm.take().unwrap();
        match (c, choice) {
            (Confirm::DiffOp { label, op, .. }, 1) => self.run_diff_op(label, op),
            (Confirm::Git { label, args, .. }, 1) => {
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                self.git_job(&label, &a);
            }
            (Confirm::CloseTab(uid), 1 | 2) => {
                if let Some(i) = self.docs.iter().position(|d| d.uid == uid) {
                    if choice == 2 || self.save(i) {
                        self.close_tab(i, true);
                    }
                }
            }
            (Confirm::Quit, 1) => {
                for i in 0..self.docs.len() {
                    if self.docs[i].is_dirty() && !self.save(i) {
                        return;
                    }
                }
                self.quit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            (Confirm::Quit, 2) => {
                self.quit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            (Confirm::SwitchProject(p), 1) => {
                for i in 0..self.docs.len() {
                    if self.docs[i].is_dirty() && !self.save(i) {
                        return;
                    }
                }
                self.do_switch(p);
            }
            (Confirm::SwitchProject(p), 2) => self.do_switch(p),
            (Confirm::ReplaceInFiles, 1) => self.replace_in_files(None),
            (Confirm::Delete(p), 1) => {
                let r = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                match r {
                    Ok(()) => {
                        self.set_status(format!("Supprimé : {}", p.display()));
                        self.exec_refresh_tree();
                    }
                    Err(e) => self.set_status(e.to_string()),
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ panels

    fn menu_bar(&mut self, ctx: &egui::Context) {
        if !self.settings.show_menu_bar {
            return;
        }
        let mut pending = Vec::new();
        let project = self.root.as_ref().and_then(|r| r.file_name()).map(|n| n.to_string_lossy().into_owned());
        let panel = ctx.style().visuals.panel_fill;
        egui::TopBottomPanel::top("menu")
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin::symmetric(10.0, 5.0)))
            .show_separator_line(false)
            .show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
                logo::paint(ui.painter(), r, false);
                ui.add_space(2.0);
                self.menus(ui, &mut pending);
                // project name, right-aligned, after the menus
                if let Some(p) = &project {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(p).weak());
                    });
                }
            });
        });
        self.pending.extend(pending);
    }

    /// The menus (in the menu bar, or under ☰ in the status bar when the bar is hidden).
    fn menus(&self, ui: &mut egui::Ui, pending: &mut Vec<Cmd>) {
        let km = &self.keymap;
        let plugins = &self.plugins;
        let plugin_cmds = &self.plugin_cmds;
        let mut item = |ui: &mut egui::Ui, label: &str, cmd: Cmd| {
            if ui.add(egui::Button::new(label).shortcut_text(km.label(cmd))).clicked() {
                pending.push(cmd);
                ui.close_menu();
            }
        };
        ui.menu_button("Fichier", |ui| {
            item(ui, "Nouveau fichier", Cmd::NewFile);
            item(ui, "Ouvrir un fichier…", Cmd::OpenFile);
            item(ui, "Ouvrir un dossier…", Cmd::OpenFolder);
            item(ui, "Changer de projet…", Cmd::SwitchProject);
            item(ui, "Fermer le dossier", Cmd::CloseFolder);
            item(ui, "Rouvrir l'onglet fermé", Cmd::ReopenClosed);
            ui.separator();
            item(ui, "Enregistrer", Cmd::Save);
            item(ui, "Enregistrer sous…", Cmd::SaveAs);
            item(ui, "Tout enregistrer", Cmd::SaveAll);
            ui.separator();
            item(ui, "Fermer l'onglet", Cmd::CloseTab);
            item(ui, "Préférences", Cmd::OpenSettings);
            item(ui, "Ouvrir settings.toml", Cmd::OpenSettingsFile);
            item(ui, "Quitter", Cmd::Quit);
        });
        ui.menu_button("Édition", |ui| {
            item(ui, "Annuler", Cmd::Undo);
            item(ui, "Rétablir", Cmd::Redo);
            ui.separator();
            item(ui, "Dupliquer la ligne", Cmd::DuplicateLine);
            item(ui, "Supprimer la ligne", Cmd::DeleteLine);
            item(ui, "Monter la ligne", Cmd::MoveLineUp);
            item(ui, "Descendre la ligne", Cmd::MoveLineDown);
            item(ui, "Joindre les lignes", Cmd::JoinLines);
            item(ui, "Commenter / décommenter", Cmd::ToggleComment);
            item(ui, "Indenter", Cmd::Indent);
            item(ui, "Désindenter", Cmd::Unindent);
            item(ui, "Trier les lignes", Cmd::SortLines);
            item(ui, "MAJUSCULES", Cmd::UpperCase);
            item(ui, "minuscules", Cmd::LowerCase);
        });
        ui.menu_button("Sélection", |ui| {
            item(ui, "Tout sélectionner", Cmd::SelectAll);
            item(ui, "Sélectionner la ligne", Cmd::SelectLine);
            item(ui, "Ajouter l'occurrence suivante", Cmd::SelectNext);
            item(ui, "Toutes les occurrences", Cmd::SelectAllOccurrences);
            item(ui, "Diviser en lignes", Cmd::SplitLines);
            item(ui, "Curseur au-dessus", Cmd::AddCursorAbove);
            item(ui, "Curseur en dessous", Cmd::AddCursorBelow);
            item(ui, "Crochet correspondant", Cmd::GotoBracket);
        });
        ui.menu_button("Rechercher", |ui| {
            item(ui, "Rechercher…", Cmd::Find);
            item(ui, "Remplacer…", Cmd::Replace);
            item(ui, "Suivant", Cmd::FindNext);
            item(ui, "Rechercher dans le dossier…", Cmd::FindInFiles);
            item(ui, "Résultat global suivant", Cmd::NextResult);
            item(ui, "Précédent", Cmd::FindPrev);
            ui.separator();
            item(ui, "Aller au fichier…", Cmd::QuickOpen);
            item(ui, "Aller à la ligne…", Cmd::GotoLine);
        });
        ui.menu_button("Affichage", |ui| {
            item(ui, "Palette de commandes", Cmd::CommandPalette);
            item(ui, "Barre latérale", Cmd::ToggleSidebar);
            item(ui, "Terminal", Cmd::ToggleTerminal);
            item(ui, "Problèmes", Cmd::ShowProblems);
            item(ui, "Sortie", Cmd::ShowOutput);
            item(ui, "Minimap", Cmd::ToggleMinimap);
            item(ui, "Barre de menus", Cmd::ToggleMenuBar);
            ui.separator();
            item(ui, "Syntaxe…", Cmd::SetSyntax);
            item(ui, "Thème…", Cmd::SelectTheme);
            item(ui, "Zoom avant", Cmd::ZoomIn);
            item(ui, "Zoom arrière", Cmd::ZoomOut);
        });
        ui.menu_button("Git", |ui| {
            item(ui, "Panneau Git", Cmd::ShowGit);
            item(ui, "Commit…", Cmd::GitCommit);
            item(ui, "Pull", Cmd::GitPull);
            item(ui, "Push", Cmd::GitPush);
            item(ui, "Fetch", Cmd::GitFetch);
            ui.separator();
            item(ui, "Changer de branche…", Cmd::GitCheckout);
            item(ui, "Nouvelle branche…", Cmd::GitNewBranch);
            item(ui, "Diff du fichier", Cmd::GitDiffFile);
            item(ui, "Historique (graphe)", Cmd::GitHistory);
            item(ui, "Diff dans l'éditeur (vert / rouge)", Cmd::ToggleInlineDiff);
            item(ui, "Blame de la ligne", Cmd::GitBlame);
            item(ui, "Historique de la ligne", Cmd::GitLineHistory);
        });
        ui.menu_button("Outils", |ui| {
            item(ui, "Linter le fichier", Cmd::Lint);
            ui.separator();
            for (k, &(pi, ci)) in plugin_cmds.iter().enumerate() {
                item(ui, &plugins[pi].commands[ci].title, Cmd::Plugin(k));
            }
            if !plugin_cmds.is_empty() {
                ui.separator();
            }
            item(ui, "Insérer un modèle (vue Odoo, menu…)…", Cmd::InsertTemplate);
            item(ui, "Dossier des modèles", Cmd::OpenTemplatesFolder);
            ui.separator();
            item(ui, "Recharger les plugins", Cmd::ReloadPlugins);
            item(ui, "Dossier des plugins", Cmd::OpenPluginsFolder);
        });
        ui.menu_button("Aide", |ui| {
            item(ui, "Guide de Florm (recherche, Git, terminal, modèles…)", Cmd::ShowGuide);
            item(ui, "Palette de commandes", Cmd::CommandPalette);
            item(ui, "Raccourcis et préférences", Cmd::OpenSettings);
        });
    }

    /// Tab strip drawn above the editor area only (not above the sidebar).
    /// Tab strip: every tab (`pane == None`, with the history / split / settings tabs) or those of one panel.
    fn tabs_strip(&mut self, ui: &mut egui::Ui, pane: Option<u8>) {
        let tc = self.hl.tab_colors();
        let (mut activate, mut close, mut close_history, mut close_prefs) = (None, None, false, false);
        let (mut show_history, mut show_split, mut drag_start, mut menu_act) = (false, false, None, None);
        let split_on = self.split.as_ref().is_some_and(|s| s.shown) && !self.prefs.open;
        let special = pane.is_none() && (self.history.open || self.prefs.open || split_on);
        let is_split = self.is_split();
        let mut counts = [0usize; 2];
        for d in &self.docs {
            counts[(d.pane as usize).min(1)] += 1;
        }
        let mut scrolled = None;
        let mut strip = (egui::Rect::NOTHING, Vec::new());
        let panel_id = egui::Id::new("tabs").with(pane);
        egui::TopBottomPanel::top(panel_id).exact_height(TAB_STRIP_H).frame(egui::Frame::none().fill(tc.bar)).show_inside(ui, |ui| {
            let bar = ui.max_rect();
            strip.0 = bar;
            ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], egui::Stroke::new(1.0_f32, tc.border.gamma_multiply(0.35)));
            egui::ScrollArea::horizontal()
                .id_salt(panel_id.with("scroll"))
                .drag_to_scroll(false)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.add_space(6.0);
                        for (i, d) in self.docs.iter().enumerate() {
                            if pane.is_some_and(|p| p != d.pane) {
                                continue;
                            }
                            let shown = match pane {
                                Some(p) if p != self.focused_pane => self.pane_active[p as usize] == Some(d.uid),
                                _ => i == self.active,
                            };
                            let active = shown && !special;
                            let error = d.diagnostics.iter().any(|g| g.severity == Severity::Error);
                            let icon_name = match &d.path {
                                Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()),
                                None if d.syntax == "Diff" => Some("x.diff".to_string()),
                                None => Some(String::new()),
                            };
                            let (body, closed) = tab_widget(ui, &d.title(), icon_name.as_deref(), d.is_dirty(), active, error, &tc);
                            strip.1.push((body.rect, i));
                            if body.drag_started() {
                                drag_start = Some(i);
                            }
                            if closed || body.middle_clicked() {
                                close = Some(i);
                            } else if body.clicked() {
                                activate = Some(i);
                            }
                            if i == self.active && self.tab_scrolled != Some(i) {
                                body.scroll_to_me(None);
                                scrolled = Some(i);
                            }
                            let alone = counts[(d.pane as usize).min(1)] == 1;
                            body.context_menu(|ui| tab_menu(ui, i, d, is_split, alone, &mut menu_act));
                            if let Some(p) = &d.path {
                                body.on_hover_text(p.display().to_string());
                            }
                        }
                        if pane.is_some() {
                            return;
                        }
                        if self.history.open {
                            let on = !split_on && !self.prefs.open;
                            let (body, closed) = tab_widget(ui, "🔀 Historique Git", None, false, on, false, &tc);
                            close_history = closed;
                            show_history = body.clicked() && !closed;
                        }
                        if let Some(sp) = &self.split {
                            let title = format!("✂ Découpage {}", sp.info.short);
                            let (body, closed) = tab_widget(ui, &title, None, false, split_on, false, &tc);
                            // the split stays in progress: × only switches away from it
                            if closed {
                                activate = activate.or(Some(self.active));
                            } else if body.clicked() {
                                show_split = true;
                            }
                            body.on_hover_text("Découpage en cours — « Terminer » ou « Annuler le découpage » dans l'onglet");
                        }
                        if self.prefs.open {
                            let (_, closed) = tab_widget(ui, "⚙ Préférences", None, false, true, false, &tc);
                            close_prefs = closed;
                        }
                    });
                });
        });
        self.drop_zones.strips.push((pane.unwrap_or(0), strip.0, strip.1));
        if scrolled.is_some() {
            self.tab_scrolled = scrolled;
        }
        if let Some(i) = drag_start {
            self.tab_drag = Some(i);
        }
        if let Some(i) = activate {
            self.activate_tab(i);
        }
        if show_history {
            self.prefs.open = false;
            if let Some(sp) = self.split.as_mut() {
                sp.shown = false;
            }
        }
        if show_split {
            self.prefs.open = false;
            if let Some(sp) = self.split.as_mut() {
                sp.shown = true;
            }
        }
        if let Some(i) = close {
            self.close_tab(i, false);
        }
        if close_history {
            self.history.open = false;
        }
        if close_prefs {
            self.prefs.open = false;
        }
        match menu_act {
            Some(TabAct::Split(i, dir)) => self.split_tab(i, Some(dir)),
            Some(TabAct::MoveOther(i)) => self.split_tab(i, None),
            Some(TabAct::Unsplit) => self.unsplit(),
            Some(TabAct::Flip) => {
                self.split_dir = if self.split_dir == SplitDir::Right { SplitDir::Down } else { SplitDir::Right };
            }
            Some(TabAct::Close(i)) => self.close_tab(i, false),
            Some(TabAct::CloseOthers(i)) => {
                let (uid, p) = (self.docs[i].uid, self.docs[i].pane);
                let before = self.docs.len();
                for j in (0..self.docs.len()).rev() {
                    let d = &self.docs[j];
                    if d.uid != uid && d.pane == p && !d.is_dirty() {
                        self.close_tab(j, true);
                    }
                }
                self.active = self.docs.iter().position(|d| d.uid == uid).unwrap_or(0);
                self.tab_scrolled = None;
                let kept = self.docs.iter().filter(|d| d.pane == p && d.uid != uid).count();
                let closed = before - self.docs.len();
                self.set_status(if kept > 0 {
                    format!("{closed} onglet(s) fermé(s), {kept} non enregistré(s) gardé(s)")
                } else {
                    format!("{closed} onglet(s) fermé(s)")
                });
            }
            Some(TabAct::CopyPath(t)) => {
                ui.ctx().output_mut(|o| o.copied_text = t.clone());
                self.set_status(format!("Copié : {t}"));
            }
            Some(TabAct::Reveal(p)) => self.reveal(&p),
            Some(TabAct::Locate(i)) => {
                self.activate_tab(i);
                if let Some(p) = self.docs.get(i).and_then(|d| d.path.clone()) {
                    self.reveal_in_tree(&p);
                }
            }
            None => {}
        }
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        let mut toggle_spaces = false;
        let panel = ctx.style().visuals.panel_fill;
        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin::symmetric(12.0, 0.0)))
            .show(ctx, |ui| {
            ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(12.5));
            ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(12.5));
            ui.visuals_mut().override_text_color = Some(ui.visuals().weak_text_color());
            ui.horizontal_centered(|ui| {
                if !self.settings.show_menu_bar {
                    let mut pending = Vec::new();
                    ui.menu_button("☰", |ui| {
                        ui.visuals_mut().override_text_color = None;
                        self.menus(ui, &mut pending);
                        ui.separator();
                        if ui.button("Afficher la barre de menus").clicked() {
                            pending.push(Cmd::ToggleMenuBar);
                            ui.close_menu();
                        }
                    })
                    .response
                    .on_hover_text("Menus (barre de menus masquée)");
                    self.pending.extend(pending);
                }
                if let Some(d) = self.docs.get(self.active) {
                    let p = d.primary_sel();
                    let (l, c) = d.line_col(p.head);
                    ui.label(format!("Ligne {}, Colonne {}", l + 1, c + 1));
                    if d.sels.len() > 1 {
                        ui.label(format!("· {} sélections", d.sels.len()));
                    } else if !p.is_empty() {
                        ui.label(format!("· {} caractères sélectionnés", p.end() - p.start()));
                    }
                    if let Some(g) = d.diagnostics.iter().find(|g| g.line == l) {
                        let col = if g.severity == Severity::Error { err() } else { warn() };
                        ui.label(RichText::new(&g.message).color(col));
                    }
                }
                if let Some((msg, t)) = &self.status {
                    if t.elapsed() < Duration::from_secs(6) {
                        ui.label(RichText::new(msg).weak());
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(d) = self.docs.get(self.active) {
                        if ui.small_button(&d.syntax).clicked() {
                            self.pending.push(Cmd::SetSyntax);
                        }
                        ui.label(if d.crlf { "CRLF" } else { "LF" });
                        let ind = if d.use_spaces {
                            format!("Espaces : {}", d.tab_size)
                        } else {
                            format!("Tabulations : {}", d.tab_size)
                        };
                        if ui.small_button(ind).clicked() {
                            toggle_spaces = true;
                        }
                    }
                    let errs: usize = self
                        .docs
                        .iter()
                        .flat_map(|d| d.diagnostics.iter())
                        .filter(|g| g.severity == Severity::Error)
                        .count();
                    let warns: usize = self.docs.iter().map(|d| d.diagnostics.len()).sum::<usize>() - errs;
                    let txt = RichText::new(format!("✖ {errs}  ⚠ {warns}")).color(if errs > 0 {
                        err()
                    } else if warns > 0 {
                        warn()
                    } else {
                        ui.visuals().weak_text_color()
                    });
                    if ui.add(egui::Button::new(txt).frame(false)).clicked() {
                        self.pending.push(Cmd::ShowProblems);
                    }
                    if let Some(fam) = self.settings.lsp.then(|| self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(lsp::language_of)).flatten().map(|l| l.0) {
                        let running = self.lsp.clients.iter().flatten().find(|c| c.family == fam);
                        match (running, self.lsp_note.get(fam)) {
                            (Some(c), _) => {
                                let t = if c.ready { format!("LSP : {}", c.name) } else { format!("LSP : {} (démarrage…)", c.name) };
                                ui.label(t).on_hover_text("Complétion Ctrl+Espace · Définition F12 · Renommer F2");
                            }
                            (None, Some(why)) if fam == "python" => {
                                if ui
                                    .add(egui::Button::new(RichText::new("LSP : installer pylsp").color(warn())).frame(false))
                                    .on_hover_text(format!("{why}\nClic : installe python-lsp-server pour Florm (pip --target, sans toucher au Python système). Ou installe pyright."))
                                    .clicked()
                                {
                                    self.pending.push(Cmd::InstallPylsp);
                                }
                            }
                            (None, Some(why)) => {
                                ui.label(RichText::new("LSP : aucun serveur").color(warn())).on_hover_text(format!(
                                    "{why}\nInstalle typescript-language-server, rust-analyzer, gopls ou clangd selon le langage, ou indique la commande dans [lsp_servers] des préférences."
                                ));
                            }
                            _ => {}
                        }
                    }
                    let t = ui
                        .add(egui::Button::new(">_ Terminal").selected(self.bottom == Some(BottomTab::Terminal)))
                        .on_hover_text(format!("Terminal en bas ({}) · clic droit : nouveau terminal dans un onglet ({})", self.keymap.label(Cmd::ToggleTerminal), self.keymap.label(Cmd::NewTerminalTab)));
                    if t.clicked() {
                        self.pending.push(Cmd::ToggleTerminal);
                    }
                    if t.secondary_clicked() {
                        self.pending.push(Cmd::NewTerminalTab);
                    }
                    if let Some(st) = &self.git.status {
                        let label = match (&self.repo, self.repos.len() > 1) {
                            (Some(r), true) => format!("🔀 {} · {}", self.repo_label(r), st.branch),
                            _ => format!("🔀 {}", st.branch),
                        };
                        if ui.add(egui::Button::new(label).frame(false)).clicked() {
                            self.pending.push(Cmd::ShowGit);
                        }
                    }
                });
            });
        });
        if toggle_spaces {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.use_spaces = !d.use_spaces;
            }
        }
    }

    fn find_bar(&mut self, ctx: &egui::Context) {
        let (mut action, mut scope_toggled) = (None, false);
        egui::TopBottomPanel::bottom("find").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let f = &mut self.find;
                if ui.toggle_value(&mut f.case, "Aa").on_hover_text("Respecter la casse").changed()
                    | ui.toggle_value(&mut f.regex, ".*").on_hover_text("Expression régulière").changed()
                    | ui.toggle_value(&mut f.whole, "« »").on_hover_text("Mot entier").changed()
                {
                    action = Some(FindAction::Incremental);
                }
                if ui
                    .toggle_value(&mut f.in_sel, "Sél.")
                    .on_hover_text("Chercher et remplacer seulement dans la sélection actuelle (zone teintée)")
                    .changed()
                {
                    scope_toggled = true;
                }
                let w = (ui.available_width() - 330.0).max(120.0);
                let te = ui.add(
                    egui::TextEdit::singleline(&mut f.query)
                        .desired_width(w)
                        .hint_text("Rechercher (Entrée : suivant, Maj+Entrée : précédent, Alt+Entrée : tout sélectionner)")
                        .font(egui::TextStyle::Monospace),
                );
                if f.focus {
                    te.request_focus();
                    f.focus = false;
                }
                if te.has_focus() {
                    if let Some(t) = history_arrows(ui, &self.state.search_history, &mut self.search.hist_pos) {
                        f.query = t;
                        action = Some(FindAction::Incremental);
                    }
                }
                if te.changed() {
                    action = Some(FindAction::Incremental);
                }
                if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    let m = ui.input(|i| i.modifiers);
                    action = Some(if m.alt {
                        FindAction::All
                    } else if m.shift {
                        FindAction::Prev
                    } else {
                        FindAction::Next
                    });
                    if !m.alt {
                        te.request_focus();
                    }
                }
                if ui.button("◀").on_hover_text("Précédent").clicked() {
                    action = Some(FindAction::Prev);
                }
                if ui.button("▶").on_hover_text("Suivant").clicked() {
                    action = Some(FindAction::Next);
                }
                if ui.button("Tout").on_hover_text("Sélectionner tous les résultats (multi-curseur)").clicked() {
                    action = Some(FindAction::All);
                }
                match &f.error {
                    Some(e) => ui.label(RichText::new(e.lines().last().unwrap_or("")).color(err())),
                    None => ui.label(RichText::new(format!("{} résultat(s)", f.matches.len())).weak()),
                };
            });
            if self.find.replace_mode {
                ui.horizontal(|ui| {
                    ui.add_space(110.0);
                    let w = (ui.available_width() - 330.0).max(120.0);
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut self.find.replacement)
                            .desired_width(w)
                            .hint_text("Remplacer par ($1 pour les groupes en mode regex)")
                            .font(egui::TextStyle::Monospace),
                    );
                    if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        action = Some(FindAction::ReplaceOne);
                        te.request_focus();
                    }
                    if ui.button("Remplacer").clicked() {
                        action = Some(FindAction::ReplaceOne);
                    }
                    let all = if self.find.in_sel { "Tout remplacer dans la sélection" } else { "Tout remplacer" };
                    if ui.button(all).clicked() {
                        action = Some(FindAction::ReplaceAll);
                    }
                    ui.toggle_value(&mut self.find.preserve, "Garder la casse")
                        .on_hover_text("chat → chien, Chat → Chien, CHAT → CHIEN");
                });
            }
            ui.add_space(2.0);
        });
        if scope_toggled {
            let in_sel = self.find.in_sel;
            if let Some(d) = self.docs.get_mut(self.active) {
                let p = d.primary_sel();
                if !in_sel {
                    d.find_scope = None;
                } else if p.is_empty() {
                    self.find.in_sel = false;
                    self.set_status("Sélectionne d'abord le texte dans lequel chercher");
                } else {
                    d.find_scope = Some((p.start(), p.end()));
                }
            }
            action = Some(FindAction::Incremental);
        }
        if matches!(action, Some(FindAction::Next | FindAction::Prev | FindAction::All)) {
            UiState::remember(&mut self.state.search_history, &self.find.query);
            project::save_state(&self.state);
            self.search.hist_pos = None;
        }
        match action {
            Some(FindAction::Incremental) => self.find_step(true, true),
            Some(FindAction::Next) => self.find_step(true, false),
            Some(FindAction::Prev) => self.find_step(false, false),
            Some(FindAction::All) => self.find_select_all(ctx),
            Some(FindAction::ReplaceOne) => self.replace_one(),
            Some(FindAction::ReplaceAll) => self.replace_all(),
            None => {}
        }
    }

    /// Thin strip at the left edge while the sidebar is hidden: a click brings it back.
    fn sidebar_strip(&mut self, ctx: &egui::Context) {
        let panel = ctx.style().visuals.panel_fill;
        egui::SidePanel::left("sidebar_strip").exact_width(10.0).resizable(false).frame(egui::Frame::none().fill(panel)).show(ctx, |ui| {
            let rect = ui.max_rect();
            let r = ui.interact(rect, egui::Id::new("sidebar_strip_btn"), Sense::click());
            let c = if r.hovered() { self.hl.colors.accent } else { ui.visuals().weak_text_color().gamma_multiply(0.5) };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "›", egui::FontId::proportional(14.0), c);
            if r.on_hover_text("Afficher la barre latérale (Ctrl+K Ctrl+B)").clicked() {
                self.show_sidebar = true;
            }
        });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        let mut tree_action = None;
        let panel = ctx.style().visuals.panel_fill;
        let shown = egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(260.0)
            .width_range(150.0..=700.0)
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin { left: 10.0, right: 8.0, top: 4.0, bottom: 6.0 }))
            .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (tab, label) in [(SideTab::Files, "Fichiers"), (SideTab::Git, "Git"), (SideTab::Projects, "Projets")] {
                    if nav_tab(ui, label, self.side_tab == tab).clicked() {
                        self.side_tab = tab;
                        if tab == SideTab::Git {
                            self.pending.push(Cmd::GitRefresh);
                        }
                    }
                }
            });
            ui.add_space(6.0);
            match self.side_tab {
                SideTab::Files => match self.tree.as_mut() {
                    Some(tree) => {
                        let active = self.docs.get(self.active).and_then(|d| d.path.clone());
                        let empty = HashMap::new();
                        let info = if self.settings.tree_git_info { &self.repo_info } else { &empty };
                        tree_action =
                            sidebar::show(ui, tree, active.as_deref(), &self.git.map, info, &mut self.tree_scroll, &mut self.tree_nav);
                    }
                    None => {
                        ui.label("Aucun dossier ouvert.");
                        if ui.button("Ouvrir un dossier…").clicked() {
                            self.pending.push(Cmd::OpenFolder);
                        }
                    }
                },
                SideTab::Git => self.git_ui(ui),
                SideTab::Projects => self.projects_ui(ui),
            }
        });
        // double click on the border between the sidebar and the editor: hide the sidebar
        let edge = shown.response.rect.right();
        let hide = ctx.input(|i| {
            i.pointer.button_double_clicked(egui::PointerButton::Primary)
                && i.pointer.interact_pos().is_some_and(|p| (p.x - edge).abs() <= 5.0 && shown.response.rect.y_range().contains(p.y))
        });
        if hide {
            self.show_sidebar = false;
            self.set_status("Barre latérale masquée : clic sur le bord gauche ou Ctrl+K Ctrl+B pour la réafficher");
        }
        if let Some(a) = tree_action {
            self.tree_action(ctx, a);
        }
    }

    fn tree_action(&mut self, ctx: &egui::Context, a: TreeAction) {
        match a {
            TreeAction::Open(p) => {
                self.open_path(p);
                self.focus_editor(ctx);
            }
            TreeAction::NewFile(d) => self.open_prompt(Prompt::NewFile(d)),
            TreeAction::NewFolder(d) => self.open_prompt(Prompt::NewFolder(d)),
            TreeAction::Rename(p) => self.open_prompt(Prompt::Rename(p)),
            TreeAction::Move(src, dir) => self.move_entry(src, dir),
            TreeAction::Delete(p) => self.confirm = Some(Confirm::Delete(p)),
            TreeAction::CopyPath(p) => ctx.output_mut(|o| o.copied_text = p.display().to_string()),
            TreeAction::Reveal(p) => self.reveal(&p),
            TreeAction::Terminal(d) => {
                let shell = self.term_shell();
                self.term.open(ctx, shell.as_deref(), Some(d));
                self.bottom = Some(BottomTab::Terminal);
            }
            TreeAction::Refresh => self.exec_refresh_tree(),
        }
    }

    fn git_ui(&mut self, ui: &mut egui::Ui) {
        if self.repo.is_none() {
            ui.label("Aucun dépôt git dans ce dossier.");
            if let Some(root) = self.root.clone() {
                if ui.button("git init").clicked() {
                    match git::run(&root, &["init"]) {
                        Ok(o) => {
                            self.log(o);
                            self.repos = git::find_repos(&root, 3);
                            if let Some(r) = self.repos.first().cloned() {
                                self.select_repo(r);
                            }
                        }
                        Err(e) => self.log(e),
                    }
                }
            }
            return;
        }
        if self.repos.len() > 1 {
            let mut chosen = None;
            let current = self.repo.as_deref().map(|r| self.repo_label(r)).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Dépôt").weak());
                egui::ComboBox::from_id_salt("repo_select").selected_text(current).width(ui.available_width() - 8.0).show_ui(
                    ui,
                    |ui| {
                        for r in &self.repos {
                            let n = self.git.map.keys().filter(|p| p.starts_with(r) && p.is_file()).count();
                            let label = if n > 0 { format!("{}  ({n})", self.repo_label(r)) } else { self.repo_label(r) };
                            if ui.selectable_label(self.repo.as_ref() == Some(r), label).clicked() {
                                chosen = Some(r.clone());
                            }
                        }
                    },
                );
            });
            if let Some(r) = chosen {
                self.select_repo(r);
            }
        }
        let accent = self.hl.colors.accent;
        let bg = ui.visuals().panel_fill;
        let mut ops: Vec<(&str, Vec<String>)> = Vec::new();
        let (mut diff, mut open, mut show_commit): (Option<(String, bool)>, Option<String>, Option<String>) = (None, None, None);
        let (mut do_commit, mut dismiss_error) = (false, false);

        // --- branch and its upstream
        if let Some(st) = &self.git.status {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("🔀 {}", st.branch)).strong().size(15.0));
                match st.upstream() {
                    Some((up, ahead, behind)) => {
                        if ahead > 0 {
                            ui.label(RichText::new(format!("↑{ahead}")).color(ok()).strong())
                                .on_hover_text(format!("{ahead} commit(s) à pousser vers {up} (Push)"));
                        }
                        if behind > 0 {
                            ui.label(RichText::new(format!("↓{behind}")).color(warn()).strong())
                                .on_hover_text(format!("{behind} commit(s) à récupérer de {up} (Pull)"));
                        }
                        if ahead == 0 && behind == 0 {
                            ui.label(RichText::new("✔ à jour").color(ok()).small()).on_hover_text(format!("Identique à {up}"));
                        }
                    }
                    None => {
                        ui.label(RichText::new("pas de branche distante").weak().small());
                    }
                }
                if self.git.busy > 0 {
                    ui.spinner();
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if ui.button("⟳").on_hover_text("Rafraîchir").clicked() {
                self.pending.push(Cmd::GitRefresh);
            }
            for (label, cmd, tip) in [
                ("⬇ Pull", Cmd::GitPull, "Récupérer et intégrer les commits distants"),
                ("⬆ Push", Cmd::GitPush, "Envoyer tes commits"),
                ("Fetch", Cmd::GitFetch, "Récupérer sans intégrer"),
            ] {
                if ui.button(label).on_hover_text(tip).clicked() {
                    self.pending.push(cmd);
                }
            }
            if ui.button("Branche ▾").on_hover_text("Changer de branche").clicked() {
                self.pending.push(Cmd::GitCheckout);
            }
            if ui.button("+").on_hover_text("Nouvelle branche").clicked() {
                self.pending.push(Cmd::GitNewBranch);
            }
            if ui.button("Graphe").on_hover_text("Historique des commits (Ctrl+Alt+G)").clicked() {
                self.pending.push(Cmd::GitHistory);
            }
        });

        // --- errors stay until dismissed (or until a later git command succeeds)
        if let Some((label, e)) = self.git.op_error.clone().or_else(|| self.git.error.clone().map(|e| ("status".into(), e))) {
            egui::Frame::none()
                .fill(icons::mix(bg, err(), 0.14))
                .stroke(egui::Stroke::new(1.0_f32, err()))
                .rounding(6.0)
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("⚠ git {label} a échoué")).color(err()).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text("Masquer").clicked() {
                                dismiss_error = true;
                            }
                            if ui.small_button("Copier").clicked() {
                                ui.ctx().output_mut(|o| o.copied_text = e.clone());
                            }
                        });
                    });
                    egui::ScrollArea::vertical().id_salt("git_err").max_height(110.0).show(ui, |ui| {
                        ui.label(RichText::new(&e).monospace().small());
                    });
                });
        }
        // --- merge / cherry-pick / revert / rebase stopped (usually on conflicts)
        if let Some((op, n_conf)) = self.git.status.as_ref().and_then(|st| st.operation.map(|o| (o, st.entries.iter().filter(|e| e.conflicted()).count()))) {
            egui::Frame::none()
                .fill(icons::mix(bg, warn(), 0.16))
                .stroke(egui::Stroke::new(1.0_f32, warn()))
                .rounding(6.0)
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(format!("⚠ {} en cours", op.label())).color(warn()).strong());
                    ui.label(
                        RichText::new(if n_conf > 0 {
                            format!("{n_conf} fichier(s) en conflit : résous-les (bouton ✔ quand c'est fait), puis Continuer.")
                        } else {
                            "Plus de conflit : tu peux continuer.".to_string()
                        })
                        .small(),
                    );
                    ui.horizontal(|ui| {
                        if ui.add_enabled(n_conf == 0, egui::Button::new("Continuer")).clicked() {
                            let mut a = vec!["-c".to_string(), "core.editor=true".to_string()];
                            a.extend(op.continue_args().iter().map(|s| s.to_string()));
                            ops.push(("continuer", a));
                        }
                        if ui.button("Abandonner").on_hover_text("Revient à l'état d'avant l'opération").clicked() {
                            ops.push(("abandonner", op.abort_args().iter().map(|s| s.to_string()).collect()));
                        }
                    });
                });
        }
        ui.add_space(4.0);

        // --- commit
        let (n_staged, n_changed) = self.git.status.as_ref().map_or((0, 0), |st| {
            (st.entries.iter().filter(|e| e.staged()).count(), st.entries.iter().filter(|e| e.unstaged() && e.x != '?').count())
        });
        let resp = ui.add(
            egui::TextEdit::multiline(&mut self.git.commit_msg)
                .hint_text("Message du commit (Ctrl+Entrée)")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        if self.git.focus_commit {
            resp.request_focus();
            self.git.focus_commit = false;
        }
        let ctrl_enter = resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
        ui.horizontal(|ui| {
            let (label, tip) = if n_staged > 0 {
                (format!("✔ Commit ({n_staged} indexé(s))"), "Valide les fichiers indexés")
            } else {
                (format!("✔ Commit tout ({n_changed})"), "Rien d'indexé : valide toutes les modifications des fichiers suivis")
            };
            let can = !self.git.commit_msg.trim().is_empty() && (n_staged > 0 || n_changed > 0);
            let b = filled_button(label, accent);
            if ui.add_enabled(can, b).on_hover_text(tip).clicked() || (ctrl_enter && can) {
                do_commit = true;
            }
        });
        ui.add_space(6.0);

        // --- changes (staged / not staged), with +/- lines
        let max_h = (ui.available_height() * 0.5).max(120.0);
        egui::ScrollArea::vertical().id_salt("git_entries").max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
            let Some(st) = &self.git.status else { return };
            if st.entries.is_empty() {
                ui.label(RichText::new("✔ Rien à valider, tout est propre.").color(ok()));
                return;
            }
            let conflicted: Vec<&git::Entry> = st.entries.iter().filter(|e| e.conflicted()).collect();
            if !conflicted.is_empty() {
                ui.label(RichText::new(format!("CONFLITS · {}", conflicted.len())).small().strong().color(err()));
                ui.label(RichText::new("Ouvre chaque fichier et choisis la version à garder, puis ✔.").small().weak());
                for e in &conflicted {
                    match git_file_row(ui, &e.path, 'C', None, "✔") {
                        RowClick::Button => ops.push(("add", vec!["add".into(), "--".into(), e.path.clone()])),
                        RowClick::Row | RowClick::Open => open = Some(e.path.clone()),
                        RowClick::None => {}
                    }
                }
                ui.add_space(6.0);
            }
            let staged: Vec<&git::Entry> = st.entries.iter().filter(|e| e.staged() && !e.conflicted()).collect();
            let changed: Vec<&git::Entry> = st.entries.iter().filter(|e| e.unstaged() && !e.conflicted()).collect();
            for (title, list, is_staged, color) in
                [("INDEXÉS", &staged, true, ok()), ("MODIFICATIONS", &changed, false, warn())]
            {
                if list.is_empty() {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{title} · {}", list.len())).small().strong().color(color));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (l, args): (&str, Vec<String>) = if is_staged {
                            ("Tout désindexer", vec!["restore".into(), "--staged".into(), ".".into()])
                        } else {
                            ("Tout indexer", vec!["add".into(), "-A".into()])
                        };
                        if ui.small_button(l).clicked() {
                            ops.push((if is_staged { "restore" } else { "add" }, args));
                        }
                    });
                });
                for e in list.iter() {
                    let letter = if is_staged { e.x } else { e.label() };
                    let stats = if is_staged { st.staged_stats.get(&e.path) } else { st.unstaged_stats.get(&e.path) };
                    let r = git_file_row(ui, &e.path, letter, stats.copied(), if is_staged { "−" } else { "+" });
                    match r {
                        RowClick::Button => {
                            if is_staged {
                                ops.push(("restore", vec!["restore".into(), "--staged".into(), "--".into(), e.path.clone()]));
                            } else {
                                ops.push(("add", vec!["add".into(), "--".into(), e.path.clone()]));
                            }
                        }
                        RowClick::Open => open = Some(e.path.clone()),
                        RowClick::Row => diff = Some((e.path.clone(), is_staged)),
                        RowClick::None => {}
                    }
                }
                ui.add_space(6.0);
            }
        });
        // --- stash
        let mut stash_act: Option<(&str, String)> = None;
        let has_changes = self.git.status.as_ref().is_some_and(|st| !st.entries.is_empty());
        let stashes = self.git.status.as_ref().map(|st| st.stashes.clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("MIS DE CÔTÉ (STASH) · {}", stashes.len())).small().strong().color(accent));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(has_changes, egui::Button::new("Mettre de côté").small())
                    .on_hover_text("git stash : range toutes les modifications (même les nouveaux fichiers) pour plus tard ; le message du commit sert de nom")
                    .clicked()
                {
                    stash_act = Some(("push", String::new()));
                }
            });
        });
        for st in &stashes {
            ui.horizontal(|ui| {
                let r = ui
                    .add(egui::Label::new(RichText::new(&st.subject).size(12.5)).sense(Sense::click()).truncate())
                    .on_hover_text(format!("{} · {}\nClic : voir le contenu", st.name, git::ago(st.time)));
                if r.clicked() {
                    stash_act = Some(("show", st.name.clone()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text("Supprimer").clicked() {
                        stash_act = Some(("drop", st.name.clone()));
                    }
                    if ui.small_button("Récupérer").on_hover_text("Remet les modifications et retire le stash (pop)").clicked() {
                        stash_act = Some(("pop", st.name.clone()));
                    }
                    if ui.small_button("Appliquer").on_hover_text("Remet les modifications et garde le stash").clicked() {
                        stash_act = Some(("apply", st.name.clone()));
                    }
                });
            });
        }
        ui.separator();

        // --- recent commits
        ui.horizontal(|ui| {
            ui.label(RichText::new("DERNIERS COMMITS").small().strong().color(accent));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Graphe →").clicked() {
                    self.pending.push(Cmd::GitHistory);
                }
            });
        });
        egui::ScrollArea::vertical().id_salt("git_log").auto_shrink([false, false]).show(ui, |ui| {
            for c in &self.git.log {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), Sense::click());
                let hovered = resp.hovered();
                if hovered {
                    ui.painter().rect_filled(rect, 5.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let p = ui.painter().with_clip_rect(rect.shrink(2.0));
                let mut x = rect.left() + 6.0;
                for r in c.refs.split(", ").filter(|r| !r.is_empty()) {
                    let r = r.strip_prefix("HEAD -> ").unwrap_or(r);
                    let g = p.layout_no_wrap(r.to_string(), egui::FontId::proportional(10.5), accent);
                    let chip = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 3.0), g.size() + egui::vec2(8.0, 2.0));
                    p.rect_filled(chip, 4.0, icons::mix(bg, accent, 0.18));
                    p.galley(chip.min + egui::vec2(4.0, 1.0), g, accent);
                    x = chip.right() + 4.0;
                }
                p.text(egui::pos2(x, rect.top() + 3.0), Align2::LEFT_TOP, &c.subject, egui::FontId::proportional(13.0), ui.visuals().strong_text_color());
                p.text(
                    egui::pos2(rect.left() + 6.0, rect.top() + 21.0),
                    Align2::LEFT_TOP,
                    format!("{} · {} · {}", c.short, c.author, git::ago(c.time)),
                    egui::FontId::proportional(11.5),
                    ui.visuals().weak_text_color(),
                );
                if resp.on_hover_text("Voir ce commit dans le graphe").clicked() {
                    show_commit = Some(c.hash.clone());
                }
            }
        });

        if dismiss_error {
            self.git.op_error = None;
            self.git.error = None;
        }
        match stash_act {
            Some(("push", _)) => {
                let msg = self.git.commit_msg.trim().lines().next().unwrap_or("").to_string();
                let mut a = vec!["stash".to_string(), "push".into(), "--include-untracked".into()];
                if !msg.is_empty() {
                    a.extend(["-m".into(), msg]);
                }
                ops.push(("stash", a));
            }
            Some(("show", name)) => {
                if let Some(repo) = self.repo.clone() {
                    self.jobs.spawn(move || {
                        let text = git::run(&repo, &["stash", "show", "-p", "--include-untracked", &name])
                            .or_else(|_| git::run(&repo, &["stash", "show", "-p", &name]))
                            .unwrap_or_else(|e| e);
                        JobResult::NewTab { title: format!("stash : {name}"), text, syntax: "Diff".into() }
                    });
                }
            }
            Some(("drop", name)) => {
                self.confirm = Some(Confirm::Git {
                    title: "Supprimer le stash".into(),
                    msg: format!("Supprimer définitivement « {name} » ?\nSes modifications seront perdues."),
                    label: "stash drop".into(),
                    args: vec!["stash".into(), "drop".into(), name],
                });
            }
            Some((verb, name)) => ops.push(("stash", vec!["stash".into(), verb.into(), name])),
            None => {}
        }
        if do_commit {
            let msg = self.git.commit_msg.trim().to_string();
            let mut args = vec!["commit".to_string()];
            if n_staged == 0 {
                args.push("-a".into()); // nothing staged: commit all tracked changes
            }
            args.push("-m".into());
            args.push(msg);
            ops.push(("commit", args));
            self.git.commit_msg.clear();
        }
        for (label, args) in ops {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            self.git_job(label, &a);
        }
        if let Some((p, staged)) = diff {
            self.show_wip_file(p, staged);
        }
        if let (Some(p), Some(repo)) = (open, self.repo.clone()) {
            self.open_path(repo.join(p));
        }
        if let Some(h) = show_commit {
            self.history.open = true;
            self.prefs.open = false;
            self.history.select_hash = Some(h);
            self.load_history();
        }
    }

    fn bottom_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom").resizable(true).default_height(230.0).height_range(80.0..=900.0).show(
            ctx,
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let nd: usize = self.docs.iter().map(|d| d.diagnostics.len()).sum();
                    let problems = if nd > 0 { format!("Problèmes {nd}") } else { "Problèmes".to_string() };
                    for (tab, label) in [
                        (BottomTab::Search, "Recherche".to_string()),
                        (BottomTab::Problems, problems),
                        (BottomTab::Terminal, "Terminal".to_string()),
                        (BottomTab::Output, "Sortie".to_string()),
                    ] {
                        if nav_tab(ui, &label, self.bottom == Some(tab)).clicked() {
                            self.bottom = Some(tab);
                            match tab {
                                BottomTab::Search => self.search.focus = true,
                                BottomTab::Terminal => self.term.want_focus = true,
                                _ => {}
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").clicked() {
                            self.bottom = None;
                        }
                    });
                });
                ui.separator();
                match self.bottom {
                    // already shown in a tab: not twice
                    Some(BottomTab::Search) if self.docs.iter().any(|d| d.view == document::View::Search) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("La recherche est dans un onglet de l'éditeur.").weak());
                            if ui.button("La remettre ici").clicked() {
                                if let Some(i) = self.docs.iter().position(|d| d.view == document::View::Search) {
                                    self.close_tab(i, true);
                                }
                                self.settings.search_in_tab = false;
                                self.persist("search_in_tab", "false".into());
                            }
                        });
                    }
                    Some(BottomTab::Search) => self.search_ui(ui, false),
                    Some(BottomTab::Problems) => self.problems_ui(ui),
                    Some(BottomTab::Terminal) => {
                        if self.term.cwd.is_none() {
                            self.term.cwd = self.root.clone();
                        }
                        let shell = self.term_shell();
                        self.term.ui(ui, shell.as_deref(), self.hl.colors, self.hl.is_light());
                    }
                    Some(BottomTab::Output) => {
                        if ui.small_button("Effacer").clicked() {
                            self.output.clear();
                        }
                        egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                            for l in &self.output {
                                ui.add(egui::Label::new(RichText::new(l).monospace()).extend());
                            }
                        });
                    }
                    None => {}
                }
            },
        );
    }

    fn problems_ui(&mut self, ui: &mut egui::Ui) {
        let mut goto = None;
        if let Some(linter) = self.python_linter.clone() {
            ui.horizontal(|ui| {
                if linter == "syntaxe" {
                    ui.label(
                        RichText::new("Python : aucun linter installé, seules les erreurs de syntaxe sont détectées.").color(warn()),
                    );
                    if ui.button("Installer flake8").on_hover_text(format!("pip install --target {}", lint::tools_dir().display())).clicked() {
                        self.pending.push(Cmd::InstallFlake8);
                    }
                } else {
                    ui.label(RichText::new(format!("Python : {linter}")).weak().small());
                }
            });
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut any = false;
            for (i, d) in self.docs.iter().enumerate() {
                if d.diagnostics.is_empty() {
                    continue;
                }
                any = true;
                ui.label(RichText::new(d.title()).strong());
                for g in &d.diagnostics {
                    let (icon, col) = if g.severity == Severity::Error { ("✖", err()) } else { ("⚠", warn()) };
                    let text = format!("  {icon} {}:{}  {}  [{}]", g.line + 1, g.col + 1, g.message, g.source);
                    let r = ui.add(egui::Label::new(RichText::new(text).color(col).monospace()).sense(Sense::click()));
                    if r.clicked() {
                        goto = Some((i, g.line, g.col));
                    }
                }
            }
            if !any {
                ui.label(RichText::new("Aucun problème détecté (le lint Python/JS s'exécute à l'enregistrement).").weak());
            }
        });
        if let Some((i, l, c)) = goto {
            self.active = i;
            self.docs[i].goto_line(l, c);
            ui.ctx().memory_mut(|m| m.request_focus(editor_id()));
        }
    }

    fn central(&mut self, ctx: &egui::Context) {
        self.normalize_panes();
        self.drop_zones = DropZones::default();
        if std::mem::take(&mut self.pending_focus) {
            ctx.memory_mut(|m| m.request_focus(editor_id()));
        }
        let mut welcome = false;
        let mut editor_resp = None;
        let special = self.prefs.open || self.history.open || self.split.as_ref().is_some_and(|s| s.shown);
        let two = !special && self.is_split();
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            if two {
                let frame = egui::Frame::none();
                match self.split_dir {
                    SplitDir::Right => {
                        let w = ui.available_width() / 2.0;
                        egui::SidePanel::right("pane_1").resizable(true).default_width(w).min_width(180.0).frame(frame).show_inside(ui, |ui| {
                            let r = self.pane_ui(ui, 1);
                            editor_resp = editor_resp.take().or(r);
                        });
                    }
                    SplitDir::Down => {
                        let h = ui.available_height() / 2.0;
                        egui::TopBottomPanel::bottom("pane_1").resizable(true).default_height(h).min_height(120.0).frame(frame).show_inside(ui, |ui| {
                            let r = self.pane_ui(ui, 1);
                            editor_resp = editor_resp.take().or(r);
                        });
                    }
                }
                egui::CentralPanel::default().frame(frame).show_inside(ui, |ui| {
                    let r = self.pane_ui(ui, 0);
                    editor_resp = editor_resp.take().or(r);
                });
                return;
            }
            if !self.docs.is_empty() || special {
                self.tabs_strip(ui, None);
            }
            if self.prefs.open {
                egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| self.prefs_ui(ui));
                return;
            }
            if self.split.as_ref().is_some_and(|s| s.shown) {
                egui::CentralPanel::default().show_inside(ui, |ui| self.split_ui(ui));
                return;
            }
            if self.history.open {
                egui::CentralPanel::default().show_inside(ui, |ui| self.history_ui(ui));
                return;
            }
            self.drop_zones.panes.push((0, ui.available_rect_before_wrap()));
            if self.active < self.docs.len() {
                self.breadcrumbs_ui(ui, self.active, 0);
                self.conflict_bar(ui, self.active, 0);
            }
            if self.active < self.docs.len() {
                editor_resp = self.doc_body(ui, self.active, editor_id(), true);
            } else {
                welcome = true;
            }
        });
        if let Some(r) = editor_resp {
            r.context_menu(|ui| self.editor_context_menu(ui));
        }
        // a click in the other panel's editor makes it the active panel
        if ctx.memory(|m| m.has_focus(editor_view::other_editor_id())) {
            if let Some(i) = self.pane_doc(1 - self.focused_pane) {
                self.active = i;
                ctx.memory_mut(|m| m.request_focus(editor_id()));
            }
        }
        self.resolve_tab_drag(ctx);
        if welcome {
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| self.welcome_ui(ui));
        }
    }

    /// Selected text (single line) or the word under the cursor.
    fn word_under_cursor(&self) -> String {
        let Some(d) = self.docs.get(self.active) else { return String::new() };
        let p = d.primary_sel();
        let (a, b) = if p.is_empty() { d.word_at(p.head) } else { (p.start(), p.end()) };
        let t = d.slice(a, b);
        if t.contains('\n') {
            String::new()
        } else {
            t
        }
    }

    /// Right click in the editor: actions on the line / word under the cursor.
    fn editor_context_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(280.0);
        let word = self.word_under_cursor();
        let Some(d) = self.docs.get(self.active) else { return };
        let ro = d.read_only;
        let line = d.rope.char_to_line(d.primary_sel().head);
        let has_change = d.hunks.iter().any(|h| h.contains(line));
        let noqa = noqa_codes(d, line);
        let has_path = d.path.is_some();
        let has_folds = !d.folds.is_empty();
        let has_lsp = self.settings.lsp && d.path.as_deref().and_then(lsp::language_of).is_some();
        let in_repo = d.path.as_deref().and_then(|p| self.repo_for(p)).is_some();
        let (km, blame_on, has_root) = (&self.keymap, self.blame_mode, self.root.is_some());
        let mut chosen = None;
        let mut item = |ui: &mut egui::Ui, label: &str, cmd: Cmd, enabled: bool| {
            let mod_key = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
            let shortcut = match cmd {
                // handled by egui's clipboard events, not by the keymap
                Cmd::Copy => format!("{mod_key}+C"),
                Cmd::Cut => format!("{mod_key}+X"),
                Cmd::Paste => format!("{mod_key}+V"),
                _ => km.label(cmd),
            };
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut)).clicked() {
                chosen = Some(cmd);
                ui.close_menu();
            }
        };
        ui.label(RichText::new(format!("Ligne {}", line + 1)).small().weak());
        item(ui, "Couper", Cmd::Cut, !ro);
        item(ui, "Copier", Cmd::Copy, true);
        item(ui, "Coller", Cmd::Paste, !ro);
        if !word.is_empty() {
            let short: String = word.chars().take(28).collect();
            let short = if short.len() < word.len() { format!("{short}…") } else { short };
            ui.separator();
            item(ui, &format!("Sélectionner toutes les occurrences de « {short} »"), Cmd::SelectAllOccurrences, true);
            item(ui, &format!("Rechercher « {short} » dans le dossier"), Cmd::FindWordInFiles, has_root);
        }
        ui.separator();
        item(ui, "Aller à la définition", Cmd::GotoDefinition, true);
        item(ui, "Suggestions", Cmd::Complete, !ro);
        if has_lsp {
            item(ui, "Renommer le symbole…", Cmd::RenameSymbol, !ro);
        }
        if !noqa.is_empty() {
            ui.separator();
            item(ui, &format!("Ignorer {} sur cette ligne (# noqa)", noqa.join(", ")), Cmd::NoqaLine, !ro);
        }
        ui.separator();
        item(ui, "Commenter / décommenter", Cmd::ToggleComment, !ro);
        item(ui, "Replier le bloc", Cmd::Fold, true);
        item(ui, "Tout replier", Cmd::FoldAll, true);
        item(ui, "Tout déplier", Cmd::UnfoldAll, has_folds);
        item(ui, "Dupliquer la ligne", Cmd::DuplicateLine, !ro);
        item(ui, "Supprimer la ligne", Cmd::DeleteLine, !ro);
        item(ui, "Monter la ligne", Cmd::MoveLineUp, !ro);
        item(ui, "Descendre la ligne", Cmd::MoveLineDown, !ro);
        if in_repo {
            ui.separator();
            item(ui, if blame_on { "Masquer le blame" } else { "Blame de la ligne" }, Cmd::GitBlame, true);
            item(ui, "Historique de la ligne", Cmd::GitLineHistory, true);
            if has_change {
                item(ui, "Voir le changement", Cmd::ShowChange, true);
                item(ui, "Revenir en arrière sur ce changement", Cmd::GitRevertChange, !ro);
            }
            item(ui, "Copier le lien GitHub de la ligne", Cmd::CopyGithubLink, true);
        }
        if has_path {
            ui.separator();
            item(ui, "Copier le chemin du fichier", Cmd::CopyFilePath, true);
            item(ui, "Copier la référence fichier:ligne", Cmd::CopyFileRef, true);
            item(ui, "Afficher dans l'arborescence", Cmd::RevealInTree, true);
            item(ui, "Afficher dans le gestionnaire de fichiers", Cmd::RevealInOs, true);
        }
        if let Some(c) = chosen {
            self.pending.push(c);
        }
    }

    fn copy_github_link(&mut self, ctx: &egui::Context) {
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(path) = d.path.clone() else { return };
        let line = d.rope.char_to_line(d.primary_sel().head);
        let head_line = d.head_line(line).unwrap_or(line);
        let Some(repo) = self.repo_for(&path) else {
            self.set_status("Fichier hors d'un dépôt git");
            return;
        };
        let Some((owner, name)) = git::stdout(&repo, &["remote", "get-url", "origin"]).ok().and_then(|u| git::parse_github(u.trim()))
        else {
            self.set_status("Le remote « origin » n'est pas un dépôt GitHub");
            return;
        };
        let rev = git::stdout(&repo, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_else(|_| "HEAD".into());
        let rel = path.strip_prefix(&repo).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        let link = format!("https://github.com/{owner}/{name}/blob/{rev}/{rel}#L{}", head_line + 1);
        ctx.output_mut(|o| o.copied_text = link.clone());
        self.set_status(format!("Lien copié : {link}"));
    }

    /// Start screen in the style of the Florm artwork: logo, title, shortcuts, recent projects.
    fn welcome_ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        logo::paint_backdrop(ui.painter(), rect);
        let size = (rect.height() * 0.28).clamp(80.0, 220.0);
        let top = rect.top() + (rect.height() * 0.10).max(16.0);
        let logo_rect = egui::Rect::from_center_size(egui::pos2(rect.center().x, top + size / 2.0), egui::Vec2::splat(size));
        logo::paint(ui.painter(), logo_rect, true);
        let title_size = size * 0.30;
        let title_y = logo_rect.bottom() + size * 0.10;
        logo::paint_title(ui.painter(), egui::pos2(rect.center().x, title_y), title_size);

        let y = title_y + title_size * 2.2;
        let width = 560.0_f32.min(rect.width() - 32.0);
        let inner = egui::Rect::from_min_max(egui::pos2(rect.center().x - width / 2.0, y), egui::pos2(rect.center().x + width / 2.0, rect.bottom() - 8.0));
        let mut cmd = None;
        let mut open = None;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
            // The backdrop is always dark navy: fixed light colors, whatever the editor theme.
            ui.visuals_mut().override_text_color = Some(WELCOME_TEXT);
            ui.columns(2, |cols| {
                welcome_card(&mut cols[0], "DÉMARRER", logo::CYAN, |ui| {
                    for (label, c) in [
                        ("Nouveau fichier", Cmd::NewFile),
                        ("Ouvrir un dossier…", Cmd::OpenFolder),
                        ("Aller au fichier…", Cmd::QuickOpen),
                        ("Palette de commandes", Cmd::CommandPalette),
                        ("Rechercher dans le dossier", Cmd::FindInFiles),
                        ("Graphe git", Cmd::GitHistory),
                    ] {
                        if welcome_row(ui, label, &self.keymap.label(c), true, logo::CYAN).clicked() {
                            cmd = Some(c);
                        }
                    }
                });
                welcome_card(&mut cols[1], "RÉCENTS", logo::PURPLE, |ui| {
                    for p in self.projects.iter().filter(|p| p.path.is_dir()).take(6) {
                        let parent = p.path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                        let r = welcome_row(ui, &p.name, &parent, false, logo::PURPLE)
                            .on_hover_text(p.path.display().to_string());
                        if r.clicked() {
                            open = Some(p.path.clone());
                        }
                    }
                    if self.projects.is_empty() {
                        ui.label(RichText::new("Aucun projet récent").color(WELCOME_DIM));
                    }
                });
            });
        });
        if let Some(c) = cmd {
            self.pending.push(c);
        }
        if let Some(p) = open {
            self.switch_project(p);
        }
    }

    /// Repositories changed outside Florm (commit, checkout, add… in a terminal): refresh the git
    /// panel, the graph and the gutter marks of their open files.
    fn check_git_stamps(&mut self) {
        let mut changed = Vec::new();
        for r in &self.repos {
            let st = git::stamp(r);
            match self.git_stamps.insert(r.clone(), st.clone()) {
                Some(old) if old != st => changed.push(r.clone()),
                _ => {}
            }
        }
        if changed.is_empty() {
            return;
        }
        self.git_refresh();
        if self.history.open && self.history.repo.as_ref().is_some_and(|r| changed.contains(r)) && self.history.rebase.is_none() {
            self.load_history();
            if let Some(staged) = self.history.wip_sel {
                self.load_wip_all(staged);
            }
        }
        for i in 0..self.docs.len() {
            let in_repo = self.docs[i].path.as_deref().and_then(|p| self.repo_for(p)).is_some_and(|r| changed.contains(&r));
            if in_repo {
                self.refresh_git_base(i);
            }
        }
    }

    /// Re-reads the HEAD version of document `i` (gutter marks), without linting it.
    fn refresh_git_base(&mut self, i: usize) {
        let Some(d) = self.docs.get(i) else { return };
        let Some(path) = d.path.clone() else { return };
        let (uid, version) = (d.uid, d.version);
        self.jobs.spawn(move || JobResult::Analyzed { uid, version, reload: false, diags: None, base: git::head_text(&path), log: Vec::new() });
    }

    fn periodic(&mut self, ctx: &egui::Context) {
        // with hot exit, unsaved changes are kept in the session: no question
        let keeps = self.settings.hot_exit && self.root.is_some();
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quit_confirmed
            && !keeps
            && self.docs.iter().any(|d| d.is_dirty())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm::Quit);
        }
        if ctx.input(|i| i.viewport().close_requested()) || self.last_session_save.elapsed() > Duration::from_secs(30) {
            self.save_session();
        }
        // Live lint for in-process linters (XML/JSON), debounced.
        if let Some(d) = self.docs.get_mut(self.active) {
            if d.lint_version != d.version {
                if let Some(lang) = d.path.as_deref().and_then(lint::lang_of).filter(|l| lint::is_in_process(l)) {
                    let el = d.last_change.elapsed();
                    if el >= Duration::from_millis(300) {
                        d.diagnostics = lint::lint_in_process(lang, &d.rope.to_string());
                        d.lint_version = d.version;
                    } else {
                        ctx.request_repaint_after(Duration::from_millis(310) - el);
                    }
                }
            }
        }
        // Live diff against HEAD for the gutter, debounced while typing.
        if let Some(d) = self.docs.get_mut(self.active) {
            if d.diff_version != d.version {
                let el = d.last_change.elapsed();
                if el >= Duration::from_millis(150) || d.diff_version == u64::MAX {
                    d.refresh_diff();
                } else {
                    ctx.request_repaint_after(Duration::from_millis(160) - el);
                }
            }
        }
        // Files changed outside the editor (checked at once when coming back to the window).
        let focused = ctx.input(|i| i.focused);
        if focused && !self.was_focused {
            self.last_disk_check = Instant::now() - Duration::from_secs(10);
        }
        self.was_focused = focused;
        if self.last_disk_check.elapsed() > Duration::from_secs(2) {
            self.last_disk_check = Instant::now();
            self.check_git_stamps();
            let mut msgs = Vec::new();
            for d in self.docs.iter_mut() {
                let Some(p) = d.path.clone() else { continue };
                let Ok(m) = std::fs::metadata(&p).and_then(|m| m.modified()) else { continue };
                if d.mtime.is_some() && d.mtime != Some(m) {
                    if d.is_dirty() {
                        d.mtime = Some(m);
                        msgs.push(format!("{} a été modifié sur le disque", d.title()));
                    } else if d.reload_from_disk().is_ok() {
                        msgs.push(format!("{} rechargé depuis le disque", d.title()));
                    }
                }
            }
            for m in msgs {
                self.set_status(m);
            }
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        let mut title = String::from("Florm");
        if let Some(d) = self.docs.get(self.active) {
            title = format!("{}{} — Florm", d.title(), if d.is_dirty() { " •" } else { "" });
        }
        if let Some(r) = &self.root {
            title = format!("{title} — {}", r.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
        }
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if std::mem::take(&mut self.visuals_dirty) {
            apply_visuals(ctx, self.hl.visuals());
        }
        self.process_jobs();
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
            self.last_active_path = cur;
        }
        self.lsp_frame(ctx);
        self.completion_keys(ctx);
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
        self.completion_popup(ctx);
        let (mb, mf) = ctx.input(|i| (i.pointer.button_pressed(egui::PointerButton::Extra1), i.pointer.button_pressed(egui::PointerButton::Extra2)));
        if mb || mf {
            self.nav_go(mb);
        }
        self.track_nav();
        self.line_history_ui(ctx);
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
            && !self.history.open
            && !self.prefs.open
            && ctx.memory(|m| m.focused().is_none())
        {
            self.focus_editor(ctx);
        }
    }
}

/// One editor tab: rounded top, active tab merged with the editor and underlined with
/// the theme accent gradient, close button (or "modified" dot).
fn tab_widget(
    ui: &mut egui::Ui,
    title: &str,
    icon: Option<&str>,
    dirty: bool,
    active: bool,
    error: bool,
    tc: &crate::highlight::TabColors,
) -> (egui::Response, bool) {
    let font = egui::FontId::proportional(14.0);
    let text_color = if error { err() } else if active { tc.text_active } else { tc.text_inactive };
    let galley = ui.painter().layout_no_wrap(title.to_string(), font, text_color);
    let icon_w = if icon.is_some() { 22.0 } else { 0.0 };
    let w = galley.size().x + 52.0 + icon_w;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, TAB_STRIP_H), Sense::click_and_drag());
    let tab = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + 5.0), rect.right_bottom());
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
    if let Some(name) = icon {
        let bg = if active { tc.active } else { tc.bar };
        let light = bg.r() as u32 + bg.g() as u32 + bg.b() as u32 > 384;
        let r = egui::Rect::from_center_size(egui::pos2(tab.left() + 20.0, tab.center().y), egui::vec2(15.0, 15.0));
        icons::paint_file(p, r, name, bg, light);
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
    let closed = close.on_hover_text("Fermer (Ctrl+W)").clicked();
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
        item(ui, "Déplacer dans l'autre panneau", TabAct::MoveOther(i), true, "");
        item(ui, "Fusionner les panneaux", TabAct::Unsplit, true, "");
        item(ui, "Changer l'orientation (côte à côte / l'un sous l'autre)", TabAct::Flip, true, "");
    } else {
        let tip = "Ouvre un deuxième fichier d'abord : un fichier ne peut être que dans un panneau.";
        item(ui, "Diviser à droite", TabAct::Split(i, SplitDir::Right), !alone, tip);
        item(ui, "Diviser en bas", TabAct::Split(i, SplitDir::Down), !alone, tip);
    }
    ui.separator();
    item(ui, "Fermer", TabAct::Close(i), true, "");
    item(ui, "Fermer les autres onglets", TabAct::CloseOthers(i), !alone, "");
    if let Some(p) = &d.path {
        ui.separator();
        item(ui, "Afficher dans l'arborescence (Alt+L)", TabAct::Locate(i), true, "");
        item(ui, "Copier le chemin", TabAct::CopyPath(p.display().to_string()), true, "");
        item(ui, "Afficher dans le gestionnaire de fichiers", TabAct::Reveal(p.clone()), true, "");
    }
}

/// What was clicked on a file row of the Git panel / graph.
#[derive(PartialEq)]
enum RowClick {
    None,
    Row,
    Open,
    Button,
}

/// "+12 −3" (or "binaire") for a file.
fn stat_text(stats: Option<(Option<u32>, Option<u32>)>) -> Option<(String, String)> {
    match stats? {
        (Some(a), Some(d)) => Some((format!("+{a}"), format!("−{d}"))),
        _ => Some(("binaire".into(), String::new())),
    }
}

/// File line: status letter, name and folder, +/- lines; `button` ("+", "−") shown on hover.
fn git_file_row(ui: &mut egui::Ui, path: &str, letter: char, stats: Option<(Option<u32>, Option<u32>)>, button: &str) -> RowClick {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    let v = ui.visuals().clone();
    if hovered {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let p = ui.painter().with_clip_rect(rect);
    let cy = rect.center().y;
    let color = match letter {
        'M' | 'R' => warn(),
        'A' | 'U' | '?' => ok(),
        'D' | 'C' => err(),
        _ => v.weak_text_color(),
    };
    let letter = if letter == '?' { 'U' } else { letter };
    p.text(egui::pos2(rect.left() + 6.0, cy), Align2::LEFT_CENTER, letter.to_string(), egui::FontId::monospace(12.5), color);
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    // stats on the right, then the name gets the remaining room
    let mut right = rect.right() - 6.0;
    let mut clicked_button = false;
    if hovered && !button.is_empty() {
        let b = egui::Rect::from_center_size(egui::pos2(right - 10.0, cy), egui::vec2(20.0, 18.0));
        let tip = match button {
            "+" => "Indexer",
            "✔" => "Marquer comme résolu (git add)",
            _ => "Désindexer",
        };
        clicked_button = ui.put(b, egui::Button::new(RichText::new(button).strong())).on_hover_text(tip).clicked();
        right = b.left() - 4.0;
    }
    if let Some((a, d)) = stat_text(stats) {
        let font = egui::FontId::monospace(11.5);
        if !d.is_empty() {
            let r = p.text(egui::pos2(right, cy), Align2::RIGHT_CENTER, &d, font.clone(), err());
            right = r.left() - 4.0;
        }
        let r = p.text(egui::pos2(right, cy), Align2::RIGHT_CENTER, &a, font, if d.is_empty() { v.weak_text_color() } else { ok() });
        right = r.left() - 6.0;
    }
    let p = p.with_clip_rect(egui::Rect::from_min_max(rect.min, egui::pos2(right, rect.max.y)));
    let r = p.text(egui::pos2(rect.left() + 22.0, cy), Align2::LEFT_CENTER, name, egui::FontId::proportional(13.0), v.strong_text_color());
    if !dir.is_empty() {
        p.text(egui::pos2(r.right() + 6.0, cy), Align2::LEFT_CENTER, dir, egui::FontId::proportional(11.5), v.weak_text_color());
    }
    let resp = resp.on_hover_text(format!("{path}\nClic : voir le diff · Double-clic : ouvrir le fichier"));
    if clicked_button {
        RowClick::Button
    } else if resp.double_clicked() {
        RowClick::Open
    } else if resp.clicked() {
        RowClick::Row
    } else {
        RowClick::None
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
            branches.push((r.to_string(), format!("Checkout de la branche « {r} »")));
        } else if let Some((_, name)) = r.split_once('/') {
            if !local.iter().any(|l| l == name) && !c.refs.iter().any(|x| x == name) {
                branches.push((name.to_string(), format!("Checkout de « {name} » (suit {r})")));
            }
        }
    }
    for (target, label) in branches {
        item(ui, &label, HistAct::Checkout(target), act);
    }
    item(ui, "Checkout de ce commit (HEAD détachée)", HistAct::Checkout(c.hash.clone()), act);
    item(ui, "Créer une branche ici…", HistAct::BranchAt(c.hash.clone()), act);
    ui.separator();
    item(ui, "Voir le diff complet", HistAct::Diff(c.hash.clone()), act);
    if let Some(s) = selected {
        let title = format!("{} → {}", &s[..s.len().min(8)], short);
        item(ui, "Comparer avec le commit sélectionné", HistAct::Compare(s.to_string(), c.hash.clone(), title), act);
    }
    ui.menu_button("Comparer avec la branche", |ui| {
        item(ui, "HEAD (branche courante)", HistAct::Compare(c.hash.clone(), "HEAD".into(), format!("{short} → HEAD")), act);
        for b in local {
            item(ui, b, HistAct::Compare(c.hash.clone(), b.clone(), format!("{short} → {b}")), act);
        }
    });
    ui.separator();
    item(ui, "Cherry-pick sur la branche courante", HistAct::CherryPick(c.hash.clone()), act);
    item(ui, "Revert : annuler ce commit par un nouveau commit", HistAct::Revert(c.hash.clone()), act);
    if c.parents.len() == 1 {
        item(ui, "Rebase interactif à partir d'ici…", HistAct::Rebase(c.hash.clone()), act);
        item(ui, "✂ Découper ce commit en plusieurs…", HistAct::Split(c.hash.clone()), act);
    }
    ui.separator();
    item(ui, "Copier le hash", HistAct::Copy(c.hash.clone()), act);
    item(ui, "Copier le message", HistAct::Copy(c.subject.clone()), act);
}

const WELCOME_TEXT: Color32 = Color32::from_rgb(0xe8, 0xee, 0xf8);
const WELCOME_DIM: Color32 = Color32::from_rgb(0xa3, 0xb3, 0xcc);

/// Rounded panel on the welcome backdrop, with a colored title.
fn welcome_card(ui: &mut egui::Ui, title: &str, color: Color32, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(Color32::from_rgb(0x16, 0x22, 0x38))
        .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(0x26, 0x36, 0x52)))
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
fn welcome_row(ui: &mut egui::Ui, label: &str, detail: &str, keycap: bool, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), Sense::click());
    let hovered = resp.hovered();
    let p = ui.painter().with_clip_rect(rect);
    if hovered {
        p.rect_filled(rect, 6.0, Color32::from_rgb(0x22, 0x33, 0x52));
        p.rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 6.0), egui::vec2(3.0, 16.0)), 1.5, color);
    }
    let cy = rect.center().y;
    let text = if hovered { Color32::WHITE } else { WELCOME_TEXT };
    let lr = p.text(egui::pos2(rect.left() + 10.0, cy), Align2::LEFT_CENTER, label, egui::FontId::proportional(14.0), text);
    if !detail.is_empty() {
        let font = egui::FontId::proportional(if keycap { 11.5 } else { 12.0 });
        let galley = p.layout_no_wrap(detail.to_string(), font, WELCOME_DIM);
        let w = galley.size().x;
        if keycap {
            let key = egui::Rect::from_min_size(egui::pos2(rect.right() - w - 18.0, cy - 10.0), egui::vec2(w + 12.0, 20.0));
            p.rect_filled(key, 4.0, Color32::from_rgb(0x0f, 0x19, 0x2b));
            p.rect_stroke(key, 4.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(0x34, 0x47, 0x68)));
            p.galley(key.center() - galley.size() / 2.0, galley, WELCOME_DIM);
        } else {
            // path after the name, cut on the left when too long
            let x = lr.right() + 10.0;
            let room = rect.right() - 8.0 - x;
            if room > 30.0 {
                let mut d = detail.to_string();
                let mut g = galley;
                while g.size().x > room && d.chars().count() > 2 {
                    d = format!("…{}", d.chars().skip(4).collect::<String>());
                    g = p.layout_no_wrap(d.clone(), egui::FontId::proportional(12.0), WELCOME_DIM);
                }
                p.galley(egui::pos2(x, cy - g.size().y / 2.0), g, WELCOME_DIM);
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
            ui.label(RichText::new("(vide)").weak());
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
fn nav_tab(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let font = egui::FontId::proportional(13.0);
    let size = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size();
    let (rect, resp) = ui.allocate_exact_size(size + egui::vec2(16.0, 14.0), Sense::click());
    let v = ui.visuals();
    let color = if active {
        v.strong_text_color()
    } else if resp.hovered() {
        v.text_color()
    } else {
        v.weak_text_color()
    };
    ui.painter().text(rect.center() - egui::vec2(0.0, 1.0), Align2::CENTER_CENTER, label, font, color);
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

// ---------------------------------------------------------------------- preferences page

/// A theme in the preferences: its background with a few lines of "code" in its colors.
fn theme_card(ui: &mut egui::Ui, name: &str, pv: &highlight::ThemePreview, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(168.0, 92.0), Sense::click());
    let p = ui.painter();
    let stroke = if selected {
        egui::Stroke::new(2.5_f32, ui.visuals().selection.stroke.color)
    } else if resp.hovered() {
        egui::Stroke::new(1.5_f32, ui.visuals().weak_text_color())
    } else {
        egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color)
    };
    p.rect_filled(rect, 8.0, pv.bg);
    // lines of "code"
    let bars: [(f32, f32, Color32); 4] = [(10.0, 50.0, pv.code[0]), (64.0, 60.0, pv.fg), (10.0, 80.0, pv.code[1]), (24.0, 60.0, pv.code[2])];
    for (k, (x, w, c)) in bars.iter().enumerate() {
        let y = rect.top() + 12.0 + k as f32 * 11.0;
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.left() + x, y), egui::vec2(*w, 5.0)), 2.5, *c);
    }
    p.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.right() - 26.0, rect.top() + 10.0), egui::vec2(16.0, 16.0)), 8.0, pv.accent);
    let band = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 26.0), rect.max);
    let band_bg = icons::mix(pv.bg, pv.fg, 0.08);
    p.rect_filled(band, egui::Rounding { nw: 0.0, ne: 0.0, sw: 8.0, se: 8.0 }, band_bg);
    p.with_clip_rect(band.shrink(4.0)).text(band.left_center() + egui::vec2(10.0, 0.0), Align2::LEFT_CENTER, name, egui::FontId::proportional(12.5), pv.fg);
    p.rect_stroke(rect, 8.0, stroke);
    resp
}

/// iOS-like switch.
fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = egui::vec2(36.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let v = ui.visuals();
    let off_bg = v.widgets.inactive.bg_fill;
    let on_bg = v.selection.stroke.color;
    let mix = |a: Color32, b: Color32| {
        let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
        Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
    };
    ui.painter().rect_filled(rect, rect.height() / 2.0, mix(off_bg, on_bg));
    let x = egui::lerp((rect.left() + 10.0)..=(rect.right() - 10.0), t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 7.0, Color32::WHITE);
    resp
}

/// Search filter + section header shown only above the first visible row.
struct RowCtx {
    filter: String,
    header: Option<(String, Color32)>,
}

impl RowCtx {
    fn matches(&self, texts: &[&str]) -> bool {
        let f = self.filter.to_lowercase();
        f.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(&f))
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        if let Some((h, color)) = self.header.take() {
            ui.add_space(14.0);
            ui.label(RichText::new(h).small().strong().color(color));
        }
    }
}

/// One setting: title + explanation on the left, control on the right.
/// Hidden when it does not match the search filter.
fn pref_row(ui: &mut egui::Ui, rc: &mut RowCtx, title: &str, desc: &str, control: impl FnOnce(&mut egui::Ui)) {
    if !rc.matches(&[title, desc]) {
        return;
    }
    rc.header(ui);
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let text_w = (ui.available_width() - 300.0).max(200.0);
        ui.vertical(|ui| {
            ui.set_width(text_w);
            ui.label(RichText::new(title).strong());
            if !desc.is_empty() {
                ui.label(RichText::new(desc).weak().size(12.5));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
    ui.add_space(10.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [egui::pos2(r.left(), r.top()), egui::pos2(r.right(), r.top())],
        egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
}

impl App {
    fn prefs_ui(&mut self, ui: &mut egui::Ui) {
        let panel = ui.visuals().panel_fill;
        egui::SidePanel::left("prefs_nav")
            .exact_width(210.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(panel).inner_margin(egui::Margin::symmetric(12.0, 16.0)))
            .show_inside(ui, |ui| {
                ui.label(RichText::new("Préférences").size(19.0).strong());
                ui.add_space(12.0);
                for (sec, label) in PREF_SECTIONS {
                    let selected = self.prefs.section == sec && self.prefs.filter.is_empty();
                    let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), Sense::click());
                    let v = ui.visuals();
                    if selected {
                        ui.painter().rect_filled(rect, 7.0, v.selection.bg_fill);
                    } else if r.hovered() {
                        ui.painter().rect_filled(rect, 7.0, v.widgets.hovered.weak_bg_fill);
                    }
                    let color = if selected { v.strong_text_color() } else { v.text_color() };
                    ui.painter().text(
                        egui::pos2(rect.left() + 12.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(14.0),
                        color,
                    );
                    if r.clicked() {
                        self.prefs.section = sec;
                        self.prefs.filter.clear();
                    }
                    ui.add_space(2.0);
                }
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(self.hl.colors.bg).inner_margin(egui::Margin::symmetric(28.0, 18.0)))
            .show_inside(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.prefs.filter)
                        .hint_text("Rechercher un réglage…")
                        .desired_width(420.0)
                        .font(egui::FontId::proportional(15.0)),
                );
                ui.add_space(8.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(860.0));
                    let sections: Vec<PrefSection> = if self.prefs.filter.is_empty() {
                        vec![self.prefs.section]
                    } else {
                        PREF_SECTIONS.iter().map(|(s, _)| *s).collect()
                    };
                    let searching = !self.prefs.filter.is_empty();
                    for sec in sections {
                        let name = PREF_SECTIONS.iter().find(|(s, _)| *s == sec).map(|(_, n)| *n).unwrap_or("");
                        let mut rc = RowCtx { filter: self.prefs.filter.clone(), header: None };
                        if searching {
                            rc.header = Some((name.to_uppercase(), self.hl.colors.accent));
                        } else {
                            ui.label(RichText::new(name).size(22.0).strong());
                            ui.add_space(4.0);
                        }
                        self.prefs_section(ui, sec, &mut rc);
                    }
                });
            });
    }

    fn prefs_section(&mut self, ui: &mut egui::Ui, sec: PrefSection, rc: &mut RowCtx) {
        let before = self.settings.clone();
        let mut actions: Vec<Cmd> = Vec::new();
        let s = &mut self.settings;
        match sec {
            PrefSection::Appearance => {
                let themes: Vec<(String, Option<highlight::ThemePreview>)> =
                    self.hl.theme_names().into_iter().map(|n| (n.clone(), self.hl.preview(&n))).collect();
                pref_row(ui, rc, "Thème", "Couleurs de l'éditeur et de l'interface. Les .tmTheme du dossier « themes » de la configuration sont listés aussi.", |ui| {
                    egui::ComboBox::from_id_salt("pref_theme").selected_text(&s.theme).width(220.0).show_ui(ui, |ui| {
                        for (t, _) in &themes {
                            ui.selectable_value(&mut s.theme, t.clone(), t);
                        }
                    });
                });
                if rc.matches(&["Thème", "couleurs", "aperçu"]) {
                    // the themes side by side, with a few colors of each: a click chooses it
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                        for (name, pv) in &themes {
                            let Some(pv) = pv else { continue };
                            if theme_card(ui, name, pv, s.theme == *name).clicked() {
                                s.theme = name.clone();
                            }
                        }
                    });
                    ui.add_space(8.0);
                }
                let theme_accent = self.hl.preview(&s.theme).map(|p| p.accent).unwrap_or(self.hl.colors.accent);
                pref_row(ui, rc, "Couleur d'accentuation", "Boutons principaux, onglet actif, sélection dans les listes. Par défaut celle du thème.", |ui| {
                    let mut c = highlight::parse_hex(&s.accent_color).unwrap_or(theme_accent);
                    if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                        s.accent_color = highlight::to_hex(c);
                    }
                    for (hex, tip) in [("#4F8CFF", "Bleu"), ("#2EA043", "Vert"), ("#D2A8FF", "Violet"), ("#F0883E", "Orange"), ("#E5534B", "Rouge"), ("#39C5CF", "Cyan")] {
                        let col = highlight::parse_hex(hex).unwrap();
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                        ui.painter().circle_filled(r.center(), 8.0, col);
                        if s.accent_color.eq_ignore_ascii_case(hex) {
                            ui.painter().circle_stroke(r.center(), 9.5, egui::Stroke::new(1.5_f32, ui.visuals().strong_text_color()));
                        }
                        if resp.on_hover_text(tip).clicked() {
                            s.accent_color = hex.to_string();
                        }
                    }
                    if !s.accent_color.is_empty() && ui.small_button("↺ Celle du thème").clicked() {
                        s.accent_color.clear();
                    }
                });
                pref_row(ui, rc, "Texte adouci", "Éclaircit les couleurs du texte (code et interface) : le texte paraît moins gras, surtout sur les thèmes clairs.", |ui| {
                    let mut pct = (s.text_softness * 100.0).round();
                    if ui.add(egui::Slider::new(&mut pct, 0.0..=40.0).step_by(5.0).suffix(" %")).changed() {
                        s.text_softness = pct / 100.0;
                    }
                });
                pref_row(ui, rc, "Taille du texte", "Taille de la police de l'éditeur (Ctrl+= / Ctrl+- aussi).", |ui| {
                    ui.add(egui::Slider::new(&mut s.font_size, 9.0..=28.0).step_by(0.5).suffix(" px"));
                });
                if self.font_list.is_none() && !self.font_list_loading {
                    self.font_list_loading = true;
                    self.jobs.spawn(|| JobResult::Fonts(crate::fonts::scan()));
                }
                let fonts = self.font_list.clone();
                let font_combo = |ui: &mut egui::Ui, id: &str, value: &mut String, mono_only: bool, default: &str| {
                    let shown = if value.is_empty() { format!("Par défaut ({default})") } else { value.clone() };
                    egui::ComboBox::from_id_salt(id).selected_text(shown).width(260.0).height(400.0).show_ui(ui, |ui| {
                        ui.selectable_value(value, String::new(), format!("Par défaut ({default})"));
                        match &fonts {
                            None => {
                                ui.spinner();
                            }
                            Some(list) => {
                                for f in list.iter().filter(|f| !mono_only || f.mono) {
                                    ui.selectable_value(value, f.family.clone(), &f.family);
                                }
                            }
                        }
                    });
                };
                let def_editor = crate::fonts::default_editor_font().first().copied().unwrap_or("Hack");
                let def_ui = crate::fonts::default_ui_font().first().copied().unwrap_or("Ubuntu");
                pref_row(ui, rc, "Police de l'éditeur", "Polices à chasse fixe installées sur le système (code, terminal).", |ui| {
                    font_combo(ui, "editor_font", &mut s.editor_font, true, def_editor);
                });
                pref_row(ui, rc, "Police de l'interface", "Menus, panneaux, onglets.", |ui| {
                    font_combo(ui, "ui_font", &mut s.ui_font, false, def_ui);
                });
                pref_row(ui, rc, "Minimap", "Aperçu du fichier à droite de l'éditeur.", |ui| {
                    toggle_switch(ui, &mut s.show_minimap);
                });
                pref_row(ui, rc, "Barre de menus", "Fichier, Édition, Sélection… en haut de la fenêtre. Masquée, les menus restent accessibles par ☰ en bas à gauche.", |ui| {
                    toggle_switch(ui, &mut s.show_menu_bar);
                });
                pref_row(ui, rc, "Fil d'Ariane", "Dossier › fichier › classe › fonction au-dessus de l'éditeur ; clic sur un élément pour y aller.", |ui| {
                    toggle_switch(ui, &mut s.breadcrumbs);
                });
            }
            PrefSection::Editor => {
                pref_row(ui, rc, "Largeur des tabulations", "Nombre de colonnes d'une tabulation et d'un niveau d'indentation.", |ui| {
                    ui.add(egui::Slider::new(&mut s.tab_size, 1..=8));
                });
                pref_row(ui, rc, "Indenter avec des espaces", "Tab insère des espaces plutôt qu'un caractère de tabulation (les fichiers existants gardent leur style).", |ui| {
                    toggle_switch(ui, &mut s.translate_tabs_to_spaces);
                });
                pref_row(ui, rc, "Fermeture automatique", "Ferme automatiquement ( [ { \" ' ` et entoure la sélection.", |ui| {
                    toggle_switch(ui, &mut s.auto_pair);
                });
                pref_row(ui, rc, "Supprimer les espaces en fin de ligne", "À chaque enregistrement.", |ui| {
                    toggle_switch(ui, &mut s.trim_trailing_whitespace_on_save);
                });
                pref_row(ui, rc, "Serveurs de langage (LSP)", "Complétion (Ctrl+Espace), aller à la définition (F12), renommer (F2). Serveurs détectés : pyright / pylsp, typescript-language-server, rust-analyzer, gopls, clangd.", |ui| {
                    toggle_switch(ui, &mut s.lsp);
                });
                pref_row(ui, rc, "Recherche dans le dossier en onglet", "Ctrl+Shift+F ouvre la recherche dans un onglet de l'éditeur plutôt que dans le panneau du bas (boutons ⇱ / ⇲ pour changer).", |ui| {
                    toggle_switch(ui, &mut s.search_in_tab);
                });
                pref_row(ui, rc, "Garder le travail non enregistré", "À la fermeture (ou en changeant de projet), les modifications non enregistrées sont gardées et rouvertes ensuite, sans question.", |ui| {
                    toggle_switch(ui, &mut s.hot_exit);
                });
            }
            PrefSection::Lint => {
                pref_row(ui, rc, "Vérifier pendant la frappe", "Les fichiers Python sont vérifiés une demi-seconde après la dernière touche, sans enregistrer.", |ui| {
                    toggle_switch(ui, &mut s.lint_live);
                });
                pref_row(ui, rc, "Vérifier à l'enregistrement", "Lance le linter (Python, JavaScript) à chaque enregistrement.", |ui| {
                    toggle_switch(ui, &mut s.lint_on_save);
                });
                let status: Vec<(&str, bool)> =
                    ["flake8", "ruff", "pyflakes", "pylint"].iter().map(|n| (*n, lint::is_installed(n))).collect();
                pref_row(ui, rc, "Linter Python", "« Automatique » prend ruff, sinon flake8, sinon pyflakes. La configuration du projet (setup.cfg, .flake8, tox.ini, pyproject.toml) est lue.", |ui| {
                    let label = |n: &str| match n {
                        "auto" => "Automatique".to_string(),
                        n => {
                            let ok = status.iter().any(|(m, i)| *m == n && *i);
                            format!("{n}{}", if ok { " — installé" } else { " — non installé" })
                        }
                    };
                    egui::ComboBox::from_id_salt("pref_linter").selected_text(label(&s.python_linter)).width(220.0).show_ui(ui, |ui| {
                        for n in ["auto", "flake8", "ruff", "pyflakes", "pylint"] {
                            ui.selectable_value(&mut s.python_linter, n.to_string(), label(n));
                        }
                    });
                });
                let args = &mut self.prefs.lint_args;
                pref_row(ui, rc, "Arguments du linter", "Ajoutés à la ligne de commande, séparés par des espaces. Ex. : --max-line-length=120 --ignore=E203", |ui| {
                    let r = ui.add(egui::TextEdit::singleline(args).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text("--max-line-length=120"));
                    if r.lost_focus() || r.changed() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        s.python_lint_args = args.split_whitespace().map(String::from).collect();
                    }
                });
                let installed = status.iter().find(|(n, _)| *n == "flake8").is_some_and(|(_, i)| *i);
                pref_row(ui, rc, "flake8", &format!("Installé dans {} (sans toucher au Python du système).", lint::tools_dir().display()), |ui| {
                    if installed {
                        ui.label(RichText::new("Installé").color(ok()));
                        if ui.button("Mettre à jour").clicked() {
                            actions.push(Cmd::InstallFlake8);
                        }
                    } else if ui.button("Installer flake8").clicked() {
                        actions.push(Cmd::InstallFlake8);
                    }
                });
            }
            PrefSection::Git => {
                pref_row(ui, rc, "Branche et remote dans l'arborescence", "Sur le dossier de chaque dépôt git : sa branche, la branche distante suivie (↑ à pousser, ↓ à récupérer) et le nombre de fichiers modifiés ; survol : les remotes.", |ui| {
                    toggle_switch(ui, &mut s.tree_git_info);
                });
                pref_row(ui, rc, "Diff dans l'éditeur", "Lignes ajoutées en vert et anciennes lignes en rouge directement dans le code (Ctrl+Alt+D).", |ui| {
                    toggle_switch(ui, &mut s.inline_diff);
                });
                let mut blame = self.blame_mode;
                pref_row(ui, rc, "Blame de la ligne", "Auteur et commit de la ligne courante en fin de ligne (Ctrl+B). Non mémorisé entre les sessions.", |ui| {
                    if toggle_switch(ui, &mut blame).changed() {
                        actions.push(Cmd::GitBlame);
                    }
                });
            }
            PrefSection::Terminal => {
                let shell = &mut self.prefs.shell;
                pref_row(ui, rc, "Shell", "Shell lancé dans le terminal intégré (ex. bash, zsh, fish, powershell). Vide : $SHELL (Linux / macOS) ou cmd (Windows). Pris en compte pour les nouveaux terminaux.", |ui| {
                    let r = ui.add(egui::TextEdit::singleline(shell).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text("/bin/bash"));
                    if r.lost_focus() {
                        s.shell = (!shell.trim().is_empty()).then(|| shell.trim().to_string());
                    }
                });
            }
            PrefSection::Keys => {
                let mut key_action = None;
                if rc.filter.is_empty() {
                    ui.label(
                        RichText::new("Clique sur un raccourci puis appuie sur la nouvelle combinaison (Ctrl+K puis une touche pour un enchaînement). Échap annule.")
                            .weak()
                            .size(12.5),
                    );
                    ui.add_space(6.0);
                }
                if let Some((cmd, seq)) = &self.prefs.proposed {
                    rc.header(ui);
                    let other = self.keymap.lookup_seq(seq);
                    let name = |c: Cmd| self.cmd_name(c);
                    egui::Frame::none()
                        .stroke(egui::Stroke::new(1.0_f32, warn()))
                        .rounding(8.0)
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.label(format!(
                                "« {} » est déjà utilisé par « {} ». L'attribuer à « {} » ?",
                                keymap::seq_label(seq),
                                other.map(name).unwrap_or_default(),
                                name(*cmd)
                            ));
                            ui.horizontal(|ui| {
                                if ui.button(RichText::new("Remplacer").color(warn())).clicked() {
                                    key_action = Some(KeyAction::Confirm);
                                }
                                if ui.button("Annuler").clicked() {
                                    key_action = Some(KeyAction::Cancel);
                                }
                            });
                        });
                    ui.add_space(8.0);
                }
                let capturing = self.prefs.capture;
                for (id, label, cmd) in commands::ALL.iter().filter(|(_, l, _)| !l.is_empty()) {
                    let keys = self.keymap.label(*cmd);
                    if !rc.matches(&[label, &keys, id]) {
                        continue;
                    }
                    rc.header(ui);
                    let customized = self.shortcut_customized(*cmd);
                    ui.horizontal(|ui| {
                        let label_w = (ui.available_width() - 420.0).max(220.0);
                        // fixed-width label column so the shortcut chips line up
                        let (r, _) = ui.allocate_exact_size(egui::vec2(label_w, 28.0), Sense::hover());
                        ui.painter().with_clip_rect(r).text(
                            egui::pos2(r.left(), r.center().y),
                            Align2::LEFT_CENTER,
                            *label,
                            egui::FontId::proportional(14.0),
                            ui.visuals().text_color(),
                        );
                        let this = capturing.filter(|c| c.cmd == *cmd);
                        let (text, color) = match this {
                            Some(c) => (
                                match c.first {
                                    Some(f) => format!("{} …", f.label()),
                                    None => "Appuyez sur une combinaison…".to_string(),
                                },
                                self.hl.colors.accent,
                            ),
                            None if keys.is_empty() => ("Ajouter".to_string(), ui.visuals().weak_text_color()),
                            None => (keys.clone(), ui.visuals().strong_text_color()),
                        };
                        let chip = egui::Button::new(RichText::new(text).monospace().color(color))
                            .fill(ui.visuals().widgets.inactive.bg_fill)
                            .min_size(egui::vec2(210.0, 24.0));
                        if ui.add(chip).on_hover_text("Cliquer pour changer").clicked() {
                            key_action = Some(KeyAction::Capture(*cmd));
                        }
                        if !keys.is_empty() && ui.button("×").on_hover_text("Supprimer ce raccourci").clicked() {
                            key_action = Some(KeyAction::Unbind(*cmd));
                        }
                        if customized {
                            if ui.button("↺").on_hover_text("Revenir au raccourci par défaut").clicked() {
                                key_action = Some(KeyAction::Reset(*cmd));
                            }
                            ui.label(RichText::new("modifié").small().color(self.hl.colors.accent));
                        }
                        ui.label(RichText::new(*id).weak().small());
                    });
                }
                if let Some(a) = key_action {
                    self.prefs.key_action = Some(a);
                }
            }
            PrefSection::Advanced => {
                pref_row(ui, rc, "Fichier de préférences", &config::settings_path().display().to_string(), |ui| {
                    if ui.button("Ouvrir settings.toml").clicked() {
                        actions.push(Cmd::OpenSettingsFile);
                    }
                });
                pref_row(ui, rc, "Plugins", &config::plugins_dir().display().to_string(), |ui| {
                    if ui.button("Recharger").clicked() {
                        actions.push(Cmd::ReloadPlugins);
                    }
                    if ui.button("Ouvrir le dossier").clicked() {
                        actions.push(Cmd::OpenPluginsFolder);
                    }
                });
                pref_row(ui, rc, "Historique de recherche", "Recherches et filtres mémorisés.", |ui| {
                    if ui.button("Effacer").clicked() {
                        actions.push(Cmd::ClearSearchHistory);
                    }
                });
            }
        }
        self.pending.extend(actions);
        self.apply_settings_change(&before);
        if let Some(a) = self.prefs.key_action.take() {
            match a {
                KeyAction::Capture(cmd) => {
                    self.prefs.proposed = None;
                    self.prefs.capture = Some(Capture { cmd, first: None });
                    // no text field must receive the keys being recorded
                    if let Some(id) = ui.ctx().memory(|m| m.focused()) {
                        ui.ctx().memory_mut(|m| m.surrender_focus(id));
                    }
                }
                KeyAction::Unbind(cmd) => self.unbind_shortcut(cmd),
                KeyAction::Reset(cmd) => self.reset_shortcut(cmd),
                KeyAction::Confirm => {
                    if let Some((cmd, seq)) = self.prefs.proposed.take() {
                        self.bind_shortcut(cmd, seq);
                    }
                }
                KeyAction::Cancel => self.prefs.proposed = None,
            }
        }
    }

    // ------------------------------------------------------------------ editable shortcuts

    /// Human name of a command (palette label, or "plugin : title").
    fn cmd_name(&self, cmd: Cmd) -> String {
        if let Cmd::Plugin(k) = cmd {
            if let Some(&(pi, ci)) = self.plugin_cmds.get(k) {
                return format!("plugin {} : {}", self.plugins[pi].name, self.plugins[pi].commands[ci].title);
            }
        }
        commands::ALL.iter().find(|(_, _, c)| *c == cmd).map(|(_, l, _)| l.to_string()).unwrap_or_else(|| format!("{cmd:?}"))
    }

    fn write_key(&mut self, key: String, value: Option<String>) {
        if let Err(e) = config::set_key(&key, value.as_deref()) {
            self.log(format!("Raccourci non enregistré : {e}"));
        }
        match value {
            Some(v) => {
                self.settings.keys.insert(key, v);
            }
            None => {
                self.settings.keys.remove(&key);
            }
        }
    }

    /// [keys] entries (exact text) describing `seq`.
    fn user_entries_for(&self, seq: &[Combo]) -> Vec<String> {
        self.settings.keys.keys().filter(|k| keymap::parse_seq(k).as_deref() == Some(seq)).cloned().collect()
    }

    fn shortcut_customized(&self, cmd: Cmd) -> bool {
        let Some(id) = commands::id_of(cmd) else { return false };
        let defaults = Keymap::default_seqs(cmd);
        self.settings
            .keys
            .iter()
            .any(|(k, v)| v == id || (v == UNBOUND && keymap::parse_seq(k).is_some_and(|s| defaults.contains(&s))))
    }

    /// Removes every current shortcut of `cmd` (built-in ones are disabled with "none").
    fn clear_shortcuts_of(&mut self, cmd: Cmd) {
        let defaults = Keymap::default_seqs(cmd);
        for seq in self.keymap.seqs_of(cmd) {
            for k in self.user_entries_for(&seq) {
                self.write_key(k, None);
            }
            if defaults.contains(&seq) {
                self.write_key(keymap::seq_config(&seq), Some(UNBOUND.to_string()));
            }
        }
    }

    /// Called when a combination was recorded: binds it, or asks first if it is taken.
    fn propose_shortcut(&mut self, cmd: Cmd, seq: Seq) {
        match self.keymap.lookup_seq(&seq) {
            Some(other) if other == cmd => self.set_status("C'est déjà ce raccourci"),
            Some(_) => self.prefs.proposed = Some((cmd, seq)),
            None => self.bind_shortcut(cmd, seq),
        }
    }

    fn bind_shortcut(&mut self, cmd: Cmd, seq: Seq) {
        let Some(id) = commands::id_of(cmd) else { return };
        self.clear_shortcuts_of(cmd);
        for k in self.user_entries_for(&seq) {
            self.write_key(k, None);
        }
        self.write_key(keymap::seq_config(&seq), Some(id.to_string()));
        self.rebuild_keymap();
        self.set_status(format!("Raccourci {} enregistré", keymap::seq_label(&seq)));
    }

    fn unbind_shortcut(&mut self, cmd: Cmd) {
        self.clear_shortcuts_of(cmd);
        self.rebuild_keymap();
    }

    fn reset_shortcut(&mut self, cmd: Cmd) {
        let Some(id) = commands::id_of(cmd) else { return };
        let defaults = Keymap::default_seqs(cmd);
        let entries: Vec<String> = self
            .settings
            .keys
            .iter()
            .filter(|(k, v)| *v == id || (*v == UNBOUND && keymap::parse_seq(k).is_some_and(|s| defaults.contains(&s))))
            .map(|(k, _)| k.clone())
            .collect();
        for k in entries {
            self.write_key(k, None);
        }
        self.rebuild_keymap();
        self.set_status("Raccourci par défaut rétabli");
    }

    /// Saves changed settings to settings.toml and applies their effects right away.
    fn apply_settings_change(&mut self, before: &Settings) {
        let s = self.settings.clone();
        if s.theme != before.theme {
            if self.hl.set_theme(&s.theme) {
                self.visuals_dirty = true;
            }
            self.persist("theme", config::toml_str(&s.theme));
        }
        if s.font_size != before.font_size {
            self.persist("font_size", format!("{:.1}", s.font_size));
        }
        if s.accent_color != before.accent_color || s.text_softness != before.text_softness {
            self.hl.set_overrides(highlight::parse_hex(&s.accent_color), s.text_softness);
            self.visuals_dirty = true;
            self.persist("accent_color", config::toml_str(&s.accent_color));
            self.persist("text_softness", format!("{:.2}", s.text_softness));
        }
        if s.editor_font != before.editor_font || s.ui_font != before.ui_font {
            self.egui_ctx.set_fonts(crate::fonts::definitions(&s.editor_font, &s.ui_font));
            self.persist("editor_font", config::toml_str(&s.editor_font));
            self.persist("ui_font", config::toml_str(&s.ui_font));
        }
        if s.tab_size != before.tab_size || s.translate_tabs_to_spaces != before.translate_tabs_to_spaces {
            for d in self.docs.iter_mut() {
                d.tab_size = s.tab_size;
            }
            self.persist("tab_size", s.tab_size.to_string());
            self.persist("translate_tabs_to_spaces", s.translate_tabs_to_spaces.to_string());
        }
        let bools = [
            ("show_minimap", s.show_minimap, before.show_minimap),
            ("breadcrumbs", s.breadcrumbs, before.breadcrumbs),
            ("show_menu_bar", s.show_menu_bar, before.show_menu_bar),
            ("search_in_tab", s.search_in_tab, before.search_in_tab),
            ("tree_git_info", s.tree_git_info, before.tree_git_info),
            ("auto_pair", s.auto_pair, before.auto_pair),
            ("trim_trailing_whitespace_on_save", s.trim_trailing_whitespace_on_save, before.trim_trailing_whitespace_on_save),
            ("hot_exit", s.hot_exit, before.hot_exit),
            ("lsp", s.lsp, before.lsp),
            ("lint_live", s.lint_live, before.lint_live),
            ("lint_on_save", s.lint_on_save, before.lint_on_save),
            ("inline_diff", s.inline_diff, before.inline_diff),
        ];
        for (key, now, was) in bools {
            if now != was {
                self.persist(key, now.to_string());
            }
        }
        let lint_changed = s.python_linter != before.python_linter || s.python_lint_args != before.python_lint_args || s.lint_live != before.lint_live;
        if s.python_linter != before.python_linter {
            self.persist("python_linter", config::toml_str(&s.python_linter));
        }
        if s.python_lint_args != before.python_lint_args {
            self.persist("python_lint_args", config::toml_str_array(&s.python_lint_args));
        }
        if lint_changed {
            for d in self.docs.iter_mut() {
                d.lint_version = u64::MAX; // re-lint with the new settings
            }
        }
        if s.lsp != before.lsp || s.lsp_servers != before.lsp_servers {
            self.lsp.reset();
            self.lsp_note.clear();
            self.completion = None;
        }
        if s.shell != before.shell {
            self.persist("shell", config::toml_str(s.shell.as_deref().unwrap_or("")));
        }
    }
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
