//! Git through the `git` CLI (no libgit2: lighter, and uses the user's own config/credentials).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// git in `dir`; for a WSL folder opened from Windows, trusted although owned by the Linux user.
fn git(dir: &Path) -> std::process::Command {
    let mut c = crate::os::command("git");
    // `git status` must not rewrite the index: that would look like a change made outside Florm
    // (see `stamp`), and could collide with a git command run by the user in a terminal
    c.env("GIT_OPTIONAL_LOCKS", "0");
    if cfg!(windows) && crate::os::is_wsl_unc(dir) {
        c.args(["-c", "safe.directory=*"]);
    }
    c
}

fn exec(dir: &Path, args: &[&str]) -> Result<(String, String), String> {
    let out = git(dir)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("git introuvable : {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if out.status.success() {
        Ok((stdout, stderr))
    } else {
        Err(format!("{stdout}{stderr}").trim().to_string())
    }
}

/// Runs git and returns stdout + stderr (git writes progress to stderr).
pub fn run(dir: &Path, args: &[&str]) -> Result<String, String> {
    exec(dir, args).map(|(o, e)| format!("{o}{e}").trim().to_string())
}

/// Like `run`, with extra environment variables and text written to stdin.
pub fn run_with(dir: &Path, args: &[&str], env: &[(&str, &str)], input: Option<&str>) -> Result<String, String> {
    let mut child = git(dir)
        .args(args)
        .current_dir(dir)
        .envs(env.iter().copied())
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git introuvable : {e}"))?;
    if let (Some(mut stdin), Some(text)) = (child.stdin.take(), input) {
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(text)
    }
}

pub fn stdout(dir: &Path, args: &[&str]) -> Result<String, String> {
    exec(dir, args).map(|(o, _)| o)
}

pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    stdout(dir, &["rev-parse", "--show-toplevel"]).ok().map(|s| PathBuf::from(s.trim()))
}

/// Git repositories inside `root` (up to `max_depth` levels), plus the repository
/// containing `root` when it is a subfolder of one.
/// Modification times of the files git rewrites on a commit, checkout, staging, fetch…: when it
/// changes, the repository was changed (possibly outside Florm, e.g. in a terminal).
pub fn stamp(repo: &Path) -> Vec<Option<std::time::SystemTime>> {
    let dot = repo.join(".git");
    let dir = if dot.is_file() {
        // worktree / submodule: ".git" is a file "gitdir: <path>"
        match std::fs::read_to_string(&dot).ok().and_then(|t| t.trim().strip_prefix("gitdir:").map(|p| p.trim().to_string())) {
            Some(p) => repo.join(p),
            None => return Vec::new(),
        }
    } else {
        dot
    };
    ["HEAD", "index", "logs/HEAD", "FETCH_HEAD", "ORIG_HEAD", "MERGE_HEAD", "rebase-merge"]
        .iter()
        .map(|f| std::fs::metadata(dir.join(f)).and_then(|m| m.modified()).ok())
        .collect()
}

pub fn find_repos(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if dir.join(".git").exists() {
            out.push(dir.to_path_buf());
        }
        if depth == 0 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut subdirs: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                !n.starts_with('.') && !matches!(n.as_ref(), "node_modules" | "__pycache__" | "target" | "venv" | "dist" | "build")
            })
            .map(|e| e.path())
            .collect();
        subdirs.sort();
        for d in subdirs {
            walk(&d, depth - 1, out);
        }
    }
    let mut out = Vec::new();
    if let Some(r) = repo_root(root) {
        out.push(crate::os::canon(&r));
    }
    walk(root, max_depth, &mut out);
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub branch: String,
    pub tracking: String,
    pub entries: Vec<Entry>,
    /// +/- lines of the staged and of the unstaged changes, per file.
    pub staged_stats: Stats,
    pub unstaged_stats: Stats,
    pub stashes: Vec<StashEntry>,
    /// Merge / cherry-pick / revert / rebase stopped in the repository.
    pub operation: Option<Operation>,
}

#[derive(Clone, Debug)]
pub struct StashEntry {
    /// "stash@{0}"
    pub name: String,
    pub subject: String,
    pub time: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Merge,
    CherryPick,
    Revert,
    Rebase,
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Operation::Merge => "Merge",
            Operation::CherryPick => "Cherry-pick",
            Operation::Revert => "Revert",
            Operation::Rebase => "Rebase",
        }
    }

    /// git arguments to continue (after the conflicts are resolved and staged) and to abort.
    pub fn continue_args(self) -> Vec<&'static str> {
        match self {
            Operation::Merge => vec!["commit", "--no-edit"],
            Operation::CherryPick => vec!["cherry-pick", "--continue"],
            Operation::Revert => vec!["revert", "--continue"],
            Operation::Rebase => vec!["rebase", "--continue"],
        }
    }

    pub fn abort_args(self) -> Vec<&'static str> {
        match self {
            Operation::Merge => vec!["merge", "--abort"],
            Operation::CherryPick => vec!["cherry-pick", "--abort"],
            Operation::Revert => vec!["revert", "--abort"],
            Operation::Rebase => vec!["rebase", "--abort"],
        }
    }
}

fn git_path_exists(root: &Path, name: &str) -> bool {
    stdout(root, &["rev-parse", "--git-path", name]).is_ok_and(|p| {
        let p = Path::new(p.trim());
        if p.is_absolute() { p.exists() } else { root.join(p).exists() }
    })
}

pub fn operation(root: &Path) -> Option<Operation> {
    if git_path_exists(root, "rebase-merge") || git_path_exists(root, "rebase-apply") {
        Some(Operation::Rebase)
    } else if git_path_exists(root, "CHERRY_PICK_HEAD") {
        Some(Operation::CherryPick)
    } else if git_path_exists(root, "REVERT_HEAD") {
        Some(Operation::Revert)
    } else if git_path_exists(root, "MERGE_HEAD") {
        Some(Operation::Merge)
    } else {
        None
    }
}

pub fn stashes(root: &Path) -> Vec<StashEntry> {
    stdout(root, &["stash", "list", "--format=%gd%x1f%s%x1f%ct"])
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split('\x1f').collect();
                    (f.len() >= 3).then(|| StashEntry { name: f[0].into(), subject: f[1].into(), time: f[2].parse().unwrap_or(0) })
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Status {
    /// Upstream branch and how far ahead / behind it is: ("origin/main", 2, 0).
    pub fn upstream(&self) -> Option<(String, u32, u32)> {
        if self.tracking.is_empty() {
            return None;
        }
        let (name, rest) = self.tracking.split_once(' ').unwrap_or((&self.tracking, ""));
        let num = |key: &str| {
            rest.split(key).nth(1).and_then(|r| r.trim().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()).unwrap_or(0)
        };
        Some((name.to_string(), num("ahead"), num("behind")))
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: String,
    pub x: char,
    pub y: char,
}

impl Entry {
    pub fn staged(&self) -> bool {
        self.x != ' ' && self.x != '?'
    }
    pub fn unstaged(&self) -> bool {
        self.y != ' '
    }
    /// Unmerged path (merge conflict): UU, AA, DD, AU, UA, DU, UD.
    pub fn conflicted(&self) -> bool {
        self.x == 'U' || self.y == 'U' || (self.x == 'A' && self.y == 'A') || (self.x == 'D' && self.y == 'D')
    }
    pub fn label(&self) -> char {
        match (self.x, self.y) {
            ('?', _) => 'U',
            (x, ' ') => x,
            (_, y) => y,
        }
    }
}

pub fn status(root: &Path) -> Result<Status, String> {
    let out = stdout(root, &["status", "--porcelain=v1", "-b", "-z", "--untracked-files=all"])?;
    let mut st = Status::default();
    let mut it = out.split('\0');
    while let Some(item) = it.next() {
        if let Some(b) = item.strip_prefix("## ") {
            let b = b.strip_prefix("No commits yet on ").unwrap_or(b);
            match b.split_once("...") {
                Some((br, rest)) => {
                    st.branch = br.to_string();
                    st.tracking = rest.to_string();
                }
                None => st.branch = b.to_string(),
            }
            continue;
        }
        if item.len() < 4 {
            continue;
        }
        let mut ch = item.chars();
        let x = ch.next().unwrap_or(' ');
        let y = ch.next().unwrap_or(' ');
        if x == 'R' || x == 'C' {
            it.next(); // original path of a rename
        }
        st.entries.push(Entry { path: item[3..].to_string(), x, y });
    }
    st.staged_stats = numstat(root, &["-c", "core.quotePath=false", "diff", "--cached", "--numstat", "--no-renames"]);
    st.unstaged_stats = numstat(root, &["-c", "core.quotePath=false", "diff", "--numstat", "--no-renames"]);
    st.stashes = stashes(root);
    st.operation = operation(root);
    Ok(st)
}

pub fn branches(root: &Path) -> Vec<String> {
    stdout(root, &["branch", "-a", "--format=%(refname:short)"])
        .map(|s| s.lines().map(str::trim).filter(|l| !l.is_empty() && !l.ends_with("/HEAD")).map(String::from).collect())
        .unwrap_or_default()
}

/// One commit of the short history shown in the Git panel.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub hash: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    pub time: i64,
    /// Branches and tags pointing to it ("HEAD -> main, origin/main").
    pub refs: String,
}

pub fn log(root: &Path) -> Vec<LogEntry> {
    stdout(root, &["log", "-n", "40", "--format=%H%x1f%h%x1f%s%x1f%an%x1f%at%x1f%D"])
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split('\x1f').collect();
                    (f.len() >= 6).then(|| LogEntry {
                        hash: f[0].into(),
                        short: f[1].into(),
                        subject: f[2].into(),
                        author: f[3].into(),
                        time: f[4].parse().unwrap_or(0),
                        refs: f[5].into(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Lines added / removed per file from `git … --numstat` (None for binary files).
pub type Stats = std::collections::HashMap<String, (Option<u32>, Option<u32>)>;

pub fn numstat(root: &Path, args: &[&str]) -> Stats {
    stdout(root, args)
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let mut f = l.splitn(3, '\t');
                    let (a, d, p) = (f.next()?, f.next()?, f.next()?);
                    Some((p.to_string(), (a.parse().ok(), d.parse().ok())))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Content of the file in HEAD (None when untracked or outside a repository).
pub fn head_text(file: &Path) -> Option<String> {
    let dir = file.parent()?;
    let name = file.file_name()?.to_str()?;
    stdout(dir, &["show", &format!("HEAD:./{name}")]).ok().map(|s| s.replace("\r\n", "\n"))
}

#[derive(Clone, Debug)]
pub struct Blame {
    pub hash: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
}

impl Blame {
    pub fn uncommitted(&self) -> bool {
        self.hash.chars().all(|c| c == '0')
    }
}

/// Blame of one line (1-based) of the buffer content (unsaved changes included).
pub fn blame_line(file: &Path, line: usize, contents: &str) -> Result<Blame, String> {
    let dir = file.parent().ok_or("chemin invalide")?;
    let name = file.file_name().and_then(|n| n.to_str()).ok_or("chemin invalide")?;
    let range = format!("{line},{line}");
    let mut child = git(dir)
        .args(["blame", "--porcelain", "-L", &range, "--contents", "-", "--", name])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let contents = contents.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(contents.as_bytes());
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let hash = lines.next().and_then(|l| l.split_whitespace().next()).unwrap_or("").to_string();
    let (mut author, mut time, mut summary) = (String::new(), 0, String::new());
    for l in lines {
        if let Some(v) = l.strip_prefix("author ") {
            author = v.to_string();
        } else if let Some(v) = l.strip_prefix("author-time ") {
            time = v.parse().unwrap_or(0);
        } else if let Some(v) = l.strip_prefix("summary ") {
            summary = v.to_string();
        }
    }
    Ok(Blame { hash, author, time, summary })
}

#[derive(Clone, Debug)]
pub struct LineCommit {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
    pub diff: String,
}

/// Every commit that touched line `line` (1-based, in HEAD), newest first, with its diff.
pub fn line_history(file: &Path, line: usize) -> Result<Vec<LineCommit>, String> {
    let dir = file.parent().ok_or("chemin invalide")?;
    let name = file.file_name().and_then(|n| n.to_str()).ok_or("chemin invalide")?;
    let range = format!("{line},{line}:{name}");
    let out = stdout(
        dir,
        &["log", "--date=format:%Y-%m-%d %H:%M", "--format=%x1e%H%x1f%an%x1f%ad%x1f%s", "-L", &range],
    )?;
    Ok(out
        .split('\x1e')
        .filter(|c| !c.trim().is_empty())
        .map(|chunk| {
            let (meta, diff) = chunk.split_once('\n').unwrap_or((chunk, ""));
            let f: Vec<&str> = meta.split('\x1f').collect();
            let get = |i: usize| f.get(i).copied().unwrap_or("").to_string();
            LineCommit { hash: get(0), author: get(1), date: get(2), subject: get(3), diff: diff.trim().to_string() }
        })
        .collect())
}

/// "il y a 3 jours" from a unix timestamp.
pub fn ago(t: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(t);
    let d = (now - t).max(0);
    let plural = |n: i64, unit: &str| format!("il y a {n} {unit}{}", if n > 1 && !unit.ends_with('s') { "s" } else { "" });
    match d {
        0..=59 => "à l'instant".into(),
        60..=3599 => format!("il y a {} min", d / 60),
        3600..=86_399 => plural(d / 3600, "heure"),
        86_400..=2_591_999 => plural(d / 86_400, "jour"),
        2_592_000..=31_535_999 => format!("il y a {} mois", d / 2_592_000),
        _ => plural(d / 31_536_000, "an"),
    }
}

pub fn parse_github(url: &str) -> Option<(String, String)> {
    let rest = ["git@github.com:", "https://github.com/", "http://github.com/", "ssh://git@github.com/"]
        .iter()
        .find_map(|p| url.strip_prefix(p))?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let (o, r) = rest.split_once('/')?;
    Some((o.to_string(), r.to_string()))
}


#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn upstream_ahead_behind() {
        let st = |t: &str| Status { tracking: t.into(), ..Default::default() };
        assert_eq!(st("origin/main").upstream(), Some(("origin/main".into(), 0, 0)));
        assert_eq!(st("origin/main [ahead 2]").upstream(), Some(("origin/main".into(), 2, 0)));
        assert_eq!(st("origin/dev [ahead 1, behind 12]").upstream(), Some(("origin/dev".into(), 1, 12)));
        assert_eq!(st("origin/x [behind 3]").upstream(), Some(("origin/x".into(), 0, 3)));
        assert_eq!(st("").upstream(), None);
    }

    #[test]
    fn stamp_sees_commits_and_staging_made_elsewhere() {
        let repo = std::env::temp_dir().join(format!("florm_stamp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let g = |args: &[&str]| run(&repo, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        g(&["init", "-q", "-b", "main"]);
        g(&["config", "user.name", "t"]);
        g(&["config", "user.email", "t@t"]);
        std::fs::write(repo.join("a"), "1").unwrap();
        g(&["add", "."]);
        g(&["commit", "-qm", "one"]);
        let s1 = stamp(&repo);
        assert!(s1.iter().any(|t| t.is_some()));
        // a status (what Florm runs itself) does not count as a change
        status(&repo).unwrap();
        assert_eq!(stamp(&repo), s1);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(repo.join("a"), "2").unwrap();
        g(&["commit", "-qam", "two"]);
        assert_ne!(stamp(&repo), s1);
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn stash_and_operations() {
        let repo = std::env::temp_dir().join(format!("florm_ops_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let g = |args: &[&str]| run(&repo, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        g(&["init", "-q", "-b", "main"]);
        g(&["config", "user.name", "t"]);
        g(&["config", "user.email", "t@t"]);
        std::fs::write(repo.join("f"), "1\n").unwrap();
        g(&["add", "."]);
        g(&["commit", "-qm", "base"]);
        std::fs::write(repo.join("f"), "2\n").unwrap();
        g(&["stash", "push", "-m", "essai"]);
        let st = status(&repo).unwrap();
        assert_eq!(st.stashes.len(), 1);
        assert_eq!(st.stashes[0].name, "stash@{0}");
        assert!(st.stashes[0].subject.contains("essai"));
        assert_eq!(st.operation, None);
        // a conflicting cherry-pick leaves an operation in progress
        g(&["checkout", "-qb", "other"]);
        std::fs::write(repo.join("f"), "other\n").unwrap();
        g(&["commit", "-qam", "other"]);
        let pick = g(&["rev-parse", "HEAD"]);
        g(&["checkout", "-q", "main"]);
        std::fs::write(repo.join("f"), "main\n").unwrap();
        g(&["commit", "-qam", "main"]);
        assert!(run(&repo, &["cherry-pick", pick.trim()]).is_err());
        let st = status(&repo).unwrap();
        assert_eq!(st.operation, Some(Operation::CherryPick));
        assert!(st.entries.iter().any(|e| e.conflicted()));
        run(&repo, &Operation::CherryPick.abort_args()).unwrap();
        assert_eq!(status(&repo).unwrap().operation, None);
        let _ = std::fs::remove_dir_all(&repo);
    }
}
