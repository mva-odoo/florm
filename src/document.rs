use crate::commands::Cmd;
use crate::diff::{self, Hunk};
use crate::highlight::{HlCache, Highlighting};
use crate::lint::Diagnostic;
use ropey::Rope;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

static CONTENT_IDS: AtomicU64 = AtomicU64::new(1);

fn next_content_id() -> u64 {
    CONTENT_IDS.fetch_add(1, Ordering::Relaxed)
}

/// A selection in char indices. `anchor == head` is a plain cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sel {
    pub anchor: usize,
    pub head: usize,
    /// Visual column to keep when moving vertically.
    pub goal: Option<usize>,
}

impl Sel {
    pub fn cursor(pos: usize) -> Self {
        Sel { anchor: pos, head: pos, goal: None }
    }
    pub fn range(anchor: usize, head: usize) -> Self {
        Sel { anchor, head, goal: None }
    }
    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }
    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    Home,
    End,
    PageUp,
    PageDown,
    DocStart,
    DocEnd,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Insert,
    Delete,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitMark {
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Copy, Debug)]
pub enum Drag {
    Normal(usize),
    Line(usize),
    Column(usize, usize),
}

/// One replacement per selection; `anchor`/`head` are offsets of the resulting
/// selection relative to the start of the inserted text.
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub anchor: usize,
    pub head: usize,
}

impl Edit {
    fn delete(start: usize, end: usize) -> Self {
        Edit { start, end, text: String::new(), anchor: 0, head: 0 }
    }
    fn stay(pos: usize, offset: usize) -> Self {
        Edit { start: pos, end: pos, text: String::new(), anchor: offset, head: offset }
    }
    fn insert(start: usize, end: usize, text: String) -> Self {
        let n = text.chars().count();
        Edit { start, end, text, anchor: n, head: n }
    }
}

struct Snapshot {
    rope: Rope,
    sels: Vec<Sel>,
    content: u64,
}

pub struct Document {
    pub uid: u64,
    pub rope: Rope,
    pub path: Option<PathBuf>,
    pub title_override: Option<String>,
    pub read_only: bool,
    pub sels: Vec<Sel>,
    pub primary: usize,
    /// Bumped on every change (cache key).
    pub version: u64,
    content_id: u64,
    saved_content: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: Option<(EditKind, Instant)>,
    pub last_change: Instant,
    pub syntax: String,
    pub highlight: HlCache,
    pub diagnostics: Vec<Diagnostic>,
    pub lint_version: u64,
    pub git_marks: HashMap<usize, GitMark>,
    /// File content in HEAD, the live diff is computed against it.
    pub git_base: Option<String>,
    pub hunks: Vec<Hunk>,
    pub diff_version: u64,
    /// Gutter line of the hunk whose inline diff is shown.
    pub peek: Option<usize>,
    /// Set by the inline diff popup to ask the app to switch to the inline diff view.
    pub request_inline_diff: bool,
    /// Symbol under the mouse pointer (char range), set by the editor view.
    pub hover_word: Option<(usize, usize)>,
    /// Lines (first, last) the user asked to give to Claude (✳ in the gutter).
    pub claude_pick: Option<(usize, usize)>,
    /// What the app found about a hovered symbol (its range, the text of the tooltip).
    pub hover_info: Option<((usize, usize), String)>,
    /// Where the hovered symbol is defined in the project (like Sublime Text): its range, the
    /// definitions (file, 0-based line, what, where) and how many more there are.
    pub hover_defs: Option<((usize, usize), Vec<(PathBuf, usize, String, String)>, usize)>,
    /// How the long lines are cut when the soft wrap is on.
    pub wraps: crate::wrap::Wraps,
    /// A definition clicked in the hover popup: the app opens it.
    pub goto_request: Option<(PathBuf, usize)>,
    /// Inline blame annotation: (line, document version, text).
    pub blame: Option<(usize, u64, String)>,
    pub mtime: Option<SystemTime>,
    pub crlf: bool,
    /// Encoding of the file on disk: kept when saving.
    pub encoding: Encoding,
    pub tab_size: usize,
    pub use_spaces: bool,
    // view state
    pub scroll_to_cursor: bool,
    pub center_cursor: bool,
    pub scroll_request: Option<f32>,
    pub scroll_y: f32,
    pub view_h: f32,
    pub page_lines: usize,
    pub drag: Option<Drag>,
    pub max_cols: usize,
    max_cols_version: u64,
    /// Char range picked in the search results, highlighted while it stays selected.
    pub flash: Option<(usize, usize, Instant)>,
    /// Editor panel showing the document (0 or 1 when split), `NO_PANE` until the app places it.
    pub pane: u8,
    /// Symbols of the text: (version, syntax, symbols).
    symbols_cache: Option<(u64, String, Vec<crate::symbols::Symbol>)>,
    /// Folded blocks: start of their first line (char position, follows the edits).
    pub folds: Vec<usize>,
    /// Bookmarked lines, as the char position of their start (moved by the edits, like folds).
    pub bookmarks: Vec<usize>,
    /// Range searched by Ctrl+F "dans la sélection" (tinted in the editor).
    pub find_scope: Option<(usize, usize)>,
    conflicts_cache: Option<(u64, Vec<Conflict>)>,
    /// Merge conflicts were seen in this document (offer "mark as resolved" once they are gone).
    pub had_conflicts: bool,
    pub view: View,
    /// Image zoom (0 = fit to the window).
    pub zoom: f32,
    /// Screen position (x, bottom) of the primary cursor when drawn (completion popup).
    pub caret_screen: Option<(f32, f32)>,
}

pub const NO_PANE: u8 = u8::MAX;

/// What a tab shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    #[default]
    Text,
    /// The image file at `path`.
    Image,
    /// Rendered Markdown of the document with this uid (updated while it is edited).
    Preview(u64),
    /// The "find in files" panel, in a tab.
    Search,
    /// An integrated terminal (its session id), in a tab.
    Terminal(u64),
    /// The guide of Florm (assets/guide.md).
    Help,
    /// The welcome page (logo, start actions, recent projects).
    Welcome,
    /// The pull requests of GitHub.
    PullRequests,
    /// Conversation with Claude Code, and the review of its changes.
    Claude,
}

/// Files shown as images rather than text.
pub fn is_image(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg")
}

/// A merge conflict block: lines of its markers (`<<<<<<<`, optional `|||||||`, `=======`, `>>>>>>>`).
#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    pub start: usize,
    pub base: Option<usize>,
    pub mid: usize,
    pub end: usize,
    /// Labels after the markers ("HEAD", "feature/x").
    pub ours: String,
    pub theirs: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resolution {
    Ours,
    Theirs,
    Both,
}

/// Conflict blocks of a text given as lines.
pub fn find_conflicts<'a>(lines: impl Iterator<Item = std::borrow::Cow<'a, str>>) -> Vec<Conflict> {
    let mut out = Vec::new();
    let mut cur: Option<(usize, Option<usize>, Option<usize>, String)> = None;
    for (i, l) in lines.enumerate() {
        let l = l.trim_end_matches(['\n', '\r']);
        if let Some(label) = l.strip_prefix("<<<<<<<") {
            cur = Some((i, None, None, label.trim().to_string()));
        } else if let Some((s, base, mid, ours)) = cur.as_mut() {
            if l.starts_with("|||||||") && mid.is_none() {
                *base = Some(i);
            } else if l.starts_with("=======") && mid.is_none() {
                *mid = Some(i);
            } else if let Some(label) = l.strip_prefix(">>>>>>>") {
                if let Some(m) = *mid {
                    out.push(Conflict { start: *s, base: *base, mid: m, end: i, ours: ours.clone(), theirs: label.trim().to_string() });
                }
                cur = None;
            }
        }
    }
    out
}

fn char_class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn pair_of(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

fn map_pos(pos: usize, spans: &[(usize, usize, usize)]) -> usize {
    let mut d: isize = 0;
    for &(s, e, n) in spans {
        if pos >= e {
            d += n as isize - (e - s) as isize;
        } else if pos > s {
            return (s as isize + d) as usize + n;
        } else {
            break;
        }
    }
    (pos as isize + d) as usize
}

/// Lines (0-based, inclusive) of a "go to line and select" request: "42", "10-20" (or "10..20",
/// "10 20"), "42:7" (column ignored); empty means the current line. Clamped to the file.
pub fn parse_line_range(spec: &str, nlines: usize, current: usize) -> Option<(usize, usize)> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Some((current, current));
    }
    let n = nlines.max(1);
    let one = |s: &str| s.trim().split(':').next()?.trim().parse::<usize>().ok().map(|l| l.clamp(1, n) - 1);
    let parts: Vec<&str> = spec.split(['-', ' ']).flat_map(|p| p.split("..")).filter(|p| !p.trim().is_empty()).collect();
    match parts.as_slice() {
        [a] => one(a).map(|l| (l, l)),
        [a, b] => {
            let (a, b) = (one(a)?, one(b)?);
            Some((a.min(b), a.max(b)))
        }
        _ => None,
    }
}

/// Adapts a replacement to the case of the text it replaces: chat→chien, Chat→Chien, CHAT→CHIEN.
pub fn preserve_case(matched: &str, repl: &str) -> String {
    let letters: Vec<char> = matched.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return repl.to_string();
    }
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return repl.to_uppercase();
    }
    if letters.iter().all(|c| c.is_lowercase()) {
        return repl.to_lowercase();
    }
    if letters[0].is_uppercase() && letters[1..].iter().all(|c| c.is_lowercase()) {
        let mut it = repl.chars();
        return match it.next() {
            Some(f) => f.to_uppercase().chain(it.flat_map(|c| c.to_lowercase())).collect(),
            None => String::new(),
        };
    }
    repl.to_string()
}

/// Replacements (char start, char end, new text) for the matches of `re` inside `scope` (char range).
pub fn replacements(
    rope: &Rope,
    re: &regex::Regex,
    repl: &str,
    regex_mode: bool,
    preserve: bool,
    scope: Option<(usize, usize)>,
) -> Vec<(usize, usize, String)> {
    let text = rope.to_string();
    let mut out = Vec::new();
    for caps in re.captures_iter(&text) {
        let m = caps.get(0).unwrap();
        if m.start() == m.end() {
            continue;
        }
        let (a, b) = (rope.byte_to_char(m.start()), rope.byte_to_char(m.end()));
        if scope.is_some_and(|(s, e)| a < s || b > e) {
            continue;
        }
        out.push((a, b, crate::search::expand_replacement(&caps, repl, regex_mode, preserve)));
    }
    out
}

pub fn build_regex(query: &str, case: bool, regex: bool, whole: bool) -> Result<regex::Regex, String> {
    let pat = if regex { query.to_string() } else { regex::escape(query) };
    let pat = if whole { format!(r"\b(?:{pat})\b") } else { pat };
    regex::RegexBuilder::new(&pat)
        .case_insensitive(!case)
        .multi_line(true)
        .build()
        .map_err(|e| e.to_string())
}

impl Document {
    pub fn new(uid: u64, tab_size: usize, use_spaces: bool) -> Self {
        let cid = next_content_id();
        Document {
            uid,
            rope: Rope::new(),
            path: None,
            title_override: None,
            read_only: false,
            sels: vec![Sel::cursor(0)],
            primary: 0,
            version: 0,
            content_id: cid,
            saved_content: cid,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: None,
            last_change: Instant::now(),
            syntax: "Plain Text".into(),
            highlight: HlCache::default(),
            diagnostics: Vec::new(),
            lint_version: 0,
            git_marks: HashMap::new(),
            git_base: None,
            hunks: Vec::new(),
            diff_version: u64::MAX,
            peek: None,
            request_inline_diff: false,
            hover_word: None,
            claude_pick: None,
            hover_info: None,
            hover_defs: None,
            wraps: Default::default(),
            goto_request: None,
            blame: None,
            mtime: None,
            crlf: false,
            encoding: Encoding::Utf8,
            tab_size: tab_size.max(1),
            use_spaces,
            scroll_to_cursor: false,
            center_cursor: false,
            scroll_request: None,
            scroll_y: 0.0,
            view_h: 600.0,
            page_lines: 30,
            drag: None,
            max_cols: 0,
            max_cols_version: u64::MAX,
            flash: None,
            pane: NO_PANE,
            symbols_cache: None,
            folds: Vec::new(),
            bookmarks: Vec::new(),
            find_scope: None,
            conflicts_cache: None,
            had_conflicts: false,
            view: View::Text,
            zoom: 0.0,
            caret_screen: None,
        }
    }

    pub fn from_text(uid: u64, text: &str, tab_size: usize, use_spaces: bool) -> Self {
        let mut d = Self::new(uid, tab_size, use_spaces);
        d.rope = Rope::from_str(text);
        d
    }

    pub fn open(uid: u64, path: &Path, tab_size: usize, use_spaces: bool, hl: &Highlighting) -> Result<Self, String> {
        let mut d = Self::new(uid, tab_size, use_spaces);
        d.path = Some(path.to_path_buf());
        if is_image(path) && path.exists() {
            d.view = View::Image;
            d.read_only = true;
            return Ok(d);
        }
        if path.exists() {
            let (text, encoding) = read_text(path)?;
            d.encoding = encoding;
            d.crlf = text.contains("\r\n");
            d.rope = Rope::from_str(&text.replace("\r\n", "\n"));
            d.mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            d.detect_indent();
        }
        let first: String = d.rope.line(0).chars().take(200).collect();
        d.syntax = hl.detect(Some(path), &first);
        Ok(d)
    }

    /// Follow the file's own indentation style when it is obvious.
    fn detect_indent(&mut self) {
        let (mut tabs, mut spaces) = (0, 0);
        for line in self.rope.lines().take(2000) {
            match line.chars().next() {
                Some('\t') => tabs += 1,
                Some(' ') => spaces += 1,
                _ => {}
            }
        }
        if tabs > spaces * 2 {
            self.use_spaces = false;
        } else if spaces > tabs * 2 {
            self.use_spaces = true;
        }
    }

    pub fn title(&self) -> String {
        if let Some(t) = &self.title_override {
            return t.clone();
        }
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sans titre".into())
    }

    pub fn is_dirty(&self) -> bool {
        !self.read_only && self.content_id != self.saved_content
    }

    pub fn is_pristine_empty(&self) -> bool {
        self.path.is_none() && !self.is_dirty() && self.rope.len_chars() == 0
    }

    pub fn save(&mut self, trim_trailing: bool) -> Result<(), String> {
        let path = self.path.clone().ok_or("aucun chemin")?;
        if trim_trailing {
            self.trim_trailing_whitespace();
        }
        let mut text = self.rope.to_string();
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        let bytes = self.encoding.encode(&text)?;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        self.saved_content = self.content_id;
        self.mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        Ok(())
    }

    /// Replaces the whole text (formatter): one undo step, the cursor stays on its line and
    /// column. Returns false when nothing changed.
    pub fn replace_all(&mut self, text: &str) -> bool {
        if text == self.rope.to_string() {
            return false;
        }
        let head = self.primary_sel().head;
        let line = self.rope.char_to_line(head);
        let col = head - self.rope.line_to_char(line);
        self.checkpoint(EditKind::Other);
        let len = self.rope.len_chars();
        self.apply(&mut [(0, len, text.to_string())]);
        self.normalize();
        self.sels = vec![Sel::cursor(self.pos_of(line, col))];
        self.primary = 0;
        self.scroll_to_cursor = true;
        true
    }

    /// Saves in UTF-8 from now on (the document becomes modified).
    pub fn convert_to_utf8(&mut self) {
        if self.encoding != Encoding::Utf8 {
            self.encoding = Encoding::Utf8;
            self.saved_content = u64::MAX;
        }
    }

    pub fn reload_from_disk(&mut self) -> Result<(), String> {
        let path = self.path.clone().ok_or("aucun chemin")?;
        let (text, encoding) = read_text(&path)?;
        self.encoding = encoding;
        self.crlf = text.contains("\r\n");
        let text = text.replace("\r\n", "\n");
        if text != self.rope.to_string() {
            self.checkpoint(EditKind::Other);
            let len = self.rope.len_chars();
            let mut e = vec![(0, len, text)];
            self.apply(&mut e);
            self.normalize();
        }
        self.saved_content = self.content_id;
        self.mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        Ok(())
    }

    // ------------------------------------------------------------------ helpers

    pub fn len(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn char_at(&self, p: usize) -> Option<char> {
        (p < self.rope.len_chars()).then(|| self.rope.char(p))
    }

    pub fn slice(&self, a: usize, b: usize) -> String {
        self.rope.slice(a..b).to_string()
    }

    pub fn line_len(&self, line: usize) -> usize {
        let l = self.rope.line(line);
        let n = l.len_chars();
        if n > 0 && l.char(n - 1) == '\n' {
            n - 1
        } else {
            n
        }
    }

    pub fn line_text(&self, line: usize) -> String {
        let s = self.rope.line_to_char(line);
        self.slice(s, s + self.line_len(line))
    }

    /// Start of the next line (or end of buffer).
    pub fn line_end_incl(&self, line: usize) -> usize {
        if line + 1 < self.rope.len_lines() {
            self.rope.line_to_char(line + 1)
        } else {
            self.len()
        }
    }

    pub fn indent_len(&self, line: usize) -> usize {
        self.rope.line(line).chars().take(self.line_len(line)).take_while(|c| *c == ' ' || *c == '\t').count()
    }

    pub fn line_col(&self, pos: usize) -> (usize, usize) {
        let line = self.rope.char_to_line(pos);
        (line, pos - self.rope.line_to_char(line))
    }

    pub fn pos_of(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.rope.len_lines() - 1);
        self.rope.line_to_char(line) + col.min(self.line_len(line))
    }

    /// Visual column (tabs expanded) of `col` in `line`.
    pub fn vcol(&self, line: usize, col: usize) -> usize {
        let ts = self.tab_size;
        let mut v = 0;
        for c in self.rope.line(line).chars().take(col) {
            v = if c == '\t' { (v / ts + 1) * ts } else { v + 1 };
        }
        v
    }

    /// Char column whose visual position contains `target`.
    pub fn col_at_vcol(&self, line: usize, target: usize) -> usize {
        let ts = self.tab_size;
        let len = self.line_len(line);
        let mut v = 0;
        for (i, c) in self.rope.line(line).chars().take(len).enumerate() {
            let nv = if c == '\t' { (v / ts + 1) * ts } else { v + 1 };
            if nv > target {
                return i;
            }
            v = nv;
        }
        len
    }

    pub fn update_max_cols(&mut self) {
        if self.max_cols_version != self.version {
            self.max_cols = self.rope.lines().map(|l| l.len_chars()).max().unwrap_or(0);
            self.max_cols_version = self.version;
        }
    }

    pub fn primary_sel(&self) -> Sel {
        self.sels[self.primary.min(self.sels.len() - 1)]
    }

    pub fn indent_unit(&self) -> String {
        if self.use_spaces {
            " ".repeat(self.tab_size)
        } else {
            "\t".into()
        }
    }

    /// The symbol at `pos`: an Odoo model or xml id between quotes ('sale.order',
    /// "base.group_user"), else the identifier. None outside of a word.
    pub fn symbol_at(&self, pos: usize) -> Option<(usize, usize)> {
        let line = self.rope.char_to_line(pos.min(self.rope.len_chars()));
        let ls = self.rope.line_to_char(line);
        let chars: Vec<char> = self.line_text(line).chars().collect();
        let col = (pos - ls).min(chars.len());
        let quote = |c: char| c == '\'' || c == '"';
        let (mut a, mut b) = (col, col);
        while a > 0 && !quote(chars[a - 1]) && !chars[a - 1].is_whitespace() {
            a -= 1;
        }
        while b < chars.len() && !quote(chars[b]) && !chars[b].is_whitespace() {
            b += 1;
        }
        if a > 0 && b < chars.len() && quote(chars[a - 1]) && quote(chars[b]) && chars[a..b].contains(&'.')
            && chars[a..b].iter().all(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
        {
            return Some((ls + a, ls + b));
        }
        let (ws, we) = self.word_at(pos);
        (we > ws).then_some((ws, we))
    }

    pub fn word_at(&self, pos: usize) -> (usize, usize) {
        let here = self.char_at(pos).is_some_and(is_word);
        let before = pos > 0 && self.char_at(pos - 1).is_some_and(is_word);
        if !here && !before {
            return (pos, pos);
        }
        let (mut s, mut e) = (pos, pos);
        while s > 0 && is_word(self.rope.char(s - 1)) {
            s -= 1;
        }
        while e < self.len() && is_word(self.rope.char(e)) {
            e += 1;
        }
        (s, e)
    }

    fn word_right(&self, pos: usize) -> usize {
        let len = self.len();
        let mut p = pos;
        if self.char_at(p) == Some('\n') {
            return p + 1;
        }
        while p < len && matches!(self.rope.char(p), ' ' | '\t') {
            p += 1;
        }
        if p < len && self.rope.char(p) != '\n' {
            let cls = char_class(self.rope.char(p));
            while p < len && self.rope.char(p) != '\n' && char_class(self.rope.char(p)) == cls {
                p += 1;
            }
        }
        p
    }

    fn word_left(&self, pos: usize) -> usize {
        let mut p = pos;
        if p > 0 && self.rope.char(p - 1) == '\n' {
            return p - 1;
        }
        while p > 0 && matches!(self.rope.char(p - 1), ' ' | '\t') {
            p -= 1;
        }
        if p > 0 && self.rope.char(p - 1) != '\n' {
            let cls = char_class(self.rope.char(p - 1));
            while p > 0 && self.rope.char(p - 1) != '\n' && char_class(self.rope.char(p - 1)) == cls {
                p -= 1;
            }
        }
        p
    }

    pub fn matching_bracket(&self, pos: usize) -> Option<(usize, usize)> {
        const LIMIT: usize = 50_000;
        let check = |p: usize| -> Option<(usize, usize)> {
            let c = self.char_at(p)?;
            let (open, close, fwd) = match c {
                '(' => ('(', ')', true),
                '[' => ('[', ']', true),
                '{' => ('{', '}', true),
                ')' => ('(', ')', false),
                ']' => ('[', ']', false),
                '}' => ('{', '}', false),
                _ => return None,
            };
            let mut depth = 0i32;
            if fwd {
                for (i, ch) in self.rope.chars_at(p).enumerate().take(LIMIT) {
                    if ch == open {
                        depth += 1;
                    } else if ch == close {
                        depth -= 1;
                        if depth == 0 {
                            return Some((p, p + i));
                        }
                    }
                }
            } else {
                let mut it = self.rope.chars_at(p + 1);
                let mut i = p + 1;
                while let Some(ch) = it.prev() {
                    i -= 1;
                    if p - i > LIMIT {
                        break;
                    }
                    if ch == close {
                        depth += 1;
                    } else if ch == open {
                        depth -= 1;
                        if depth == 0 {
                            return Some((p, i));
                        }
                    }
                }
            }
            None
        };
        check(pos).or_else(|| if pos > 0 { check(pos - 1) } else { None })
    }

    /// (first, last) line covered by each selection.
    fn line_spans(&self) -> Vec<(usize, usize)> {
        self.sels
            .iter()
            .map(|s| {
                let l0 = self.rope.char_to_line(s.start());
                let mut l1 = self.rope.char_to_line(s.end());
                if l1 > l0 && s.end() == self.rope.line_to_char(l1) {
                    l1 -= 1;
                }
                (l0, l1)
            })
            .collect()
    }

    fn selected_lines(&self) -> Vec<usize> {
        let mut set = BTreeSet::new();
        for (a, b) in self.line_spans() {
            set.extend(a..=b);
        }
        set.into_iter().collect()
    }

    // ------------------------------------------------------------------ core editing

    fn touch(&mut self, from_line: usize) {
        self.version += 1;
        self.hover_info = None;
        self.hover_defs = None;
        self.content_id = next_content_id();
        self.highlight.invalidate(from_line);
        self.scroll_to_cursor = true;
        self.last_change = Instant::now();
    }

    pub fn end_coalesce(&mut self) {
        self.last_edit = None;
    }

    fn checkpoint(&mut self, kind: EditKind) {
        let now = Instant::now();
        let merge = kind != EditKind::Other
            && matches!(self.last_edit, Some((k, t)) if k == kind && now.duration_since(t).as_millis() < 1500);
        if !merge {
            self.undo.push(Snapshot { rope: self.rope.clone(), sels: self.sels.clone(), content: self.content_id });
            if self.undo.len() > 2000 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_edit = Some((kind, now));
    }

    /// Applies sorted, non-overlapping replacements. Returns the new start of each one.
    fn apply(&mut self, edits: &mut [(usize, usize, String)]) -> Vec<usize> {
        let len = self.rope.len_chars();
        let mut delta: isize = 0;
        let mut prev_end = 0;
        let mut starts = Vec::with_capacity(edits.len());
        let mut min_line = usize::MAX;
        for (s, e, t) in edits.iter_mut() {
            *s = (*s).max(prev_end).min(len);
            *e = (*e).max(*s).min(len);
            let s2 = (*s as isize + delta) as usize;
            let e2 = (*e as isize + delta) as usize;
            let n = t.chars().count();
            if e2 > s2 || n > 0 {
                min_line = min_line.min(self.rope.char_to_line(s2));
                if e2 > s2 {
                    self.rope.remove(s2..e2);
                }
                if n > 0 {
                    self.rope.insert(s2, t);
                }
            }
            starts.push(s2);
            delta += n as isize - (*e - *s) as isize;
            prev_end = *e;
        }
        if min_line != usize::MAX {
            self.touch(min_line);
        }
        if !self.folds.is_empty() {
            let spans: Vec<(usize, usize, usize)> = edits.iter().map(|(s, e, t)| (*s, *e, t.chars().count())).collect();
            let len = self.rope.len_chars();
            let mut f: Vec<usize> = self
                .folds
                .iter()
                .map(|&p| self.rope.line_to_char(self.rope.char_to_line(map_pos(p, &spans).min(len))))
                .collect();
            f.sort_unstable();
            f.dedup();
            self.folds = f;
        }
        if !self.bookmarks.is_empty() {
            let spans: Vec<(usize, usize, usize)> = edits.iter().map(|(s, e, t)| (*s, *e, t.chars().count())).collect();
            let len = self.rope.len_chars();
            let mut b: Vec<usize> = self
                .bookmarks
                .iter()
                .map(|&p| self.rope.line_to_char(self.rope.char_to_line(map_pos(p, &spans).min(len))))
                .collect();
            b.sort_unstable();
            b.dedup();
            self.bookmarks = b;
        }
        starts
    }

    /// One edit per selection (sorted like `self.sels`); selections are replaced by the edits' results.
    pub fn edit(&mut self, kind: EditKind, items: Vec<Edit>) {
        if items.iter().all(|e| e.start == e.end && e.text.is_empty()) {
            self.sels = items.iter().map(|e| Sel::range(e.start + e.anchor, e.start + e.head)).collect();
            self.normalize();
            self.scroll_to_cursor = true;
            return;
        }
        self.checkpoint(kind);
        let mut edits: Vec<(usize, usize, String)> = items.iter().map(|e| (e.start, e.end, e.text.clone())).collect();
        let starts = self.apply(&mut edits);
        self.sels = items.iter().zip(starts).map(|(e, s)| Sel::range(s + e.anchor, s + e.head)).collect();
        self.normalize();
    }

    /// Arbitrary replacements; existing selections are mapped through them.
    pub fn replace_keep(&mut self, kind: EditKind, mut edits: Vec<(usize, usize, String)>) {
        if edits.is_empty() {
            return;
        }
        edits.sort_by_key(|e| (e.0, e.1));
        self.checkpoint(kind);
        let old = self.sels.clone();
        self.apply(&mut edits);
        let spans: Vec<(usize, usize, usize)> = edits.iter().map(|(s, e, t)| (*s, *e, t.chars().count())).collect();
        self.sels = old
            .iter()
            .map(|s| Sel { anchor: map_pos(s.anchor, &spans), head: map_pos(s.head, &spans), goal: None })
            .collect();
        self.normalize();
    }

    pub fn set_text(&mut self, text: &str) {
        self.folds.clear();
        let len = self.len();
        self.replace_keep(EditKind::Other, vec![(0, len, text.to_string())]);
    }

    /// Sort, clamp and merge overlapping selections.
    pub fn normalize(&mut self) {
        let len = self.rope.len_chars();
        let prim_head = self.sels.get(self.primary).or(self.sels.last()).map(|s| s.head).unwrap_or(0);
        let mut v = std::mem::take(&mut self.sels);
        for s in v.iter_mut() {
            s.anchor = s.anchor.min(len);
            s.head = s.head.min(len);
        }
        v.sort_by_key(|s| (s.start(), s.end()));
        let mut out: Vec<Sel> = Vec::with_capacity(v.len());
        for s in v {
            if let Some(last) = out.last_mut() {
                let overlap = s.start() < last.end()
                    || s.start() == last.start()
                    || (s.is_empty() && s.head == last.end())
                    || (last.is_empty() && last.head == s.start());
                if overlap {
                    let (start, end) = (last.start(), last.end().max(s.end()));
                    let forward = last.anchor <= last.head;
                    *last = if forward { Sel::range(start, end) } else { Sel::range(end, start) };
                    continue;
                }
            }
            out.push(s);
        }
        if out.is_empty() {
            out.push(Sel::cursor(0));
        }
        self.primary = out
            .iter()
            .position(|s| s.start() <= prim_head && prim_head <= s.end())
            .unwrap_or(out.len() - 1);
        self.sels = out;
        self.reveal_cursors();
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(Snapshot { rope: self.rope.clone(), sels: self.sels.clone(), content: self.content_id });
            self.restore(s);
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(Snapshot { rope: self.rope.clone(), sels: self.sels.clone(), content: self.content_id });
            self.restore(s);
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.rope = s.rope;
        self.sels = s.sels;
        self.touch(0);
        self.content_id = s.content;
        self.normalize();
        self.last_edit = None;
    }

    // ------------------------------------------------------------------ commands

    pub fn exec(&mut self, cmd: Cmd, auto_pair: bool) {
        use Cmd::*;
        if self.read_only && cmd.modifies() {
            return;
        }
        match cmd {
            Move(m, extend) => self.move_sels(m, extend),
            Backspace => self.backspace(auto_pair),
            Delete => self.delete_forward(),
            DeleteWordBack => self.delete_word(false),
            DeleteWordForward => self.delete_word(true),
            Newline => self.newline(),
            Tab => self.tab(),
            Indent => self.indent(),
            Unindent => self.unindent(),
            SelectAll => {
                self.sels = vec![Sel::range(0, self.len())];
                self.primary = 0;
            }
            SelectLine => self.select_line(),
            SelectNext => self.select_next(),
            SelectAllOccurrences => self.select_all_occurrences(),
            SplitLines => self.split_lines(),
            AddCursorAbove => self.add_cursor(false),
            AddCursorBelow => self.add_cursor(true),
            DuplicateLine => self.duplicate(),
            DeleteLine => self.delete_line(),
            MoveLineUp => self.move_lines(true),
            MoveLineDown => self.move_lines(false),
            ToggleComment => self.toggle_comment(),
            JoinLines => self.join_lines(),
            InsertLineAfter => self.insert_line(true),
            InsertLineBefore => self.insert_line(false),
            UpperCase => self.change_case(true),
            LowerCase => self.change_case(false),
            SortLines => self.sort_lines(),
            GotoBracket => self.goto_bracket(),
            GitNextChange => self.goto_change(true),
            GitPrevChange => self.goto_change(false),
            GitRevertChange => {
                let line = self.rope.char_to_line(self.primary_sel().head);
                if self.diff_version != self.version {
                    self.refresh_diff();
                }
                if let Some(i) = self.hunks.iter().position(|h| h.contains(line)) {
                    self.revert_hunk(i);
                }
            }
            GitRevertFile => {
                if let Some(base) = self.git_base.clone() {
                    self.set_text(&base);
                    self.refresh_diff();
                }
            }
            Undo => self.undo(),
            Redo => self.redo(),
            _ => {}
        }
        self.scroll_to_cursor = true;
    }

    pub fn move_sels(&mut self, m: Motion, extend: bool) {
        let len = self.len();
        let nlines = self.rope.len_lines();
        let hidden = if self.folds.is_empty() { Vec::new() } else { self.hidden_ranges() };
        let page = self.page_lines.max(1) as isize;
        let new: Vec<Sel> = self
            .sels
            .iter()
            .map(|s| {
                let (pos, goal) = match m {
                    Motion::Left if !extend && !s.is_empty() => (s.start(), None),
                    Motion::Right if !extend && !s.is_empty() => (s.end(), None),
                    Motion::Left => (s.head.saturating_sub(1), None),
                    Motion::Right => ((s.head + 1).min(len), None),
                    // one row of the screen: inside a wrapped line, or to the next / previous line
                    // (`goal`: the column on the screen, the same as the visual one without wrap)
                    Motion::Up | Motion::Down if !self.wraps.is_empty() => {
                        let (line, col) = self.line_col(s.head);
                        let (k, x) = self.wraps.place(line, self.vcol(line, col));
                        let goal = s.goal.unwrap_or(x);
                        let down = matches!(m, Motion::Down);
                        let mut target = if down && k + 1 < self.wraps.rows(line) {
                            Some((line, k + 1))
                        } else if !down && k > 0 {
                            Some((line, k - 1))
                        } else {
                            let mut nl = line as isize + if down { 1 } else { -1 };
                            if let Some(&(s, e)) = hidden.iter().find(|(s, e)| (*s as isize..=*e as isize).contains(&nl)) {
                                nl = if down { e as isize + 1 } else { s as isize - 1 };
                            }
                            (nl >= 0 && (nl as usize) < nlines).then(|| (nl as usize, if down { 0 } else { self.wraps.rows(nl as usize) - 1 }))
                        };
                        match target.take() {
                            Some((tl, tk)) => {
                                let v = self.wraps.unplace(tl, tk, goal);
                                (self.pos_of(tl, self.col_at_vcol(tl, v)), Some(goal))
                            }
                            None => (if down { len } else { 0 }, Some(goal)),
                        }
                    }
                    Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => {
                        let (line, col) = self.line_col(s.head);
                        let goal = s.goal.unwrap_or_else(|| self.vcol(line, col));
                        let delta = match m {
                            Motion::Up => -1,
                            Motion::Down => 1,
                            Motion::PageUp => -page,
                            _ => page,
                        };
                        let mut nl = line as isize + delta;
                        // jump over folded blocks
                        if let Some(&(s, e)) = hidden.iter().find(|(s, e)| (*s as isize..=*e as isize).contains(&nl)) {
                            nl = if delta < 0 { s as isize - 1 } else if e + 1 < nlines { e as isize + 1 } else { s as isize - 1 };
                        }
                        if nl < 0 {
                            (0, Some(goal))
                        } else if nl as usize >= nlines {
                            (len, Some(goal))
                        } else {
                            let nl = nl as usize;
                            (self.pos_of(nl, self.col_at_vcol(nl, goal)), Some(goal))
                        }
                    }
                    Motion::WordLeft => (self.word_left(s.head), None),
                    Motion::WordRight => (self.word_right(s.head), None),
                    Motion::Home => {
                        let (line, col) = self.line_col(s.head);
                        let ls = self.rope.line_to_char(line);
                        let ind = self.indent_len(line);
                        (if col == ind { ls } else { ls + ind }, None)
                    }
                    Motion::End => {
                        let line = self.rope.char_to_line(s.head);
                        (self.rope.line_to_char(line) + self.line_len(line), None)
                    }
                    Motion::DocStart => (0, None),
                    Motion::DocEnd => (len, None),
                };
                if extend {
                    Sel { anchor: s.anchor, head: pos, goal }
                } else {
                    Sel { anchor: pos, head: pos, goal }
                }
            })
            .collect();
        self.sels = new;
        self.normalize();
        self.scroll_to_cursor = true;
        self.last_edit = None;
    }

    pub fn insert_text(&mut self, text: &str, auto_pair: bool) {
        if self.read_only {
            return;
        }
        let text = text.replace("\r\n", "\n");
        let mut it = text.chars();
        let single = match (it.next(), it.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        };
        let items: Vec<Edit> = self
            .sels
            .iter()
            .map(|s| {
                if let (Some(c), true) = (single, auto_pair) {
                    let next = self.char_at(s.head);
                    let prev = if s.start() > 0 { self.char_at(s.start() - 1) } else { None };
                    if s.is_empty() && matches!(c, ')' | ']' | '}' | '"' | '\'' | '`') && next == Some(c) {
                        return Edit::stay(s.head, 1);
                    }
                    if let Some(close) = pair_of(c) {
                        if !s.is_empty() {
                            let inner = self.slice(s.start(), s.end());
                            let n = inner.chars().count();
                            return Edit {
                                start: s.start(),
                                end: s.end(),
                                text: format!("{c}{inner}{close}"),
                                anchor: 1,
                                head: 1 + n,
                            };
                        }
                        let is_quote = c == close;
                        let next_ok = next.map_or(true, |n| n.is_whitespace() || ")]};,:".contains(n));
                        let prev_ok = !is_quote || prev.map_or(true, |p| !is_word(p) && p != c);
                        if next_ok && prev_ok {
                            return Edit { start: s.head, end: s.head, text: format!("{c}{close}"), anchor: 1, head: 1 };
                        }
                    }
                }
                Edit::insert(s.start(), s.end(), text.clone())
            })
            .collect();
        self.edit(EditKind::Insert, items);
    }

    fn newline(&mut self) {
        let unit = self.indent_unit();
        let items: Vec<Edit> = self
            .sels
            .iter()
            .map(|s| {
                let (line, col) = self.line_col(s.start());
                let indent: String = self
                    .rope
                    .line(line)
                    .chars()
                    .take(col)
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .collect();
                let prev = if col > 0 { self.char_at(s.start() - 1) } else { None };
                let next = self.char_at(s.end());
                let opens = matches!(prev, Some(':' | '{' | '(' | '['));
                let between = matches!((prev, next), (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']')));
                if between {
                    let t = format!("\n{indent}{unit}\n{indent}");
                    let off = 1 + indent.chars().count() + unit.chars().count();
                    Edit { start: s.start(), end: s.end(), text: t, anchor: off, head: off }
                } else if opens {
                    Edit::insert(s.start(), s.end(), format!("\n{indent}{unit}"))
                } else {
                    Edit::insert(s.start(), s.end(), format!("\n{indent}"))
                }
            })
            .collect();
        self.edit(EditKind::Other, items);
    }

    fn backspace(&mut self, auto_pair: bool) {
        let items: Vec<Edit> = self
            .sels
            .iter()
            .map(|s| {
                if !s.is_empty() {
                    return Edit::delete(s.start(), s.end());
                }
                let h = s.head;
                if h == 0 {
                    return Edit::stay(h, 0);
                }
                if auto_pair {
                    if let (Some(p), Some(n)) = (self.char_at(h - 1), self.char_at(h)) {
                        if pair_of(p) == Some(n) {
                            return Edit::delete(h - 1, h + 1);
                        }
                    }
                }
                let (_, col) = self.line_col(h);
                if self.use_spaces && col > 0 && self.slice(h - col, h).chars().all(|c| c == ' ') {
                    let rem = col % self.tab_size;
                    let n = if rem == 0 { self.tab_size } else { rem };
                    return Edit::delete(h - n.min(col), h);
                }
                Edit::delete(h - 1, h)
            })
            .collect();
        self.edit(EditKind::Delete, items);
    }

    fn delete_forward(&mut self) {
        let len = self.len();
        let items: Vec<Edit> = self
            .sels
            .iter()
            .map(|s| {
                if !s.is_empty() {
                    Edit::delete(s.start(), s.end())
                } else {
                    Edit::delete(s.head, (s.head + 1).min(len))
                }
            })
            .collect();
        self.edit(EditKind::Delete, items);
    }

    fn delete_word(&mut self, forward: bool) {
        let items: Vec<Edit> = self
            .sels
            .iter()
            .map(|s| {
                if !s.is_empty() {
                    Edit::delete(s.start(), s.end())
                } else if forward {
                    Edit::delete(s.head, self.word_right(s.head))
                } else {
                    Edit::delete(self.word_left(s.head), s.head)
                }
            })
            .collect();
        self.edit(EditKind::Other, items);
    }

    fn tab(&mut self) {
        let multi_line = self.sels.iter().any(|s| self.rope.char_to_line(s.start()) != self.rope.char_to_line(s.end()));
        if multi_line {
            return self.indent();
        }
        let items: Vec<Edit> = self
            .sels
            .iter()
            .map(|s| {
                let t = if self.use_spaces {
                    let (l, c) = self.line_col(s.start());
                    " ".repeat(self.tab_size - self.vcol(l, c) % self.tab_size)
                } else {
                    "\t".to_string()
                };
                Edit::insert(s.start(), s.end(), t)
            })
            .collect();
        self.edit(EditKind::Insert, items);
    }

    fn indent(&mut self) {
        let unit = self.indent_unit();
        let edits = self
            .selected_lines()
            .into_iter()
            .filter(|&l| self.line_len(l) > 0)
            .map(|l| {
                let s = self.rope.line_to_char(l);
                (s, s, unit.clone())
            })
            .collect();
        self.replace_keep(EditKind::Other, edits);
    }

    fn unindent(&mut self) {
        let edits = self
            .selected_lines()
            .into_iter()
            .filter_map(|l| {
                let s = self.rope.line_to_char(l);
                let n = if self.char_at(s) == Some('\t') {
                    1
                } else {
                    self.rope.line(l).chars().take(self.tab_size).take_while(|c| *c == ' ').count()
                };
                (n > 0).then(|| (s, s + n, String::new()))
            })
            .collect();
        self.replace_keep(EditKind::Other, edits);
    }

    fn select_line(&mut self) {
        let sels: Vec<Sel> = self
            .sels
            .iter()
            .map(|s| {
                let l0 = self.rope.char_to_line(s.start());
                let l1 = self.rope.char_to_line(s.end());
                Sel::range(self.rope.line_to_char(l0), self.line_end_incl(l1))
            })
            .collect();
        self.sels = sels;
        self.normalize();
    }

    fn select_next(&mut self) {
        if self.sels.iter().any(|s| s.is_empty()) {
            let sels: Vec<Sel> = self
                .sels
                .iter()
                .map(|s| {
                    if s.is_empty() {
                        let (a, b) = self.word_at(s.head);
                        Sel::range(a, b)
                    } else {
                        *s
                    }
                })
                .collect();
            self.sels = sels;
            self.normalize();
            return;
        }
        let prim = self.primary_sel();
        let needle = self.slice(prim.start(), prim.end());
        if needle.is_empty() {
            return;
        }
        let text = self.rope.to_string();
        let from_b = self.rope.char_to_byte(prim.end());
        let mut search_from = from_b;
        let mut wrapped = false;
        loop {
            let hit = match text[search_from..].find(&needle) {
                Some(i) => i + search_from,
                None => {
                    if wrapped {
                        return;
                    }
                    wrapped = true;
                    search_from = 0;
                    continue;
                }
            };
            if wrapped && hit >= from_b {
                return;
            }
            let cs = self.rope.byte_to_char(hit);
            if self.sels.iter().any(|s| s.start() == cs) {
                search_from = hit + needle.len();
                continue;
            }
            let ce = cs + needle.chars().count();
            self.sels.push(Sel::range(cs, ce));
            self.primary = self.sels.len() - 1;
            self.normalize();
            return;
        }
    }

    fn select_all_occurrences(&mut self) {
        let prim = self.primary_sel();
        let (a, b) = if prim.is_empty() { self.word_at(prim.head) } else { (prim.start(), prim.end()) };
        if a == b {
            return;
        }
        let needle = self.slice(a, b);
        let text = self.rope.to_string();
        let n = needle.chars().count();
        let sels: Vec<Sel> = text
            .match_indices(&needle)
            .map(|(i, _)| {
                let cs = self.rope.byte_to_char(i);
                Sel::range(cs, cs + n)
            })
            .collect();
        if !sels.is_empty() {
            self.sels = sels;
            self.primary = 0;
            self.normalize();
        }
    }

    fn split_lines(&mut self) {
        let mut out = Vec::new();
        for s in &self.sels {
            let l0 = self.rope.char_to_line(s.start());
            let l1 = self.rope.char_to_line(s.end());
            if s.is_empty() || l0 == l1 {
                out.push(*s);
                continue;
            }
            for l in l0..=l1 {
                let ls = self.rope.line_to_char(l);
                let a = if l == l0 { s.start() } else { ls };
                let b = if l == l1 { s.end() } else { ls + self.line_len(l) };
                if l == l1 && b == ls {
                    continue;
                }
                out.push(Sel::range(a, b));
            }
        }
        self.sels = out;
        self.primary = self.sels.len() - 1;
        self.normalize();
    }

    fn add_cursor(&mut self, down: bool) {
        let s = if down { *self.sels.last().unwrap() } else { self.sels[0] };
        let (line, col) = self.line_col(s.head);
        let goal = s.goal.unwrap_or_else(|| self.vcol(line, col));
        let nl = if down {
            line + 1
        } else {
            match line.checked_sub(1) {
                Some(l) => l,
                None => return,
            }
        };
        if nl >= self.rope.len_lines() {
            return;
        }
        let p = self.pos_of(nl, self.col_at_vcol(nl, goal));
        self.sels.push(Sel { anchor: p, head: p, goal: Some(goal) });
        self.primary = self.sels.len() - 1;
        self.normalize();
    }

    fn duplicate(&mut self) {
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for s in &self.sels {
            if s.is_empty() {
                let l = self.rope.char_to_line(s.head);
                let ls = self.rope.line_to_char(l);
                if edits.last().is_some_and(|e| e.0 == ls) {
                    continue;
                }
                edits.push((ls, ls, format!("{}\n", self.line_text(l))));
            } else {
                edits.push((s.start(), s.start(), self.slice(s.start(), s.end())));
            }
        }
        self.replace_keep(EditKind::Other, edits);
    }

    fn delete_line(&mut self) {
        let len = self.len();
        let nl = self.rope.len_lines();
        let mut ranges: Vec<(usize, usize)> = self
            .line_spans()
            .into_iter()
            .map(|(l0, l1)| {
                let e = self.line_end_incl(l1);
                let mut s = self.rope.line_to_char(l0);
                if l1 + 1 >= nl && l0 > 0 && e == len {
                    s -= 1; // last line: also remove the preceding newline
                }
                (s, e)
            })
            .collect();
        ranges.sort();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for r in ranges {
            match merged.last_mut() {
                Some(last) if r.0 <= last.1 => last.1 = last.1.max(r.1),
                _ => merged.push(r),
            }
        }
        self.replace_keep(EditKind::Other, merged.into_iter().map(|(s, e)| (s, e, String::new())).collect());
    }

    fn move_lines(&mut self, up: bool) {
        let spans = self.line_spans();
        let first = spans.iter().map(|s| s.0).min().unwrap_or(0);
        let last = spans.iter().map(|s| s.1).max().unwrap_or(0);
        let nlines = self.rope.len_lines();
        if (up && first == 0) || (!up && last + 1 >= nlines) {
            return;
        }
        let (a, b) = if up { (first - 1, last) } else { (first, last + 1) };
        let mut lines: Vec<String> = (a..=b).map(|l| self.line_text(l)).collect();
        let shift: isize = if up {
            let x = lines.remove(0);
            let n = x.chars().count() as isize + 1;
            lines.push(x);
            -n
        } else {
            let x = lines.pop().unwrap();
            let n = x.chars().count() as isize + 1;
            lines.insert(0, x);
            n
        };
        let start = self.rope.line_to_char(a);
        let end = self.rope.line_to_char(b) + self.line_len(b);
        let new_sels: Vec<Sel> = self
            .sels
            .iter()
            .map(|s| Sel {
                anchor: (s.anchor as isize + shift) as usize,
                head: (s.head as isize + shift) as usize,
                goal: s.goal,
            })
            .collect();
        self.checkpoint(EditKind::Other);
        let mut e = vec![(start, end, lines.join("\n"))];
        self.apply(&mut e);
        self.sels = new_sels;
        self.normalize();
    }

    pub fn comment_tokens(&self) -> (&'static str, Option<&'static str>) {
        let ext = self
            .path
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        match ext.as_str() {
            "rs" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "c" | "h" | "cpp" | "hpp" | "cc" | "java" | "go"
            | "cs" | "swift" | "kt" | "scss" | "less" | "php" | "dart" | "scala" | "jsonc" => ("//", None),
            "css" => ("/*", Some("*/")),
            "html" | "htm" | "xml" | "svg" | "xsd" | "xsl" | "xslt" | "vue" | "md" | "rng" => ("<!--", Some("-->")),
            "sql" | "lua" | "hs" | "elm" => ("--", None),
            "bat" | "cmd" => ("REM", None),
            "tex" | "erl" => ("%", None),
            "vim" => ("\"", None),
            _ => match self.syntax.as_str() {
                "Rust" | "JavaScript" | "C" | "C++" | "Java" | "Go" | "C#" => ("//", None),
                "HTML" | "XML" => ("<!--", Some("-->")),
                _ => ("#", None),
            },
        }
    }

    fn toggle_comment(&mut self) {
        let (open, close) = self.comment_tokens();
        let lines: Vec<usize> =
            self.selected_lines().into_iter().filter(|&l| !self.line_text(l).trim().is_empty()).collect();
        if lines.is_empty() {
            return;
        }
        let all_commented = lines.iter().all(|&l| self.line_text(l).trim_start().starts_with(open));
        let min_indent = lines.iter().map(|&l| self.indent_len(l)).min().unwrap_or(0);
        let mut edits = Vec::new();
        for &l in &lines {
            let text = self.line_text(l);
            let chars: Vec<char> = text.chars().collect();
            let ls = self.rope.line_to_char(l);
            if all_commented {
                let ind = self.indent_len(l);
                let mut n = open.chars().count();
                if chars.get(ind + n) == Some(&' ') {
                    n += 1;
                }
                edits.push((ls + ind, ls + ind + n, String::new()));
                if let Some(c) = close {
                    let t = text.trim_end();
                    if t.ends_with(c) {
                        let tlen = t.chars().count();
                        let mut cl = c.chars().count();
                        let mut st = tlen - cl;
                        if st > ind + n && chars.get(st - 1) == Some(&' ') {
                            st -= 1;
                            cl += 1;
                        }
                        if st >= ind + n {
                            edits.push((ls + st, ls + st + cl, String::new()));
                        }
                    }
                }
            } else {
                edits.push((ls + min_indent, ls + min_indent, format!("{open} ")));
                if let Some(c) = close {
                    let tlen = text.trim_end().chars().count();
                    edits.push((ls + tlen, ls + tlen, format!(" {c}")));
                }
            }
        }
        self.replace_keep(EditKind::Other, edits);
    }

    fn join_lines(&mut self) {
        let nl = self.rope.len_lines();
        let mut edits = Vec::new();
        let mut done = BTreeSet::new();
        for (l0, l1) in self.line_spans() {
            let l1 = if l1 == l0 { l0 + 1 } else { l1 };
            for l in l0..l1 {
                if l + 1 >= nl || !done.insert(l) {
                    continue;
                }
                let ls = self.rope.line_to_char(l);
                let trimmed = self.line_text(l).trim_end().chars().count();
                let next_start = self.rope.line_to_char(l + 1) + self.indent_len(l + 1);
                let sep = if trimmed == 0 || self.line_len(l + 1) == 0 { "" } else { " " };
                edits.push((ls + trimmed, next_start, sep.to_string()));
            }
        }
        self.replace_keep(EditKind::Other, edits);
    }

    fn insert_line(&mut self, after: bool) {
        let mut seen = BTreeSet::new();
        let items: Vec<Edit> = self
            .sels
            .iter()
            .filter(|s| seen.insert(self.rope.char_to_line(s.head)))
            .map(|s| {
                let l = self.rope.char_to_line(s.head);
                let ls = self.rope.line_to_char(l);
                let ind: String = self.line_text(l).chars().take(self.indent_len(l)).collect();
                if after {
                    let le = ls + self.line_len(l);
                    Edit::insert(le, le, format!("\n{ind}"))
                } else {
                    let k = ind.chars().count();
                    Edit { start: ls, end: ls, text: format!("{ind}\n"), anchor: k, head: k }
                }
            })
            .collect();
        self.edit(EditKind::Other, items);
    }

    fn change_case(&mut self, upper: bool) {
        let items: Vec<Edit> = self
            .sels
            .iter()
            .map(|s| {
                let (a, b) = if s.is_empty() { self.word_at(s.head) } else { (s.start(), s.end()) };
                let t = self.slice(a, b);
                let t = if upper { t.to_uppercase() } else { t.to_lowercase() };
                let n = t.chars().count();
                Edit { start: a, end: b, text: t, anchor: 0, head: n }
            })
            .collect();
        self.edit(EditKind::Other, items);
    }

    fn sort_lines(&mut self) {
        let nl = self.rope.len_lines();
        let spans: Vec<(usize, usize)> = if self.sels.iter().all(|s| s.is_empty()) {
            let last = if nl > 1 && self.line_len(nl - 1) == 0 { nl - 2 } else { nl - 1 };
            vec![(0, last)]
        } else {
            self.sels.iter().zip(self.line_spans()).filter(|(s, _)| !s.is_empty()).map(|(_, sp)| sp).collect()
        };
        let edits = spans
            .into_iter()
            .map(|(l0, l1)| {
                let mut lines: Vec<String> = (l0..=l1).map(|l| self.line_text(l)).collect();
                lines.sort();
                let s = self.rope.line_to_char(l0);
                let e = self.rope.line_to_char(l1) + self.line_len(l1);
                (s, e, lines.join("\n"))
            })
            .collect();
        self.replace_keep(EditKind::Other, edits);
    }

    fn goto_bracket(&mut self) {
        let h = self.primary_sel().head;
        if let Some((_, b)) = self.matching_bracket(h) {
            self.sels = vec![Sel::cursor(b)];
            self.primary = 0;
        }
    }

    fn trim_trailing_whitespace(&mut self) {
        let edits: Vec<(usize, usize, String)> = (0..self.rope.len_lines())
            .filter_map(|l| {
                let t = self.line_text(l);
                let trimmed = t.trim_end().chars().count();
                let full = t.chars().count();
                let ls = self.rope.line_to_char(l);
                (trimmed < full).then(|| (ls + trimmed, ls + full, String::new()))
            })
            .collect();
        self.replace_keep(EditKind::Other, edits);
    }

    // ------------------------------------------------------------------ git diff

    pub fn refresh_diff(&mut self) {
        self.hunks = match &self.git_base {
            Some(base) => diff::compute(base, &self.rope.to_string()),
            None => Vec::new(),
        };
        self.git_marks = diff::marks(&self.hunks);
        self.diff_version = self.version;
        if let Some(l) = self.peek {
            if !self.hunks.iter().any(|h| h.contains(l)) {
                self.peek = None;
            }
        }
    }

    /// Appends `# noqa: CODE` to `line` (or adds the codes to an existing `# noqa:`).
    pub fn add_noqa(&mut self, line: usize, codes: &[String]) {
        let text = self.line_text(line);
        let missing: Vec<&String> = codes.iter().filter(|c| !text.contains(c.as_str())).collect();
        if missing.is_empty() || (text.contains("# noqa") && !text.contains("# noqa:")) {
            return; // nothing to add, or a bare "# noqa" already ignores everything
        }
        let list = missing.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", ");
        let tail = if text.contains("# noqa:") { format!(", {list}") } else { format!("  # noqa: {list}") };
        let end = self.rope.line_to_char(line) + text.trim_end().chars().count();
        let line_end = self.rope.line_to_char(line) + self.line_len(line);
        self.replace_keep(EditKind::Other, vec![(end, line_end, tail)]);
    }

    /// Line in HEAD corresponding to buffer line `l` (None for a line added since HEAD).
    pub fn head_line(&self, l: usize) -> Option<usize> {
        let mut delta: isize = 0;
        for h in &self.hunks {
            if h.new_end <= l && !(h.new_start == h.new_end && h.new_start == l) {
                delta += h.new_lines.len() as isize - h.old_lines.len() as isize;
            } else if l >= h.new_start && l < h.new_end {
                if h.old_lines.is_empty() {
                    return None;
                }
                return Some(h.old_start + (l - h.new_start).min(h.old_lines.len() - 1));
            } else {
                break;
            }
        }
        Some((l as isize - delta).max(0) as usize)
    }

    /// Puts back the HEAD version of one hunk (undoable).
    pub fn revert_hunk(&mut self, i: usize) {
        let Some(h) = self.hunks.get(i).cloned() else { return };
        let (nl, len) = (self.rope.len_lines(), self.len());
        let at = |l: usize| if l < nl { self.rope.line_to_char(l) } else { len };
        let (s, e) = (at(h.new_start), at(h.new_end));
        self.replace_keep(EditKind::Other, vec![(s, e, h.old_lines.concat())]);
        self.peek = None;
        self.refresh_diff();
    }

    pub fn goto_change(&mut self, forward: bool) {
        if self.diff_version != self.version {
            self.refresh_diff();
        }
        let line = self.rope.char_to_line(self.primary_sel().head);
        let h = if forward {
            self.hunks.iter().find(|h| h.mark_lines().start > line).or(self.hunks.first())
        } else {
            self.hunks.iter().rev().find(|h| h.mark_lines().start < line).or(self.hunks.last())
        };
        let Some(l) = h.map(|h| h.mark_lines().start) else { return };
        self.goto_line(l, 0);
        self.peek = Some(l);
    }

    // ------------------------------------------------------------------ merge conflicts

    pub fn conflicts(&mut self) -> Vec<Conflict> {
        if self.rope.len_bytes() > 8_000_000 {
            return Vec::new();
        }
        if self.conflicts_cache.as_ref().map(|c| c.0) != Some(self.version) {
            let c = find_conflicts(self.rope.lines().map(std::borrow::Cow::from));
            self.had_conflicts |= !c.is_empty();
            self.conflicts_cache = Some((self.version, c));
        }
        self.conflicts_cache.as_ref().unwrap().1.clone()
    }

    /// Replaces conflict block `c` by the chosen side(s), in one undoable edit.
    pub fn resolve_conflict(&mut self, c: &Conflict, how: Resolution) {
        let text = |a: usize, b: usize| -> String { (a..b).map(|l| self.rope.line(l).to_string()).collect() };
        let ours = text(c.start + 1, c.base.unwrap_or(c.mid));
        let theirs = text(c.mid + 1, c.end);
        let new = match how {
            Resolution::Ours => ours,
            Resolution::Theirs => theirs,
            Resolution::Both => ours + &theirs,
        };
        let a = self.rope.line_to_char(c.start);
        let b = self.rope.line_to_char((c.end + 1).min(self.rope.len_lines())).max(self.line_end_incl(c.end));
        self.replace_range(a, b, &new);
        let p = a.min(self.len());
        self.sels = vec![Sel::cursor(p)];
        self.primary = 0;
        self.scroll_to_cursor = true;
    }

    // ------------------------------------------------------------------ folding

    fn is_markdown(&self) -> bool {
        self.syntax == "Markdown" || self.syntax == "MultiMarkdown"
    }

    /// Lines of the block that can be folded under `line`: (first hidden, last hidden).
    pub fn fold_range(&self, line: usize) -> Option<(usize, usize)> {
        let n = self.rope.len_lines();
        let get = |i: usize| self.line_text(i);
        let end = if self.is_markdown() {
            crate::symbols::heading_end_with(n, line, get)
        } else {
            crate::symbols::block_end_with(n, line, self.tab_size, get)
        }?;
        (end > line).then_some((line + 1, end))
    }

    /// Cheap test for the gutter: the next non-blank line is indented deeper (or a Markdown heading).
    pub fn can_fold(&self, line: usize) -> bool {
        let head = self.line_text(line);
        if head.trim().is_empty() {
            return false;
        }
        if self.is_markdown() {
            return crate::symbols::heading_level(&head).is_some() && self.fold_range(line).is_some();
        }
        let ind = crate::symbols::indent_of(&head, self.tab_size);
        let n = self.rope.len_lines();
        (line + 1..n.min(line + 200))
            .map(|i| self.line_text(i))
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| crate::symbols::indent_of(&l, self.tab_size) > ind)
    }

    pub fn bookmark_lines(&self) -> Vec<usize> {
        let len = self.len();
        let mut v: Vec<usize> = self.bookmarks.iter().map(|&p| self.rope.char_to_line(p.min(len))).collect();
        v.dedup();
        v
    }

    pub fn set_bookmark_lines(&mut self, lines: &[usize]) {
        let n = self.rope.len_lines();
        self.bookmarks = lines.iter().filter(|&&l| l < n).map(|&l| self.rope.line_to_char(l)).collect();
        self.bookmarks.sort_unstable();
        self.bookmarks.dedup();
    }

    /// Adds or removes the bookmark of the cursor line; true when it is now bookmarked.
    pub fn toggle_bookmark(&mut self) -> bool {
        let line = self.rope.char_to_line(self.primary_sel().head);
        let mut lines = self.bookmark_lines();
        let on = match lines.iter().position(|&l| l == line) {
            Some(i) => {
                lines.remove(i);
                false
            }
            None => {
                lines.push(line);
                true
            }
        };
        lines.sort_unstable();
        self.set_bookmark_lines(&lines);
        on
    }

    /// Moves the cursor to the next (or previous) bookmark, around the end of the file.
    pub fn goto_bookmark(&mut self, forward: bool) -> bool {
        let lines = self.bookmark_lines();
        let cur = self.rope.char_to_line(self.primary_sel().head);
        let target = if forward {
            lines.iter().find(|&&l| l > cur).or(lines.first())
        } else {
            lines.iter().rev().find(|&&l| l < cur).or(lines.last())
        };
        match target.copied() {
            Some(l) => {
                let ind = self.indent_len(l);
                self.goto_line(l, ind);
                true
            }
            None => false,
        }
    }

    pub fn fold_lines(&self) -> Vec<usize> {
        let len = self.len();
        self.folds.iter().map(|&p| self.rope.char_to_line(p.min(len))).collect()
    }

    pub fn is_folded(&self, line: usize) -> bool {
        self.fold_lines().contains(&line)
    }

    /// Hidden line ranges (inclusive), sorted and merged.
    pub fn hidden_ranges(&self) -> Vec<(usize, usize)> {
        let mut v: Vec<(usize, usize)> = self.fold_lines().into_iter().filter_map(|l| self.fold_range(l)).collect();
        v.sort();
        let mut out: Vec<(usize, usize)> = Vec::new();
        for (s, e) in v {
            match out.last_mut() {
                Some(last) if s <= last.1 + 1 => last.1 = last.1.max(e),
                _ => out.push((s, e)),
            }
        }
        out
    }

    pub fn fold(&mut self, line: usize) -> bool {
        if self.fold_range(line).is_none() || self.is_folded(line) {
            return false;
        }
        self.folds.push(self.rope.line_to_char(line));
        self.folds.sort_unstable();
        // the cursors must stay visible: move those inside the block to its first line
        let (s, e) = self.fold_range(line).unwrap();
        let head = self.rope.line_to_char(line) + self.line_len(line);
        let rope = &self.rope;
        for sel in self.sels.iter_mut() {
            if (s..=e).contains(&rope.char_to_line(sel.head)) {
                *sel = Sel::cursor(head);
            }
        }
        self.normalize();
        true
    }

    pub fn unfold(&mut self, line: usize) -> bool {
        let before = self.folds.len();
        let lines = self.fold_lines();
        let keep: Vec<usize> = self.folds.iter().zip(lines).filter(|(_, l)| *l != line).map(|(p, _)| *p).collect();
        self.folds = keep;
        self.folds.len() != before
    }

    pub fn toggle_fold(&mut self, line: usize) {
        if !self.unfold(line) {
            self.fold(line);
        }
    }

    /// Folds the block containing the cursor (its own block when the line opens one).
    pub fn fold_at_cursor(&mut self) {
        let line = self.rope.char_to_line(self.primary_sel().head);
        let header = (0..=line).rev().take(5000).find(|&l| {
            !self.is_folded(l) && self.fold_range(l).is_some_and(|(s, e)| l == line || (s..=e).contains(&line))
        });
        if let Some(h) = header {
            self.fold(h);
        }
    }

    /// Unfolds the folds on the cursor line (or containing it).
    pub fn unfold_at_cursor(&mut self) {
        let line = self.rope.char_to_line(self.primary_sel().head);
        let lines = self.fold_lines();
        let hit: Vec<usize> = lines
            .into_iter()
            .filter(|&l| l == line || self.fold_range(l).is_some_and(|(s, e)| (s..=e).contains(&line)))
            .collect();
        for l in hit {
            self.unfold(l);
        }
    }

    /// Folds every class / function block (or the top-level blocks when no symbol is known).
    pub fn fold_all(&mut self) {
        let mut lines: Vec<usize> = self.symbols().iter().filter(|s| s.end > s.line).map(|s| s.line).collect();
        if lines.is_empty() {
            lines = (0..self.rope.len_lines()).filter(|&l| self.indent_len(l) == 0 && self.can_fold(l)).collect();
        }
        for l in lines {
            let p = self.rope.line_to_char(l);
            if !self.folds.contains(&p) {
                self.folds.push(p);
            }
        }
        self.folds.sort_unstable();
        // cursors inside a folded block go to its first visible line
        let hidden = self.hidden_ranges();
        let sels: Vec<Sel> = self
            .sels
            .iter()
            .map(|sel| {
                let l = self.rope.char_to_line(sel.head);
                match hidden.iter().find(|(s, e)| (*s..=*e).contains(&l)) {
                    Some(&(s, _)) => Sel::cursor(self.rope.line_to_char(s - 1) + self.line_len(s - 1)),
                    None => *sel,
                }
            })
            .collect();
        self.sels = sels;
        self.normalize();
    }

    pub fn unfold_all(&mut self) {
        self.folds.clear();
    }

    /// A cursor moved into a folded block (search, go to line…): open the block.
    pub fn reveal_cursors(&mut self) {
        if self.folds.is_empty() {
            return;
        }
        let heads: Vec<usize> = self.sels.iter().map(|s| self.rope.char_to_line(s.head)).collect();
        let lines = self.fold_lines();
        let open: Vec<usize> = lines
            .into_iter()
            .filter(|&l| self.fold_range(l).is_some_and(|(s, e)| heads.iter().any(|h| (s..=e).contains(h))))
            .collect();
        for l in open {
            self.unfold(l);
        }
    }

    /// Selects whole lines `a..=b` (with their line break) and shows them.
    pub fn select_lines(&mut self, a: usize, b: usize) {
        let last = self.rope.len_lines().saturating_sub(1);
        let (a, b) = (a.min(last), b.min(last));
        self.goto_line(a, 0);
        self.sels = vec![Sel::range(self.rope.line_to_char(a), self.line_end_incl(b))];
        self.primary = 0;
        self.scroll_to_cursor = true;
        self.center_cursor = true;
    }

    /// Classes, functions… of the text (recomputed when it changed).
    pub fn symbols(&mut self) -> &[crate::symbols::Symbol] {
        if self.rope.len_bytes() > 4_000_000 {
            return &[];
        }
        let stale = self.symbols_cache.as_ref().map_or(true, |(v, s, _)| *v != self.version || *s != self.syntax);
        if stale {
            let syms = crate::symbols::extract(&self.rope.to_string(), &self.syntax, self.tab_size);
            self.symbols_cache = Some((self.version, self.syntax.clone(), syms));
        }
        &self.symbols_cache.as_ref().unwrap().2
    }

    pub fn goto_line(&mut self, line: usize, col: usize) {
        let p = self.pos_of(line, col);
        self.sels = vec![Sel::cursor(p)];
        self.primary = 0;
        self.scroll_to_cursor = true;
        self.center_cursor = true;
        self.reveal_cursors();
    }

    // ------------------------------------------------------------------ clipboard

    /// Returns the text and whether it is a whole-line copy (no selection).
    pub fn copy_text(&self) -> (String, bool) {
        if self.sels.iter().all(|s| s.is_empty()) {
            let mut out = String::new();
            let mut last = usize::MAX;
            for s in &self.sels {
                let l = self.rope.char_to_line(s.head);
                if l == last {
                    continue;
                }
                last = l;
                out += &self.line_text(l);
                out.push('\n');
            }
            (out, true)
        } else {
            let parts: Vec<String> =
                self.sels.iter().filter(|s| !s.is_empty()).map(|s| self.slice(s.start(), s.end())).collect();
            (parts.join("\n"), false)
        }
    }

    pub fn cut(&mut self) -> (String, bool) {
        let (t, linewise) = self.copy_text();
        if !self.read_only {
            if linewise {
                self.delete_line();
            } else {
                let items = self.sels.iter().map(|s| Edit::delete(s.start(), s.end())).collect();
                self.edit(EditKind::Other, items);
            }
        }
        (t, linewise)
    }

    pub fn paste(&mut self, text: &str, linewise: bool) {
        if self.read_only {
            return;
        }
        let text = text.replace("\r\n", "\n");
        if linewise && self.sels.iter().all(|s| s.is_empty()) {
            let mut edits = Vec::new();
            let mut last = usize::MAX;
            for s in &self.sels {
                let l = self.rope.char_to_line(s.head);
                if l != last {
                    last = l;
                    let ls = self.rope.line_to_char(l);
                    edits.push((ls, ls, text.clone()));
                }
            }
            self.replace_keep(EditKind::Other, edits);
            return;
        }
        let parts: Vec<&str> = text.strip_suffix('\n').unwrap_or(&text).split('\n').collect();
        let n = self.sels.len();
        let items = self
            .sels
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let t = if n > 1 && parts.len() == n { parts[i].to_string() } else { text.clone() };
                Edit::insert(s.start(), s.end(), t)
            })
            .collect();
        self.edit(EditKind::Other, items);
    }

    // ------------------------------------------------------------------ search

    pub fn find_all(&self, re: &regex::Regex) -> Vec<(usize, usize)> {
        let text = self.rope.to_string();
        let mut out = Vec::new();
        for m in re.find_iter(&text) {
            if m.start() == m.end() {
                continue;
            }
            out.push((self.rope.byte_to_char(m.start()), self.rope.byte_to_char(m.end())));
            if out.len() >= 100_000 {
                break;
            }
        }
        out
    }

    pub fn replace_range(&mut self, a: usize, b: usize, text: &str) {
        let n = text.chars().count();
        self.sels = vec![Sel::range(a, b)];
        self.primary = 0;
        self.edit(EditKind::Other, vec![Edit { start: a, end: b, text: text.to_string(), anchor: 0, head: n }]);
    }
}

/// Encoding of a text file. Files that are not valid UTF-8 are read as Windows-1252 (latin-1
/// and its extensions, the usual case) and written back in it, byte for byte: replacing the
/// invalid bytes, as before, lost the accents of the whole file at the first save.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    Utf8,
    Windows1252,
}

/// Characters of the bytes 0x80..=0x9F in Windows-1252 (the five unassigned bytes keep their
/// C1 control code point, as in browsers: every byte comes back unchanged).
const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™',
    'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Windows1252 => "Windows-1252",
        }
    }

    fn decode(self, bytes: &[u8]) -> String {
        match self {
            Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Encoding::Windows1252 => bytes.iter().map(|&b| if (0x80..0xa0).contains(&b) { CP1252_HIGH[(b - 0x80) as usize] } else { b as char }).collect(),
        }
    }

    /// Bytes to write; an error names the first character the encoding cannot hold.
    pub fn encode(self, text: &str) -> Result<Vec<u8>, String> {
        match self {
            Encoding::Utf8 => Ok(text.as_bytes().to_vec()),
            Encoding::Windows1252 => text
                .chars()
                .enumerate()
                .map(|(i, c)| match c as u32 {
                    n if n < 0x80 || (0xa0..=0xff).contains(&n) => Ok(n as u8),
                    _ => CP1252_HIGH.iter().position(|h| *h == c).map(|k| 0x80 + k as u8).ok_or_else(|| {
                        let line = text.chars().take(i).filter(|c| *c == '\n').count() + 1;
                        format!("« {c} » (ligne {line}) n'existe pas en Windows-1252, l'encodage du fichier : clic sur « Windows-1252 » dans la barre d'état pour l'enregistrer en UTF-8")
                    }),
                })
                .collect(),
        }
    }
}

fn read_text(path: &Path) -> Result<(String, Encoding), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Err(format!("{} : fichier binaire", path.display()));
    }
    Ok(match String::from_utf8(bytes) {
        Ok(s) => (s, Encoding::Utf8),
        Err(e) => (Encoding::Windows1252.decode(e.as_bytes()), Encoding::Windows1252),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_follow_the_edits() {
        let mut d = Document::from_text(1, "a\nb\nc\nd\n", 4, true);
        d.goto_line(2, 0);
        assert!(d.toggle_bookmark());
        d.goto_line(0, 0);
        d.toggle_bookmark();
        assert_eq!(d.bookmark_lines(), [0, 2]);
        // a line inserted above "c" moves its bookmark down
        d.goto_line(1, 0);
        d.insert_text("x\n", false);
        assert_eq!(d.bookmark_lines(), [0, 3]);
        d.goto_line(1, 0);
        assert!(d.goto_bookmark(true));
        assert_eq!(d.rope.char_to_line(d.primary_sel().head), 3);
        assert!(d.goto_bookmark(true)); // around the end
        assert_eq!(d.rope.char_to_line(d.primary_sel().head), 0);
        assert!(d.goto_bookmark(false));
        assert_eq!(d.rope.char_to_line(d.primary_sel().head), 3);
        assert!(!d.toggle_bookmark());
        assert_eq!(d.bookmark_lines(), [0]);
    }

    #[test]
    fn symbol_at_models_and_identifiers() {
        let d = Document::from_text(1, "    _inherit = 'sale.order'\n    x = self.env.ref(\"base.main_company\")\n", 4, true);
        let at = |line: usize, col: usize| d.symbol_at(d.pos_of(line, col)).map(|(a, b)| d.slice(a, b));
        assert_eq!(at(0, 20).as_deref(), Some("sale.order"));
        assert_eq!(at(0, 6).as_deref(), Some("_inherit"));
        assert_eq!(at(1, 28).as_deref(), Some("base.main_company"));
        assert_eq!(at(1, 13).as_deref(), Some("env"));
        assert_eq!(at(0, 1), None);
    }

    #[test]
    fn replace_all_keeps_cursor_and_undoes_at_once() {
        let mut d = Document::from_text(1, "a=1\nbb=2\n", 4, true);
        d.goto_line(1, 2);
        assert!(d.replace_all("a = 1\nbb = 2\n"));
        assert_eq!(d.rope.char_to_line(d.primary_sel().head), 1);
        assert!(!d.replace_all("a = 1\nbb = 2\n"));
        d.undo();
        assert_eq!(d.rope.to_string(), "a=1\nbb=2\n");
    }

    #[test]
    fn windows_1252_round_trip() {
        let all: Vec<u8> = (1..=255u8).collect();
        let text = Encoding::Windows1252.decode(&all);
        assert_eq!(Encoding::Windows1252.encode(&text).unwrap(), all);
        assert_eq!(Encoding::Windows1252.decode(b"caf\xe9 \x80"), "café €");
        let err = Encoding::Windows1252.encode("a\nb ✓").unwrap_err();
        assert!(err.contains("ligne 2"), "{err}");
    }

    #[test]
    fn latin1_file_saved_unchanged() {
        let p = std::env::temp_dir().join(format!("florm_enc_{}.txt", std::process::id()));
        std::fs::write(&p, b"Cr\xe9\xe9 le \x93test\x94\n").unwrap();
        let hl = crate::highlight::Highlighting::new("Light+ (VS Code)", Path::new("/nonexistent"));
        let mut d = Document::open(1, &p, 4, true, &hl).unwrap();
        assert_eq!(d.encoding, Encoding::Windows1252);
        assert_eq!(d.rope.to_string(), "Créé le “test”\n");
        d.save(false).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"Cr\xe9\xe9 le \x93test\x94\n");
        let _ = std::fs::remove_file(&p);
    }

    fn doc(text: &str) -> Document {
        Document::from_text(1, text, 4, true)
    }

    fn cursors(d: &mut Document, pos: &[usize]) {
        d.sels = pos.iter().map(|&p| Sel::cursor(p)).collect();
        d.normalize();
    }

    #[test]
    fn multi_cursor_typing_and_backspace() {
        let mut d = doc("aa\nbb\ncc");
        cursors(&mut d, &[0, 3, 6]);
        d.insert_text("x", true);
        assert_eq!(d.rope.to_string(), "xaa\nxbb\nxcc");
        d.exec(Cmd::Backspace, true);
        assert_eq!(d.rope.to_string(), "aa\nbb\ncc");
        assert_eq!(d.sels.len(), 3);
    }

    #[test]
    fn auto_pair_and_skip_closer() {
        let mut d = doc("");
        d.insert_text("(", true);
        assert_eq!(d.rope.to_string(), "()");
        d.insert_text(")", true);
        assert_eq!(d.rope.to_string(), "()");
        assert_eq!(d.sels[0].head, 2);
    }

    #[test]
    fn select_next_occurrence() {
        let mut d = doc("foo bar foo baz foo");
        cursors(&mut d, &[1]);
        d.exec(Cmd::SelectNext, true); // expands to word
        d.exec(Cmd::SelectNext, true);
        d.exec(Cmd::SelectNext, true);
        assert_eq!(d.sels.len(), 3);
        d.insert_text("qux", true);
        assert_eq!(d.rope.to_string(), "qux bar qux baz qux");
    }

    #[test]
    fn move_and_duplicate_lines() {
        let mut d = doc("one\ntwo\nthree");
        cursors(&mut d, &[5]);
        d.exec(Cmd::MoveLineUp, true);
        assert_eq!(d.rope.to_string(), "two\none\nthree");
        assert_eq!(d.sels[0].head, 1);
        d.exec(Cmd::MoveLineDown, true);
        d.exec(Cmd::MoveLineDown, true);
        assert_eq!(d.rope.to_string(), "one\nthree\ntwo");
        d.exec(Cmd::DuplicateLine, true);
        assert_eq!(d.rope.to_string(), "one\nthree\ntwo\ntwo");
        d.exec(Cmd::DeleteLine, true);
        assert_eq!(d.rope.to_string(), "one\nthree\ntwo");
    }

    #[test]
    fn toggle_comment_python() {
        let mut d = doc("    a = 1\n    b = 2\n");
        d.path = Some("x.py".into());
        d.sels = vec![Sel::range(0, 15)];
        d.exec(Cmd::ToggleComment, true);
        assert_eq!(d.rope.to_string(), "    # a = 1\n    # b = 2\n");
        d.exec(Cmd::ToggleComment, true);
        assert_eq!(d.rope.to_string(), "    a = 1\n    b = 2\n");
    }

    #[test]
    fn toggle_comment_xml() {
        let mut d = doc("<a/>");
        d.path = Some("x.xml".into());
        d.exec(Cmd::ToggleComment, true);
        assert_eq!(d.rope.to_string(), "<!-- <a/> -->");
        d.exec(Cmd::ToggleComment, true);
        assert_eq!(d.rope.to_string(), "<a/>");
    }

    #[test]
    fn paste_distributes_lines_over_cursors() {
        let mut d = doc("a\nb\nc");
        cursors(&mut d, &[1, 3, 5]);
        d.paste("1\n2\n3", false);
        assert_eq!(d.rope.to_string(), "a1\nb2\nc3");
    }

    #[test]
    fn undo_redo_and_dirty() {
        let mut d = doc("abc");
        cursors(&mut d, &[3]);
        assert!(!d.is_dirty());
        d.insert_text("d", true);
        assert!(d.is_dirty());
        d.undo();
        assert_eq!(d.rope.to_string(), "abc");
        assert!(!d.is_dirty());
        d.redo();
        assert_eq!(d.rope.to_string(), "abcd");
    }

    #[test]
    fn newline_indents_after_colon() {
        let mut d = doc("def f():");
        cursors(&mut d, &[8]);
        d.exec(Cmd::Newline, true);
        assert_eq!(d.rope.to_string(), "def f():\n    ");
    }

    #[test]
    fn split_selection_and_column_cursors() {
        let mut d = doc("ab\ncd\nef");
        d.sels = vec![Sel::range(0, 8)];
        d.exec(Cmd::SplitLines, true);
        assert_eq!(d.sels.len(), 3);
        cursors(&mut d, &[1]);
        d.exec(Cmd::AddCursorBelow, true);
        d.exec(Cmd::AddCursorBelow, true);
        d.insert_text("-", true);
        assert_eq!(d.rope.to_string(), "a-b\nc-d\ne-f");
    }

    #[test]
    fn revert_git_hunks() {
        let base = "a\nb\nc\nd";
        let mut d = doc("a\nB\nc\nnew\nd");
        d.git_base = Some(base.into());
        d.refresh_diff();
        assert_eq!(d.hunks.len(), 2);
        assert_eq!(d.git_marks.get(&1), Some(&GitMark::Modified));
        assert_eq!(d.git_marks.get(&3), Some(&GitMark::Added));
        assert_eq!(d.head_line(0), Some(0));
        assert_eq!(d.head_line(1), Some(1)); // modified line: same place in HEAD
        assert_eq!(d.head_line(3), None); // added line
        assert_eq!(d.head_line(4), Some(3)); // shifted by the added line
        d.revert_hunk(1);
        assert_eq!(d.rope.to_string(), "a\nB\nc\nd");
        d.revert_hunk(0);
        assert_eq!(d.rope.to_string(), base);
        assert!(d.hunks.is_empty());
        d.undo();
        assert_eq!(d.rope.to_string(), "a\nB\nc\nd");
        // deletion at the end of the file
        let mut d = doc("a\n");
        d.git_base = Some("a\nb\n".into());
        d.refresh_diff();
        d.revert_hunk(0);
        assert_eq!(d.rope.to_string(), "a\nb\n");
    }

    #[test]
    fn noqa_comments() {
        let mut d = doc("import os\nx = 1  # noqa: E501\n");
        d.add_noqa(0, &["F401".to_string()]);
        d.add_noqa(1, &["E225".to_string(), "E501".to_string()]);
        assert_eq!(d.rope.to_string(), "import os  # noqa: F401\nx = 1  # noqa: E501, E225\n");
    }

    #[test]
    fn brackets_match() {
        let d = doc("f(a[1], {b})");
        assert_eq!(d.matching_bracket(1), Some((1, 11)));
        assert_eq!(d.matching_bracket(12), Some((11, 1)));
    }

    #[test]
    fn folding_follows_edits_and_moves() {
        let mut d = doc("class A:\n    def f(self):\n        x = 1\n        y = 2\n\n    def g(self):\n        pass\nz = 3\n");
        assert!(d.can_fold(0) && d.can_fold(1) && !d.can_fold(2));
        assert_eq!(d.fold_range(1), Some((2, 3)));
        assert_eq!(d.fold_range(0), Some((1, 6)));
        assert!(d.fold(1));
        assert_eq!(d.hidden_ranges(), vec![(2, 3)]);
        // moving down from the folded line jumps over the block
        let p = d.rope.line_to_char(1);
        cursors(&mut d, &[p]);
        d.move_sels(Motion::Down, false);
        assert_eq!(d.rope.char_to_line(d.primary_sel().head), 4);
        d.move_sels(Motion::Up, false);
        assert_eq!(d.rope.char_to_line(d.primary_sel().head), 1);
        // a line inserted above shifts the fold
        cursors(&mut d, &[0]);
        d.insert_text("import os\n", false);
        assert_eq!(d.fold_lines(), vec![2]);
        assert_eq!(d.hidden_ranges(), vec![(3, 4)]);
        // going to a hidden line opens the block
        d.goto_line(3, 0);
        assert!(d.folds.is_empty());
        d.fold_all();
        assert_eq!(d.hidden_ranges(), vec![(2, 7)]);
        d.unfold_all();
        assert!(d.hidden_ranges().is_empty());
    }

    #[test]
    fn preserve_case_and_scoped_replacements() {
        assert_eq!(preserve_case("chat", "Chien"), "chien");
        assert_eq!(preserve_case("Chat", "chien"), "Chien");
        assert_eq!(preserve_case("CHAT", "chien"), "CHIEN");
        assert_eq!(preserve_case("cHaT", "chien"), "chien");
        assert_eq!(preserve_case("_x1", "y"), "y");
        let d = doc("chat Chat CHAT\nchat\n");
        let re = build_regex("chat", false, false, false).unwrap();
        let r = replacements(&d.rope, &re, "chien", false, true, None);
        let v: Vec<&str> = r.iter().map(|x| x.2.as_str()).collect();
        assert_eq!(v, ["chien", "Chien", "CHIEN", "chien"]);
        // only inside the first line
        assert_eq!(replacements(&d.rope, &re, "x", false, false, Some((0, 14))).len(), 3);
        let re = build_regex(r"(\w)hat", false, true, false).unwrap();
        assert_eq!(replacements(&d.rope, &re, "${1}ien", true, false, Some((5, 9)))[0].2, "Cien");
    }

    #[test]
    fn merge_conflicts() {
        let src = "a\n<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> feature\nb\n<<<<<<< ours\nx\n||||||| base\nold\n=======\ny\n>>>>>>> other\n";
        let mut d = doc(src);
        let c = d.conflicts();
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].start, c[0].mid, c[0].end, c[0].ours.as_str(), c[0].theirs.as_str()), (1, 3, 5, "HEAD", "feature"));
        assert_eq!(c[1].base, Some(9));
        d.resolve_conflict(&c[1], Resolution::Both);
        assert_eq!(d.rope.to_string(), "a\n<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> feature\nb\nx\ny\n");
        let c = d.conflicts();
        d.resolve_conflict(&c[0], Resolution::Theirs);
        assert_eq!(d.rope.to_string(), "a\ntheirs\nb\nx\ny\n");
        assert!(d.conflicts().is_empty());
        d.undo();
        let first = d.conflicts()[0].clone();
        d.resolve_conflict(&first, Resolution::Ours);
        assert_eq!(d.rope.to_string(), "a\nmine\nb\nx\ny\n");
    }

    #[test]
    fn line_ranges() {
        assert_eq!(parse_line_range("", 100, 7), Some((7, 7)));
        assert_eq!(parse_line_range("42", 100, 0), Some((41, 41)));
        assert_eq!(parse_line_range(" 20-10 ", 100, 0), Some((9, 19)));
        assert_eq!(parse_line_range("10..12", 100, 0), Some((9, 11)));
        assert_eq!(parse_line_range("5:3", 100, 0), Some((4, 4)));
        assert_eq!(parse_line_range("999", 100, 0), Some((99, 99)));
        assert_eq!(parse_line_range("abc", 100, 0), None);
        let mut d = doc("a\nbb\nccc\nd");
        d.select_lines(1, 2);
        assert_eq!(d.slice(d.primary_sel().start(), d.primary_sel().end()), "bb\nccc\n");
    }
}
