//! Overlay: command palette, go to file, prompts.

use super::*;
use crate::os::Pick;

impl App {
    pub(super) fn open_overlay(&mut self, mode: Mode) {
        let items: Vec<Item> = match &mode {
            Mode::Commands => {
                let mut v: Vec<Item> = commands::ALL
                    .iter()
                    .filter(|(_, l, _)| !l.is_empty())
                    .map(|(_, l, c)| Item { label: l.to_string(), detail: self.keymap.label(*c), action: Action::Cmd(*c) })
                    .collect();
                for (k, &(pi, ci)) in self.plugin_cmds.iter().enumerate() {
                    let p = &self.plugins[pi];
                    v.push(Item {
                        label: format!("{} : {}", p.name, p.commands[ci].title),
                        detail: self.keymap.label(Cmd::Plugin(k)),
                        action: Action::Cmd(Cmd::Plugin(k)),
                    });
                }
                v
            }
            Mode::Files => self.file_items(),
            Mode::Projects => self
                .projects
                .iter()
                .filter(|p| p.path.is_dir())
                .map(|p| Item { label: p.name.clone(), detail: p.path.display().to_string(), action: Action::Project(p.path.clone()) })
                .collect(),
            Mode::Syntax => self
                .hl
                .syntax_names()
                .into_iter()
                .map(|n| Item { label: n.clone(), detail: String::new(), action: Action::Syntax(n) })
                .collect(),
            Mode::Symbols => match self.docs.get_mut(self.active) {
                Some(d) => {
                    let syms = d.symbols().to_vec();
                    syms.iter()
                        .map(|sy| {
                            let parents: Vec<&str> =
                                symbols::path_at(&syms, sy.line).iter().filter(|p| p.line != sy.line).map(|p| p.name.as_str()).collect();
                            let mut detail = parents.join(" › ");
                            if !sy.detail.is_empty() {
                                detail = if detail.is_empty() { sy.detail.clone() } else { format!("{detail} · {}", sy.detail) };
                            }
                            let detail = if detail.is_empty() { format!("ligne {}", sy.line + 1) } else { format!("{detail} · ligne {}", sy.line + 1) };
                            Item { label: format!("{}  {}", sy.kind.icon(), sy.name), detail, action: Action::Line(sy.line) }
                        })
                        .collect()
                }
                None => Vec::new(),
            },
            Mode::Templates => {
                self.templates = crate::templates::load(&crate::templates::dir());
                let ext = self
                    .docs
                    .get(self.active)
                    .and_then(|d| d.path.as_deref())
                    .and_then(|p| p.extension())
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                // the templates for the current kind of file first
                let mut order: Vec<usize> = (0..self.templates.len()).collect();
                order.sort_by_key(|&i| self.templates[i].ext != ext);
                order
                    .into_iter()
                    .map(|i| {
                        let t = &self.templates[i];
                        let detail = match (&t.path, t.ext.is_empty()) {
                            (Some(_), _) => format!(".{} · perso", t.ext),
                            (None, false) => format!(".{}", t.ext),
                            (None, true) => String::new(),
                        };
                        Item { label: t.name.clone(), detail, action: Action::Template(i) }
                    })
                    .collect()
            }
            Mode::Themes => self
                .hl
                .theme_names()
                .into_iter()
                .map(|n| Item { label: n.clone(), detail: String::new(), action: Action::Theme(n) })
                .collect(),
            _ => Vec::new(),
        };
        let files = matches!(mode, Mode::Files);
        self.open_overlay_with(mode, items, String::new());
        if files {
            let boost = self.recent_boost();
            if let Some(o) = self.overlay.as_mut() {
                o.boost = boost;
            }
        }
    }

    pub(super) fn file_items(&self) -> Vec<Item> {
        if self.root.is_none() {
            return Vec::new();
        }
        self.index.files
            .iter()
            .enumerate()
            .map(|(i, p)| Item { label: p.to_string_lossy().into_owned(), detail: String::new(), action: Action::OpenIndexed(i) })
            .collect()
    }

    /// Go to file: bonus of each entry of the file list, for the files opened lately (the more
    /// recent, the higher; the current file gets none, it is already in front of you).
    pub(super) fn recent_boost(&self) -> Vec<i32> {
        let Some(root) = &self.root else { return Vec::new() };
        let current = self.docs.get(self.active).and_then(|d| d.path.as_ref());
        let rank: HashMap<&Path, i32> = self
            .index
            .recent
            .iter()
            .filter(|p| Some(*p) != current)
            .filter_map(|p| p.strip_prefix(root).ok())
            .enumerate()
            .map(|(k, p)| (p, 60 - k as i32))
            .collect();
        if rank.is_empty() {
            return Vec::new();
        }
        self.index.files.iter().map(|p| rank.get(p.as_path()).copied().unwrap_or(0)).collect()
    }

    pub(super) fn open_overlay_with(&mut self, mode: Mode, items: Vec<Item>, query: String) {
        self.overlay = Some(Overlay { mode, query, selected: 0, focus: true, items, filtered: Vec::new(), last_query: None, boost: Vec::new(), matched: None, paths: None });
    }

    pub(super) fn open_prompt(&mut self, p: Prompt) {
        let root = self.root.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let with_sep = |p: &Path| format!("{}{}", p.display(), std::path::MAIN_SEPARATOR);
        let query = match &p {
            Prompt::OpenFolder => with_sep(&root),
            Prompt::OpenFile => with_sep(&root),
            Prompt::SaveAs => match self.docs.get(self.active).and_then(|d| d.path.clone()) {
                Some(path) => path.display().to_string(),
                None => with_sep(&root),
            },
            Prompt::Rename(path) => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            _ => String::new(),
        };
        self.open_overlay_with(Mode::Prompt(p), Vec::new(), query);
    }

    pub(super) fn overlay_ui(&mut self, ctx: &egui::Context) {
        let Some(mut ov) = self.overlay.take() else { return };
        let title = match &ov.mode {
            Mode::Commands => "Palette de commandes".to_string(),
            Mode::Files => "Aller au fichier   (fichier:ligne, ou :ligne dans le fichier courant)".into(),
            Mode::GotoLine if self.goto_select => {
                "Sélectionner la ligne   (42, ou 10-20 pour plusieurs lignes ; vide + Entrée : la ligne courante)".into()
            }
            Mode::GotoLine => "Aller à la ligne   (ligne ou ligne:colonne)".into(),
            Mode::Syntax => "Syntaxe du fichier".into(),
            Mode::Themes => "Thème".into(),
            Mode::Templates => "Insérer un modèle   (${model}, ${Model}, ${module} remplis d'après le fichier courant)".into(),
            Mode::Branches => "Changer de branche".into(),
            Mode::Projects => "Changer de projet".into(),
            Mode::Symbols => "Aller au symbole   (classe, fonction, méthode, record Odoo, titre…)".into(),
            Mode::Definitions(w) => format!("Définitions de « {w} »"),
            Mode::References(w) => format!("Usages de « {w} »"),
            Mode::Bookmarks => "Signets".into(),
            Mode::Prompt(p) => match p {
                Prompt::OpenFolder => "Ouvrir le dossier".into(),
                Prompt::OpenFile => "Ouvrir le fichier".into(),
                Prompt::SaveAs => "Enregistrer sous".into(),
                Prompt::NewFile(d) => format!("Nouveau fichier dans {}", d.display()),
                Prompt::NewFolder(d) => format!("Nouveau dossier dans {}", d.display()),
                Prompt::Rename(p) => format!("Renommer {}", p.display()),
                Prompt::NewBranch => "Nom de la nouvelle branche".into(),
                Prompt::RenameSymbol => "Nouveau nom (renommé partout dans le projet)".into(),
                Prompt::NewTemplate { ext, .. } if ext.is_empty() => "Nouveau modèle : son déclencheur, avec l'extension des fichiers visés (ipdb.py)".into(),
                Prompt::NewTemplate { ext, .. } => format!("Nouveau modèle pour les .{ext} : son déclencheur (ex. ipdb ; nom.xml pour un autre type)"),
                Prompt::RenameProject(_) => "Nom du projet".into(),
                Prompt::BranchAt(h) => format!("Nouvelle branche sur le commit {}", &h[..h.len().min(8)]),
            },
        };
        let is_list = !matches!(ov.mode, Mode::Prompt(_) | Mode::GotoLine);
        // prompts asking for a path complete it with Tab, like a shell
        let path_base = match &ov.mode {
            Mode::Prompt(Prompt::OpenFolder | Prompt::OpenFile | Prompt::SaveAs) => {
                Some((self.root.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default()), matches!(ov.mode, Mode::Prompt(Prompt::OpenFolder))))
            }
            Mode::Prompt(Prompt::NewFile(d) | Prompt::NewFolder(d)) => Some((d.clone(), matches!(ov.mode, Mode::Prompt(Prompt::NewFolder(_))))),
            _ => None,
        };
        let mut cursor_to_end = false;
        // the same prompts can go through the system's file chooser
        let dialog_kind = match &ov.mode {
            Mode::Prompt(Prompt::OpenFolder) => Some(Pick::Folder),
            Mode::Prompt(Prompt::OpenFile) => Some(Pick::File),
            Mode::Prompt(Prompt::SaveAs) => Some(Pick::SaveAs),
            _ => None,
        };
        let mut browse = false;
        if let Some((base, dirs_only)) = &path_base {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab)) {
                let (done, _) = complete_path(&ov.query, base, *dirs_only);
                ov.query = done;
                cursor_to_end = true;
            }
            if ov.paths.as_ref().map(|(q, _)| q) != Some(&ov.query) {
                let (_, found) = complete_path(&ov.query, base, *dirs_only);
                ov.paths = Some((ov.query.clone(), found));
            }
        }
        let files_mode = matches!(ov.mode, Mode::Files);
        let mut chosen: Option<Action> = None;
        let mut submit = false;
        let mut goto_only: Option<String> = None;
        let indexing = files_mode && self.index.files.is_empty() && self.root.is_some();
        let area = egui::Area::new(egui::Id::new("overlay"))
            .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 40.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(12.0)).rounding(14.0).show(ui, |ui| {
                    ui.set_width(640.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&title).small().weak());
                        if dialog_kind.is_some() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                browse = ui.small_button("📂 Parcourir…").on_hover_text("Choisir dans le gestionnaire de fichiers").clicked();
                            });
                        }
                    });
                    ui.add_space(2.0);
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut ov.query)
                            .desired_width(f32::INFINITY)
                            .frame(false)
                            .hint_text("Tapez pour filtrer…")
                            .font(egui::FontId::proportional(17.0))
                            // Tab completes the path: it must not also move the focus to the next widget
                            .lock_focus(path_base.is_some()),
                    );
                    ui.add_space(4.0);
                    ui.separator();
                    if ov.focus {
                        te.request_focus();
                        ov.focus = false;
                        // the text field keeps its cursor from the previous prompt: put it at the end,
                        // or select the name (without extension) of the file to save
                        {
                            let mut st = egui::TextEdit::load_state(ui.ctx(), te.id).unwrap_or_default();
                            let chars: Vec<char> = ov.query.chars().collect();
                            let end = chars.len();
                            let name_start = chars.iter().rposition(|c| matches!(c, '/' | '\\')).map_or(0, |i| i + 1);
                            let range = match chars[name_start..].iter().rposition(|c| *c == '.') {
                                Some(dot) if matches!(ov.mode, Mode::Prompt(Prompt::SaveAs)) && dot > 0 => egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(name_start),
                                    egui::text::CCursor::new(name_start + dot),
                                ),
                                _ => egui::text::CCursorRange::one(egui::text::CCursor::new(end)),
                            };
                            st.cursor.set_char_range(Some(range));
                            st.store(ui.ctx(), te.id);
                        }
                    }
                    if cursor_to_end {
                        if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), te.id) {
                            let end = egui::text::CCursor::new(ov.query.chars().count());
                            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                            st.store(ui.ctx(), te.id);
                        }
                        te.request_focus();
                    }
                    let (down, up, enter) =
                        ui.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::Enter)));
                    if let Some((_, found)) = ov.paths.as_ref().filter(|_| path_base.is_some()) {
                        let cut = ov.query.rfind(['/', '\\']).map_or(0, |i| i + 1);
                        let mut fill = None;
                        if found.is_empty() {
                            ui.label(RichText::new("Tab : compléter le chemin").small().weak());
                        } else {
                            ui.label(RichText::new(format!("Tab : compléter · {} entrée(s)", found.len())).small().weak());
                            egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                                for (name, is_dir) in found.iter().take(200) {
                                    let label = if *is_dir { format!("{name}{}", std::path::MAIN_SEPARATOR) } else { name.clone() };
                                    let text = RichText::new(label.clone()).monospace();
                                    let text = if *is_dir { text.color(ui.visuals().hyperlink_color) } else { text };
                                    if ui.add(egui::Label::new(text).sense(Sense::click())).clicked() {
                                        fill = Some(format!("{}{label}", &ov.query[..cut]));
                                    }
                                }
                            });
                        }
                        if let Some(f) = fill {
                            ov.query = f;
                            ov.focus = true;
                            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), te.id) {
                                let end = egui::text::CCursor::new(ov.query.chars().count());
                                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                                st.store(ui.ctx(), te.id);
                            }
                        }
                    }
                    if !is_list {
                        submit = enter;
                        return;
                    }
                    if matches!(ov.mode, Mode::Files) && ov.query.starts_with(':') {
                        ui.label(RichText::new("Entrée : aller à cette ligne du fichier courant").weak());
                        if enter {
                            goto_only = Some(ov.query[1..].to_string());
                        }
                        return;
                    }
                    if ov.last_query.as_ref() != Some(&ov.query) {
                        ov.last_query = Some(ov.query.clone());
                        ov.selected = 0;
                        let q = ov.query.split(':').next().unwrap_or("").to_string();
                        // typing one more letter: only the items that matched before can match
                        let narrowing = ov.matched.as_ref().filter(|(prev, _)| !prev.is_empty() && q.starts_with(prev.as_str()));
                        let pool: Box<dyn Iterator<Item = usize>> = match narrowing {
                            Some((_, m)) => Box::new(m.clone().into_iter()),
                            None => Box::new(0..ov.items.len()),
                        };
                        let boost = &ov.boost;
                        let mut scored: Vec<(i32, usize)> = pool
                            .filter_map(|i| fuzzy::score(&q, &ov.items[i].label).map(|s| (s + boost.get(i).copied().unwrap_or(0), i)))
                            .collect();
                        ov.matched = Some((q.clone(), scored.iter().map(|(_, i)| *i).collect()));
                        // ranked by score, or (nothing typed) by boost: recent files first
                        let ranked = !q.is_empty() || !ov.boost.is_empty();
                        if ranked && scored.len() > 300 {
                            // only the best 300 are shown: no need to sort everything
                            scored.select_nth_unstable_by(300, |a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                            scored.truncate(300);
                        }
                        if ranked {
                            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                        }
                        ov.filtered = scored.into_iter().take(300).map(|(_, i)| i).collect();
                    }
                    let n = ov.filtered.len();
                    if n > 0 && down {
                        ov.selected = (ov.selected + 1) % n;
                    }
                    if n > 0 && up {
                        ov.selected = (ov.selected + n - 1) % n;
                    }
                    // same height whatever the number of results: the window does not jump while typing
                    let fixed = matches!(ov.mode, Mode::Files | Mode::Commands);
                    egui::ScrollArea::vertical()
                        .max_height(440.0)
                        .min_scrolled_height(if fixed { 440.0 } else { 0.0 })
                        .auto_shrink([false, !fixed])
                        .show(ui, |ui| {
                        for (row, &i) in ov.filtered.iter().enumerate() {
                            let it = &ov.items[i];
                            // full-width row: label on the left, shortcut / detail on the right
                            let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::click());
                            let v = ui.visuals();
                            if row == ov.selected {
                                ui.painter().rect_filled(rect, 7.0, v.selection.bg_fill);
                            } else if r.hovered() {
                                ui.painter().rect_filled(rect, 7.0, v.widgets.hovered.weak_bg_fill);
                            }
                            let text_color = if row == ov.selected { v.strong_text_color() } else { v.text_color() };
                            let detail_w = if it.detail.is_empty() {
                                0.0
                            } else {
                                let g = ui.painter().layout_no_wrap(it.detail.clone(), egui::FontId::proportional(12.0), v.weak_text_color());
                                let w = g.size().x;
                                ui.painter().galley(egui::pos2(rect.right() - w - 12.0, rect.center().y - g.size().y / 2.0), g, v.weak_text_color());
                                w + 24.0
                            };
                            let clip = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - detail_w, rect.bottom()));
                            let mut text_x = rect.left() + 12.0;
                            if files_mode {
                                let name = it.label.rsplit(['/', '\\']).next().unwrap_or(&it.label);
                                let bg = if row == ov.selected { v.selection.bg_fill } else { v.window_fill };
                                let light = bg.r() as u32 + bg.g() as u32 + bg.b() as u32 > 384;
                                let r = egui::Rect::from_center_size(egui::pos2(rect.left() + 20.0, rect.center().y), egui::vec2(16.0, 16.0));
                                icons::paint_file(ui.painter(), r, name, bg, light);
                                text_x += 22.0;
                            }
                            ui.painter().with_clip_rect(clip).text(
                                egui::pos2(text_x, rect.center().y),
                                Align2::LEFT_CENTER,
                                &it.label,
                                egui::FontId::proportional(14.0),
                                text_color,
                            );
                            if row == ov.selected && (up || down) {
                                r.scroll_to_me(None);
                            }
                            if r.clicked() {
                                chosen = Some(it.action.clone());
                            }
                        }
                        if n == 0 && indexing {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("Liste des fichiers du projet en cours…").weak());
                            });
                        } else if n == 0 {
                            ui.label(RichText::new("Aucun résultat").weak());
                        }
                    });
                    if enter {
                        if let Some(&i) = ov.filtered.get(ov.selected) {
                            chosen = Some(ov.items[i].action.clone());
                        }
                    }
                });
            });
        // A click elsewhere closes the overlay: otherwise it would stay open, holding the keyboard
        // (the editor ignores typing while an overlay is open).
        if let Some(kind) = dialog_kind.filter(|_| browse) {
            self.file_dialog(kind);
            return;
        }
        let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p)));
        if outside && chosen.is_none() && goto_only.is_none() && !submit {
            return;
        }

        if let Some(spec) = goto_only {
            self.goto_spec(&spec);
            self.focus_editor(ctx);
        } else if let Some(action) = chosen {
            let line_spec = match ov.mode {
                Mode::Files => ov.query.split_once(':').map(|(_, l)| l.to_string()),
                _ => None,
            };
            self.choose(ctx, action);
            if let Some(spec) = line_spec {
                self.goto_spec(&spec);
            }
            if self.overlay.is_none() {
                self.focus_editor(ctx);
            }
        } else if submit {
            match ov.mode {
                Mode::GotoLine if self.goto_select => {
                    if let Some(d) = self.docs.get_mut(self.active) {
                        let cur = d.rope.char_to_line(d.primary_sel().head);
                        match document::parse_line_range(&ov.query, d.rope.len_lines(), cur) {
                            Some((a, b)) => d.select_lines(a, b),
                            None => self.set_status("Numéro de ligne attendu : 42, ou 10-20"),
                        }
                    }
                }
                Mode::GotoLine => self.goto_spec(&ov.query),
                Mode::Prompt(p) => self.submit_prompt(p, ov.query.clone()),
                _ => {}
            }
            if self.overlay.is_none() {
                self.focus_editor(ctx);
            }
        } else {
            self.overlay = Some(ov);
        }
    }

    fn goto_spec(&mut self, spec: &str) {
        let mut it = spec.split(':').map(|s| s.trim().parse::<usize>().ok());
        let line = it.next().flatten();
        let col = it.next().flatten().unwrap_or(1);
        if let (Some(l), Some(d)) = (line, self.docs.get_mut(self.active)) {
            d.goto_line(l.saturating_sub(1), col.saturating_sub(1));
        }
    }

    fn choose(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Cmd(c) => self.exec(ctx, c),
            Action::OpenIndexed(i) => {
                if let (Some(root), Some(p)) = (self.root.clone(), self.index.files.get(i)) {
                    self.open_path(root.join(p));
                }
            }
            Action::Location(p, l) => self.open_at(p, l),
            Action::Line(l) => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    let ind = d.indent_len(l);
                    d.goto_line(l, ind);
                }
            }
            Action::Syntax(n) => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    d.syntax = n;
                }
            }
            Action::Theme(n) => {
                self.hl.set_theme(&n);
                apply_visuals(ctx, self.hl.visuals());
                self.settings.theme = n.clone();
                self.persist("theme", config::toml_str(&n));
                self.set_status(format!("Thème « {n} » enregistré"));
            }
            Action::Branch(b) => {
                let name = b.split_once('/').filter(|(r, _)| *r == "origin").map(|(_, n)| n.to_string()).unwrap_or(b);
                self.git_job("checkout", &["checkout", &name]);
            }
            Action::Project(p) => self.switch_project(p),
            Action::Template(k) => self.insert_template(k),
        }
    }

    /// Inserts template `k` at every cursor, indented like the line, with the cursor at its `$0`.
    fn insert_template(&mut self, k: usize) {
        let Some(t) = self.templates.get(k).cloned() else { return };
        if self.docs.get(self.active).map_or(true, |d| d.read_only || d.view != document::View::Text) {
            self.new_doc();
            let first = t.body.lines().next().unwrap_or("").to_string();
            let syntax = self.hl.detect(Some(Path::new(&format!("x.{}", t.ext))), &first);
            if let Some(d) = self.docs.get_mut(self.active) {
                d.syntax = syntax;
            }
        }
        let Some(d) = self.docs.get_mut(self.active) else { return };
        let vars = crate::templates::variables(d.path.as_deref());
        let items: Vec<Edit> = d
            .sels
            .clone()
            .iter()
            .map(|s| {
                let line = d.rope.char_to_line(s.start());
                let line_start = d.rope.line_to_char(line);
                let indent: String = d.line_text(line).chars().take(s.start() - line_start).take_while(|c| *c == ' ' || *c == '\t').collect();
                let (text, cursor) = crate::templates::expand(&t.body, &vars, &indent);
                Edit { start: s.start(), end: s.end(), text, anchor: cursor, head: cursor }
            })
            .collect();
        d.edit(EditKind::Other, items);
        d.scroll_to_cursor = true;
        self.set_status(format!("Modèle « {} » inséré", t.name));
    }

    pub(super) fn submit_prompt(&mut self, p: Prompt, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        match p {
            Prompt::OpenFolder => {
                let path = self.resolve(PathBuf::from(&text));
                if path.is_dir() {
                    self.switch_project(path);
                } else {
                    self.set_status(format!("Dossier introuvable : {}", path.display()));
                }
            }
            Prompt::OpenFile => {
                let path = self.resolve(PathBuf::from(&text));
                self.open_path(path);
            }
            Prompt::SaveAs => {
                let path = self.resolve(PathBuf::from(&text));
                if let Some(d) = self.docs.get_mut(self.active) {
                    let first = d.line_text(0);
                    d.path = Some(path.clone());
                    d.title_override = None;
                    d.read_only = false;
                    d.syntax = self.hl.detect(Some(&path), &first);
                }
                if self.save(self.active) {
                    self.refresh_index();
                }
            }
            Prompt::NewFile(dir) => {
                let path = dir.join(&text);
                if !path.exists() {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(&path, "") {
                        self.set_status(e.to_string());
                        return;
                    }
                }
                self.open_path(path);
                self.exec_refresh_tree();
            }
            Prompt::NewTemplate { body, ext } => {
                let dir = crate::templates::dir();
                crate::templates::install_readme(&dir);
                let name = if text.contains('.') || ext.is_empty() { text.clone() } else { format!("{text}.{ext}") };
                let path = dir.join(&name);
                if !path.exists() {
                    let body = if body.is_empty() { String::new() } else { format!("{body}$0") };
                    if let Err(e) = std::fs::write(&path, body) {
                        self.set_status(e.to_string());
                        return;
                    }
                }
                self.open_path(path);
                let trigger = name.split('.').next().unwrap_or("");
                self.set_status(format!("Écris le modèle ($0 = place du curseur), enregistre, puis tape « {trigger} » + Tab"));
            }
            Prompt::NewFolder(dir) => {
                if let Err(e) = std::fs::create_dir_all(dir.join(&text)) {
                    self.set_status(e.to_string());
                }
                self.exec_refresh_tree();
            }
            Prompt::Rename(old) => {
                let new = old.with_file_name(&text);
                match std::fs::rename(&old, &new) {
                    Ok(()) => {
                        self.relocate_docs(&old, &new);
                        self.exec_refresh_tree();
                    }
                    Err(e) => self.set_status(e.to_string()),
                }
            }
            Prompt::NewBranch => self.git_job("checkout -b", &["checkout", "-b", &text]),
            Prompt::BranchAt(hash) => self.git_job("branch", &["branch", &text, &hash]),
            Prompt::RenameSymbol => {
                let ctx = self.egui_ctx.clone();
                let i = self.active;
                if let (Some((ci, _)), Some((doc, pos))) = (self.lsp_for(i, &ctx, true), self.lsp_text_position(i)) {
                    if let Some(c) = self.lsp.manager.client(ci) {
                        let id = c.request("textDocument/rename", serde_json::json!({ "textDocument": doc, "position": pos, "newName": text }));
                        self.lsp.requests.insert((ci, id), LspReq::Rename { name: text });
                    }
                }
            }
            Prompt::RenameProject(path) => {
                if let Some(p) = self.projects.iter_mut().find(|p| p.path == path) {
                    p.name = text;
                }
                project::save_list(&self.projects);
            }
        }
    }

    pub(super) fn exec_refresh_tree(&mut self) {
        if let Some(t) = self.tree.as_mut() {
            t.refresh();
        }
        self.refresh_index();
        self.git_refresh();
    }

    pub(super) fn confirm_ui(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        let (title, msg) = match c {
            Confirm::CloseTab(uid) => {
                let name = self.docs.iter().find(|d| d.uid == *uid).map(|d| d.title()).unwrap_or_default();
                ("Fichier modifié", format!("Enregistrer les modifications de « {name} » ?"))
            }
            Confirm::Quit => ("Quitter", "Des fichiers ne sont pas enregistrés.".to_string()),
            Confirm::Delete(p) => ("Supprimer", format!("Supprimer définitivement « {} » ?", p.display())),
            Confirm::SwitchProject(p) => {
                ("Changer de projet", format!("Des fichiers ne sont pas enregistrés. Ouvrir « {} » ?", p.display()))
            }
            Confirm::DiffOp { msg, .. } => ("Annuler le changement", msg.clone()),
            Confirm::ReplaceInFiles => {
                let n = self.search.hit_count();
                (
                    "Remplacer dans les fichiers",
                    format!(
                        "Remplacer {n} occurrence(s) dans {} fichier(s) ?\nLes fichiers non ouverts sont modifiés directement sur le disque.",
                        self.search.files.len()
                    ),
                )
            }
        };
        let mut choice = None;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(msg);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let (yes, no) = match c {
                        Confirm::CloseTab(_) => ("Enregistrer", Some("Ne pas enregistrer")),
                        Confirm::Quit => ("Tout enregistrer et quitter", Some("Quitter sans enregistrer")),
                        Confirm::Delete(_) => ("Supprimer", None),
                        Confirm::SwitchProject(_) => ("Tout enregistrer et changer", Some("Changer sans enregistrer")),
                        Confirm::ReplaceInFiles => ("Remplacer", None),
                        Confirm::DiffOp { .. } => ("Oui, annuler le changement", None),
                    };
                    if ui.button(yes).clicked() {
                        choice = Some(1);
                    }
                    if let Some(no) = no {
                        if ui.button(no).clicked() {
                            choice = Some(2);
                        }
                    }
                    if ui.button("Annuler").clicked() {
                        choice = Some(0);
                    }
                });
            });
        let Some(choice) = choice else { return };
        let c = self.confirm.take().unwrap();
        match (c, choice) {
            (Confirm::DiffOp { label, op, .. }, 1) => self.run_diff_op(label, op),
            (Confirm::CloseTab(uid), 1 | 2) => {
                if let Some(i) = self.docs.iter().position(|d| d.uid == uid) {
                    if choice == 2 || self.save(i) {
                        self.close_tab(i, true);
                    }
                }
            }
            (Confirm::Quit, 1) => {
                for i in 0..self.docs.len() {
                    if self.docs[i].is_dirty() && !self.save(i) {
                        return;
                    }
                }
                self.quit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            (Confirm::Quit, 2) => {
                self.quit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            (Confirm::SwitchProject(p), 1) => {
                for i in 0..self.docs.len() {
                    if self.docs[i].is_dirty() && !self.save(i) {
                        return;
                    }
                }
                self.do_switch(p);
            }
            (Confirm::SwitchProject(p), 2) => self.do_switch(p),
            (Confirm::ReplaceInFiles, 1) => self.replace_in_files(None),
            (Confirm::Delete(p), 1) => {
                let r = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                match r {
                    Ok(()) => {
                        self.set_status(format!("Supprimé : {}", p.display()));
                        self.exec_refresh_tree();
                    }
                    Err(e) => self.set_status(e.to_string()),
                }
            }
            _ => {}
        }
    }

    /// Native dialog of the system (Windows Explorer, macOS Finder) to choose a folder, a file
    /// to open or where to save. Returns false where Florm uses its own path entry (Linux).
    #[cfg(any(windows, target_os = "macos"))]
    pub(super) fn native_dialog(&mut self, kind: Pick) -> bool {
        self.file_dialog(kind);
        true
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub(super) fn native_dialog(&mut self, _kind: Pick) -> bool {
        false
    }

    /// File chooser of the system (Explorer, Finder, the desktop's one through the xdg portal on
    /// Linux) for a folder, a file to open or where to save.
    pub(super) fn file_dialog(&mut self, kind: Pick) {
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
            let picked = crate::os::portal_pick(kind, &dir, name.as_deref().filter(|_| kind == Pick::SaveAs));
            #[cfg(not(target_os = "linux"))]
            let picked = {
                let d = rfd::FileDialog::new().set_directory(&dir);
                Ok(match kind {
                    Pick::Folder => d.set_title("Ouvrir un dossier (les projets WSL : \\\\wsl.localhost\\…)").pick_folder(),
                    Pick::File => d.set_title("Ouvrir un fichier").pick_file(),
                    Pick::SaveAs => {
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
}
