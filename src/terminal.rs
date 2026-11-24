//! Integrated terminal, like VS Code's: a real shell in a pseudo-terminal (portable-pty: Unix PTY
//! or Windows ConPTY), its output interpreted by a VT100/xterm emulator (vt100) and drawn as a grid.
//! Several terminals can be open, each in its own tab.

use crate::highlight::Colors;
use eframe::egui::{self, Align2, Color32, CursorIcon, Event, FontId, Id, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Lines of history kept by a terminal: each line is a row of cells, so this is most of its memory.
const SCROLLBACK: usize = 3_000;
/// Most bytes of output kept as text by a session that keeps it.
const CAPTURE: usize = 1 << 20;

/// Base of the focus ids of the terminal grids (shortcuts other than a few global ones go to the shell).
fn term_id() -> Id {
    Id::new("florm_terminal")
}

fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
    }
}

/// Answers to the queries a shell sends to its terminal (fish waits 10 s for the "primary device
/// attributes" one, then complains): device attributes, status, cursor position.
fn replies(out: &[u8], cursor: impl Fn() -> (u16, u16)) -> Vec<u8> {
    let mut r = Vec::new();
    let mut i = 0;
    while let Some(k) = out[i..].windows(2).position(|w| w == b"\x1b[") {
        let rest = &out[i + k + 2..];
        let reply: Option<Vec<u8>> = if rest.starts_with(b"c") || rest.starts_with(b"0c") {
            // VT220 with ANSI colors, like xterm
            Some(b"\x1b[?62;22c".to_vec())
        } else if rest.starts_with(b">c") || rest.starts_with(b">0c") {
            Some(b"\x1b[>1;10;0c".to_vec())
        } else if rest.starts_with(b"5n") {
            Some(b"\x1b[0n".to_vec())
        } else if rest.starts_with(b"6n") {
            let (row, col) = cursor();
            Some(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes())
        } else {
            None
        };
        if let Some(b) = reply {
            r.extend(b);
        }
        i += k + 2;
    }
    r
}

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// A link a program wrapped around its text (OSC 8): its address, where it starts and ends (rows
/// counted from the first line ever printed, end excluded), and the text it covered then.
#[derive(Clone, Debug)]
struct Osc8 {
    uri: String,
    start: (u64, u16),
    end: (u64, u16),
    text: String,
}

/// Most OSC 8 links kept (a program redrawing its screen sends the same ones again and again).
const OSC8_MAX: usize = 400;

/// What a program tells its terminal besides the screen: its title, its folder, what it copies,
/// its links, its bell; and what it asks of it (the colors of the theme).
struct Told {
    ctx: egui::Context,
    title: String,
    /// Its folder (OSC 7, sent by fish and by shells set up for it): the relative paths of its
    /// links are from there.
    cwd: Option<PathBuf>,
    /// To answer its questions (the colors of the theme).
    writer: Option<SharedWriter>,
    /// Text and background of the theme of Florm, given to the programs that ask (OSC 10 / 11):
    /// Claude Code, vim… choose their light or dark colors from them.
    colors: (Color32, Color32),
    /// It rang (BEL) since its terminal was last looked at.
    bell: bool,
    /// Rows dropped off the top of the history: the first line ever printed is row 0.
    dropped: u64,
    /// The OSC 8 link being written (its address and start), and those written.
    osc8_open: Option<(String, (u64, u16))>,
    osc8: Vec<Osc8>,
}

impl Told {
    fn new(ctx: &egui::Context, writer: Option<SharedWriter>) -> Self {
        Told { ctx: ctx.clone(), title: String::new(), cwd: None, writer, colors: (Color32::WHITE, Color32::BLACK), bell: false, dropped: 0, osc8_open: None, osc8: Vec::new() }
    }

    /// Row `r` of the live screen, counted from the first line ever printed.
    fn abs_row(&self, sc: &vt100::Screen, r: u16) -> u64 {
        self.dropped + sc.scrollback_len() as u64 + r as u64
    }

    fn answer(&self, bytes: &[u8]) {
        if let Some(w) = &self.writer {
            let mut w = w.lock().unwrap();
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// The OSC 8 link at cell `cell` of the screen shown (scrolled back `offset` rows), if its
    /// text is still there (a program redrawing its screen leaves old links behind).
    fn osc8_at(&self, sc: &vt100::Screen, cell: (u16, u16)) -> Option<(String, Vec<(u16, u16)>)> {
        let top = (self.dropped + sc.scrollback_len() as u64).checked_sub(sc.scrollback() as u64)?;
        let pos = (top + cell.0 as u64, cell.1);
        let cols = sc.size().1.get();
        let rows = sc.size().0.get() as u64;
        let l = self.osc8.iter().rev().find(|l| l.start <= pos && pos < l.end)?;
        // its cells on the screen shown, and the text they hold now
        let (r1, r2) = (l.start.0.checked_sub(top)?, l.end.0.checked_sub(top)?);
        if r2 >= rows && !(r2 == rows && l.end.1 == 0) {
            return None;
        }
        let (r1, r2) = (r1 as u16, r2 as u16);
        let text = sc.contents_between(r1, l.start.1, r2, l.end.1);
        if !l.text.is_empty() && text != l.text {
            return None;
        }
        let mut cells = Vec::new();
        let (mut r, mut c) = (r1, l.start.1);
        while (r, c) < (r2, l.end.1) {
            cells.push((r, c));
            c += 1;
            if c >= cols {
                (r, c) = (r + 1, 0);
            }
        }
        Some((l.uri.clone(), cells))
    }
}

impl vt100::Callbacks for Told {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.bell = true;
        // Florm behind another window: its icon asks for attention
        if !self.ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
        self.ctx.request_repaint();
    }

    fn visual_bell(&mut self, sc: &mut vt100::Screen) {
        self.audible_bell(sc);
    }

    fn on_scroll(&mut self, _: vt100::capture::RowContents<'_>, alternate: bool) {
        if !alternate {
            self.dropped += 1;
        }
    }
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title).into_owned();
    }

    /// OSC 52: a program copies (Claude Code's /copy, vim, tmux…), even far away (ssh).
    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, _ty: &[u8], data: &[u8]) {
        if let Some(text) = crate::claude::base64_decode(&String::from_utf8_lossy(data)).and_then(|b| String::from_utf8(b).ok()) {
            self.ctx.copy_text(text);
            self.ctx.request_repaint();
        }
    }

    fn unhandled_osc(&mut self, sc: &mut vt100::Screen, params: &[&[u8]]) {
        match params {
            // OSC 7 ; file://host/path
            [b"7", url, ..] => {
                let url = String::from_utf8_lossy(url);
                if let Some(path) = url.strip_prefix("file://").and_then(|r| r.find('/').map(|i| &r[i..])) {
                    self.cwd = Some(PathBuf::from(percent_decode(path)));
                }
            }
            // OSC 10 / 11 ; ? : the text and background colors of the theme
            [n @ (b"10" | b"11"), b"?"] => {
                let c = if *n == b"10" { self.colors.0 } else { self.colors.1 };
                let n = String::from_utf8_lossy(n);
                self.answer(format!("\x1b]{n};rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}\x1b\\", c.r(), c.g(), c.b()).as_bytes());
            }
            // OSC 8 ; params ; uri : a link starts (an empty uri ends it)
            [b"8", _, uri, ..] => {
                let (r, c) = sc.cursor_position();
                let at = (self.abs_row(sc, r), c);
                if let Some((uri, start)) = self.osc8_open.take() {
                    if start < at {
                        // the text it covers, read now if it is all on the live screen
                        let live = (self.dropped + sc.scrollback_len() as u64, sc.scrollback());
                        let text = match (start.0.checked_sub(live.0), live.1) {
                            (Some(r1), 0) => sc.contents_between(r1 as u16, start.1, r, c),
                            _ => String::new(),
                        };
                        self.osc8.push(Osc8 { uri, start, end: at, text });
                        if self.osc8.len() > OSC8_MAX {
                            self.osc8.drain(..OSC8_MAX / 4);
                        }
                    }
                }
                let uri = String::from_utf8_lossy(uri).into_owned();
                if !uri.is_empty() {
                    self.osc8_open = Some((uri, at));
                }
            }
            _ => {}
        }
    }
}

/// `path` as a shell reads it back: as is when plain, else between quotes.
fn shell_quote(path: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-+~:@%,=".contains(c) || (cfg!(windows) && c == '\\');
    if !path.is_empty() && path.chars().all(plain) {
        path.to_string()
    } else if cfg!(windows) {
        format!("\"{path}\"")
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

/// `%20` and the like decoded (the paths of OSC 7).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Rows and columns as the emulator takes them (at least one of each).
fn nz(n: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(n.max(1)).unwrap_or(std::num::NonZeroU16::MIN)
}

type Parser = vt100::Parser<Told>;

/// Text to type in a shell once it started (`None`: it printed something, type at once): typed
/// earlier, fish would take it for the answer it waits from the terminal.
type Typeahead = Arc<Mutex<Option<Vec<u8>>>>;

struct Session {
    /// Stable id (editor tabs refer to it).
    id: u64,
    /// Shown in an editor tab rather than in the bottom panel.
    docked: bool,
    /// Shown by another view (the Docker one), neither in the bottom panel nor in a tab of its own.
    embedded: bool,
    /// Program run instead of a shell (started again by Enter once it ended).
    cmd: Option<Vec<String>>,
    /// Its program cannot be started again (the program of a debugging session).
    once: bool,
    /// Output kept as text (the end of it), for those who read it (the summary of tests).
    capture: Option<Arc<Mutex<Vec<u8>>>>,
    /// Folders of a container and where they are here: (container folder, folder here), for the
    /// files of its tracebacks.
    mounts: Vec<(String, PathBuf)>,
    name: String,
    parser: Arc<Mutex<Parser>>,
    writer: SharedWriter,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    exited: Arc<AtomicBool>,
    /// Pass during which its grid was last drawn: the output of a terminal not shown repaints
    /// the window less often.
    drawn: Arc<AtomicU64>,
    typeahead: Typeahead,
    cwd: PathBuf,
    size: (u16, u16),
    /// Selection in visible cells: (row, col) anchor and end.
    sel: Option<((u16, u16), (u16, u16))>,
    /// The last word looked at as a link (the word, its first and last cells) and what it opens:
    /// the disk is asked once per word, not at each frame.
    link_cache: Option<(LinkKey, Option<Link>)>,
    /// The search in its history, while its bar is open.
    search: Option<Search>,
    /// Last key typed: the cursor does not blink while typing.
    typed_at: std::time::Instant,
    /// Last text pasted with Ctrl+V: a Ctrl+V without one had an image (or nothing) to paste.
    pasted_at: Option<std::time::Instant>,
    /// V seen pressed (typed): its release is not a Ctrl+V.
    v_down: bool,
}

/// The search in the history of a terminal (Ctrl+Shift+F, ⌘F on macOS).
#[derive(Default)]
struct Search {
    query: String,
    /// The query `hits` are of.
    of: String,
    /// Its matches, oldest first: row (counted from the first line ever printed), first char, end.
    hits: Vec<(u64, u16, u16)>,
    /// The match shown (index in `hits`).
    current: Option<usize>,
    /// The field takes the keyboard at the next frame.
    focus: bool,
}

/// Every line of the history then of the screen, oldest first, and the row of the first one
/// (counted from the first line ever printed). The view is left where it was.
fn all_lines(p: &mut Parser) -> (Vec<String>, u64) {
    let first = p.callbacks().dropped;
    let sc = p.screen_mut();
    let cols = sc.size().1.get();
    let (keep, total) = (sc.scrollback(), sc.scrollback_len());
    let mut lines: Vec<String> = Vec::with_capacity(total + 50);
    loop {
        let off = total.saturating_sub(lines.len());
        sc.set_scrollback(off);
        let top = total - off;
        let before = lines.len();
        for (k, row) in sc.rows(0, cols).enumerate() {
            if top + k == lines.len() {
                lines.push(row);
            }
        }
        if off == 0 || lines.len() == before {
            break;
        }
    }
    sc.set_scrollback(keep);
    (lines, first)
}

/// Where `query` is in `lines` (the first one at row `first`): case ignored unless it has a capital.
fn find_hits(lines: &[String], first: u64, query: &str) -> Vec<(u64, u16, u16)> {
    if query.is_empty() {
        return Vec::new();
    }
    let exact = query.chars().any(char::is_uppercase);
    let fold = |t: &str| if exact { t.to_string() } else { t.to_lowercase() };
    let q: Vec<char> = fold(query).chars().collect();
    let mut hits = Vec::new();
    for (k, line) in lines.iter().enumerate() {
        let l: Vec<char> = fold(line).chars().collect();
        let mut i = 0;
        while i + q.len() <= l.len() {
            if l[i..i + q.len()] == q[..] {
                hits.push((first + k as u64, i as u16, (i + q.len()) as u16));
                i += q.len();
            } else {
                i += 1;
            }
        }
    }
    hits
}

impl Session {
    fn spawn(shell: &str, cwd: PathBuf, ctx: &egui::Context) -> Result<Session, String> {
        let argv: Vec<&str> = shell.split_whitespace().collect();
        Self::spawn_argv(&argv, &[], cwd, ctx, false)
    }

    /// Runs `argv` in a new pseudo-terminal; `prefill` is shown before its output, which is also
    /// kept as text when `capture`.
    fn spawn_argv(argv: &[&str], prefill: &[u8], cwd: PathBuf, ctx: &egui::Context, capture: bool) -> Result<Session, String> {
        Self::spawn_argv_env(argv, prefill, cwd, ctx, capture, &[])
    }

    /// `spawn_argv` with these variables in the environment of the program (an empty value
    /// takes the variable out of it).
    fn spawn_argv_env(argv: &[&str], prefill: &[u8], cwd: PathBuf, ctx: &egui::Context, capture: bool, env: &[(String, String)]) -> Result<Session, String> {
        let shell = argv.join(" ");
        let size = (24, 80);
        let pair = native_pty_system()
            .openpty(PtySize { rows: size.0, cols: size.1, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(argv.first().copied().unwrap_or("sh"));
        cmd.args(argv.iter().skip(1));
        cmd.cwd(&cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "florm");
        // the links of the programs (OSC 8) are understood: Claude Code and the tools in Node send
        // them when this is set
        cmd.env("FORCE_HYPERLINK", "1");
        for (k, v) in env {
            if v.is_empty() {
                cmd.env_remove(k);
            } else {
                cmd.env(k, v);
            }
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("{shell} : {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer: SharedWriter = Arc::new(Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?));
        let told = Told::new(ctx, Some(writer.clone()));
        let parser = Arc::new(Mutex::new(Parser::new_with_callbacks(nz(size.0), nz(size.1), SCROLLBACK, told)));
        parser.lock().unwrap().process(prefill);
        let exited = Arc::new(AtomicBool::new(false));
        let drawn = Arc::new(AtomicU64::new(ctx.cumulative_pass_nr()));
        let typeahead: Typeahead = Arc::new(Mutex::new(Some(Vec::new())));
        let capture = capture.then(|| Arc::new(Mutex::new(Vec::new())));
        let (p, ex, ctx, w, cap, ta, dr) = (parser.clone(), exited.clone(), ctx.clone(), writer.clone(), capture.clone(), typeahead.clone(), drawn.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if let Some(c) = &cap {
                            let mut c = c.lock().unwrap();
                            c.extend_from_slice(&buf[..n]);
                            if c.len() > CAPTURE {
                                let cut = c.len() - CAPTURE / 2;
                                c.drain(..cut);
                            }
                        }
                        p.lock().unwrap().process(&buf[..n]);
                        let answer = replies(&buf[..n], || p.lock().unwrap().screen().cursor_position());
                        if !answer.is_empty() {
                            let mut w = w.lock().unwrap();
                            let _ = w.write_all(&answer);
                            let _ = w.flush();
                        }
                        if let Some(text) = ta.lock().unwrap().take().filter(|t| !t.is_empty()) {
                            let mut w = w.lock().unwrap();
                            let _ = w.write_all(&text);
                            let _ = w.flush();
                        }
                        // drawn during the last pass (the window may be idle since): at once;
                        // not shown (a Claude tab in the background): twice a second is enough
                        if dr.load(Ordering::Relaxed) + 1 >= ctx.cumulative_pass_nr() {
                            ctx.request_repaint();
                        } else {
                            ctx.request_repaint_after(std::time::Duration::from_millis(500));
                        }
                    }
                }
            }
            ex.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        });
        let name = std::path::Path::new(argv.first().copied().unwrap_or(&shell))
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| shell.to_string());
        Ok(Session { id: 0, docked: false, embedded: false, cmd: None, once: false, capture, mounts: Vec::new(), name, parser, writer, master: pair.master, child, exited, drawn, typeahead, cwd, size, sel: None, link_cache: None, search: None, typed_at: std::time::Instant::now(), pasted_at: None, v_down: false })
    }

    fn write(&mut self, bytes: &[u8]) {
        {
            let mut w = self.writer.lock().unwrap();
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
        // typing brings the view back to the prompt
        self.parser.lock().unwrap().screen_mut().set_scrollback(0);
        self.sel = None;
        self.typed_at = std::time::Instant::now();
    }

    /// Scrolls the history by `n` lines (up: positive), as far as it goes.
    fn scroll_by(&mut self, n: isize) {
        let mut p = self.parser.lock().unwrap();
        let cur = p.screen().scrollback() as isize;
        p.screen_mut().set_scrollback((cur + n).max(0) as usize);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        if self.size != (rows, cols) {
            self.size = (rows, cols);
            self.parser.lock().unwrap().set_size(nz(rows), nz(cols));
            let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }

    fn title(&self) -> String {
        let (t, bell) = {
            let p = self.parser.lock().unwrap();
            (p.callbacks().title.clone(), p.callbacks().bell)
        };
        let t = if t.trim().is_empty() { self.name.clone() } else { t };
        // it rang while not looked at
        if bell {
            format!("🔔 {t}")
        } else {
            t
        }
    }

    /// Shown in the bottom panel.
    fn in_panel(&self) -> bool {
        !self.docked && !self.embedded
    }

    /// The link at cell `cell` (visible): a URL, or a file printed by the program that is there.
    fn link_at(&mut self, cell: (u16, u16)) -> Option<Link> {
        let osc8 = {
            let p = self.parser.lock().unwrap();
            p.callbacks().osc8_at(p.screen(), cell)
        };
        if let Some((uri, cells)) = osc8 {
            return osc8_target(&uri).map(|target| Link { target, cells });
        }
        let (text, cells, cwd) = {
            let p = self.parser.lock().unwrap();
            let (text, cells) = line_at(p.screen(), cell.0);
            (text, cells, p.callbacks().cwd.clone().unwrap_or_else(|| self.cwd.clone()))
        };
        let at = cells.iter().position(|&c| c == cell)?;
        let (a, b) = word_at(&text, at)?;
        let word: String = text[a..b].iter().collect();
        let key = (word.clone(), cells[a], cells[b - 1]);
        if let Some((k, link)) = &self.link_cache {
            if *k == key {
                return link.clone();
            }
        }
        let target = match candidate(&word) {
            // a file:// address printed by a program opens the file in Florm, never in the system
            Some(Cand::Url(u)) => osc8_target(&u),
            Some(Cand::Path(p, line)) => resolve(&p, &cwd, &self.mounts).map(|f| Target::File(f, line)),
            None => None,
        };
        let mut covered: Vec<(u16, u16)> = cells[a..b].to_vec();
        covered.dedup();
        let link = target.map(|target| Link { target, cells: covered });
        self.link_cache = Some((key, link.clone()));
        link
    }

    /// The cells of the word (double click) or of the whole line (triple click) at `cell`.
    fn select_around(&mut self, cell: (u16, u16), line: bool) {
        let (text, cells) = line_at(self.parser.lock().unwrap().screen(), cell.0);
        let Some(at) = cells.iter().position(|&c| c == cell) else { return };
        let range = if line {
            let end = text.iter().rposition(|c| !c.is_whitespace()).map(|e| e + 1);
            end.map(|e| (0, e))
        } else {
            word_at(&text, at)
        };
        if let Some((a, b)) = range {
            self.sel = Some((cells[a], cells[b - 1]));
        }
    }

    /// The matches of the search read again (the history grew), the one shown kept if it is there.
    fn search_refresh(&mut self) {
        let Some(q) = self.search.as_ref().map(|q| q.query.clone()) else { return };
        let (lines, first) = all_lines(&mut self.parser.lock().unwrap());
        let hits = find_hits(&lines, first, &q);
        let Some(search) = self.search.as_mut() else { return };
        let kept = search.current.and_then(|i| search.hits.get(i).copied()).filter(|_| search.of == q);
        search.current = kept.and_then(|h| hits.iter().position(|x| *x == h)).or_else(|| hits.len().checked_sub(1));
        search.hits = hits;
        search.of = q;
    }

    /// The next match (`older`: up, toward the start of the history), scrolled into view.
    fn search_step(&mut self, older: bool) {
        self.search_refresh();
        let Some(search) = self.search.as_mut() else { return };
        let n = search.hits.len();
        if n == 0 {
            return;
        }
        let i = match search.current {
            Some(i) if older => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
            None => n - 1,
        };
        search.current = Some(i);
        self.search_show();
    }

    /// The match shown scrolled into view.
    fn search_show(&mut self) {
        let Some(row) = self.search.as_ref().and_then(|q| q.hits.get(q.current?)).map(|h| h.0) else { return };
        let mut p = self.parser.lock().unwrap();
        let dropped = p.callbacks().dropped;
        let sc = p.screen_mut();
        let (rows, total) = (sc.size().0.get() as usize, sc.scrollback_len());
        let local = row.saturating_sub(dropped) as usize;
        // on the live screen: no scrolling; in the history: the match in the middle
        let off = if local >= total { 0 } else { (total - local + rows / 2).min(total) };
        sc.set_scrollback(off);
    }

    /// The matches on the screen shown: (row, first col, end, the one shown).
    fn search_marks(&self) -> Vec<(u16, u16, u16, bool)> {
        let Some(search) = &self.search else { return Vec::new() };
        let p = self.parser.lock().unwrap();
        let sc = p.screen();
        let rows = sc.size().0.get() as u64;
        let top = p.callbacks().dropped + sc.scrollback_len() as u64 - sc.scrollback() as u64;
        search
            .hits
            .iter()
            .enumerate()
            .filter(|(_, h)| h.0 >= top && h.0 < top + rows)
            .map(|(i, h)| ((h.0 - top) as u16, h.1, h.2, search.current == Some(i)))
            .collect()
    }

    fn selected_text(&self) -> Option<String> {
        let ((r1, c1), (r2, c2)) = self.sel?;
        let ((r1, c1), (r2, c2)) = if (r1, c1) <= (r2, c2) { ((r1, c1), (r2, c2)) } else { ((r2, c2), (r1, c1)) };
        let t = self.parser.lock().unwrap().screen().contents_between(r1, c1, r2, c2 + 1);
        (!t.is_empty()).then_some(t)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

pub struct Terminal {
    /// Folder for the next terminal (project root, or a folder chosen in the tree).
    pub cwd: Option<PathBuf>,
    /// Give the keyboard to the terminal of the bottom panel at the next frame.
    pub want_focus: bool,
    /// Terminal tab (session id) to give the keyboard to at the next frame.
    pub focus_tab: Option<u64>,
    /// Session of the bottom panel asked to move into an editor tab ("⇱").
    pub dock_request: Option<u64>,
    /// File (and line) of a traceback or a path Ctrl+clicked in a terminal, for the app to open.
    pub open_request: Option<(PathBuf, usize)>,
    /// Where each grid was drawn, and in which pass: files dropped there are typed into it.
    grids: std::collections::HashMap<u64, (u64, Rect)>,
    sessions: Vec<Session>,
    /// Shown session of the bottom panel (index in `sessions`, never a docked one).
    active: usize,
    next_id: u64,
    error: Option<String>,
}

/// What the grid of a session asked for.
enum GridAct {
    /// The shell ended and Enter was pressed: start a new one in the same place.
    Restart,
    NewTerminal,
    /// Ctrl+V with no text in the clipboard (an image, for Claude).
    PasteImage,
}

impl Terminal {
    pub fn new() -> Self {
        Terminal { cwd: None, want_focus: false, focus_tab: None, dock_request: None, open_request: None, grids: Default::default(), sessions: Vec::new(), active: 0, next_id: 1, error: None }
    }

    fn cwd(&self) -> PathBuf {
        self.cwd.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    /// Focus id of the grid of session `id`.
    fn grid_id(id: u64) -> Id {
        term_id().with(id)
    }

    /// Whether the keyboard is in one of the terminals (keys go to the shell).
    pub fn has_focus(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.focused()).is_some_and(|f| self.sessions.iter().any(|s| Self::grid_id(s.id) == f))
    }

    fn spawn(&mut self, ctx: &egui::Context, shell: Option<&str>, cwd: Option<PathBuf>) -> Result<Session, String> {
        let shell = shell.map(str::to_string).filter(|s| !s.trim().is_empty()).unwrap_or_else(default_shell);
        let cwd = cwd.unwrap_or_else(|| self.cwd());
        let mut s = Session::spawn(&shell, cwd, ctx).map_err(|e| tf!("Impossible de lancer le terminal : {e}", "Cannot start the terminal: {e}"))?;
        s.id = self.next_id;
        self.next_id += 1;
        Ok(s)
    }

    /// Opens a new terminal in the bottom panel, in `cwd` (or the current folder).
    pub fn open(&mut self, ctx: &egui::Context, shell: Option<&str>, cwd: Option<PathBuf>) {
        if let Err(e) = self.open_panel(ctx, shell, cwd) {
            self.error = Some(e);
        }
    }

    /// Opens a new terminal meant for an editor tab; returns its id and title.
    pub fn open_tab(&mut self, ctx: &egui::Context, shell: Option<&str>, cwd: Option<PathBuf>) -> Result<(u64, String), String> {
        let mut s = self.spawn(ctx, shell, cwd)?;
        s.docked = true;
        let r = (s.id, s.title());
        self.focus_tab = Some(s.id);
        self.sessions.push(s);
        self.fix_active();
        Ok(r)
    }

    /// Moves session `id` into an editor tab (`docked`) or back into the bottom panel.
    pub fn set_docked(&mut self, id: u64, docked: bool) {
        if let Some(i) = self.sessions.iter().position(|s| s.id == id) {
            self.sessions[i].docked = docked;
            if docked {
                self.focus_tab = Some(id);
            } else {
                self.active = i;
                self.want_focus = true;
            }
        }
        self.fix_active();
    }

    /// Ends session `id` (its tab was closed).
    pub fn close(&mut self, id: u64) {
        self.sessions.retain(|s| s.id != id);
        self.fix_active();
    }

    pub fn title_of(&self, id: u64) -> Option<String> {
        self.sessions.iter().find(|s| s.id == id).map(|s| s.title())
    }

    /// Keeps `active` on a session of the bottom panel.
    fn fix_active(&mut self) {
        if !self.sessions.get(self.active).is_some_and(Session::in_panel) {
            self.active = self.sessions.iter().rposition(Session::in_panel).unwrap_or(0);
        }
    }

    fn restart(&mut self, i: usize, ctx: &egui::Context, shell: Option<&str>) {
        let old = &self.sessions[i];
        let (cwd, id, docked, embedded) = (old.cwd.clone(), old.id, old.docked, old.embedded);
        let (cmd, capture, mounts) = (old.cmd.clone(), old.capture.is_some(), old.mounts.clone());
        let new = match &cmd {
            Some(argv) => Session::spawn_argv(&argv.iter().map(String::as_str).collect::<Vec<_>>(), &[], cwd, ctx, capture),
            None => self.spawn(ctx, shell, Some(cwd)),
        };
        match new {
            Ok(mut s) => {
                s.id = id;
                s.docked = docked;
                s.embedded = embedded;
                s.cmd = cmd;
                s.mounts = mounts;
                self.sessions[i] = s;
                if docked {
                    self.focus_tab = Some(id);
                } else {
                    self.want_focus = true;
                }
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// The terminals of the bottom panel, for its ▾ menu (no row of tabs): id, title, shown, ended.
    pub fn panel_sessions(&self) -> Vec<(u64, String, bool, bool)> {
        let shown = self.sessions.get(self.active).filter(|s| s.in_panel()).map(|s| s.id);
        self.sessions.iter().filter(|s| s.in_panel()).map(|s| (s.id, s.title(), Some(s.id) == shown, s.exited.load(Ordering::Relaxed))).collect()
    }

    /// Folder of the terminal shown in the bottom panel.
    pub fn shown_cwd(&self) -> Option<&Path> {
        self.sessions.get(self.active).filter(|s| s.in_panel()).map(|s| s.cwd.as_path())
    }

    /// The terminal shown in the bottom panel goes into an editor tab ("⇱").
    pub fn dock_shown(&mut self) {
        if let Some(s) = self.sessions.get(self.active).filter(|s| s.in_panel()) {
            self.dock_request = Some(s.id);
        }
    }

    /// The bottom panel: the terminal shown (the others are in its ▾ menu).
    pub fn ui(&mut self, ui: &mut egui::Ui, shell: Option<&str>, colors: Colors, light: bool) {
        let ctx = ui.ctx().clone();
        self.fix_active();
        if !self.sessions.iter().any(Session::in_panel) && self.error.is_none() {
            self.open(&ctx, shell, None);
        }
        if let Some(e) = &self.error {
            ui.label(RichText::new(e).color(crate::highlight::red()));
            if ui.button(t!("Réessayer", "Retry")).clicked() {
                self.error = None;
            }
            return;
        }
        let i = self.active;
        if !self.sessions.get(i).is_some_and(Session::in_panel) {
            ui.label(RichText::new(t!("Aucun terminal ouvert — « + » en haut pour en ouvrir un.", "No terminal open — “+” at the top to open one.")).weak());
            return;
        }
        let focus = std::mem::take(&mut self.want_focus);
        // a rounded block of the editor's color in the panel
        let act = egui::Frame::none().fill(colors.bg).rounding(6.0).show(ui, |ui| self.grid(ui, i, focus, colors, light)).inner;
        match act {
            Some(GridAct::Restart) => self.restart(i, &ctx, shell),
            Some(GridAct::NewTerminal) => self.open(&ctx, shell, None),
            Some(GridAct::PasteImage) | None => {}
        }
    }

    /// A terminal shown in an editor tab.
    pub fn tab_ui(&mut self, ui: &mut egui::Ui, id: u64, shell: Option<&str>, colors: Colors, light: bool) {
        let ctx = ui.ctx().clone();
        let Some(i) = self.sessions.iter().position(|s| s.id == id) else {
            ui.label(RichText::new(t!("Ce terminal est fermé.", "This terminal is closed.")).weak());
            return;
        };
        let focus = self.focus_tab == Some(id);
        if focus {
            self.focus_tab = None;
        }
        let mut back = false;
        egui::TopBottomPanel::top(egui::Id::new("term_tab_bar").with(id)).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let s = &self.sessions[i];
                ui.label(RichText::new(s.cwd.display().to_string()).small().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::view_bar::controls(ui, Some(true), None, None).is_some() {
                        back = true;
                    }
                });
            });
        });
        if back {
            self.set_docked(id, false);
            self.dock_request = None;
            return;
        }
        let bg = colors.bg;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(bg)).show_inside(ui, |ui| match self.grid(ui, i, focus, colors, light) {
            Some(GridAct::Restart) => self.restart(i, &ctx, shell),
            Some(GridAct::NewTerminal) => {
                if let Ok((nid, _)) = self.open_tab(&ctx, shell, None) {
                    // a new tab is created by the app for docked sessions without one
                    self.dock_request = Some(nid);
                }
            }
            Some(GridAct::PasteImage) | None => {}
        });
    }

    /// Runs `argv` (not a shell) in the bottom panel, its output kept as text (`output`); Enter
    /// once it ended runs it again. Returns its id.
    pub fn open_command_panel(&mut self, ctx: &egui::Context, argv: &[String], cwd: Option<PathBuf>) -> Result<u64, String> {
        let mut s = Session::spawn_argv(&argv.iter().map(String::as_str).collect::<Vec<_>>(), &[], cwd.unwrap_or_else(|| self.cwd()), ctx, true)
            .map_err(|e| format!("{} : {e}", argv.first().map_or("", String::as_str)))?;
        s.id = self.next_id;
        s.cmd = Some(argv.to_vec());
        self.next_id += 1;
        let id = s.id;
        self.sessions.push(s);
        self.active = self.sessions.len() - 1;
        self.want_focus = true;
        Ok(id)
    }

    /// Runs `argv` once in the bottom panel, with these variables (the program of a debugging
    /// session: Enter does not start it again). Returns its id.
    pub fn open_once_panel(&mut self, ctx: &egui::Context, argv: &[String], cwd: Option<PathBuf>, env: &[(String, String)]) -> Result<u64, String> {
        let mut s = Session::spawn_argv_env(&argv.iter().map(String::as_str).collect::<Vec<_>>(), &[], cwd.unwrap_or_else(|| self.cwd()), ctx, true, env)
            .map_err(|e| format!("{} : {e}", argv.first().map_or("", String::as_str)))?;
        s.id = self.next_id;
        s.once = true;
        self.next_id += 1;
        let id = s.id;
        self.sessions.push(s);
        self.active = self.sessions.len() - 1;
        self.want_focus = true;
        Ok(id)
    }

    /// The output kept by session `id` (`open_command_panel`), without its colors.
    pub fn output(&self, id: u64) -> Option<String> {
        let s = self.sessions.iter().find(|s| s.id == id)?;
        let bytes = s.capture.as_ref()?.lock().unwrap().clone();
        Some(strip_ansi(&String::from_utf8_lossy(&bytes)))
    }

    /// Name of a session, shown as its title while its program sets none.
    pub fn set_name(&mut self, id: u64, name: String) {
        if let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) {
            s.name = name;
        }
    }

    /// Folders of the container a session shows, for the files of its tracebacks (Ctrl+clic).
    pub fn set_mounts(&mut self, id: u64, mounts: Vec<(String, PathBuf)>) {
        if let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) {
            s.mounts = mounts;
        }
    }

    /// Types `text` at the prompt of session `id`, without Enter: the user reads it and runs it.
    /// False if the session is gone or its shell ended.
    pub fn type_text(&mut self, id: u64, text: &str) -> bool {
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) else { return false };
        if s.exited.load(Ordering::Relaxed) {
            return false;
        }
        let mut ta = s.typeahead.lock().unwrap();
        match ta.as_mut() {
            Some(waiting) => waiting.extend_from_slice(text.as_bytes()),
            None => {
                drop(ta);
                s.write(text.as_bytes());
            }
        }
        true
    }

    /// Runs `argv` for a view of its own (the Claude tab): neither in the bottom panel nor in a
    /// tab, shown by `embedded_ui`. Enter once it ended runs it again. Returns its id.
    pub fn open_embedded(&mut self, ctx: &egui::Context, argv: &[String], cwd: PathBuf, env: &[(String, String)]) -> Result<u64, String> {
        let mut s = Session::spawn_argv_env(&argv.iter().map(String::as_str).collect::<Vec<_>>(), &[], cwd, ctx, false, env)
            .map_err(|e| format!("{} : {e}", argv.first().map_or("", String::as_str)))?;
        s.id = self.next_id;
        s.cmd = Some(argv.to_vec());
        s.embedded = true;
        self.next_id += 1;
        let id = s.id;
        self.sessions.push(s);
        self.focus_tab = Some(id);
        Ok(id)
    }

    /// Session `id` (`open_embedded`) drawn in `ui`: the whole room, no bar. `focus`: it takes the keyboard.
    /// Returns true on a Ctrl+V with no text to paste (the caller may paste an image).
    pub fn embedded_ui(&mut self, ui: &mut egui::Ui, id: u64, focus: bool, colors: Colors, light: bool) -> bool {
        let ctx = ui.ctx().clone();
        let Some(i) = self.sessions.iter().position(|s| s.id == id) else { return false };
        let focus = focus || self.focus_tab == Some(id);
        if focus {
            self.focus_tab = None;
        }
        match self.grid(ui, i, focus, colors, light) {
            Some(GridAct::Restart) => self.restart(i, &ctx, None),
            Some(GridAct::PasteImage) => return true,
            _ => {}
        }
        false
    }

    /// Pastes `text` into session `id` as the keyboard's Ctrl+V does (framed when the program
    /// asks for it: several lines arrive as one text, not as lines sent one by one). False if
    /// the session is gone or ended.
    pub fn paste(&mut self, id: u64, text: &str) -> bool {
        let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) else { return false };
        if s.exited.load(Ordering::Relaxed) {
            return false;
        }
        let bracketed = s.parser.lock().unwrap().screen().bracketed_paste();
        let text = without_escapes(text);
        if bracketed {
            s.write(format!("\x1b[200~{text}\x1b[201~").as_bytes());
        } else {
            s.write(text.as_bytes());
        }
        self.focus_tab = Some(id);
        true
    }

    /// The program of session `id` is ready for a paste: it asked for framed pastes (a full-screen
    /// program such as `claude` does once it shows its field).
    pub fn ready(&self, id: u64) -> bool {
        self.sessions.iter().find(|s| s.id == id).is_some_and(|s| !s.exited.load(Ordering::Relaxed) && s.parser.lock().unwrap().screen().bracketed_paste())
    }

    /// The sessions of `open_embedded`.
    pub fn embedded_ids(&self) -> Vec<u64> {
        self.sessions.iter().filter(|s| s.embedded).map(|s| s.id).collect()
    }

    /// The command run again by Enter once session `id` ended (`claude --resume` once its session is known).
    pub fn set_command(&mut self, id: u64, argv: Vec<String>) {
        if let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) {
            s.cmd = Some(argv);
        }
    }

    /// Opens a terminal in the bottom panel; returns its id.
    pub fn open_panel(&mut self, ctx: &egui::Context, shell: Option<&str>, cwd: Option<PathBuf>) -> Result<u64, String> {
        let s = self.spawn(ctx, shell, cwd)?;
        let id = s.id;
        self.sessions.push(s);
        self.active = self.sessions.len() - 1;
        self.error = None;
        self.want_focus = true;
        Ok(id)
    }

    /// Shows session `id` in the bottom panel (when it is one of its) and gives it the keyboard.
    pub fn show(&mut self, id: u64) {
        if let Some(i) = self.sessions.iter().position(|s| s.id == id) {
            if self.sessions[i].in_panel() {
                self.active = i;
                self.want_focus = true;
            } else {
                self.focus_tab = Some(id);
            }
        }
    }

    /// Whether the program of session `id` ended (or the session is gone).
    pub fn exited(&self, id: u64) -> bool {
        self.sessions.iter().find(|s| s.id == id).is_none_or(|s| s.exited.load(Ordering::Relaxed))
    }

    /// Whether some terminal is left for the bottom panel.
    pub fn has_panel_sessions(&self) -> bool {
        self.sessions.iter().any(Session::in_panel)
    }

    /// Ids of the sessions in editor tabs.
    pub fn docked_ids(&self) -> Vec<u64> {
        self.sessions.iter().filter(|s| s.docked && !s.embedded).map(|s| s.id).collect()
    }

    /// The session whose grid is at `pos` (drawn during pass `pass` or the one before).
    pub fn grid_at(&self, pos: Pos2, pass: u64) -> Option<u64> {
        self.grids.iter().find(|(_, (p, r))| p + 1 >= pass && r.contains(pos)).map(|(id, _)| *id)
    }

    /// Files dropped on the terminal of session `id`: their paths typed at its prompt, quoted for
    /// the shell (`claude` takes the images among them as images).
    pub fn drop_paths(&mut self, id: u64, paths: &[PathBuf]) -> bool {
        let text: Vec<String> = paths.iter().map(|p| shell_quote(&p.to_string_lossy())).collect();
        self.paste(id, &format!("{} ", text.join(" ")))
    }

    /// The grid of session `i`: drawing, mouse selection, wheel, keyboard, context menu.
    fn grid(&mut self, ui: &mut egui::Ui, i: usize, want_focus: bool, colors: Colors, light: bool) -> Option<GridAct> {
        let pass = ui.ctx().cumulative_pass_nr();
        let id = self.sessions[i].id;
        self.grids.insert(id, (pass, ui.available_rect_before_wrap()));
        let s = &mut self.sessions[i];
        s.drawn.store(pass, Ordering::Relaxed);
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let (rh, cw) = ui.fonts(|f| (f.row_height(&font), crate::editor_view::char_width(f, &font)));
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let gid = Self::grid_id(s.id);
        let resp = ui.interact(rect, gid, Sense::click_and_drag()).union(resp);
        // room above the first row and under the last one (the whole area still takes the clicks)
        let rect = rect.shrink2(egui::vec2(0.0, 4.0));
        let pad = 6.0;
        let cols = (((rect.width() - 2.0 * pad) / cw).floor() as u16).max(10);
        let rows = ((rect.height() / rh).floor() as u16).max(2);
        s.resize(rows, cols);

        if want_focus || resp.clicked() || resp.drag_started() {
            resp.request_focus();
        }
        let focused = resp.has_focus();
        {
            let mut p = s.parser.lock().unwrap();
            let told = p.callbacks_mut();
            told.colors = (colors.fg, colors.bg);
            if focused {
                told.bell = false;
            }
        }
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(gid, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
            });
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
        let cell_at = |p: Pos2| -> (u16, u16) {
            let c = ((p.x - rect.left() - pad) / cw).floor().clamp(0.0, (cols - 1) as f32) as u16;
            let r = ((p.y - rect.top()) / rh).floor().clamp(0.0, (rows - 1) as f32) as u16;
            (r, c)
        };
        // Ctrl+clic (Cmd+clic) on a link: a URL, a file printed by the program (src/app.rs:12), a
        // traceback line (File "…", line N)
        let command = ui.input(|i| i.modifiers.command);
        let hover = resp.hover_pos().map(cell_at);
        let link = hover.and_then(|c| s.link_at(c));
        let traceback = if link.is_none() && command { hover.and_then(|c| traceback_at(s, c.0)) } else { None };
        if link.is_some() || traceback.is_some() {
            if command {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                if resp.clicked() {
                    match link.map(|l| l.target) {
                        Some(Target::Url(u)) => ui.ctx().open_url(egui::OpenUrl::new_tab(u)),
                        Some(Target::File(f, line)) => self.open_request = Some((f, line)),
                        None => self.open_request = traceback,
                    }
                    return None;
                }
            } else {
                let tip = if cfg!(target_os = "macos") { t!("⌘+clic : ouvrir", "⌘+click: open") } else { t!("Ctrl+clic : ouvrir", "Ctrl+click: open") };
                resp.clone().on_hover_text_at_pointer(tip);
            }
        }
        let shown_link = link.filter(|_| command);
        // selection with the mouse
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                let c = cell_at(p);
                s.sel = Some((c, c));
            }
        } else if resp.dragged() {
            if let (Some(p), Some((a, _))) = (resp.interact_pointer_pos(), s.sel) {
                s.sel = Some((a, cell_at(p)));
            }
        } else if resp.clicked() {
            s.sel = None;
        }
        // double click: the word (a path, a URL whole); triple click: the line
        if let Some(c) = resp.interact_pointer_pos().map(cell_at) {
            if resp.triple_clicked() {
                s.select_around(c, true);
            } else if resp.double_clicked() {
                s.select_around(c, false);
            }
        }

        // mouse wheel: to the program when it asked for the mouse (it scrolls itself), else the
        // scrollback, or arrow keys for full-screen programs (less, vim…)
        if resp.hovered() {
            let dy = ui.input(|i| i.raw_scroll_delta.y);
            if dy != 0.0 {
                let lines = ((dy.abs() / rh).ceil() as usize).max(1);
                let (alt, wheel) = {
                    let p = s.parser.lock().unwrap();
                    let sc = p.screen();
                    let at = resp.hover_pos().map_or((0, 0), cell_at);
                    (sc.alternate_screen(), mouse_wheel(sc.mouse_protocol_mode(), sc.mouse_protocol_encoding(), dy > 0.0, at))
                };
                if let Some(ev) = wheel {
                    let mut w = s.writer.lock().unwrap();
                    for _ in 0..lines {
                        let _ = w.write_all(&ev);
                    }
                    let _ = w.flush();
                } else if alt {
                    let key: &[u8] = if dy > 0.0 { b"\x1b[A" } else { b"\x1b[B" };
                    let mut w = s.writer.lock().unwrap();
                    for _ in 0..lines {
                        let _ = w.write_all(key);
                    }
                    let _ = w.flush();
                } else {
                    let mut p = s.parser.lock().unwrap();
                    let cur = p.screen().scrollback();
                    let next = if dy > 0.0 { cur + lines } else { cur.saturating_sub(lines) };
                    p.screen_mut().set_scrollback(next);
                }
            }
        }

        // keyboard
        let mut outcome = None;
        if focused {
            let exited = s.exited.load(Ordering::Relaxed);
            let app_cursor = s.parser.lock().unwrap().screen().application_cursor();
            let bracketed = s.parser.lock().unwrap().screen().bracketed_paste();
            let events = ui.input(|i| i.events.clone());
            for ev in events {
                match ev {
                    Event::Text(t) if !exited => s.write(t.as_bytes()),
                    Event::Paste(t) if !exited => {
                        s.pasted_at = Some(std::time::Instant::now());
                        let t = without_escapes(&t.replace("\r\n", "\r").replace('\n', "\r"));
                        if bracketed {
                            s.write(format!("\x1b[200~{t}\x1b[201~").as_bytes());
                        } else {
                            s.write(t.as_bytes());
                        }
                    }
                    Event::Copy => match s.selected_text() {
                        Some(t) => {
                            ui.ctx().output_mut(|o| o.copied_text = t);
                            s.sel = None;
                        }
                        None if !exited => s.write(b"\x03"),
                        None => {}
                    },
                    Event::Cut if !exited => s.write(b"\x18"),
                    // Ctrl+Shift+F (⌘F): search the history
                    Event::Key { key: Key::F, pressed: true, modifiers, .. } if (modifiers.ctrl && modifiers.shift) || (cfg!(target_os = "macos") && modifiers.mac_cmd) => {
                        let query = s.search.take().map(|q| q.query).or_else(|| s.selected_text().filter(|t| !t.contains('\n'))).unwrap_or_default();
                        s.search = Some(Search { query, focus: true, ..Default::default() });
                        s.search_refresh();
                    }
                    // egui keeps the press of Ctrl+V for itself (a Paste event), and gives nothing
                    // when the clipboard holds no text (an image): a release of V whose press was
                    // not seen, with no text pasted just before, is that Ctrl+V
                    Event::Key { key: Key::V, pressed: false, .. } if !exited => {
                        if !std::mem::take(&mut s.v_down) && s.pasted_at.is_none_or(|t| t.elapsed() > std::time::Duration::from_millis(1500)) {
                            outcome = Some(GridAct::PasteImage);
                        }
                    }
                    Event::Key { key, pressed: true, modifiers, .. } => {
                        if key == Key::V {
                            s.v_down = true;
                        }
                        if exited {
                            if key == Key::Enter && !s.once {
                                outcome = Some(GridAct::Restart);
                            }
                        } else if let Some(n) = shift_scroll(key, modifiers, rows).filter(|_| !s.parser.lock().unwrap().screen().alternate_screen()) {
                            // Maj+↑ ↓ (a line), Maj+PgUp PgDn (a page): the history, as the wheel
                            s.scroll_by(n);
                        } else if let Some(b) = key_bytes(key, modifiers, app_cursor) {
                            s.write(&b);
                        }
                    }
                    _ => {}
                }
            }
            if outcome.is_some() {
                return outcome;
            }
        }

        // context menu
        let has_sel = s.sel.is_some_and(|(a, b)| a != b);
        let mut action = None;
        resp.context_menu(|ui| {
            for (label, a, on) in [
                (t!("Copier", "Copy"), 0, has_sel),
                (t!("Coller", "Paste"), 1, true),
                (t!("Rechercher… (Ctrl+Maj+F)", "Find… (Ctrl+Shift+F)"), 5, true),
                (t!("Tout effacer", "Clear all"), 2, true),
                (t!("Interrompre (Ctrl+C)", "Interrupt (Ctrl+C)"), 3, true),
                (t!("Nouveau terminal", "New terminal"), 4, true),
            ] {
                if ui.add_enabled(on, egui::Button::new(label)).clicked() {
                    action = Some(a);
                    ui.close_menu();
                }
            }
        });
        match action {
            Some(0) => {
                if let Some(t) = s.selected_text() {
                    ui.ctx().output_mut(|o| o.copied_text = t);
                }
            }
            Some(1) => {
                if let Ok(t) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                    s.write(t.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
                }
            }
            Some(2) => {
                // clear the screen and the scrollback, then redraw the prompt
                s.parser.lock().unwrap().process(b"\x1b[3J\x1b[H\x1b[2J");
                s.write(b"\x0c");
            }
            Some(3) => s.write(b"\x03"),
            Some(4) => return Some(GridAct::NewTerminal),
            Some(5) => {
                s.search = Some(Search { query: s.search.take().map(|q| q.query).unwrap_or_default(), focus: true, ..Default::default() });
                s.search_refresh();
            }
            _ => {}
        }
        if s.search.is_some() {
            search_bar(ui, s, rect, gid);
        }
        let marks = s.search_marks();
        paint(ui, rect, pad, rh, cw, &font, s, colors, light, focused, shown_link.as_ref(), &marks);
        scrollbar(ui, rect, gid, s, colors);
        None
    }
}

/// The file and line of the traceback line at visible row `r` (with the rows it wraps onto).
fn traceback_at(s: &Session, r: u16) -> Option<(PathBuf, usize)> {
    let p = s.parser.lock().unwrap();
    let sc = p.screen();
    let (rows, cols) = (sc.size().0.get(), sc.size().1.get());
    let (mut a, mut b) = (r, r);
    while a > 0 && sc.row_wrapped(a - 1) {
        a -= 1;
    }
    while b + 1 < rows && sc.row_wrapped(b) {
        b += 1;
    }
    let (path, line) = crate::odoo_test::traceback_location(&sc.contents_between(a, 0, b, cols))?;
    Some((crate::odoo_test::map_path(&path, &s.mounts), line))
}

/// The bar of the search in the history, at the top right of the terminal: the field, where it is
/// (3/12), ↑ ↓ and ×. Enter goes up to the older match, Shift+Enter down, Échap closes it.
/// The scrollbar of the history, on the right edge (when there is a history, not for a
/// full-screen program): its thumb is the part shown; dragged, or a click on the track, it moves
/// there.
fn scrollbar(ui: &mut egui::Ui, rect: Rect, gid: Id, s: &mut Session, c: Colors) {
    let (total, back, rows) = {
        let p = s.parser.lock().unwrap();
        let sc = p.screen();
        if sc.alternate_screen() {
            return;
        }
        (sc.scrollback_len(), sc.scrollback(), sc.size().0.get() as usize)
    };
    if total == 0 {
        return;
    }
    let track = Rect::from_min_max(egui::pos2(rect.right() - 8.0, rect.top()), rect.right_bottom());
    let resp = ui.interact(track, gid.with("scrollbar"), Sense::click_and_drag());
    let h = track.height();
    let all = (total + rows) as f32;
    let thumb_h = (h * rows as f32 / all).max(24.0).min(h);
    // the top of the part shown, in lines from the top of the history
    let top = (total - back) as f32;
    let y = track.top() + (h - thumb_h) * top / total as f32;
    if let Some(p) = resp.interact_pointer_pos() {
        let f = ((p.y - track.top() - thumb_h / 2.0) / (h - thumb_h).max(1.0)).clamp(0.0, 1.0);
        let want = total - (f * total as f32).round() as usize;
        s.parser.lock().unwrap().screen_mut().set_scrollback(want);
    }
    let active = resp.hovered() || resp.dragged();
    if active {
        ui.ctx().set_cursor_icon(CursorIcon::Default);
    }
    let strong = if active { 0.55 } else if back > 0 { 0.35 } else { 0.18 };
    let w = if active { 6.0 } else { 4.0 };
    let thumb = Rect::from_min_size(egui::pos2(track.right() - w - 2.0, y), egui::vec2(w, thumb_h));
    ui.painter().rect_filled(thumb, w / 2.0, crate::icons::mix(c.bg, c.fg, strong));
}

/// The lines Maj+↑ ↓ and Maj+PgUp PgDn scroll the history by (up: positive).
fn shift_scroll(key: Key, m: Modifiers, rows: u16) -> Option<isize> {
    if !m.shift || m.ctrl || m.alt || m.command {
        return None;
    }
    let page = rows.saturating_sub(1).max(1) as isize;
    match key {
        Key::ArrowUp => Some(1),
        Key::ArrowDown => Some(-1),
        Key::PageUp => Some(page),
        Key::PageDown => Some(-page),
        _ => None,
    }
}

/// A turn of the wheel for a program that asked for the mouse (`None`: it did not): button 64
/// (up) or 65 (down) at the cell (row, col), in the encoding it asked for.
fn mouse_wheel(mode: vt100::MouseProtocolMode, enc: vt100::MouseProtocolEncoding, up: bool, (row, col): (u16, u16)) -> Option<Vec<u8>> {
    if mode == vt100::MouseProtocolMode::None {
        return None;
    }
    let b = if up { 64u32 } else { 65 };
    let (x, y) = (col as u32 + 1, row as u32 + 1);
    Some(match enc {
        vt100::MouseProtocolEncoding::Sgr => format!("\x1b[<{b};{x};{y}M").into_bytes(),
        _ => {
            let mut v = b"\x1b[M".to_vec();
            for n in [b + 32, x + 32, y + 32] {
                let n = n.min(255);
                if enc == vt100::MouseProtocolEncoding::Utf8 {
                    v.extend(char::from_u32(n).unwrap_or(' ').to_string().as_bytes());
                } else {
                    v.push(n as u8);
                }
            }
            v
        }
    })
}

fn search_bar(ui: &mut egui::Ui, s: &mut Session, rect: Rect, gid: Id) {
    let mut close = false;
    let mut step = None;
    let area = egui::Area::new(gid.with("search")).order(egui::Order::Foreground).fixed_pos(rect.right_top() + egui::vec2(-330.0, 6.0));
    area.show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                let Some(search) = s.search.as_mut() else { return };
                let field = ui.add(egui::TextEdit::singleline(&mut search.query).desired_width(150.0).hint_text(t!("Rechercher", "Find")));
                if std::mem::take(&mut search.focus) {
                    field.request_focus();
                }
                let (enter, shift, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.modifiers.shift, i.key_pressed(Key::Escape)));
                if field.lost_focus() && enter {
                    step = Some(!shift);
                    field.request_focus();
                } else if esc && (field.has_focus() || field.lost_focus()) {
                    close = true;
                }
                let count = match (search.current, search.hits.len()) {
                    (_, 0) if search.query.is_empty() => String::new(),
                    (_, 0) => t!("aucun", "none").to_string(),
                    (Some(i), n) => format!("{}/{n}", i + 1),
                    (None, n) => n.to_string(),
                };
                ui.label(RichText::new(count).small().weak());
                if ui.small_button("↑").on_hover_text(t!("Plus ancien (Entrée)", "Older (Enter)")).clicked() {
                    step = Some(true);
                }
                if ui.small_button("↓").on_hover_text(t!("Plus récent (Maj+Entrée)", "Newer (Shift+Enter)")).clicked() {
                    step = Some(false);
                }
                if ui.small_button("×").on_hover_text(t!("Fermer (Échap)", "Close (Esc)")).clicked() {
                    close = true;
                }
            });
        });
    });
    if close {
        s.search = None;
        ui.memory_mut(|m| m.request_focus(gid));
        return;
    }
    // typing: the newest match shown
    if s.search.as_ref().is_some_and(|q| q.query != q.of) {
        s.search_refresh();
        s.search_show();
    }
    if let Some(older) = step {
        s.search_step(older);
    }
}

/// What a Ctrl+click (Cmd+click on macOS) on the text opens.
#[derive(Clone, Debug, PartialEq)]
enum Target {
    Url(String),
    /// A file here, at a line (1-based; 0: none).
    File(PathBuf, usize),
}

/// A link of the screen: what it opens and the cells it covers (visible rows).
#[derive(Clone, Debug, PartialEq)]
struct Link {
    target: Target,
    cells: Vec<(u16, u16)>,
}

/// A word of the screen looked at as a link: the word, its first and last cells.
type LinkKey = (String, (u16, u16), (u16, u16));

/// A word of a line that may be a link: a URL, or a path (with its line and column) to look for.
#[derive(Clone, Debug, PartialEq)]
enum Cand {
    Url(String),
    Path(String, usize),
}

/// The text of the line at visible row `r` with the rows it wraps onto, and the cell of each char.
fn line_at(sc: &vt100::Screen, r: u16) -> (Vec<char>, Vec<(u16, u16)>) {
    let (rows, cols) = (sc.size().0.get(), sc.size().1.get());
    let (mut a, mut b) = (r, r);
    while a > 0 && sc.row_wrapped(a - 1) {
        a -= 1;
    }
    while b + 1 < rows && sc.row_wrapped(b) {
        b += 1;
    }
    let (mut text, mut cells) = (Vec::new(), Vec::new());
    for row in a..=b {
        for col in 0..cols {
            let Some(cell) = sc.cell(row, col) else { break };
            if cell.is_wide_continuation() {
                continue;
            }
            let t = cell.contents();
            for ch in if t.is_empty() { " " } else { t }.chars() {
                text.push(ch);
                cells.push((row, col));
            }
        }
    }
    (text, cells)
}

/// Not part of a word that could be a link: spaces, quotes, brackets, and the frames, bullets and
/// arrows of the programs (Claude Code's: written as codes, they are never drawn).
fn breaks_word(c: char) -> bool {
    c.is_whitespace() || "\"'`<>()[]{}|,;".contains(c) || ('\u{2500}'..='\u{25ff}').contains(&c) || ('\u{2190}'..='\u{21ff}').contains(&c) || ('\u{23e9}'..='\u{23ff}').contains(&c) || matches!(c, '\u{23bf}' | '\u{273b}' | '\u{2733}' | '\u{23fa}' | '\u{2026}')
}

/// The word of `text` around char `at`: its bounds (end excluded), without the punctuation that
/// ends a sentence.
fn word_at(text: &[char], at: usize) -> Option<(usize, usize)> {
    if at >= text.len() || breaks_word(text[at]) {
        return None;
    }
    let mut a = at;
    while a > 0 && !breaks_word(text[a - 1]) {
        a -= 1;
    }
    let mut b = at + 1;
    while b < text.len() && !breaks_word(text[b]) {
        b += 1;
    }
    while b > a && matches!(text[b - 1], '.' | ':' | '!' | '?') {
        b -= 1;
    }
    (b > a && at < b).then_some((a, b))
}

/// What the word `w` could open: a URL, or a path that looks like one (a folder in it, or a name
/// with an extension), with `:line`, `:line:col` or `#Lline` after it.
fn candidate(w: &str) -> Option<Cand> {
    if ["https://", "http://", "file://"].iter().any(|p| w.starts_with(p)) {
        return (w.len() > 8).then(|| Cand::Url(w.to_string()));
    }
    if w.contains("://") {
        return None;
    }
    let (path, line) = match w.split_once("#L") {
        Some((p, l)) => (p, l.parse().unwrap_or(0)),
        None => {
            let mut parts = w.splitn(3, ':');
            let first = parts.next().unwrap_or("");
            // C:\x on Windows: the drive is part of the path
            let (path, rest) = if first.len() == 1 && w.len() > 2 && w.as_bytes()[2] == b'\\' {
                let mut it = w[2..].splitn(2, ':');
                (&w[..2 + it.next().unwrap_or("").len()], it.next().unwrap_or(""))
            } else {
                (first, &w[first.len()..])
            };
            let line = rest.trim_start_matches(':').split(':').next().and_then(|l| l.parse().ok()).unwrap_or(0);
            (path, line)
        }
    };
    let name = path.rsplit(['/', '\\']).next().unwrap_or("");
    let ext = name.rsplit_once('.').is_some_and(|(stem, e)| !stem.is_empty() && !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()) && e.chars().any(|c| c.is_ascii_alphabetic()));
    let looks = path.contains('/') || path.contains('\\') || ext;
    (looks && !path.is_empty() && !path.chars().all(|c| c.is_ascii_digit() || c == '.')).then(|| Cand::Path(path.to_string(), line))
}

/// What the address of an OSC 8 link opens: a page, or a file here (`file://host/path`, with
/// `#Lline` or `:line` after it). Other schemes are left alone (only http(s) goes to the browser).
fn osc8_target(uri: &str) -> Option<Target> {
    if let Some(rest) = uri.strip_prefix("file://") {
        let path = percent_decode(&rest[rest.find('/')?..]);
        return match candidate(&path) {
            Some(Cand::Path(p, line)) if Path::new(&p).is_file() => Some(Target::File(PathBuf::from(p), line)),
            _ => Path::new(&path).is_file().then(|| Target::File(PathBuf::from(&path), 0)),
        };
    }
    crate::os::web_url(uri).map(Target::Url)
}

/// The file of path `p` printed by a program working in `cwd`, if it is there (`a/` and `b/` of a
/// git diff tried too, container folders through `mounts`).
fn resolve(p: &str, cwd: &Path, mounts: &[(String, PathBuf)]) -> Option<PathBuf> {
    let home = |rest: &str| crate::config::home().join(rest);
    let mut tries = vec![match p.strip_prefix("~/") {
        Some(rest) => home(rest),
        None if Path::new(p).is_absolute() => PathBuf::from(p),
        None => cwd.join(p),
    }];
    if let Some(rest) = p.strip_prefix("a/").or_else(|| p.strip_prefix("b/")) {
        tries.push(cwd.join(rest));
    }
    // a folder of a container is a Unix path, even on Windows
    if (Path::new(p).is_absolute() || p.starts_with('/')) && !mounts.is_empty() {
        tries.push(crate::odoo_test::map_path(p, mounts));
    }
    tries.into_iter().find(|t| t.is_file())
}

/// Text without its escape sequences (colors, cursor moves, titles) and carriage returns.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\x1b' => match it.next() {
                // CSI: parameters up to a final letter
                Some('[') => {
                    for c in it.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: up to BEL or ESC \
                Some(']') => {
                    while let Some(c) = it.next() {
                        if c == '\x07' || (c == '\x1b' && it.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// How the text of a cell is drawn besides its colors.
#[derive(Clone, Copy, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
}

fn style_of(cell: &vt100::Cell) -> Style {
    Style { bold: cell.bold(), italic: cell.italic(), underline: cell.underline() }
}

/// Text of one style at `pos`: italic through the layout, bold drawn twice a little apart (the
/// monospace font has no bold of its own), underline as a line under the cells.
#[allow(clippy::too_many_arguments)]
fn draw_text(painter: &egui::Painter, pos: Pos2, text: &str, font: &FontId, fg: Color32, st: Style, rh: f32, width: f32) {
    let job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font.clone(), color: fg, italics: st.italic, ..Default::default() });
    let galley = painter.layout_job(job);
    if st.bold {
        painter.galley(pos + egui::vec2(0.6, 0.0), galley.clone(), fg);
    }
    painter.galley(pos, galley, fg);
    if st.underline {
        let y = pos.y + rh - 2.0;
        painter.line_segment([Pos2::new(pos.x, y), Pos2::new(pos.x + width, y)], Stroke::new(1.0_f32, fg));
    }
}

#[allow(clippy::too_many_arguments)]
fn paint(ui: &egui::Ui, rect: Rect, pad: f32, rh: f32, cw: f32, font: &FontId, s: &Session, c: Colors, light: bool, focused: bool, link: Option<&Link>, marks: &[(u16, u16, u16, bool)]) {
    let painter = ui.painter_at(rect);
    // rounded: it stays inside the rounded block of the bottom panel
    painter.rect_filled(rect, 4.0, c.bg);
    let parser = s.parser.lock().unwrap();
    let screen = parser.screen();
    let (rows, cols) = (screen.size().0.get(), screen.size().1.get());
    let sel = s.sel.map(|(a, b)| if a <= b { (a, b) } else { (b, a) }).filter(|(a, b)| a != b);
    let in_sel = |r: u16, col: u16| sel.is_some_and(|(a, b)| (r, col) >= a && (r, col) <= b);
    let x_of = |col: u16| rect.left() + pad + col as f32 * cw;
    for r in 0..rows {
        let y = rect.top() + r as f32 * rh;
        let mut col = 0;
        while col < cols {
            let Some(cell) = screen.cell(r, col) else { break };
            let (fg, bg) = cell_colors(cell, c, light);
            let st = style_of(cell);
            let selected = in_sel(r, col);
            // run of cells drawn the same way (background first, then the text)
            let start = col;
            while col < cols {
                let Some(cc) = screen.cell(r, col) else { break };
                if cell_colors(cc, c, light) != (fg, bg) || style_of(cc) != st || in_sel(r, col) != selected {
                    break;
                }
                col += 1;
            }
            let end = col;
            let bg = if selected { c.selection } else { bg };
            if bg != c.bg {
                painter.rect_filled(Rect::from_min_size(Pos2::new(x_of(start), y), egui::vec2((end - start) as f32 * cw, rh)), 0.0, bg);
            }
            // ASCII text is drawn in one go, any other character (prompt icons, box drawing, wide
            // characters…) at the position of its own cell: a glyph of another width (fallback
            // font) must not shift the rest of the line
            let mut text = String::new();
            let mut text_col = start;
            let flush = |text: &mut String, at: u16| {
                if !text.trim().is_empty() {
                    draw_text(&painter, Pos2::new(x_of(at), y), text, font, fg, st, rh, text.chars().count() as f32 * cw);
                }
                text.clear();
            };
            for k in start..end {
                let Some(cc) = screen.cell(r, k) else { break };
                if cc.is_wide_continuation() {
                    continue;
                }
                let t = cc.contents();
                if t.is_ascii() {
                    if text.is_empty() {
                        text_col = k;
                    }
                    text.push_str(if t.is_empty() { " " } else { t });
                    continue;
                }
                flush(&mut text, text_col);
                let x = x_of(k);
                let cells = if cc.is_wide() { 2.0 } else { 1.0 };
                let g = painter.layout_no_wrap(t.to_string(), font.clone(), fg);
                let gw = g.size().x;
                if gw > cw * cells + 0.5 {
                    // wider than its cell(s): shrunk to fit
                    let f = FontId::new(font.size * cw * cells / gw, font.family.clone());
                    painter.text(Pos2::new(x, y + rh / 2.0), Align2::LEFT_CENTER, t, f, fg);
                } else {
                    painter.galley(Pos2::new(x + (cw * cells - gw) / 2.0, y), g, fg);
                }
                if st.underline {
                    painter.line_segment([Pos2::new(x, y + rh - 2.0), Pos2::new(x + cw * cells, y + rh - 2.0)], Stroke::new(1.0_f32, fg));
                }
            }
            flush(&mut text, text_col);
        }
    }
    // the matches of the search: framed, the one shown filled
    for &(r, c0, c1, current) in marks {
        let m = Rect::from_min_size(Pos2::new(x_of(c0), rect.top() + r as f32 * rh), egui::vec2((c1 - c0) as f32 * cw, rh));
        if current {
            painter.rect_filled(m, 2.0, c.find);
            let text: String = (c0..c1).filter_map(|k| screen.cell(r, k)).map(|x| if x.contents().is_empty() { " " } else { x.contents() }).collect();
            painter.text(m.min, Align2::LEFT_TOP, text, font.clone(), c.bg);
        } else {
            painter.rect_stroke(m, 2.0, Stroke::new(1.0_f32, c.find));
        }
    }
    // the link under the mouse while Ctrl (Cmd) is held: underlined in the accent color
    if let Some(l) = link {
        for &(r, col) in &l.cells {
            let y = rect.top() + (r as f32 + 1.0) * rh - 1.5;
            painter.line_segment([Pos2::new(x_of(col), y), Pos2::new(x_of(col) + cw, y)], Stroke::new(1.5_f32, c.accent));
        }
    }
    // cursor (only when looking at the live screen), in the shape the program asked for; a
    // blinking one stays on while typing
    use vt100::CursorStyle as Cs;
    let style = screen.cursor_style();
    let blinks = focused && matches!(style, Cs::BlinkBlock | Cs::BlinkUnderline | Cs::BlinkVerticalBar);
    let ms = s.typed_at.elapsed().as_millis() as u64;
    let lit = !blinks || ms < 530 || (ms / 530).is_multiple_of(2);
    if blinks {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(530 - ms % 530));
    }
    if lit && !screen.hide_cursor() && screen.scrollback() == 0 && !s.exited.load(Ordering::Relaxed) {
        let (r, col) = screen.cursor_position();
        let cell = Rect::from_min_size(Pos2::new(x_of(col), rect.top() + r as f32 * rh), egui::vec2(cw, rh));
        match style {
            Cs::BlinkVerticalBar | Cs::SteadyVerticalBar => {
                painter.rect_filled(Rect::from_min_size(cell.min, egui::vec2(2.0, rh)), 0.0, c.caret);
            }
            Cs::BlinkUnderline | Cs::SteadyUnderline => {
                painter.rect_filled(Rect::from_min_size(Pos2::new(cell.left(), cell.bottom() - 2.0), egui::vec2(cw, 2.0)), 0.0, c.caret);
            }
            _ if focused => {
                painter.rect_filled(cell, 1.0, c.caret);
                if let Some(cc) = screen.cell(r, col) {
                    let t = cc.contents();
                    if !t.is_empty() {
                        painter.text(cell.min, Align2::LEFT_TOP, t, font.clone(), c.bg);
                    }
                }
            }
            _ => {
                painter.rect_stroke(cell, 1.0, Stroke::new(1.0_f32, c.caret));
            }
        }
    }
    if screen.scrollback() > 0 {
        let t = tf!("↑ historique ({} lignes) — molette vers le bas ou tape pour revenir", "↑ history ({} lines) — scroll down or type to come back", screen.scrollback());
        let galley = painter.layout_no_wrap(t, FontId::proportional(11.5), c.bg);
        let pos = rect.right_top() + egui::vec2(-12.0 - galley.size().x, 6.0);
        painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand(4.0), 4.0, c.caret);
        painter.galley(pos, galley, c.bg);
    }
    if s.exited.load(Ordering::Relaxed) {
        let y = rect.top() + (screen.cursor_position().0 as f32 + 1.0).min(rows as f32 - 1.0) * rh;
        painter.text(
            Pos2::new(rect.left() + pad, y),
            Align2::LEFT_TOP,
            if s.once { t!("[processus terminé]", "[process ended]") } else { t!("[processus terminé — Entrée pour relancer un terminal]", "[process ended — Enter to start a terminal again]") },
            font.clone(),
            crate::highlight::yellow(),
        );
    }
}

/// ANSI colors 0–15 in the theme of Florm: its black, grays and white are made from its background
/// and text (the gray of the hints of Claude Code is the gray of the theme), the other colors
/// readable on its background, a little toward its text color.
fn ansi(i: u8, c: Colors, light: bool) -> Color32 {
    use crate::icons::mix;
    match i {
        0 if light => mix(c.fg, Color32::BLACK, 0.2),
        0 => mix(c.bg, c.fg, 0.22),
        7 if light => mix(c.bg, c.fg, 0.55),
        7 => mix(c.fg, c.bg, 0.12),
        8 => mix(c.bg, c.fg, 0.5),
        15 if light => c.fg,
        15 => mix(c.fg, Color32::WHITE, 0.5),
        _ => mix(ansi_base(i, light), c.fg, 0.12),
    }
}

/// The colors of `ansi` before the theme.
fn ansi_base(i: u8, light: bool) -> Color32 {
    const DARK: [u32; 16] = [
        0x3b4252, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xc8ccd4, //
        0x6b7385, 0xff7a85, 0xb5e890, 0xffd68a, 0x82c4ff, 0xdd9bf0, 0x7fd8e4, 0xffffff,
    ];
    const LIGHT: [u32; 16] = [
        0x24304a, 0xc93545, 0x2e8540, 0xa3680a, 0x1a63c9, 0x8b3fe0, 0x0a7aa8, 0x8a94a6, //
        0x5f6f88, 0xd8404f, 0x23943f, 0xb8760c, 0x3b5bdb, 0xa04fe8, 0x0a8fb0, 0x24304a,
    ];
    let h = if light { LIGHT[i as usize] } else { DARK[i as usize] };
    Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

fn color(c: vt100::Color, default: Color32, theme: Colors, light: bool) -> Color32 {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) if i < 16 => ansi(i, theme, light),
        vt100::Color::Idx(i) if i < 232 => {
            let i = i - 16;
            let v = |x: u8| if x == 0 { 0 } else { 55 + x * 40 };
            Color32::from_rgb(v(i / 36), v((i / 6) % 6), v(i % 6))
        }
        vt100::Color::Idx(i) => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

fn cell_colors(cell: &vt100::Cell, c: Colors, light: bool) -> (Color32, Color32) {
    let mut fg = color(cell.fgcolor(), c.fg, c, light);
    let mut bg = color(cell.bgcolor(), c.bg, c, light);
    // bold with a basic color: its bright variant, like most terminals
    if cell.bold() {
        if let vt100::Color::Idx(i) = cell.fgcolor() {
            if i < 8 {
                fg = ansi(i + 8, c, light);
            }
        }
    }
    // dim (Claude Code's hints, what is less important): halfway to the background
    if cell.dim() {
        fg = crate::icons::mix(fg, bg, 0.45);
    }
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    (fg, bg)
}

/// Bytes sent to the shell for a key that does not produce text.
fn key_bytes(key: Key, m: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let name = key.name();
    if m.ctrl && !m.alt && name.len() == 1 {
        let ch = name.as_bytes()[0];
        if ch.is_ascii_alphabetic() {
            return Some(vec![ch.to_ascii_uppercase() - b'A' + 1]);
        }
    }
    // xterm modifier parameter: 2 shift, 3 alt, 5 ctrl…
    let modp = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.ctrl as u8;
    let csi = |fin: char| -> Vec<u8> {
        if modp > 1 {
            format!("\x1b[1;{modp}{fin}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{fin}").into_bytes()
        } else {
            format!("\x1b[{fin}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modp > 1 {
            format!("\x1b[{n};{modp}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    Some(match key {
        // Shift/Alt+Enter: ESC CR (Meta+Enter), a new line without sending in Claude Code, zsh, fish
        Key::Enter if (m.shift || m.alt) && !m.ctrl => b"\x1b\r".to_vec(),
        Key::Enter => b"\r".to_vec(),
        Key::Backspace if m.ctrl || m.alt => b"\x1b\x7f".to_vec(),
        Key::Backspace => b"\x7f".to_vec(),
        Key::Tab if m.shift => b"\x1b[Z".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Escape => b"\x1b".to_vec(),
        Key::ArrowUp => csi('A'),
        Key::ArrowDown => csi('B'),
        Key::ArrowRight => csi('C'),
        Key::ArrowLeft => csi('D'),
        Key::Home => csi('H'),
        Key::End => csi('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => b"\x1bOP".to_vec(),
        Key::F2 => b"\x1bOQ".to_vec(),
        Key::F3 => b"\x1bOR".to_vec(),
        Key::F4 => b"\x1bOS".to_vec(),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        _ => return None,
    })
}


/// A text pasted without its escape characters: a `\x1b[201~` in it (from a file shown, or put in
/// the clipboard by a program through OSC 52) would end the bracketed paste early, and the shell
/// would run what follows.
fn without_escapes(text: &str) -> String {
    text.chars().filter(|&c| c != '\x1b' && c != '\u{9b}').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parser of `rows` × `cols` with the callbacks of Florm, `bytes` already shown.
    fn screen_of(rows: u16, cols: u16, bytes: &[u8]) -> Parser {
        let told = Told::new(&egui::Context::default(), None);
        let mut p = Parser::new_with_callbacks(nz(rows), nz(cols), 100, told);
        p.process(bytes);
        p
    }

    fn words(line: &str) -> Vec<String> {
        let text: Vec<char> = line.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < text.len() {
            match word_at(&text, i) {
                Some((a, b)) => {
                    out.push(text[a..b].iter().collect());
                    i = b.max(i + 1);
                }
                None => i += 1,
            }
        }
        out
    }

    #[test]
    fn words_of_a_line() {
        // the frames and bullets of Claude Code, the brackets of its tools, the end of a sentence
        assert_eq!(words("⏺ Update(src/app.rs)"), ["Update", "src/app.rs"]);
        assert_eq!(words("  ⎿  Voir https://github.com/o/r/pull/12."), ["Voir", "https://github.com/o/r/pull/12"]);
        assert_eq!(words("│ src/terminal.rs:1234:5: warning"), ["src/terminal.rs:1234:5", "warning"]);
        assert_eq!(words("\"a b\""), ["a", "b"]);
    }

    #[test]
    fn what_a_word_opens() {
        assert_eq!(candidate("https://x.io/a?b=1"), Some(Cand::Url("https://x.io/a?b=1".into())));
        assert_eq!(candidate("https://"), None);
        assert_eq!(candidate("src/app.rs:12:5"), Some(Cand::Path("src/app.rs".into(), 12)));
        assert_eq!(candidate("src/app.rs#L40"), Some(Cand::Path("src/app.rs".into(), 40)));
        assert_eq!(candidate("README.md"), Some(Cand::Path("README.md".into(), 0)));
        assert_eq!(candidate("~/notes.txt"), Some(Cand::Path("~/notes.txt".into(), 0)));
        assert_eq!(candidate(r"C:\src\app.rs:3"), Some(Cand::Path(r"C:\src\app.rs".into(), 3)));
        // not paths: versions, numbers, words, an extension of digits only ("e.g" is one, but only
        // a link if such a file is there)
        for w in ["1.70.2", "42", "Update", "x.123", "ssh://h/x", "."] {
            assert!(!matches!(candidate(w), Some(Cand::Path(..))), "{w}");
        }
    }

    #[test]
    fn paths_are_looked_for_from_the_folder() {
        let dir = std::env::temp_dir().join(format!("florm_term_links_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/app.rs"), "").unwrap();
        assert_eq!(resolve("src/app.rs", &dir, &[]), Some(dir.join("src/app.rs")));
        // the a/ and b/ of a git diff
        assert_eq!(resolve("b/src/app.rs", &dir, &[]), Some(dir.join("b/src/app.rs")).filter(|p| p.is_file()).or(Some(dir.join("src/app.rs"))));
        let abs = dir.join("src/app.rs").to_string_lossy().into_owned();
        assert_eq!(resolve(&abs, Path::new("/elsewhere"), &[]), Some(PathBuf::from(&abs)));
        // a folder of a container, and a file that is not there
        let mounts = vec![("/mnt/code".to_string(), dir.clone())];
        assert_eq!(resolve("/mnt/code/src/app.rs", Path::new("/"), &mounts), Some(dir.join("src/app.rs")));
        assert_eq!(resolve("src/gone.rs", &dir, &[]), None);
        assert_eq!(resolve("src", &dir, &[]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A URL too long for one row: one word over the rows it wraps onto.
    #[test]
    fn a_wrapped_url_is_one_link() {
        // "see https://example." then "com/a/very/long/path" (20 columns each), then " ok"
        let p = screen_of(4, 20, b"see https://example.com/a/very/long/path ok");
        let (text, cells) = line_at(p.screen(), 1);
        let at = cells.iter().position(|&c| c == (1, 3)).unwrap();
        let (a, b) = word_at(&text, at).unwrap();
        let w: String = text[a..b].iter().collect();
        assert_eq!(w, "https://example.com/a/very/long/path");
        assert_eq!((cells[a], cells[b - 1]), ((0, 4), (1, 19)));
    }

    #[test]
    fn what_programs_tell() {
        // OSC 7: the folder of the shell, for the relative paths
        let p = screen_of(4, 20, b"\x1b]7;file://host/home/x/my%20dir\x07\x1b]2;titre\x07");
        assert_eq!(p.callbacks().cwd.as_deref(), Some(Path::new("/home/x/my dir")));
        assert_eq!(p.callbacks().title, "titre");
        // dim text: halfway to the background
        let p = screen_of(2, 10, b"\x1b[2mgris\x1b[0m net");
        let sc = p.screen();
        assert!(sc.cell(0, 0).unwrap().dim() && !sc.cell(0, 5).unwrap().dim());
        let c = Colors { bg: Color32::BLACK, fg: Color32::WHITE, selection: Color32::GRAY, line_hl: Color32::GRAY, gutter_bg: Color32::BLACK, gutter_fg: Color32::GRAY, caret: Color32::WHITE, find: Color32::GRAY, guide: Color32::GRAY, accent: Color32::BLUE, accent2: Color32::BLUE };
        let (fg, _) = cell_colors(sc.cell(0, 0).unwrap(), c, false);
        assert!(fg.r() > 100 && fg.r() < 200, "{fg:?}");
        // the shape of the cursor (Claude Code's field: a bar)
        let p = screen_of(2, 10, b"\x1b[6 q");
        assert_eq!(p.screen().cursor_style(), vt100::CursorStyle::SteadyVerticalBar);
    }

    /// A real program prints a URL and a path: Ctrl+click opens them, a double click selects the
    /// whole URL.
    #[cfg(unix)]
    #[test]
    fn ctrl_click_opens_the_links_of_a_program() {
        let dir = std::env::temp_dir().join(format!("florm_term_click_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/app.rs"), "").unwrap();
        let ctx = egui::Context::default();
        let mut t = Terminal::new();
        let argv: Vec<String> = ["sh", "-c", "printf 'voir https://x.io/doc et src/app.rs:7\\n'; sleep 5"].map(String::from).to_vec();
        let id = t.open_embedded(&ctx, &argv, dir.clone(), &[]).unwrap();
        let screen = |t: &Terminal| t.sessions[0].parser.lock().unwrap().screen().contents();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !screen(&t).contains("src/app.rs:7") {
            assert!(std::time::Instant::now() < deadline, "{}", screen(&t));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let gray = Color32::GRAY;
        let colors = Colors { bg: Color32::BLACK, fg: Color32::WHITE, selection: gray, line_hl: gray, gutter_bg: gray, gutter_fg: gray, caret: gray, find: gray, guide: gray, accent: Color32::BLUE, accent2: Color32::BLUE };
        let screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 300.0)));
        // a clock: the clicks far apart in time are not taken for a double click
        let clock = std::cell::Cell::new(0.0);
        let frame = |t: &mut Terminal, events: Vec<Event>, command: bool| {
            let modifiers = Modifiers { ctrl: command, command, ..Default::default() };
            clock.set(clock.get() + 1.0);
            ctx.run(egui::RawInput { screen_rect, events, modifiers, time: Some(clock.get()), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| t.embedded_ui(ui, id, false, colors, false));
            })
        };
        frame(&mut t, Vec::new(), false);
        // the middle of the cell of `text` on the screen
        let font = egui::TextStyle::Monospace.resolve(&ctx.style());
        let (rh, cw) = ctx.fonts(|f| (f.row_height(&font), crate::editor_view::char_width(f, &font)));
        let grid = t.grids[&id].1;
        let at = |t: &Terminal, text: &str| {
            let contents = screen(t);
            let (row, line) = contents.lines().enumerate().find(|(_, l)| l.contains(text)).unwrap();
            let col = line[..line.find(text).unwrap()].chars().count() + 1;
            Pos2::new(grid.left() + 6.0 + (col as f32 + 0.5) * cw, grid.top() + 4.0 + (row as f32 + 0.5) * rh)
        };
        let click = |p: Pos2, command: bool| {
            let modifiers = Modifiers { ctrl: command, command, ..Default::default() };
            vec![
                Event::PointerMoved(p),
                Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers },
                Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers },
            ]
        };
        let p = at(&t, "x.io");
        frame(&mut t, vec![Event::PointerMoved(p)], true);
        let out = frame(&mut t, click(p, true), true);
        assert_eq!(out.platform_output.open_url.map(|u| u.url).as_deref(), Some("https://x.io/doc"));
        let p = at(&t, "app.rs");
        frame(&mut t, vec![Event::PointerMoved(p)], true);
        frame(&mut t, click(p, true), true);
        assert_eq!(t.open_request.take(), Some((dir.join("src/app.rs"), 7)));
        // without Ctrl, nothing opens; a double click selects the whole URL
        let p = at(&t, "x.io");
        let mut twice = click(p, false);
        twice.extend(click(p, false));
        let out = frame(&mut t, twice, false);
        assert!(out.platform_output.open_url.is_none());
        assert_eq!(t.sessions[0].selected_text().as_deref(), Some("https://x.io/doc"));
        t.close(id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a program writes to its terminal's input, kept to be read.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn programs_learn_the_colors_of_the_theme() {
        let sink = Sink::default();
        let writer: SharedWriter = Arc::new(Mutex::new(Box::new(sink.clone())));
        let mut told = Told::new(&egui::Context::default(), Some(writer));
        told.colors = (Color32::from_rgb(0xee, 0xdd, 0xcc), Color32::from_rgb(0x12, 0x34, 0x56));
        let mut p = Parser::new_with_callbacks(nz(4), nz(20), 10, told);
        // asked as Claude Code or vim do, ended by BEL or by ESC \
        p.process(b"\x1b]11;?\x07\x1b]10;?\x1b\\");
        let answer = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert_eq!(answer, "\x1b]11;rgb:1212/3434/5656\x1b\\\x1b]10;rgb:eeee/dddd/cccc\x1b\\");
    }

    #[test]
    fn links_sent_by_programs() {
        let mut p = screen_of(4, 30, b"voir \x1b]8;;https://x.io/pr/12\x07PR #12\x1b]8;;\x07 ok");
        let (uri, cells) = p.callbacks().osc8_at(p.screen(), (0, 7)).unwrap();
        assert_eq!(uri, "https://x.io/pr/12");
        assert_eq!(cells, (5..11).map(|c| (0, c)).collect::<Vec<_>>());
        assert!(p.callbacks().osc8_at(p.screen(), (0, 12)).is_none());
        // a link that scrolled into the history follows its text
        p.process(b"\r\n1\r\n2\r\n3\r\n");
        let shown = |p: &Parser| (0..4).find(|&r| p.callbacks().osc8_at(p.screen(), (r, 6)).is_some());
        assert_eq!(shown(&p), None);
        p.screen_mut().set_scrollback(3);
        assert_eq!(shown(&p), Some(0));
        // redrawn over by other text: the old link is gone
        p.screen_mut().set_scrollback(0);
        let mut p = screen_of(4, 30, b"\x1b]8;;https://x.io\x07lien\x1b]8;;\x07");
        p.process(b"\r\x1b[2Kautre");
        assert!(p.callbacks().osc8_at(p.screen(), (0, 1)).is_none());
        // what their addresses open
        assert_eq!(osc8_target("https://x.io/a"), Some(Target::Url("https://x.io/a".into())));
        assert_eq!(osc8_target("javascript:alert(1)"), None);
        assert_eq!(osc8_target("mailto:a@b.c"), None);
        assert_eq!(osc8_target("https://x.io/a\"&calc"), None);
        let f = std::env::temp_dir().join(format!("florm_osc8_{}.txt", std::process::id()));
        std::fs::write(&f, "").unwrap();
        let uri = format!("file://host{}", f.display());
        if cfg!(unix) {
            assert_eq!(osc8_target(&uri), Some(Target::File(f.clone(), 0)));
        }
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn search_in_the_history() {
        let mut out = Vec::new();
        for i in 1..=30 {
            out.extend(format!("ligne {i}\r\n").into_bytes());
        }
        let mut p = screen_of(5, 20, &out);
        let (lines, first) = all_lines(&mut p);
        assert_eq!(first, 0);
        assert_eq!(lines.len(), 31);
        assert_eq!((lines[0].as_str(), lines[29].as_str()), ("ligne 1", "ligne 30"));
        assert_eq!(p.screen().scrollback(), 0);
        // case ignored, unless a capital is typed
        let hits = find_hits(&lines, first, "LIGNE 2");
        assert!(hits.is_empty());
        let hits = find_hits(&lines, first, "ligne 2");
        assert_eq!(hits.len(), 11);
        assert_eq!(hits[0], (1, 0, 7));
        assert_eq!(find_hits(&lines, 0, "e 3").len(), 2);
        assert!(find_hits(&lines, 0, "").is_empty());
    }

    #[test]
    fn bell_and_palette() {
        let p = screen_of(2, 10, b"x\x07");
        assert!(p.callbacks().bell);
        let gray = Color32::GRAY;
        let c = Colors { bg: Color32::from_rgb(10, 20, 30), fg: Color32::from_rgb(210, 220, 230), selection: gray, line_hl: gray, gutter_bg: gray, gutter_fg: gray, caret: gray, find: gray, guide: gray, accent: gray, accent2: gray };
        // the gray of the hints: between the background and the text of the theme
        assert_eq!(ansi(8, c, false), crate::icons::mix(c.bg, c.fg, 0.5));
        assert_eq!(ansi(15, c, true), c.fg);
        assert_ne!(ansi(1, c, false), ansi_base(1, false));
    }

    /// Ctrl+V with an image in the clipboard: egui gives only the release of V, the Claude tab
    /// is told so; V typed, or a text pasted, is not that.
    #[cfg(unix)]
    #[test]
    fn a_ctrl_v_with_no_text_asks_for_an_image() {
        let ctx = egui::Context::default();
        let mut t = Terminal::new();
        let argv: Vec<String> = ["sh", "-c", "sleep 5"].map(String::from).to_vec();
        let id = t.open_embedded(&ctx, &argv, std::env::temp_dir(), &[]).unwrap();
        let gray = Color32::GRAY;
        let colors = Colors { bg: Color32::BLACK, fg: Color32::WHITE, selection: gray, line_hl: gray, gutter_bg: gray, gutter_fg: gray, caret: gray, find: gray, guide: gray, accent: gray, accent2: gray };
        let screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(700.0, 300.0)));
        let first = std::cell::Cell::new(true);
        let frame = |t: &mut Terminal, events: Vec<Event>| {
            let mut asked = false;
            let _ = ctx.run(egui::RawInput { screen_rect, events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| asked = t.embedded_ui(ui, id, first.replace(false), colors, false));
            });
            asked
        };
        let key = |pressed: bool, ctrl: bool| {
            let modifiers = Modifiers { ctrl, command: ctrl, ..Default::default() };
            Event::Key { key: Key::V, physical_key: None, pressed, repeat: false, modifiers }
        };
        frame(&mut t, Vec::new());
        // the image: only the release, whether Ctrl is still held or not
        assert!(frame(&mut t, vec![key(false, true)]));
        assert!(frame(&mut t, vec![key(false, false)]));
        // v typed
        assert!(!frame(&mut t, vec![key(true, false), Event::Text("v".into())]));
        assert!(!frame(&mut t, vec![key(false, false)]));
        // a text pasted
        assert!(!frame(&mut t, vec![Event::Paste("texte".into())]));
        assert!(!frame(&mut t, vec![key(false, true)]));
    }

    /// Ctrl+Shift+F in a real terminal: its history searched, the view scrolled to the match.
    #[cfg(unix)]
    #[test]
    fn the_search_scrolls_to_its_match() {
        let ctx = egui::Context::default();
        let mut t = Terminal::new();
        let argv: Vec<String> = ["sh", "-c", "i=1; while [ $i -le 300 ]; do echo ligne-$i; i=$((i+1)); done; sleep 5"].map(String::from).to_vec();
        let id = t.open_embedded(&ctx, &argv, std::env::temp_dir(), &[]).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !t.sessions[0].parser.lock().unwrap().screen().contents().contains("ligne-300") {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let gray = Color32::GRAY;
        let colors = Colors { bg: Color32::BLACK, fg: Color32::WHITE, selection: gray, line_hl: gray, gutter_bg: gray, gutter_fg: gray, caret: gray, find: gray, guide: gray, accent: gray, accent2: gray };
        let screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(700.0, 300.0)));
        // the keyboard given to the terminal once, as the Claude tab does
        let first = std::cell::Cell::new(true);
        let frame = |t: &mut Terminal, events: Vec<Event>, modifiers: Modifiers| {
            let _ = ctx.run(egui::RawInput { screen_rect, events, modifiers, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| t.embedded_ui(ui, id, first.replace(false), colors, false));
            });
        };
        frame(&mut t, Vec::new(), Modifiers::default());
        let m = Modifiers { ctrl: true, shift: true, command: true, ..Default::default() };
        frame(&mut t, vec![Event::Key { key: Key::F, physical_key: None, pressed: true, repeat: false, modifiers: m }], m);
        assert!(t.sessions[0].search.is_some());
        frame(&mut t, vec![Event::Text("ligne-42".into())], Modifiers::default());
        frame(&mut t, Vec::new(), Modifiers::default());
        let s = &t.sessions[0];
        let search = s.search.as_ref().unwrap();
        assert_eq!((search.query.as_str(), search.hits.len()), ("ligne-42", 1));
        assert!(s.parser.lock().unwrap().screen().scrollback() > 0);
        let marks = s.search_marks();
        assert_eq!(marks.len(), 1);
        assert!(marks[0].3);
        let row = marks[0].0;
        assert!(s.parser.lock().unwrap().screen().contents_between(row, 0, row, 20).starts_with("ligne-42"));
        t.close(id);
    }

    /// The wheel goes to a program that asked for the mouse (Claude Code in full screen, htop…),
    /// in its encoding; Maj+↑ ↓ PgUp PgDn scroll the history, as far as it goes.
    #[test]
    fn wheel_and_shift_arrows() {
        use vt100::{MouseProtocolEncoding as E, MouseProtocolMode as M};
        assert_eq!(mouse_wheel(M::None, E::Sgr, true, (0, 0)), None);
        assert_eq!(mouse_wheel(M::AnyMotion, E::Sgr, true, (4, 9)), Some(b"\x1b[<64;10;5M".to_vec()));
        assert_eq!(mouse_wheel(M::PressRelease, E::Default, false, (0, 0)), Some(vec![0x1b, b'[', b'M', 65 + 32, 33, 33]));
        let p = screen_of(3, 10, b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(p.screen().mouse_protocol_mode(), M::PressRelease);
        assert_eq!(p.screen().mouse_protocol_encoding(), E::Sgr);

        let shift = Modifiers { shift: true, ..Default::default() };
        assert_eq!(shift_scroll(Key::ArrowUp, shift, 10), Some(1));
        assert_eq!(shift_scroll(Key::PageDown, shift, 10), Some(-9));
        assert_eq!(shift_scroll(Key::ArrowUp, Modifiers::default(), 10), None);
        assert_eq!(shift_scroll(Key::ArrowUp, Modifiers { ctrl: true, ..shift }, 10), None);

        let mut p = screen_of(3, 10, b"1\r\n2\r\n3\r\n4\r\n5\r\n6");
        assert_eq!(p.screen().scrollback_len(), 3);
        p.screen_mut().set_scrollback(99);
        assert_eq!(p.screen().scrollback(), 3, "as far as it goes");
    }

    #[test]
    fn dropped_paths_quoted() {
        assert_eq!(shell_quote("/tmp/a-b_c.png"), "/tmp/a-b_c.png");
        if cfg!(unix) {
            assert_eq!(shell_quote("/tmp/my file.png"), "'/tmp/my file.png'");
            assert_eq!(shell_quote("/tmp/it's"), r"'/tmp/it'\''s'");
        } else {
            assert_eq!(shell_quote(r"C:\my file.png"), "\"C:\\my file.png\"");
        }
        assert_eq!(percent_decode("a%2Fb%zz"), "a/b%zz");
    }

    #[test]
    fn answers_terminal_queries() {
        let at = || (4u16, 9u16);
        assert_eq!(replies(b"\x1b[c", at), b"\x1b[?62;22c");
        assert_eq!(replies(b"prompt\x1b[?u\x1b[0c", at), b"\x1b[?62;22c");
        assert_eq!(replies(b"\x1b[6n", at), b"\x1b[5;10R");
        assert_eq!(replies(b"\x1b[31mrouge\x1b[0m", at), b"");
    }

    #[test]
    fn keys() {
        let none = Modifiers::NONE;
        assert_eq!(key_bytes(Key::C, Modifiers::CTRL, false), Some(vec![3]));
        assert_eq!(key_bytes(Key::D, Modifiers::CTRL, false), Some(vec![4]));
        assert_eq!(key_bytes(Key::ArrowUp, none, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, none, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(Key::ArrowLeft, Modifiers::CTRL, false), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(key_bytes(Key::Delete, none, false), Some(b"\x1b[3~".to_vec()));
        assert_eq!(key_bytes(Key::A, none, false), None);
        assert_eq!(key_bytes(Key::Enter, none, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(Key::Enter, Modifiers::SHIFT, false), Some(b"\x1b\r".to_vec()));
        assert_eq!(key_bytes(Key::Enter, Modifiers::ALT, false), Some(b"\x1b\r".to_vec()));
    }

    #[cfg(unix)]
    #[test]
    fn terminals_in_tabs() {
        let ctx = egui::Context::default();
        let mut t = Terminal::new();
        t.cwd = Some(std::env::temp_dir());
        t.open(&ctx, Some("sh"), None);
        let (id, _) = t.open_tab(&ctx, Some("sh"), None).unwrap();
        assert_eq!(t.docked_ids(), [id]);
        assert!(t.has_panel_sessions());
        // the panel's terminal goes into a tab too, then back
        let panel = t.sessions[0].id;
        t.set_docked(panel, true);
        assert!(!t.has_panel_sessions());
        assert_eq!(t.docked_ids().len(), 2);
        t.set_docked(panel, false);
        assert_eq!(t.sessions[t.active].id, panel);
        t.close(id);
        assert!(t.docked_ids().is_empty());
        assert!(t.title_of(id).is_none() && t.title_of(panel).is_some());
    }

    /// The install guides type a command at the prompt (no Enter): once the shell started, even
    /// when asked at once; the text typed before and not run is cleared by Ctrl+U.
    #[cfg(unix)]
    #[test]
    fn types_at_the_prompt() {
        let ctx = egui::Context::default();
        let mut t = Terminal::new();
        t.cwd = Some(std::env::temp_dir());
        let id = t.open_panel(&ctx, Some("bash --norc --noprofile"), None).unwrap();
        assert!(t.type_text(id, "echo un"));
        assert!(t.type_text(id, "\x15echo $((6*7))"));
        let screen = |t: &Terminal| t.sessions[0].parser.lock().unwrap().screen().contents();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !screen(&t).contains("echo $((6*7))") {
            assert!(std::time::Instant::now() < deadline, "{}", screen(&t));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // not run: no Enter
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!screen(&t).contains("42") && !screen(&t).contains("echo un"), "{}", screen(&t));
        t.sessions[0].write(b"\r");
        while !screen(&t).lines().any(|l| l.trim() == "42") {
            assert!(std::time::Instant::now() < deadline, "{}", screen(&t));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        t.close(id);
        assert!(!t.type_text(id, "x"));
    }

    #[cfg(unix)]
    #[test]
    fn runs_a_real_shell() {
        let ctx = egui::Context::default();
        let mut s = Session::spawn("sh", std::env::temp_dir(), &ctx).unwrap();
        s.write(b"printf '\\033[31mrouge\\033[0m %s\\n' $((6*7))\r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let found = {
                let p = s.parser.lock().unwrap();
                let txt = p.screen().contents();
                txt.contains("rouge 42").then(|| {
                    // the "r" of "rouge" is red
                    let (rows, cols) = (p.screen().size().0.get(), p.screen().size().1.get());
                    (0..rows)
                        .flat_map(|r| (0..cols).map(move |c| (r, c)))
                        .find(|&(r, c)| {
                            p.screen().cell(r, c).is_some_and(|x| x.contents() == "r")
                                && p.screen().cell(r, c + 1).is_some_and(|x| x.contents() == "o")
                                && p.screen().cell(r, c + 4).is_some_and(|x| x.contents() == "e")
                                && p.screen().cell(r, c + 6).is_some_and(|x| x.contents() == "4")
                        })
                        .map(|(r, c)| p.screen().cell(r, c).unwrap().fgcolor())
                })
            };
            if let Some(color) = found {
                assert_eq!(color, Some(vt100::Color::Idx(1)));
                break;
            }
            assert!(std::time::Instant::now() < deadline, "pas de sortie du shell");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        s.resize(10, 40);
        assert_eq!(s.parser.lock().unwrap().screen().size(), (nz(10), nz(40)));
    }
}
