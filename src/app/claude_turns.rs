//! The conversation question by question: each message of the user starts a turn, which can be
//! folded (▾ / ▸), whose changes can be shown alone (±), and to which the files and the
//! conversation can go back (↶). Before each message the files are written into git as a tree,
//! through a private index (nothing in the history, nothing staged): its checkpoint.

use super::*;
use super::claude_ui::{rgit, Item};
use serde::{Deserialize, Serialize};

/// The state before a message of the user: where to go back to.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    /// Index of the message in the conversation.
    pub(super) item: usize,
    /// The files just before it (a git tree; None outside a git repository).
    pub(super) tree: Option<String>,
    /// The Claude Code session then, and its last message (`--resume-session-at`); None: the
    /// message started the conversation.
    pub(super) session: Option<String>,
    pub(super) after: Option<String>,
}

/// The changes of a single question, shown instead of the review.
pub(super) struct TurnDiff {
    pub(super) item: usize,
    pub(super) diff: Option<HistoryDiff>,
    pub(super) collapsed: HashSet<usize>,
}

/// What goes back to before a message.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Rewind {
    Both,
    Conversation,
    Files,
}

/// A private index of conversation `conv` in `repo` (inside .git: not seen by git status).
fn snap_index(repo: &Path, conv: u64, what: &str) -> PathBuf {
    let name = format!("florm-snap-index-{conv}{what}");
    let p = PathBuf::from(git::stdout(repo, &["rev-parse", "--git-path", &name]).unwrap_or_default().trim());
    if p.is_absolute() {
        p
    } else {
        repo.join(p)
    }
}

/// The files of `repo` now, written as a git tree (new files included, ignored ones not). The
/// index is kept between calls: only the files changed since are read again.
pub(super) fn snapshot(repo: &Path, index: &Path) -> Result<String, String> {
    rgit(repo, index, &["add", "-A"], None)?;
    let tree = rgit(repo, index, &["write-tree"], None)?;
    Ok(tree.trim().to_string())
}

/// The diff between two trees, or between `from` and the files now (`to` None).
fn turn_diff_text(repo: &Path, index: &Path, from: &str, to: Option<&str>) -> Result<String, String> {
    let to = match to {
        Some(t) => t.to_string(),
        None => snapshot(repo, index)?,
    };
    git::stdout(repo, &["-c", "core.quotePath=false", "diff", "--no-renames", from, &to])
}

/// The files of `repo` put back as in `tree`: those created since are deleted, the others
/// written again (only the ones that differ). Returns how many files changed.
pub(super) fn restore(repo: &Path, snap: &Path, work: &Path, tree: &str) -> Result<usize, String> {
    let now = snapshot(repo, snap)?;
    let changes = git::stdout(repo, &["-c", "core.quotePath=false", "diff", "--no-renames", "--name-status", "-z", tree, &now])?;
    let mut fields = changes.split('\0').filter(|f| !f.is_empty());
    let (mut gone, mut back) = (Vec::new(), Vec::new());
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        if status.starts_with('A') {
            gone.push(path.to_string());
        } else {
            back.push(path.to_string());
        }
    }
    for p in &gone {
        let full = repo.join(p);
        std::fs::remove_file(&full).map_err(|e| format!("{p} : {e}"))?;
        // the folders it leaves empty go with it
        let mut dir = full.parent();
        while let Some(d) = dir.filter(|d| *d != repo && std::fs::remove_dir(d).is_ok()) {
            dir = d.parent();
        }
    }
    if !back.is_empty() {
        rgit(repo, work, &["read-tree", tree], None)?;
        let list: String = back.iter().map(|p| format!("{p}\0")).collect();
        rgit(repo, work, &["checkout-index", "-f", "-z", "--stdin"], Some(&list))?;
    }
    Ok(gone.len() + back.len())
}

/// What a folded turn holds: its steps (answers and tools) and the files it changed.
pub(super) fn turn_summary(items: &[Item], k: usize) -> (usize, usize) {
    let mut files: Vec<&str> = Vec::new();
    let mut steps = 0;
    for it in items.iter().skip(k + 1).take_while(|i| !matches!(i, Item::User(..))) {
        match it {
            Item::Tool { name, summary, .. } => {
                steps += 1;
                if matches!(name.as_str(), "Edit" | "MultiEdit" | "Write" | "NotebookEdit") && !files.contains(&summary.as_str()) {
                    files.push(summary);
                }
            }
            Item::Text(_) | Item::Streaming(_) => steps += 1,
            _ => {}
        }
    }
    (steps, files.len())
}

impl App {
    /// The checkpoint before a message starting work: the files as they are now, and where the
    /// conversation is. `item`: where the message goes.
    pub(super) fn claude_checkpoint(&self, item: usize) -> Checkpoint {
        let c = &self.claude;
        let tree = c.dir.as_deref().filter(|d| d.join(".git").exists()).and_then(|repo| snapshot(repo, &snap_index(repo, c.id, "")).ok());
        Checkpoint { item, tree, session: c.session_id.clone(), after: c.last_uuid.clone() }
    }

    /// The changes of the question at `k` alone: from its checkpoint to the next one (or to now).
    pub(super) fn claude_show_turn_diff(&mut self, k: usize) {
        let c = &self.claude;
        let Some(repo) = c.dir.clone() else { return };
        let Some(i) = c.checkpoints.iter().position(|p| p.item == k && p.tree.is_some()) else { return };
        let from = c.checkpoints[i].tree.clone().unwrap_or_default();
        let to = c.checkpoints[i + 1..].iter().find_map(|p| p.tree.clone());
        let (id, index) = (c.id, snap_index(&repo, c.id, "-diff"));
        self.claude.turn_diff = Some(TurnDiff { item: k, diff: None, collapsed: HashSet::new() });
        self.jobs.spawn(move || JobResult::ClaudeTurnDiff(id, k, turn_diff_text(&repo, &index, &from, to.as_deref())));
    }

    /// The changes of a question of conversation `id`, read.
    pub(super) fn claude_turn_diff_loaded(&mut self, id: u64, k: usize, r: Result<String, String>) {
        let Some(c) = std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).find(|c| c.id == id) else { return };
        let Some(td) = c.turn_diff.as_mut().filter(|t| t.item == k) else { return };
        let text = r.unwrap_or_else(|e| e);
        let lines = history::parse_diff(&text);
        let files = history::diff_files(&lines);
        td.diff = Some(HistoryDiff { hash: String::new(), path: None, text, lines, files });
    }

    /// Back to before the message at `k`: the files as they were then, and / or the conversation
    /// without this message and what followed (the message goes back to the field).
    pub(super) fn claude_rewind(&mut self, k: usize, how: Rewind) {
        self.claude.confirm_rewind = None;
        if self.claude.busy {
            self.set_status(t!("Claude travaille : arrête-le d'abord (■ Arrêter)", "Claude is working: stop it first (■ Stop)"));
            return;
        }
        let Some(cp) = self.claude.checkpoints.iter().find(|p| p.item == k).cloned() else { return };
        let mut done = Vec::new();
        if let (true, Some(repo), Some(tree)) = (how != Rewind::Conversation, self.claude.dir.clone(), cp.tree.as_deref()) {
            let id = self.claude.id;
            match restore(&repo, &snap_index(&repo, id, ""), &snap_index(&repo, id, "-rewind"), tree) {
                Ok(n) => done.push(tf!("{n} fichier(s) remis comme avant", "{n} file(s) put back as before")),
                Err(e) => {
                    self.claude.items.push(Item::Error(tf!("Fichiers non remis comme avant : {e}", "Files not put back: {e}")));
                    return;
                }
            }
            self.last_disk_check = Instant::now() - Duration::from_secs(10);
            self.claude_reload_review();
            self.git_refresh();
        }
        let c = &mut self.claude;
        if how != Rewind::Files {
            let (text, notes, images) = match c.items.get(k) {
                Some(Item::User(t, _, notes, images)) => (t.clone(), notes.clone(), images.clone()),
                _ => Default::default(),
            };
            c.items.truncate(k);
            c.heights.truncate(k);
            c.checkpoints.retain(|p| p.item < k);
            c.folded.retain(|f| *f < k);
            c.checks.retain(|(a, _)| *a < k);
            c.answers = None;
            if c.turn_diff.as_ref().is_some_and(|t| t.item >= k) {
                c.turn_diff = None;
            }
            if c.input.trim().is_empty() {
                c.input = text;
            }
            // the texts given with it (a pasted text…) come back in the context
            if c.attach.is_empty() {
                let dir = c.dir.clone().unwrap_or_default();
                c.attach = notes.into_iter().map(|n| super::claude_ui::Attach::note(dir.clone(), n)).collect();
                c.attach.extend(images.into_iter().filter(|(_, p)| p.exists()).map(|i| super::claude_ui::Attach::image(dir.clone(), i)));
            }
            // the process stops; the next message goes on from a copy of the session cut there
            c.session = None;
            c.session_id = cp.after.as_ref().and(cp.session.clone());
            c.resume_at = cp.after.clone();
            c.last_uuid = cp.after.clone();
            c.context = (0, c.context.1);
            c.context_high = false;
            if !c.items.iter().any(|i| matches!(i, Item::User(..))) {
                c.wish = None;
            }
            c.focus = true;
            done.push(t!("la conversation reprend avant ce message", "the conversation goes on from before this message").into());
        }
        let what = done.join(", ");
        self.claude.items.push(Item::Info(tf!("— ↶ retour en arrière : {what} —", "— ↶ rewind: {what} —")));
        self.claude_save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoints_restore_the_files() {
        let dir = std::env::temp_dir().join(format!("florm_turns_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git::run(&dir, &["init", "-q"]).unwrap();
        std::fs::write(dir.join("a.txt"), "un\n").unwrap();
        std::fs::write(dir.join("gone.txt"), "parti\n").unwrap();
        git::run(&dir, &["add", "."]).unwrap();
        git::run(&dir, &["-c", "user.email=a@b", "-c", "user.name=a", "commit", "-qm", "init"]).unwrap();
        std::fs::write(dir.join("mine.txt"), "pas commité\n").unwrap();
        let (snap, work) = (snap_index(&dir, 1, ""), snap_index(&dir, 1, "-rewind"));
        let before = snapshot(&dir, &snap).unwrap();
        // the turn: a file changed, one deleted, one created in a new folder
        std::fs::write(dir.join("a.txt"), "deux\n").unwrap();
        std::fs::remove_file(dir.join("gone.txt")).unwrap();
        std::fs::create_dir_all(dir.join("new/sub")).unwrap();
        std::fs::write(dir.join("new/sub/b.txt"), "nouveau\n").unwrap();
        let d = turn_diff_text(&dir, &snap_index(&dir, 1, "-diff"), &before, None).unwrap();
        assert!(d.contains("+deux") && d.contains("-parti") && d.contains("+nouveau") && !d.contains("pas commité"), "{d}");
        assert_eq!(restore(&dir, &snap, &work, &before).unwrap(), 3);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "un\n");
        assert_eq!(std::fs::read_to_string(dir.join("gone.txt")).unwrap(), "parti\n");
        assert!(!dir.join("new").exists());
        // what was there before the turn stays, and nothing is staged
        assert_eq!(std::fs::read_to_string(dir.join("mine.txt")).unwrap(), "pas commité\n");
        assert_eq!(git::run(&dir, &["diff", "--cached", "--name-only"]).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folded_turn_is_summed_up() {
        let tool = |name: &str, file: &str| Item::Tool { id: String::new(), name: name.into(), summary: file.into(), result: None };
        let items = vec![
            Item::User("a".into(), vec![], vec![], vec![]),
            Item::Text("je regarde".into()),
            tool("Read", "x.rs"),
            tool("Edit", "x.rs"),
            tool("Edit", "x.rs"),
            tool("Write", "y.rs"),
            Item::Usage { input: 1, cached: 0, output: 1, cost: 0.0 },
            Item::User("b".into(), vec![], vec![], vec![]),
            Item::Text("ok".into()),
        ];
        assert_eq!(turn_summary(&items, 0), (5, 2));
        assert_eq!(turn_summary(&items, 7), (1, 0));
    }
}
