//! The changes not committed read as blocks (hunks, or whole files), so that Claude can share
//! them out by idea (the guided tour, `crate::tour`).

use crate::git;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Hunk {
    /// "@@ … @@" line followed by the hunk lines.
    pub lines: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct FileChange {
    pub path: String,
    /// 'M', 'A' or 'D'.
    pub kind: char,
    /// Diff lines before the first hunk (diff --git, index, ---, +++).
    pub header: Vec<String>,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
    /// New file not yet in the index: only taken whole.
    pub untracked: bool,
}

impl FileChange {
    /// Whether its hunks can be taken apart (else the file goes whole).
    pub fn partial_ok(&self) -> bool {
        self.kind == 'M' && !self.binary && !self.untracked
    }
}

impl Hunk {
    /// Whether line `i` (index in `lines`) is an added or removed line.
    pub fn is_change(&self, i: usize) -> bool {
        i > 0 && self.lines.get(i).is_some_and(|l| l.starts_with('+') || l.starts_with('-'))
    }

}

fn git_dir_path(repo: &Path, name: &str) -> Option<std::path::PathBuf> {
    let p = git::stdout(repo, &["rev-parse", "--git-path", name]).ok()?;
    let p = Path::new(p.trim());
    Some(if p.is_absolute() { p.to_path_buf() } else { repo.join(p) })
}

pub fn rebase_in_progress(repo: &Path) -> bool {
    ["rebase-merge", "rebase-apply"].iter().any(|n| git_dir_path(repo, n).is_some_and(|p| p.exists()))
}

pub fn parse_diff(text: &str) -> Vec<FileChange> {
    let mut files: Vec<FileChange> = Vec::new();
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("diff --git ") {
            let path = rest.split_once(" b/").map_or(rest, |(_, b)| b).to_string();
            files.push(FileChange { path, kind: 'M', header: vec![l.to_string()], hunks: Vec::new(), binary: false, untracked: false });
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        if l.starts_with("@@") {
            f.hunks.push(Hunk { lines: vec![l.to_string()] });
        } else if let Some(h) = f.hunks.last_mut() {
            h.lines.push(l.to_string());
        } else {
            if l.starts_with("new file mode") {
                f.kind = 'A';
            } else if l.starts_with("deleted file mode") {
                f.kind = 'D';
            } else if l.starts_with("Binary files") || l.starts_with("GIT binary patch") {
                f.binary = true;
            } else if let Some(p) = l.strip_prefix("+++ b/") {
                f.path = p.trim_end_matches('\t').to_string();
            }
            f.header.push(l.to_string());
        }
    }
    files
}

/// Changes still to commit: working tree vs HEAD (staged or not), plus the new files `added` (untracked).
fn changes(repo: &Path, added: &[String]) -> Result<Vec<FileChange>, String> {
    let diff = git::stdout(repo, &["-c", "core.quotePath=false", "diff", "HEAD", "--no-color", "--no-ext-diff", "--no-renames", "-U3"])?;
    let mut files = parse_diff(&diff);
    let status = git::stdout(repo, &["status", "--porcelain", "-z", "--untracked-files=all"])?;
    for e in status.split('\0') {
        let Some(path) = e.strip_prefix("?? ") else { continue };
        if !added.iter().any(|a| a == path) {
            continue;
        }
        let bytes = std::fs::read(repo.join(path)).unwrap_or_default();
        let binary = bytes.iter().take(8192).any(|b| *b == 0);
        let text = String::from_utf8_lossy(&bytes);
        let mut lines = vec![tf!("@@ nouveau fichier : {} ligne(s) @@", "@@ new file: {} line(s) @@", text.lines().count())];
        if !binary {
            lines.extend(text.lines().take(400).map(|l| format!("+{l}")));
        }
        files.push(FileChange { path: path.to_string(), kind: 'A', header: Vec::new(), hunks: vec![Hunk { lines }], binary, untracked: true });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The changes not committed (staged or not, new files included), for the guided tour.
pub fn worktree_changes(repo: &Path) -> Result<Vec<FileChange>, String> {
    let untracked: Vec<String> = git::stdout(repo, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"])?
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    changes(repo, &untracked)
}

/// A part of the changes: a hunk, or a whole file (new, deleted,
/// binary: not splittable).
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub path: String,
    /// Its added and removed lines (the hunk, without its context); empty: the whole file.
    pub changed: Vec<String>,
    /// Hunk lines shown to read it ("@@" first).
    pub lines: Vec<String>,
    pub kind: char,
}

/// The blocks of `files`, in order.
pub fn blocks(files: &[FileChange]) -> Vec<Block> {
    let mut out = Vec::new();
    for f in files {
        if f.partial_ok() {
            for h in &f.hunks {
                let changed = (1..h.lines.len()).filter(|&i| h.is_change(i)).map(|i| h.lines[i].clone()).collect();
                out.push(Block { path: f.path.clone(), changed, lines: h.lines.clone(), kind: f.kind });
            }
        } else {
            let lines = f.hunks.iter().flat_map(|h| h.lines.iter().cloned()).collect();
            out.push(Block { path: f.path.clone(), changed: Vec::new(), lines, kind: f.kind });
        }
    }
    out
}

/// The blocks numbered B1, B2… for Claude; long ones are cut (their start says enough).
pub fn blocks_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for (k, b) in blocks.iter().enumerate() {
        let what = match b.kind {
            'A' => " (nouveau fichier)",
            'D' => " (supprimé)",
            _ => "",
        };
        out.push_str(&format!("### B{} — {}{what}\n", k + 1, b.path));
        let max = if out.len() > 120_000 { 6 } else { 80 };
        for l in b.lines.iter().take(max) {
            out.push_str(l);
            out.push('\n');
        }
        if b.lines.len() > max {
            out.push_str(&format!("… ({} lignes de plus)\n", b.lines.len() - max));
        }
        out.push('\n');
    }
    out
}
