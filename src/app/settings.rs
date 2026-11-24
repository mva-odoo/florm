//! Settings: read again, the keymap built from them.

use super::*;

impl App {
    pub(super) fn reload_settings(&mut self) {
        let (s, err) = config::load();
        if let Some(e) = err {
            self.log(e.clone());
            self.set_error(tf!("Préférences : {e}", "Preferences: {e}"));
            return;
        }
        if s.theme != self.hl.theme_name {
            if self.hl.set_theme(&s.theme) {
                self.visuals_dirty = true;
            } else {
                self.log(tf!("Thème inconnu : {}", "Unknown theme: {}", s.theme));
            }
        }
        if s.ui_style != self.settings.ui_style {
            highlight::SQUARE.store(s.ui_style == "square", Ordering::Relaxed);
            self.visuals_dirty = true;
        }
        if s.editor_font != self.settings.editor_font || s.ui_font != self.settings.ui_font || s.ui_style != self.settings.ui_style {
            self.egui_ctx.set_fonts(crate::fonts::definitions(&s.editor_font, &s.ui_font, s.ui_style == "square"));
        }
        if s.accent_color != self.settings.accent_color || s.text_softness != self.settings.text_softness {
            self.hl.set_overrides(highlight::parse_hex(&s.accent_color), s.text_softness);
            self.visuals_dirty = true;
        }
        let parts = (s.github, s.debugger, s.claude) != (self.settings.github, self.settings.debugger, self.settings.claude);
        let sandbox = s.claude_sandbox != self.settings.claude_sandbox;
        history::SIDE_BY_SIDE.store(s.diff_side_by_side, Ordering::Relaxed);
        self.settings = s;
        if parts {
            self.apply_features();
        }
        // changed in the file too: the conversations open start again with it, as from the preferences
        if sandbox {
            self.claude_restart_terms();
        }
        self.rebuild_keymap();
        self.set_status(t!("Préférences rechargées", "Preferences reloaded"));
    }

    /// Git, GitHub, the debugger and Claude on or off as the settings say: the views of those turned
    /// off are closed (tab and bottom panel), a debugging session is stopped.
    pub(super) fn apply_features(&mut self) {
        use crate::features::{self, Feature};
        let s = &self.settings;
        features::set_all(s.github, s.debugger, s.claude, s.git && crate::tools::Tool::Git.installed());
        crate::claude_account::set_default(super::claude_ui::default_account_dir(s));
        let off = |v: &document::View| match v {
            document::View::PullRequests => !features::on(Feature::GitHub),
            document::View::Git | document::View::History => !features::on(Feature::Git),
            document::View::Debug => !features::on(Feature::Debugger),
            document::View::Claude => !features::on(Feature::Claude),
            _ => false,
        };
        for i in (0..self.docs.len()).rev() {
            if off(&self.docs[i].view) {
                self.close_tab(i, true);
            }
        }
        if self.bottom.is_some_and(|b| !b.feature_on()) {
            self.bottom = None;
        }
        if !features::on(Feature::Debugger) && self.debug_active() {
            self.debug_stop();
        }
    }

    /// Synchronizes, publishes or creates the shared configuration in the background.
    pub(super) fn shared_job(&mut self, op: crate::shared::Op) {
        use crate::shared::Op;
        let spec = self.settings.shared_config.trim().to_string();
        if spec.is_empty() && op != Op::Create {
            self.set_error(t!("Aucune configuration partagée : indiquez un dépôt GitHub dans Préférences → Avancé", "No shared configuration: give a GitHub repository in Preferences → Advanced"));
            return;
        }
        if self.upd.shared_busy {
            return;
        }
        self.upd.shared_busy = true;
        self.set_status(match op {
            Op::Sync => t!("Configuration partagée : synchronisation…", "Shared configuration: synchronizing…"),
            Op::Publish => t!("Configuration partagée : publication…", "Shared configuration: publishing…"),
            Op::Create => t!("Configuration partagée : création du dépôt…", "Shared configuration: creating the repository…"),
        });
        self.jobs.spawn(move || JobResult::Shared(op, crate::shared::run(op, &spec)));
    }

    pub(super) fn shared_done(&mut self, op: crate::shared::Op, result: Result<(String, String), String>) {
        self.upd.shared_busy = false;
        match result {
            Ok((spec, msg)) => {
                if spec != self.settings.shared_config {
                    self.persist("shared_config", config::toml_str(&spec));
                    self.prefs.shared = spec;
                }
                if op != crate::shared::Op::Publish {
                    self.reload_shared();
                }
                self.log(msg.clone());
                self.set_status(msg);
            }
            Err(e) => {
                self.log(tf!("Configuration partagée : {e}", "Shared configuration: {e}"));
                self.set_error(t!("Configuration partagée : échec (voir Sortie)", "Shared configuration: failed (see Output)"));
            }
        }
    }

    /// Preferences, templates and themes read again (the shared ones changed).
    pub(super) fn reload_shared(&mut self) {
        // read first: the shared folder depends on `shared_config`
        let _ = config::load();
        if let Some(d) = crate::shared::sub("themes") {
            self.hl.add_folder(&d);
        }
        self.reload_settings();
        self.templates = crate::templates::load_all();
    }

    pub(super) fn rebuild_keymap(&mut self) {
        let (km, errs) = Keymap::new(&self.settings.keys);
        self.keymap = km;
        for e in errs {
            self.log(e);
        }
    }

}
