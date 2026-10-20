//! Integrated terminal, like VS Code's: a real shell in a pseudo-terminal (portable-pty: Unix PTY
//! or Windows ConPTY), its output interpreted by a VT100/xterm emulator (vt100) and drawn as a grid.
//! Several terminals can be open, each in its own tab.

use crate::highlight::Colors;
use eframe::egui::{self, Align2, Color32, CursorIcon, Event, FontId, Id, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const SCROLLBACK: usize = 10_000;

/// Command line run through the shell, for plugins (not the interactive terminal).
#[cfg(windows)]
pub fn shell_command(cmd: &str, _shell: Option<&str>) -> Command {
    let mut c = crate::os::command("cmd");
    c.args(["/C", cmd]);
    c
}

#[cfg(not(windows))]
pub fn shell_command(cmd: &str, shell: Option<&str>) -> Command {
    let mut c = crate::os::command(shell.unwrap_or("sh"));
    c.arg("-c").arg(cmd);
    c
}

/// Focus id of the terminal grid (shortcuts other than a few global ones go to the shell).
pub fn term_id() -> Id {
    Id::new("florm_terminal")
}

fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
    }
}

struct Session {
    name: String,
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    exited: Arc<AtomicBool>,
    cwd: PathBuf,
    size: (u16, u16),
    /// Selection in visible cells: (row, col) anchor and end.
    sel: Option<((u16, u16), (u16, u16))>,
}

impl Session {
    fn spawn(shell: &str, cwd: PathBuf, ctx: &egui::Context) -> Result<Session, String> {
        let size = (24, 80);
        let pair = native_pty_system()
            .openpty(PtySize { rows: size.0, cols: size.1, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut parts = shell.split_whitespace();
        let mut cmd = CommandBuilder::new(parts.next().unwrap_or("sh"));
        cmd.args(parts);
        cmd.cwd(&cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "florm");
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("{shell} : {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let parser = Arc::new(Mutex::new(vt100::Parser::new(size.0, size.1, SCROLLBACK)));
        let exited = Arc::new(AtomicBool::new(false));
        let (p, ex, ctx) = (parser.clone(), exited.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        p.lock().unwrap().process(&buf[..n]);
                        ctx.request_repaint();
                    }
                }
            }
            ex.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        });
        let name = std::path::Path::new(shell.split_whitespace().next().unwrap_or(shell))
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| shell.to_string());
        Ok(Session { name, parser, writer, master: pair.master, child, exited, cwd, size, sel: None })
    }

    fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
        // typing brings the view back to the prompt
        self.parser.lock().unwrap().set_scrollback(0);
        self.sel = None;
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        if self.size != (rows, cols) {
            self.size = (rows, cols);
            self.parser.lock().unwrap().set_size(rows, cols);
            let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }

    fn title(&self) -> String {
        let t = self.parser.lock().unwrap().screen().title().to_string();
        if t.trim().is_empty() {
            self.name.clone()
        } else {
            t
        }
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
    pub want_focus: bool,
    sessions: Vec<Session>,
    active: usize,
    error: Option<String>,
}

impl Terminal {
    pub fn new() -> Self {
        Terminal { cwd: None, want_focus: false, sessions: Vec::new(), active: 0, error: None }
    }

    fn cwd(&self) -> PathBuf {
        self.cwd.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    /// Opens a new terminal tab in `cwd` (or the current folder).
    pub fn open(&mut self, ctx: &egui::Context, shell: Option<&str>, cwd: Option<PathBuf>) {
        let shell = shell.map(str::to_string).filter(|s| !s.trim().is_empty()).unwrap_or_else(default_shell);
        let cwd = cwd.unwrap_or_else(|| self.cwd());
        match Session::spawn(&shell, cwd, ctx) {
            Ok(s) => {
                self.sessions.push(s);
                self.active = self.sessions.len() - 1;
                self.error = None;
                self.want_focus = true;
            }
            Err(e) => self.error = Some(format!("Impossible de lancer le terminal : {e}")),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, shell: Option<&str>, colors: Colors, light: bool) {
        let ctx = ui.ctx().clone();
        if self.sessions.is_empty() && self.error.is_none() {
            self.open(&ctx, shell, None);
        }
        // --- tabs
        let (mut close, mut new) = (None, false);
        ui.horizontal(|ui| {
            for (i, s) in self.sessions.iter().enumerate() {
                let done = s.exited.load(Ordering::Relaxed);
                let label = format!("{} {}{}", i + 1, s.title(), if done { " (terminé)" } else { "" });
                let r = ui.selectable_label(i == self.active, RichText::new(label).monospace());
                if r.clicked() {
                    self.active = i;
                    self.want_focus = true;
                }
                let r = r.on_hover_text(s.cwd.display().to_string());
                if r.middle_clicked() {
                    close = Some(i);
                }
                if ui.small_button("×").on_hover_text("Fermer ce terminal").clicked() {
                    close = Some(i);
                }
                ui.add_space(6.0);
            }
            let plus = egui::Button::new(RichText::new("+").size(16.0).strong()).min_size(egui::vec2(26.0, 22.0));
            if ui.add(plus).on_hover_text("Nouveau terminal").clicked() {
                new = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new("Ctrl+C : interrompre (ou copier la sélection) · Ctrl+V : coller · clic droit : menu")
                        .small()
                        .weak(),
                );
            });
        });
        if let Some(i) = close {
            self.sessions.remove(i);
            self.active = self.active.min(self.sessions.len().saturating_sub(1));
        }
        if new {
            self.open(&ctx, shell, None);
        }
        if let Some(e) = &self.error {
            ui.label(RichText::new(e).color(crate::highlight::red()));
            if ui.button("Réessayer").clicked() {
                self.error = None;
            }
            return;
        }
        let Some(s) = self.sessions.get_mut(self.active) else {
            ui.label(RichText::new("Aucun terminal ouvert — « + » pour en ouvrir un.").weak());
            return;
        };

        // --- grid
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let (rh, cw) = ui.fonts(|f| (f.row_height(&font), crate::editor_view::char_width(f, &font)));
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let resp = ui.interact(rect, term_id(), Sense::click_and_drag()).union(resp);
        let pad = 6.0;
        let cols = (((rect.width() - 2.0 * pad) / cw).floor() as u16).max(10);
        let rows = ((rect.height() / rh).floor() as u16).max(2);
        s.resize(rows, cols);

        if self.want_focus || resp.clicked() || resp.drag_started() {
            resp.request_focus();
            self.want_focus = false;
        }
        let focused = resp.has_focus();
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    term_id(),
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                )
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

        // mouse wheel: scrollback, or arrow keys for full-screen programs (less, vim…)
        if resp.hovered() {
            let dy = ui.input(|i| i.raw_scroll_delta.y);
            if dy != 0.0 {
                let lines = ((dy.abs() / rh).ceil() as usize).max(1);
                let alt = s.parser.lock().unwrap().screen().alternate_screen();
                if alt {
                    let key: &[u8] = if dy > 0.0 { b"\x1b[A" } else { b"\x1b[B" };
                    for _ in 0..lines {
                        let _ = s.writer.write_all(key);
                    }
                    let _ = s.writer.flush();
                } else {
                    let mut p = s.parser.lock().unwrap();
                    let cur = p.screen().scrollback();
                    let next = if dy > 0.0 { cur + lines } else { cur.saturating_sub(lines) };
                    p.set_scrollback(next);
                }
            }
        }

        // keyboard
        if focused {
            let exited = s.exited.load(Ordering::Relaxed);
            let app_cursor = s.parser.lock().unwrap().screen().application_cursor();
            let bracketed = s.parser.lock().unwrap().screen().bracketed_paste();
            let events = ui.input(|i| i.events.clone());
            let mut restart = false;
            for ev in events {
                match ev {
                    Event::Text(t) if !exited => s.write(t.as_bytes()),
                    Event::Paste(t) if !exited => {
                        let t = t.replace("\r\n", "\r").replace('\n', "\r");
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
                    Event::Key { key, pressed: true, modifiers, .. } => {
                        if exited {
                            restart |= key == Key::Enter;
                        } else if let Some(b) = key_bytes(key, modifiers, app_cursor) {
                            s.write(&b);
                        }
                    }
                    _ => {}
                }
            }
            if restart {
                let cwd = s.cwd.clone();
                self.sessions.remove(self.active);
                self.open(&ctx, shell, Some(cwd));
                return;
            }
        }

        // context menu
        let has_sel = s.sel.is_some_and(|(a, b)| a != b);
        let mut action = None;
        resp.context_menu(|ui| {
            for (label, a, on) in [
                ("Copier", 0, has_sel),
                ("Coller", 1, true),
                ("Tout effacer", 2, true),
                ("Interrompre (Ctrl+C)", 3, true),
                ("Nouveau terminal", 4, true),
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
            Some(4) => {
                self.open(&ctx, shell, None);
                return;
            }
            _ => {}
        }
        let Some(s) = self.sessions.get(self.active) else { return };
        paint(ui, rect, pad, rh, cw, &font, s, colors, light, focused);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint(ui: &egui::Ui, rect: Rect, pad: f32, rh: f32, cw: f32, font: &FontId, s: &Session, c: Colors, light: bool, focused: bool) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, c.bg);
    let parser = s.parser.lock().unwrap();
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let sel = s.sel.map(|(a, b)| if a <= b { (a, b) } else { (b, a) }).filter(|(a, b)| a != b);
    let in_sel = |r: u16, col: u16| sel.is_some_and(|(a, b)| (r, col) >= a && (r, col) <= b);
    for r in 0..rows {
        let y = rect.top() + r as f32 * rh;
        let mut col = 0;
        while col < cols {
            let Some(cell) = screen.cell(r, col) else { break };
            let (fg, bg) = cell_colors(cell, c, light);
            let selected = in_sel(r, col);
            // run of cells with the same colors
            let start = col;
            let mut text = String::new();
            while col < cols {
                let Some(cc) = screen.cell(r, col) else { break };
                if cell_colors(cc, c, light) != (fg, bg) || in_sel(r, col) != selected {
                    break;
                }
                if !cc.is_wide_continuation() {
                    let t = cc.contents();
                    text.push_str(if t.is_empty() { " " } else { &t });
                }
                col += 1;
            }
            let x = rect.left() + pad + start as f32 * cw;
            let w = (col - start) as f32 * cw;
            let bg = if selected { c.selection } else { bg };
            if bg != c.bg {
                painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), egui::vec2(w, rh)), 0.0, bg);
            }
            if !text.trim().is_empty() {
                painter.text(Pos2::new(x, y), Align2::LEFT_TOP, &text, font.clone(), fg);
            }
        }
    }
    // cursor (only when looking at the live screen)
    if !screen.hide_cursor() && screen.scrollback() == 0 && !s.exited.load(Ordering::Relaxed) {
        let (r, col) = screen.cursor_position();
        let cur = Rect::from_min_size(Pos2::new(rect.left() + pad + col as f32 * cw, rect.top() + r as f32 * rh), egui::vec2(cw, rh));
        if focused {
            painter.rect_filled(cur, 1.0, c.caret);
            if let Some(cell) = screen.cell(r, col) {
                let t = cell.contents();
                if !t.is_empty() {
                    painter.text(cur.min, Align2::LEFT_TOP, t, font.clone(), c.bg);
                }
            }
        } else {
            painter.rect_stroke(cur, 1.0, Stroke::new(1.0_f32, c.caret));
        }
    }
    if screen.scrollback() > 0 {
        let t = format!("↑ historique ({} lignes) — molette vers le bas ou tape pour revenir", screen.scrollback());
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
            "[processus terminé — Entrée pour relancer un terminal]",
            font.clone(),
            crate::highlight::yellow(),
        );
    }
}

/// ANSI colors 0–15, readable on the theme background.
fn ansi(i: u8, light: bool) -> Color32 {
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

fn color(c: vt100::Color, default: Color32, light: bool) -> Color32 {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) if i < 16 => ansi(i, light),
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
    let mut fg = color(cell.fgcolor(), c.fg, light);
    let mut bg = color(cell.bgcolor(), c.bg, light);
    // bold with a basic color: its bright variant, like most terminals
    if cell.bold() {
        if let vt100::Color::Idx(i) = cell.fgcolor() {
            if i < 8 {
                fg = ansi(i + 8, light);
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
                    let (rows, cols) = p.screen().size();
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
        assert_eq!(s.parser.lock().unwrap().screen().size(), (10, 40));
    }
}
