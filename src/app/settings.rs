//! Settings and plugins.

use super::*;

impl App {
    pub(super) fn reload_settings(&mut self) {
        let (s, err) = config::load();
        if let Some(e) = err {
            self.log(e.clone());
            self.set_status(e);
            return;
        }
        if s.theme != self.hl.theme_name {
            if self.hl.set_theme(&s.theme) {
                self.visuals_dirty = true;
            } else {
                self.log(format!("Thème inconnu : {}", s.theme));
            }
        }
        if s.editor_font != self.settings.editor_font || s.ui_font != self.settings.ui_font {
            self.egui_ctx.set_fonts(crate::fonts::definitions(&s.editor_font, &s.ui_font));
        }
        if s.accent_color != self.settings.accent_color || s.text_softness != self.settings.text_softness {
            self.hl.set_overrides(highlight::parse_hex(&s.accent_color), s.text_softness);
            self.visuals_dirty = true;
        }
        self.settings = s;
        self.rebuild_keymap();
        self.set_status("Préférences rechargées");
    }

    pub(super) fn reload_plugins(&mut self) {
        let mut dirs = vec![config::plugins_dir()];
        if let Some(r) = &self.root {
            dirs.push(r.join(".florm").join("plugins"));
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
}
