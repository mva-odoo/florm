//! Results of background jobs.

use super::*;

impl App {
    pub(super) fn process_jobs(&mut self) {
        while let Ok(r) = self.jobs.rx.try_recv() {
            match r {
                JobResult::Analyzed { uid, version, reload, diags, base, log } => {
                    for l in log {
                        self.log(l);
                    }
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                        if reload && d.version == version {
                            let _ = d.reload_from_disk();
                        }
                        if let Some((dg, linter)) = diags {
                            d.diagnostics = dg;
                            d.lint_version = d.version;
                            if lint::lang_of(d.path.as_deref().unwrap_or(Path::new(""))) == Some("python") {
                                self.python_linter = Some(linter);
                            }
                        }
                        d.git_base = base;
                        d.refresh_diff();
                    }
                }
                JobResult::Lint { uid, version, diags, linter } => {
                    self.lint_inflight.remove(&uid);
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                        if d.version == version {
                            d.diagnostics = diags;
                            d.lint_version = version;
                        }
                    }
                    self.python_linter = Some(linter);
                }
                JobResult::ToolInstalled(r) => {
                    // a language server may have been installed: try again
                    self.lsp.manager.reset();
                    self.lsp.notes.clear();
                    match r {
                        Ok(msg) => {
                            self.log(msg.clone());
                            self.set_status(msg);
                        }
                        Err(e) => {
                            self.log(e);
                            self.set_status("Installation échouée (voir Sortie)");
                            self.bottom = Some(BottomTab::Output);
                        }
                    }
                    for d in self.docs.iter_mut() {
                        d.lint_version = u64::MAX; // re-lint with the new tool
                    }
                }
                JobResult::FileMatches { gen, query, matched, top } => {
                    if let Some(o) = self.overlay.as_mut().filter(|o| o.gen.load(Ordering::SeqCst) == gen) {
                        o.matched = Some((query, Arc::new(matched)));
                        o.filtered = top;
                        o.selected = 0;
                        o.pending = false;
                    }
                }
                JobResult::FileIndex(root, list) => {
                    if self.root.as_ref() == Some(&root) && self.index.symbols_for.as_ref() != Some(&root) {
                        self.index.symbols_for = Some(root.clone());
                        let r = root.clone();
                        // the repositories being worked on first
                        let mut first: Vec<PathBuf> = self.docs.iter().filter_map(|d| d.path.as_deref()).filter_map(|p| self.repo_for(p)).collect();
                        first.extend(self.repo.clone());
                        let ex = crate::walk::Exclusions::new(&root, &self.settings.exclude);
                        let max = self.settings.index_max_files;
                        let send = self.jobs.sender();
                        self.jobs.spawn(move || {
                            let r2 = r.clone();
                            let idx = crate::index::Index::build_in_steps(&r, &first, &ex, max, move |part| send(JobResult::SymbolIndex(r2, part, true)));
                            JobResult::SymbolIndex(r, idx, false)
                        });
                    }
                    if let Some((files, labels)) = list.filter(|_| self.root.as_ref() == Some(&root)) {
                        self.index.files = Arc::new(files);
                        let items: Arc<[Item]> =
                            labels.into_iter().enumerate().map(|(i, label)| Item { label, detail: String::new(), action: Action::OpenIndexed(i) }).collect();
                        self.index.file_items = Some(items);
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
                JobResult::GitStatus { selected, statuses, remotes, stashes } => {
                    if self.repo.as_ref() != Some(&selected) {
                        continue;
                    }
                    self.git.stashes = stashes;
                    // branch / remote of every repository, for the tree
                    self.repo_info.clear();
                    for (repo, st) in &statuses {
                        let Ok(st) = st else { continue };
                        if self.pr.repo.as_ref() == Some(repo) {
                            self.pr.branch = Some(st.branch.clone());
                        }
                        let mut label = st.branch.clone();
                        if let Some((up, ahead, behind)) = st.upstream() {
                            // the usual origin/<same branch> is left to the tooltip
                            if !up.split_once('/').is_some_and(|(_, b)| b == st.branch) {
                                label.push_str(&format!(" → {up}"));
                            }
                            if ahead > 0 {
                                label.push_str(&format!(" ↑{ahead}"));
                            }
                            if behind > 0 {
                                label.push_str(&format!(" ↓{behind}"));
                            }
                        }
                        let raw = remotes.iter().find(|(r, _)| r == repo).map(|(_, t)| t.as_str()).unwrap_or("");
                        let mut urls: Vec<String> = raw
                            .lines()
                            .filter(|l| l.ends_with("(fetch)"))
                            .map(|l| l.trim_end_matches("(fetch)").trim().replacen('\t', " : ", 1))
                            .collect();
                        urls.dedup();
                        let mut tip = format!("Branche : {}\n", st.branch);
                        match st.upstream() {
                            Some((up, a, b)) => tip.push_str(&format!("Suit : {up} ({a} à pousser, {b} à récupérer)\n")),
                            None => tip.push_str("Pas de branche distante suivie\n"),
                        }
                        if urls.is_empty() {
                            tip.push_str("Aucun remote");
                        } else {
                            tip.push_str(&format!("Remote(s) :\n{}", urls.join("\n")));
                        }
                        let n = st.entries.len();
                        if n > 0 {
                            label.push_str(&format!(" · {n} modif."));
                        }
                        self.repo_info.insert(repo.clone(), (label, tip));
                    }
                    self.git.map.clear();
                    self.git.status = None;
                    self.git.error = None;
                    let root = self.root.clone();
                    for (repo, st) in statuses {
                        match st {
                            Ok(st) => {
                                for e in &st.entries {
                                    let abs = repo.join(&e.path);
                                    // Color parent folders up to the opened folder (repos may be nested in it).
                                    for a in abs.ancestors().skip(1) {
                                        if root.as_ref().is_some_and(|r| !a.starts_with(r) || a == r.as_path()) {
                                            break;
                                        }
                                        self.git.map.entry(a.to_path_buf()).or_insert('M');
                                    }
                                    self.git.map.insert(abs, e.label());
                                }
                                if repo == selected {
                                    self.git.status = Some(st);
                                }
                            }
                            Err(e) if repo == selected => self.git.error = Some(e),
                            Err(_) => {}
                        }
                    }
                    // the branch may have changed: comments of the pull request in the editor or not
                    self.pr_update_notes();
                }
                JobResult::PrList(repo, r) => self.pr_list_loaded(repo, r),
                JobResult::ClaudeReview(id, r) => self.claude_review_loaded(id, r),
                JobResult::CommitMessage(r) => self.commit_message_ready(r),
                JobResult::QueueCommit(id, r) => self.queue_commit_loaded(id, r),
                JobResult::AiReview(key, r) => self.ai_review_loaded(key, r),
                JobResult::InlineAi(n, r) => self.inline_ai_loaded(n, r),
                JobResult::GhBase(repo, r) => self.gh_base_loaded(repo, r),
                JobResult::GhBranchPrs(repo, branch, r) => self.gh_branch_prs_loaded(repo, branch, r),
                JobResult::GhCommitPrs(sha, r) => self.gh_commit_prs_loaded(sha, r),
                JobResult::PrCreated(repo, r) => self.pr_created(repo, r),
                JobResult::PrDetail(repo, n, r) => self.pr_detail_loaded(repo, n, r),
                JobResult::PrCommitDiff(sha, r) => self.pr_commit_diff_loaded(sha, r),
                JobResult::PrOp { label, result, reload, draft } => self.pr_op_done(label, result, reload, draft),
                JobResult::History(repo, gen, r) => {
                    if self.history.repo.as_ref() == Some(&repo) && gen == self.history.gen {
                        self.history.loading = false;
                        match r {
                            Ok(g) => {
                                let wanted = self.history.select_hash.take();
                                let pick = wanted.as_ref().and_then(|h| g.commits.iter().position(|c| &c.hash == h));
                                // older than the commits loaded: load more of them
                                let more = self.history.seek && pick.is_none() && g.commits.len() >= self.history.limit.max(HISTORY_LIMIT) && self.history.limit < HISTORY_MAX;
                                self.history.graph = Some(g);
                                self.history.error = None;
                                if more {
                                    self.history.limit = HISTORY_MAX;
                                    self.history.select_hash = wanted;
                                    self.load_history();
                                } else if let Some(i) = pick {
                                    self.history.seek = false;
                                    self.history.wip_sel = None;
                                    self.history.diff = None;
                                    self.history.scroll_to_sel = true;
                                    self.select_commit(i);
                                } else if let Some(h) = wanted.filter(|_| std::mem::take(&mut self.history.seek)) {
                                    self.history.notice = Some((format!("Commit {} introuvable dans le graphe", &h[..h.len().min(8)]), err()));
                                }
                            }
                            Err(e) => self.history.error = Some(e),
                        }
                    }
                }
                JobResult::Blame { uid, line, version, result } => {
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                        let text = match result {
                            Ok(b) if b.uncommitted() => "Vous · pas encore commité".to_string(),
                            Ok(b) => format!(
                                "{} · {} · {} ({})",
                                b.author,
                                git::ago(b.time),
                                b.summary,
                                &b.hash[..b.hash.len().min(7)]
                            ),
                            Err(_) => "pas de blame (fichier hors git ou non suivi)".to_string(),
                        };
                        d.blame = Some((line, version, text));
                    }
                }
                JobResult::LineHistory(result) => {
                    if let Some(lh) = self.line_history.as_mut() {
                        lh.loading = false;
                        match result {
                            Ok(commits) => {
                                lh.diffs = commits.iter().map(|c| history::parse_diff(&c.diff)).collect();
                                lh.commits = commits;
                            }
                            Err(e) => lh.error = Some(e),
                        }
                    }
                }
                JobResult::HistoryDiff { hash, path, result } => {
                    if self.history_key().as_deref() == Some(hash.as_str()) {
                        self.history.diff_loading = false;
                        let text = result.unwrap_or_else(|e| e);
                        let text = if text.trim().is_empty() { "(aucune différence)".to_string() } else { text };
                        let lines = history::parse_diff(&text);
                        let files = history::diff_files(&lines);
                        // a reload of the same diff (after staging…) keeps the folded files
                        let same = self.history.diff.as_ref().is_some_and(|d| d.hash == hash);
                        if !same {
                            self.history.collapsed.clear();
                        }
                        if let Some(p) = self.history.jump_path.take() {
                            if let Some(k) = files.iter().position(|f| f.path == p) {
                                self.history.jump = Some(history::FileJump::To(k));
                            }
                        }
                        self.history.diff = Some(HistoryDiff { hash, path, text, lines, files });
                    }
                }
                JobResult::HistoryDetails(hash, r) => {
                    if self.history_key().as_deref() == Some(hash.as_str()) {
                        match r {
                            Ok(d) => self.history.details = Some(d),
                            Err(e) => self.log(e),
                        }
                    }
                }
                JobResult::GitBranches(list) => {
                    let items: Vec<Item> =
                        list.into_iter().map(|b| Item { label: b.clone(), detail: String::new(), action: Action::Branch(b) }).collect();
                    self.open_overlay_with(Mode::Branches, items, String::new());
                }
                JobResult::GitOp { label, result, status } => {
                    self.git.busy = self.git.busy.saturating_sub(1);
                    if label.starts_with("stash") {
                        self.stash_reload();
                    }
                    if let Some((repo, Ok(st))) = status {
                        if self.repo.as_ref() == Some(&repo) {
                            self.git.status = Some(st);
                        }
                    }
                    match result {
                        Ok(out) => {
                            self.log(out);
                            self.git.op_error = None;
                            self.set_status(format!("git {label} : OK"));
                        }
                        Err(e) => {
                            self.log(e.clone());
                            if self.git_shown() {
                                let first = e.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
                                self.history.notice = Some((format!("git {label} a échoué : {first}"), err()));
                            }
                            self.git.op_error = Some((label.clone(), e));
                            self.set_status(format!("git {label} : échec (détail dans le panneau Git)"));
                        }
                    }
                    // the rebase stopped on the commit to split: open the assistant on it
                    if label == "rebase (découpage)" && self.split.is_none() && self.history.repo.as_deref().is_some_and(crate::split::rebase_in_progress) {
                        self.resume_split();
                    }
                    self.git_refresh();
                    // staging, unstaging or discarding does not move branches: keep the selection
                    let refs = !matches!(label.as_str(), "add" | "restore" | "apply" | "rm");
                    if self.git_shown() && refs {
                        self.load_history();
                    }
                    if let (true, Some(staged)) = (self.git_shown(), self.history.wip_sel) {
                        self.load_wip_all(staged);
                    }
                    if let Some(t) = self.tree.as_mut() {
                        t.refresh();
                    }
                    for i in 0..self.docs.len() {
                        if self.docs[i].path.is_some() {
                            self.analyze(i, false);
                        }
                    }
                }
                JobResult::NewTab { title, text, syntax } => {
                    self.history.open = false;
                    self.new_scratch(title, text, &syntax, true);
                }
                JobResult::SplitStarted(repo, r) => {
                    self.history.loading = false;
                    match r {
                        Ok((info, files)) => {
                            self.set_status(format!("Découpage de {} : choisis le contenu de chaque partie", info.short));
                            let mut s = SplitUi {
                                repo,
                                message: info.message.clone(),
                                info,
                                files: Vec::new(),
                                sel: Vec::new(),
                                folded: HashSet::new(),
                                parts: Vec::new(),
                                busy: false,
                                error: None,
                                confirm_abort: false,
                                shown: true,
                            };
                            s.set_files(files);
                            self.split = Some(s);
                            self.history.notice = None;
                        }
                        Err(e) => {
                            self.log(e.clone());
                            self.history.notice = Some((e, err()));
                        }
                    }
                    self.after_split_change();
                }
                JobResult::SplitChanges { result, committed } => {
                    if let Some(s) = self.split.as_mut() {
                        s.busy = false;
                        if let Some(c) = committed {
                            s.parts.push(c);
                            s.message.clear();
                        }
                        match result {
                            Ok(files) => s.set_files(files),
                            Err(e) => s.error = Some(e),
                        }
                    }
                    self.after_split_change();
                }
                JobResult::SplitEnded { aborted, result } => {
                    match result {
                        Ok(_) => {
                            let n = self.split.take().map_or(0, |s| s.parts.len());
                            let msg = if aborted {
                                "Découpage annulé : tout est revenu comme avant (commit d'origine rétabli).".to_string()
                            } else {
                                format!("Commit découpé en {n} commits.")
                            };
                            self.set_status(msg.clone());
                            self.history.notice = Some((msg, ok()));
                            self.history.open = true;
                            self.prefs.open = false;
                            self.load_history();
                        }
                        Err(e) => {
                            self.log(e.clone());
                            if let Some(s) = self.split.as_mut() {
                                s.busy = false;
                                s.error = Some(e);
                            }
                        }
                    }
                    self.after_split_change();
                }
                JobResult::Picked(kind, path) => {
                    self.dialog_open = false;
                    if let Err(e) = &path {
                        self.set_status(e.clone());
                    }
                    if let Ok(Some(p)) = path {
                        let prompt = match kind {
                            crate::os::Pick::Folder => Prompt::OpenFolder,
                            crate::os::Pick::File => Prompt::OpenFile,
                            crate::os::Pick::SaveAs => Prompt::SaveAs,
                        };
                        self.submit_prompt(prompt, p.display().to_string());
                    }
                }
                JobResult::Repos(root, repos) => {
                    if self.root.as_ref() == Some(&root) {
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
                }
                JobResult::Update { manual, result } => match result {
                    Ok(Some(u)) => {
                        self.log(format!("Florm {} est disponible : {}", u.version, u.page));
                        if manual {
                            self.set_status(format!("Florm {} est disponible (en bas à droite)", u.version));
                        }
                        self.update = Some(u);
                    }
                    Ok(None) if manual => self.set_status(format!("Florm est à jour ({})", crate::update::current())),
                    Ok(None) => {}
                    Err(e) => {
                        if manual {
                            self.set_status("Recherche de mise à jour impossible (voir Sortie)");
                        }
                        self.log(e);
                    }
                },
                JobResult::DockerList(list) => self.docker_listed(list),
                JobResult::DockerFollow { name, result } => self.docker_followed(name, result),
                JobResult::DockerDone { label, result } => self.docker_done(label, result),
                JobResult::TestsReady { tags, result } => self.tests_ready(tags, result),
                JobResult::UpdateInstalled { version, result } => {
                    self.update_installing = false;
                    match result {
                        Ok(exe) => {
                            self.log(format!("Florm {version} est installé : redémarrez pour l'utiliser"));
                            self.set_status(format!("Florm {version} est installé : « Redémarrer » en bas à droite"));
                            self.update_ready = Some(exe);
                        }
                        Err(e) => {
                            self.log(format!("Mise à jour vers Florm {version} impossible : {e}"));
                            self.set_status("Mise à jour impossible (voir Sortie) : le fichier reste téléchargeable en bas à droite");
                        }
                    }
                }
                JobResult::Fonts(list) => {
                    self.prefs.font_list = Some(list);
                    self.prefs.font_list_loading = false;
                }
                JobResult::SymbolIndex(root, idx, partial) => {
                    if self.root.as_ref() == Some(&root) {
                        let what = if partial { "Dépôts en cours indexés" } else { "Projet indexé" };
                        self.log(format!("{what} : {} symboles dans {} fichiers", idx.len(), idx.files.len()));
                        self.index.symbols = Some(idx);
                    }
                }
                JobResult::Search(gen, out) => {
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
                JobResult::Plugin { uid, version, title, output, result } => {
                    self.apply_plugin_result(uid, version, title, output, result)
                }
            }
        }
    }

    fn apply_plugin_result(&mut self, uid: u64, version: u64, title: String, output: Output, result: Result<Vec<String>, String>) {
        let outs = match result {
            Ok(o) => o,
            Err(e) => {
                self.log(format!("[{title}] {e}"));
                self.bottom = Some(BottomTab::Output);
                self.set_status(format!("Plugin « {title} » : erreur"));
                return;
            }
        };
        match output {
            Output::None => {}
            Output::Panel => {
                self.log(format!("── {title} ──"));
                for o in outs {
                    self.log(o);
                }
                self.bottom = Some(BottomTab::Output);
            }
            Output::NewTab => self.new_scratch(title, outs.join("\n"), "Plain Text", false),
            _ => {
                let Some(doc) = self.docs.iter_mut().find(|d| d.uid == uid) else { return };
                if doc.version != version {
                    self.set_status("Le document a changé pendant le plugin : résultat ignoré");
                    return;
                }
                // Keep the trailing newline only if the input had one.
                let fit = |input: &str, out: String| -> String {
                    if !input.ends_with('\n') {
                        out.strip_suffix('\n').map(String::from).unwrap_or(out)
                    } else {
                        out
                    }
                };
                match output {
                    Output::ReplaceBuffer => {
                        let cur = doc.rope.to_string();
                        let new = fit(&cur, outs.concat());
                        doc.set_text(&new);
                    }
                    Output::ReplaceSelection if outs.len() == doc.sels.len() => {
                        let items: Vec<Edit> = doc
                            .sels
                            .iter()
                            .zip(outs)
                            .map(|(s, o)| {
                                let t = fit(&doc.slice(s.start(), s.end()), o);
                                let n = t.chars().count();
                                Edit { start: s.start(), end: s.end(), text: t, anchor: 0, head: n }
                            })
                            .collect();
                        doc.edit(EditKind::Other, items);
                    }
                    Output::Insert => {
                        let t = fit("", outs.concat());
                        doc.insert_text(&t, false);
                    }
                    _ => {}
                }
                self.set_status(format!("Plugin « {title} » : OK"));
            }
        }
    }
}
