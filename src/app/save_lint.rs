//! Saving and linting documents.

use super::*;

impl App {
    /// Saves by itself the files changed (setting `auto_save`): "focus" when Florm loses the
    /// focus or another tab is shown, "delay" a second after the last keystroke. Not the new files
    /// (no name yet), nor those changed on disk meanwhile, nor formatted with ruff while typing.
    pub(super) fn auto_save(&mut self, ctx: &egui::Context) {
        let mode = self.settings.auto_save.as_str();
        if mode != "focus" && mode != "delay" {
            return;
        }
        let focused = ctx.input(|i| i.focused);
        let shown = self.docs.get(self.active).map_or(0, |d| d.uid);
        let (was_focused, was_shown) = std::mem::replace(&mut self.auto_save_seen, (focused, shown));
        let delay = std::time::Duration::from_secs(1);
        let due: Vec<usize> = (0..self.docs.len())
            .filter(|&i| {
                let d = &self.docs[i];
                let ok = d.is_dirty() && d.path.is_some() && !d.read_only && d.disk_change != Some(document::DiskChange::Changed);
                ok && match mode {
                    "focus" => (was_focused && !focused) || (was_shown != shown && d.uid == was_shown),
                    _ => d.last_change.elapsed() >= delay,
                }
            })
            .collect();
        if mode == "delay" && self.docs.iter().any(|d| d.is_dirty() && d.path.is_some()) {
            ctx.request_repaint_after(delay);
        }
        // while typing, the trailing spaces stay (the space just typed would go)
        let format = std::mem::replace(&mut self.settings.format_on_save, false);
        let trim = self.settings.trim_trailing_whitespace_on_save;
        self.settings.trim_trailing_whitespace_on_save = trim && mode == "focus";
        for i in due {
            self.save(i);
        }
        self.settings.format_on_save = format;
        self.settings.trim_trailing_whitespace_on_save = trim;
    }

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
        // another program changed the file: saving would lose its version without a word
        if doc.disk_change == Some(document::DiskChange::Changed) {
            self.active = idx;
            self.confirm = Some(Confirm::Overwrite(doc.uid));
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
                Err(e) => self.set_error(tf!("Pas formaté : {e}", "Not formatted: {e}")),
            }
        }
        let doc = &mut self.docs[idx];
        // the version replaced, and the new one, in the local history
        if let Some(p) = &doc.path {
            crate::local_history::keep(p);
        }
        match doc.save(trim) {
            Ok(()) => {
                let path = doc.path.clone().unwrap();
                crate::local_history::keep(&path);
                self.set_status(tf!("Enregistré : {}", "Saved: {}", path.display()));
                self.after_save(idx, &path);
                true
            }
            Err(e) => {
                self.set_error(tf!("Enregistrement impossible : {e}", "Cannot save: {e}"));
                false
            }
        }
    }

    /// Ctrl+S: saved, formatted first with ruff off the UI thread when `format_on_save` asks for
    /// it (saved when ruff answers). Closing and quitting go through `save`, which waits.
    pub(super) fn save_formatted(&mut self, idx: usize) {
        let Some(d) = self.docs.get(idx) else { return };
        let python = d.path.as_deref().and_then(lint::lang_of) == Some("python");
        if !(self.settings.format_on_save && python && !d.read_only && d.disk_change != Some(document::DiskChange::Changed)) {
            self.save(idx);
            return;
        }
        self.ruff_job(idx, lint::Rewrite::Format, None, true);
    }

    /// ruff run on document `idx` in a job: `on_rewritten` applies its text.
    fn ruff_job(&mut self, idx: usize, what: lint::Rewrite, lines: Option<(usize, usize)>, then_save: bool) {
        let Some(d) = self.docs.get(idx) else { return };
        let Some(path) = d.path.clone() else { return };
        let root = self.repo_for(&path).or_else(|| self.root.clone());
        let (uid, version, text) = (d.uid, d.version, d.rope.to_string());
        self.jobs.spawn(move || JobResult::Rewritten { uid, version, what, lines, then_save, result: lint::ruff_rewrite(&path, &text, root.as_deref(), what, lines) });
    }

    /// ruff answered: its text put in the document if nothing was typed meanwhile, then saved
    /// (Ctrl+S), formatted or not.
    pub(super) fn on_rewritten(&mut self, uid: u64, version: u64, what: lint::Rewrite, lines: Option<(usize, usize)>, then_save: bool, result: Result<String, String>) {
        let Some(idx) = self.docs.iter().position(|d| d.uid == uid) else { return };
        let fresh = self.docs[idx].version == version;
        let changed = match result {
            Ok(text) if fresh => self.docs[idx].replace_all(&text),
            Ok(_) => false,
            Err(e) => {
                self.set_error(if then_save { tf!("Pas formaté : {e}", "Not formatted: {e}") } else { tf!("ruff : {e}", "ruff: {e}") });
                false
            }
        };
        if then_save {
            let format = std::mem::replace(&mut self.settings.format_on_save, false);
            self.save(idx);
            self.settings.format_on_save = format;
            return;
        }
        if !fresh {
            self.set_status(t!("Pas formaté : le fichier a changé entre-temps", "Not formatted: the file changed meanwhile"));
            return;
        }
        self.set_status(match (changed, what, lines) {
            (false, _, _) => t!("Rien à changer", "Nothing to change").to_string(),
            (true, lint::Rewrite::Fix, _) => t!("Corrigé (ruff --fix)", "Fixed (ruff --fix)").to_string(),
            (true, _, Some((a, b))) => tf!("Lignes {a} à {b} formatées (ruff format)", "Lines {a} to {b} formatted (ruff format)"),
            (true, _, None) => t!("Fichier formaté (ruff format)", "File formatted (ruff format)").to_string(),
        });
    }

    /// Formats (or fixes) the active Python file with ruff; with a selection, `Format` only
    /// formats its lines (useful where the project is not formatted with ruff).
    pub(super) fn python_rewrite(&mut self, what: lint::Rewrite) {
        let Some(d) = self.docs.get(self.active) else { return };
        if d.path.as_deref().and_then(lint::lang_of) != Some("python") {
            self.set_status(t!("Formatage : fichiers Python seulement", "Formatting: Python files only"));
            return;
        }
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
        self.ruff_job(self.active, what, lines, false);
    }

    fn after_save(&mut self, idx: usize, path: &Path) {
        self.ext_event("save", idx);
        if let (Some(ix), Some(d)) = (self.index.symbols.as_mut(), self.docs.get(idx)) {
            ix.update(path, &d.rope.to_string());
        }
        if config::is_settings_file(path) {
            self.reload_settings();
        }
        // a template written here: read again at its next use
        if path.starts_with(config::config_dir().join("templates")) || crate::shared::sub("templates").is_some_and(|d| path.starts_with(d)) {
            self.templates.clear();
        }
        self.analyze(idx, true);
        self.git_refresh_file(path);
        if let Some(t) = self.tree.as_mut() {
            t.saved(path);
        }
    }

    /// Lints (in-process now, external in background), updates git gutter marks.
    pub(super) fn analyze(&mut self, idx: usize, saving: bool) {
        let Some(doc) = self.docs.get_mut(idx) else { return };
        doc.analyzed = true;
        let Some(path) = doc.path.clone() else { return };
        if doc.view != document::View::Text {
            return;
        }
        if let Some(lang) = lint::lang_of(&path).filter(|l| lint::is_in_process(l)) {
            let (uid, version, rope) = (doc.uid, doc.version, doc.rope.clone());
            doc.lint_version = version;
            self.jobs.spawn(move || JobResult::LintedInProcess { uid, version, diags: lint::lint_in_process(lang, &rope.to_string()) });
        }
        let uid = doc.uid;
        let lint_on = self.settings.lint_on_save || !saving;
        let root = self.repo_for(&path).or_else(|| self.root.clone());
        let cfg = self.lint_config(&path);
        let text = self.docs[idx].rope.to_string();
        self.jobs.spawn(move || {
            let diags = if lint_on { lint::lint_external(&path, &text, root.as_deref(), &cfg) } else { None };
            JobResult::Analyzed { uid, diags, base: git::head_text(&path) }
        });
    }

    /// The tabs shown not analyzed yet (opened, or their repository changed meanwhile) are now.
    pub(super) fn analyze_shown(&mut self) {
        for i in 0..self.docs.len() {
            let d = &self.docs[i];
            if !d.analyzed && (i == self.active || self.panes.active.contains(&Some(d.uid))) {
                self.analyze(i, false);
            }
        }
    }

    /// Every tab analyzed again: those shown now, the others once shown.
    pub(super) fn analyze_all_later(&mut self) {
        for d in &mut self.docs {
            d.analyzed = false;
        }
    }

    /// The linters for `path`: the configuration of its project used only in a trusted folder.
    pub(super) fn lint_config(&self, path: &Path) -> lint::LintConfig {
        let s = &self.settings;
        let ignore = [("flake8", &s.lint_ignore_flake8), ("ruff", &s.lint_ignore_ruff), ("pylint", &s.lint_ignore_pylint), ("eslint", &s.lint_ignore_eslint)];
        lint::LintConfig {
            python_linter: s.python_linter.clone(),
            python_args: s.python_lint_args.clone(),
            ignore: ignore.into_iter().filter(|(_, c)| !c.is_empty()).map(|(n, c)| (n.to_string(), c.clone())).collect(),
            trusted: self.trusted(path),
            linters: s.linters.clone(),
        }
    }

    /// Lints the active buffer while typing, Python or a language with a linter (debounced, one run
    /// at a time per document).
    pub(super) fn update_live_lint(&mut self, ctx: &egui::Context) {
        if !self.settings.lint_live {
            return;
        }
        let Some(d) = self.docs.get(self.active) else { return };
        let Some(path) = d.path.clone() else { return };
        if d.lint_version == d.version || self.lint_inflight.contains(&d.uid) {
            return;
        }
        // Python, and the other languages with a linter (on the text typed: their standard input)
        let python = lint::lang_of(&path) == Some("python");
        let generic = if python || !self.trusted(&path) { Vec::new() } else { lint::generic_linters(&path, &self.settings.linters) };
        if !python && generic.is_empty() {
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
        let cfg = self.lint_config(&path);
        self.lint_inflight.insert(uid);
        self.jobs.spawn(move || {
            let (diags, linter) = if python { lint::lint_python(&path, &text, root.as_deref(), &cfg) } else { lint::lint_generic(&path, &text, root.as_deref(), &generic) };
            JobResult::Lint { uid, version, diags, linter }
        });
    }
}
