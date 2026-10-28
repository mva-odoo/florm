//! Projects: open folders, sessions, switching.

use super::*;

impl App {
    pub(super) fn open_project(&mut self, path: PathBuf) {
        if crate::remote::needs_mount(&path) {
            self.remote_mount(path, false);
            self.reload_plugins();
            return;
        }
        self.set_root(path);
        self.restore_session();
    }

    pub(super) fn switch_project(&mut self, path: PathBuf) {
        // a project over SSH: its host is mounted first (an empty folder otherwise)
        if crate::remote::needs_mount(&path) {
            self.remote_mount(path, true);
            return;
        }
        let path = crate::os::canon(&path);
        if self.root.as_ref() == Some(&path) {
            return;
        }
        if !path.is_dir() {
            self.set_error(tf!("Dossier introuvable : {}", "Folder not found: {}", path.display()));
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
        let pane_of = |d: &Document| if d.pane == document::NO_PANE { 0 } else { d.pane };
        let shown = |i: usize, d: &Document| i == self.active || self.pane_active[pane_of(d) as usize & 1] == Some(d.uid);
        for (i, d) in self.docs.iter().enumerate() {
            // the views come back in their tab (not the terminals: their programs are gone)
            if let Some(view) = self.session_view(d) {
                if i == self.active {
                    active = files.len();
                }
                files.push(SessionFile { path: PathBuf::new(), line: 0, col: 0, backup: None, bookmarks: Vec::new(), view: Some(view), group: d.group.clone(), pane: pane_of(d), shown: shown(i, d) });
                continue;
            }
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
            files.push(SessionFile { path, line, col, backup, bookmarks: d.bookmark_lines(), view: None, group: d.group.clone(), pane: pane_of(d), shown: shown(i, d) });
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
            breakpoints: self.debug_session_breakpoints(),
            split: self.is_split().then(|| if self.split_dir == SplitDir::Down { "down" } else { "right" }.to_string()),
            bottom: self.bottom.map(|b| b.key()),
            sidebar_hidden: !self.show_sidebar,
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
        self.debug_set_breakpoints(Vec::new());
        let Some(root) = self.root.clone() else { return };
        let Some(s) = project::load_session(&root) else { return };
        self.debug_set_breakpoints(s.breakpoints.clone());
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
            if let Some(view) = &f.view {
                let ok = self.restore_view(view);
                if let (true, Some(d)) = (ok, self.docs.get_mut(self.active)) {
                    d.group = f.group.clone();
                    d.pane = f.pane.min(1);
                }
                opened.push(ok.then_some(self.active));
                continue;
            }
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
                self.docs[i].group = f.group.clone();
                self.docs[i].pane = f.pane.min(1);
                opened.push(Some(i));
            } else {
                opened.push(None);
            }
        }
        // the split, the tab shown in each pane, the panels
        if s.split.is_some() && self.is_split() {
            self.split_dir = if s.split.as_deref() == Some("down") { SplitDir::Down } else { SplitDir::Right };
            for (f, i) in s.files.iter().zip(&opened) {
                if let (true, Some(d)) = (f.shown, i.and_then(|i| self.docs.get(i))) {
                    self.pane_active[d.pane as usize & 1] = Some(d.uid);
                }
            }
        } else {
            self.unsplit();
        }
        if let Some(Some(i)) = opened.get(s.active) {
            self.active = *i;
            self.focused_pane = self.docs.get(*i).map_or(0, |d| d.pane);
        }
        self.show_sidebar = !s.sidebar_hidden;
        if let Some(b) = s.bottom.as_deref().and_then(BottomTab::from_key) {
            self.show_bottom(b, false);
            // back where it was, without taking the keyboard from the editor
            self.search.focus = false;
            self.term.want_focus = false;
        }
        if restored > 0 {
            self.set_status(tf!("{restored} fichier(s) avec des modifications non enregistrées restauré(s)", "{restored} file(s) with unsaved changes restored"));
        }
    }

    /// What a view tab is called in the session.
    fn session_view(&self, d: &Document) -> Option<String> {
        use document::View;
        Some(match d.view {
            View::Claude => format!("claude:{}", self.claude_tab_key(d.uid)),
            View::Docker => "docker".into(),
            View::PullRequests => "pulls".into(),
            View::Debug => "debug".into(),
            View::Search => "search".into(),
            View::Help => "help".into(),
            View::Welcome => "welcome".into(),
            View::Git => "git".into(),
            View::Settings => "settings".into(),
            // a split is not kept between sessions
            View::Split => return None,
            View::Text | View::Image | View::Preview(_) | View::Terminal(_) => return None,
        })
    }

    /// Opens again a view tab of the session; false when it is unknown.
    fn restore_view(&mut self, view: &str) -> bool {
        match view.split_once(':').unwrap_or((view, "")) {
            ("claude", key) => self.claude_restore_tab(key.parse().unwrap_or(0)),
            ("docker", _) => self.open_docker(),
            ("pulls", _) => self.open_pulls(),
            ("debug", _) => self.open_debug(),
            ("git", _) => self.open_git_view(),
            ("settings", _) => self.open_settings(),
            ("search", _) => self.open_search_tab(),
            ("help", _) => self.open_view_tab(document::View::Help, t!("Guide de Florm", "Florm guide").into()),
            ("welcome", _) => self.open_view_tab(document::View::Welcome, t!("Accueil", "Welcome").into()),
            _ => return false,
        }
        true
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
            self.set_error(tf!("Déplacement impossible : {} existe déjà", "Cannot move: {} already exists", dest.display()));
            return;
        }
        match std::fs::rename(&src, &dest) {
            Ok(()) => {
                self.relocate_docs(&src, &dest);
                self.log(tf!("Déplacé : {} → {}", "Moved: {} → {}", src.display(), dest.display()));
                self.set_status(tf!("Déplacé vers {}", "Moved to {}", dir.display()));
                self.exec_refresh_tree();
            }
            Err(e) => self.set_error(tf!("Déplacement impossible : {e}", "Cannot move: {e}")),
        }
    }

    /// Put a file or folder of the tree on the system clipboard (as the file manager does).
    pub(super) fn copy_entry(&mut self, path: &Path) {
        match arboard::Clipboard::new().and_then(|mut c| c.set().file_list(&[path])) {
            Ok(()) => self.set_status(tf!("Copié : {}", "Copied: {}", path.display())),
            Err(e) => self.set_error(tf!("Copie impossible : {e}", "Cannot copy: {e}")),
        }
    }

    /// Paste the files copied in the file manager (or in the tree) into `dir`. A name already taken
    /// becomes "name (copie).ext" instead of replacing the file.
    pub(super) fn paste_into(&mut self, dir: PathBuf) {
        let files = match arboard::Clipboard::new().and_then(|mut c| c.get().file_list()) {
            Ok(f) if !f.is_empty() => f,
            _ => {
                self.set_error(t!("Aucun fichier copié : copie-en un dans le gestionnaire de fichiers", "No file copied: copy one in the file manager"));
                return;
            }
        };
        let (mut done, mut first) = (0, None);
        for src in files {
            let Some(name) = src.file_name() else { continue };
            let dest = free_name(&dir, name);
            // a folder into itself would never end
            if src.is_dir() && dest.starts_with(&src) {
                self.set_error(tf!("Impossible de copier {} dans lui-même", "Cannot copy {} into itself", src.display()));
                continue;
            }
            match copy_recursive(&src, &dest) {
                Ok(()) => {
                    done += 1;
                    first.get_or_insert(dest);
                }
                Err(e) => self.set_error(tf!("Copie de {} impossible : {e}", "Cannot copy {}: {e}", src.display())),
            }
        }
        if done > 0 {
            self.log(tf!("Collé {done} élément(s) dans {}", "Pasted {done} item(s) into {}", dir.display()));
            self.set_status(tf!("{done} élément(s) collé(s) dans {}", "{done} item(s) pasted into {}", dir.display()));
            if let (Some(tree), Some(p)) = (self.tree.as_mut(), first) {
                tree.reveal(&p);
                self.tree_scroll = Some(p);
            }
            self.exec_refresh_tree();
        }
    }

    /// `git init` in a folder of the tree, then it is known as a repository of the project.
    pub(super) fn init_repo(&mut self, dir: PathBuf) {
        let Some(root) = self.root.clone() else { return };
        self.log("$ git init".to_string());
        self.set_status(t!("git init…", "git init…"));
        self.jobs.spawn(move || {
            let result = git::run(&dir, &["init"]);
            JobResult::RepoInit { root: root.clone(), dir, result, repos: git::find_repos(&root, 3) }
        });
    }

    /// The repositories found in the opened folder: the one of the active file (else the first) is selected.
    pub(super) fn apply_repos(&mut self, root: PathBuf, repos: Vec<PathBuf>) {
        if self.root.as_ref() != Some(&root) {
            return;
        }
        self.repos = repos;
        let active = self.docs.get(self.active).and_then(|d| d.path.clone());
        let pick = active.as_deref().and_then(|p| self.repo_for(p)).or_else(|| self.repos.first().cloned());
        if let Some(r) = pick {
            self.select_repo(r);
        }
        // git marks of the files already open
        for i in 0..self.docs.len() {
            if self.docs[i].path.is_some() {
                self.analyze(i, false);
            }
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

/// `dir/name`, or `dir/name (copie).ext`, `dir/name (copie 2).ext`… when it exists.
fn free_name(dir: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let first = dir.join(name);
    if first.symlink_metadata().is_err() {
        return first;
    }
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..)
        .map(|k| {
            let copy = if k == 1 { t!("copie", "copy").to_string() } else { format!("{} {k}", t!("copie", "copy")) };
            dir.join(format!("{stem} ({copy}){ext}"))
        })
        .find(|p| p.symlink_metadata().is_err())
        .expect("endless range")
}

fn copy_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dest)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            copy_recursive(&e.path(), &dest.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dest).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_name_never_replaces_a_file() {
        let dir = std::env::temp_dir().join(format!("florm_paste_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let name = std::ffi::OsStr::new("a.txt");
        assert_eq!(free_name(&dir, name), dir.join("a.txt"));
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let second = free_name(&dir, name);
        assert_ne!(second, dir.join("a.txt"));
        std::fs::write(&second, "y").unwrap();
        assert_ne!(free_name(&dir, name), second);
        let src = dir.join("d");
        std::fs::create_dir_all(src.join("s")).unwrap();
        std::fs::write(src.join("s/f"), "z").unwrap();
        copy_recursive(&src, &dir.join("d2")).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("d2/s/f")).unwrap(), "z");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
