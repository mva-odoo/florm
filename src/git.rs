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
        .map_err(|e| tf!("git introuvable : {e}", "git not found: {e}"))?;
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
        .map_err(|e| tf!("git introuvable : {e}", "git not found: {e}"))?;
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
            // follows symbolic links too (repositories linked into the project)
            .filter(|e| e.path().is_dir())
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

/// Versions of a project side by side ("multiverse"): `odoo/master` holds the repository and
/// `odoo/17.0`, `odoo/18.0`… are its worktrees, one folder per branch.
#[derive(Clone, Debug, PartialEq)]
pub struct Versions {
    /// The folder of the versions (`odoo`).
    pub dir: PathBuf,
    /// The folder with the repository itself (`odoo/master`): new worktrees are added from it.
    pub main: PathBuf,
    /// Folder names of the versions, sorted.
    pub names: Vec<String>,
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
    v.sort();
    v
}

fn folder_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `dir` holds versions side by side: a single repository in a subfolder, and worktrees of it next
/// to it (or none yet, when the repository's folder is named after its branch: `odoo/master`).
pub fn versions(dir: &Path) -> Option<Versions> {
    let subs = subdirs(dir);
    let mut mains = subs.iter().filter(|s| s.join(".git").is_dir());
    let main = mains.next()?.clone();
    if mains.next().is_some() {
        // several repositories (`internal/`, `upgrade/`…): not versions of one project
        return None;
    }
    let worktrees = crate::os::canon(&main.join(".git").join("worktrees"));
    let mut names = vec![folder_name(&main)];
    for s in &subs {
        let gitdir = std::fs::read_to_string(s.join(".git")).ok();
        let Some(p) = gitdir.as_deref().and_then(|t| t.trim().strip_prefix("gitdir:")) else { continue };
        if crate::os::canon(&s.join(p.trim())).starts_with(&worktrees) {
            names.push(folder_name(s));
        }
    }
    let head = std::fs::read_to_string(main.join(".git/HEAD")).unwrap_or_default();
    let on_own_branch = head.trim().strip_prefix("ref: refs/heads/") == Some(names[0].as_str());
    if names.len() == 1 && !on_own_branch {
        return None;
    }
    names.sort();
    Some(Versions { dir: dir.to_path_buf(), main, names })
}

/// The versions around `dir`: when it holds versions (`odoo`) or is one of them (`odoo/17.0`), those
/// and the ones next to them (`enterprise`, `design-themes`); else the ones inside it.
pub fn versions_near(dir: &Path) -> Vec<Versions> {
    let all_in = |d: &Path| subdirs(d).iter().filter_map(|s| versions(s)).collect::<Vec<_>>();
    let parent = dir.parent();
    if versions(dir).is_some() {
        return parent.map(all_in).unwrap_or_default();
    }
    if let Some(p) = parent.filter(|p| versions(p).is_some()) {
        return p.parent().map(all_in).unwrap_or_default();
    }
    all_in(dir)
}

/// Branches a new version can come from: the local ones and the remote ones without their remote
/// (`odoo/saas-18.3` → `saas-18.3`), each once, sorted.
pub fn version_branches(repo: &Path) -> Result<Vec<String>, String> {
    let out = stdout(repo, &["for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"])?;
    let mut v: Vec<String> = out
        .lines()
        .filter_map(|r| r.strip_prefix("refs/heads/").or_else(|| r.strip_prefix("refs/remotes/").and_then(|r| r.split_once('/')).map(|(_, b)| b)))
        .filter(|b| *b != "HEAD")
        .map(str::to_string)
        .collect();
    v.sort();
    v.dedup();
    Ok(v)
}

/// Folder of the version of `branch` (a "/" in its name would make a subfolder).
pub fn version_folder(branch: &str) -> String {
    branch.trim().replace(['/', '\\'], "-")
}

/// A remote with `branch`: the one named like the folder of the versions (`odoo`), else `origin`,
/// else the first.
fn remote_with(repo: &Path, branch: &str, prefer: &str) -> Option<String> {
    let remotes = stdout(repo, &["remote"]).ok()?;
    let mut has: Vec<&str> = remotes
        .lines()
        .map(str::trim)
        .filter(|r| !r.is_empty() && exec(repo, &["show-ref", "--verify", "--quiet", &format!("refs/remotes/{r}/{branch}")]).is_ok())
        .collect();
    has.sort_by_key(|r| (*r != prefer, *r != "origin"));
    has.first().map(|r| r.to_string())
}

/// The version `branch` added to `v`: a worktree `<v.dir>/<branch>` on the local branch, else on a
/// new one tracking the remote branch (fetched first when no remote knows it yet). What git said.
pub fn add_version(v: &Versions, branch: &str) -> Result<String, String> {
    let branch = branch.trim();
    let dest = v.dir.join(version_folder(branch));
    if dest.exists() {
        return Err(tf!("{} existe déjà", "{} already exists", dest.display()));
    }
    let dest = dest.to_string_lossy().into_owned();
    if exec(&v.main, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok() {
        return run(&v.main, &["worktree", "add", &dest, branch]);
    }
    let prefer = folder_name(&v.dir);
    let mut fetched = String::new();
    let mut remote = remote_with(&v.main, branch, &prefer);
    if remote.is_none() {
        // a remote that cannot be reached does not stop the others
        fetched = run(&v.main, &["fetch", "--all"]).unwrap_or_else(|e| e);
        remote = remote_with(&v.main, branch, &prefer);
    }
    let Some(remote) = remote else {
        return Err(format!("{fetched}\n{}", tf!("Branche {branch} introuvable, ni en local ni sur les remotes", "Branch {branch} not found, neither locally nor on the remotes")).trim().to_string());
    };
    let out = run(&v.main, &["worktree", "add", "--track", "-b", branch, &dest, &format!("{remote}/{branch}")])?;
    Ok(format!("{fetched}\n{out}").trim().to_string())
}

/// The state of the folder of a version: its branch and what it tracks, the commits to push /
/// to pull (since the last fetch), the files changed (not counting the untracked ones).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VersionState {
    pub branch: String,
    pub upstream: String,
    pub ahead: usize,
    pub behind: usize,
    pub changes: usize,
}

/// `git status` of a version's folder, read as a `VersionState`.
pub fn version_state(dir: &Path) -> Result<VersionState, String> {
    Ok(parse_version_state(&stdout(dir, &["status", "--porcelain=v1", "-b", "--untracked-files=no"])?))
}

fn parse_version_state(out: &str) -> VersionState {
    let mut s = VersionState::default();
    let mut lines = out.lines();
    if let Some(head) = lines.next().and_then(|l| l.strip_prefix("## ")) {
        let (refs, counts) = head.split_once(" [").map_or((head, ""), |(r, c)| (r, c.trim_end_matches(']')));
        let (branch, upstream) = refs.split_once("...").unwrap_or((refs, ""));
        s.branch = branch.trim_start_matches("No commits yet on ").to_string();
        s.upstream = upstream.to_string();
        for part in counts.split(", ") {
            if let Some(n) = part.strip_prefix("ahead ") {
                s.ahead = n.parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                s.behind = n.parse().unwrap_or(0);
            }
        }
    }
    s.changes = lines.filter(|l| !l.trim().is_empty()).count();
    s
}

/// A version brought up to date: `git pull --ff-only` (never a merge commit).
pub fn pull_version(dir: &Path) -> Result<String, String> {
    run(dir, &["pull", "--ff-only"])
}

/// The version `name` taken out of `v`: its folder removed (`git worktree remove`; `force`: even
/// with changed files). The folder of the repository itself is never removed; the branch stays.
pub fn remove_version(v: &Versions, name: &str, force: bool) -> Result<String, String> {
    let dir = v.dir.join(name);
    if dir == v.main {
        return Err(t!("C'est le dépôt lui-même : il ne se supprime pas d'ici", "It is the repository itself: it cannot be removed from here").into());
    }
    let dir = dir.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&dir);
    run(&v.main, &args)
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub branch: String,
    pub tracking: String,
    pub entries: Vec<Entry>,
    /// +/- lines of the staged and of the unstaged changes, per file.
    pub staged_stats: Stats,
    pub unstaged_stats: Stats,
    /// Merge / cherry-pick / revert / rebase stopped in the repository.
    pub operation: Option<Operation>,
    /// `git bisect` in progress.
    pub bisecting: bool,
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
    st.operation = operation(root);
    st.bisecting = git_path_exists(root, "BISECT_LOG");
    Ok(st)
}

/// What a branch chosen in the list is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BranchPick {
    Checkout,
    /// Merged into the current branch.
    Merge,
    /// The current branch rebased on it.
    Rebase,
    Rename,
    Delete,
}

/// A branch of the repository.
#[derive(Clone, Debug, PartialEq)]
pub struct Branch {
    /// "feature", or "origin/feature" for a remote one.
    pub name: String,
    pub remote: bool,
    /// Upstream of a local branch ("origin/feature"), empty when none.
    pub upstream: String,
}

/// Local branches, then the remote ones.
pub fn branch_list(root: &Path) -> Vec<Branch> {
    stdout(root, &["for-each-ref", "--format=%(refname)%1f%(upstream:short)", "refs/heads", "refs/remotes"])
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let (full, up) = l.split_once('\x1f').unwrap_or((l, ""));
                    let (name, remote) = match full.strip_prefix("refs/heads/") {
                        Some(n) => (n, false),
                        None => (full.strip_prefix("refs/remotes/")?, true),
                    };
                    (!name.ends_with("/HEAD")).then(|| Branch { name: name.into(), remote, upstream: up.trim().into() })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// "origin/feature" -> ("origin", "feature"), with the remote names of the repository (a remote
/// name may contain a slash).
pub fn split_remote(remotes: &[String], full: &str) -> Option<(String, String)> {
    remotes
        .iter()
        .filter(|r| full.len() > r.len() + 1 && full.starts_with(r.as_str()) && full.as_bytes()[r.len()] == b'/')
        .max_by_key(|r| r.len())
        .map(|r| (r.clone(), full[r.len() + 1..].to_string()))
}

/// Remotes from the output of `git remote -v`: (name, fetch URL).
pub fn parse_remotes(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for l in text.lines() {
        let mut f = l.split_whitespace();
        if let (Some(n), Some(u)) = (f.next(), f.next()) {
            if !out.iter().any(|(m, _)| m == n) {
                out.push((n.to_string(), u.to_string()));
            }
        }
    }
    out
}

/// Submodules declared in `.gitmodules` (their paths), read from the file: no git process.
pub fn submodules(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join(".gitmodules"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == "path").then(|| v.trim().to_string())
        })
        .collect()
}

/// Where HEAD has been, newest first: commits, checkouts, resets, rebases… (the safety net:
/// a commit lost by a reset is still there).
pub fn reflog(root: &Path, limit: usize) -> Result<Vec<LineCommit>, String> {
    let n = format!("-n{limit}");
    let out = stdout(root, &["reflog", &n, "--date=format:%Y-%m-%d %H:%M", "--format=%H%x1f%gd%x1f%gs%x1f%cd%x1f%an"])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\x1f').collect();
            (f.len() >= 5).then(|| LineCommit {
                hash: f[0].into(),
                author: format!("{} · {}", f[1], f[4]),
                date: f[3].into(),
                subject: f[2].into(),
                diff: String::new(),
            })
        })
        .collect())
}

/// What to look for in the history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchKind {
    /// In the commit messages.
    Message,
    Author,
    /// Commits that added or removed this text (`-S`).
    Content,
    /// Commits whose changed lines match this regex (`-G`).
    Regex,
    /// Commits of a file, through its renames (`--follow`).
    File,
}

impl SearchKind {
    pub const ALL: [SearchKind; 5] = [SearchKind::Message, SearchKind::Author, SearchKind::Content, SearchKind::Regex, SearchKind::File];

    pub fn label(self) -> &'static str {
        match self {
            SearchKind::Message => t!("Message du commit", "Commit message"),
            SearchKind::Author => t!("Auteur", "Author"),
            SearchKind::Content => t!("Texte ajouté ou supprimé (-S)", "Text added or removed (-S)"),
            SearchKind::Regex => t!("Regex dans les lignes changées (-G)", "Regex in the changed lines (-G)"),
            SearchKind::File => t!("Fichier (avec ses renommages, --follow)", "File (with its renames, --follow)"),
        }
    }

    /// The git log arguments for `query`: with the diffs (only of what matched) when they are small.
    fn args(self, query: &str) -> Vec<String> {
        match self {
            SearchKind::Message => vec!["-i".into(), format!("--grep={query}")],
            SearchKind::Author => vec!["-i".into(), format!("--author={query}")],
            SearchKind::Content => vec!["-p".into(), format!("-S{query}")],
            SearchKind::Regex => vec!["-p".into(), format!("-G{query}")],
            SearchKind::File => vec!["-p".into(), "-m".into(), "--first-parent".into(), "--follow".into(), "--".into(), query.into()],
        }
    }
}

/// Commits of HEAD matching `query`, newest first (at most `limit`); their diff when git gives
/// it at once (content, file), else empty: read when the commit is chosen.
pub fn search(root: &Path, kind: SearchKind, query: &str, limit: usize) -> Result<Vec<LineCommit>, String> {
    let n = format!("-n{limit}");
    let mut args: Vec<String> = ["-c", "core.quotePath=false", "log", &n, "--date=format:%Y-%m-%d %H:%M", "--format=%x1e%H%x1f%an%x1f%ad%x1f%s"].map(String::from).to_vec();
    args.extend(kind.args(query));
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(parse_log(&stdout(root, &a)?))
}

/// Commits written by `--format=%x1e%H%x1f%an%x1f%ad%x1f%s`, each followed by its diff.
fn parse_log(out: &str) -> Vec<LineCommit> {
    out.split('\x1e')
        .filter(|c| !c.trim().is_empty())
        .map(|chunk| {
            let (meta, diff) = chunk.split_once('\n').unwrap_or((chunk, ""));
            let f: Vec<&str> = meta.split('\x1f').collect();
            let get = |i: usize| f.get(i).copied().unwrap_or("").to_string();
            // a very large commit is cut: the window shows the beginning
            let diff: String = diff.trim().chars().take(400_000).collect();
            LineCommit { hash: get(0), author: get(1), date: get(2), subject: get(3), diff }
        })
        .collect()
}

/// Changes put aside with `git stash`.
#[derive(Clone, Debug, PartialEq)]
pub struct Stash {
    /// Its commit (stays the same when other stashes come or go, unlike its name).
    pub hash: String,
    /// "stash@{0}", the newest.
    pub name: String,
    /// "On main: message" or "WIP on main: …".
    pub message: String,
    /// When, in unix seconds.
    pub time: i64,
}

/// The stashes of the repository, newest first.
pub fn stashes(root: &Path) -> Vec<Stash> {
    stdout(root, &["stash", "list", "--format=%H%x1f%gd%x1f%gs%x1f%ct"])
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let mut f = l.split('\x1f');
                    let (hash, name, message, time) = (f.next()?, f.next()?, f.next()?, f.next()?);
                    Some(Stash { hash: hash.into(), name: name.into(), message: message.into(), time: time.trim().parse().unwrap_or(0) })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `git stash <verb>` (apply, pop, drop) on the stash of commit `hash`, by its name of now: the
/// names shift when a stash comes or goes.
pub fn stash_op(root: &Path, verb: &str, hash: &str) -> Result<String, String> {
    let name = stashes(root).into_iter().find(|s| s.hash == hash).map(|s| s.name).ok_or(t!("ce stash n'existe plus", "this stash no longer exists"))?;
    let mut args = vec!["stash", verb];
    if verb != "drop" {
        // the index as it was too, when it can be
        args.push("--index");
    }
    args.push(&name);
    run(root, &args).or_else(|e| if verb != "drop" && e.contains("--index") { run(root, &["stash", verb, &name]) } else { Err(e) })
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
    let dir = file.parent().ok_or(t!("chemin invalide", "invalid path"))?;
    let name = file.file_name().and_then(|n| n.to_str()).ok_or(t!("chemin invalide", "invalid path"))?;
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

/// Every commit that touched lines `first..=last` (1-based, in HEAD), newest first, with its diff.
pub fn line_history(file: &Path, first: usize, last: usize) -> Result<Vec<LineCommit>, String> {
    let dir = file.parent().ok_or(t!("chemin invalide", "invalid path"))?;
    let name = file.file_name().and_then(|n| n.to_str()).ok_or(t!("chemin invalide", "invalid path"))?;
    let range = format!("{first},{}:{name}", last.max(first));
    // HEAD explicitly: a branch named like the file would be taken for it
    let out = stdout(dir, &["log", "--date=format:%Y-%m-%d %H:%M", "--format=%x1e%H%x1f%an%x1f%ad%x1f%s", "-L", &range, "HEAD"])?;
    let mut commits = parse_log(&out);
    // git shows only the followed lines: the whole hunks around them (with some context) read
    // better, like Sublime Merge does
    for c in commits.iter_mut().take(300) {
        let Some(path) = diff_path(&c.diff) else { continue };
        let top = format!(":(top){path}");
        let full = stdout(dir, &["show", "--format=", "--no-color", "-m", "--first-parent", "-U3", &c.hash, "--", &top]);
        if let Some(d) = full.ok().and_then(|f| hunks_around(&f, &c.diff)) {
            c.diff = d;
        }
    }
    Ok(commits)
}

/// The file of a one-file diff (its new path, or the old one when deleted).
fn diff_path(diff: &str) -> Option<String> {
    let new = diff.lines().find_map(|l| l.strip_prefix("+++ b/"));
    let old = diff.lines().find_map(|l| l.strip_prefix("--- a/"));
    new.or(old).map(|p| p.trim_end_matches(['\t', '\r']).to_string())
}

/// (old, new) line ranges of a "@@ -a,b +c,d @@" line, half-open, at least one line long.
fn hunk_ranges(line: &str) -> Option<((usize, usize), (usize, usize))> {
    let mut parts = line.strip_prefix("@@ ")?.split(' ');
    let range = |s: &str| -> Option<(usize, usize)> {
        let (start, len) = s.split_once(',').map_or((s, "1"), |(a, b)| (a, b));
        let (start, len): (usize, usize) = (start.parse().ok()?, len.parse().ok()?);
        Some((start, start + len.max(1)))
    };
    Some((range(parts.next()?.strip_prefix('-')?)?, range(parts.next()?.strip_prefix('+')?)?))
}

/// The hunks of `full` (the whole diff of a commit for one file) that touch the lines followed
/// by `git log -L` (`tracked`, its diff of the same commit), with the file header; None when
/// none does.
fn hunks_around(full: &str, tracked: &str) -> Option<String> {
    let wanted: Vec<_> = tracked.lines().filter_map(hunk_ranges).collect();
    let overlaps = |a: (usize, usize), b: (usize, usize)| a.0 < b.1 && b.0 < a.1;
    let mut out: Vec<&str> = Vec::new();
    let mut keep = false;
    let mut any = false;
    for l in full.lines() {
        if let Some((old, new)) = l.starts_with("@@").then(|| hunk_ranges(l)).flatten() {
            keep = wanted.iter().any(|(o, n)| overlaps(old, *o) || overlaps(new, *n));
            any |= keep;
        } else if l.starts_with("diff --git") {
            keep = true;
        }
        if keep {
            out.push(l);
        }
    }
    any.then(|| out.join("\n"))
}

/// "il y a 3 jours" from a unix timestamp.
pub fn ago(t: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(t);
    let d = (now - t).max(0);
    let plural = |n: i64, unit: &str| {
        let s = if n > 1 && !unit.ends_with('s') { "s" } else { "" };
        tf!("il y a {n} {unit}{s}", "{n} {unit}{s} ago")
    };
    match d {
        0..=59 => t!("à l'instant", "just now").into(),
        60..=3599 => tf!("il y a {} min", "{} min ago", d / 60),
        3600..=86_399 => plural(d / 3600, t!("heure", "hour")),
        86_400..=2_591_999 => plural(d / 86_400, t!("jour", "day")),
        2_592_000..=31_535_999 => {
            let n = d / 2_592_000;
            if crate::i18n::en() {
                format!("{n} month{} ago", if n > 1 { "s" } else { "" })
            } else {
                format!("il y a {n} mois")
            }
        }
        _ => plural(d / 31_536_000, t!("an", "year")),
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
    fn line_history_keeps_whole_hunks() {
        let full = "diff --git a/m.py b/m.py\n--- a/m.py\n+++ b/m.py\n@@ -10,3 +10,3 @@\n a\n-b\n+B\n c\n@@ -249,8 +249,7 @@ def f():\n x\n-    if not LOCAL:\n-        old = f(o)\n-        new = f(n)\n+    old = f(o)\n+    new = f(n)\n y";
        let tracked = "diff --git a/m.py b/m.py\n--- a/m.py\n+++ b/m.py\n@@ -250,3 +250,1 @@ def f():\n-    if not LOCAL:\n-        old = f(o)\n-        new = f(n)\n+    old = f(o)";
        assert_eq!(diff_path(tracked).as_deref(), Some("m.py"));
        let d = hunks_around(full, tracked).unwrap();
        assert!(d.starts_with("diff --git") && d.contains("+    new = f(n)") && d.contains("\n x\n") && !d.contains("+B"), "{d}");
        // no hunk touches the lines: git's own diff is kept
        assert!(hunks_around(full, "@@ -500,1 +500,1 @@\n-z\n+Z").is_none());
    }

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
        std::fs::write(repo.join("f"), "3\n").unwrap();
        g(&["stash", "push", "-m", "deuxième"]);
        let list = stashes(&repo);
        assert_eq!(list.iter().map(|s| (s.name.as_str(), s.message.as_str())).collect::<Vec<_>>(), [("stash@{0}", "On main: deuxième"), ("stash@{1}", "On main: essai")]);
        assert!(list[0].hash.len() == 40 && list[0].time > 0);
        // by its commit: the older one, though its name was stash@{1}
        stash_op(&repo, "drop", &list[1].hash).unwrap();
        assert_eq!(stashes(&repo).iter().map(|s| s.message.as_str()).collect::<Vec<_>>(), ["On main: deuxième"]);
        stash_op(&repo, "pop", &list[0].hash).unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("f")).unwrap(), "3\n");
        assert!(stashes(&repo).is_empty() && stash_op(&repo, "pop", &list[0].hash).is_err());
        g(&["checkout", "-q", "--", "f"]);
        let st = status(&repo).unwrap();
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

    #[test]
    fn branches_search_reflog_bisect() {
        let repo = std::env::temp_dir().join(format!("florm_hist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let g = |args: &[&str]| run(&repo, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        g(&["init", "-q", "-b", "main"]);
        g(&["config", "user.name", "Ana"]);
        g(&["config", "user.email", "a@t"]);
        std::fs::write(repo.join("a.txt"), "un\ndeux\ntrois\n").unwrap();
        g(&["add", "."]);
        g(&["commit", "-qm", "premier"]);
        std::fs::write(repo.join("a.txt"), "un\nDEUX\ntrois\n").unwrap();
        g(&["commit", "-qam", "deuxième ligne changée"]);
        g(&["mv", "a.txt", "b.txt"]);
        std::fs::write(repo.join("b.txt"), "un\nDEUX\ntrois\nquatre\n").unwrap();
        g(&["add", "."]);
        g(&["commit", "-qm", "renommé"]);
        g(&["branch", "feature"]);
        // a remote whose name has a slash, and its branch
        g(&["update-ref", "refs/remotes/my/fork/feature", "HEAD"]);
        let list = branch_list(&repo);
        assert!(list.contains(&Branch { name: "feature".into(), remote: false, upstream: String::new() }));
        assert!(list.iter().any(|b| b.name == "my/fork/feature" && b.remote));
        let names = vec!["my".to_string(), "my/fork".to_string()];
        assert_eq!(split_remote(&names, "my/fork/feature"), Some(("my/fork".into(), "feature".into())));
        assert_eq!(split_remote(&names, "other/x"), None);
        // search: message, author, content, file through its rename
        let subjects = |k, q: &str| search(&repo, k, q, 50).unwrap().into_iter().map(|c| c.subject).collect::<Vec<_>>();
        assert_eq!(subjects(SearchKind::Message, "LIGNE"), ["deuxième ligne changée"]);
        assert_eq!(subjects(SearchKind::Author, "ana").len(), 3);
        assert_eq!(subjects(SearchKind::Content, "DEUX"), ["deuxième ligne changée"]);
        assert_eq!(subjects(SearchKind::Regex, "^qua"), ["renommé"]);
        assert_eq!(subjects(SearchKind::File, "b.txt"), ["renommé", "deuxième ligne changée", "premier"]);
        let found = search(&repo, SearchKind::Content, "DEUX", 50).unwrap();
        assert!(found[0].diff.contains("+DEUX"));
        // the history of lines 2-3 (in HEAD)
        let lines = line_history(&repo.join("b.txt"), 2, 2).unwrap();
        assert_eq!(lines.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), ["deuxième ligne changée", "premier"]);
        // the reflog keeps a commit lost by a reset
        let lost = g(&["rev-parse", "HEAD"]);
        g(&["reset", "-q", "--hard", "HEAD~1"]);
        let log = reflog(&repo, 10).unwrap();
        assert!(log[0].subject.starts_with("reset:") && log.iter().any(|c| c.hash == lost.trim()));
        // bisect in progress
        assert!(!status(&repo).unwrap().bisecting);
        g(&["bisect", "start"]);
        assert!(status(&repo).unwrap().bisecting);
        g(&["bisect", "reset"]);
        let _ = std::fs::remove_dir_all(&repo);
    }
}

/// The first commit of a new repository (`init_repo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstCommit {
    /// `git add .` then a commit of everything.
    All,
    /// An empty commit (to push the branch without any file).
    Empty,
    /// No commit (and so nothing pushed).
    Nothing,
}

/// A new repository in `dir`: `git init` on `branch`, `origin` set to `remote` (empty: none), the
/// first commit, and the branch pushed when there is a remote and a commit. What git said, or
/// where it stopped (the steps before stay done).
pub fn init_repo(dir: &Path, remote: &str, branch: &str, first: FirstCommit, message: &str) -> Result<String, String> {
    let mut log = Vec::new();
    let mut step = |label: String, args: &[&str]| -> Result<(), String> {
        log.push(format!("$ git {}", args.join(" ")));
        match run(dir, args) {
            Ok(out) => {
                if !out.is_empty() {
                    log.push(out);
                }
                Ok(())
            }
            Err(e) => Err(format!("{}\n{label} : {e}", log.join("\n"))),
        }
    };
    let branch = if branch.trim().is_empty() { "main" } else { branch.trim() };
    // `-b` needs git 2.28: before, the branch is named after the init
    if step(t!("git init", "git init").into(), &["init", "-b", branch]).is_err() {
        step(t!("git init", "git init").into(), &["init"])?;
        step(t!("nommer la branche", "naming the branch").into(), &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")])?;
    }
    let remote = remote.trim();
    if !remote.is_empty() {
        step(t!("ajouter le remote", "adding the remote").into(), &["remote", "add", "origin", remote])?;
    }
    let message = if message.trim().is_empty() { "Initial commit" } else { message.trim() };
    match first {
        FirstCommit::All => {
            step(t!("git add", "git add").into(), &["add", "-A", "."])?;
            step(t!("le premier commit", "the first commit").into(), &["commit", "-m", message])?;
        }
        FirstCommit::Empty => step(t!("le commit vide", "the empty commit").into(), &["commit", "--allow-empty", "-m", message])?,
        FirstCommit::Nothing => {}
    }
    if !remote.is_empty() && first != FirstCommit::Nothing {
        step(t!("git push", "git push").into(), &["push", "-u", "origin", branch])?;
    }
    Ok(log.join("\n"))
}

#[cfg(test)]
mod init_tests {
    use super::*;

    /// A new repository, everything committed and pushed to a remote (a bare repository here),
    /// then an empty one: the remote gets the branch with its commit.
    #[test]
    fn a_new_repository_is_pushed() {
        // a commit needs a git identity (not set here for the whole test process: other tests read the author)
        if run(&std::env::temp_dir(), &["config", "user.email"]).is_err() {
            return;
        }
        let base = std::env::temp_dir().join(format!("florm-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (remote, work, empty) = (base.join("remote.git"), base.join("work"), base.join("empty"));
        for d in [&remote, &work, &empty] {
            std::fs::create_dir_all(d).unwrap();
        }
        run(&remote, &["init", "--bare"]).unwrap();
        std::fs::write(work.join("a.txt"), "a\n").unwrap();
        let url = remote.display().to_string();
        init_repo(&work, &url, "main", FirstCommit::All, "Premier commit").unwrap();
        assert_eq!(run(&remote, &["log", "--format=%s", "main"]).unwrap(), "Premier commit");
        assert_eq!(run(&remote, &["ls-tree", "--name-only", "main"]).unwrap(), "a.txt");
        // an empty commit on another branch, no file
        init_repo(&empty, &url, "vide", FirstCommit::Empty, "").unwrap();
        assert_eq!(run(&remote, &["ls-tree", "--name-only", "vide"]).unwrap(), "");
        assert_eq!(run(&remote, &["log", "--format=%s", "vide"]).unwrap(), "Initial commit");
        // a remote that does not exist: it stops at the push, the repository stays
        let bad = base.join("bad");
        std::fs::create_dir_all(&bad).unwrap();
        let e = init_repo(&bad, "/nowhere/x.git", "main", FirstCommit::Empty, "x").unwrap_err();
        assert!(e.contains("push"), "{e}");
        assert!(bad.join(".git").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Versions side by side: `odoo/master` cloned from a remote named `odoo`, then 17.0 and
    /// saas-18.3 added as worktrees tracking it; a branch pushed later is fetched when added.
    #[test]
    fn versions_side_by_side() {
        let base = crate::os::canon(&std::env::temp_dir()).join(format!("florm-versions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (remote, work, src) = (base.join("remote.git"), base.join("work"), base.join("src"));
        for d in [&remote, &work, &src.join("odoo"), &src.join("internal/a"), &src.join("upgrade/b")] {
            std::fs::create_dir_all(d).unwrap();
        }
        let id = ["-c", "user.name=Florm", "-c", "user.email=florm@example.com"];
        let commit = |msg: &str| run(&work, &[&id[..], &["commit", "--allow-empty", "-m", msg]].concat()).unwrap();
        run(&remote, &["init", "--bare"]).unwrap();
        run(&work, &["init", "-b", "master"]).unwrap();
        commit("m");
        run(&work, &["remote", "add", "origin", &remote.display().to_string()]).unwrap();
        for b in ["17.0", "saas-18.3"] {
            run(&work, &["checkout", "-b", b]).unwrap();
            commit(b);
        }
        run(&work, &["push", "origin", "master", "17.0", "saas-18.3"]).unwrap();
        let odoo = src.join("odoo");
        run(&odoo, &["clone", "-o", "odoo", "-b", "master", &remote.display().to_string(), "master"]).unwrap();
        // a fresh clone in a folder named after its branch: already versions
        let v = versions(&odoo).unwrap();
        assert_eq!((v.main.clone(), v.names.clone()), (odoo.join("master"), vec!["master".to_string()]));
        // several repositories side by side are not versions
        run(&src.join("internal"), &["init", "a"]).unwrap();
        run(&src.join("internal"), &["init", "b"]).unwrap();
        assert!(versions(&src.join("internal")).is_none());
        assert!(version_branches(&v.main).unwrap().contains(&"saas-18.3".to_string()));
        add_version(&v, "17.0").unwrap();
        add_version(&v, " saas-18.3 ").unwrap();
        assert_eq!(run(&odoo.join("17.0"), &["rev-parse", "--abbrev-ref", "@{u}"]).unwrap(), "odoo/17.0");
        assert_eq!(run(&odoo.join("saas-18.3"), &["log", "-1", "--format=%s"]).unwrap(), "saas-18.3");
        assert_eq!(versions(&odoo).unwrap().names, ["17.0", "master", "saas-18.3"]);
        // from the folder, a version or the folder above: the same versions
        for d in [odoo.clone(), odoo.join("17.0"), src.clone()] {
            assert_eq!(versions_near(&d).iter().map(|v| v.dir.clone()).collect::<Vec<_>>(), [odoo.clone()], "{d:?}");
        }
        // already there, unknown, then pushed after the clone: fetched when added
        assert!(add_version(&v, "17.0").unwrap_err().contains("17.0"));
        assert!(add_version(&v, "20.0").is_err());
        run(&work, &["checkout", "-b", "20.0"]).unwrap();
        commit("20");
        run(&work, &["push", "origin", "20.0"]).unwrap();
        add_version(&v, "20.0").unwrap();
        assert_eq!(run(&odoo.join("20.0"), &["log", "-1", "--format=%s"]).unwrap(), "20");
        // a local branch: checked out as it is
        run(&v.main, &["branch", "17.0-fix", "odoo/17.0"]).unwrap();
        add_version(&v, "17.0-fix").unwrap();
        assert!(odoo.join("17.0-fix/.git").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn the_state_of_a_version_is_read() {
        let s = parse_version_state("## 20.0...odoo/20.0 [ahead 1, behind 3]\n M a.py\nM  b.py\n");
        assert_eq!(s, VersionState { branch: "20.0".into(), upstream: "odoo/20.0".into(), ahead: 1, behind: 3, changes: 2 });
        let s = parse_version_state("## master...origin/master\n");
        assert_eq!((s.ahead, s.behind, s.changes, s.upstream.as_str()), (0, 0, 0, "origin/master"));
        assert_eq!(parse_version_state("## HEAD (no branch)\n").branch, "HEAD (no branch)");
        assert_eq!(parse_version_state("## 17.0-fix\n").upstream, "");
    }

    /// A version added from a repository, then removed: refused with a changed file unless forced;
    /// the repository itself is never removed.
    #[test]
    fn a_version_is_removed() {
        if run(&std::env::temp_dir(), &["config", "user.email"]).is_err() {
            return;
        }
        let base = std::env::temp_dir().join(format!("florm-version-remove-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let main = base.join("proj/master");
        std::fs::create_dir_all(&main).unwrap();
        run(&main, &["init", "-b", "master"]).unwrap();
        std::fs::write(main.join("a.txt"), "a\n").unwrap();
        run(&main, &["add", "."]).unwrap();
        run(&main, &["commit", "-m", "a"]).unwrap();
        run(&main, &["branch", "17.0"]).unwrap();
        let v = Versions { dir: base.join("proj"), main: main.clone(), names: vec!["master".into()] };
        add_version(&v, "17.0").unwrap();
        let v = versions(&base.join("proj")).unwrap();
        assert!(v.names.contains(&"17.0".to_string()) && v.names.contains(&"master".to_string()), "{:?}", v.names);
        assert_eq!(version_state(&base.join("proj/17.0")).unwrap().branch, "17.0");
        std::fs::write(base.join("proj/17.0/a.txt"), "changed\n").unwrap();
        assert!(remove_version(&v, "17.0", false).is_err());
        remove_version(&v, "17.0", true).unwrap();
        assert!(!base.join("proj/17.0").exists());
        assert!(remove_version(&v, "master", true).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
