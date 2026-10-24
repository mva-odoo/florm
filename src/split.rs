//! Splitting a commit in several: `git rebase -i` stopped on the commit (edit), `git reset HEAD~1`
//! so that its changes are back in the working tree, then partial commits (whole files or hunks)
//! until nothing is left, and `git rebase --continue` replays the commits that came after.

use crate::git;
use std::path::Path;

/// Argument that makes florm act as git's sequence editor (see `edit_todo`).
pub const TODO_FLAG: &str = "--florm-split-todo";

/// Rebase todo list: the first `pick` becomes `edit` (the commit to split comes first).
pub fn edit_todo(file: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let mut done = false;
    let out: Vec<String> = text
        .lines()
        .map(|l| {
            let t = l.trim_start();
            if !done && !t.is_empty() && !t.starts_with('#') {
                done = true;
                if let Some(rest) = t.strip_prefix("pick ").or_else(|| t.strip_prefix("p ")) {
                    return format!("edit {rest}");
                }
            }
            l.to_string()
        })
        .collect();
    std::fs::write(file, out.join("\n") + "\n").map_err(|e| e.to_string())
}

/// Sequence editor command running this executable with `TODO_FLAG`.
pub fn self_editor() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(format!("\"{}\" {TODO_FLAG}", exe.display().to_string().replace('\\', "/")))
}

#[derive(Clone, Debug)]
pub struct Info {
    pub short: String,
    pub subject: String,
    pub message: String,
    /// Files created by the commit: untracked after the reset.
    pub added: Vec<String>,
}

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
    /// New file not yet in the index: only committed whole.
    pub untracked: bool,
}

impl FileChange {
    /// Whether hunks (and lines of hunks) can be committed separately.
    pub fn partial_ok(&self) -> bool {
        self.kind == 'M' && !self.binary && !self.untracked
    }

    /// Selection with nothing chosen: one flag per line of each hunk (only +/- lines matter).
    pub fn empty_selection(&self) -> Vec<Vec<bool>> {
        self.hunks.iter().map(|h| vec![false; h.lines.len()]).collect()
    }
}

impl Hunk {
    /// Whether line `i` (index in `lines`) is an added or removed line.
    pub fn is_change(&self, i: usize) -> bool {
        i > 0 && self.lines.get(i).is_some_and(|l| l.starts_with('+') || l.starts_with('-'))
    }

    /// The hunk restricted to the chosen lines, like editing it in `git add -p` (e): an added line
    /// not chosen is dropped, a removed line not chosen stays as context. None if nothing is chosen.
    pub fn restricted(&self, chosen: &[bool]) -> Option<String> {
        if !(1..self.lines.len()).any(|i| self.is_change(i) && chosen.get(i).copied().unwrap_or(false)) {
            return None;
        }
        let mut out = vec![self.lines[0].clone()];
        let mut dropped = false;
        for (i, l) in self.lines.iter().enumerate().skip(1) {
            let on = chosen.get(i).copied().unwrap_or(false);
            match l.chars().next() {
                Some('+') if !on => {
                    dropped = true;
                    continue;
                }
                Some('-') if !on => out.push(format!(" {}", &l[1..])),
                // "\ No newline at end of file" belongs to the line before
                Some('\\') if dropped => {}
                _ => out.push(l.clone()),
            }
            dropped = false;
        }
        Some(out.join("\n"))
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

/// Commit on which the current interactive rebase stopped.
pub fn stopped_commit(repo: &Path) -> Option<String> {
    let sha = std::fs::read_to_string(git_dir_path(repo, "rebase-merge/stopped-sha")?).ok()?;
    git::stdout(repo, &["rev-parse", sha.trim()]).ok().map(|s| s.trim().to_string())
}

pub fn info(repo: &Path, hash: &str) -> Result<Info, String> {
    let out = git::stdout(repo, &["show", "-s", "--format=%h%x1f%s%x1f%B", hash])?;
    let mut f = out.splitn(3, '\x1f');
    let short = f.next().unwrap_or("").to_string();
    let subject = f.next().unwrap_or("").to_string();
    let message = f.next().unwrap_or("").trim().to_string();
    let added = git::stdout(repo, &["-c", "core.quotePath=false", "diff-tree", "-r", "--no-commit-id", "--name-only", "--diff-filter=A", hash])?
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    Ok(Info { short, subject, message, added })
}

/// Checks that `hash` can be split, then stops a rebase on it and un-commits it.
pub fn start(repo: &Path, hash: &str, seq_editor: &str) -> Result<Info, String> {
    if rebase_in_progress(repo) {
        return Err(t!("Un rebase est déjà en cours dans ce dépôt : termine-le ou annule-le d'abord.", "A rebase is already in progress in this repository: finish or abort it first.").into());
    }
    if !git::stdout(repo, &["status", "--porcelain", "--untracked-files=no"])?.trim().is_empty() {
        return Err(t!("Le dépôt a des modifications non validées : valide-les ou mets-les de côté (stash) avant de découper un commit.", "The repository has uncommitted changes: commit them or put them aside (stash) before splitting a commit.").into());
    }
    let parents = git::stdout(repo, &["rev-list", "--parents", "-n", "1", hash])?.split_whitespace().count() - 1;
    if parents == 0 {
        return Err(t!("Le premier commit du dépôt ne peut pas être découpé.", "The first commit of the repository cannot be split.").into());
    }
    if parents > 1 {
        return Err(t!("Un commit de merge ne peut pas être découpé.", "A merge commit cannot be split.").into());
    }
    if git::run(repo, &["merge-base", "--is-ancestor", hash, "HEAD"]).is_err() {
        return Err(t!("Le commit doit faire partie de la branche courante : fais d'abord un checkout de sa branche.", "The commit must be part of the current branch: checkout its branch first.").into());
    }
    if !git::stdout(repo, &["rev-list", "--merges", &format!("{hash}..HEAD")])?.trim().is_empty() {
        return Err(t!("Il y a des merges après ce commit : le découpage les aplatirait, il est donc refusé.", "There are merges after this commit: splitting would flatten them, so it is refused.").into());
    }
    let info = info(repo, hash)?;
    let env = [("GIT_SEQUENCE_EDITOR", seq_editor), ("GIT_EDITOR", "true")];
    git::run_with(repo, &["rebase", "-i", "--no-autosquash", &format!("{hash}^")], &env, None)?;
    if !rebase_in_progress(repo) {
        return Err(t!("Le rebase ne s'est pas arrêté sur le commit à découper.", "The rebase did not stop on the commit to split.").into());
    }
    let head = git::stdout(repo, &["rev-parse", "HEAD"])?;
    if head.trim() != hash {
        let _ = git::run(repo, &["rebase", "--abort"]);
        return Err(t!("Le rebase ne s'est pas arrêté sur le commit à découper : annulé.", "The rebase did not stop on the commit to split: aborted.").into());
    }
    git::run(repo, &["reset", "-q", "HEAD~1"])?;
    Ok(info)
}

fn parse_diff(text: &str) -> Vec<FileChange> {
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

/// Changes still to commit: working tree vs HEAD (staged or not), plus the commit's new files still untracked.
pub fn changes(repo: &Path, added: &[String]) -> Result<Vec<FileChange>, String> {
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

/// Choice of each +/- line of hunk `h` in `chosen`; a hunk without such lines (binary file) has one
/// flag, the one of its header line.
pub fn hunk_flags(h: &Hunk, chosen: &[bool]) -> Vec<bool> {
    let flag = |i: usize| chosen.get(i).copied().unwrap_or(false);
    let v: Vec<bool> = (1..h.lines.len()).filter(|&i| h.is_change(i)).map(flag).collect();
    if v.is_empty() {
        vec![flag(0)]
    } else {
        v
    }
}

/// Whether something of file `f` is chosen in `sel` (its lines, per hunk).
pub fn any_chosen(f: &FileChange, sel: &[Vec<bool>]) -> bool {
    f.hunks.iter().zip(sel).any(|(h, s)| hunk_flags(h, s).iter().any(|&b| b))
}

/// Whether every change of file `f` is chosen.
pub fn all_chosen(f: &FileChange, sel: &[Vec<bool>]) -> bool {
    f.hunks.iter().zip(sel).all(|(h, s)| hunk_flags(h, s).iter().all(|&b| b))
}

/// Stages the chosen lines (`sel[file][hunk][line]`), whole files when everything or a
/// non-splittable file is chosen. The index is reset first, so hunks (computed against HEAD)
/// apply to it as they are.
pub fn stage(repo: &Path, files: &[FileChange], sel: &[Vec<Vec<bool>>]) -> Result<(), String> {
    git::run(repo, &["reset", "-q"])?;
    for (f, s) in files.iter().zip(sel) {
        if !any_chosen(f, s) {
            continue;
        }
        if !f.partial_ok() || all_chosen(f, s) {
            git::run(repo, &["add", "-A", "--", &f.path])?;
            continue;
        }
        let mut patch = f.header.join("\n") + "\n";
        for (h, chosen) in f.hunks.iter().zip(s) {
            if let Some(text) = h.restricted(chosen) {
                patch.push_str(&text);
                patch.push('\n');
            }
        }
        git::run_with(repo, &["apply", "--cached", "--recount", "-"], &[], Some(&patch))
            .map_err(|e| format!("{} : {e}", f.path))?;
    }
    Ok(())
}

/// Stages the selection and commits it; returns (short hash, subject). On failure nothing stays staged.
pub fn commit_part(repo: &Path, files: &[FileChange], sel: &[Vec<Vec<bool>>], message: &str) -> Result<(String, String), String> {
    let r = stage(repo, files, sel).and_then(|()| commit(repo, message));
    if r.is_err() {
        let _ = git::run(repo, &["reset", "-q"]);
    }
    r
}

/// Commits what is staged; returns (short hash, subject).
pub fn commit(repo: &Path, message: &str) -> Result<(String, String), String> {
    git::run_with(repo, &["commit", "-q", "-F", "-"], &[], Some(message))?;
    let out = git::stdout(repo, &["log", "-1", "--format=%h%x1f%s"])?;
    let (h, s) = out.trim().split_once('\x1f').unwrap_or((out.trim(), ""));
    Ok((h.to_string(), s.to_string()))
}

pub fn finish(repo: &Path) -> Result<String, String> {
    git::run_with(repo, &["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
}

pub fn abort(repo: &Path) -> Result<String, String> {
    // the files created by the commit being split are untracked after the reset: `rebase --abort`
    // refuses to overwrite them, and puts them back itself
    if let Some(hash) = stopped_commit(repo) {
        let added = git::stdout(repo, &["-c", "core.quotePath=false", "diff-tree", "-r", "--no-commit-id", "--name-only", "--diff-filter=A", &hash]).unwrap_or_default();
        let untracked = git::stdout(repo, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
        for p in added.lines().filter(|p| !p.is_empty() && untracked.lines().any(|u| u == *p)) {
            let _ = std::fs::remove_file(repo.join(p));
        }
    }
    git::run(repo, &["rebase", "--abort"])
}

/// The changes not committed (staged or not, new files included), for a split by theme.
pub fn worktree_changes(repo: &Path) -> Result<Vec<FileChange>, String> {
    let untracked: Vec<String> = git::stdout(repo, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"])?
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    changes(repo, &untracked)
}

/// A part of the changes that goes into one commit: a hunk, or a whole file (new, deleted,
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

/// Commits `planned` (blocks of an earlier reading of the changes) with `message`. The changes
/// are read again: each block is found by its file and its lines (the commits before it moved the
/// line numbers). Returns (short hash, subject).
pub fn commit_blocks(repo: &Path, planned: &[Block], message: &str) -> Result<(String, String), String> {
    let files = worktree_changes(repo)?;
    let mut sel: Vec<Vec<Vec<bool>>> = files.iter().map(FileChange::empty_selection).collect();
    for b in planned {
        let gone = || tf!("{} : ce bloc n'est plus dans les fichiers (modifiés depuis ?)", "{}: this block is no longer in the files (changed since?)", b.path);
        let i = files.iter().position(|f| f.path == b.path).ok_or_else(gone)?;
        let f = &files[i];
        let hunks: Vec<usize> = if b.changed.is_empty() || !f.partial_ok() {
            (0..f.hunks.len()).collect()
        } else {
            let k = f.hunks.iter().position(|h| (1..h.lines.len()).filter(|&j| h.is_change(j)).map(|j| &h.lines[j]).eq(b.changed.iter())).ok_or_else(gone)?;
            vec![k]
        };
        for k in hunks {
            sel[i][k] = vec![true; f.hunks[k].lines.len()];
        }
    }
    commit_part(repo, &files, &sel, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(repo: &Path, args: &[&str]) -> String {
        git::run(repo, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"))
    }

    #[test]
    fn commits_blocks_of_one_file_apart() {
        let repo = std::env::temp_dir().join(format!("florm_plan_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        g(&repo, &["init", "-q", "-b", "main"]);
        g(&repo, &["config", "user.name", "t"]);
        g(&repo, &["config", "user.email", "t@t"]);
        let lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
        std::fs::write(repo.join("a.txt"), lines.join("\n") + "\n").unwrap();
        g(&repo, &["add", "."]);
        g(&repo, &["commit", "-qm", "base"]);
        let mut l2 = lines.clone();
        l2[2] = "top".into();
        l2.insert(30, "added below".into());
        std::fs::write(repo.join("a.txt"), l2.join("\n") + "\n").unwrap();
        std::fs::write(repo.join("new.txt"), "new\n").unwrap();
        let b = blocks(&worktree_changes(&repo).unwrap());
        assert_eq!(b.len(), 3);
        assert!(blocks_text(&b).contains("### B3 — new.txt (nouveau fichier)"));
        // the second hunk first: the first one has to be found again after it
        commit_blocks(&repo, &[b[1].clone(), b[2].clone()], "second").unwrap();
        commit_blocks(&repo, &[b[0].clone()], "first").unwrap();
        assert!(g(&repo, &["status", "--porcelain"]).trim().is_empty());
        assert_eq!(g(&repo, &["log", "--format=%s", "-2"]).trim(), "first\nsecond");
        assert!(commit_blocks(&repo, &[b[0].clone()], "again").is_err());
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn splits_a_commit_in_three() {
        let repo = std::env::temp_dir().join(format!("florm_split_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        g(&repo, &["init", "-q", "-b", "main"]);
        g(&repo, &["config", "user.name", "t"]);
        g(&repo, &["config", "user.email", "t@t"]);
        let lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
        std::fs::write(repo.join("a.txt"), lines.join("\n") + "\n").unwrap();
        g(&repo, &["add", "."]);
        g(&repo, &["commit", "-qm", "base"]);
        let mut l2 = lines.clone();
        l2[2] = "CHANGED top".into();
        l2[35] = "CHANGED bottom".into();
        std::fs::write(repo.join("a.txt"), l2.join("\n") + "\n").unwrap();
        std::fs::write(repo.join("b.txt"), "new file\n").unwrap();
        g(&repo, &["add", "."]);
        g(&repo, &["commit", "-qm", "big commit"]);
        let big = g(&repo, &["rev-parse", "HEAD"]);
        std::fs::write(repo.join("c.txt"), "after\n").unwrap();
        g(&repo, &["add", "."]);
        g(&repo, &["commit", "-qm", "after"]);
        let tree_before = g(&repo, &["rev-parse", "HEAD^{tree}"]);

        let info = start(&repo, &big, "sed -i -e '1s/^pick/edit/'").unwrap();
        assert_eq!(info.subject, "big commit");
        assert_eq!(info.added, ["b.txt"]);
        assert!(rebase_in_progress(&repo));
        assert_eq!(stopped_commit(&repo).as_deref(), Some(big.as_str()));

        let files = changes(&repo, &info.added).unwrap();
        assert_eq!(files.iter().map(|f| (f.path.as_str(), f.kind, f.hunks.len())).collect::<Vec<_>>(), [("a.txt", 'M', 2), ("b.txt", 'A', 1)]);
        let pick = |files: &[FileChange], chosen: &[&[bool]]| -> Vec<Vec<Vec<bool>>> {
            files
                .iter()
                .zip(chosen)
                .map(|(f, c)| f.hunks.iter().zip(c.iter()).map(|(h, &on)| vec![on; h.lines.len()]).collect())
                .collect()
        };
        // part 1: top hunk only
        stage(&repo, &files, &pick(&files, &[&[true, false], &[false]])).unwrap();
        commit(&repo, "part 1").unwrap();
        // part 2: the new file
        let files = changes(&repo, &info.added).unwrap();
        assert_eq!(files.len(), 2);
        stage(&repo, &files, &pick(&files, &[&[false], &[true]])).unwrap();
        commit(&repo, "part 2").unwrap();
        // part 3: the rest
        let files = changes(&repo, &info.added).unwrap();
        assert_eq!(files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["a.txt"]);
        stage(&repo, &files, &pick(&files, &[&[true]])).unwrap();
        commit(&repo, "part 3").unwrap();
        assert!(changes(&repo, &info.added).unwrap().is_empty());
        finish(&repo).unwrap();

        assert!(!rebase_in_progress(&repo));
        assert_eq!(g(&repo, &["log", "--format=%s"]), "after\npart 3\npart 2\npart 1\nbase");
        assert_eq!(g(&repo, &["rev-parse", "HEAD^{tree}"]), tree_before);
        assert!(g(&repo, &["show", "HEAD~3"]).contains("+CHANGED top"));
        assert!(!g(&repo, &["show", "HEAD~3"]).contains("CHANGED bottom"));
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn chosen_lines_of_a_hunk() {
        let h = Hunk { lines: ["@@ -1,3 +1,3 @@", " a", "-b", "-c", "+B", "+C", " d"].iter().map(|s| s.to_string()).collect() };
        // nothing chosen
        assert_eq!(h.restricted(&[false; 7]), None);
        // only "-b" and "+B": c stays, C is not added
        let mut on = vec![false; 7];
        on[2] = true;
        on[4] = true;
        assert_eq!(h.restricted(&on).unwrap(), "@@ -1,3 +1,3 @@\n a\n-b\n c\n+B\n d");
        // an added line without newline at the end of the file: its marker goes with it
        let h = Hunk { lines: ["@@ -1 +1,2 @@", " a", "+b", "\\ No newline at end of file"].iter().map(|s| s.to_string()).collect() };
        assert_eq!(h.restricted(&[false, false, false, false]), None);
        assert_eq!(h.restricted(&[false, false, true, false]).unwrap(), "@@ -1 +1,2 @@\n a\n+b\n\\ No newline at end of file");
    }

    #[test]
    fn splits_one_hunk_line_by_line() {
        let repo = std::env::temp_dir().join(format!("florm_split_lines_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        g(&repo, &["init", "-q", "-b", "main"]);
        g(&repo, &["config", "user.name", "t"]);
        g(&repo, &["config", "user.email", "t@t"]);
        std::fs::write(repo.join("a.txt"), "1\n2\n3\n").unwrap();
        g(&repo, &["add", "."]);
        g(&repo, &["commit", "-qm", "base"]);
        std::fs::write(repo.join("a.txt"), "1\ndeux\n3\nquatre\n").unwrap();
        g(&repo, &["commit", "-qam", "two changes"]);
        let head = g(&repo, &["rev-parse", "HEAD"]);
        let info = start(&repo, &head, "sed -i -e '1s/^pick/edit/'").unwrap();
        let files = changes(&repo, &info.added).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].hunks.len(), 1);
        // part 1: "2" → "deux" only (its - and + lines)
        let mut sel = vec![files[0].empty_selection()];
        for (i, l) in files[0].hunks[0].lines.iter().enumerate() {
            if l == "-2" || l == "+deux" {
                sel[0][0][i] = true;
            }
        }
        stage(&repo, &files, &sel).unwrap();
        commit(&repo, "deux").unwrap();
        assert_eq!(g(&repo, &["show", "HEAD:a.txt"]), "1\ndeux\n3");
        // part 2: the rest
        let files = changes(&repo, &info.added).unwrap();
        let sel: Vec<Vec<Vec<bool>>> = files.iter().map(|f| f.hunks.iter().map(|h| vec![true; h.lines.len()]).collect()).collect();
        stage(&repo, &files, &sel).unwrap();
        commit(&repo, "quatre").unwrap();
        finish(&repo).unwrap();
        assert_eq!(g(&repo, &["log", "--format=%s"]), "quatre\ndeux\nbase");
        assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "1\ndeux\n3\nquatre\n");
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn refuses_dirty_tree_and_aborts() {
        let repo = std::env::temp_dir().join(format!("florm_split_abort_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        g(&repo, &["init", "-q", "-b", "main"]);
        g(&repo, &["config", "user.name", "t"]);
        g(&repo, &["config", "user.email", "t@t"]);
        std::fs::write(repo.join("a.txt"), "1\n").unwrap();
        g(&repo, &["add", "."]);
        g(&repo, &["commit", "-qm", "one"]);
        std::fs::write(repo.join("a.txt"), "2\n").unwrap();
        g(&repo, &["commit", "-qam", "two"]);
        let head = g(&repo, &["rev-parse", "HEAD"]);
        // a todo list left unchanged replays the commit as is: nothing to split, nothing changed
        assert!(start(&repo, &head, "true").is_err());
        assert!(!rebase_in_progress(&repo));
        assert_eq!(g(&repo, &["rev-parse", "HEAD"]), head);
        std::fs::write(repo.join("a.txt"), "dirty\n").unwrap();
        assert!(start(&repo, &head, "sed -i -e '1s/^pick/edit/'").unwrap_err().contains("non validées"));
        g(&repo, &["checkout", "--", "a.txt"]);
        start(&repo, &head, "sed -i -e '1s/^pick/edit/'").unwrap();
        abort(&repo).unwrap();
        assert_eq!(g(&repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "2\n");
        let _ = std::fs::remove_dir_all(&repo);
    }
}
