use eframe::egui::{self, Color32, Rounding, Stroke};
use ropey::Rope;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use syntect::highlighting::{Color, HighlightIterator, HighlightState, Highlighter, Theme, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

#[derive(Clone, Copy)]
pub struct Colors {
    pub bg: Color32,
    pub fg: Color32,
    pub selection: Color32,
    pub line_hl: Color32,
    pub gutter_bg: Color32,
    pub gutter_fg: Color32,
    pub caret: Color32,
    pub find: Color32,
    pub guide: Color32,
    /// Interface accents (cursor color and find color of the theme).
    pub accent: Color32,
    pub accent2: Color32,
}

#[derive(Clone, Copy)]
pub struct TabColors {
    pub bar: Color32,
    pub hover: Color32,
    pub active: Color32,
    pub border: Color32,
    pub text_active: Color32,
    pub text_inactive: Color32,
    pub close_hover: Color32,
    pub accent: Color32,
    pub accent2: Color32,
}

pub struct Highlighting {
    pub ss: SyntaxSet,
    themes: BTreeMap<String, Theme>,
    pub theme: Theme,
    pub theme_name: String,
    pub colors: Colors,
    /// Bumped when the theme changes so cached spans get recomputed.
    pub generation: u64,
    /// Accent chosen in the preferences instead of the theme's.
    accent_override: Option<Color32>,
    /// 0 = the theme's text colors; up to ~0.4: text colors mixed with the background (lighter
    /// text on light themes, which egui renders heavier than other editors do).
    softness: f32,
}

/// Small preview of a theme for the preferences: background, text, accent, keyword / string / comment colors.
pub struct ThemePreview {
    pub bg: Color32,
    pub fg: Color32,
    pub accent: Color32,
    pub code: [Color32; 3],
}

/// "#RRGGBB" → color.
pub fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn to_hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

static LIGHT_UI: AtomicBool = AtomicBool::new(false);

/// Status colors (errors, warnings, additions), darker on light themes to stay readable.
pub fn red() -> Color32 {
    if LIGHT_UI.load(Ordering::Relaxed) { Color32::from_rgb(0xc9, 0x35, 0x45) } else { Color32::from_rgb(0xe0, 0x6c, 0x75) }
}

pub fn yellow() -> Color32 {
    if LIGHT_UI.load(Ordering::Relaxed) { Color32::from_rgb(0xa3, 0x68, 0x0a) } else { Color32::from_rgb(0xe5, 0xc0, 0x7b) }
}

pub fn green() -> Color32 {
    if LIGHT_UI.load(Ordering::Relaxed) { Color32::from_rgb(0x2e, 0x85, 0x40) } else { Color32::from_rgb(0x98, 0xc3, 0x79) }
}

pub fn blue() -> Color32 {
    if LIGHT_UI.load(Ordering::Relaxed) { Color32::from_rgb(0x3b, 0x5b, 0xdb) } else { Color32::from_rgb(0x7a, 0xa2, 0xff) }
}

fn col(c: Option<Color>, d: Color32) -> Color32 {
    c.map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)).unwrap_or(d)
}

impl Highlighting {
    pub fn new(theme_name: &str, extra_themes: &Path) -> Self {
        let ss = SyntaxSet::load_defaults_newlines();
        let mut ts = ThemeSet::load_defaults();
        for (name, src) in [
            ("Florm", include_str!("../assets/Florm.tmTheme")),
            ("Florm Light", include_str!("../assets/Florm Light.tmTheme")),
            ("Light+ (VS Code)", include_str!("../assets/Light+.tmTheme")),
        ] {
            if let Ok(t) = ThemeSet::load_from_reader(&mut std::io::Cursor::new(src)) {
                ts.themes.insert(name.into(), t);
            }
        }
        if extra_themes.is_dir() {
            let _ = ts.add_from_folder(extra_themes);
        }
        let mut h = Highlighting {
            ss,
            themes: ts.themes,
            theme: Theme::default(),
            theme_name: String::new(),
            colors: Colors {
                bg: Color32::BLACK,
                fg: Color32::WHITE,
                selection: Color32::GRAY,
                line_hl: Color32::BLACK,
                gutter_bg: Color32::BLACK,
                gutter_fg: Color32::GRAY,
                caret: Color32::WHITE,
                find: Color32::YELLOW,
                guide: Color32::DARK_GRAY,
                accent: Color32::LIGHT_BLUE,
                accent2: Color32::LIGHT_BLUE,
            },
            generation: 0,
            accent_override: None,
            softness: 0.0,
        };
        if !h.set_theme(theme_name) {
            h.set_theme("Florm");
        }
        h
    }

    pub fn theme_names(&self) -> Vec<String> {
        self.themes.keys().cloned().collect()
    }

    pub fn set_theme(&mut self, name: &str) -> bool {
        let Some(theme) = self.themes.get(name).cloned() else { return false };
        let s = &theme.settings;
        let bg = col(s.background, Color32::from_rgb(0x2b, 0x30, 0x3b));
        let fg = col(s.foreground, Color32::from_rgb(0xc0, 0xc5, 0xce));
        let light = bg.r() as u32 + bg.g() as u32 + bg.b() as u32 > 384;
        LIGHT_UI.store(light, Ordering::Relaxed);
        let shade = |amount: i16| {
            let f = |c: u8| (c as i16 + if light { -amount } else { amount }).clamp(0, 255) as u8;
            Color32::from_rgb(f(bg.r()), f(bg.g()), f(bg.b()))
        };
        let fg = mix(fg, bg, self.softness);
        self.colors = Colors {
            bg,
            fg,
            selection: col(s.selection, shade(40)),
            line_hl: col(s.line_highlight, shade(10)),
            gutter_bg: col(s.gutter, bg),
            gutter_fg: col(s.gutter_foreground, fg.gamma_multiply(0.45)),
            caret: col(s.caret, fg),
            find: col(s.find_highlight, Color32::from_rgb(0xe5, 0xc0, 0x7b)),
            guide: col(s.guide, shade(28)),
            // interface accent: the theme's "accent" when it has one, else its cursor color
            accent: col(s.accent, col(s.caret, fg)),
            accent2: col(s.find_highlight, col(s.accent, col(s.caret, fg))),
        };
        if let Some(a) = self.accent_override {
            self.colors.accent = a;
            self.colors.accent2 = a;
        }
        self.theme = theme;
        self.theme_name = name.to_string();
        self.generation += 1;
        true
    }

    /// Accent color and text softness from the preferences (applied to the current theme).
    pub fn set_overrides(&mut self, accent: Option<Color32>, softness: f32) {
        let softness = softness.clamp(0.0, 0.5);
        if self.accent_override == accent && self.softness == softness {
            return;
        }
        self.accent_override = accent;
        self.softness = softness;
        let name = self.theme_name.clone();
        self.set_theme(&name);
    }

    /// A syntax color with the text softness applied.
    fn code_color(&self, c: Color32) -> Color32 {
        if self.softness > 0.0 {
            mix(c, self.colors.bg, self.softness)
        } else {
            c
        }
    }

    pub fn preview(&self, name: &str) -> Option<ThemePreview> {
        let theme = self.themes.get(name)?;
        let s = &theme.settings;
        let bg = col(s.background, Color32::from_rgb(0x2b, 0x30, 0x3b));
        let fg = col(s.foreground, Color32::from_rgb(0xc0, 0xc5, 0xce));
        let h = Highlighter::new(theme);
        let scope = |n: &str| {
            let st = h.style_for_stack(&[syntect::parsing::Scope::new(n).unwrap()]);
            Color32::from_rgb(st.foreground.r, st.foreground.g, st.foreground.b)
        };
        Some(ThemePreview {
            bg,
            fg,
            accent: col(s.accent, col(s.caret, fg)),
            code: [scope("keyword.control"), scope("string.quoted"), scope("comment.line")],
        })
    }

    pub fn is_light(&self) -> bool {
        let bg = self.colors.bg;
        bg.r() as u32 + bg.g() as u32 + bg.b() as u32 > 384
    }

    /// Colors of the editor tab strip, chosen for readable contrast on dark and light themes.
    pub fn tab_colors(&self) -> TabColors {
        let c = self.colors;
        let light = self.is_light();
        let adj = |col: Color32, d: i16| {
            let f = |x: u8| (x as i16 + d).clamp(0, 255) as u8;
            Color32::from_rgb(f(col.r()), f(col.g()), f(col.b()))
        };
        let mix = |a: Color32, b: Color32, t: f32| {
            let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
            Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
        };
        let bar = self.visuals().panel_fill;
        TabColors {
            bar,
            hover: adj(c.bg, if light { -5 } else { 9 }),
            active: c.bg,
            border: adj(c.bg, if light { -40 } else { 16 }),
            text_active: if light { mix(c.fg, Color32::BLACK, 0.5) } else { mix(c.fg, Color32::WHITE, 0.5) },
            text_inactive: if light { mix(c.fg, Color32::BLACK, 0.1) } else { mix(c.fg, bar, 0.22) },
            close_hover: adj(c.bg, if light { -30 } else { 26 }),
            accent: c.accent,
            accent2: c.accent2,
        }
    }

    /// Interface colors (panels, buttons, selection) derived from the editor theme.
    pub fn visuals(&self) -> egui::Visuals {
        let c = self.colors;
        let light = c.bg.r() as u32 + c.bg.g() as u32 + c.bg.b() as u32 > 384;
        let shade = |amount: i16| {
            let f = |x: u8| (x as i16 + if light { -amount } else { amount }).clamp(0, 255) as u8;
            Color32::from_rgb(f(c.bg.r()), f(c.bg.g()), f(c.bg.b()))
        };
        let mix = |a: Color32, b: Color32, t: f32| {
            let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
            Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
        };
        let mut v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
        v.panel_fill = shade(-4);
        v.window_fill = shade(5);
        v.extreme_bg_color = shade(-9);
        v.faint_bg_color = shade(5);
        v.code_bg_color = shade(8);
        v.hyperlink_color = c.accent;
        v.selection.bg_fill = mix(c.bg, mix(c.accent, c.accent2, 0.6), 0.38);
        v.selection.stroke = Stroke::new(1.0_f32, mix(c.accent, c.bg, 0.15));
        v.window_stroke = Stroke::new(1.0_f32, shade(14));
        v.window_rounding = Rounding::same(12.0);
        v.menu_rounding = Rounding::same(10.0);
        let shadow = Color32::from_black_alpha(if light { 40 } else { 110 });
        v.window_shadow = egui::epaint::Shadow { offset: egui::vec2(0.0, 10.0), blur: 30.0, spread: 0.0, color: shadow };
        v.popup_shadow = egui::epaint::Shadow { offset: egui::vec2(0.0, 6.0), blur: 18.0, spread: 0.0, color: shadow };
        // UI text a bit brighter than code text, for readability on panels.
        // on light themes, pure black text looks bold with egui's rendering: a dark grey instead
        let text = if light { mix(mix(c.fg, Color32::BLACK, 0.25), c.bg, 0.18) } else { mix(c.fg, Color32::WHITE, 0.3) };
        v.widgets.noninteractive.bg_fill = v.panel_fill;
        v.widgets.noninteractive.weak_bg_fill = v.panel_fill;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, shade(8)); // separators: barely visible
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, text);
        // Ghost buttons: no fill or border until hovered.
        for (w, fill, weak) in [
            (&mut v.widgets.inactive, shade(12), Color32::TRANSPARENT),
            (&mut v.widgets.hovered, shade(18), shade(14)),
            (&mut v.widgets.active, shade(26), shade(22)),
            (&mut v.widgets.open, shade(14), shade(12)),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = weak;
            w.bg_stroke = Stroke::NONE;
            w.fg_stroke = Stroke::new(1.0_f32, text);
            w.rounding = Rounding::same(6.0);
            w.expansion = 0.0;
        }
        v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, mix(text, c.bg, 0.12));
        v
    }

    pub fn syntax_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.ss.syntaxes().iter().filter(|s| !s.hidden).map(|s| s.name.clone()).collect();
        v.sort();
        v
    }

    pub fn detect(&self, path: Option<&Path>, first_line: &str) -> String {
        let ext = path
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let alias = match ext.as_str() {
            "ts" | "tsx" | "mjs" | "cjs" | "jsx" => Some("JavaScript"),
            "vue" | "svelte" | "jinja" | "j2" | "jinja2" => Some("HTML"),
            "csproj" | "xaml" | "rng" | "kml" | "plist" | "xsl" => Some("XML"),
            "jsonc" | "json5" => Some("JSON"),
            "toml" | "ini" | "cfg" | "conf" | "env" => Some("Bourne Again Shell (bash)"),
            _ => None,
        };
        let syn = alias
            .and_then(|a| self.ss.find_syntax_by_name(a))
            .or_else(|| self.ss.find_syntax_by_extension(&ext))
            .or_else(|| {
                path.and_then(|p| p.file_name()).and_then(|n| n.to_str()).and_then(|n| self.ss.find_syntax_by_extension(n))
            })
            .or_else(|| self.ss.find_syntax_by_first_line(first_line))
            .unwrap_or_else(|| self.ss.find_syntax_plain_text());
        syn.name.clone()
    }
}

/// Incremental highlighting cache: parser state at the start of every line plus
/// colored byte spans for every line already highlighted.
#[derive(Default)]
pub struct HlCache {
    states: Vec<(ParseState, HighlightState)>,
    spans: Vec<Vec<(Color32, Range<usize>)>>,
    syntax: String,
    generation: u64,
}

impl HlCache {
    pub fn invalidate(&mut self, line: usize) {
        self.spans.truncate(line);
        self.states.truncate(line + 1);
    }

    pub fn spans(&self, line: usize) -> Option<&[(Color32, Range<usize>)]> {
        self.spans.get(line).map(|v| v.as_slice())
    }

    /// Highlights up to `upto` (inclusive), parsing at most `budget` lines.
    /// Returns false when the budget ran out (caller should repaint).
    pub fn ensure(&mut self, hl: &Highlighting, syntax: &str, rope: &Rope, upto: usize, budget: usize) -> bool {
        if self.syntax != syntax || self.generation != hl.generation {
            self.states.clear();
            self.spans.clear();
            self.syntax = syntax.to_string();
            self.generation = hl.generation;
        }
        if rope.len_bytes() > 30_000_000 {
            return true;
        }
        let upto = upto.min(rope.len_lines().saturating_sub(1));
        if self.spans.len() > upto {
            return true;
        }
        let syn = hl.ss.find_syntax_by_name(syntax).unwrap_or_else(|| hl.ss.find_syntax_plain_text());
        let highlighter = Highlighter::new(&hl.theme);
        if self.states.is_empty() {
            self.states.push((ParseState::new(syn), HighlightState::new(&highlighter, ScopeStack::new())));
        }
        let mut done = 0;
        while self.spans.len() <= upto {
            if done >= budget {
                return false;
            }
            let i = self.spans.len();
            let (mut ps, mut hs) = self.states[i].clone();
            let line = rope.line(i).to_string();
            let mut spans = Vec::new();
            // Very long lines (minified files) are not worth parsing.
            if line.len() <= 10_000 {
                let ops = ps.parse_line(&line, &hl.ss).unwrap_or_default();
                let mut off = 0;
                for (style, text) in HighlightIterator::new(&mut hs, &ops, &line, &highlighter) {
                    let f = style.foreground;
                    spans.push((hl.code_color(Color32::from_rgb(f.r, f.g, f.b)), off..off + text.len()));
                    off += text.len();
                }
            }
            self.spans.push(spans);
            self.states.push((ps, hs));
            done += 1;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_and_softer_text() {
        let mut hl = Highlighting::new("Light+ (VS Code)", Path::new("/nonexistent"));
        let (fg, accent) = (hl.colors.fg, hl.colors.accent);
        let red = parse_hex("#E5534B").unwrap();
        assert_eq!(to_hex(red), "#E5534B");
        assert!(parse_hex("E5534B").is_none() && parse_hex("#E553").is_none());
        hl.set_overrides(Some(red), 0.3);
        assert_eq!(hl.colors.accent, red);
        // softer text: closer to the white background
        assert!(hl.colors.fg.r() > fg.r());
        let gen = hl.generation;
        hl.set_overrides(None, 0.0);
        assert_eq!((hl.colors.fg, hl.colors.accent), (fg, accent));
        assert!(hl.generation > gen, "spans must be recomputed");
        let pv = hl.preview("Florm").unwrap();
        assert_ne!(pv.bg, pv.fg);
    }

    #[test]
    fn vscode_light_plus() {
        let hl = Highlighting::new("Light+ (VS Code)", Path::new("/nonexistent"));
        assert_eq!(hl.theme_name, "Light+ (VS Code)");
        assert!(hl.is_light());
        assert_eq!(hl.colors.caret, Color32::BLACK);
        assert_eq!(hl.colors.accent, Color32::from_rgb(0x00, 0x7a, 0xcc));
        // colors of a Python line, like in VS Code
        let rope = Rope::from_str("def f(x):  # note\n    return 'a'\n");
        let mut cache = HlCache::default();
        assert!(cache.ensure(&hl, "Python", &rope, 1, 100));
        let color_of = |line: usize, word: &str| {
            let text = rope.line(line).to_string();
            let at = text.find(word).unwrap();
            cache.spans(line).unwrap().iter().find(|(_, r)| r.contains(&at)).map(|(c, _)| *c).unwrap()
        };
        let rgb = |h: u32| Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8);
        assert_eq!(color_of(0, "def"), rgb(0x0000ff));
        assert_eq!(color_of(0, "f("), rgb(0x795e26));
        assert_eq!(color_of(0, "note"), rgb(0x008000));
        assert_eq!(color_of(1, "return"), rgb(0xaf00db));
        assert_eq!(color_of(1, "a'"), rgb(0xa31515));
    }
}
