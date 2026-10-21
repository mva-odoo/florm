//! Cursor history and moves between places.

use super::*;

impl App {
    /// Records the previous place when the cursor jumps (another file, or 8+ lines away).
    pub(super) fn track_nav(&mut self) {
        let Some(d) = self.docs.get(self.active).filter(|d| d.view == document::View::Text) else { return };
        let pos = d.primary_sel().head;
        let line = d.rope.char_to_line(pos);
        let cur = NavPos { uid: d.uid, path: d.path.clone(), pos };
        if let Some((last, last_line)) = self.nav_last.take() {
            let jumped = last.uid != cur.uid || line.abs_diff(last_line) >= 8;
            if jumped && !std::mem::take(&mut self.nav_jumping) {
                let same = self.nav_back.last().is_some_and(|b| b.uid == last.uid && b.pos == last.pos);
                if !same {
                    self.nav_back.push(last);
                    if self.nav_back.len() > 100 {
                        self.nav_back.remove(0);
                    }
                }
                self.nav_fwd.clear();
            }
        }
        self.nav_jumping = false;
        self.nav_last = Some((cur, line));
    }

    /// Alt+- (back) / Alt++ (forward).
    pub(super) fn nav_go(&mut self, back: bool) {
        let here = self.docs.get(self.active).map(|d| NavPos { uid: d.uid, path: d.path.clone(), pos: d.primary_sel().head });
        loop {
            let target = if back { self.nav_back.pop() } else { self.nav_fwd.pop() };
            let Some(t) = target else {
                self.set_status(if back { "Pas de position précédente" } else { "Pas de position suivante" });
                return;
            };
            // the document may have been closed: reopen it from its path
            let idx = match self.docs.iter().position(|d| d.uid == t.uid) {
                Some(i) => Some(i),
                None => t.path.clone().filter(|p| p.is_file()).and_then(|p| self.open_path(p).then_some(self.active)),
            };
            let Some(i) = idx else { continue };
            if let Some(h) = here.clone() {
                if back { self.nav_fwd.push(h) } else { self.nav_back.push(h) }
            }
            self.activate_tab(i);
            let d = &mut self.docs[i];
            let p = t.pos.min(d.len());
            let (l, c) = d.line_col(p);
            d.goto_line(l, c);
            self.nav_jumping = true;
            return;
        }
    }

    /// Like Sublime Text: definitions of `word` in the project index; one opens, several are listed.
    pub(super) fn index_definition(&mut self, word: &str) {
        let mut found: Vec<(PathBuf, crate::index::Entry)> = match &self.sym_index {
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
                let more = if self.sym_index.is_none() { " (indexation du projet en cours…)" } else { "" };
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

    pub(super) fn open_at(&mut self, p: PathBuf, line: usize) {
        if self.open_path(p) {
            if let Some(d) = self.docs.get_mut(self.active) {
                let ind = d.indent_len(line.min(d.rope.len_lines().saturating_sub(1)));
                d.goto_line(line, ind);
            }
            self.history.open = false;
            self.pending_focus = true;
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
        if let Some(ix) = &self.sym_index {
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
        self.completion = Some(CompletionUi { uid, start, items, shown: Vec::new(), selected: 0, incomplete: false, client: usize::MAX });
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
                self.completion = Some(CompletionUi { uid, start, items, shown: Vec::new(), selected: 0, incomplete, client: ci });
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
        let Some(c) = self.completion.as_mut() else { return };
        let Some(d) = self.docs.get(self.active).filter(|d| d.uid == c.uid && d.sels.len() == 1) else {
            self.completion = None;
            return;
        };
        let head = d.primary_sel().head;
        if head < c.start || d.rope.char_to_line(head) != d.rope.char_to_line(c.start) {
            self.completion = None;
            return;
        }
        let prefix = d.slice(c.start, head);
        if prefix.chars().any(|ch| !(ch.is_alphanumeric() || ch == '_' || ch == '$')) {
            self.completion = None;
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
            self.completion = None;
            return;
        }
        c.selected = c.selected.min(c.shown.len() - 1);
    }

    fn accept_completion(&mut self, k: usize) {
        let Some(c) = self.completion.take() else { return };
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
        if self.completion.is_none() || !ctx.memory(|m| m.has_focus(editor_id())) {
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
            let Some(c) = self.completion.as_mut() else { break };
            let n = c.shown.len();
            match key {
                Key::ArrowDown => c.selected = (c.selected + 1) % n,
                Key::ArrowUp => c.selected = (c.selected + n - 1) % n,
                Key::PageDown => c.selected = (c.selected + 8).min(n - 1),
                Key::PageUp => c.selected = c.selected.saturating_sub(8),
                Key::Enter | Key::Tab => accept = Some(c.selected),
                Key::Escape => self.completion = None,
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
        let Some(c) = &self.completion else { return };
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
