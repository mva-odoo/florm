//! Results of the jobs on the files: lint, spelling, index, search, file dialogs.

use super::*;
use crate::lint::Diagnostic;

impl App {
    /// The lint and git marks of a document, analysed in the background.
    pub(super) fn on_analyzed(&mut self, uid: u64, diags: Option<(Vec<Diagnostic>, String)>, base: Option<String>) {
        if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
            if let Some((dg, linter)) = diags {
                d.set_lint_diagnostics(dg);
                d.lint_version = d.version;
                if lint::lang_of(d.path.as_deref().unwrap_or(Path::new(""))) == Some("python") {
                    self.python_linter = Some(linter);
                }
            }
            d.git_base = base;
            d.refresh_diff();
        }
    }

    /// A lint run done: its diagnostics, if the text did not change meanwhile.
    pub(super) fn on_lint(&mut self, uid: u64, version: u64, diags: Vec<Diagnostic>, linter: String) {
        self.lint_inflight.remove(&uid);
        if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
            if d.version == version {
                d.set_lint_diagnostics(diags);
                d.lint_version = version;
            }
        }
        self.python_linter = Some(linter);
    }

    /// The files matching the query of the quick open (Ctrl+P).
    pub(super) fn on_file_matches(&mut self, gen: u64, query: String, matched: Vec<usize>, top: Vec<usize>) {
        if let Some(o) = self.overlay.as_mut().filter(|o| o.gen.load(Ordering::SeqCst) == gen) {
            o.matched = Some((query, Arc::new(matched)));
            o.filtered = top;
            o.selected = 0;
            o.pending = false;
        }
    }

    /// The files of the project listed: the index of symbols follows.
    pub(super) fn on_file_index(&mut self, root: PathBuf, list: Option<crate::filelist::FileList>) {
        if self.root.as_ref() == Some(&root) && self.index.symbols_for.as_ref() != Some(&root) {
            self.index.symbols_for = Some(root.clone());
            let r = root.clone();
            // the repositories being worked on first
            let mut first: Vec<PathBuf> = self.docs.iter().filter_map(|d| d.path.as_deref()).filter_map(|p| self.repo_for(p)).collect();
            first.extend(self.repo.clone());
            let ex = crate::walk::Exclusions::new(&root, &self.index_exclude());
            let max = self.settings.index_max_files;
            let send = self.jobs.sender();
            self.jobs.spawn(move || {
                let r2 = r.clone();
                // the index of the last run: the files that did not change are not read again
                let cache_file = crate::index::cache_path(&r);
                let old = crate::index::Index::load(&cache_file);
                let idx = crate::index::Index::build_in_steps(&r, &first, &ex, max, old.as_ref(), move |part| send(JobResult::SymbolIndex(r2, part, true)));
                drop(old);
                // a cache that cannot be written only makes the next start slower
                let _ = idx.save(&cache_file);
                JobResult::SymbolIndex(r, idx, false)
            });
        }
        if let Some(files) = list.filter(|_| self.root.as_ref() == Some(&root)) {
            self.index.files = Arc::new(files);
            // "go to file" opened while the list was being made: show the new list
            if self.overlay.as_ref().is_some_and(|o| matches!(o.mode, Mode::Files)) {
                let (items, boost) = (self.file_items(), self.recent_boost());
                if let Some(o) = self.overlay.as_mut() {
                    o.items = items;
                    o.boost = boost;
                    o.last_query = None;
                    o.matched = None;
                }
            }
        }
    }

    /// The misspelled words of a document.
    pub(super) fn on_spelled(&mut self, uid: u64, version: u64, list: Vec<crate::spell::Misspelled>) {
        if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
            if d.version == version {
                d.spelling = list;
            } else if d.spell_version == version {
                // changed meanwhile: checked again
                d.spell_version = u64::MAX;
            }
        }
    }

    /// Files or folders chosen in the dialog of the system.
    pub(super) fn on_picked(&mut self, kind: crate::os::Pick, paths: Result<Vec<PathBuf>, String>) {
        self.dialog_open = false;
        let paths = paths.unwrap_or_else(|e| {
            self.set_error(tf!("Sélecteur de fichiers : {e}", "File chooser: {e}"));
            Vec::new()
        });
        let prompt = match kind {
            crate::os::Pick::Folder => Some(Prompt::OpenFolder),
            crate::os::Pick::File => Some(Prompt::OpenFile),
            crate::os::Pick::SaveAs => Some(Prompt::SaveAs),
            crate::os::Pick::Image | crate::os::Pick::ClaudeDir | crate::os::Pick::AccountProject | crate::os::Pick::AccountConfig | crate::os::Pick::SkillsDir => None,
        };
        match prompt {
            Some(prompt) => {
                if let Some(p) = paths.first() {
                    self.submit_prompt(prompt, p.display().to_string());
                }
            }
            None if matches!(kind, crate::os::Pick::AccountProject | crate::os::Pick::AccountConfig) => {
                if let Some(p) = paths.first() {
                    self.claude_account_picked(kind, p.clone());
                }
            }
            None if kind == crate::os::Pick::SkillsDir => {
                if let Some(p) = paths.first() {
                    self.skills_dir_set(p.display().to_string());
                }
            }
            None if kind == crate::os::Pick::ClaudeDir => {
                if let Some(p) = paths.first() {
                    self.claude_set_dir(p.clone());
                }
            }
            // images: all the ones chosen go with the next message
            None => paths.iter().for_each(|p| self.claude_attach_image_file(p)),
        }
    }

    /// The index of symbols of the project (in steps while it is built).
    pub(super) fn on_symbol_index(&mut self, root: PathBuf, idx: crate::index::Index, partial: bool) {
        if self.root.as_ref() == Some(&root) {
            let what = if partial { t!("Dépôts en cours indexés", "Current repositories indexed") } else { t!("Projet indexé", "Project indexed") };
            self.log(tf!("{what} : {} symboles dans {} fichiers", "{what}: {} symbols in {} files", idx.len(), idx.file_count()));
            let old = self.index.symbols.replace(idx);
            // freeing the previous index and giving the memory back take a moment: not here
            std::thread::spawn(move || {
                drop(old);
                crate::os::release_memory();
            });
        }
    }

    /// The results of the search in the folder.
    pub(super) fn on_search(&mut self, gen: u64, out: search::Outcome) {
        if gen == self.search.gen.load(Ordering::SeqCst) {
            let s = &mut self.search;
            s.running = false;
            s.error = out.error;
            s.stats = Some((out.scanned, out.millis, out.truncated));
            s.files = out.files;
            s.colors.clear();
            s.syntaxes.clear();
            s.collapsed.clear();
            s.rebuild_rows();
            s.cursor = None;
        }
    }
}
