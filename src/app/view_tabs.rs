//! The Git view, the preferences and the commit being split are editor tabs like the others:
//! dragged, moved to the other panel, listed by ▾, closed by Ctrl+W, kept in the session.
//! Échap on one of them goes back to the tab shown before it (it stays open).

use super::*;
use document::View;

impl App {
    /// The tab of `view` is the one shown (in the panel with the keyboard).
    pub(super) fn showing(&self, view: View) -> bool {
        self.docs.get(self.active).is_some_and(|d| d.view == view)
    }

    /// Git, the preferences or the split is shown (no editor takes the keys then).
    pub(super) fn special_shown(&self) -> bool {
        self.docs.get(self.active).is_some_and(|d| matches!(d.view, View::Git | View::Settings | View::Split))
    }

    pub(super) fn has_tab(&self, view: View) -> bool {
        self.docs.iter().any(|d| d.view == view)
    }

    fn view_title(&self, view: View) -> String {
        match view {
            View::Git => "Git".into(),
            View::Settings => t!("Préférences", "Preferences").into(),
            View::Split => tf!("Découpage {}", "Splitting {}", self.split.as_ref().map_or("", |s| s.info.short.as_str())),
            _ => String::new(),
        }
    }

    /// Shows the tab of `view` (made when it is missing).
    pub(super) fn show_tab(&mut self, view: View) {
        let title = self.view_title(view.clone());
        self.open_view_tab(view, title);
    }

    /// `view` is left when it is the one shown: back to the tab shown before it, which stays open.
    pub(super) fn leave_tab(&mut self, view: View) {
        if !self.showing(view) {
            return;
        }
        let cur = self.active;
        let back = self.prev_tab.and_then(|u| self.docs.iter().position(|d| d.uid == u)).filter(|&i| i != cur);
        let near = if cur > 0 { Some(cur - 1) } else { (cur + 1 < self.docs.len()).then_some(cur + 1) };
        if let Some(i) = back.or(near) {
            self.activate_tab(i);
        }
    }

    pub(super) fn close_view(&mut self, view: View) {
        if let Some(i) = self.docs.iter().position(|d| d.view == view) {
            self.close_tab(i, true);
        }
    }

    /// The tab of the split follows it: there while the split is in progress, gone after.
    pub(super) fn sync_split_tab(&mut self) {
        match (self.split.is_some(), self.has_tab(View::Split)) {
            (true, false) => {
                // added without being shown (the split may have been started from elsewhere)
                let keep = self.docs.get(self.active).map(|d| d.uid);
                self.show_tab(View::Split);
                if let Some(i) = keep.and_then(|u| self.docs.iter().position(|d| d.uid == u)) {
                    self.activate_tab(i);
                }
            }
            (false, true) => self.close_view(View::Split),
            _ => {}
        }
    }

    pub(super) fn open_settings(&mut self) {
        self.prefs.lint_args = self.settings.python_lint_args.join(" ");
        for (k, (name, _)) in lint::WITH_CODES.iter().enumerate() {
            self.prefs.lint_ignore[k] = self.settings.lint_ignore_mut(name).join(", ");
        }
        self.prefs.exclude = self.settings.exclude.join(", ");
        self.prefs.rulers = self.settings.rulers.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ");
        self.prefs.shell = self.settings.shell.clone().unwrap_or_default();
        self.prefs.shared = self.settings.shared_config.clone();
        self.show_tab(View::Settings);
    }
}
