//! The review of Claude's changes: a private index holding the files as they were, blocks
//! kept (added to it) or rejected (taken out of the file).

use super::*;

/// Review index of a repository for conversation `conv` (inside .git: not seen by git status,
/// not committed); each conversation has its own.
pub(super) fn review_index_of(repo: &Path, conv: u64) -> PathBuf {
    let name = if conv == 0 { "florm-review-index".to_string() } else { format!("florm-review-index-{conv}") };
    let p = git::stdout(repo, &["rev-parse", "--git-path", &name]).unwrap_or_default();
    let p = PathBuf::from(p.trim());
    if p.is_absolute() {
        p
    } else {
        repo.join(p)
    }
}

/// git with the review index instead of the real one.
pub(in crate::app) fn rgit(repo: &Path, index: &Path, args: &[&str], input: Option<&str>) -> Result<String, String> {
    git::run_with(repo, args, &[("GIT_INDEX_FILE", &index.to_string_lossy())], input)
}

/// The changes since the review started: modified files, then new ones.
pub(super) fn review_diff(repo: &Path, index: &Path) -> Result<String, String> {
    let mut text = rgit(repo, index, &["-c", "core.quotePath=false", "diff"], None)?;
    let others = rgit(repo, index, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"], None).unwrap_or_default();
    for p in others.lines().filter(|p| !p.is_empty()).take(200) {
        if std::fs::metadata(repo.join(p)).is_ok_and(|m| m.len() > 2_000_000) {
            continue;
        }
        let d = match git::run(repo, &["diff", "--no-index", "--", "/dev/null", p]) {
            Err(e) if e.starts_with("diff --git") => e,
            Ok(o) => o,
            Err(_) => continue,
        };
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&d);
    }
    Ok(text)
}

impl App {
    /// The review starts from the files as they are now.
    pub(super) fn claude_start_review(&mut self) {
        let Some(repo) = self.claude.dir.clone().filter(|d| d.join(".git").exists()) else { return };
        let index = review_index_of(&repo, self.claude.id);
        let r = rgit(&repo, &index, &["read-tree", "HEAD"], None).and_then(|_| rgit(&repo, &index, &["add", "-A"], None));
        match r {
            Ok(_) => {
                self.claude.review_index = Some(index);
                self.claude.review = None;
                self.claude.collapsed.clear();
            }
            Err(e) => self.claude.items.push(Item::Error(tf!("Revue impossible : {e}", "Review impossible: {e}"))),
        }
    }

    pub(in crate::app) fn claude_reload_review(&mut self) {
        let (Some(repo), Some(index)) = (self.claude.dir.clone(), self.claude.review_index.clone()) else { return };
        self.claude.review_loading = true;
        let id = self.claude.id;
        self.jobs.spawn(move || JobResult::ClaudeReview(id, review_diff(&repo, &index)));
    }

    /// The review of conversation `id` read again (wherever the conversation is now).
    pub(in crate::app) fn claude_review_loaded(&mut self, id: u64, r: Result<String, String>) {
        let Some(c) = std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).find(|c| c.id == id) else { return };
        c.review_loading = false;
        let text = r.unwrap_or_else(|e| e);
        let lines = history::parse_diff(&text);
        let files = history::diff_files(&lines);
        c.review = Some(HistoryDiff { hash: String::new(), path: None, text, lines, files });
    }

    /// Keeps or rejects a block or a file of the review.
    pub(super) fn claude_review_action(&mut self, click: DiffClick) {
        let (Some(repo), Some(index), Some(d)) = (self.claude.dir.clone(), self.claude.review_index.clone(), self.claude.review.as_ref()) else { return };
        let idx = match click {
            DiffClick::Line(i) | DiffClick::Hunk(i, _) | DiffClick::File(i, _) => i,
        };
        let Some(path) = history::file_at(&d.text, idx) else { return };
        let new_file = history::file_patch(&d.text, idx).is_some_and(|f| f.contains("\n--- /dev/null"));
        let text = d.text.clone();
        let r = match click {
            DiffClick::Line(_) | DiffClick::File(_, DiffAct::Open) => {
                let line = history::target_line(&d.lines, idx);
                self.pr_open_path(&repo.join(&path), line);
                return;
            }
            DiffClick::File(_, DiffAct::Stage) => rgit(&repo, &index, &["add", "-A", "--", &path], None),
            DiffClick::Hunk(_, DiffAct::Stage) if new_file => rgit(&repo, &index, &["add", "--", &path], None),
            DiffClick::Hunk(i, DiffAct::Stage) => match history::hunk_patch(&text, i) {
                Some(p) => rgit(&repo, &index, &["apply", "--cached", "--recount", "-"], Some(&p)),
                None => return,
            },
            DiffClick::File(..) | DiffClick::Hunk(..) if new_file => std::fs::remove_file(repo.join(&path)).map(|_| String::new()).map_err(|e| e.to_string()),
            DiffClick::File(..) => {
                // the file as it was (or removed again when Claude deleted it… restored)
                rgit(&repo, &index, &["checkout-index", "-f", "--", &path], None)
            }
            DiffClick::Hunk(i, _) => match history::hunk_patch(&text, i) {
                Some(p) => git::run_with(&repo, &["apply", "-R", "--recount", "-"], &[], Some(&p)),
                None => return,
            },
        };
        // shown at once without it, git's new diff follows
        if let Some(d) = self.claude.review.as_mut() {
            d.text = history::without(&d.text, idx, matches!(click, DiffClick::File(..)));
            d.lines = history::parse_diff(&d.text);
            d.files = history::diff_files(&d.lines);
        }
        if let Err(e) = r {
            self.claude.items.push(Item::Error(format!("{path} : {e}")));
        }
        self.last_disk_check = Instant::now() - Duration::from_secs(10);
        self.claude_reload_review();
        self.git_refresh();
    }

    /// Keeps everything (the review starts again from here) or rejects everything.
    pub(super) fn claude_review_all(&mut self, keep: bool) {
        let (Some(repo), Some(index)) = (self.claude.dir.clone(), self.claude.review_index.clone()) else { return };
        let r = if keep {
            rgit(&repo, &index, &["add", "-A"], None)
        } else {
            let news = rgit(&repo, &index, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"], None).unwrap_or_default();
            for p in news.lines().filter(|p| !p.is_empty()) {
                let _ = std::fs::remove_file(repo.join(p));
            }
            rgit(&repo, &index, &["checkout-index", "-a", "-f"], None)
        };
        if let Err(e) = r {
            self.claude.items.push(Item::Error(e));
        }
        self.claude.confirm_reject = false;
        self.last_disk_check = Instant::now() - Duration::from_secs(10);
        self.claude_reload_review();
        self.git_refresh();
    }

    pub(super) fn pr_open_path(&mut self, path: &Path, line: usize) {
        if self.open_path(path.to_path_buf()) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.goto_line(line.saturating_sub(1), 0);
            }
            self.pending_focus = true;
        }
    }
}
