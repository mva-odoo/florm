//! Language servers: completion, definition, rename.

use super::*;

impl App {
    /// Server for document `i` (started if needed) and its language id; sends the latest text.
    pub(super) fn lsp_for(&mut self, i: usize, ctx: &egui::Context, force_sync: bool) -> Option<(usize, &'static str)> {
        if !self.settings.lsp {
            return None;
        }
        let d = self.docs.get(i)?;
        if d.view != document::View::Text {
            return None;
        }
        let path = d.path.clone()?;
        let (family, lang) = lsp::language_of(&path)?;
        let root = self.repo_for(&path).or_else(|| self.root.clone()).or_else(|| path.parent().map(Path::to_path_buf))?;
        let ci = match self.lsp.ensure(family, &root, &self.settings.lsp_servers, ctx) {
            Ok(ci) => ci,
            Err(e) => {
                if !e.is_empty() {
                    self.lsp_note.insert(family.to_string(), e);
                }
                return None;
            }
        };
        let d = &self.docs[i];
        let sent = self.lsp.client(ci).and_then(|c| c.opened.get(&d.uid).map(|o| o.0));
        // while typing, the whole text is sent at most every 200 ms
        if force_sync || sent.is_none() || (sent != Some(d.version) && d.last_change.elapsed() > Duration::from_millis(200)) {
            let (uid, version) = (d.uid, d.version);
            let rope = &self.docs[i].rope;
            if let Some(c) = self.lsp.client(ci) {
                c.sync(uid, &path, lang, version, || rope.to_string());
            }
        }
        Some((ci, lang))
    }

    pub(super) fn lsp_frame(&mut self, ctx: &egui::Context) {
        for (ci, msg) in self.lsp.poll() {
            match msg {
                lsp::Incoming::Response { id, result } => {
                    let Some(req) = self.lsp_req.remove(&(ci, id)) else { continue };
                    match result {
                        Ok(v) => self.lsp_response(ci, req, v, ctx),
                        Err(e) => self.set_status(format!("Serveur de langage : {e}")),
                    }
                }
                lsp::Incoming::Exited(name) => {
                    self.set_status(format!("Le serveur de langage {name} s'est arrêté (LSP : redémarrer pour réessayer)"));
                }
            }
        }
        if self.active < self.docs.len() {
            self.lsp_for(self.active, ctx, false);
        }
        // closed tabs: didClose
        let alive: HashSet<u64> = self.docs.iter().map(|d| d.uid).collect();
        for c in self.lsp.clients.iter_mut().flatten() {
            c.close_missing(&alive);
        }
        if self.complete_at.is_some_and(|t| Instant::now() >= t) {
            self.complete_at = None;
            if self.completion.is_none() {
                self.request_completion(ctx);
            }
        } else if let Some(t) = self.complete_at {
            ctx.request_repaint_after(t.saturating_duration_since(Instant::now()));
        }
        self.completion_filter();
    }

    pub(super) fn lsp_text_position(&self, i: usize) -> Option<(serde_json::Value, serde_json::Value)> {
        let d = self.docs.get(i)?;
        let uri = lsp::path_to_uri(d.path.as_ref()?);
        Some((serde_json::json!({ "uri": uri }), lsp::to_position(&d.rope, d.primary_sel().head)))
    }

    /// Start of the identifier before the cursor.
    pub(super) fn word_start(d: &Document) -> usize {
        let head = d.primary_sel().head;
        let mut p = head;
        while p > 0 && d.char_at(p - 1).is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$') {
            p -= 1;
        }
        p
    }

    pub(super) fn request_completion(&mut self, ctx: &egui::Context) {
        let i = self.active;
        let Some((ci, _)) = self.lsp_for(i, ctx, true) else {
            self.local_completion(true);
            return;
        };
        let Some((doc, pos)) = self.lsp_text_position(i) else { return };
        let d = &self.docs[i];
        let (uid, start) = (d.uid, Self::word_start(d));
        if let Some(c) = self.lsp.client(ci) {
            let id = c.request("textDocument/completion", serde_json::json!({ "textDocument": doc, "position": pos }));
            self.lsp_req.insert((ci, id), LspReq::Completion { uid, start });
        }
    }

    /// Word to look up for "go to definition": the identifier at the cursor, or a quoted dotted
    /// name ('sale.order', 'module.xml_id') when the cursor is in one.
    pub(super) fn definition_word(d: &Document) -> String {
        let head = d.primary_sel().head;
        let line = d.rope.char_to_line(head);
        let text = d.line_text(line);
        let col = head - d.rope.line_to_char(line);
        let chars: Vec<char> = text.chars().collect();
        let quote = |c: char| c == '\'' || c == '"';
        let (mut a, mut b) = (col.min(chars.len()), col.min(chars.len()));
        while a > 0 && !quote(chars[a - 1]) && !chars[a - 1].is_whitespace() {
            a -= 1;
        }
        while b < chars.len() && !quote(chars[b]) && !chars[b].is_whitespace() {
            b += 1;
        }
        let quoted: String = chars[a..b].iter().collect();
        if a > 0 && b < chars.len() && quote(chars[a - 1]) && quote(chars[b]) && quoted.contains('.')
            && quoted.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            return quoted;
        }
        let (ws, we) = d.word_at(head);
        d.slice(ws, we)
    }

    pub(super) fn goto_definition(&mut self, ctx: &egui::Context) {
        let i = self.active;
        let Some(d) = self.docs.get(i) else { return };
        let word = Self::definition_word(d);
        if word.trim().is_empty() {
            return;
        }
        match self.lsp_for(i, ctx, true) {
            Some((ci, _)) => {
                let Some((doc, pos)) = self.lsp_text_position(i) else { return };
                if let Some(cl) = self.lsp.client(ci) {
                    let id = cl.request("textDocument/definition", serde_json::json!({ "textDocument": doc, "position": pos }));
                    self.lsp_req.insert((ci, id), LspReq::Definition { word });
                }
            }
            None => self.index_definition(&word),
        }
    }

    /// Native dialog of the system (Windows Explorer, macOS Finder) to choose a folder (0), a file
    /// to open (1) or where to save (2). Returns false where Florm uses its own path entry (Linux).
    #[cfg(any(windows, target_os = "macos"))]
    pub(super) fn native_dialog(&mut self, kind: u8) -> bool {
        self.file_dialog(kind);
        true
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub(super) fn native_dialog(&mut self, _kind: u8) -> bool {
        false
    }

    /// File chooser of the system (Explorer, Finder, the desktop's one through the xdg portal on
    /// Linux) for a folder (0), a file to open (1) or where to save (2).
    pub(super) fn file_dialog(&mut self, kind: u8) {
        if self.dialog_open {
            return;
        }
        self.dialog_open = true;
        let current = self.docs.get(self.active).and_then(|d| d.path.clone());
        let dir = current
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| self.root.clone())
            .unwrap_or_else(config::home);
        let name = current.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
        self.jobs.spawn(move || {
            #[cfg(target_os = "linux")]
            let picked = crate::os::portal_pick(kind, &dir, name.as_deref().filter(|_| kind == 2));
            #[cfg(not(target_os = "linux"))]
            let picked = {
                let d = rfd::FileDialog::new().set_directory(&dir);
                Ok(match kind {
                    0 => d.set_title("Ouvrir un dossier (les projets WSL : \\\\wsl.localhost\\…)").pick_folder(),
                    1 => d.set_title("Ouvrir un fichier").pick_file(),
                    _ => {
                        let d = d.set_title("Enregistrer sous");
                        match name {
                            Some(n) => d.set_file_name(n).save_file(),
                            None => d.save_file(),
                        }
                    }
                })
            };
            JobResult::Picked(kind, picked)
        });
    }

    /// Shows `path` in the file tree: sidebar opened, folders expanded, entry scrolled into view.
    pub(super) fn reveal_in_tree(&mut self, path: &Path) {
        let Some(tree) = self.tree.as_mut() else {
            self.set_status("Aucun dossier ouvert");
            return;
        };
        let path = crate::os::canon(path);
        if !tree.reveal(&path) {
            self.set_status("Ce fichier n'est pas dans le dossier ouvert");
            return;
        }
        self.show_sidebar = true;
        self.side_tab = SideTab::Files;
        self.tree_nav.cursor = Some(path.clone());
        self.tree_scroll = Some(path);
    }
}
