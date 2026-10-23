//! Projects: open folders, sessions, switching.

use super::*;

impl App {
    pub(super) fn open_project(&mut self, path: PathBuf) {
        self.set_root(path);
        self.restore_session();
    }

    pub(super) fn switch_project(&mut self, path: PathBuf) {
        let path = crate::os::canon(&path);
        if self.root.as_ref() == Some(&path) {
            return;
        }
        if !path.is_dir() {
            self.set_status(tf!("Dossier introuvable : {}", "Folder not found: {}", path.display()));
            return;
        }
        // with hot exit, the unsaved changes stay in the session of the project we leave
        if !self.settings.hot_exit && self.docs.iter().any(|d| d.is_dirty()) {
            self.confirm = Some(Confirm::SwitchProject(path));
            return;
        }
        self.do_switch(path);
    }

    /// Empty search state that keeps the last used filters.
    pub(super) fn fresh_search(&self) -> SearchUi {
        let mut s = SearchUi::default();
        s.q.include = self.state.last_include.clone();
        s.q.exclude = self.state.last_exclude.clone();
        s
    }

    /// Writes one value into settings.toml so UI choices survive a restart.
    pub(super) fn persist(&mut self, key: &str, value: String) {
        if let Err(e) = config::set_setting(key, &value) {
            self.log(tf!("Préférences non enregistrées : {e}", "Preferences not saved: {e}"));
        }
    }

    pub(super) fn do_switch(&mut self, path: PathBuf) {
        self.save_session();
        self.docs.clear();
        self.active = 0;
        self.search.gen.store(u64::MAX, Ordering::SeqCst); // cancel a running search
        self.search = self.fresh_search();
        self.find = FindState::default();
        self.open_project(path);
    }

    pub(super) fn close_folder(&mut self) {
        if self.docs.iter().any(|d| d.is_dirty()) {
            self.set_status(t!("Enregistre ou ferme d'abord les fichiers modifiés", "Save or close the changed files first"));
            return;
        }
        self.save_session();
        self.docs.clear();
        self.root = None;
        self.repo = None;
        self.repos.clear();
        self.history = HistoryUi::default();
        self.tree = None;
        self.index.files = Default::default();
        self.index.file_items = None;
        self.git = GitUi::default();
        self.search.gen.store(u64::MAX, Ordering::SeqCst);
        self.search = self.fresh_search();
        self.reload_plugins();
    }

    /// Session of the project; with hot exit, unsaved contents go in `backups` (file name, text).
    fn current_session(&self) -> (Session, Vec<(String, String)>) {
        let mut files = Vec::new();
        let mut backups = Vec::new();
        let mut active = 0;
        for (i, d) in self.docs.iter().enumerate() {
            if d.read_only {
                continue;
            }
            let dirty = self.settings.hot_exit && d.is_dirty();
            let path = match &d.path {
                Some(p) => p.clone(),
                None if dirty && !d.is_pristine_empty() => PathBuf::new(),
                None => continue,
            };
            if i == self.active {
                active = files.len();
            }
            let (line, col) = d.line_col(d.primary_sel().head);
            let backup = dirty.then(|| format!("doc{}.txt", files.len()));
            if let Some(name) = &backup {
                backups.push((name.clone(), d.rope.to_string()));
            }
            files.push(SessionFile { path, line, col, backup, bookmarks: d.bookmark_lines() });
        }
        let mut expanded = Vec::new();
        if let Some(t) = &self.tree {
            t.expanded_paths(&mut expanded);
        }
        let session = Session {
            active,
            files,
            expanded,
            search_include: self.search.q.include.clone(),
            search_exclude: self.search.q.exclude.clone(),
            recent: self.index.recent.clone(),
        };
        (session, backups)
    }

    pub(super) fn save_session(&mut self) {
        if let Some(r) = &self.root {
            let (session, backups) = self.current_session();
            project::save_session(r, &session);
            project::save_backups(r, &backups);
        }
        self.last_session_save = Instant::now();
    }

    fn restore_session(&mut self) {
        self.index.recent.clear();
        let Some(root) = self.root.clone() else { return };
        let Some(s) = project::load_session(&root) else { return };
        self.index.recent = s.recent.iter().filter(|p| p.is_file()).cloned().collect();
        if !s.search_include.is_empty() {
            self.search.q.include = s.search_include;
        }
        if !s.search_exclude.is_empty() {
            self.search.q.exclude = s.search_exclude;
        }
        if let Some(t) = self.tree.as_mut() {
            let set: HashSet<PathBuf> = s.expanded.into_iter().collect();
            t.expand_paths(&set);
        }
        let mut opened = Vec::new();
        let mut restored = 0;
        for f in &s.files {
            let backup = f.backup.as_deref().and_then(|b| project::load_backup(&root, b));
            let ok = if f.path.as_os_str().is_empty() || (!f.path.is_file() && backup.is_some()) {
                // untitled document, or a file deleted since: recreate it from its backup
                match &backup {
                    Some(_) => {
                        self.new_doc();
                        if !f.path.as_os_str().is_empty() {
                            let d = &mut self.docs[self.active];
                            d.path = Some(f.path.clone());
                            d.syntax = self.hl.detect(Some(&f.path), "");
                        }
                        true
                    }
                    None => false,
                }
            } else {
                f.path.is_file() && self.open_path(f.path.clone())
            };
            if ok {
                let i = self.active;
                if let Some(text) = backup.as_ref().filter(|t| **t != self.docs[i].rope.to_string()) {
                    self.docs[i].set_text(text);
                    restored += 1;
                }
                self.docs[i].goto_line(f.line, f.col);
                self.docs[i].set_bookmark_lines(&f.bookmarks);
                opened.push(Some(i));
            } else {
                opened.push(None);
            }
        }
        if let Some(Some(i)) = opened.get(s.active) {
            self.active = *i;
        }
        if restored > 0 {
            self.set_status(tf!("{restored} fichier(s) avec des modifications non enregistrées restauré(s)", "{restored} file(s) with unsaved changes restored"));
        }
    }

    /// A folder of the tree left out of the index (go to file, F12, find in files) or taken back:
    /// an anchored pattern ("/odoo/16.0/") in the exclusions of the preferences.
    pub(super) fn exclude_folder(&mut self, path: &Path, on: bool) {
        let Some(root) = self.root.clone() else { return };
        let Ok(rel) = path.strip_prefix(&root) else { return };
        let pattern = format!("/{}/", rel.to_string_lossy().replace('\\', "/"));
        let before = self.settings.clone();
        if on {
            if !self.settings.exclude.contains(&pattern) {
                self.settings.exclude.push(pattern.clone());
            }
        } else {
            self.settings.exclude.retain(|p| *p != pattern);
        }
        self.apply_settings_change(&before);
        self.prefs.exclude = self.settings.exclude.join(", ");
        let still = crate::walk::Exclusions::new(&root, &self.settings.exclude).excludes_here_or_above(path, true);
        self.set_status(match (on, still) {
            (true, _) => tf!("{} exclu de l'index ({pattern})", "{} excluded from the index ({pattern})", rel.display()),
            (false, false) => tf!("{} remis dans l'index", "{} put back in the index", rel.display()),
            (false, true) => t!("Toujours exclu par une autre règle : Préférences → Avancé → Fichiers exclus", "Still excluded by another rule: Preferences → Advanced → Excluded files").to_string(),
        });
    }

    pub(super) fn relocate_docs(&mut self, old: &Path, new: &Path) {
        for d in self.docs.iter_mut() {
            if let Some(p) = d.path.clone() {
                if let Ok(rest) = p.strip_prefix(old) {
                    d.path = Some(if rest.as_os_str().is_empty() { new.to_path_buf() } else { new.join(rest) });
                }
            }
        }
    }

    pub(super) fn move_entry(&mut self, src: PathBuf, dir: PathBuf) {
        let Some(name) = src.file_name() else { return };
        let dest = dir.join(name);
        if dest.exists() {
            self.set_status(tf!("{} existe déjà", "{} already exists", dest.display()));
            return;
        }
        match std::fs::rename(&src, &dest) {
            Ok(()) => {
                self.relocate_docs(&src, &dest);
                self.log(tf!("Déplacé : {} → {}", "Moved: {} → {}", src.display(), dest.display()));
                self.set_status(tf!("Déplacé vers {}", "Moved to {}", dir.display()));
                self.exec_refresh_tree();
            }
            Err(e) => self.set_status(tf!("Déplacement impossible : {e}", "Cannot move: {e}")),
        }
    }

    pub(super) fn refresh_index(&mut self) {
        let Some(root) = self.root.clone() else { return };
        self.index.files_at = Some(Instant::now());
        let ex = crate::walk::Exclusions::new(&root, &self.settings.exclude);
        let current = self.index.files.clone();
        self.jobs.spawn(move || {
            let mut v = Vec::new();
            let walker = crate::walk::walker(&root, &ex, Vec::new()).build();
            for e in walker.flatten() {
                if e.file_type().is_some_and(|t| t.is_file()) {
                    if let Ok(rel) = e.path().strip_prefix(&root) {
                        v.push(rel.to_path_buf());
                    }
                    if v.len() >= 1_500_000 {
                        break;
                    }
                }
            }
            // sorted here: the same list compares equal (nothing to redo), and recent files are
            // found by binary search
            v.sort_unstable();
            // a big project has close to a million files: compared, written for go to file and
            // freed here rather than in the interface
            if *current == v {
                return JobResult::FileIndex(root, None);
            }
            let labels = v.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            JobResult::FileIndex(root, Some((v, labels)))
        });
    }
}
