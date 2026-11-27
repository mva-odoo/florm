use eframe::egui::{self, Color32, CornerRadius, Stroke};
use ropey::Rope;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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
    /// The active tab: the editor's background.
    pub active: Color32,
    /// The lines between the tabs and under the strip.
    pub border: Color32,
    pub text_active: Color32,
    pub text_inactive: Color32,
    pub close_hover: Color32,
    pub accent: Color32,
}

/// The surfaces of the interface, as Zed makes them from its editor's background (its theme One):
/// on a dark theme each one a step lighter than the editor, on a light theme a step darker.
#[derive(Clone, Copy)]
pub struct Surfaces {
    /// The text: the editor.
    pub editor: Color32,
    /// The panels, the tab strip, the menus and windows.
    pub surface: Color32,
    /// The title bar and the status bar.
    pub bar: Color32,
    /// The lines between the parts, the edges of the windows and fields.
    pub border: Color32,
    /// Fainter lines (separators inside a panel, indent guides).
    pub border_variant: Color32,
    pub hover: Color32,
    pub selected: Color32,
    /// The edge of what has the keyboard (a row of a list, a field).
    pub focused: Color32,
}

pub struct Highlighting {
    pub ss: Arc<SyntaxSet>,
    /// The languages prepared on a thread (true once done, see `ready`), and what redraws the
    /// window then. Without it (tests) every language is highlighted at once.
    warm: Arc<Mutex<HashMap<String, bool>>>,
    repaint: Option<egui::Context>,
    themes: BTreeMap<String, Theme>,
    pub theme: Theme,
    pub theme_name: String,
    pub colors: Colors,
    /// The color of each kind of semantic token in the theme (None: the syntax's color is kept).
    pub semantic: [Option<Color32>; crate::lsp::SEMANTIC_KINDS],
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
        // only the themes of Florm: its own ones, and others after the usual ones of VS Code and Zed
        let mut ts = ThemeSet::new();
        for (name, src) in [
            ("Florm", include_str!("../assets/Florm.tmTheme")),
            ("Florm Light", include_str!("../assets/Florm Light.tmTheme")),
            ("Florm Minimal", include_str!("../assets/Florm Minimal.tmTheme")),
            ("Florm Minimal Light", include_str!("../assets/Florm Minimal Light.tmTheme")),
            ("Florm Monokai", include_str!("../assets/Florm Monokai.tmTheme")),
            ("Florm One Dark", include_str!("../assets/Florm One Dark.tmTheme")),
            ("Florm Dracula", include_str!("../assets/Florm Dracula.tmTheme")),
            ("Florm GitHub Light", include_str!("../assets/Florm GitHub Light.tmTheme")),
            // the most installed themes of VS Code and Zed, with their own palettes
            ("Florm Catppuccin Mocha", include_str!("../assets/Florm Catppuccin Mocha.tmTheme")),
            ("Florm Catppuccin Latte", include_str!("../assets/Florm Catppuccin Latte.tmTheme")),
            ("Florm Tokyo Night", include_str!("../assets/Florm Tokyo Night.tmTheme")),
            ("Florm Gruvbox Dark", include_str!("../assets/Florm Gruvbox Dark.tmTheme")),
            ("Florm Gruvbox Light", include_str!("../assets/Florm Gruvbox Light.tmTheme")),
            ("Florm Nord", include_str!("../assets/Florm Nord.tmTheme")),
            ("Florm Solarized Dark", include_str!("../assets/Florm Solarized Dark.tmTheme")),
            ("Florm Solarized Light", include_str!("../assets/Florm Solarized Light.tmTheme")),
            ("Florm GitHub Dark", include_str!("../assets/Florm GitHub Dark.tmTheme")),
            ("Florm One Light", include_str!("../assets/Florm One Light.tmTheme")),
            ("Florm Rosé Pine", include_str!("../assets/Florm Rosé Pine.tmTheme")),
            ("Florm Ayu Dark", include_str!("../assets/Florm Ayu Dark.tmTheme")),
            ("Florm Night Owl", include_str!("../assets/Florm Night Owl.tmTheme")),
            ("Florm Everforest Dark", include_str!("../assets/Florm Everforest Dark.tmTheme")),
            ("Florm Kanagawa", include_str!("../assets/Florm Kanagawa.tmTheme")),
        ] {
            if let Ok(t) = ThemeSet::load_from_reader(&mut std::io::Cursor::new(src)) {
                ts.themes.insert(name.into(), t);
            }
        }
        if extra_themes.is_dir() {
            let _ = ts.add_from_folder(extra_themes);
        }
        let mut h = Highlighting {
            ss: Arc::new(ss),
            warm: Default::default(),
            repaint: None,
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
            semantic: [None; crate::lsp::SEMANTIC_KINDS],
        };
        if !h.set_theme(theme_name) {
            h.set_theme(replacement(theme_name));
        }
        h
    }

    /// Languages are prepared on a thread from now on, and the window redrawn when one is ready.
    pub fn prepare_on_thread(&mut self, ctx: egui::Context) {
        self.repaint = Some(ctx);
    }

    /// Whether `syntax` can be highlighted at once. syntect compiles the expressions of a language
    /// the first time they are used: 20 to 50 ms for the first lines, which the frame budget cannot
    /// cut (the first image of the window waited for it). The first lines of the file are parsed on
    /// a thread instead: the text is shown at once, its colors a moment later.
    fn ready(&self, syntax: &str, rope: &Rope) -> bool {
        let Some(ctx) = self.repaint.as_ref().filter(|_| syntax != "Plain Text") else { return true };
        let mut warm = self.warm.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(done) = warm.get(syntax) {
            return *done;
        }
        warm.insert(syntax.to_string(), false);
        /// Ready even if the parser panicked: highlighting then goes on (or fails) on the UI thread.
        struct Done(Arc<Mutex<HashMap<String, bool>>>, String, egui::Context);
        impl Drop for Done {
            fn drop(&mut self) {
                self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(std::mem::take(&mut self.1), true);
                self.2.request_repaint();
            }
        }
        let done = Done(self.warm.clone(), syntax.to_string(), ctx.clone());
        let (ss, rope) = (self.ss.clone(), rope.clone());
        std::thread::spawn(move || {
            let syn = ss.find_syntax_by_name(&done.1).unwrap_or_else(|| ss.find_syntax_plain_text());
            let mut ps = ParseState::new(syn);
            for line in rope.lines().take(200) {
                let _ = ps.parse_line(&line.to_string(), &ss);
            }
            drop(done);
        });
        false
    }

    /// Adds (or replaces) theme `name` from the text of a .tmTheme.
    pub fn add_theme_xml(&mut self, name: &str, xml: &str) -> Result<(), String> {
        let t = ThemeSet::load_from_reader(&mut std::io::Cursor::new(xml)).map_err(|e| e.to_string())?;
        self.themes.insert(name.to_string(), t);
        Ok(())
    }

    /// Adds (or replaces) the .tmTheme files of `dir`.
    pub fn add_folder(&mut self, dir: &Path) {
        if let Ok(ts) = ThemeSet::load_from_folder(dir) {
            self.themes.extend(ts.themes);
        }
    }

    pub fn remove_theme(&mut self, name: &str) {
        self.themes.remove(name);
    }

    /// Color and italic of `scope` (e.g. "comment.line") in the current theme.
    pub fn scope_style(&self, scope: &str) -> (Color32, bool) {
        let h = Highlighter::new(&self.theme);
        let Ok(sc) = syntect::parsing::Scope::new(scope) else { return (self.colors.fg, false) };
        let st = h.style_for_stack(&[sc]);
        (Color32::from_rgb(st.foreground.r, st.foreground.g, st.foreground.b), st.font_style.contains(syntect::highlighting::FontStyle::ITALIC))
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
        let fg = mix(fg, bg, self.soft(light));
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
        // the semantic tokens take the colors the theme gives these scopes (as VS Code maps them)
        const SCOPES: [&[&str]; crate::lsp::SEMANTIC_KINDS] = [
            &["variable.parameter"],
            &["variable.other.property", "support.type.property-name", "variable.other.member"],
            &["entity.name.function", "support.function"],
            &["entity.name.type", "entity.name.class", "support.class"],
            &["entity.name.namespace", "entity.name.type.module", "entity.name.type"],
            &["variable.other.constant", "constant.other", "constant.language"],
            &["entity.name.function.macro", "support.function"],
            &["entity.name.function.decorator", "meta.decorator", "entity.name.function"],
            &["variable.language"],
        ];
        // the theme's own text color (before the softness): a scope with no rule of its own gets it
        let fg = self.scope_style("florm.no.rule").0;
        for (k, scopes) in SCOPES.iter().enumerate() {
            self.semantic[k] = scopes.iter().map(|s| self.scope_style(s).0).find(|c| *c != fg).map(|c| self.code_color(c));
        }
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
        let soft = self.soft(self.is_light());
        if soft > 0.0 {
            mix(c, self.colors.bg, soft)
        } else {
            c
        }
    }

    /// How much the code text goes toward the background: the preference, plus a step on light
    /// themes, where dark text looks bold (Zed's code is a grey, #383a42, not a near-black).
    fn soft(&self, light: bool) -> f32 {
        self.softness + if light { 0.14 } else { 0.0 }
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

    /// The surfaces around the editor, after Zed's theme One (offsets from the editor's background:
    /// One Dark #282c33 → #2f343e panels, #3b414d bars, #464b57 borders; One Light the other way).
    pub fn surfaces(&self) -> Surfaces {
        let c = self.colors;
        let light = self.is_light();
        let step = |dark: i16, light_d: i16| {
            let d = if light { -light_d } else { dark };
            let f = |x: u8| (x as i16 + d).clamp(0, 255) as u8;
            Color32::from_rgb(f(c.bg.r()), f(c.bg.g()), f(c.bg.b()))
        };
        let mix = |a: Color32, b: Color32, t: f32| {
            let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
            Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
        };
        Surfaces {
            editor: c.bg,
            // Zed One Light: #ebebec beside a white editor
            surface: step(7, 20),
            bar: step(19, 30),
            border: step(30, 49),
            border_variant: step(14, 27),
            hover: step(14, 27),
            selected: step(29, 48),
            focused: mix(c.accent, c.bg, if light { 0.15 } else { 0.35 }),
        }
    }

    /// The background of the bottom panel (terminal, debugger, problems, output): the editor's,
    /// a touch toward the bars (an off-white on a light theme, as Zed's terminal).
    pub fn bottom_paper(&self) -> Color32 {
        let (bg, bar) = (self.colors.bg, self.surfaces().surface);
        let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * 0.35) as u8;
        Color32::from_rgb(f(bg.r(), bar.r()), f(bg.g(), bar.g()), f(bg.b(), bar.b()))
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
        // as in Zed: the strip and the other tabs on the panels' surface, the active one on the
        // editor's background, lines between them
        let s = self.surfaces();
        // the bar at the top a step darker than the panels, as the status bar at the bottom
        let bar = s.bar;
        let _ = adj;
        TabColors {
            bar,
            active: c.bg,
            border: s.border,
            text_active: if light { mix(c.fg, Color32::BLACK, 0.3) } else { mix(c.fg, Color32::WHITE, 0.4) },
            text_inactive: if light { mix(c.fg, bar, 0.3) } else { mix(c.fg, bar, 0.3) },
            close_hover: s.hover,
            accent: c.accent,
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
        // Zed's surfaces: the panels, menus and windows a step from the editor, fields on the
        // editor's background, lines in its border color
        let s = self.surfaces();
        let _ = shade;
        v.panel_fill = s.surface;
        v.window_fill = s.surface;
        v.extreme_bg_color = s.editor;
        v.faint_bg_color = s.hover;
        v.code_bg_color = s.border_variant;
        v.hyperlink_color = c.accent;
        // the selection: the accent, faint (Zed's 0x3d over the background)
        v.selection.bg_fill = mix(c.bg, c.accent, 0.26);
        v.selection.stroke = Stroke::new(1.0_f32, s.focused);
        v.window_stroke = Stroke::new(1.0_f32, s.border);
        v.window_corner_radius = CornerRadius::same(8);
        v.menu_corner_radius = CornerRadius::same(6);
        let shadow = Color32::from_black_alpha(if light { 22 } else { 80 });
        v.window_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 20, spread: 0, color: shadow };
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 3], blur: 10, spread: 0, color: shadow };
        // UI text a bit brighter than code text, for readability on panels.
        // on light themes, pure black text looks bold with egui's rendering: a dark grey instead
        // the text softness already lightens c.fg: no more fading on top, or menus and lists turn pale
        // UI text: on light themes the theme's text color with only the softness of the
        // preferences (the code is a bit lighter still), else panels and lists on white lack
        // contrast; on dark themes a bit brighter than the code
        let text = if light {
            let (t, k) = (self.soft(true), self.softness);
            let f = |fg: u8, bg: u8| (bg as f32 + (fg as f32 - bg as f32) * (1.0 - k) / (1.0 - t)).clamp(0.0, 255.0) as u8;
            Color32::from_rgb(f(c.fg.r(), c.bg.r()), f(c.fg.g(), c.bg.g()), f(c.fg.b(), c.bg.b()))
        } else {
            mix(c.fg, Color32::WHITE, 0.3)
        };
        v.widgets.noninteractive.bg_fill = v.panel_fill;
        v.widgets.noninteractive.weak_bg_fill = v.panel_fill;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, s.border); // separators: Zed's lines
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, text);
        // Ghost buttons: no fill or border until hovered.
        for (w, fill, weak) in [
            (&mut v.widgets.inactive, s.hover, Color32::TRANSPARENT),
            (&mut v.widgets.hovered, s.hover, s.hover),
            (&mut v.widgets.active, s.selected, s.selected),
            (&mut v.widgets.open, s.hover, s.hover),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = weak;
            w.bg_stroke = Stroke::NONE;
            w.fg_stroke = Stroke::new(1.0_f32, text);
            w.corner_radius = CornerRadius::same(4);
            w.expansion = 0.0;
        }
        if !light {
            v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, mix(text, c.bg, (0.12 - self.soft(light) / 2.0).max(0.0)));
        }
        v
    }

    /// Colors of one line taken alone (search results): (color, byte range) spans.
    pub fn line_spans(&self, syntax: &str, line: &str) -> Vec<(Color32, Range<usize>)> {
        if line.len() > 2_000 {
            return Vec::new();
        }
        let syn = self.ss.find_syntax_by_name(syntax).unwrap_or_else(|| self.ss.find_syntax_plain_text());
        let highlighter = Highlighter::new(&self.theme);
        let mut ps = ParseState::new(syn);
        let mut hs = HighlightState::new(&highlighter, ScopeStack::new());
        let text = format!("{line}\n");
        let ops = ps.parse_line(&text, &self.ss).unwrap_or_default();
        let mut off = 0;
        let mut spans = Vec::new();
        for (style, t) in HighlightIterator::new(&mut hs, &ops, &text, &highlighter) {
            let f = style.foreground;
            let end = (off + t.len()).min(line.len());
            if off < end {
                spans.push((self.code_color(Color32::from_rgb(f.r, f.g, f.b)), off..end));
            }
            off += t.len();
        }
        spans
    }

    /// Colors of a block of code whose language is given as in Markdown (```python, ```sh, ```rs…):
    /// (color, byte range) spans over the whole text, its lines parsed one after the other (a
    /// string over several lines stays a string). None: language unknown or not given.
    pub fn code_spans(&self, lang: &str, code: &str) -> Option<Vec<(Color32, Range<usize>)>> {
        let lang = lang.split([' ', ',', '{']).next().unwrap_or("").trim().to_lowercase();
        let name = match lang.as_str() {
            "" | "text" | "txt" | "plain" | "plaintext" | "output" => return None,
            "sh" | "shell" | "console" | "zsh" | "fish" | "bash" | "terminal" | "toml" | "ini" | "env" => "Bourne Again Shell (bash)",
            "ts" | "typescript" | "tsx" | "jsx" | "mjs" | "javascript" | "js" => "JavaScript",
            "vue" | "svelte" | "jinja" | "jinja2" | "j2" | "html" => "HTML",
            "jsonc" | "json5" | "json" => "JSON",
            "py" | "python" | "python3" => "Python",
            "xml" | "qweb" | "svg" => "XML",
            _ => "",
        };
        let syn = if name.is_empty() { self.ss.find_syntax_by_token(&lang)? } else { self.ss.find_syntax_by_name(name)? };
        if code.len() > 200_000 {
            return None;
        }
        let highlighter = Highlighter::new(&self.theme);
        let mut ps = ParseState::new(syn);
        let mut hs = HighlightState::new(&highlighter, ScopeStack::new());
        let mut spans = Vec::new();
        let mut start = 0;
        for line in code.split_inclusive('\n') {
            // syntect wants the end of line
            let owned;
            let text = if line.ends_with('\n') {
                line
            } else {
                owned = format!("{line}\n");
                &owned
            };
            let ops = ps.parse_line(text, &self.ss).ok()?;
            let mut off = start;
            for (style, t) in HighlightIterator::new(&mut hs, &ops, text, &highlighter) {
                let f = style.foreground;
                let end = (off + t.len()).min(start + line.len());
                if off < end {
                    spans.push((self.code_color(Color32::from_rgb(f.r, f.g, f.b)), off..end));
                }
                off += t.len();
            }
            start += line.len();
        }
        Some(spans)
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

/// Lines between two kept parser states: a state is ~4 KB, one per line was 30 MB for a file
/// of 7000 lines.
const CHECKPOINT: usize = 16;

/// Time given to highlighting in a frame: the rest comes in the next frames.
pub const FRAME_BUDGET: Duration = Duration::from_millis(6);

type State = (ParseState, HighlightState);
type Spans = Vec<(Color32, Range<usize>)>;

/// One highlighted line: its colored byte spans, and a hash of its text (to know, after an edit
/// above it, whether it is still the same line).
#[derive(Default)]
struct Line {
    hash: u64,
    spans: Spans,
}

fn line_hash(rope: &Rope, line: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for c in rope.line(line).chunks() {
        c.hash(&mut h);
    }
    h.finish()
}

/// The lines that were after an edit, kept aside: when the parser reaches one of their states
/// again (same state, same text after it), the rest is taken back as it was instead of being
/// parsed again. Typing a letter costs one line, not every line down to the bottom of the screen.
struct Tail {
    /// The line of the document `lines[0]` was expected at when the document had `nlines` lines.
    base: usize,
    nlines: usize,
    lines: Vec<Line>,
    /// States at the start of `lines[k]` (k may be `lines.len()`: the state after the last one).
    states: Vec<(usize, State)>,
}

/// Incremental highlighting cache: parser states every `CHECKPOINT` lines or so, plus colored
/// byte spans for every line already highlighted.
#[derive(Default)]
pub struct HlCache {
    /// States at the start of some lines (line 0 first, ascending, one every ~`CHECKPOINT`).
    states: Vec<(usize, State)>,
    lines: Vec<Line>,
    /// State at the start of line `lines.len()`, where highlighting goes on (None: from the last
    /// state of `states`, which is then at `lines.len()`).
    next: Option<State>,
    /// The line last changed and its state when known: typing on it again starts from there,
    /// not from the checkpoint before it.
    hot: Option<(usize, Option<State>)>,
    tail: Option<Tail>,
    /// Lines of the document when it was last highlighted.
    nlines: usize,
    /// Lines parsed so far (the tests check that an edit does not parse everything again).
    #[cfg(test)]
    parsed: usize,
    syntax: String,
    generation: u64,
    /// The syntax tree, for the languages with a grammar (tree-sitter): the colors come from it
    /// instead of the regexes, line by line for the lines shown.
    ts: Option<crate::syntax_tree::SyntaxTree>,
    /// Colors read from the tree, per line (None: to read).
    ts_lines: Vec<Option<Spans>>,
    /// The first line changed since the colors were read.
    ts_dirty: Option<usize>,
    /// The edits since the last parse were told to the tree (else: parsed from scratch).
    ts_told: bool,
    ts_untold: bool,
    /// Colors of the scopes of the grammar in the theme.
    ts_colors: std::collections::HashMap<&'static str, Color32>,
    /// The first line shown (set by the editor): the tree's colors are read from there.
    pub view_from: usize,
    /// The regexes even when the language has a grammar (the tests of the regex highlighting).
    pub regex_only: bool,
    /// The extension of the file (set by the editor): it chooses the grammar (TypeScript and
    /// JavaScript share a syntax, TOML has none of its own).
    pub ext: String,
}

impl HlCache {
    /// The syntax tree, when the language has a grammar (as last parsed).
    pub fn tree(&self) -> Option<&tree_sitter::Tree> {
        self.ts.as_ref().and_then(|t| t.tree())
    }

    /// The text is about to change (`syntax_tree::input_edit`): told to the tree, if any.
    pub fn tree_edit(&mut self, e: tree_sitter::InputEdit) {
        if let Some(t) = self.ts.as_mut() {
            t.edit(e);
            self.ts_told = true;
        }
    }

    /// The colors come from a syntax tree.
    pub fn uses_tree(&self) -> bool {
        self.ts.is_some()
    }

    pub fn invalidate(&mut self, line: usize) {
        if self.ts.is_some() {
            self.ts_dirty = Some(self.ts_dirty.map_or(line, |d| d.min(line)));
            // changed without its edits told (undo restores a whole text): parsed from scratch
            if !self.ts_told {
                self.ts_untold = true;
            }
            self.ts_told = false;
            return;
        }
        if line >= self.lines.len() {
            // not highlighted yet: only the state kept for a line after it is no longer right
            // (the lines kept aside are checked against the text when they are taken back)
            if self.hot.as_ref().is_some_and(|(l, _)| *l > line) {
                self.hot = None;
            }
            return;
        }
        let end = self.next.take();
        let old_len = self.lines.len();
        let hot = match self.hot.take() {
            Some((l, Some(st))) if l == line => Some(st),
            _ => None,
        };
        let from = match hot {
            Some(st) => {
                self.next = Some(st.clone());
                self.hot = Some((line, Some(st)));
                line
            }
            None => {
                self.hot = Some((line, None));
                self.states.iter().rev().find(|(l, _)| *l <= line).map_or(0, |(l, _)| *l)
            }
        };
        // the lines from `from` and the states after `line` are set aside: they may still be right
        // further down (the state at `line` only depends on the lines above it)
        let keep = self.states.partition_point(|(l, _)| *l <= line);
        let mut states: Vec<(usize, State)> = self.states.drain(keep..).map(|(l, st)| (l - from, st)).collect();
        let mut lines: Vec<Line> = self.lines.drain(from..).collect();
        if let Some(st) = end {
            states.push((old_len - from, st));
        }
        match self.tail.take() {
            // lines set aside by an edit just before, not highlighted again since: one after the other
            Some(mut t) if t.base == old_len => {
                let n = lines.len();
                states.extend(t.states.drain(..).map(|(k, st)| (k + n, st)));
                lines.append(&mut t.lines);
                self.tail = Some(Tail { base: from, nlines: t.nlines, lines, states });
            }
            _ => self.tail = Some(Tail { base: from, nlines: self.nlines, lines, states }),
        }
    }

    pub fn spans(&self, line: usize) -> Option<&[(Color32, Range<usize>)]> {
        if self.ts.is_some() {
            return self.ts_lines.get(line).and_then(|l| l.as_deref());
        }
        self.lines.get(line).map(|l| l.spans.as_slice())
    }

    /// With a syntax tree: the colors of some lines out of the ones shown (the pinned lines at
    /// the top of the editor), after `ensure` (the tree is in sync).
    pub fn ensure_lines(&mut self, hl: &Highlighting, rope: &Rope, lines: &[usize]) {
        let Some(ts) = self.ts.as_ref() else { return };
        if ts.tree().is_none_or(|t| t.root_node().end_byte() != rope.len_bytes()) {
            return;
        }
        let colors = &self.ts_colors;
        let color = |s: &'static str| -> Color32 { colors.get(s).copied().unwrap_or(hl.colors.fg) };
        for &l in lines {
            if l >= rope.len_lines() || self.ts_lines.get(l).is_some_and(|x| x.is_some()) {
                continue;
            }
            if self.ts_lines.len() <= l {
                self.ts_lines.resize(l + 1, None);
            }
            self.ts_lines[l] = ts.line_spans(rope, l..l + 1, &color).into_iter().next();
        }
    }

    /// `ensure` with a syntax tree: parsed again if the text changed, the colors of the lines
    /// from `view_from` to `upto` read from it.
    fn ensure_tree(&mut self, hl: &Highlighting, rope: &Rope, upto: usize) -> bool {
        let Some(ts) = self.ts.as_mut() else { return true };
        if std::mem::take(&mut self.ts_untold) {
            if let Some(g) = crate::syntax_tree::grammar_for(ts.name()) {
                if let Some(fresh) = crate::syntax_tree::SyntaxTree::new(g) {
                    *ts = fresh;
                }
            }
            self.ts_lines.clear();
        }
        if ts.sync(rope).is_none() {
            // the first parse of a long file is still running: asked again at the next frame
            return !ts.parsing();
        }
        let changed = ts.changed_from.take();
        let from = match (changed, self.ts_dirty.take()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if let Some(f) = from {
            self.ts_lines.truncate(f);
        }
        let nlines = rope.len_lines();
        let upto = upto.min(nlines.saturating_sub(1));
        if self.ts_lines.len() <= upto {
            self.ts_lines.resize(upto + 1, None);
        }
        // the lines to read: from the first one shown not read yet, at most 2000 at a time
        let start = self.view_from.min(upto);
        let Some(a) = (start..=upto).find(|&l| self.ts_lines[l].is_none()) else { return true };
        let b = (a..=upto).rev().find(|&l| self.ts_lines[l].is_none()).unwrap_or(a).min(a + 2000);
        let ts_grammar = self.ts.as_ref().map(|t| t.grammar());
        let colors = &mut self.ts_colors;
        // the colors of the scopes, once per theme
        if colors.is_empty() {
            for s in ts_grammar.map(|g| g.scopes()).unwrap_or_default() {
                colors.insert(s, hl.code_color(hl.scope_style(s).0));
            }
        }
        let colors = &self.ts_colors;
        let color = |s: &'static str| -> Color32 { colors.get(s).copied().unwrap_or(hl.colors.fg) };
        let spans = self.ts.as_ref().map(|t| t.line_spans(rope, a..b + 1, &color)).unwrap_or_default();
        for (k, sp) in spans.into_iter().enumerate() {
            self.ts_lines[a + k] = Some(sp);
        }
        (start..=upto).all(|l| self.ts_lines[l].is_some())
    }

    /// Highlights up to `upto` (inclusive), for at most `budget` of time (the UI thread: a frame
    /// must not freeze on a long file). Returns false when the budget ran out (caller should repaint).
    pub fn ensure(&mut self, hl: &Highlighting, syntax: &str, rope: &Rope, upto: usize, budget: Duration) -> bool {
        let grammar = if rope.len_bytes() <= crate::syntax_tree::MAX_BYTES && !self.regex_only { crate::syntax_tree::grammar_for_file(syntax, &self.ext) } else { None };
        if self.syntax != syntax || self.generation != hl.generation || grammar.map(|g| g.name) != self.ts.as_ref().map(|t| t.name()) {
            let (view_from, regex_only, ext) = (self.view_from, self.regex_only, std::mem::take(&mut self.ext));
            *self = HlCache { syntax: syntax.to_string(), generation: hl.generation, view_from, regex_only, ext, ..Default::default() };
            self.ts = grammar.and_then(crate::syntax_tree::SyntaxTree::new);
        }
        if self.ts.is_some() {
            return self.ensure_tree(hl, rope, upto);
        }
        if rope.len_bytes() > 30_000_000 || !hl.ready(syntax, rope) {
            return true;
        }
        let nlines = rope.len_lines();
        let upto = upto.min(nlines.saturating_sub(1));
        if self.lines.len() > upto {
            return true;
        }
        let syn = hl.ss.find_syntax_by_name(syntax).unwrap_or_else(|| hl.ss.find_syntax_plain_text());
        let highlighter = Highlighter::new(&hl.theme);
        if self.states.is_empty() {
            self.lines.clear();
            self.states.push((0, (ParseState::new(syn), HighlightState::new(&highlighter, ScopeStack::new()))));
        }
        let (mut ps, mut hs) = match self.next.take() {
            Some(st) => st,
            None => {
                // the last state is where highlighting goes on (lines after it, if any, dropped)
                let (l, st) = self.states.last().expect("state of line 0");
                self.lines.truncate(*l);
                st.clone()
            }
        };
        // lines set aside: the line `k` of the tail is expected at the line `k + shift` now
        let shift = self.tail.as_ref().map_or(0, |t| t.base as isize + nlines as isize - t.nlines as isize);
        let mut taken = 0usize;
        let deadline = Instant::now().checked_add(budget);
        let mut done = 0usize;
        while self.lines.len() <= upto {
            // the clock read every few lines only
            if done.is_multiple_of(32) && done > 0 && deadline.is_some_and(|d| Instant::now() >= d) {
                break;
            }
            let i = self.lines.len();
            if let Some(n) = self.take_back(rope, i, shift, &mut ps, &mut hs) {
                taken = (i as isize - shift) as usize + n;
                done += n;
                continue;
            }
            if self.states.last().is_none_or(|(l, _)| i >= l + CHECKPOINT) {
                self.states.push((i, (ps.clone(), hs.clone())));
            }
            if let Some((l, st @ None)) = &mut self.hot {
                if *l == i {
                    *st = Some((ps.clone(), hs.clone()));
                }
            }
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
            spans.shrink_to_fit();
            self.lines.push(Line { hash: line_hash(rope, i), spans });
            #[cfg(test)]
            {
                self.parsed += 1;
            }
            done += 1;
        }
        self.next = Some((ps, hs));
        self.nlines = nlines;
        // what is left of the lines set aside: from where they were last taken back (now known to
        // be the next lines), else as they were; dropped once nothing of them is ahead
        if let Some(mut t) = self.tail.take() {
            if taken > 0 {
                t.lines.drain(..taken);
                t.states.retain(|(k, _)| *k >= taken);
                t.states.iter_mut().for_each(|(k, _)| *k -= taken);
                t.base = (taken as isize + shift) as usize;
                t.nlines = nlines;
            }
            let ahead = t.base + t.lines.len() + nlines > t.nlines + self.lines.len();
            if ahead && !t.lines.is_empty() {
                self.tail = Some(t);
            }
        }
        self.lines.len() > upto
    }

    /// Line `i` is about to be highlighted from the state `ps`, `hs`: when the lines set aside have
    /// that same state at that place, with the same text up to their next state, they are taken back
    /// (and the state moved to theirs). Returns how many.
    fn take_back(&mut self, rope: &Rope, i: usize, shift: isize, ps: &mut ParseState, hs: &mut HighlightState) -> Option<usize> {
        let t = self.tail.as_mut()?;
        let k = usize::try_from(i as isize - shift).ok()?;
        let at = t.states.binary_search_by_key(&k, |(k, _)| *k).ok()?;
        let ((_, st), (end, next)) = (&t.states[at], t.states.get(at + 1)?);
        if (&st.0, &st.1) != (&*ps, &*hs) {
            return None;
        }
        let n = end - k;
        if i + n > rope.len_lines() || (0..n).any(|d| t.lines[k + d].hash != line_hash(rope, i + d)) {
            return None;
        }
        (*ps, *hs) = next.clone();
        self.lines.extend(t.lines[k..k + n].iter_mut().map(std::mem::take));
        self.states.push((i + n, next.clone()));
        Some(n)
    }
}

/// The theme of Florm taking the place of one that is no longer built in (or was removed).
fn replacement(old: &str) -> &'static str {
    match old {
        "Light+ (VS Code)" | "InspiredGitHub" => "Florm GitHub Light",
        _ if old.to_lowercase().contains("solarized") && old.to_lowercase().contains("light") => "Florm Solarized Light",
        _ if old.to_lowercase().contains("solarized") => "Florm Solarized Dark",
        _ if old.to_lowercase().contains("light") => "Florm Light",
        _ if old.to_lowercase().contains("monokai") => "Florm Monokai",
        _ => "Florm",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_and_softer_text() {
        let mut hl = Highlighting::new("Florm GitHub Light", Path::new("/nonexistent"));
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
    fn florm_themes_only() {
        let hl = Highlighting::new("Florm Monokai", Path::new("/nonexistent"));
        let mut names = hl.theme_names();
        names.sort();
        assert_eq!(names.len(), 23);
        assert!(names.contains(&"Florm Catppuccin Mocha".to_string()) && names.contains(&"Florm Tokyo Night".to_string()));
        assert!(!hl.is_light());
        // the sober ones: the light one light
        let mut min = Highlighting::new("Florm Minimal Light", Path::new("/nonexistent"));
        assert!(min.is_light() && min.set_theme("Florm Minimal") && !min.is_light());
        // the old built-in themes give way to the closest one
        assert_eq!(Highlighting::new("Light+ (VS Code)", Path::new("/nonexistent")).theme_name, "Florm GitHub Light");
        assert_eq!(Highlighting::new("Solarized (light)", Path::new("/nonexistent")).theme_name, "Florm Solarized Light");
        assert_eq!(Highlighting::new("Solarized (dark)", Path::new("/nonexistent")).theme_name, "Florm Solarized Dark");
        assert_eq!(Highlighting::new("base16-ocean.dark", Path::new("/nonexistent")).theme_name, "Florm");
    }

    #[test]
    fn github_light() {
        let hl = Highlighting::new("Florm GitHub Light", Path::new("/nonexistent"));
        assert_eq!(hl.theme_name, "Florm GitHub Light");
        assert!(hl.is_light());
        assert_eq!(hl.colors.accent, Color32::from_rgb(0x09, 0x69, 0xda));
        // colors of a Python line, like in VS Code
        let rope = Rope::from_str("def f(x):  # note\n    return 'a'\n");
        let mut cache = HlCache { regex_only: true, ..Default::default() };
        assert!(cache.ensure(&hl, "Python", &rope, 1, Duration::MAX));
        let color_of = |line: usize, word: &str| {
            let text = rope.line(line).to_string();
            let at = text.find(word).unwrap();
            cache.spans(line).unwrap().iter().find(|(_, r)| r.contains(&at)).map(|(c, _)| *c).unwrap()
        };
        // the theme's colors, a step toward the background on a light theme (code_color)
        let rgb = |h: u32| hl.code_color(Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8));
        assert_eq!(color_of(0, "def"), rgb(0xcf222e));
        assert_eq!(color_of(0, "f("), rgb(0x8250df));
        assert_eq!(color_of(0, "note"), rgb(0x6e7781));
        assert_eq!(color_of(1, "return"), rgb(0xcf222e));
        assert_eq!(color_of(1, "a'"), rgb(0x0a3069));
        // a line alone (search results) gets the same colors
        let spans = hl.line_spans("Python", "return 'a'");
        let at = |i: usize| spans.iter().find(|(_, r)| r.contains(&i)).map(|(c, _)| *c).unwrap();
        assert_eq!(at(0), rgb(0xcf222e));
        assert_eq!(at(8), rgb(0x0a3069));
        assert_eq!(spans.last().unwrap().1.end, "return 'a'".len());
        // a block of an answer of Claude: its language as in Markdown, lines parsed together
        let code = "x = \"\"\"un\ndeux\"\"\"\nreturn x";
        let spans = hl.code_spans("py", code).unwrap();
        let at = |i: usize| spans.iter().find(|(_, r)| r.contains(&i)).map(|(c, _)| *c).unwrap();
        assert_eq!(at(code.find("deux").unwrap()), rgb(0x0a3069));
        assert_eq!(at(code.find("return").unwrap()), rgb(0xcf222e));
        assert_eq!(spans.last().unwrap().1.end, code.len());
        assert!(hl.code_spans("sh", "ls -la").is_some() && hl.code_spans("rust", "fn f() {}").is_some());
        assert!(hl.code_spans("", "x").is_none() && hl.code_spans("text", "x").is_none() && hl.code_spans("nope", "x").is_none());
    }

    #[test]
    fn cache_after_edits_matches_a_fresh_highlight() {
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let text: String = (0..200).map(|i| format!("def f{i}(x):\n    '''doc {i}'''\n    return x + {i}  # c\n")).collect();
        let mut rope = Rope::from_str(&text);
        let mut cache = HlCache { regex_only: true, ..Default::default() };
        assert!(cache.ensure(&hl, "Python", &rope, usize::MAX, Duration::MAX));
        // (line, text inserted at its start, lines removed after it): typing on one line, a quote
        // that changes all the lines after it, lines added and removed
        let edits = [(100, "a", 0), (100, "b", 0), (100, "\"", 0), (100, "\"\"", 0), (37, "\n\n", 0), (38, "", 3), (5, "'''", 0), (5, "x", 0), (590, "z", 0)];
        for (line, ins, remove) in edits {
            let at = rope.line_to_char(line);
            rope.insert(at, ins);
            for _ in 0..remove {
                let (a, b) = (rope.line_to_char(line + 1), rope.line_to_char(line + 2));
                rope.remove(a..b);
            }
            cache.invalidate(line);
            assert!(cache.ensure(&hl, "Python", &rope, usize::MAX, Duration::MAX));
            let mut fresh = HlCache { regex_only: true, ..Default::default() };
            assert!(fresh.ensure(&hl, "Python", &rope, usize::MAX, Duration::MAX));
            for l in 0..rope.len_lines() {
                assert_eq!(cache.spans(l), fresh.spans(l), "line {l} after editing line {line}");
            }
        }
    }

    /// Edits anywhere (several before the next highlighting, some not highlighted to the end, as
    /// on a screen) always give the colors of a highlighting from scratch.
    #[test]
    fn cache_after_random_edits_matches_a_fresh_highlight() {
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let text: String = (0..120).map(|i| format!("def f{i}(x):\n    s = '''doc {i}\n    more'''\n    return x + {i}  # c\n")).collect();
        let mut rope = Rope::from_str(&text);
        let mut cache = HlCache { regex_only: true, ..Default::default() };
        let mut seed = 7u64;
        let mut rand = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n.max(1)
        };
        let pieces = ["a", "\n", "'''", "\"", "#", "\n\n\n", "def g():\n", "x"];
        for round in 0..300 {
            for _ in 0..1 + rand(3) {
                let line = rand(rope.len_lines());
                let at = rope.line_to_char(line) + rand(rope.line(line).len_chars());
                if rand(4) == 0 {
                    let end = (at + 1 + rand(80)).min(rope.len_chars());
                    rope.remove(at..end);
                } else {
                    rope.insert(at, pieces[rand(pieces.len())]);
                }
                cache.invalidate(line);
            }
            // a screen somewhere, or the whole file
            let upto = if rand(3) == 0 { usize::MAX } else { rand(rope.len_lines()) + 40 };
            assert!(cache.ensure(&hl, "Python", &rope, upto, Duration::MAX));
            let mut fresh = HlCache { regex_only: true, ..Default::default() };
            assert!(fresh.ensure(&hl, "Python", &rope, usize::MAX, Duration::MAX));
            for l in 0..cache.lines.len() {
                assert_eq!(cache.spans(l), fresh.spans(l), "line {l}, round {round}");
            }
        }
    }

    /// With a syntax tree (Python): random edits told to the cache as the document does, the
    /// colors of the lines shown always those of a fresh parse.
    #[test]
    fn tree_colors_follow_the_edits() {
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let text: String = (0..200).map(|i| format!("def f{i}(x):\n    return x + {i}  # c\n    s = 'a{i}'\n")).collect();
        let mut rope = Rope::from_str(&text);
        let mut cache = HlCache::default();
        cache.view_from = 0;
        assert!(cache.ensure(&hl, "Python", &rope, 80, Duration::MAX));
        assert!(cache.uses_tree());
        let mut seed = 7u64;
        let mut rnd = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n.max(1)
        };
        for round in 0..60 {
            let len = rope.len_chars();
            let a = rnd(len);
            let b = (a + rnd(6)).min(len);
            let ins = ["x", "\n", "'", "def ", "", "# ", "(", ")"][rnd(8)];
            cache.tree_edit(crate::syntax_tree::input_edit(&rope, a, b, ins));
            rope.remove(a..b);
            rope.insert(a, ins);
            cache.invalidate(rope.char_to_line(a));
            let from = rnd(rope.len_lines().saturating_sub(40));
            cache.view_from = from;
            assert!(cache.ensure(&hl, "Python", &rope, from + 40, Duration::MAX));
            let mut fresh = HlCache { view_from: from, ..Default::default() };
            assert!(fresh.ensure(&hl, "Python", &rope, from + 40, Duration::MAX));
            for l in from..from + 40 {
                assert_eq!(cache.spans(l), fresh.spans(l), "line {l}, round {round}");
            }
        }
    }

    /// A long file: its first parse runs on a thread (no colors, no wait), the edits made
    /// meanwhile are applied to its tree once back: the same colors as a parse from scratch.
    #[test]
    fn a_long_file_is_parsed_on_a_thread() {
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let text: String = (0..3000).map(|i| format!("def f{i}(x):\n    return x + {i}  # c\n")).collect();
        assert!(text.len() > 64 * 1024);
        let mut rope = Rope::from_str(&text);
        let mut cache = HlCache::default();
        // its query compiled first (once per language, ahead at startup)
        assert!(crate::syntax_tree::grammar_for("Python").is_some());
        let t = std::time::Instant::now();
        assert!(!cache.ensure(&hl, "Python", &rope, 40, Duration::MAX), "not done: on a thread");
        assert!(t.elapsed() < Duration::from_millis(20), "the UI does not wait: {:?}", t.elapsed());
        assert!(cache.spans(0).is_none());
        // typed while it parses: a string opened on line 0
        cache.tree_edit(crate::syntax_tree::input_edit(&rope, 0, 0, "s = \"\"\"\n"));
        rope.insert(0, "s = \"\"\"\n");
        cache.invalidate(0);
        let t = std::time::Instant::now();
        while !cache.ensure(&hl, "Python", &rope, 40, Duration::MAX) {
            assert!(t.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut fresh = HlCache::default();
        while !fresh.ensure(&hl, "Python", &rope, 40, Duration::MAX) {
            std::thread::sleep(Duration::from_millis(5));
        }
        for l in 0..40 {
            assert!(cache.spans(l).is_some());
            assert_eq!(cache.spans(l), fresh.spans(l), "line {l}");
        }
    }

    /// With a syntax tree, only the lines shown are colored; the pinned lines above them on demand.
    #[test]
    fn tree_colors_the_lines_shown_and_the_pinned_ones() {
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let text: String = (0..300).map(|i| format!("def f{i}(x):\n    return x\n")).collect();
        let rope = Rope::from_str(&text);
        let mut cache = HlCache { view_from: 400, ..Default::default() };
        assert!(cache.ensure(&hl, "Python", &rope, 450, Duration::MAX));
        assert!(cache.spans(400).is_some() && cache.spans(10).is_none());
        cache.ensure_lines(&hl, &rope, &[10]);
        assert!(cache.spans(10).is_some_and(|s| !s.is_empty()));
    }

    /// `FLORM_BENCH_HL=<fichier> cargo test --release bench_tree_typing -- --nocapture`: keys typed
    /// in the middle of a file highlighted by its syntax tree, the screen read again after each.
    #[test]
    fn bench_tree_typing() {
        let Ok(file) = std::env::var("FLORM_BENCH_HL") else { return };
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let mut rope = Rope::from_str(&std::fs::read_to_string(&file).unwrap());
        let mut cache = HlCache::default();
        let mid = rope.len_lines() / 2;
        cache.view_from = mid;
        let t = Instant::now();
        assert!(cache.ensure(&hl, "Python", &rope, mid + 60, Duration::MAX));
        println!("opening: {:?} ({} lines)", t.elapsed(), rope.len_lines());
        let t = Instant::now();
        let n = 50;
        for k in 0..n {
            let at = rope.line_to_char(mid + 10) + 4 + k;
            cache.tree_edit(crate::syntax_tree::input_edit(&rope, at, at, "x"));
            rope.insert(at, "x");
            cache.invalidate(mid + 10);
            assert!(cache.ensure(&hl, "Python", &rope, mid + 60, Duration::MAX));
        }
        println!("per key: {:?}", t.elapsed() / n as u32);
    }

    /// With a window, a language is first prepared on a thread: nothing highlighted until then
    /// (the frame is not held), then the colors as usual.
    #[test]
    fn language_prepared_on_a_thread() {
        let mut hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        hl.prepare_on_thread(egui::Context::default());
        let rope = Rope::from_str("def f(x):\n    return 'a'\n");
        let mut cache = HlCache { regex_only: true, ..Default::default() };
        assert!(cache.ensure(&hl, "Python", &rope, 1, Duration::MAX));
        assert!(cache.spans(0).is_none());
        let t = Instant::now();
        while !hl.warm.lock().unwrap().get("Python").copied().unwrap_or(false) {
            assert!(t.elapsed() < Duration::from_secs(10));
            std::thread::yield_now();
        }
        assert!(cache.ensure(&hl, "Python", &rope, 1, Duration::MAX));
        assert!(cache.spans(1).is_some_and(|s| s.len() > 1));
        // plain text needs no preparing
        let mut plain = HlCache { regex_only: true, ..Default::default() };
        assert!(plain.ensure(&hl, "Plain Text", &rope, 1, Duration::MAX) && plain.spans(1).is_some());
    }

    /// Typing on a line parses that line and a few after it, not all the lines below.
    #[test]
    fn typing_parses_a_few_lines() {
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let text: String = (0..500).map(|i| format!("def f{i}(x):\n    return x + {i}  # c\n")).collect();
        let mut rope = Rope::from_str(&text);
        let mut cache = HlCache { regex_only: true, ..Default::default() };
        assert!(cache.ensure(&hl, "Python", &rope, usize::MAX, Duration::MAX));
        for (n, ins) in ["a", "\n", "b", "\n\n"].iter().enumerate() {
            let before = cache.parsed;
            let at = rope.line_to_char(100 + n) + 4;
            rope.insert(at, ins);
            cache.invalidate(100 + n);
            assert!(cache.ensure(&hl, "Python", &rope, usize::MAX, Duration::MAX));
            // at worst from the state before the line to the state after the lines typed
            assert!(cache.parsed - before <= 2 * CHECKPOINT + 3, "{} lines parsed for {ins:?}", cache.parsed - before);
            let mut fresh = HlCache { regex_only: true, ..Default::default() };
            assert!(fresh.ensure(&hl, "Python", &rope, usize::MAX, Duration::MAX));
            assert!((0..rope.len_lines()).all(|l| cache.spans(l) == fresh.spans(l)));
        }
        // an opened string changes every line after it: all of them parsed again
        let before = cache.parsed;
        rope.insert(rope.line_to_char(10), "'''");
        cache.invalidate(10);
        assert!(cache.ensure(&hl, "Python", &rope, usize::MAX, Duration::MAX));
        assert!(cache.parsed - before > 900);
    }

    /// `FLORM_BENCH_HL=<fichier> cargo test bench_typing -- --nocapture`: typing near the top of
    /// a file, the screen and the minimap highlighted again after each key.
    #[test]
    fn bench_typing() {
        let Ok(file) = std::env::var("FLORM_BENCH_HL") else { return };
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let syntax = crate::symbols::syntax_for(Path::new(&file)).unwrap_or("Plain Text");
        let mut rope = Rope::from_str(&std::fs::read_to_string(&file).unwrap());
        let mut cache = HlCache { regex_only: true, ..Default::default() };
        cache.ensure(&hl, syntax, &rope, usize::MAX, Duration::MAX);
        let t = std::time::Instant::now();
        for k in 0..200 {
            let line = 20 + k % 3;
            let at = rope.line_to_char(line) + 2;
            if k % 2 == 0 { rope.insert(at, "x"); } else { rope.remove(at..at + 1); }
            cache.invalidate(line);
            cache.ensure(&hl, syntax, &rope, 60, Duration::MAX);
            cache.ensure(&hl, syntax, &rope, 700, Duration::MAX);
        }
        eprintln!("200 frappes ligne 20 (écran + minimap) : {:?} soit {:?} par frappe", t.elapsed(), t.elapsed() / 200);
    }

    /// `FLORM_BENCH_HL=<fichier> cargo test --release bench_highlight -- --nocapture`
    #[test]
    fn bench_highlight() {
        let Ok(file) = std::env::var("FLORM_BENCH_HL") else { return };
        let rss = || std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split(' ').nth(1)?.parse::<usize>().ok()).map_or(0, |p| p * 4096 / 1_000_000);
        let hl = Highlighting::new("Florm", Path::new("/nonexistent"));
        let syntax = crate::symbols::syntax_for(Path::new(&file)).unwrap_or("Plain Text");
        let rope = Rope::from_str(&std::fs::read_to_string(&file).unwrap());
        let before = rss();
        let mut cache = HlCache { regex_only: true, ..Default::default() };
        let t = std::time::Instant::now();
        cache.ensure(&hl, syntax, &rope, usize::MAX, Duration::MAX);
        eprintln!("{} lignes {syntax} : {:?}, cache {} Mo", rope.len_lines(), t.elapsed(), rss() - before);
        // typing at the end of the file, then highlighting the screen again
        let t = std::time::Instant::now();
        for _ in 0..100 {
            cache.invalidate(rope.len_lines() - 50);
            cache.ensure(&hl, syntax, &rope, usize::MAX, Duration::MAX);
        }
        eprintln!("100 frappes en fin de fichier : {:?}", t.elapsed());
    }
}
