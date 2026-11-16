//! A picture of Florm's window for Claude (Ctrl+Alt+I, or « + Contexte »): taken by egui itself, so
//! without leaving Florm nor any permission of the system; from any view (a pull request, a file…),
//! it goes with the next message of the current conversation.

use super::*;

/// A picture asked: when to take it (a little later: the menu or the palette closed first, or the
/// few seconds to go to the view to show), and whether egui was asked for it.
pub(super) struct Shot {
    at: Instant,
    asked: bool,
}

impl App {
    /// A picture of the window in `delay` (zero: as soon as the menu or palette is gone).
    pub(super) fn screenshot_for_claude(&mut self, delay: Duration) {
        self.shot = Some(Shot { at: Instant::now() + delay.max(Duration::from_millis(150)), asked: false });
        if delay >= Duration::from_secs(1) {
            self.set_status(tf!(
                "Capture de Florm dans {} s : va sur la vue à montrer",
                "Picture of Florm in {} s: go to the view to show",
                delay.as_secs()
            ));
        }
    }

    /// At each frame: asks egui for the picture when it is time, then gives it to Claude.
    pub(super) fn screenshot_tick(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.shot.as_mut() else { return };
        if !shot.asked {
            let left = shot.at.saturating_duration_since(Instant::now());
            if left.is_zero() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
                shot.asked = true;
            } else {
                ctx.request_repaint_after(left);
            }
            return;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else {
            ctx.request_repaint();
            return;
        };
        self.shot = None;
        let (w, h, rgba) = (image.width(), image.height(), image.as_raw().to_vec());
        let label = t!("Capture de Florm", "Picture of Florm").to_string();
        // scaled down and made PNG away from the interface (a big screen is a few million pixels)
        self.jobs.spawn(move || JobResult::ClaudeImage { label, png: crate::claude::png_of(w, h, rgba) });
    }
}
