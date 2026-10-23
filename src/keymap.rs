use crate::commands::{self, Cmd};
use eframe::egui::{Key, Modifiers};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Combo {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Combo {
    pub fn from_event(key: Key, m: Modifiers) -> Self {
        Combo { key, ctrl: m.command || m.ctrl, shift: m.shift, alt: m.alt }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_lowercase();
        let (mods, key) = match s.rsplit_once('+') {
            // "ctrl++" style is not supported, use "ctrl+plus".
            Some((m, k)) if !k.is_empty() => (m, k),
            _ => ("", s.as_str()),
        };
        let mut c = Combo { key: key_from_name(key)?, ctrl: false, shift: false, alt: false };
        for m in mods.split('+').filter(|m| !m.is_empty()) {
            match m {
                "ctrl" | "cmd" | "super" | "primary" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" | "option" => c.alt = true,
                _ => return None,
            }
        }
        Some(c)
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s += if cfg!(target_os = "macos") { "Cmd+" } else { "Ctrl+" };
        }
        if self.alt {
            s += "Alt+";
        }
        if self.shift {
            s += "Shift+";
        }
        s += self.key.symbol_or_name();
        s
    }
}

fn key_from_name(n: &str) -> Option<Key> {
    use Key::*;
    Some(match n {
        "up" => ArrowUp,
        "down" => ArrowDown,
        "left" => ArrowLeft,
        "right" => ArrowRight,
        "enter" | "return" => Enter,
        "tab" => Tab,
        "escape" | "esc" => Escape,
        "backspace" => Backspace,
        "delete" | "del" => Delete,
        "insert" => Insert,
        "home" => Home,
        "end" => End,
        "pageup" => PageUp,
        "pagedown" => PageDown,
        "space" => Space,
        "/" | "slash" => Slash,
        "\\" | "backslash" => Backslash,
        "[" => OpenBracket,
        "]" => CloseBracket,
        "`" | "backtick" => Backtick,
        "-" | "minus" => Minus,
        "=" | "equals" => Equals,
        "plus" => Plus,
        "," | "comma" => Comma,
        "." | "period" => Period,
        ";" | "semicolon" => Semicolon,
        _ => return Key::from_name(&n.to_uppercase()).or_else(|| Key::from_name(n)),
    })
}

const DEFAULTS: &[(&str, &str)] = &[
    ("ctrl+shift+p", "command_palette"),
    ("f1", "command_palette"),
    ("ctrl+p", "quick_open"),
    ("ctrl+g", "goto_line"),
    ("ctrl+f", "find"),
    ("ctrl+h", "replace"),
    ("ctrl+shift+f", "find_in_files"),
    ("f4", "next_result"),
    ("shift+f4", "prev_result"),
    ("ctrl+alt+p", "switch_project"),
    ("f3", "find_next"),
    ("shift+f3", "find_prev"),
    ("alt+f3", "select_all_occurrences"),
    ("alt+z", "toggle_word_wrap"),
    ("ctrl+s", "save"),
    ("ctrl+shift+s", "save_as"),
    ("ctrl+alt+s", "save_all"),
    ("ctrl+n", "new_file"),
    ("ctrl+o", "open_file"),
    ("ctrl+shift+o", "open_folder"),
    ("ctrl+w", "close_tab"),
    ("ctrl+shift+t", "reopen_closed"),
    ("ctrl+tab", "next_tab"),
    ("ctrl+shift+tab", "prev_tab"),
    ("ctrl+pagedown", "next_tab"),
    ("ctrl+pageup", "prev_tab"),
    ("ctrl+`", "toggle_terminal"),
    ("ctrl+alt+t", "toggle_terminal"),
    ("ctrl+alt+n", "insert_template"),
    ("ctrl+shift+`", "new_terminal_tab"),
    ("ctrl+\\", "split_right"),
    ("ctrl+r", "goto_symbol"),
    ("alt+l", "reveal_in_tree"),
    ("alt+g", "goto_line_select"),
    ("alt+minus", "nav_back"),
    ("alt+plus", "nav_forward"),
    ("alt+equals", "nav_forward"),
    ("alt+shift+equals", "nav_forward"),
    ("ctrl+alt+minus", "nav_back"),
    ("ctrl+shift+minus", "nav_forward"),
    ("ctrl+space", "complete"),
    ("f12", "goto_definition"),
    ("shift+f12", "find_references"),
    ("f2", "rename_symbol"),
    ("ctrl+f2", "toggle_bookmark"),
    ("f8", "next_bookmark"),
    ("shift+f8", "prev_bookmark"),
    ("ctrl+shift+f2", "clear_bookmarks"),
    ("ctrl+shift+v", "markdown_preview"),
    ("ctrl+shift+[", "fold"),
    ("ctrl+shift+]", "unfold"),
    ("ctrl+shift+m", "show_problems"),
    ("ctrl+shift+g", "show_git"),
    ("ctrl+alt+g", "git_history"),
    ("ctrl+alt+r", "pull_requests"),
    ("ctrl+alt+c", "claude"),
    ("ctrl+alt+k", "claude_ask"),
    ("ctrl+i", "claude_inline"),
    ("ctrl+alt+d", "toggle_inline_diff"),
    ("ctrl+period", "quick_fix"),
    ("ctrl+alt+f", "format_document"),
    ("ctrl+b", "git_blame"),
    ("ctrl+alt+b", "git_line_history"),
    ("ctrl+shift+e", "show_files"),
    ("ctrl+equals", "zoom_in"),
    ("ctrl+plus", "zoom_in"),
    ("ctrl+minus", "zoom_out"),
    ("ctrl+q", "quit"),
    ("ctrl+,", "open_settings"),
    ("escape", "escape"),
    ("ctrl+z", "undo"),
    ("ctrl+shift+z", "redo"),
    ("ctrl+y", "redo"),
    ("ctrl+a", "select_all"),
    ("ctrl+l", "select_line"),
    ("ctrl+d", "select_next"),
    ("ctrl+shift+l", "split_selection_into_lines"),
    ("ctrl+alt+up", "add_cursor_above"),
    ("ctrl+alt+down", "add_cursor_below"),
    ("alt+shift+up", "add_cursor_above"),
    ("alt+shift+down", "add_cursor_below"),
    ("ctrl+shift+d", "duplicate_line"),
    ("ctrl+shift+k", "delete_line"),
    ("ctrl+shift+up", "move_line_up"),
    ("ctrl+shift+down", "move_line_down"),
    ("ctrl+/", "toggle_comment"),
    ("ctrl+]", "indent"),
    ("ctrl+[", "unindent"),
    ("ctrl+j", "join_lines"),
    ("ctrl+enter", "insert_line_after"),
    ("ctrl+shift+enter", "insert_line_before"),
    ("ctrl+m", "goto_bracket"),
    ("ctrl+f9", "sort_lines"),
    ("ctrl+f6", "odoo_test_again"),
    ("ctrl+shift+f6", "odoo_test_here"),
    ("alt+f6", "odoo_debug_test_here"),
    // the debugger, like VS Code
    ("f5", "debug_start"),
    ("shift+f5", "debug_stop"),
    ("ctrl+shift+f5", "debug_restart"),
    ("f6", "debug_pause"),
    ("f9", "toggle_breakpoint"),
    ("f10", "debug_step_over"),
    ("f11", "debug_step_into"),
    ("shift+f11", "debug_step_out"),
    ("alt+f5", "git_next_change"),
    ("alt+shift+f5", "git_prev_change"),
    ("backspace", "backspace"),
    ("shift+backspace", "backspace"),
    ("delete", "delete"),
    ("ctrl+backspace", "delete_word_back"),
    ("ctrl+delete", "delete_word_forward"),
    ("enter", "newline"),
    ("shift+enter", "newline"),
    ("tab", "tab"),
    ("shift+tab", "unindent"),
    ("left", "move_left"),
    ("right", "move_right"),
    ("up", "move_up"),
    ("down", "move_down"),
    ("ctrl+left", "move_word_left"),
    ("ctrl+right", "move_word_right"),
    ("alt+left", "move_word_left"),
    ("alt+right", "move_word_right"),
    ("home", "move_home"),
    ("end", "move_end"),
    ("pageup", "move_page_up"),
    ("pagedown", "move_page_down"),
    ("ctrl+home", "move_doc_start"),
    ("ctrl+end", "move_doc_end"),
    ("shift+left", "select_left"),
    ("shift+right", "select_right"),
    ("shift+up", "select_up"),
    ("shift+down", "select_down"),
    ("ctrl+shift+left", "select_word_left"),
    ("ctrl+shift+right", "select_word_right"),
    ("shift+home", "select_home"),
    ("shift+end", "select_end"),
    ("shift+pageup", "select_page_up"),
    ("shift+pagedown", "select_page_down"),
    ("ctrl+shift+home", "select_doc_start"),
    ("ctrl+shift+end", "select_doc_end"),
];

/// Second key of `ctrl+k` chords.
const CHORDS: &[(&str, &str)] = &[
    ("ctrl+b", "toggle_sidebar"),
    ("ctrl+u", "upper_case"),
    ("ctrl+l", "lower_case"),
    ("ctrl+m", "toggle_minimap"),
    ("ctrl+0", "fold_all"),
    ("ctrl+z", "git_revert_change"),
    ("ctrl+shift+z", "git_revert_file"),
    ("v", "markdown_preview_side"),
    ("ctrl+j", "unfold_all"),
];

/// Canonical config name of a key ("up", "/", "a", "f3"…), readable by `Combo::parse`.
fn key_config_name(k: Key) -> String {
    use Key::*;
    match k {
        ArrowUp => "up".into(),
        ArrowDown => "down".into(),
        ArrowLeft => "left".into(),
        ArrowRight => "right".into(),
        PageUp => "pageup".into(),
        PageDown => "pagedown".into(),
        Slash => "/".into(),
        Backslash => "\\".into(),
        OpenBracket => "[".into(),
        CloseBracket => "]".into(),
        Backtick => "`".into(),
        Minus => "minus".into(),
        Equals => "equals".into(),
        Plus => "plus".into(),
        Comma => ",".into(),
        Period => ".".into(),
        Semicolon => ";".into(),
        other => other.name().to_lowercase(),
    }
}

impl Combo {
    /// "ctrl+alt+shift+key", the form written to settings.toml.
    pub fn to_config(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s += "ctrl+";
        }
        if self.alt {
            s += "alt+";
        }
        if self.shift {
            s += "shift+";
        }
        s + &key_config_name(self.key)
    }
}

/// A shortcut: one combo, or two for `ctrl+k` chords.
pub type Seq = Vec<Combo>;

pub fn parse_seq(s: &str) -> Option<Seq> {
    let v: Option<Seq> = s.split_whitespace().map(Combo::parse).collect();
    v.filter(|v| matches!(v.len(), 1 | 2))
}

pub fn seq_config(seq: &[Combo]) -> String {
    seq.iter().map(|c| c.to_config()).collect::<Vec<_>>().join(" ")
}

pub fn seq_label(seq: &[Combo]) -> String {
    seq.iter().map(|c| c.label()).collect::<Vec<_>>().join(" ")
}

/// Value of a [keys] entry that disables a combo (to replace or remove a default shortcut).
pub const UNBOUND: &str = "none";

pub struct Keymap {
    map: HashMap<Combo, Cmd>,
    chords: HashMap<Combo, Cmd>,
    chord_start: Combo,
    labels: HashMap<Cmd, String>,
}

impl Keymap {
    pub fn new(user: &HashMap<String, String>, plugin_keys: &[(String, Cmd)]) -> (Self, Vec<String>) {
        let chord_start = Combo::parse("ctrl+k").unwrap();
        let mut km = Keymap { map: HashMap::new(), chords: HashMap::new(), chord_start, labels: HashMap::new() };
        let mut errors = Vec::new();
        for (k, id) in DEFAULTS {
            km.bind(k, Some(commands::by_id(id).expect(id)), &mut errors);
        }
        for (k, id) in CHORDS {
            km.bind(&format!("ctrl+k {k}"), Some(commands::by_id(id).expect(id)), &mut errors);
        }
        for (k, cmd) in plugin_keys {
            km.bind(k, Some(*cmd), &mut errors);
        }
        // user entries: sorted so the result does not depend on HashMap order
        let mut user: Vec<(&String, &String)> = user.iter().collect();
        user.sort();
        for (k, id) in user {
            if id == UNBOUND {
                km.bind(k, None, &mut errors);
                continue;
            }
            match commands::by_id(id) {
                Some(cmd) => km.bind(k, Some(cmd), &mut errors),
                None => errors.push(tf!("Commande inconnue dans [keys] : {id}", "Unknown command in [keys]: {id}")),
            }
        }
        km.compute_labels();
        (km, errors)
    }

    /// Binds (or with `None` unbinds) a shortcut.
    fn bind(&mut self, keys: &str, cmd: Option<Cmd>, errors: &mut Vec<String>) {
        match parse_seq(keys).as_deref() {
            Some([c]) => match cmd {
                Some(cmd) => {
                    self.map.insert(*c, cmd);
                }
                None => {
                    self.map.remove(c);
                }
            },
            Some([a, b]) if *a == self.chord_start => match cmd {
                Some(cmd) => {
                    self.chords.insert(*b, cmd);
                }
                None => {
                    self.chords.remove(b);
                }
            },
            _ => errors.push(tf!("Raccourci invalide : {keys}", "Invalid shortcut: {keys}")),
        }
    }

    /// Label of each command: its shortest shortcut (stable choice).
    fn compute_labels(&mut self) {
        self.labels.clear();
        for seq in self.all_seqs() {
            let cmd = self.lookup_seq(&seq).unwrap();
            let l = seq_label(&seq);
            let better = match self.labels.get(&cmd) {
                Some(cur) => (l.len(), &l) < (cur.len(), cur),
                None => true,
            };
            if better {
                self.labels.insert(cmd, l);
            }
        }
    }

    fn all_seqs(&self) -> Vec<Seq> {
        let mut v: Vec<Seq> = self.map.keys().map(|c| vec![*c]).collect();
        v.extend(self.chords.keys().map(|c| vec![self.chord_start, *c]));
        v
    }

    /// Every shortcut currently bound to `cmd`.
    pub fn seqs_of(&self, cmd: Cmd) -> Vec<Seq> {
        let mut v: Vec<Seq> = self.all_seqs().into_iter().filter(|s| self.lookup_seq(s) == Some(cmd)).collect();
        v.sort_by_key(|s| seq_config(s));
        v
    }

    /// Exact lookup of a shortcut (no Shift tolerance).
    pub fn lookup_seq(&self, seq: &[Combo]) -> Option<Cmd> {
        match seq {
            [c] => self.map.get(c).copied(),
            [a, b] if *a == self.chord_start => self.chords.get(b).copied(),
            _ => None,
        }
    }

    /// Built-in shortcuts of `cmd`.
    pub fn default_seqs(cmd: Cmd) -> Vec<Seq> {
        let defaults = DEFAULTS.iter().map(|(k, id)| (k.to_string(), *id));
        let chords = CHORDS.iter().map(|(k, id)| (format!("ctrl+k {k}"), *id));
        defaults
            .chain(chords)
            .filter(|(_, id)| commands::by_id(id) == Some(cmd))
            .filter_map(|(k, _)| parse_seq(&k))
            .collect()
    }

    pub fn is_chord_start(&self, c: &Combo) -> bool {
        *c == self.chord_start
    }
    pub fn lookup(&self, c: Combo, in_chord: bool) -> Option<Cmd> {
        let table = if in_chord { &self.chords } else { &self.map };
        table.get(&c).copied().or_else(|| {
            // On layouts like AZERTY, symbols such as '/' need Shift: ignore it for symbol keys.
            let symbol = matches!(
                c.key,
                Key::Slash | Key::OpenBracket | Key::CloseBracket | Key::Backtick | Key::Plus | Key::Equals | Key::Minus
            );
            if c.shift && symbol {
                table.get(&Combo { shift: false, ..c }).copied()
            } else {
                None
            }
        })
    }

    pub fn label(&self, cmd: Cmd) -> String {
        self.labels.get(&cmd).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebind_and_unbind() {
        let mut user = HashMap::new();
        user.insert("ctrl+d".to_string(), UNBOUND.to_string());
        user.insert("ctrl+alt+shift+d".to_string(), "select_next".to_string());
        user.insert("ctrl+k ctrl+b".to_string(), UNBOUND.to_string());
        let (km, errs) = Keymap::new(&user, &[]);
        assert!(errs.is_empty(), "{errs:?}");
        let d = Combo::parse("ctrl+d").unwrap();
        assert_eq!(km.lookup(d, false), None);
        assert_eq!(km.label(Cmd::SelectNext), "Ctrl+Alt+Shift+D");
        assert_eq!(km.label(Cmd::ToggleSidebar), "");
        assert_eq!(Keymap::default_seqs(Cmd::SelectNext), vec![vec![d]]);
    }

    #[test]
    fn config_names_roundtrip() {
        for s in ["ctrl+shift+p", "alt+shift+up", "ctrl+/", "ctrl+equals", "f3", "ctrl+pagedown", "ctrl+k ctrl+b", "ctrl+`", "ctrl+,"] {
            let seq = parse_seq(s).unwrap();
            assert_eq!(parse_seq(&seq_config(&seq)), Some(seq), "{s}");
        }
    }

    #[test]
    fn navigation_and_git_shortcuts() {
        let (km, errs) = Keymap::new(&HashMap::new(), &[]);
        assert!(errs.is_empty(), "{errs:?}");
        let k = |s: &str| km.lookup_seq(&parse_seq(s).unwrap());
        assert_eq!(k("alt+minus"), Some(Cmd::NavBack));
        assert_eq!(k("alt+plus"), Some(Cmd::NavForward));
        assert_eq!(k("alt+shift+equals"), Some(Cmd::NavForward));
        assert_eq!(k("ctrl+k ctrl+z"), Some(Cmd::GitRevertChange));
        assert_eq!(k("ctrl+k ctrl+shift+z"), Some(Cmd::GitRevertFile));
        // plain Ctrl+Z is still undo
        assert_eq!(k("ctrl+z"), Some(Cmd::Undo));
    }
}
