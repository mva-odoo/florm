//! Settings and plugins.

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
        if s.editor_font != self.settings.editor_font || s.ui_font != self.settings.ui_font {
            self.egui_ctx.set_fonts(crate::fonts::definitions(&s.editor_font, &s.ui_font));
        }
        if s.accent_color != self.settings.accent_color || s.text_softness != self.settings.text_softness {
            self.hl.set_overrides(highlight::parse_hex(&s.accent_color), s.text_softness);
            self.visuals_dirty = true;
        }
        let parts = (s.github, s.debugger, s.claude) != (self.settings.github, self.settings.debugger, self.settings.claude);
        self.settings = s;
        if parts {
            self.apply_features();
        }
        self.rebuild_keymap();
        self.set_status(t!("Préférences rechargées", "Preferences reloaded"));
    }

    /// GitHub, the debugger and Claude on or off as the settings say: the views of those turned
    /// off are closed (tab and bottom panel), a debugging session is stopped.
    pub(super) fn apply_features(&mut self) {
        use crate::features::{self, Feature};
        let s = &self.settings;
        features::set_all(s.github, s.debugger, s.claude);
        let off = |v: &document::View| match v {
            document::View::PullRequests => !features::on(Feature::GitHub),
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
        if self.shared_busy {
            return;
        }
        self.shared_busy = true;
        self.set_status(match op {
            Op::Sync => t!("Configuration partagée : synchronisation…", "Shared configuration: synchronizing…"),
            Op::Publish => t!("Configuration partagée : publication…", "Shared configuration: publishing…"),
            Op::Create => t!("Configuration partagée : création du dépôt…", "Shared configuration: creating the repository…"),
        });
        self.jobs.spawn(move || JobResult::Shared(op, crate::shared::run(op, &spec)));
    }

    pub(super) fn shared_done(&mut self, op: crate::shared::Op, result: Result<(String, String), String>) {
        self.shared_busy = false;
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

    /// Preferences, templates, themes and plugins read again (the shared ones changed).
    pub(super) fn reload_shared(&mut self) {
        // read first: the shared folder depends on `shared_config`
        let _ = config::load();
        if let Some(d) = crate::shared::sub("themes") {
            self.hl.add_folder(&d);
        }
        self.reload_settings();
        self.templates = crate::templates::load_all();
        self.reload_plugins();
    }

    pub(super) fn reload_plugins(&mut self) {
        let mut dirs = vec![config::plugins_dir()];
        dirs.extend(crate::shared::sub("plugins"));
        // the plugins of the folder run commands: only once it is trusted
        if let Some(r) = self.root.clone() {
            let local = r.join(".florm").join("plugins");
            if plugins::has_plugins(&local) {
                if plugins::is_trusted(&r) {
                    dirs.push(local);
                } else if self.confirm.is_none() && self.trust_asked.insert(r.clone()) {
                    self.confirm = Some(Confirm::TrustPlugins(r));
                }
            }
        }
        let (p, errs) = plugins::load_all(&dirs);
        for e in errs {
            self.log(e);
        }
        self.plugins = p;
        self.plugin_cmds = self
            .plugins
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| (0..p.commands.len()).map(move |ci| (pi, ci)))
            .collect();
        self.rebuild_keymap();
    }

    pub(super) fn rebuild_keymap(&mut self) {
        let pk: Vec<(String, Cmd)> = self
            .plugin_cmds
            .iter()
            .enumerate()
            .filter_map(|(k, &(pi, ci))| self.plugins[pi].commands[ci].keys.clone().map(|key| (key, Cmd::Plugin(k))))
            .collect();
        let (km, errs) = Keymap::new(&self.settings.keys, &pk);
        self.keymap = km;
        for e in errs {
            self.log(e);
        }
    }

    pub(super) fn run_plugin(&mut self, k: usize) {
        let Some(&(pi, ci)) = self.plugin_cmds.get(k) else { return };
        let def = self.plugins[pi].commands[ci].clone();
        let dir = self.plugins[pi].dir.clone();
        let env = self.plugin_env(self.active);
        let (uid, version, inputs, output) = match self.docs.get(self.active) {
            Some(d) => {
                let all_empty = d.sels.iter().all(|s| s.is_empty());
                let (inputs, whole) = match def.input {
                    Input::None => (vec![String::new()], false),
                    Input::Buffer => (vec![d.rope.to_string()], true),
                    Input::Selection if all_empty => (vec![d.rope.to_string()], true),
                    Input::Selection => (d.sels.iter().map(|s| d.slice(s.start(), s.end())).collect(), false),
                };
                let output =
                    if whole && def.output == Output::ReplaceSelection { Output::ReplaceBuffer } else { def.output };
                (d.uid, d.version, inputs, output)
            }
            None => (0, 0, vec![String::new()], Output::Panel),
        };
        self.set_status(format!("Plugin : {}…", def.title));
        let title = def.title.clone();
        self.jobs.spawn(move || {
            let mut outs = Vec::new();
            for inp in &inputs {
                match plugins::exec(&dir, &def.run, inp, &env) {
                    Ok(o) => outs.push(o),
                    Err(e) => return JobResult::Plugin { uid, version, title, output, result: Err(e) },
                }
            }
            JobResult::Plugin { uid, version, title, output, result: Ok(outs) }
        });
    }

    pub(super) fn plugin_env(&self, idx: usize) -> Vec<(String, String)> {
        let mut env = Vec::new();
        if let Some(r) = &self.root {
            env.push(("FLORM_PROJECT".into(), r.display().to_string()));
        }
        if let Some(d) = self.docs.get(idx) {
            if let Some(p) = &d.path {
                env.push(("FLORM_FILE".into(), p.display().to_string()));
            }
            let (l, c) = d.line_col(d.primary_sel().head);
            env.push(("FLORM_LINE".into(), (l + 1).to_string()));
            env.push(("FLORM_COLUMN".into(), (c + 1).to_string()));
            env.push(("FLORM_SYNTAX".into(), d.syntax.clone()));
        }
        env
    }
}
