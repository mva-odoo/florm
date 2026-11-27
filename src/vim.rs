//! Vim mode: the keys of Vim's normal, insert and visual modes on a document (the primary cursor).
//! The keys come as chars (the text typed, so that any keyboard layout works) and a few control
//! chars for the other keys: Escape, Enter, Backspace, Ctrl+R/D/U/F/B. What concerns the app
//! (saving, searching, the command line, the clipboard) comes back as `Out`.

use crate::commands::Cmd;
use crate::document::{Document, Edit, EditKind, Sel};

pub const ESC: char = '\u{1b}';
pub const ENTER: char = '\r';
pub const BACKSPACE: char = '\u{8}';
/// Ctrl+letter as its control char (Ctrl+R: 0x12).
pub fn ctrl(c: char) -> char {
    char::from_u32((c.to_ascii_lowercase() as u32).wrapping_sub('a' as u32) + 1).unwrap_or(ESC)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
    VisualLine,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => t!("INSERTION", "INSERT"),
            Mode::Visual => t!("VISUEL", "VISUAL"),
            Mode::VisualLine => t!("VISUEL LIGNE", "VISUAL LINE"),
        }
    }
}

/// What the app does for a key.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// `/` (true) or `?`: the search field.
    Find(bool),
    /// `n` (true) / `N`: the next / previous result.
    FindNext(bool),
    /// `*` (true) / `#`: the word under the cursor searched.
    Word(String, bool),
    /// `:` the command line.
    CommandLine,
    /// Yanked or deleted: also in the clipboard (linewise or not).
    Clipboard(String, bool),
    /// `ZZ`: save and close; `ZQ`: close.
    SaveClose,
    Close,
}

#[derive(Default)]
pub struct Vim {
    pub mode: Mode,
    /// The keys of the command being typed (`2d`, `ci`…), shown in the status bar.
    pub pending: String,
    /// The unnamed register: its text and whether it is whole lines.
    register: (String, bool),
    /// The keys of the last change, replayed by `.`.
    last_change: Vec<char>,
    /// The change being recorded (its command, then what is typed in insert mode).
    rec: Option<Vec<char>>,
    replaying: bool,
    /// The last f/F/t/T and its char, for `;` and `,`.
    last_find: Option<(char, char)>,
    /// Visual mode: where it started and the cursor (on a char, included).
    anchor: usize,
    head: usize,
    /// The column j / k keep (usize::MAX after `$`).
    goal: Option<usize>,
}

/// Where a motion goes and how it counts for an operator.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Exclusive,
    Inclusive,
    Linewise,
}

enum P<T> {
    /// More keys needed.
    More,
    Bad,
    Ok(T),
}

// ------------------------------------------------------------------ text helpers

fn len(d: &Document) -> usize {
    d.len()
}

fn ch(d: &Document, p: usize) -> char {
    d.char_at(p).unwrap_or('\n')
}

fn line_of(d: &Document, p: usize) -> usize {
    d.rope.char_to_line(p.min(d.len()))
}

fn line_start(d: &Document, l: usize) -> usize {
    d.rope.line_to_char(l)
}

fn line_end(d: &Document, l: usize) -> usize {
    line_start(d, l) + d.line_len(l)
}

/// The last line that has text (ropey counts an empty one after a final newline).
fn last_line(d: &Document) -> usize {
    let n = d.rope.len_lines();
    if n > 1 && d.line_len(n - 1) == 0 && d.len() > 0 {
        n - 2
    } else {
        n - 1
    }
}

fn first_non_blank(d: &Document, l: usize) -> usize {
    let s = line_start(d, l);
    let e = line_end(d, l);
    (s..e).find(|&p| !matches!(ch(d, p), ' ' | '\t')).unwrap_or(e)
}

/// 0: blank, 1: word, 2: punctuation (`big`: words are all the non-blank chars).
fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        0
    } else if big || c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

fn set_cursor(d: &mut Document, p: usize) {
    d.sels = vec![Sel::cursor(p.min(d.len()))];
    d.primary = 0;
    d.scroll_to_cursor = true;
}

fn head(d: &Document) -> usize {
    d.primary_sel().head.min(d.len())
}

/// In normal mode the cursor is on a char: not after the last one of a line.
fn clamp_normal(d: &Document, p: usize) -> usize {
    let l = line_of(d, p);
    let (s, e) = (line_start(d, l), line_end(d, l));
    if e > s && p >= e {
        e - 1
    } else {
        p.max(s).min(e)
    }
}

fn read_count(s: &[char], i: &mut usize) -> Option<usize> {
    let start = *i;
    if s.get(*i).is_some_and(|c| *c == '0') {
        return None;
    }
    while s.get(*i).is_some_and(|c| c.is_ascii_digit()) {
        *i += 1;
    }
    (*i > start).then(|| s[start..*i].iter().collect::<String>().parse().unwrap_or(1).min(100_000))
}

// ------------------------------------------------------------------ motions

fn word_forward(d: &Document, mut p: usize, big: bool) -> usize {
    let n = len(d);
    if p >= n {
        return n;
    }
    let c = class(ch(d, p), big);
    if c != 0 {
        while p < n && class(ch(d, p), big) == c {
            p += 1;
        }
    }
    while p < n && class(ch(d, p), big) == 0 {
        // an empty line is a word
        if ch(d, p) == '\n' && p + 1 < n && ch(d, p + 1) == '\n' {
            return p + 1;
        }
        p += 1;
    }
    p
}

fn word_end(d: &Document, mut p: usize, big: bool) -> usize {
    let n = len(d);
    if p + 1 >= n {
        return p;
    }
    p += 1;
    while p < n && class(ch(d, p), big) == 0 {
        p += 1;
    }
    let c = class(ch(d, p), big);
    while p + 1 < n && class(ch(d, p + 1), big) == c && c != 0 {
        p += 1;
    }
    p.min(n.saturating_sub(1))
}

fn word_back(d: &Document, mut p: usize, big: bool) -> usize {
    if p == 0 {
        return 0;
    }
    p -= 1;
    while p > 0 && class(ch(d, p), big) == 0 {
        p -= 1;
    }
    let c = class(ch(d, p), big);
    while p > 0 && class(ch(d, p - 1), big) == c {
        p -= 1;
    }
    p
}

/// f / F / t / T `c` on the line of `p`, `count` times.
fn find_in_line(d: &Document, p: usize, kind: char, c: char, count: usize) -> Option<usize> {
    let l = line_of(d, p);
    let (s, e) = (line_start(d, l), line_end(d, l));
    let mut at = p;
    for _ in 0..count {
        at = match kind {
            'f' | 't' => {
                let from = if kind == 't' { at + 2 } else { at + 1 };
                let from = if kind == 't' && at + 1 < e && ch(d, at + 1) == c { at + 2 } else { from };
                (from.min(e)..e).find(|&q| ch(d, q) == c).map(|q| if kind == 't' { q - 1 } else { q })?
            }
            _ => {
                let to = if kind == 'T' && at > s && ch(d, at - 1) == c { at - 1 } else { at };
                (s..to).rev().find(|&q| ch(d, q) == c).map(|q| if kind == 'T' { q + 1 } else { q })?
            }
        };
    }
    Some(at)
}

/// The previous (`back`) / next empty line, or the start / end of the text.
fn paragraph(d: &Document, p: usize, back: bool, count: usize) -> usize {
    let mut l = line_of(d, p);
    let last = last_line(d);
    for _ in 0..count {
        if back {
            while l > 0 && d.line_len(l - 1) == 0 {
                l -= 1;
            }
            while l > 0 && d.line_len(l - 1) > 0 {
                l -= 1;
            }
            l = l.saturating_sub(1);
            if l == 0 {
                return 0;
            }
        } else {
            while l < last && d.line_len(l + 1) == 0 {
                l += 1;
            }
            while l < last && d.line_len(l + 1) > 0 {
                l += 1;
            }
            if l >= last {
                return line_end(d, last);
            }
            l += 1;
        }
    }
    line_start(d, l)
}

// ------------------------------------------------------------------ text objects

/// `iw` / `aw` (`big`: `iW` / `aW`) around `p`.
fn word_object(d: &Document, p: usize, around: bool, big: bool) -> (usize, usize) {
    let n = len(d);
    let l = line_of(d, p);
    let (s, e) = (line_start(d, l), line_end(d, l));
    if p >= e {
        return (p, p);
    }
    let c = class(ch(d, p), big);
    let (mut a, mut b) = (p, p + 1);
    while a > s && class(ch(d, a - 1), big) == c {
        a -= 1;
    }
    while b < e && class(ch(d, b), big) == c {
        b += 1;
    }
    if around && c != 0 {
        // the blanks after it, else those before
        let mut b2 = b;
        while b2 < e && class(ch(d, b2), big) == 0 {
            b2 += 1;
        }
        if b2 > b {
            b = b2;
        } else {
            while a > s && class(ch(d, a - 1), big) == 0 {
                a -= 1;
            }
        }
    }
    (a, b.min(n))
}

/// `i"` / `a"` (and ' `) on the line of `p`: the quoted text around it, or the next one.
fn quote_object(d: &Document, p: usize, q: char, around: bool) -> Option<(usize, usize)> {
    let l = line_of(d, p);
    let (s, e) = (line_start(d, l), line_end(d, l));
    let quotes: Vec<usize> = (s..e).filter(|&k| ch(d, k) == q && (k == s || ch(d, k - 1) != '\\')).collect();
    let pair = quotes.chunks(2).filter(|c| c.len() == 2).find(|c| p <= c[1]).map(|c| (c[0], c[1]))?;
    Some(if around { (pair.0, pair.1 + 1) } else { (pair.0 + 1, pair.1) })
}

/// `i(` / `a(` (and [ { <) around `p`.
fn bracket_object(d: &Document, p: usize, open: char, close: char, around: bool) -> Option<(usize, usize)> {
    let n = len(d);
    // the open bracket before (or under) the cursor that is not closed before it
    let mut depth = 0;
    let mut a = None;
    let mut k = (p + 1).min(n);
    if p < n && ch(d, p) == close {
        k = p;
        depth = -1;
    }
    while k > 0 {
        k -= 1;
        let c = ch(d, k);
        if c == close && k != p {
            depth += 1;
        } else if c == open {
            if depth <= 0 {
                a = Some(k);
                break;
            }
            depth -= 1;
        }
    }
    let a = a?;
    let mut depth = 0;
    let mut b = None;
    for k in a + 1..n {
        let c = ch(d, k);
        if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                b = Some(k);
                break;
            }
            depth -= 1;
        }
    }
    let b = b?;
    Some(if around { (a, b + 1) } else { (a + 1, b) })
}

/// `ip` / `ap`: the lines of the paragraph around `p` (`ap`: with the empty lines after it).
fn paragraph_object(d: &Document, p: usize, around: bool) -> (usize, usize) {
    let last = last_line(d);
    let l = line_of(d, p);
    let empty = |k: usize| d.line_len(k) == 0;
    let here = empty(l);
    let (mut a, mut b) = (l, l);
    while a > 0 && empty(a - 1) == here {
        a -= 1;
    }
    while b < last && empty(b + 1) == here {
        b += 1;
    }
    if around {
        while b < last && empty(b + 1) != here {
            b += 1;
        }
    }
    (a, b)
}

impl Vim {
    pub fn new() -> Self {
        Self::default()
    }

    /// The selection shown in visual mode.
    fn show_visual(&self, d: &mut Document) {
        let (a, h) = (self.anchor.min(d.len()), self.head.min(d.len()));
        let sel = match self.mode {
            Mode::VisualLine => {
                let (l0, l1) = (line_of(d, a.min(h)), line_of(d, a.max(h)));
                let end = d.line_end_incl(l1);
                if h >= a {
                    Sel::range(line_start(d, l0), end)
                } else {
                    Sel::range(end, line_start(d, l0))
                }
            }
            _ if h >= a => Sel::range(a, (h + 1).min(d.len())),
            _ => Sel::range((a + 1).min(d.len()), h),
        };
        d.sels = vec![sel];
        d.primary = 0;
        d.scroll_to_cursor = true;
    }

    /// The app saw a selection made otherwise (mouse, search): visual mode on it. Several cursors,
    /// or the cursor after the end of a line: brought back to one, on a char.
    pub fn sync(&mut self, d: &mut Document) {
        match self.mode {
            Mode::Normal => {
                let s = d.primary_sel();
                if !s.is_empty() {
                    self.mode = Mode::Visual;
                    self.anchor = s.anchor;
                    self.head = if s.head > s.anchor { s.head - 1 } else { s.head };
                    self.show_visual(d);
                } else if d.sels.len() > 1 || clamp_normal(d, s.head) != s.head {
                    set_cursor(d, clamp_normal(d, s.head));
                }
            }
            Mode::Visual | Mode::VisualLine => {
                let s = d.primary_sel();
                if s.is_empty() {
                    self.mode = Mode::Normal;
                    set_cursor(d, clamp_normal(d, s.head));
                }
            }
            Mode::Insert => {}
        }
    }

    /// What is typed in insert mode (live, by the editor), recorded for `.`.
    pub fn typed(&mut self, t: &str) {
        if let Some(r) = self.rec.as_mut() {
            r.extend(t.chars().map(|c| if c == '\n' { ENTER } else { c }));
        }
    }

    /// A key of the user. In insert mode only Escape is the mode's (the editor types the rest).
    pub fn key(&mut self, d: &mut Document, c: char) -> Vec<Out> {
        let mut out = Vec::new();
        self.feed(d, c, &mut out);
        out
    }

    fn feed(&mut self, d: &mut Document, c: char, out: &mut Vec<Out>) {
        if self.mode == Mode::Insert {
            if c == ESC {
                self.leave_insert(d);
                return;
            }
            // replaying `.`: the text typed then
            if self.replaying {
                match c {
                    ENTER => d.exec(Cmd::Newline, false),
                    BACKSPACE => d.exec(Cmd::Backspace, false),
                    _ => d.insert_text(&c.to_string(), false),
                }
            }
            return;
        }
        if c == ESC && self.pending.is_empty() {
            if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                self.mode = Mode::Normal;
                set_cursor(d, clamp_normal(d, self.head));
            } else {
                set_cursor(d, clamp_normal(d, head(d)));
            }
            return;
        }
        if c == ESC {
            self.pending.clear();
            return;
        }
        self.pending.push(c);
        let keys: Vec<char> = self.pending.chars().collect();
        let before = self.mode;
        match self.run(d, &keys, out) {
            P::More => {}
            P::Bad => self.pending.clear(),
            P::Ok(change) => {
                self.pending.clear();
                if change && !self.replaying {
                    if self.mode == Mode::Insert {
                        self.rec = Some(keys);
                    } else {
                        self.last_change = keys;
                    }
                } else if change && before != Mode::Normal && !self.replaying {
                    self.last_change.clear();
                }
                if self.mode == Mode::Normal {
                    let h = head(d);
                    if d.primary_sel().is_empty() {
                        set_cursor(d, clamp_normal(d, h));
                    }
                }
            }
        }
    }

    fn leave_insert(&mut self, d: &mut Document) {
        self.mode = Mode::Normal;
        if let Some(mut r) = self.rec.take() {
            r.push(ESC);
            if !self.replaying {
                self.last_change = r;
            }
        }
        // the cursor goes back onto the last char typed
        let h = head(d);
        let s = line_start(d, line_of(d, h));
        set_cursor(d, clamp_normal(d, if h > s { h - 1 } else { h }));
    }

    fn enter_insert(&mut self, d: &mut Document, at: usize) {
        self.mode = Mode::Insert;
        set_cursor(d, at);
    }

    /// A motion at `keys[*i..]`: where it goes from `from`. Not a motion: Bad.
    fn motion(&mut self, d: &Document, keys: &[char], i: &mut usize, count: Option<usize>, from: usize) -> P<(usize, Kind)> {
        let Some(&c) = keys.get(*i) else { return P::More };
        *i += 1;
        let n = count.unwrap_or(1);
        let l = line_of(d, from);
        let col_goal = |vim: &mut Vim, d: &Document| -> usize {
            *vim.goal.get_or_insert_with(|| from - line_start(d, line_of(d, from)))
        };
        let horizontal = !matches!(c, 'j' | 'k' | '\u{4}' | '\u{15}' | '\u{6}' | '\u{2}' | 'G');
        if horizontal && c != 'g' {
            self.goal = None;
        }
        let r = match c {
            'h' => (from.saturating_sub(n).max(line_start(d, l)), Kind::Exclusive),
            'l' | ' ' => ((from + n).min(line_end(d, l)), Kind::Exclusive),
            'j' | 'k' | '\u{4}' | '\u{15}' | '\u{6}' | '\u{2}' => {
                let page = d.page_lines.max(2);
                let steps = match c {
                    '\u{4}' | '\u{15}' => page / 2,
                    '\u{6}' | '\u{2}' => page,
                    _ => n,
                };
                let down = matches!(c, 'j' | '\u{4}' | '\u{6}');
                let target = if down { (l + steps).min(last_line(d)) } else { l.saturating_sub(steps) };
                let goal = col_goal(self, d);
                let col = goal.min(d.line_len(target).saturating_sub(1));
                (line_start(d, target) + col, Kind::Linewise)
            }
            'w' | 'W' => {
                let mut p = from;
                for _ in 0..n {
                    p = word_forward(d, p, c == 'W');
                }
                (p, Kind::Exclusive)
            }
            'e' | 'E' => {
                let mut p = from;
                for _ in 0..n {
                    p = word_end(d, p, c == 'E');
                }
                (p, Kind::Inclusive)
            }
            'b' | 'B' => {
                let mut p = from;
                for _ in 0..n {
                    p = word_back(d, p, c == 'B');
                }
                (p, Kind::Exclusive)
            }
            '0' => (line_start(d, l), Kind::Exclusive),
            '^' | '_' => (first_non_blank(d, l), Kind::Exclusive),
            '$' => {
                self.goal = Some(usize::MAX);
                let t = (l + n - 1).min(last_line(d));
                (line_end(d, t).saturating_sub(1).max(line_start(d, t)), Kind::Inclusive)
            }
            'G' => {
                let t = count.map_or(last_line(d), |k| (k - 1).min(last_line(d)));
                (first_non_blank(d, t), Kind::Linewise)
            }
            'g' => match keys.get(*i) {
                None => return P::More,
                Some('g') => {
                    *i += 1;
                    let t = count.map_or(0, |k| (k - 1).min(last_line(d)));
                    (first_non_blank(d, t), Kind::Linewise)
                }
                Some('_') => {
                    *i += 1;
                    let e = line_end(d, l);
                    let p = (line_start(d, l)..e).rev().find(|&p| !matches!(ch(d, p), ' ' | '\t')).unwrap_or(line_start(d, l));
                    (p, Kind::Inclusive)
                }
                _ => return P::Bad,
            },
            'f' | 'F' | 't' | 'T' => {
                let Some(&target) = keys.get(*i) else { return P::More };
                *i += 1;
                self.last_find = Some((c, target));
                match find_in_line(d, from, c, target, n) {
                    Some(p) => (p, if matches!(c, 'f' | 't') { Kind::Inclusive } else { Kind::Exclusive }),
                    None => return P::Bad,
                }
            }
            ';' | ',' => {
                let Some((k, target)) = self.last_find else { return P::Bad };
                let k = if c == ',' {
                    match k {
                        'f' => 'F',
                        'F' => 'f',
                        't' => 'T',
                        _ => 't',
                    }
                } else {
                    k
                };
                match find_in_line(d, from, k, target, n) {
                    Some(p) => (p, if matches!(k, 'f' | 't') { Kind::Inclusive } else { Kind::Exclusive }),
                    None => return P::Bad,
                }
            }
            '%' => match d.matching_bracket(from) {
                Some((a, b)) => (if from == a { b } else { a }, Kind::Inclusive),
                None => return P::Bad,
            },
            '}' => (paragraph(d, from, false, n), Kind::Exclusive),
            '{' => (paragraph(d, from, true, n), Kind::Exclusive),
            _ => return P::Bad,
        };
        P::Ok(r)
    }

    /// A text object (`iw`, `a(`…) at `keys[*i..]`: its range and whether it is whole lines.
    fn object(&self, d: &Document, keys: &[char], i: &mut usize, from: usize) -> P<(usize, usize, bool)> {
        let (Some(&ia), Some(&o)) = (keys.get(*i), keys.get(*i + 1)) else { return P::More };
        if !matches!(ia, 'i' | 'a') {
            return P::Bad;
        }
        *i += 2;
        let around = ia == 'a';
        let r = match o {
            'w' | 'W' => {
                let (a, b) = word_object(d, from, around, o == 'W');
                Some((a, b, false))
            }
            '"' | '\'' | '`' => quote_object(d, from, o, around).map(|(a, b)| (a, b, false)),
            '(' | ')' | 'b' => bracket_object(d, from, '(', ')', around).map(|(a, b)| (a, b, false)),
            '[' | ']' => bracket_object(d, from, '[', ']', around).map(|(a, b)| (a, b, false)),
            '{' | '}' | 'B' => bracket_object(d, from, '{', '}', around).map(|(a, b)| (a, b, false)),
            '<' | '>' => bracket_object(d, from, '<', '>', around).map(|(a, b)| (a, b, false)),
            'p' => {
                let (a, b) = paragraph_object(d, from, around);
                Some((line_start(d, a), d.line_end_incl(b), true))
            }
            _ => None,
        };
        match r {
            Some(r) => P::Ok(r),
            None => P::Bad,
        }
    }

    /// Runs the command in `keys`: More while incomplete; Ok(it changed the text).
    fn run(&mut self, d: &mut Document, keys: &[char], out: &mut Vec<Out>) -> P<bool> {
        let visual = matches!(self.mode, Mode::Visual | Mode::VisualLine);
        let mut i = 0;
        let count = read_count(keys, &mut i);
        let Some(&c) = keys.get(i) else { return P::More };
        let from = if visual { self.head } else { head(d) };
        let n = count.unwrap_or(1);
        // a motion: the cursor (or the end of the visual selection) goes there
        let command = matches!(c, 'd' | 'c' | 'y' | '>' | '<' | 'x' | 'X' | 's' | 'S' | 'D' | 'C' | 'Y' | 'p' | 'P' | 'u' | 'J' | '~' | 'r' | 'i' | 'a' | 'I' | 'A' | 'o' | 'O' | 'v' | 'V' | 'n' | 'N' | '*' | '#' | '/' | '?' | ':' | 'z' | 'Z' | '.' | '\u{12}')
            || (c == 'g' && keys.get(i + 1) == Some(&'J'));
        if !command {
            let mut j = i;
            return match self.motion(d, keys, &mut j, count, from) {
                P::More => P::More,
                P::Bad => P::Bad,
                P::Ok((p, _)) => {
                    if visual {
                        self.head = p.min(d.len().saturating_sub(1).max(0));
                        self.show_visual(d);
                    } else {
                        set_cursor(d, clamp_normal(d, p));
                        if matches!(c, '\u{4}' | '\u{15}' | '\u{6}' | '\u{2}') {
                            d.center_cursor = true;
                        }
                    }
                    P::Ok(false)
                }
            };
        }
        if visual {
            return self.visual_command(d, keys, i, out);
        }
        let l = line_of(d, from);
        match c {
            'i' => self.enter_insert(d, from),
            'a' => {
                let e = line_end(d, l);
                self.enter_insert(d, (from + 1).min(e));
            }
            'I' => self.enter_insert(d, first_non_blank(d, l)),
            'A' => self.enter_insert(d, line_end(d, l)),
            'o' => {
                // a new line as Enter at the end of the line makes it (indented after a « : »)
                set_cursor(d, line_end(d, l));
                d.exec(Cmd::Newline, false);
                self.mode = Mode::Insert;
            }
            'O' => {
                d.exec(Cmd::InsertLineBefore, false);
                self.mode = Mode::Insert;
            }
            'v' => {
                self.mode = Mode::Visual;
                (self.anchor, self.head) = (from, from);
                self.show_visual(d);
                return P::Ok(false);
            }
            'V' => {
                self.mode = Mode::VisualLine;
                (self.anchor, self.head) = (from, from);
                self.show_visual(d);
                return P::Ok(false);
            }
            'u' => {
                for _ in 0..n {
                    d.undo();
                }
                let h = head(d);
                set_cursor(d, clamp_normal(d, h));
                return P::Ok(false);
            }
            '\u{12}' => {
                for _ in 0..n {
                    d.redo();
                }
                let h = head(d);
                set_cursor(d, clamp_normal(d, h));
                return P::Ok(false);
            }
            '.' => {
                let keys = self.last_change.clone();
                if keys.is_empty() {
                    return P::Ok(false);
                }
                self.replaying = true;
                let saved = std::mem::take(&mut self.pending);
                for _ in 0..n {
                    for &k in &keys {
                        self.feed(d, k, out);
                    }
                    if self.mode == Mode::Insert {
                        self.leave_insert(d);
                    }
                }
                self.pending = saved;
                self.replaying = false;
                return P::Ok(false);
            }
            'n' | 'N' => {
                out.push(Out::FindNext(c == 'n'));
                return P::Ok(false);
            }
            '*' | '#' => {
                let (a, b) = word_object(d, from, false, false);
                let w = d.slice(a, b);
                if w.trim().is_empty() {
                    return P::Bad;
                }
                out.push(Out::Word(w, c == '*'));
                return P::Ok(false);
            }
            '/' | '?' => {
                out.push(Out::Find(c == '/'));
                return P::Ok(false);
            }
            ':' => {
                out.push(Out::CommandLine);
                return P::Ok(false);
            }
            'z' => {
                let Some(&k) = keys.get(i + 1) else { return P::More };
                if matches!(k, 'z' | '.' | 't' | 'b' | ENTER | '-') {
                    d.center_cursor = true;
                    return P::Ok(false);
                }
                return P::Bad;
            }
            'Z' => {
                let Some(&k) = keys.get(i + 1) else { return P::More };
                match k {
                    'Z' => out.push(Out::SaveClose),
                    'Q' => out.push(Out::Close),
                    _ => return P::Bad,
                }
                return P::Ok(false);
            }
            'x' | 'X' | 's' | 'D' | 'C' => {
                let e = line_end(d, l);
                let (a, b) = match c {
                    'x' | 's' => (from, (from + n).min(e)),
                    'X' => (from.saturating_sub(n).max(line_start(d, l)), from),
                    _ => (from, line_end(d, (l + n - 1).min(last_line(d)))),
                };
                let op = if matches!(c, 's' | 'C') { 'c' } else { 'd' };
                if a == b && op == 'd' {
                    return P::Ok(false);
                }
                self.apply(d, op, a, b, false, out);
            }
            'S' => {
                let l1 = (l + n - 1).min(last_line(d));
                self.apply(d, 'c', line_start(d, l), d.line_end_incl(l1), true, out);
            }
            'Y' => {
                let l1 = (l + n - 1).min(last_line(d));
                self.apply(d, 'y', line_start(d, l), d.line_end_incl(l1), true, out);
                return P::Ok(false);
            }
            'p' | 'P' => {
                self.put(d, c == 'p', n);
            }
            'r' => {
                let Some(&k) = keys.get(i + 1) else { return P::More };
                let e = line_end(d, l);
                if k == ESC || from + n > e {
                    return P::Bad;
                }
                let text: String = if k == ENTER { "\n".into() } else { std::iter::repeat_n(k, n).collect() };
                d.edit(EditKind::Other, vec![Edit { start: from, end: from + n, text: text.clone(), anchor: 0, head: 0 }]);
                set_cursor(d, from + text.chars().count() - 1);
            }
            'J' | 'g' => {
                // gJ: no space between (taken as J here)
                let l1 = (l + n.max(2) - 1).min(last_line(d));
                if l1 == l {
                    return P::Bad;
                }
                d.sels = vec![Sel::range(line_start(d, l), line_end(d, l1))];
                d.exec(Cmd::JoinLines, false);
                let h = head(d);
                set_cursor(d, h);
            }
            '~' => {
                let e = line_end(d, l);
                let b = (from + n).min(e);
                if from == b {
                    return P::Bad;
                }
                let t: String = d.slice(from, b).chars().map(swap_case).collect();
                d.edit(EditKind::Other, vec![Edit { start: from, end: b, text: t, anchor: 0, head: 0 }]);
                set_cursor(d, b.min(e.saturating_sub(1)));
            }
            'd' | 'c' | 'y' | '>' | '<' => {
                let mut j = i + 1;
                let count2 = read_count(keys, &mut j);
                let n = n * count2.unwrap_or(1);
                let total = (count.is_some() || count2.is_some()).then_some(n);
                let Some(&m) = keys.get(j) else { return P::More };
                // dd, cc, yy, >>, <<: whole lines
                if m == c {
                    let l1 = (l + n - 1).min(last_line(d));
                    self.apply(d, c, line_start(d, l), d.line_end_incl(l1), true, out);
                    return P::Ok(c != 'y');
                }
                if matches!(m, 'i' | 'a') {
                    return match self.object(d, keys, &mut j, from) {
                        P::More => P::More,
                        P::Bad => P::Bad,
                        P::Ok((a, b, lw)) => {
                            self.apply(d, c, a, b, lw, out);
                            P::Ok(c != 'y')
                        }
                    };
                }
                // cw on a word: up to the end of this word (then of the next ones), the blank after it kept
                let big = m == 'W';
                let (p, kind) = if c == 'c' && matches!(m, 'w' | 'W') && class(ch(d, from), big) != 0 {
                    let k = class(ch(d, from), big);
                    let mut p = from;
                    while p + 1 < d.len() && class(ch(d, p + 1), big) == k {
                        p += 1;
                    }
                    for _ in 1..n {
                        p = word_end(d, p, big);
                    }
                    (p, Kind::Inclusive)
                } else {
                    match self.motion(d, keys, &mut j, total, from) {
                        P::More => return P::More,
                        P::Bad => return P::Bad,
                        P::Ok(r) => r,
                    }
                };
                let (mut a, mut b) = if p >= from { (from, p) } else { (p, from) };
                match kind {
                    Kind::Linewise => {
                        let (la, lb) = (line_of(d, a), line_of(d, b));
                        self.apply(d, c, line_start(d, la), d.line_end_incl(lb), true, out);
                        return P::Ok(c != 'y');
                    }
                    Kind::Inclusive => b = (b + 1).min(d.len()),
                    Kind::Exclusive => {
                        // dw at the last word of a line stops at its end
                        if matches!(m, 'w' | 'W') && line_of(d, b) > line_of(d, a) {
                            b = line_end(d, line_of(d, a)).max(a);
                        }
                    }
                }
                if a == b {
                    return P::Bad;
                }
                a = a.min(d.len());
                self.apply(d, c, a, b, false, out);
                return P::Ok(c != 'y');
            }
            _ => return P::Bad,
        }
        P::Ok(true)
    }

    /// An operator, an x, a ~… on the visual selection.
    fn visual_command(&mut self, d: &mut Document, keys: &[char], i: usize, out: &mut Vec<Out>) -> P<bool> {
        let c = keys[i];
        let lines = self.mode == Mode::VisualLine;
        let (a, h) = (self.anchor.min(self.head), self.anchor.max(self.head));
        let (start, end) = if lines { (line_start(d, line_of(d, a)), d.line_end_incl(line_of(d, h))) } else { (a, (h + 1).min(d.len())) };
        match c {
            'v' | 'V' => {
                let want = if c == 'v' { Mode::Visual } else { Mode::VisualLine };
                if self.mode == want {
                    self.mode = Mode::Normal;
                    set_cursor(d, clamp_normal(d, self.head));
                } else {
                    self.mode = want;
                    self.show_visual(d);
                }
                P::Ok(false)
            }
            'o' => {
                std::mem::swap(&mut self.anchor, &mut self.head);
                self.show_visual(d);
                P::Ok(false)
            }
            'i' | 'a' => {
                let mut j = i;
                match self.object(d, keys, &mut j, self.head) {
                    P::More => P::More,
                    P::Bad => P::Bad,
                    P::Ok((x, y, _)) => {
                        if y > x {
                            (self.anchor, self.head) = (x, y - 1);
                            self.show_visual(d);
                        }
                        P::Ok(false)
                    }
                }
            }
            'd' | 'x' | 'X' | 'D' | 'c' | 's' | 'S' | 'C' | 'y' | 'Y' | '>' | '<' => {
                let op = match c {
                    'd' | 'x' | 'X' | 'D' => 'd',
                    'c' | 's' | 'S' | 'C' => 'c',
                    'y' | 'Y' => 'y',
                    _ => c,
                };
                let whole = lines || matches!(c, 'X' | 'D' | 'S' | 'C' | 'Y');
                let (s, e) = if whole && !lines { (line_start(d, line_of(d, a)), d.line_end_incl(line_of(d, h))) } else { (start, end) };
                self.mode = Mode::Normal;
                self.apply(d, op, s, e, whole, out);
                if op == 'y' {
                    set_cursor(d, clamp_normal(d, s));
                }
                P::Ok(op != 'y')
            }
            '~' | 'u' | 'U' => {
                let t: String = d.slice(start, end).chars().map(|ch| match c {
                    'u' => ch.to_lowercase().next().unwrap_or(ch),
                    'U' => ch.to_uppercase().next().unwrap_or(ch),
                    _ => swap_case(ch),
                }).collect();
                d.edit(EditKind::Other, vec![Edit { start, end, text: t, anchor: 0, head: 0 }]);
                self.mode = Mode::Normal;
                set_cursor(d, clamp_normal(d, start));
                P::Ok(true)
            }
            'J' => {
                d.sels = vec![Sel::range(start, end.saturating_sub(1).max(start))];
                d.exec(Cmd::JoinLines, false);
                self.mode = Mode::Normal;
                let hd = head(d);
                set_cursor(d, clamp_normal(d, hd));
                P::Ok(true)
            }
            'p' | 'P' => {
                let (text, lw) = self.register.clone();
                let text = if lw && !lines { text.trim_end_matches('\n').to_string() } else { text };
                d.edit(EditKind::Other, vec![Edit { start, end, text: text.clone(), anchor: 0, head: 0 }]);
                self.mode = Mode::Normal;
                set_cursor(d, clamp_normal(d, start + text.chars().count().saturating_sub(1)));
                P::Ok(true)
            }
            _ => P::Bad,
        }
    }

    /// Operator `op` (d c y > <) on chars `a..b` (`lines`: whole lines, `b` after their newline).
    fn apply(&mut self, d: &mut Document, op: char, a: usize, b: usize, lines: bool, out: &mut Vec<Out>) {
        let n = d.len();
        let (a, b) = (a.min(n), b.min(n));
        if matches!(op, '>' | '<') {
            d.sels = vec![Sel::range(a, b.saturating_sub(1).max(a))];
            d.exec(if op == '>' { Cmd::Indent } else { Cmd::Unindent }, false);
            let l = line_of(d, a);
            set_cursor(d, first_non_blank(d, l));
            return;
        }
        let mut text = d.slice(a, b);
        if lines && !text.ends_with('\n') {
            text.push('\n');
        }
        self.register = (text.clone(), lines);
        out.push(Out::Clipboard(text, lines));
        match op {
            'y' => set_cursor(d, a),
            'd' if lines => {
                // the last lines: with the newline before them
                let (a, b) = if b == n && a > 0 && b > a && !d.slice(a, b).ends_with('\n') { (a - 1, b) } else { (a, b) };
                d.edit(EditKind::Delete, vec![Edit { start: a, end: b, text: String::new(), anchor: 0, head: 0 }]);
                let l = line_of(d, a.min(d.len()));
                set_cursor(d, first_non_blank(d, l.min(last_line(d))));
            }
            'd' => {
                d.edit(EditKind::Delete, vec![Edit { start: a, end: b, text: String::new(), anchor: 0, head: 0 }]);
                set_cursor(d, a);
            }
            'c' if lines => {
                // the lines emptied, the indentation of the first one kept
                let l = line_of(d, a);
                let indent = d.slice(line_start(d, l), first_non_blank(d, l));
                let end = if d.slice(a, b).ends_with('\n') { b - 1 } else { b };
                d.edit(EditKind::Delete, vec![Edit { start: a, end, text: indent.clone(), anchor: 0, head: 0 }]);
                self.enter_insert(d, a + indent.chars().count());
            }
            _ => {
                d.edit(EditKind::Delete, vec![Edit { start: a, end: b, text: String::new(), anchor: 0, head: 0 }]);
                self.enter_insert(d, a);
            }
        }
    }

    /// p / P: the register after / before the cursor (whole lines: under / above its line).
    fn put(&mut self, d: &mut Document, after: bool, n: usize) {
        let (text, lines) = self.register.clone();
        if text.is_empty() {
            return;
        }
        let text: String = text.repeat(n);
        let h = head(d);
        if lines {
            let l = line_of(d, h);
            let (at, text) = if after {
                let e = d.line_end_incl(l);
                // the last line without a newline: one before the text
                if e == d.len() && !d.slice(line_start(d, l), e).ends_with('\n') {
                    (e, format!("\n{}", text.trim_end_matches('\n')))
                } else {
                    (e, text)
                }
            } else {
                (line_start(d, l), text)
            };
            d.edit(EditKind::Other, vec![Edit { start: at, end: at, text: text.clone(), anchor: 0, head: 0 }]);
            let first = if text.starts_with('\n') { at + 1 } else { at };
            let l = line_of(d, first);
            set_cursor(d, first_non_blank(d, l));
        } else {
            let e = line_end(d, line_of(d, h));
            let at = if after && h < e { h + 1 } else { h };
            d.edit(EditKind::Other, vec![Edit { start: at, end: at, text: text.clone(), anchor: 0, head: 0 }]);
            set_cursor(d, at + text.chars().count() - 1);
        }
    }
}

fn swap_case(c: char) -> char {
    if c.is_uppercase() {
        c.to_lowercase().next().unwrap_or(c)
    } else {
        c.to_uppercase().next().unwrap_or(c)
    }
}

/// `:s/a/b/g` (current line) or `:%s/a/b/gi` (the whole text) on `d`: how many were replaced.
/// None when `cmd` is not a substitution (or its pattern is wrong).
pub fn substitute(d: &mut Document, cmd: &str) -> Option<Result<usize, String>> {
    let (whole, rest) = match cmd.strip_prefix('%') {
        Some(r) => (true, r),
        None => (false, cmd),
    };
    let rest = rest.strip_prefix('s')?;
    let sep = rest.chars().next().filter(|c| !c.is_alphanumeric() && *c != ' ')?;
    let parts: Vec<&str> = rest[sep.len_utf8()..].splitn(3, sep).collect();
    let (pat, rep) = (parts.first().copied().unwrap_or(""), parts.get(1).copied().unwrap_or(""));
    let flags = parts.get(2).copied().unwrap_or("");
    let re = match regex::RegexBuilder::new(pat).case_insensitive(flags.contains('i')).build() {
        Ok(r) => r,
        Err(e) => return Some(Err(e.to_string())),
    };
    // Vim's & and \1 in the replacement, as the regex crate writes them
    let rep = rep.replace("\\&", "\u{0}").replace('&', "${0}").replace('\u{0}', "&");
    let rep = (1..10).fold(rep, |r, k| r.replace(&format!("\\{k}"), &format!("${{{k}}}")));
    let all = flags.contains('g');
    let l = line_of(d, head(d));
    let (a, b) = if whole { (0, d.len()) } else { (line_start(d, l), line_end(d, l)) };
    let text = d.slice(a, b);
    let mut count = 0;
    let new: String = text
        .split_inclusive('\n')
        .map(|line| {
            let (body, nl) = line.strip_suffix('\n').map_or((line, ""), |b| (b, "\n"));
            let hits = if all { re.find_iter(body).count() } else { usize::from(re.is_match(body)) };
            count += hits;
            let replaced = if all { re.replace_all(body, rep.as_str()) } else { re.replace(body, rep.as_str()) };
            format!("{replaced}{nl}")
        })
        .collect();
    if count > 0 {
        d.edit(EditKind::Other, vec![Edit { start: a, end: b, text: new, anchor: 0, head: 0 }]);
        set_cursor(d, clamp_normal(d, line_start(d, l.min(last_line(d)))));
    }
    Some(Ok(count))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str, at: usize) -> Document {
        let mut d = Document::from_text(1, text, 4, true);
        d.sels = vec![Sel::cursor(at)];
        d
    }

    fn keys(v: &mut Vim, d: &mut Document, k: &str) -> Vec<Out> {
        let mut out = Vec::new();
        for c in k.chars() {
            out.extend(v.key(d, c));
        }
        out
    }

    /// Typed in insert mode: as the editor would, recorded for `.`.
    fn typed(v: &mut Vim, d: &mut Document, t: &str) {
        d.insert_text(t, false);
        v.typed(t);
    }

    #[test]
    fn motions_and_counts() {
        let mut d = doc("let foo = bar(1, 2);\nsecond line\n", 0);
        let mut v = Vim::new();
        keys(&mut v, &mut d, "w");
        assert_eq!(head(&d), 4);
        keys(&mut v, &mut d, "2w");
        assert_eq!(head(&d), 10);
        keys(&mut v, &mut d, "e");
        assert_eq!(head(&d), 12);
        keys(&mut v, &mut d, "$");
        assert_eq!(head(&d), 19, "on the last char, not after it");
        keys(&mut v, &mut d, "j");
        assert_eq!(head(&d), 31, "$ is kept on the next line");
        keys(&mut v, &mut d, "0");
        assert_eq!(head(&d), 21);
        keys(&mut v, &mut d, "gg");
        assert_eq!(head(&d), 0);
        keys(&mut v, &mut d, "f(");
        assert_eq!(head(&d), 13);
        keys(&mut v, &mut d, ";");
        assert_eq!(head(&d), 13, "no other ( on the line");
        keys(&mut v, &mut d, "%");
        assert_eq!(head(&d), 18);
        keys(&mut v, &mut d, "G");
        assert_eq!(head(&d), 21);
        keys(&mut v, &mut d, "b");
        assert_eq!(head(&d), 18);
        keys(&mut v, &mut d, "b");
        assert_eq!(head(&d), 17);
    }

    #[test]
    fn operators_with_motions_and_objects() {
        let mut v = Vim::new();
        let mut d = doc("one two three\n", 4);
        keys(&mut v, &mut d, "dw");
        assert_eq!(d.rope.to_string(), "one three\n");
        keys(&mut v, &mut d, "u");
        assert_eq!(d.rope.to_string(), "one two three\n");
        let mut d = doc("    x = f(a, b)\n", 11);
        keys(&mut v, &mut d, "ci(");
        assert_eq!(v.mode, Mode::Insert);
        typed(&mut v, &mut d, "c");
        keys(&mut v, &mut d, &ESC.to_string());
        assert_eq!(d.rope.to_string(), "    x = f(c)\n");
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(head(&d), 10);
        let mut d = doc("say \"hello world\" now\n", 7);
        keys(&mut v, &mut d, "di\"");
        assert_eq!(d.rope.to_string(), "say \"\" now\n");
        // cw on a word changes it up to its end (the space stays)
        let mut d = doc("a b\n", 0);
        keys(&mut v, &mut d, "cw");
        typed(&mut v, &mut d, "xy");
        keys(&mut v, &mut d, &ESC.to_string());
        assert_eq!(d.rope.to_string(), "xy b\n");
    }

    #[test]
    fn lines_registers_and_put() {
        let mut v = Vim::new();
        let mut d = doc("a\nb\nc\n", 0);
        let out = keys(&mut v, &mut d, "dd");
        assert_eq!(d.rope.to_string(), "b\nc\n");
        assert_eq!(out, vec![Out::Clipboard("a\n".into(), true)]);
        keys(&mut v, &mut d, "p");
        assert_eq!(d.rope.to_string(), "b\na\nc\n");
        assert_eq!(head(&d), 2);
        keys(&mut v, &mut d, "yyP");
        assert_eq!(d.rope.to_string(), "b\na\na\nc\n");
        keys(&mut v, &mut d, "Gdd");
        assert_eq!(d.rope.to_string(), "b\na\na\n");
        // dd took the cursor to the last line: k first, then two lines
        keys(&mut v, &mut d, "k2dd");
        assert_eq!(d.rope.to_string(), "b\n");
        let mut d = doc("abc\n", 0);
        keys(&mut v, &mut d, "xp");
        assert_eq!(d.rope.to_string(), "bac\n");
        keys(&mut v, &mut d, "rZ");
        assert_eq!(d.rope.to_string(), "bZc\n");
        keys(&mut v, &mut d, "~");
        assert_eq!(d.rope.to_string(), "bzc\n");
    }

    #[test]
    fn insert_commands_and_dot() {
        let mut v = Vim::new();
        let mut d = doc("x\ny\n", 0);
        keys(&mut v, &mut d, "A");
        typed(&mut v, &mut d, ";");
        keys(&mut v, &mut d, &ESC.to_string());
        assert_eq!(d.rope.to_string(), "x;\ny\n");
        keys(&mut v, &mut d, "j.");
        assert_eq!(d.rope.to_string(), "x;\ny;\n", ". repeats A; on the next line");
        let mut d = doc("a1 a2 a3\n", 0);
        keys(&mut v, &mut d, "dw..");
        assert_eq!(d.rope.to_string(), "\n");
        let mut d = doc("    if x:\n", 4);
        keys(&mut v, &mut d, "o");
        assert_eq!(v.mode, Mode::Insert);
        assert_eq!(d.rope.to_string(), "    if x:\n        \n", "the new line indented");
    }

    #[test]
    fn visual_mode() {
        let mut v = Vim::new();
        let mut d = doc("hello world\nnext\n", 0);
        keys(&mut v, &mut d, "vee");
        assert_eq!(v.mode, Mode::Visual);
        assert_eq!(d.slice(d.primary_sel().start(), d.primary_sel().end()), "hello world");
        keys(&mut v, &mut d, "d");
        assert_eq!(d.rope.to_string(), "\nnext\n");
        assert_eq!(v.mode, Mode::Normal);
        let mut d = doc("a\nb\nc\n", 0);
        keys(&mut v, &mut d, "Vjy");
        assert_eq!(v.register, ("a\nb\n".to_string(), true));
        keys(&mut v, &mut d, "Gp");
        assert_eq!(d.rope.to_string(), "a\nb\nc\na\nb\n");
        let mut d = doc("f(abc)\n", 3);
        keys(&mut v, &mut d, "vi(c");
        assert_eq!(v.mode, Mode::Insert);
        assert_eq!(d.rope.to_string(), "f()\n");
        // a selection made with the mouse: visual mode on it
        let mut v = Vim::new();
        let mut d = doc("abcdef\n", 0);
        d.sels = vec![Sel::range(1, 4)];
        v.sync(&mut d);
        assert_eq!(v.mode, Mode::Visual);
        keys(&mut v, &mut d, "d");
        assert_eq!(d.rope.to_string(), "aef\n");
    }

    #[test]
    fn app_commands_and_substitute() {
        let mut v = Vim::new();
        let mut d = doc("foo bar foo\nfoo\n", 0);
        assert_eq!(keys(&mut v, &mut d, "*"), vec![Out::Word("foo".into(), true)]);
        assert_eq!(keys(&mut v, &mut d, ":"), vec![Out::CommandLine]);
        assert_eq!(keys(&mut v, &mut d, "ZZ"), vec![Out::SaveClose]);
        assert_eq!(substitute(&mut d, "s/foo/baz/").unwrap().unwrap(), 1);
        assert_eq!(d.rope.to_string(), "baz bar foo\nfoo\n");
        assert_eq!(substitute(&mut d, "%s/fo(o)/<&\\1>/g").unwrap().unwrap(), 2);
        assert_eq!(d.rope.to_string(), "baz bar <fooo>\n<fooo>\n");
        assert!(substitute(&mut d, "w").is_none());
        assert!(substitute(&mut d, "s/(/x/").unwrap().is_err());
    }
}
