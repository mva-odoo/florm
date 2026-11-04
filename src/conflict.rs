//! The merge, rebase or cherry-pick stopped on conflicts: what is going on (step, commit being
//! applied, the two sides by their real names), the files in conflict and their blocks, for the
//! conflicts assistant.

use crate::document::{find_conflicts, Conflict, Resolution};
use crate::git::{self, Operation};
use std::path::Path;

/// The operation stopped, in words.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpInfo {
    /// Step of a rebase: (number of the commit being applied, number of commits).
    pub step: Option<(usize, usize)>,
    /// The commit being applied (rebase, cherry-pick, revert) or merged: short hash and subject.
    pub commit: Option<(String, String)>,
    /// The branch being rebased (or merged into).
    pub branch: String,
    /// What it goes onto: the base of the rebase, the branch merged…
    pub onto: String,
}

fn git_file(root: &Path, name: &str) -> Option<String> {
    let p = git::stdout(root, &["rev-parse", "--git-path", name]).ok()?;
    let p = Path::new(p.trim());
    let p = if p.is_absolute() { p.to_path_buf() } else { root.join(p) };
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// A commit by a name of a branch when there is one ("master", "origin/17.0~2"), else its hash.
fn name_of(root: &Path, sha: &str) -> String {
    let name = git::stdout(root, &["name-rev", "--name-only", "--no-undefined", "--exclude=refs/tags/*", sha]).unwrap_or_default();
    let name = name.trim().trim_start_matches("remotes/");
    if name.is_empty() {
        sha.chars().take(8).collect()
    } else {
        name.to_string()
    }
}

fn short_and_subject(root: &Path, rev: &str) -> Option<(String, String)> {
    let out = git::stdout(root, &["log", "-1", "--format=%h%x1f%s", rev]).ok()?;
    let (h, s) = out.trim().split_once('\x1f')?;
    Some((h.to_string(), s.to_string()))
}

pub fn info(root: &Path, op: Operation) -> OpInfo {
    let branch = git::stdout(root, &["branch", "--show-current"]).unwrap_or_default().trim().to_string();
    match op {
        Operation::Rebase => {
            let (dir, num, end) = if git_file(root, "rebase-merge/end").is_some() { ("rebase-merge", "msgnum", "end") } else { ("rebase-apply", "next", "last") };
            let n = |f: &str| git_file(root, &format!("{dir}/{f}")).and_then(|s| s.parse().ok());
            let head = git_file(root, &format!("{dir}/head-name")).unwrap_or_default();
            OpInfo {
                step: n(num).zip(n(end)),
                commit: short_and_subject(root, "REBASE_HEAD"),
                branch: head.strip_prefix("refs/heads/").unwrap_or(&head).to_string(),
                onto: git_file(root, &format!("{dir}/onto")).map(|s| name_of(root, &s)).unwrap_or_default(),
            }
        }
        Operation::Merge => {
            let head = git_file(root, "MERGE_HEAD").unwrap_or_default();
            let first = head.lines().next().unwrap_or("").to_string();
            OpInfo { step: None, commit: short_and_subject(root, &first), branch, onto: name_of(root, &first) }
        }
        Operation::CherryPick | Operation::Revert => {
            let head = if op == Operation::CherryPick { "CHERRY_PICK_HEAD" } else { "REVERT_HEAD" };
            OpInfo { step: None, commit: short_and_subject(root, head), onto: branch.clone(), branch }
        }
    }
}

/// The names of the two sides of a block: (between `<<<<<<<` and `=======`, after it). In a
/// rebase, the first one is the base (with the commits already applied), not your branch.
pub fn sides(op: Operation, i: &OpInfo) -> (String, String) {
    let commit = i.commit.as_ref().map(|(h, s)| format!("{h} « {s} »")).unwrap_or_default();
    match op {
        Operation::Rebase => (
            tf!("{} (avec les commits déjà rejoués)", "{} (with the commits already applied)", if i.onto.is_empty() { "base" } else { &i.onto }),
            tf!("ton commit {commit}", "your commit {commit}"),
        ),
        Operation::Merge => (tf!("{} (ta branche)", "{} (your branch)", i.branch), i.onto.clone()),
        Operation::CherryPick => (tf!("{} (ta branche)", "{} (your branch)", i.branch), tf!("le commit {commit}", "the commit {commit}")),
        Operation::Revert => (tf!("{} (ta branche)", "{} (your branch)", i.branch), tf!("l'annulation de {commit}", "the undoing of {commit}")),
    }
}

/// What a file in conflict is about (from `git status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Changed on both sides: blocks to choose from (UU, AA).
    Both,
    /// Removed on the first side, changed on the second (DU), or the other way (UD).
    DeletedFirst,
    DeletedSecond,
    /// Added on one side only (AU, UA), removed on both (DD).
    Other,
}

pub fn kind(x: char, y: char) -> Kind {
    match (x, y) {
        ('U', 'U') | ('A', 'A') => Kind::Both,
        ('D', 'U') => Kind::DeletedFirst,
        ('U', 'D') => Kind::DeletedSecond,
        _ => Kind::Other,
    }
}

/// A block of a file in conflict, its sides as text.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub at: Conflict,
    pub first: String,
    /// The common ancestor (with `merge.conflictStyle=diff3`).
    pub base: Option<String>,
    pub second: String,
}

pub fn blocks(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let part = |a: usize, b: usize| lines.get(a..b).map(|l| l.concat()).unwrap_or_default();
    find_conflicts(lines.iter().map(|l| std::borrow::Cow::Borrowed(*l)))
        .into_iter()
        .map(|c| Block {
            first: part(c.start + 1, c.base.unwrap_or(c.mid)),
            base: c.base.map(|b| part(b + 1, c.mid)),
            second: part(c.mid + 1, c.end),
            at: c,
        })
        .collect()
}

/// `text` with block `c` replaced by the side(s) chosen; `Both` puts the first one then the
/// second.
pub fn resolve(text: &str, c: &Conflict, how: Resolution) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let part = |a: usize, b: usize| lines.get(a..b).map(|l| l.concat()).unwrap_or_default();
    let (first, second) = (part(c.start + 1, c.base.unwrap_or(c.mid)), part(c.mid + 1, c.end));
    let new = match how {
        Resolution::Ours => first,
        Resolution::Theirs => second,
        Resolution::Both => first + &second,
    };
    part(0, c.start) + &new + &part(c.end + 1, lines.len())
}

/// Arguments of git that put the conflict markers with the common ancestor between the sides.
pub const STYLE: [&str; 2] = ["-c", "merge.conflictStyle=diff3"];

/// The git command (its first arguments) starts an operation that can stop on conflicts.
pub fn may_conflict(args: &[&str]) -> bool {
    args.iter().find(|a| !a.starts_with('-') && !a.contains('=')).is_some_and(|c| matches!(*c, "merge" | "rebase" | "cherry-pick" | "revert" | "pull" | "stash"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "a\n<<<<<<< HEAD\nmine\n||||||| base\nold\n=======\ntheirs\n>>>>>>> abc (fix)\nz\n";

    #[test]
    fn blocks_and_their_sides() {
        let b = blocks(TEXT);
        assert_eq!(b.len(), 1);
        assert_eq!((b[0].first.as_str(), b[0].base.as_deref(), b[0].second.as_str()), ("mine\n", Some("old\n"), "theirs\n"));
        assert_eq!(resolve(TEXT, &b[0].at, Resolution::Ours), "a\nmine\nz\n");
        assert_eq!(resolve(TEXT, &b[0].at, Resolution::Theirs), "a\ntheirs\nz\n");
        assert_eq!(resolve(TEXT, &b[0].at, Resolution::Both), "a\nmine\ntheirs\nz\n");
        // without a newline at the end
        let t = "<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> b";
        assert_eq!(resolve(t, &blocks(t)[0].at, Resolution::Theirs), "y\n");
    }

    #[test]
    fn kinds_and_commands() {
        assert_eq!(kind('U', 'U'), Kind::Both);
        assert_eq!(kind('D', 'U'), Kind::DeletedFirst);
        assert_eq!(kind('U', 'D'), Kind::DeletedSecond);
        assert!(may_conflict(&["-c", "core.editor=true", "rebase", "master"]));
        assert!(may_conflict(&["pull", "--rebase"]));
        assert!(!may_conflict(&["push"]));
    }

    /// A real rebase stopped on a conflict: its step, its commit, its sides.
    #[test]
    fn a_rebase_stopped() {
        let d = std::env::temp_dir().join(format!("florm-conflict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let g = |args: &[&str]| git::run(&d, args);
        g(&["init", "-q", "-b", "master"]).unwrap();
        for (k, v) in [("user.name", "t"), ("user.email", "t@t"), ("commit.gpgsign", "false")] {
            g(&["config", k, v]).unwrap();
        }
        std::fs::write(d.join("f"), "one\n").unwrap();
        g(&["add", "f"]).unwrap();
        g(&["commit", "-qm", "base"]).unwrap();
        g(&["checkout", "-qb", "feature"]).unwrap();
        std::fs::write(d.join("f"), "mine\n").unwrap();
        g(&["commit", "-qam", "my change"]).unwrap();
        g(&["checkout", "-q", "master"]).unwrap();
        std::fs::write(d.join("f"), "theirs\n").unwrap();
        g(&["commit", "-qam", "their change"]).unwrap();
        g(&["checkout", "-q", "feature"]).unwrap();
        assert!(g(&[STYLE[0], STYLE[1], "rebase", "master"]).is_err());
        assert_eq!(git::operation(&d), Some(Operation::Rebase));
        let i = info(&d, Operation::Rebase);
        assert_eq!(i.step, Some((1, 1)));
        assert_eq!(i.commit.as_ref().map(|c| c.1.as_str()), Some("my change"));
        assert_eq!((i.branch.as_str(), i.onto.as_str()), ("feature", "master"));
        let (first, second) = sides(Operation::Rebase, &i);
        assert!(first.starts_with("master") && second.contains("my change"), "{first} / {second}");
        // in a rebase, the first side is master, the second one your commit
        let b = blocks(&std::fs::read_to_string(d.join("f")).unwrap());
        assert_eq!((b[0].first.as_str(), b[0].base.as_deref(), b[0].second.as_str()), ("theirs\n", Some("one\n"), "mine\n"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
