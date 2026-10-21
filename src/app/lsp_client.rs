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
        let ci = match self.lsp.manager.ensure(family, &root, &self.settings.lsp_servers, ctx) {
            Ok(ci) => ci,
            Err(e) => {
                if !e.is_empty() {
                    self.lsp.notes.insert(family.to_string(), e);
                }
                return None;
            }
        };
        let d = &self.docs[i];
        let sent = self.lsp.manager.client(ci).and_then(|c| c.opened.get(&d.uid).map(|o| o.0));
        // while typing, the whole text is sent at most every 200 ms
        if force_sync || sent.is_none() || (sent != Some(d.version) && d.last_change.elapsed() > Duration::from_millis(200)) {
            let (uid, version) = (d.uid, d.version);
            let rope = &self.docs[i].rope;
            if let Some(c) = self.lsp.manager.client(ci) {
                c.sync(uid, &path, lang, version, || rope.to_string());
            }
        }
        Some((ci, lang))
    }

    pub(super) fn lsp_frame(&mut self, ctx: &egui::Context) {
        for (ci, msg) in self.lsp.manager.poll() {
            match msg {
                lsp::Incoming::Response { id, result } => {
                    let Some(req) = self.lsp.requests.remove(&(ci, id)) else { continue };
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
        for c in self.lsp.manager.clients.iter_mut().flatten() {
            c.close_missing(&alive);
        }
        if self.lsp.complete_at.is_some_and(|t| Instant::now() >= t) {
            self.lsp.complete_at = None;
            if self.lsp.completion.is_none() {
                self.request_completion(ctx);
            }
        } else if let Some(t) = self.lsp.complete_at {
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
        if let Some(c) = self.lsp.manager.client(ci) {
            let id = c.request("textDocument/completion", serde_json::json!({ "textDocument": doc, "position": pos }));
            self.lsp.requests.insert((ci, id), LspReq::Completion { uid, start });
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
                if let Some(cl) = self.lsp.manager.client(ci) {
                    let id = cl.request("textDocument/definition", serde_json::json!({ "textDocument": doc, "position": pos }));
                    self.lsp.requests.insert((ci, id), LspReq::Definition { word });
                }
            }
            None => self.index_definition(&word),
        }
    }

    /// Like Sublime Text: definitions of `word` in the project index; one opens, several are listed.
    pub(super) fn index_definition(&mut self, word: &str) {
        let mut found: Vec<(PathBuf, crate::index::Entry)> = match &self.index.symbols {
            Some(ix) => ix.lookup(word),
            None => Vec::new(),
        };
        if found.is_empty() {
            // not indexed yet (or unsaved): the open documents
            for d in self.docs.iter_mut() {
                let Some(p) = d.path.clone() else { continue };
                for s in d.symbols().iter().filter(|s| s.name == word) {
                    found.push((p.clone(), crate::index::Entry { name: s.name.clone(), kind: s.kind, line: s.line, detail: s.detail.clone() }));
                }
            }
        }
        match found.len() {
            0 => {
                let more = if self.index.symbols.is_none() { " (indexation du projet en cours…)" } else { "" };
                self.set_status(format!("Aucune définition de « {word} » trouvée dans le projet{more}"));
            }
            1 => {
                let (p, e) = found.remove(0);
                self.open_at(p, e.line);
            }
            _ => {
                let root = self.root.clone();
                let items = found
                    .into_iter()
                    .map(|(p, e)| {
                        let rel = root.as_ref().and_then(|r| p.strip_prefix(r).ok()).unwrap_or(&p).display().to_string();
                        let detail = if e.detail.is_empty() { format!("{} {}", e.kind.icon(), e.name) } else { format!("{} {} · {}", e.kind.icon(), e.name, e.detail) };
                        Item { label: format!("{rel}:{}", e.line + 1), detail, action: Action::Location(p, e.line) }
                    })
                    .collect();
                self.open_overlay_with(Mode::Definitions(word.to_string()), items, String::new());
            }
        }
    }

    /// Suggestions without a language server, like Sublime Text: words of the open files and
    /// the project's symbols starting like the word being typed.
    pub(super) fn local_completion(&mut self, manual: bool) {
        let Some(d) = self.docs.get(self.active) else { return };
        if d.read_only || d.view != document::View::Text {
            return;
        }
        let (uid, start, head) = (d.uid, Self::word_start(d), d.primary_sel().head);
        let prefix = d.slice(start, head);
        if prefix.chars().count() < if manual { 1 } else { 2 } {
            return;
        }
        let lower = prefix.to_lowercase();
        let mut seen: HashSet<String> = HashSet::new();
        let mut items = Vec::new();
        let mut push = |w: &str, detail: &str, kind: u64| {
            if w != prefix && w.to_lowercase().starts_with(&lower) && seen.insert(w.to_string()) {
                items.push(lsp::CompletionItem {
                    label: w.to_string(),
                    detail: detail.to_string(),
                    kind,
                    filter: w.to_string(),
                    sort: w.to_string(),
                    insert: w.to_string(),
                    range: None,
                });
            }
        };
        static WORD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = WORD.get_or_init(|| regex::Regex::new(r"[A-Za-z_][A-Za-z0-9_]{2,}").unwrap());
        // the current file first, then the other open files
        let order: Vec<usize> = std::iter::once(self.active).chain((0..self.docs.len()).filter(|&k| k != self.active)).collect();
        for k in order {
            let doc = &self.docs[k];
            if doc.view != document::View::Text || doc.rope.len_bytes() > 2_000_000 {
                continue;
            }
            let text = doc.rope.to_string();
            let label = if k == self.active { "" } else { "autre fichier" };
            for m in re.find_iter(&text) {
                push(m.as_str(), label, 1);
            }
        }
        if let Some(ix) = &self.index.symbols {
            for n in ix.names_with_prefix(&prefix, 200) {
                push(n, "symbole du projet", 6);
            }
            if prefix.chars().next().is_some_and(|c| c.is_lowercase()) {
                let cap: String = prefix.chars().take(1).flat_map(char::to_uppercase).chain(prefix.chars().skip(1)).collect();
                for n in ix.names_with_prefix(&cap, 100) {
                    push(n, "symbole du projet", 7);
                }
            }
        }
        if items.is_empty() {
            return;
        }
        self.lsp.completion = Some(CompletionUi { uid, start, items, shown: Vec::new(), selected: 0, incomplete: false, client: usize::MAX });
        self.completion_filter();
    }

    pub(super) fn lsp_response(&mut self, ci: usize, req: LspReq, v: serde_json::Value, ctx: &egui::Context) {
        match req {
            LspReq::Completion { uid, start } => {
                let Some(d) = self.docs.get(self.active).filter(|d| d.uid == uid) else { return };
                if d.primary_sel().head < start {
                    return;
                }
                let (items, incomplete) = lsp::parse_completion(&v);
                if items.is_empty() {
                    self.local_completion(false);
                    return;
                }
                self.lsp.completion = Some(CompletionUi { uid, start, items, shown: Vec::new(), selected: 0, incomplete, client: ci });
                self.completion_filter();
            }
            LspReq::Definition { word } => match lsp::parse_location(&v) {
                Some((path, pos)) => {
                    if self.open_path(path) {
                        if let Some(d) = self.docs.get_mut(self.active) {
                            let p = lsp::from_position(&d.rope, &pos);
                            let (l, c) = d.line_col(p);
                            d.goto_line(l, c);
                        }
                        self.history.open = false;
                        self.pending_focus = true;
                    }
                }
                None => self.index_definition(&word),
            },
            LspReq::Rename { name } => {
                let edits = lsp::parse_workspace_edit(&v);
                if edits.is_empty() {
                    self.set_status("Rien à renommer ici");
                    return;
                }
                let back = self.docs.get(self.active).map(|d| d.uid);
                let (mut n, files) = (0, edits.len());
                for (path, list) in edits {
                    let i = match self.docs.iter().position(|d| d.path.as_ref() == Some(&path)) {
                        Some(i) => i,
                        None if self.open_path(path.clone()) => self.active,
                        None => continue,
                    };
                    let d = &mut self.docs[i];
                    let changes: Vec<(usize, usize, String)> = list
                        .iter()
                        .map(|(range, t)| (lsp::from_position(&d.rope, &range["start"]), lsp::from_position(&d.rope, &range["end"]), t.clone()))
                        .collect();
                    n += changes.len();
                    d.replace_keep(EditKind::Other, changes);
                }
                if let Some(i) = back.and_then(|u| self.docs.iter().position(|d| d.uid == u)) {
                    self.active = i;
                }
                self.set_status(format!("Renommé en « {name} » : {n} endroit(s) dans {files} fichier(s) — à enregistrer (Ctrl+Alt+S : tout enregistrer)"));
                let _ = ctx;
            }
        }
    }

    /// Keeps the suggestions matching the word being typed; closes the list when it no longer applies.
    pub(super) fn completion_filter(&mut self) {
        let Some(c) = self.lsp.completion.as_mut() else { return };
        let Some(d) = self.docs.get(self.active).filter(|d| d.uid == c.uid && d.sels.len() == 1) else {
            self.lsp.completion = None;
            return;
        };
        let head = d.primary_sel().head;
        if head < c.start || d.rope.char_to_line(head) != d.rope.char_to_line(c.start) {
            self.lsp.completion = None;
            return;
        }
        let prefix = d.slice(c.start, head);
        if prefix.chars().any(|ch| !(ch.is_alphanumeric() || ch == '_' || ch == '$')) {
            self.lsp.completion = None;
            return;
        }
        let lower = prefix.to_lowercase();
        let mut scored: Vec<(i32, usize)> = c
            .items
            .iter()
            .enumerate()
            .filter_map(|(k, it)| {
                if prefix.is_empty() {
                    return Some((0, k));
                }
                let f = it.filter.to_lowercase();
                let bonus = if f.starts_with(&lower) { 1000 } else { 0 };
                fuzzy::score(&prefix, &it.filter).map(|s| (s + bonus, k))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| c.items[a.1].sort.cmp(&c.items[b.1].sort)));
        c.shown = scored.into_iter().map(|(_, k)| k).take(200).collect();
        if c.shown.is_empty() {
            self.lsp.completion = None;
            return;
        }
        c.selected = c.selected.min(c.shown.len() - 1);
    }

    fn accept_completion(&mut self, k: usize) {
        let Some(c) = self.lsp.completion.take() else { return };
        let Some(&item) = c.shown.get(k).as_ref() else { return };
        let it = &c.items[*item];
        let Some(d) = self.docs.get_mut(self.active).filter(|d| d.uid == c.uid) else { return };
        let head = d.primary_sel().head;
        let a = match &it.range {
            Some(r) => lsp::from_position(&d.rope, &r["start"]).min(c.start),
            None => c.start,
        };
        let n = it.insert.chars().count();
        d.replace_range(a, head, &it.insert);
        d.sels = vec![Sel::cursor(a + n)];
        d.primary = 0;
        let _ = c.client;
    }

    /// Up / Down / Enter / Tab / Escape go to the suggestion list while it is open.
    pub(super) fn completion_keys(&mut self, ctx: &egui::Context) {
        if self.lsp.completion.is_none() || !ctx.memory(|m| m.has_focus(editor_id())) {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let mut used = vec![false; events.len()];
        let mut accept = None;
        for (k, ev) in events.iter().enumerate() {
            let Event::Key { key, pressed: true, modifiers, .. } = ev else { continue };
            if !modifiers.is_none() && !(modifiers.shift_only() && *key == Key::Tab) {
                continue;
            }
            let Some(c) = self.lsp.completion.as_mut() else { break };
            let n = c.shown.len();
            match key {
                Key::ArrowDown => c.selected = (c.selected + 1) % n,
                Key::ArrowUp => c.selected = (c.selected + n - 1) % n,
                Key::PageDown => c.selected = (c.selected + 8).min(n - 1),
                Key::PageUp => c.selected = c.selected.saturating_sub(8),
                Key::Enter | Key::Tab => accept = Some(c.selected),
                Key::Escape => self.lsp.completion = None,
                _ => continue,
            }
            used[k] = true;
        }
        if used.iter().any(|u| *u) {
            ctx.input_mut(|inp| {
                let mut k = 0;
                inp.events.retain(|_| {
                    let keep = !used.get(k).copied().unwrap_or(false);
                    k += 1;
                    keep
                });
            });
        }
        if let Some(k) = accept {
            self.accept_completion(k);
        }
    }

    pub(super) fn completion_popup(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.lsp.completion else { return };
        let Some(d) = self.docs.get(self.active).filter(|d| d.uid == c.uid) else { return };
        let Some((x, y)) = d.caret_screen else { return };
        let mut clicked = None;
        let font = egui::TextStyle::Monospace.resolve(&ctx.style());
        egui::Area::new(egui::Id::new("completion"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(x - 22.0, y + 2.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(4.0)).show(ui, |ui| {
                    ui.set_width(440.0);
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (row, &k) in c.shown.iter().enumerate() {
                            let it = &c.items[k];
                            let sel = row == c.selected;
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), Sense::click());
                            if sel {
                                ui.painter().rect_filled(rect, 4.0, ui.visuals().selection.bg_fill);
                                resp.scroll_to_me(None);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
                            }
                            let p = ui.painter().with_clip_rect(rect);
                            let cy = rect.center().y;
                            p.text(egui::pos2(rect.left() + 10.0, cy), Align2::CENTER_CENTER, it.icon(), font.clone(), self.hl.colors.accent);
                            let r = p.text(egui::pos2(rect.left() + 22.0, cy), Align2::LEFT_CENTER, &it.label, font.clone(), ui.visuals().strong_text_color());
                            if !it.detail.is_empty() {
                                // the detail on the right, without covering the label
                                let p = p.with_clip_rect(egui::Rect::from_min_max(egui::pos2(r.right() + 12.0, rect.top()), rect.max));
                                p.text(
                                    egui::pos2(rect.right() - 6.0, cy),
                                    Align2::RIGHT_CENTER,
                                    it.detail.lines().next().unwrap_or(""),
                                    egui::FontId::proportional(11.5),
                                    ui.visuals().weak_text_color(),
                                );
                            }
                            if resp.clicked() {
                                clicked = Some(row);
                            }
                        }
                    });
                    ui.label(RichText::new("↑ ↓ choisir · Entrée / Tab insérer · Échap fermer").small().weak());
                });
            });
        if let Some(k) = clicked {
            self.accept_completion(k);
            self.pending_focus = true;
        }
    }
}
