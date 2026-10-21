//! Saving and linting documents.

use super::*;

impl App {
    pub(super) fn save(&mut self, idx: usize) -> bool {
        let trim = self.settings.trim_trailing_whitespace_on_save;
        let Some(doc) = self.docs.get_mut(idx) else { return false };
        if doc.read_only {
            return false;
        }
        if doc.path.is_none() {
            self.active = idx;
            self.open_prompt(Prompt::SaveAs);
            return false;
        }
        match doc.save(trim) {
            Ok(()) => {
                let path = doc.path.clone().unwrap();
                self.set_status(format!("Enregistré : {}", path.display()));
                self.after_save(idx, &path);
                true
            }
            Err(e) => {
                self.set_status(format!("Erreur : {e}"));
                false
            }
        }
    }

    fn after_save(&mut self, idx: usize, path: &Path) {
        if let (Some(ix), Some(d)) = (self.sym_index.as_mut(), self.docs.get(idx)) {
            ix.update(path, &d.rope.to_string());
        }
        let canon = |p: &Path| crate::os::canon(p);
        if canon(path) == canon(&config::settings_path()) {
            self.reload_settings();
        }
        if path.file_name().is_some_and(|n| n == "plugin.toml") {
            self.reload_plugins();
        }
        self.analyze(idx, true);
        self.git_refresh();
        if let Some(t) = self.tree.as_mut() {
            t.refresh();
        }
    }

    /// Lints (in-process now, external in background), runs on_save hooks, updates git gutter marks.
    pub(super) fn analyze(&mut self, idx: usize, saving: bool) {
        let env = self.plugin_env(idx);
        let Some(doc) = self.docs.get_mut(idx) else { return };
        let Some(path) = doc.path.clone() else { return };
        if doc.view != document::View::Text {
            return;
        }
        if let Some(lang) = lint::lang_of(&path) {
            if lint::is_in_process(lang) {
                doc.diagnostics = lint::lint_in_process(lang, &doc.rope.to_string());
                doc.lint_version = doc.version;
            }
        }
        let (uid, version) = (doc.uid, doc.version);
        let hooks: Vec<(PathBuf, String, bool)> = if saving {
            self.plugins
                .iter()
                .flat_map(|p| p.hooks_for("on_save", &path).map(move |h| (p.dir.clone(), h.run.clone(), h.reload)))
                .collect()
        } else {
            Vec::new()
        };
        let lint_on = self.settings.lint_on_save || !saving;
        let root = self.repo_for(&path).or_else(|| self.root.clone());
        let cfg = self.lint_config();
        let text = self.docs[idx].rope.to_string();
        self.jobs.spawn(move || {
            let mut log = Vec::new();
            let mut reload = false;
            for (dir, run, rl) in hooks {
                match plugins::exec(&dir, &run, "", &env) {
                    Ok(o) if !o.trim().is_empty() => log.push(o),
                    Ok(_) => {}
                    Err(e) => log.push(e),
                }
                reload |= rl;
            }
            // a formatting hook rewrote the file: lint what is on disk now
            let text = if reload { std::fs::read_to_string(&path).unwrap_or(text) } else { text };
            let diags = if lint_on { lint::lint_external(&path, &text, root.as_deref(), &cfg) } else { None };
            JobResult::Analyzed { uid, version, reload, diags, base: git::head_text(&path), log }
        });
    }

    /// Moves tab `from` before tab `to` (`to == len`: at the end); the moved tab becomes active.
    pub(super) fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.docs.len() {
            return;
        }
        let d = self.docs.remove(from);
        let to = if to > from { to - 1 } else { to }.min(self.docs.len());
        self.docs.insert(to, d);
        self.active = to;
        self.tab_scrolled = None;
        self.history.open = false;
        self.prefs.open = false;
        if let Some(sp) = self.split.as_mut() {
            sp.shown = false;
        }
    }

    pub(super) fn close_tab(&mut self, idx: usize, force: bool) {
        let Some(doc) = self.docs.get(idx) else { return };
        if doc.is_dirty() && !force {
            self.confirm = Some(Confirm::CloseTab(doc.uid));
            return;
        }
        let was_active = idx == self.active;
        let d = self.docs.remove(idx);
        if let document::View::Terminal(id) = d.view {
            self.term.close(id);
        }
        if let Some(p) = d.path {
            self.closed.push(p);
        }
        if was_active {
            // next tab of the same panel (the one on the right, else on the left)
            let same = self.docs.iter().enumerate().filter(|(_, x)| x.pane == d.pane).map(|(j, _)| j);
            self.active = same.min_by_key(|&j| (j < idx, j.abs_diff(idx))).unwrap_or(idx.min(self.docs.len().saturating_sub(1)));
        } else if idx < self.active {
            self.active -= 1;
        }
    }
}
