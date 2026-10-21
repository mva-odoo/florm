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
        if self.settings.format_on_save && doc.path.as_deref().and_then(lint::lang_of) == Some("python") {
            let path = doc.path.clone().unwrap();
            let root = self.repo_for(&path).or_else(|| self.root.clone());
            let doc = &mut self.docs[idx];
            match lint::ruff_rewrite(&path, &doc.rope.to_string(), root.as_deref(), lint::Rewrite::Format, None) {
                Ok(text) => {
                    doc.replace_all(&text);
                }
                Err(e) => self.set_status(format!("Pas formaté : {e}")),
            }
        }
        let doc = &mut self.docs[idx];
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

    /// Formats (or fixes) the active Python file with ruff; with a selection, `Format` only
    /// formats its lines (useful where the project is not formatted with ruff, like Odoo).
    pub(super) fn python_rewrite(&mut self, what: lint::Rewrite) {
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(path) = d.path.clone().filter(|p| lint::lang_of(p) == Some("python")) else {
            self.set_status("Formatage : fichiers Python seulement");
            return;
        };
        if d.read_only {
            return;
        }
        let s = d.primary_sel();
        let lines = (what == lint::Rewrite::Format && !s.is_empty()).then(|| {
            let a = d.rope.char_to_line(s.start());
            let mut b = d.rope.char_to_line(s.end());
            if b > a && s.end() == d.rope.line_to_char(b) {
                b -= 1;
            }
            (a + 1, b + 1)
        });
        let root = self.repo_for(&path).or_else(|| self.root.clone());
        let text = d.rope.to_string();
        match lint::ruff_rewrite(&path, &text, root.as_deref(), what, lines) {
            Ok(new) => {
                let changed = self.docs[self.active].replace_all(&new);
                self.set_status(match (changed, what, lines) {
                    (false, _, _) => "Rien à changer".to_string(),
                    (true, lint::Rewrite::Fix, _) => "Corrigé (ruff --fix)".to_string(),
                    (true, _, Some((a, b))) => format!("Lignes {a} à {b} formatées (ruff format)"),
                    (true, _, None) => "Fichier formaté (ruff format)".to_string(),
                });
            }
            Err(e) => self.set_status(e),
        }
    }

    fn after_save(&mut self, idx: usize, path: &Path) {
        if let (Some(ix), Some(d)) = (self.index.symbols.as_mut(), self.docs.get(idx)) {
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

    pub(super) fn lint_config(&self) -> lint::LintConfig {
        lint::LintConfig { python_linter: self.settings.python_linter.clone(), python_args: self.settings.python_lint_args.clone() }
    }

    /// Lints the active Python buffer while typing (debounced, one run at a time per document).
    pub(super) fn update_live_lint(&mut self, ctx: &egui::Context) {
        if !self.settings.lint_live {
            return;
        }
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(path) = d.path.clone() else { return };
        if lint::lang_of(&path) != Some("python") || d.lint_version == d.version || self.lint_inflight.contains(&d.uid) {
            return;
        }
        let wait = Duration::from_millis(500);
        let el = d.last_change.elapsed();
        if el < wait && d.lint_version != u64::MAX {
            ctx.request_repaint_after(wait - el);
            return;
        }
        let (uid, version, text) = (d.uid, d.version, d.rope.to_string());
        let root = self.repo_for(&path).or_else(|| self.root.clone());
        let cfg = self.lint_config();
        self.lint_inflight.insert(uid);
        self.jobs.spawn(move || {
            let (diags, linter) = lint::lint_python(&path, &text, root.as_deref(), &cfg);
            JobResult::Lint { uid, version, diags, linter }
        });
    }
}
