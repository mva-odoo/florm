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
            self.set_status(format!("Dossier introuvable : {}", path.display()));
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
            self.log(format!("Préférences non enregistrées : {e}"));
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
            self.set_status("Enregistre ou ferme d'abord les fichiers modifiés");
            return;
        }
        self.save_session();
        self.docs.clear();
        self.root = None;
        self.repo = None;
        self.repos.clear();
        self.history = HistoryUi::default();
        self.tree = None;
        self.index.files.clear();
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
            self.set_status(format!("{restored} fichier(s) avec des modifications non enregistrées restauré(s)"));
        }
    }

    pub(super) fn projects_ui(&mut self, ui: &mut egui::Ui) {
        let (mut switch, mut remove, mut rename) = (None, None, None);
        ui.horizontal(|ui| {
            if ui.button("Ouvrir un dossier…").clicked() {
                self.pending.push(Cmd::OpenFolder);
            }
            if self.root.is_some() && ui.button("Fermer").on_hover_text("Fermer le dossier courant").clicked() {
                self.pending.push(Cmd::CloseFolder);
            }
        });
        ui.separator();
        if self.projects.is_empty() {
            ui.label(RichText::new("Aucun projet : ouvre un dossier.").weak());
        }
        egui::ScrollArea::vertical().id_salt("projects").auto_shrink([false, false]).show(ui, |ui| {
            for p in &self.projects {
                let current = self.root.as_ref() == Some(&p.path);
                let exists = p.path.is_dir();
                let mut name = RichText::new(&p.name).strong();
                if !exists {
                    name = name.strikethrough().weak();
                }
                let r = ui.selectable_label(current, name).on_hover_text("Clic : ouvrir · Clic droit : options");
                ui.label(RichText::new(p.path.display().to_string()).weak().small());
                if r.clicked() && !current && exists {
                    switch = Some(p.path.clone());
                }
                r.context_menu(|ui| {
                    if ui.button("Renommer…").clicked() {
                        rename = Some(p.clone());
                        ui.close_menu();
                    }
                    if ui.button("Retirer de la liste").clicked() {
                        remove = Some(p.path.clone());
                        ui.close_menu();
                    }
                    if ui.button("Copier le chemin").clicked() {
                        ui.ctx().output_mut(|o| o.copied_text = p.path.display().to_string());
                        ui.close_menu();
                    }
                });
                ui.add_space(4.0);
            }
        });
        if let Some(p) = switch {
            self.switch_project(p);
        }
        if let Some(p) = remove {
            self.projects.retain(|x| x.path != p);
            project::save_list(&self.projects);
        }
        if let Some(p) = rename {
            self.open_overlay_with(Mode::Prompt(Prompt::RenameProject(p.path)), Vec::new(), p.name);
        }
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
            self.set_status(format!("{} existe déjà", dest.display()));
            return;
        }
        match std::fs::rename(&src, &dest) {
            Ok(()) => {
                self.relocate_docs(&src, &dest);
                self.log(format!("Déplacé : {} → {}", src.display(), dest.display()));
                self.set_status(format!("Déplacé vers {}", dir.display()));
                self.exec_refresh_tree();
            }
            Err(e) => self.set_status(format!("Déplacement impossible : {e}")),
        }
    }

    pub(super) fn refresh_index(&mut self) {
        let Some(root) = self.root.clone() else { return };
        self.index.files_at = Some(Instant::now());
        let ex = crate::walk::Exclusions::new(&root, &self.settings.exclude);
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
            JobResult::FileIndex(root, v)
        });
    }
}
