//! Results of background jobs.

use super::*;

mod file_results;
mod git_results;

impl App {
    pub(super) fn process_jobs(&mut self) {
        while let Ok(r) = self.jobs.rx.try_recv() {
            match r {
                JobResult::ClaudeCommit(repo, r) => self.claude_commit_read(repo, r),
                JobResult::ConflictsForClaude(repo, n, text) => self.conflicts_sent(repo, n, text),
                JobResult::Stashes(repo, list) => {
                    if self.repo.as_ref() == Some(&repo) {
                        self.git.stashes = list;
                    }
                }
                JobResult::DiskCheck { dirs, stamps, files } => {
                    self.watch.busy = false;
                    if self.tree.as_mut().is_some_and(|t| t.apply_scan(dirs)) {
                        self.index.files_at = None;
                    }
                    self.check_git_stamps(stamps);
                    self.files_checked(files);
                }
                JobResult::Diffed { uid, version, hunks } => {
                    // a newer version is diffed in its turn
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid && d.version == version) {
                        d.set_hunks(hunks, version);
                    }
                }
                JobResult::Rewritten { uid, version, what, lines, then_save, result } => self.on_rewritten(uid, version, what, lines, then_save, result),
                JobResult::Analyzed { uid, diags, base, .. } => self.on_analyzed(uid, diags, base),
                JobResult::Lint { uid, version, diags, linter } => self.on_lint(uid, version, diags, linter),
                JobResult::LintedInProcess { uid, version, diags } => {
                    // a newer version is checked in its turn
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid && d.version == version) {
                        d.set_lint_diagnostics(diags);
                    }
                }
                JobResult::ToolInstalled(r) => self.on_tool_installed(r),
                JobResult::PackStatus(list, status) => self.on_pack_status(list, status),
                JobResult::PackInstalled(r) => self.on_pack_installed(r),
                JobResult::PythonReady(r) => self.on_python_ready(r),
                JobResult::CatalogFetched(errors) => self.on_catalog_fetched(errors),
                JobResult::DebugStarted(r) => self.debug_started(r),
                JobResult::FileMatches { gen, query, matched, top } => self.on_file_matches(gen, query, matched, top),
                JobResult::FileIndex(root, list) => self.on_file_index(root, list),
                JobResult::GitStatus { selected, statuses, remotes, stashes } => self.on_git_status(selected, statuses, remotes, stashes),
                JobResult::PrList(repo, r) => self.pr_list_loaded(repo, r),
                JobResult::ClaudeCommits(id, out) => self.claude_commits_loaded(id, out),
                JobResult::ClaudeAuth(name, r) => self.claude_auth_loaded(name, r),
                JobResult::PrCommitFetched(repo, sha, r) => match r {
                    Ok(_) => self.git_show_commit(repo, sha),
                    Err(e) => {
                        self.log(e.clone());
                        self.set_error(tf!("Commit {} pas récupéré : {}", "Commit {} not fetched: {}", &sha[..sha.len().min(8)], e.lines().next().unwrap_or("")));
                    }
                },
                JobResult::CommitMessage(r) => self.commit_message_ready(r),
                JobResult::AiReview(key, r) => self.ai_review_loaded(key, r),
                JobResult::Spelled { uid, version, list } => self.on_spelled(uid, version, list),
                JobResult::AiRefined { key, id, result } => self.ai_refined(key, id, result),
                JobResult::AiComplete(n, r) => self.ai_complete_loaded(n, r),
                JobResult::CopilotState(r) => self.copilot_state(r),
                JobResult::CopilotCode(r) => self.copilot_code(r),
                JobResult::GhBase(repo, r) => self.gh_base_loaded(repo, r),
                JobResult::PrCreateInfo { repo, branch, upstream, remotes, title, body, refs } => self.pr_create_info(repo, branch, upstream, remotes, title, body, refs),
                JobResult::GhBranchPrs(repo, branch, r) => self.gh_branch_prs_loaded(repo, branch, r),
                JobResult::GhCommitPrs(sha, r) => self.gh_commit_prs_loaded(sha, r),
                JobResult::PrCreated(repo, r) => self.pr_created(repo, r),
                JobResult::PrBody(r) => self.pr_body_ready(r),
                JobResult::PrDetail(repo, n, r) => self.pr_detail_loaded(repo, n, r),
                JobResult::PrCommitDiff(sha, r) => self.pr_commit_diff_loaded(sha, r),
                JobResult::PrOp { label, result, reload, draft } => self.pr_op_done(label, result, reload, draft),
                JobResult::PrLog(name, r) => self.pr_log_loaded(name, r),
                JobResult::Compare(title, r) => self.compare_loaded(title, r),
                JobResult::History(repo, gen, r) => self.on_history(repo, gen, r),
                JobResult::Blame { uid, line, version, result } => self.on_blame(uid, line, version, result),
                JobResult::LineHistory(result) => self.on_line_history(result),
                JobResult::LineHistoryDiff(hash, result) => self.on_line_history_diff(hash, result),
                JobResult::HistoryDiff { hash, path, result } => self.on_history_diff(hash, path, result),
                JobResult::HistoryDetails(hash, r) => self.on_history_details(hash, r),
                JobResult::GitBranches(list, pick, target) => self.branches_ready(list, pick, target),
                JobResult::RebasePlan(repo, r) => self.rebase_plan_ready(repo, r),
                JobResult::CleanPreview(r) => self.clean_preview_ready(r),
                JobResult::LastCommitMessage(r) => match r {
                    // what was typed meanwhile stays
                    Ok(m) if self.git.amend && self.git.commit_msg.trim().is_empty() => self.git.commit_msg = m,
                    Ok(_) => {}
                    Err(e) => {
                        self.git.amend = false;
                        self.set_error(tf!("Pas de dernier commit à modifier : {}", "No last commit to amend: {}", e.lines().next().unwrap_or("")));
                    }
                },
                JobResult::GitOp { label, result, status } => self.on_git_op(label, result, status),
                JobResult::NewTab { title, text, syntax } => {
                    self.new_scratch(title, text, &syntax, true);
                }
                JobResult::Picked(kind, paths) => self.on_picked(kind, paths),
                JobResult::ClaudeImage { label, png } => match png {
                    Ok(png) => self.claude_attach_image(png, label),
                    Err(e) => self.set_error(tf!("{label} : {e}", "{label}: {e}")),
                },
                JobResult::Repos(root, repos) => self.apply_repos(root, repos),
                JobResult::RepoInit { root, dir, result, repos } => self.on_repo_init(root, dir, result, repos),
                JobResult::Update { manual, result } => self.on_update(manual, result),
                JobResult::Shared(op, result) => self.shared_done(op, result),
                JobResult::UpdateInstalled { version, result } => self.on_update_installed(version, result),
                JobResult::Fonts(list) => {
                    self.prefs.font_list = Some(list);
                    self.prefs.font_list_loading = false;
                }
                JobResult::SymbolIndex(root, idx, partial) => self.on_symbol_index(root, idx, partial),
                JobResult::Search(gen, out) => self.on_search(gen, out),
            }
        }
    }

    /// A tool installed by Florm (linter, language server…).
    pub(super) fn on_tool_installed(&mut self, r: Result<String, String>) {
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
                self.set_error(t!("Installation échouée (voir Sortie)", "Installation failed (see Output)"));
                self.bottom = Some(BottomTab::Output);
            }
        }
        for d in self.docs.iter_mut() {
            d.lint_version = u64::MAX; // re-lint with the new tool
        }
    }

    /// The check for a new version of Florm.
    pub(super) fn on_update(&mut self, manual: bool, result: Result<Option<crate::update::Update>, String>) {
        match result {
        Ok(Some(u)) => {
            self.log(tf!("Florm {} est disponible : {}", "Florm {} is available: {}", u.version, u.page));
            if manual {
                self.set_status(tf!("Florm {} est disponible (en bas à droite)", "Florm {} is available (bottom right)", u.version));
            }
            self.upd.found = Some(u);
        }
        Ok(None) if manual => self.set_status(tf!("Florm est à jour ({})", "Florm is up to date ({})", crate::update::current())),
        Ok(None) => {}
        Err(e) => {
            if manual {
                self.set_error(t!("Recherche de mise à jour impossible (voir Sortie)", "Cannot check for updates (see Output)"));
            }
            self.log(e);
        }
        }
    }

    /// A new version of Florm installed.
    pub(super) fn on_update_installed(&mut self, version: String, result: Result<std::path::PathBuf, String>) {
        self.upd.installing = false;
        match result {
            Ok(exe) => {
                self.log(tf!("Florm {version} est installé : redémarrez pour l'utiliser", "Florm {version} is installed: restart to use it"));
                // nothing unsaved: `update` restarts Florm right away
                if self.docs.iter().any(|d| d.is_dirty()) {
                    self.set_status(tf!("Florm {version} est installé : « Redémarrer » en bas à droite", "Florm {version} is installed: “Restart” at the bottom right"));
                } else {
                    self.set_status(tf!("Florm {version} est installé : Florm redémarre", "Florm {version} is installed: Florm restarts"));
                }
                self.upd.ready = Some(exe);
            }
            Err(e) => {
                self.log(tf!("Mise à jour vers Florm {version} impossible : {e}", "Cannot update to Florm {version}: {e}"));
                self.set_error(t!("Mise à jour impossible (voir Sortie) : le fichier reste téléchargeable en bas à droite", "Update impossible (see Output): the file can still be downloaded at the bottom right"));
            }
        }
    }
}
