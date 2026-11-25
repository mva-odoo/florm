//! Frame profiler, on with `FLORM_PROFILE=1`: in a corner, the frames per second, the time of a
//! frame (average, worst), the parts of the window that cost the most, and what asks egui to
//! redraw when nothing happens (a timer, a spinner…). Off, it costs one branch per frame.

use eframe::egui;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// How far back the numbers go.
const WINDOW: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(super) struct Profiler {
    pub(super) on: bool,
    /// Start of the frame being measured, and of its current part.
    start: Option<Instant>,
    part: Option<Instant>,
    /// Time of each part in this frame.
    parts: Vec<(&'static str, Duration)>,
    /// The frames of the last seconds: when, how long, their parts, why they were drawn.
    frames: VecDeque<Frame>,
    /// When the numbers were last written to stderr.
    logged: Option<Instant>,
}

struct Frame {
    at: Instant,
    took: Duration,
    parts: Vec<(&'static str, Duration)>,
    causes: Vec<String>,
}

impl Profiler {
    pub(super) fn new() -> Self {
        Self { on: std::env::var_os("FLORM_PROFILE").is_some_and(|v| !v.is_empty() && v != "0"), ..Default::default() }
    }

    /// A frame starts (after the pacing sleep: only the work is counted).
    pub(super) fn begin(&mut self) {
        if self.on {
            let now = Instant::now();
            self.start = Some(now);
            self.part = Some(now);
            self.parts.clear();
        }
    }

    /// The part `name` of the frame ends here.
    pub(super) fn mark(&mut self, name: &'static str) {
        if let Some(t) = self.part.filter(|_| self.on) {
            let now = Instant::now();
            self.parts.push((name, now - t));
            self.part = Some(now);
        }
    }

    /// The frame ends: kept, and the numbers drawn.
    pub(super) fn end(&mut self, ctx: &egui::Context) {
        let Some(start) = self.start.take().filter(|_| self.on) else { return };
        let now = Instant::now();
        let causes = ctx.repaint_causes().iter().map(|c| format!("{}:{} {}", short(c.file), c.line, c.reason)).collect();
        self.frames.push_back(Frame { at: now, took: now - start, parts: std::mem::take(&mut self.parts), causes });
        while self.frames.front().is_some_and(|f| now - f.at > WINDOW) {
            self.frames.pop_front();
        }
        let text = self.summary();
        // also on stderr, every window: readable when the corner is hidden, and by a script
        if self.logged.is_none_or(|t| now - t >= WINDOW) {
            self.logged = Some(now);
            eprintln!("[florm profile]\n{text}");
        }
        paint(ctx, text);
    }

    fn summary(&self) -> String {
        let n = self.frames.len().max(1);
        let secs = WINDOW.as_secs_f32();
        let ms = |d: Duration| d.as_secs_f32() * 1000.0;
        let total: Duration = self.frames.iter().map(|f| f.took).sum();
        let worst = self.frames.iter().map(|f| f.took).max().unwrap_or_default();
        let mut lines = vec![format!(
            "{:.0} img/s · {:.2} ms moy. · {:.1} ms max · CPU UI {:.0} %",
            self.frames.len() as f32 / secs,
            ms(total) / n as f32,
            ms(worst),
            100.0 * total.as_secs_f32() / secs
        )];
        // the parts, the most costly first
        let mut parts: HashMap<&str, Duration> = HashMap::new();
        for f in &self.frames {
            for (k, d) in &f.parts {
                *parts.entry(k).or_default() += *d;
            }
        }
        let mut parts: Vec<_> = parts.into_iter().collect();
        parts.sort_by(|a, b| b.1.cmp(&a.1));
        for (k, d) in parts.iter().take(8) {
            lines.push(format!("  {k:<14} {:>6.2} ms", ms(*d) / n as f32));
        }
        // what asked for the frames (none: an input, a key or the mouse)
        let mut causes: HashMap<&str, usize> = HashMap::new();
        for f in &self.frames {
            if f.causes.is_empty() {
                *causes.entry("(entrée : clavier, souris…)").or_default() += 1;
            }
            for c in &f.causes {
                *causes.entry(c).or_default() += 1;
            }
        }
        let mut causes: Vec<_> = causes.into_iter().collect();
        causes.sort_by(|a, b| b.1.cmp(&a.1));
        lines.push("redessiné par :".into());
        for (c, k) in causes.iter().take(6) {
            lines.push(format!("  {k:>4}× {c}"));
        }
        lines.join("\n")
    }
}

fn paint(ctx: &egui::Context, text: String) {
    {
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::unique("florm_profile")));
        let font = egui::FontId::monospace(11.0);
        let galley = p.layout_no_wrap(text, font, egui::Color32::WHITE);
        let screen = ctx.content_rect();
        let at = egui::pos2(screen.right() - galley.size().x - 16.0, screen.bottom() - galley.size().y - 40.0);
        let bg = egui::Rect::from_min_size(at, galley.size()).expand(6.0);
        p.rect_filled(bg, 4.0, egui::Color32::from_black_alpha(200));
        p.galley(at, galley, egui::Color32::WHITE);
    }
}

/// `src/app/panels.rs` from the path the compiler gave.
fn short(file: &str) -> &str {
    file.find("src/").map_or(file, |i| &file[i..])
}
