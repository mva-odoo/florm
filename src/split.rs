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
    /// Whether hunks can be committed separately.
    pub fn partial_ok(&self) -> bool {
        self.kind == 'M' && !self.binary && !self.untracked && self.hunks.len() > 1
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
        return Err("Un rebase est déjà en cours dans ce dépôt : termine-le ou annule-le d'abord.".into());
    }
    if !git::stdout(repo, &["status", "--porcelain", "--untracked-files=no"])?.trim().is_empty() {
        return Err("Le dépôt a des modifications non validées : valide-les ou mets-les de côté (stash) avant de découper un commit.".into());
    }
    let parents = git::stdout(repo, &["rev-list", "--parents", "-n", "1", hash])?.split_whitespace().count() - 1;
    if parents == 0 {
        return Err("Le premier commit du dépôt ne peut pas être découpé.".into());
    }
    if parents > 1 {
        return Err("Un commit de merge ne peut pas être découpé.".into());
    }
    if git::run(repo, &["merge-base", "--is-ancestor", hash, "HEAD"]).is_err() {
        return Err("Le commit doit faire partie de la branche courante : fais d'abord un checkout de sa branche.".into());
    }
    if !git::stdout(repo, &["rev-list", "--merges", &format!("{hash}..HEAD")])?.trim().is_empty() {
        return Err("Il y a des merges après ce commit : le découpage les aplatirait, il est donc refusé.".into());
    }
    let info = info(repo, hash)?;
    let env = [("GIT_SEQUENCE_EDITOR", seq_editor), ("GIT_EDITOR", "true")];
    git::run_with(repo, &["rebase", "-i", "--no-autosquash", &format!("{hash}^")], &env, None)?;
    if !rebase_in_progress(repo) {
        return Err("Le rebase ne s'est pas arrêté sur le commit à découper.".into());
    }
    let head = git::stdout(repo, &["rev-parse", "HEAD"])?;
    if head.trim() != hash {
        let _ = git::run(repo, &["rebase", "--abort"]);
        return Err("Le rebase ne s'est pas arrêté sur le commit à découper : annulé.".into());
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
        let mut lines = vec![format!("@@ nouveau fichier : {} ligne(s) @@", text.lines().count())];
        if !binary {
            lines.extend(text.lines().take(400).map(|l| format!("+{l}")));
        }
        files.push(FileChange { path: path.to_string(), kind: 'A', header: Vec::new(), hunks: vec![Hunk { lines }], binary, untracked: true });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Stages the chosen hunks (`sel[i][h]`), whole files when everything or a non-splittable file is chosen.
/// The index is reset first, so hunks (computed against HEAD) apply to it as they are.
pub fn stage(repo: &Path, files: &[FileChange], sel: &[Vec<bool>]) -> Result<(), String> {
    git::run(repo, &["reset", "-q"])?;
    for (f, s) in files.iter().zip(sel) {
        if !s.iter().any(|&b| b) {
            continue;
        }
        if !f.partial_ok() || s.iter().all(|&b| b) {
            git::run(repo, &["add", "-A", "--", &f.path])?;
            continue;
        }
        let mut patch = f.header.join("\n") + "\n";
        for (h, _) in f.hunks.iter().zip(s).filter(|(_, &on)| on) {
            patch.push_str(&h.lines.join("\n"));
            patch.push('\n');
        }
        git::run_with(repo, &["apply", "--cached", "--recount", "-"], &[], Some(&patch))
            .map_err(|e| format!("{} : {e}", f.path))?;
    }
    Ok(())
}

/// Stages the selection and commits it; returns (short hash, subject). On failure nothing stays staged.
pub fn commit_part(repo: &Path, files: &[FileChange], sel: &[Vec<bool>], message: &str) -> Result<(String, String), String> {
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
    git::run(repo, &["rebase", "--abort"])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(repo: &Path, args: &[&str]) -> String {
        git::run(repo, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"))
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
        // part 1: top hunk only
        stage(&repo, &files, &[vec![true, false], vec![false]]).unwrap();
        commit(&repo, "part 1").unwrap();
        // part 2: the new file
        let files = changes(&repo, &info.added).unwrap();
        assert_eq!(files.len(), 2);
        stage(&repo, &files, &[vec![false], vec![true]]).unwrap();
        commit(&repo, "part 2").unwrap();
        // part 3: the rest
        let files = changes(&repo, &info.added).unwrap();
        assert_eq!(files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["a.txt"]);
        stage(&repo, &files, &[vec![true]]).unwrap();
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
