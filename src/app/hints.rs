//! Everything clickable reached from the keyboard: Alt alone (pressed and released) puts one or two
//! letters on each button, link, tab, box and field on screen; typing them clicks it (Escape: none).
//! The widgets are those egui saw at the previous frame; the click is a press and a release of the
//! pointer given to egui before the next frames, as if made with the mouse.

use super::*;

/// The letters of the labels: the home row first, the easiest to type.
const LETTERS: &str = "asdfjklghqweruiotypzxcvbnm";

#[derive(Default)]
pub(super) struct Hints {
    /// The labels shown: letters and where the widget is.
    targets: Vec<(String, egui::Rect)>,
    /// The letters typed so far.
    typed: String,
    pub(super) active: bool,
    /// Alt went down with nothing else since: its release opens the labels.
    alt_alone: bool,
    alt_was_down: bool,
    /// A click being made: the release of the pointer comes at the next frame.
    release_at: Option<egui::Pos2>,
    /// Asked from the palette: shown in a few frames, once the palette is gone.
    pub(super) open_in: u8,
}

/// The labels of `n` widgets: all of the same length, so none is the start of another.
fn codes(n: usize) -> Vec<String> {
    let letters: Vec<char> = LETTERS.chars().collect();
    let mut len = 1;
    while letters.len().pow(len as u32) < n {
        len += 1;
    }
    (0..n)
        .map(|mut k| {
            let mut s = Vec::with_capacity(len);
            for _ in 0..len {
                s.push(letters[k % letters.len()]);
                k /= letters.len();
            }
            s.into_iter().rev().collect()
        })
        .collect()
}

impl App {
    /// Before egui reads the input of a frame: Alt alone opens the labels; while they are shown, the
    /// keys typed choose one (they go nowhere else), and its click is given as pointer events.
    pub(super) fn hints_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let h = &mut self.hints;
        // the second half of a click: the release, one frame after the press
        if let Some(p) = h.release_at.take() {
            raw.events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
        }
        if h.open_in > 0 {
            h.open_in -= 1;
            if h.open_in == 0 {
                self.hints_open(ctx);
                return;
            }
        }
        let h = &mut self.hints;
        let alt = raw.modifiers.alt;
        let other = raw.events.iter().any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::PointerButton { .. } | egui::Event::MouseWheel { .. }));
        if alt && !h.alt_was_down {
            h.alt_alone = !other;
        } else if alt && other {
            h.alt_alone = false;
        }
        let tapped = !alt && h.alt_was_down && h.alt_alone && !other;
        h.alt_was_down = alt;
        if tapped && self.settings.keyboard_hints {
            if self.hints.active {
                self.hints_close();
            } else {
                self.hints_open(ctx);
            }
            return;
        }
        if !self.hints.active {
            return;
        }
        let h = &mut self.hints;
        let mut chosen = None;
        let mut close = false;
        raw.events.retain(|e| match e {
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => {
                close = true;
                false
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                h.typed.pop();
                false
            }
            egui::Event::Text(t) => {
                for c in t.chars().filter(|c| c.is_alphabetic()) {
                    h.typed.extend(c.to_lowercase());
                }
                false
            }
            egui::Event::Key { .. } => false,
            // a click with the mouse: the labels go
            egui::Event::PointerButton { pressed: true, .. } => {
                close = true;
                true
            }
            _ => true,
        });
        if !h.typed.is_empty() {
            match h.targets.iter().find(|(code, _)| *code == h.typed) {
                Some((_, rect)) => chosen = Some(rect.center()),
                // letters leading nowhere: started again
                None if !h.targets.iter().any(|(code, _)| code.starts_with(&h.typed)) => h.typed.clear(),
                None => {}
            }
        }
        if let Some(p) = chosen {
            raw.events.push(egui::Event::PointerMoved(p));
            raw.events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
            self.hints.release_at = Some(p);
            close = true;
        }
        if close {
            self.hints_close();
        }
        ctx.request_repaint();
    }

    /// The labels put on what egui saw clickable at the last frame: only the dialog or the menu
    /// open when there is one (what is behind cannot be clicked anyway).
    pub(super) fn hints_open(&mut self, ctx: &egui::Context) {
        let screen = ctx.screen_rect();
        // copied first: reading the state of a field locks egui, which `viewport` holds
        let widgets: Vec<(egui::Id, egui::LayerId, egui::Rect, bool)> = ctx.viewport(|v| {
            v.prev_pass
                .widgets
                .layers()
                .flat_map(|(_, w)| w.iter())
                .filter(|w| w.enabled && w.sense.click && w.interact_rect.width() >= 4.0 && w.interact_rect.height() >= 4.0)
                .filter(|w| screen.intersects(w.interact_rect))
                .map(|w| (w.id, w.layer_id, w.interact_rect, w.sense.drag))
                .collect()
        });
        // what can be dragged too is text that can be selected (left out), or a field (kept)
        let mut found: Vec<(egui::LayerId, egui::Rect)> =
            widgets.into_iter().filter(|(id, _, _, drag)| !drag || egui::TextEdit::load_state(ctx, *id).is_some()).map(|(_, l, r, _)| (l, r)).collect();
        // the dark backdrop of a dialog covers the whole window: only what is above it counts
        let backdrop = found.iter().any(|(l, r)| l.order == egui::Order::PanelResizeLine && r.area() >= screen.area() * 0.9);
        let top = found.iter().map(|(l, _)| l.order).max().unwrap_or(egui::Order::Background);
        found.retain(|(l, r)| {
            let big = r.area() >= screen.area() * 0.9;
            if top >= egui::Order::Foreground {
                l.order >= egui::Order::Foreground
            } else if backdrop {
                l.order > egui::Order::PanelResizeLine && !big
            } else {
                !big
            }
        });
        // in reading order: the letters follow the screen
        found.sort_by(|a, b| (a.1.top() as i32 / 8, a.1.left() as i32).cmp(&(b.1.top() as i32 / 8, b.1.left() as i32)));
        found.dedup_by(|a, b| a.1 == b.1);
        if found.is_empty() {
            self.set_status(t!("Rien à cliquer ici", "Nothing to click here"));
            return;
        }
        let codes = codes(found.len());
        self.hints.targets = codes.into_iter().zip(found.into_iter().map(|(_, r)| r)).collect();
        self.hints.typed.clear();
        self.hints.active = true;
        ctx.request_repaint();
    }

    pub(super) fn hints_close(&mut self) {
        self.hints.active = false;
        self.hints.typed.clear();
        self.hints.targets.clear();
    }

    /// The labels, over everything: the letters already typed dimmed, the others in bold.
    pub(super) fn hints_ui(&mut self, ctx: &egui::Context) {
        if !self.hints.active {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::new("keyboard_hints")));
        let accent = self.hl.colors.accent;
        let (fill, text) = (Color32::from_rgb(0xff, 0xd8, 0x4d), Color32::from_rgb(0x20, 0x20, 0x20));
        let font = egui::FontId::monospace(12.0);
        let typed = &self.hints.typed;
        for (code, rect) in &self.hints.targets {
            if !code.starts_with(typed.as_str()) {
                continue;
            }
            let mut job = egui::text::LayoutJob::default();
            job.append(typed, 0.0, egui::TextFormat { font_id: font.clone(), color: text.gamma_multiply(0.45), ..Default::default() });
            job.append(&code[typed.len()..], 0.0, egui::TextFormat { font_id: font.clone(), color: text, ..Default::default() });
            let galley = ctx.fonts(|f| f.layout_job(job));
            let at = egui::pos2(rect.left().max(0.0), rect.top().max(0.0));
            let r = egui::Rect::from_min_size(at, galley.size() + egui::vec2(8.0, 2.0));
            painter.rect(r, 3.0, fill, egui::Stroke::new(1.0_f32, accent));
            painter.galley(r.min + egui::vec2(4.0, 1.0), galley, text);
        }
        // the way out, at the bottom
        let tip = t!("Tape les lettres pour cliquer · Retour arrière : corriger · Échap : fermer", "Type the letters to click · Backspace: fix · Esc: close");
        let screen = ctx.screen_rect();
        let galley = ctx.fonts(|f| f.layout_no_wrap(tip.to_string(), egui::FontId::proportional(13.0), text));
        let r = egui::Rect::from_center_size(egui::pos2(screen.center().x, screen.bottom() - 46.0), galley.size() + egui::vec2(20.0, 10.0));
        painter.rect(r, 6.0, fill, egui::Stroke::new(1.0_f32, accent));
        painter.galley(r.min + egui::vec2(10.0, 5.0), galley, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_short_and_none_starts_another() {
        assert_eq!(codes(3), ["a", "s", "d"]);
        let many = codes(100);
        assert!(many.iter().all(|c| c.len() == 2));
        assert_eq!(many.iter().collect::<std::collections::HashSet<_>>().len(), 100);
        assert!(codes(0).is_empty());
    }

    /// Alt alone shows the labels; the letters of one click it (and do not reach the editor).
    #[test]
    fn alt_then_letters_click_a_button() {
        let dir = std::env::temp_dir().join(format!("florm_hints_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        let mut clicks = 0;
        let alt = egui::Modifiers { alt: true, ..Default::default() };
        for frame in 0..10 {
            let mut raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
            match frame {
                2 => raw.modifiers = alt,
                5 => raw.events.push(egui::Event::Text("s".into())),
                _ => {}
            }
            app.hints_input(&ctx, &mut raw);
            if frame == 3 {
                assert!(app.hints.active, "Alt alone shows the labels");
                assert_eq!(app.hints.targets.len(), 2);
            }
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let _ = ui.button("Un");
                    if ui.button("Deux").clicked() {
                        clicks += 1;
                    }
                    ui.label("du texte, pas un bouton");
                });
                app.hints_ui(ctx);
            });
        }
        assert_eq!(clicks, 1, "« s » clicks the second button");
        assert!(!app.hints.active);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
