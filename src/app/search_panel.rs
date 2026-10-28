//! Find in files: panel and replace.

use super::*;

/// What marks work left in the code.
pub(super) const TODO_PATTERN: &str = r"\b(TODO|FIXME|XXX|HACK)\b";

/// The search of the TODO / FIXME of the project (the filters of files kept).
pub(super) fn set_todo_query(q: &mut search::Query) {
    q.pattern = TODO_PATTERN.into();
    (q.regex, q.case, q.whole) = (true, true, false);
}

impl App {
    /// The TODO / FIXME of the project, in the search (panel or tab, as it is set).
    pub(super) fn list_todos(&mut self) {
        if self.settings.search_in_tab {
            self.open_search_tab();
        } else {
            self.bottom = Some(BottomTab::Search);
        }
        set_todo_query(&mut self.search.q);
        self.start_search();
    }

    pub(super) fn start_search(&mut self) {
        let Some(root) = self.root.clone() else {
            self.set_status(t!("Ouvre d'abord un dossier", "Open a folder first"));
            return;
        };
        if self.search.q.pattern.is_empty() {
            return;
        }
        let q = &self.search.q;
        UiState::remember(&mut self.state.search_history, &q.pattern);
        UiState::remember(&mut self.state.include_history, &q.include);
        UiState::remember(&mut self.state.exclude_history, &q.exclude);
        self.state.last_include = q.include.trim().to_string();
        self.state.last_exclude = q.exclude.trim().to_string();
        project::save_state(&self.state);
        self.search.hist_pos = None;
        let gen = self.search.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let (q, current) = (self.search.q.clone(), self.search.gen.clone());
        self.search.done = q.clone();
        self.search.running = true;
        let ex = crate::walk::Exclusions::new(&root, &self.settings.exclude);
        self.jobs.spawn(move || JobResult::Search(gen, search::run(&root, &q, &ex, gen, &current)));
    }

    fn open_hit(&mut self, fi: usize, hi: usize) {
        let Some(f) = self.search.files.get(fi) else { return };
        let Some(h) = f.hits.get(hi) else { return };
        let (path, line, col, len) = (f.path.clone(), h.line, h.col, h.len);
        self.search.cursor = Some((fi, hi));
        if self.open_path(path) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.goto_line(line, col);
                let p = d.primary_sel().head;
                let end = (p + len).min(d.len());
                d.sels = vec![Sel::range(p, end)];
                d.primary = 0;
                d.flash = Some((p, end, Instant::now()));
            }
        }
        self.search.scroll_to_cursor = true;
    }

    pub(super) fn step_result(&mut self, forward: bool) {
        let s = &self.search;
        let hits: Vec<(usize, usize)> =
            s.files.iter().enumerate().flat_map(|(fi, f)| (0..f.hits.len()).map(move |h| (fi, h))).collect();
        let n = hits.len();
        if n == 0 {
            return;
        }
        let idx = match s.cursor.and_then(|c| hits.iter().position(|&x| x == c)) {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            None if forward => 0,
            None => n - 1,
        };
        let (f, h) = hits[idx];
        if self.search.collapsed.remove(&f) {
            self.search.rebuild_rows();
        }
        self.open_hit(f, h);
    }

    /// Replaces every listed hit, those of one file (`Some((file, None))`) or a single one.
    pub(super) fn replace_in_files(&mut self, only: Option<(usize, Option<usize>)>) {
        let q = &self.search.done;
        let Ok(re) = document::build_regex(&q.pattern, q.case, q.regex, q.whole) else { return };
        let (rep, regex_mode, preserve) = (self.search.replacement.clone(), q.regex, self.search.q.preserve);
        let keep_all = |f: &FileHits| f.partial.then(|| f.hits.iter().map(|h| (h.line, h.col)).collect::<HashSet<_>>());
        let targets: Vec<(PathBuf, Option<HashSet<(usize, usize)>>)> = match only {
            None => self.search.files.iter().map(|f| (f.path.clone(), keep_all(f))).collect(),
            Some((fi, hit)) => {
                let Some(f) = self.search.files.get(fi) else { return };
                let keep = match hit.and_then(|h| f.hits.get(h)) {
                    Some(h) => Some([(h.line, h.col)].into()),
                    None => keep_all(f),
                };
                vec![(f.path.clone(), keep)]
            }
        };
        let (mut files, mut count, mut in_editor) = (0, 0, 0);
        let mut errors = Vec::new();
        for (p, keep) in targets {
            // Open files are changed in the editor (undoable, saved by the user).
            if let Some(d) = self.docs.iter_mut().find(|d| d.path.as_ref() == Some(&p)) {
                if let Some((new, n)) = search::replace_text(&re, &d.rope.to_string(), &rep, regex_mode, preserve, keep.as_ref()) {
                    d.set_text(&new);
                    (files, count, in_editor) = (files + 1, count + n, in_editor + 1);
                }
                continue;
            }
            match std::fs::read_to_string(&p) {
                Ok(text) => {
                    if let Some((new, n)) = search::replace_text(&re, &text, &rep, regex_mode, preserve, keep.as_ref()) {
                        match std::fs::write(&p, new) {
                            Ok(()) => (files, count) = (files + 1, count + n),
                            Err(e) => errors.push(format!("{} : {e}", p.display())),
                        }
                    }
                }
                Err(e) => errors.push(format!("{} : {e}", p.display())),
            }
        }
        for e in errors {
            self.log(e);
        }
        let open = if in_editor > 0 { tf!(" — {in_editor} fichier(s) ouvert(s) à enregistrer", " — {in_editor} open file(s) to save") } else { String::new() };
        self.set_status(tf!("{count} remplacement(s) dans {files} fichier(s){open}", "{count} replacement(s) in {files} file(s){open}"));
        self.start_search();
        self.git_refresh();
    }

    pub(super) fn search_ui(&mut self, ui: &mut egui::Ui, in_tab: bool) {
        let (mut go, mut act) = (false, None);
        let accent = self.hl.colors.accent;
        let st = &self.state;
        let hl = &self.hl;
        let s = &mut self.search;
        let enter = |r: &egui::Response, ui: &egui::Ui| r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let field_w = (ui.available_width() * 0.45).clamp(220.0, 520.0);

        // --- search line: [▸ replace] [pattern] [Aa .* « »] [⏷] [Rechercher]
        ui.horizontal(|ui| {
            let arrow = if s.show_replace { "⏷" } else { "⏵" };
            if ui
                .add(egui::Button::new(RichText::new(arrow).color(accent)).frame(false).min_size(egui::vec2(18.0, 0.0)))
                .on_hover_text(t!("Afficher / masquer le remplacement", "Show / hide the replacement"))
                .clicked()
            {
                s.show_replace = !s.show_replace;
            }
            let te = ui.add(
                egui::TextEdit::singleline(&mut s.q.pattern)
                    .desired_width(field_w)
                    .hint_text(t!("🔍  Rechercher dans le dossier… (Entrée)", "🔍  Find in folder… (Enter)"))
                    .font(egui::TextStyle::Monospace),
            );
            if s.focus {
                te.request_focus();
                s.focus = false;
            }
            if enter(&te, ui) {
                go = true;
                te.request_focus();
            }
            if te.has_focus() {
                if let Some(t) = history_arrows(ui, &st.search_history, &mut s.hist_pos) {
                    s.q.pattern = t;
                }
            }
            go |= history_menu(ui, &st.search_history, &mut s.q.pattern, t!("Recherches récentes", "Recent searches"));
            ui.toggle_value(&mut s.q.case, "Aa").on_hover_text(t!("Respecter la casse", "Match case"));
            ui.toggle_value(&mut s.q.regex, ".*").on_hover_text(t!("Expression régulière", "Regular expression"));
            ui.toggle_value(&mut s.q.whole, "« »").on_hover_text(t!("Mot entier", "Whole word"));
            let search_btn = filled_button(t!("Rechercher", "Find"), accent);
            if ui.add(search_btn).clicked() {
                go = true;
            }
            if s.running {
                spinner(ui);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(crate::view_bar::BarAct::Move(b)) = crate::view_bar::controls(ui, in_tab, None, None) {
                    act = Some(SearchAct::Move(b));
                }
                if ui.button("TODO / FIXME").on_hover_text(t!("Les TODO, FIXME, XXX et HACK du projet, par fichier", "The TODO, FIXME, XXX and HACK of the project, by file")).clicked() {
                    set_todo_query(&mut s.q);
                    go = true;
                }
            });
        });

        // --- replace line
        if s.show_replace {
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                let hint = if s.q.regex { t!("Remplacer par… ($1, $2 = groupes, \\n = retour à la ligne)", "Replace with… ($1, $2 = groups, \\n = new line)") } else { t!("Remplacer par… (vide = supprimer)", "Replace with… (empty = delete)") };
                ui.add(replace_field(&mut s.replacement, hint).desired_width(field_w)).on_hover_text(replace_tip());
                let n = s.hit_count();
                if ui.add_enabled(n > 0 && !s.running, filled_button(tf!("Tout remplacer ({n})", "Replace all ({n})"), warn())).clicked() {
                    act = Some(SearchAct::ReplaceAll);
                }
                ui.toggle_value(&mut s.q.preserve, t!("Garder la casse", "Keep case")).on_hover_text(t!("chat → chien, Chat → Chien, CHAT → CHIEN", "cat → dog, Cat → Dog, CAT → DOG"));
            });
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                ui.spacing_mut().item_spacing.x = 4.0;
                let small = |t: &str| RichText::new(t).small();
                ui.label(small(t!("Aperçu :", "Preview:")).weak());
                ui.label(small(t!(" ancien ", " old ")).strikethrough().background_color(icons::mix(ui.visuals().panel_fill, err(), 0.4)));
                ui.label(small("→").weak());
                ui.label(small(t!(" nouveau ", " new ")).background_color(icons::mix(ui.visuals().panel_fill, ok(), 0.4)));
                ui.label(
                    small(t!("· survol ou clic droit sur un résultat pour le remplacer seul ou l'écarter (× = ne pas remplacer)", "· hover or right click a result to replace it alone or drop it (× = don't replace)"))
                        .weak(),
                );
            });
        }

        // --- filters line
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            ui.label(RichText::new(t!("Fichiers", "Files")).color(ok()));
            let inc = ui
                .add(egui::TextEdit::singleline(&mut s.q.include).desired_width(220.0).hint_text("*.py, models/*.py, !tests"))
                .on_hover_text(t!(
                    "Plusieurs règles, séparées par des virgules ; ! devant une règle l'exclut :\n\
                     *.py — tous les fichiers Python\n\
                     models/*.py — les fichiers Python des dossiers « models » (où qu'ils soient) et de leurs sous-dossiers\n\
                     tests — tout dossier (ou fichier) nommé « tests »\n\
                     ./src/*.rs ou /chemin/absolu/*.py — à partir de la racine du projet\n\
                     *.py, !tests — les .py, sauf ceux des dossiers « tests »\n\
                     florm/*.py, !florm/test — le nom du projet ouvert peut se mettre devant\n\
                     Aide → Guide de Florm pour plus d'exemples",
                    "Several rules, separated by commas; ! in front of a rule excludes it:\n\
                     *.py — all the Python files\n\
                     models/*.py — the Python files of the “models” folders (wherever they are) and of their subfolders\n\
                     tests — any folder (or file) named “tests”\n\
                     ./src/*.rs or /absolute/path/*.py — from the root of the project\n\
                     *.py, !tests — the .py files, except those of the “tests” folders\n\
                     florm/*.py, !florm/test — the name of the open project can go in front\n\
                     Help → Florm guide for more examples"
                ));
            go |= history_menu(ui, &st.include_history, &mut s.q.include, t!("Filtres récents", "Recent filters"));
            ui.add_space(8.0);
            ui.label(RichText::new(t!("Exclure", "Exclude")).color(err()));
            let exc = ui.add(
                egui::TextEdit::singleline(&mut s.q.exclude).desired_width(170.0).hint_text("tests, *.min.js"),
            ).on_hover_text(t!(
                "Ce qui est ignoré, en plus du .gitignore (mêmes règles que « Fichiers », séparées par des virgules) :\n\
                 tests — tout dossier nommé « tests »\n\
                 addons/sale/tests — seulement celui-là\n\
                 ./test ou florm/test — le dossier « test » à la racine du projet « florm »\n\
                 *.min.js, i18n — ces fichiers et ces dossiers\n\n\
                 Ex. : Fichiers « *.py » + Exclure « test » = les .py du projet, sauf le dossier test",
                "What is ignored, on top of the .gitignore (same rules as “Files”, separated by commas):\n\
                 tests — any folder named “tests”\n\
                 addons/sale/tests — only that one\n\
                 ./test or florm/test — the “test” folder at the root of the “florm” project\n\
                 *.min.js, i18n — these files and folders\n\n\
                 E.g.: Files “*.py” + Exclude “test” = the .py files of the project, except the test folder"
            ));
            go |= history_menu(ui, &st.exclude_history, &mut s.q.exclude, t!("Exclusions récentes", "Recent exclusions"));
            go |= enter(&inc, ui) || enter(&exc, ui);
        });

        // --- summary
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            ui.spacing_mut().item_spacing.x = 0.0;
            if let Some(e) = &s.error {
                ui.label(RichText::new(format!("⚠ {e}")).color(err()));
            } else if let Some((scanned, millis, truncated)) = s.stats {
                let n = s.hit_count();
                if scanned == 0 && !s.done.include.trim().is_empty() {
                    ui.label(
                        RichText::new(tf!("Aucun fichier ne correspond au filtre « {} »", "No file matches the filter “{}”", s.done.include.trim()))
                            .color(warn()),
                    );
                } else if n == 0 {
                    ui.label(RichText::new(tf!("Aucun résultat pour « {} »", "No result for “{}”", s.done.pattern)).color(warn()));
                } else {
                    ui.label(RichText::new(n.to_string()).color(accent).strong());
                    ui.label(t!(" résultat(s) dans ", " result(s) in "));
                    ui.label(RichText::new(s.files.len().to_string()).color(accent).strong());
                    ui.label(t!(" fichier(s)", " file(s)"));
                }
                ui.label(RichText::new(tf!("   ·   {scanned} fichiers parcourus en {millis} ms", "   ·   {scanned} files scanned in {millis} ms")).weak());
                if truncated {
                    ui.label(RichText::new(t!("   ·   limite atteinte", "   ·   limit reached")).color(warn()));
                }
                if n > 0 {
                    ui.label(RichText::new(t!("   ·   F4 / Maj+F4 pour naviguer   ", "   ·   F4 / Shift+F4 to navigate   ")).weak());
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if ui.small_button(t!("Tout replier", "Fold all")).clicked() {
                        s.collapsed = (0..s.files.len()).collect();
                        s.rebuild_rows();
                    }
                    if ui.small_button(t!("Tout déplier", "Unfold all")).clicked() {
                        s.collapsed.clear();
                        s.rebuild_rows();
                    }
                }
            } else {
                ui.label(
                    RichText::new(t!("Tape un texte puis Entrée. Filtre les fichiers avec « *.py », exclus des dossiers avec « tests ».", "Type a text then Enter. Filter the files with “*.py”, exclude folders with “tests”."))
                        .weak(),
                );
            }
        });
        ui.add_space(2.0);

        // --- results
        let v = ui.visuals().clone();
        let bg = v.panel_fill;
        let light = bg.r() as u32 + bg.g() as u32 + bg.b() as u32 > 384;
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let fg = v.text_color();
        let preview_re = if s.show_replace {
            let q = &s.done;
            document::build_regex(&q.pattern, q.case, q.regex, q.whole).ok()
        } else {
            None
        };
        let match_fmt = TextFormat {
            font_id: font.clone(),
            color: if light { Color32::BLACK } else { v.strong_text_color() },
            background: icons::mix(bg, warn(), if light { 0.65 } else { 0.45 }),
            ..Default::default()
        };
        let old_fmt = TextFormat {
            font_id: font.clone(),
            color: fg,
            background: icons::mix(bg, err(), 0.4),
            strikethrough: egui::Stroke::new(1.0_f32, if light { Color32::BLACK } else { fg }),
            ..Default::default()
        };
        let new_fmt = TextFormat {
            font_id: font.clone(),
            color: if light { Color32::BLACK } else { v.strong_text_color() },
            background: icons::mix(bg, ok(), 0.4),
            ..Default::default()
        };
        let row_h = 22.0;
        ui.spacing_mut().item_spacing.y = 0.0; // show_rows positions rows with this spacing
        let mut area = egui::ScrollArea::both().id_salt("search_results").auto_shrink([false, false]);
        if std::mem::take(&mut s.scroll_to_cursor) {
            if let Some(i) = s.cursor.and_then(|(f, h)| s.rows.iter().position(|&r| r == (f, Some(h)))) {
                let (y, view_h) = (i as f32 * row_h, ui.available_height());
                let id = ui.make_persistent_id("search_results");
                let top = egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |st| st.offset.y);
                if y < top || y + row_h > top + view_h {
                    area = area.vertical_scroll_offset((y - view_h / 2.0).max(0.0));
                }
            }
        }
        area.show_rows(
            ui,
            row_h,
            s.rows.len(),
            |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let visible_right = ui.clip_rect().right();
                for r in range {
                    let (fi, hi) = s.rows[r];
                    let f = &s.files[fi];
                    let width = ui.available_width();
                    match hi {
                        None => {
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::click());
                            let hovered = ui.rect_contains_pointer(rect);
                            let fill = if hovered { v.widgets.hovered.weak_bg_fill } else { v.faint_bg_color };
                            ui.painter().rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), 4.0, fill);
                            let p = ui.painter().with_clip_rect(rect);
                            let cy = rect.center().y;
                            let collapsed = s.collapsed.contains(&fi);
                            icons::paint_chevron(&p, egui::pos2(rect.left() + 9.0, cy), !collapsed, v.weak_text_color());
                            let name = f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 28.0, cy), egui::vec2(16.0, 16.0));
                            icons::paint_file(&p, icon, &name, fill, light);
                            let mut x = rect.left() + 42.0;
                            let name_r = p.text(
                                egui::pos2(x, cy),
                                Align2::LEFT_CENTER,
                                &name,
                                egui::FontId::proportional(13.5),
                                v.strong_text_color(),
                            );
                            x = name_r.right() + 8.0;
                            let dir = f.rel.strip_suffix(&name).unwrap_or("").trim_end_matches(['/', '\\']);
                            if !dir.is_empty() {
                                let r = p.text(
                                    egui::pos2(x, cy),
                                    Align2::LEFT_CENTER,
                                    dir,
                                    egui::FontId::proportional(12.0),
                                    v.weak_text_color(),
                                );
                                x = r.right() + 8.0;
                            }
                            // hit count pill
                            let count = f.hits.len().to_string();
                            let galley = p.layout_no_wrap(count, egui::FontId::proportional(11.5), accent);
                            let pill = egui::Rect::from_min_size(
                                egui::pos2(x, cy - 8.0),
                                egui::vec2(galley.size().x + 12.0, 16.0),
                            );
                            p.rect_filled(pill, 8.0, icons::mix(fill, accent, 0.22));
                            p.galley(pill.center() - galley.size() / 2.0, galley, accent);
                            let mut clicked_button = false;
                            if hovered {
                                let mut right = rect.right().min(visible_right) - 4.0;
                                let mut btn = |ui: &mut egui::Ui, text: &str, tip: &str| {
                                    let w = 10.0 + 7.5 * text.chars().count() as f32;
                                    let r = egui::Rect::from_min_max(egui::pos2(right - w, cy - 9.0), egui::pos2(right, cy + 9.0));
                                    right -= w + 4.0;
                                    ui.put(r, egui::Button::new(RichText::new(text).small())).on_hover_text(tip).clicked()
                                };
                                if btn(ui, "×", t!("Écarter ce fichier des résultats (il ne sera pas modifié)", "Drop this file from the results (it will not be changed)")) {
                                    act = Some(SearchAct::Dismiss(fi, None));
                                    clicked_button = true;
                                }
                                if s.show_replace && btn(ui, t!("Remplacer", "Replace"), t!("Remplacer dans ce fichier", "Replace in this file")) {
                                    act = Some(SearchAct::Replace(fi, None));
                                    clicked_button = true;
                                }
                                if btn(ui, "📂", t!("Afficher dans le gestionnaire de fichiers", "Show in the file manager")) {
                                    act = Some(SearchAct::Reveal(fi));
                                    clicked_button = true;
                                }
                            }
                            let resp = resp.on_hover_text_at_pointer(f.path.display().to_string());
                            if resp.clicked() && !clicked_button {
                                act = Some(SearchAct::Fold(fi));
                            }
                            if resp.double_clicked() {
                                act = Some(SearchAct::Open(fi, 0));
                            }
                            resp.context_menu(|ui| {
                                let mut item = |ui: &mut egui::Ui, label: &str, a: SearchAct| {
                                    if ui.button(label).clicked() {
                                        act = Some(a);
                                        ui.close_menu();
                                    }
                                };
                                item(ui, t!("Ouvrir le fichier", "Open the file"), SearchAct::Open(fi, 0));
                                item(ui, if collapsed { t!("Déplier", "Unfold") } else { t!("Replier", "Fold") }, SearchAct::Fold(fi));
                                ui.separator();
                                if s.show_replace {
                                    item(ui, t!("Remplacer dans ce fichier", "Replace in this file"), SearchAct::Replace(fi, None));
                                }
                                item(ui, t!("Écarter ce fichier des résultats", "Drop this file from the results"), SearchAct::Dismiss(fi, None));
                                ui.separator();
                                item(ui, t!("Afficher dans le gestionnaire de fichiers", "Show in the file manager"), SearchAct::Reveal(fi));
                                item(ui, t!("Copier le chemin", "Copy the path"), SearchAct::Copy(f.path.display().to_string()));
                                item(ui, t!("Copier le chemin relatif", "Copy the relative path"), SearchAct::Copy(f.rel.clone()));
                            });
                        }
                        Some(h) => {
                            let hit = &f.hits[h];
                            let syntax = s.syntaxes.entry(fi).or_insert_with(|| hl.detect(Some(&f.path), ""));
                            let spans = s.colors.entry((fi, h)).or_insert_with(|| hl.line_spans(syntax, &hit.preview));
                            let mut job = LayoutJob::default();
                            append_colored(&mut job, &hit.preview, 0, hit.start, spans, &font, fg);
                            let matched = &hit.preview[hit.start..hit.end];
                            match &preview_re {
                                Some(re) => {
                                    job.append(matched, 0.0, old_fmt.clone());
                                    // a line break of the replacement, on the one line of the result
                                    let new = search::replacement_preview(re, matched, &s.replacement, s.done.regex, s.q.preserve).replace('\n', "↵");
                                    job.append(&new, 0.0, new_fmt.clone());
                                }
                                None => job.append(matched, 0.0, match_fmt.clone()),
                            }
                            append_colored(&mut job, &hit.preview, hit.end, hit.preview.len(), spans, &font, fg);
                            let galley = ui.fonts(|fo| fo.layout_job(job));
                            let text_x = 92.0;
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width.max(text_x + galley.size().x + 16.0), row_h),
                                Sense::click(),
                            );
                            let hovered = ui.rect_contains_pointer(rect);
                            let current = s.cursor == Some((fi, h));
                            if current {
                                ui.painter().rect_filled(rect, 4.0, v.selection.bg_fill);
                            } else if hovered {
                                ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
                            }
                            let p = ui.painter().with_clip_rect(rect);
                            let cy = rect.center().y;
                            if current {
                                p.rect_filled(
                                    egui::Rect::from_min_size(egui::pos2(rect.left() + 2.0, rect.top() + 3.0), egui::vec2(3.0, row_h - 6.0)),
                                    1.5,
                                    accent,
                                );
                            }
                            p.text(
                                egui::pos2(rect.left() + text_x - 12.0, cy),
                                Align2::RIGHT_CENTER,
                                (hit.line + 1).to_string(),
                                font.clone(),
                                icons::mix(bg, accent, 0.75),
                            );
                            p.galley(egui::pos2(rect.left() + text_x, cy - galley.size().y / 2.0), galley, fg);
                            let mut clicked_button = false;
                            if hovered {
                                let mut right = rect.right().min(visible_right) - 4.0;
                                let mut btn = |ui: &mut egui::Ui, text: &str, tip: &str| {
                                    let w = 10.0 + 7.5 * text.chars().count() as f32;
                                    let r = egui::Rect::from_min_max(egui::pos2(right - w, cy - 9.0), egui::pos2(right, cy + 9.0));
                                    right -= w + 4.0;
                                    ui.put(r, egui::Button::new(RichText::new(text).small())).on_hover_text(tip).clicked()
                                };
                                if btn(ui, "×", t!("Écarter ce résultat (il ne sera pas remplacé)", "Drop this result (it will not be replaced)")) {
                                    act = Some(SearchAct::Dismiss(fi, Some(h)));
                                    clicked_button = true;
                                }
                                if s.show_replace && btn(ui, t!("Remplacer", "Replace"), t!("Remplacer cette occurrence", "Replace this occurrence")) {
                                    act = Some(SearchAct::Replace(fi, Some(h)));
                                    clicked_button = true;
                                }
                            }
                            if resp.clicked() && !clicked_button {
                                act = Some(SearchAct::Open(fi, h));
                            }
                            let ref_text = format!("{}:{}", f.rel, hit.line + 1);
                            resp.context_menu(|ui| {
                                let mut item = |ui: &mut egui::Ui, label: &str, a: SearchAct| {
                                    if ui.button(label).clicked() {
                                        act = Some(a);
                                        ui.close_menu();
                                    }
                                };
                                item(ui, t!("Ouvrir", "Open"), SearchAct::Open(fi, h));
                                if s.show_replace {
                                    item(ui, t!("Remplacer cette occurrence", "Replace this occurrence"), SearchAct::Replace(fi, Some(h)));
                                }
                                item(ui, t!("Écarter ce résultat", "Drop this result"), SearchAct::Dismiss(fi, Some(h)));
                                ui.separator();
                                item(ui, t!("Afficher dans le gestionnaire de fichiers", "Show in the file manager"), SearchAct::Reveal(fi));
                                item(ui, t!("Copier la référence fichier:ligne", "Copy the file:line reference"), SearchAct::Copy(ref_text.clone()));
                            });
                        }
                    }
                }
            },
        );
        if go {
            self.start_search();
        }
        match act {
            Some(SearchAct::Open(f, h)) => {
                self.open_hit(f, h);
                ui.ctx().memory_mut(|m| m.request_focus(editor_id()));
            }
            Some(SearchAct::Fold(f)) => {
                let s = &mut self.search;
                if !s.collapsed.remove(&f) {
                    s.collapsed.insert(f);
                }
                s.rebuild_rows();
            }
            Some(SearchAct::Dismiss(f, h)) => self.search.dismiss(f, h),
            Some(SearchAct::Replace(f, h)) => self.replace_in_files(Some((f, h))),
            Some(SearchAct::ReplaceAll) => self.confirm = Some(Confirm::ReplaceInFiles),
            Some(SearchAct::Reveal(f)) => {
                if let Some(p) = self.search.files.get(f).map(|f| f.path.clone()) {
                    self.reveal(&p);
                }
            }
            Some(SearchAct::Copy(t)) => {
                ui.ctx().output_mut(|o| o.copied_text = t.clone());
                self.set_status(tf!("Copié : {t}", "Copied: {t}"));
            }
            Some(SearchAct::Move(true)) => {
                self.open_search_tab();
                self.settings.search_in_tab = true;
                self.persist("search_in_tab", "true".into());
            }
            Some(SearchAct::Move(false)) => {
                if let Some(i) = self.docs.iter().position(|d| d.view == document::View::Search) {
                    self.close_tab(i, true);
                }
                self.bottom = Some(BottomTab::Search);
                self.search.focus = true;
                self.settings.search_in_tab = false;
                self.persist("search_in_tab", "false".into());
            }
            None => {}
        }
    }

}
