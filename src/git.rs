//! Git through the `git` CLI (no libgit2: lighter, and uses the user's own config/credentials).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// git with `args`.
///
/// A repository received from someone else (a zip, a download) brings its `.git/config`, which
/// can name commands: Florm runs git by itself (status when a folder is opened, diffs, blame),
/// so those are turned off here: `core.fsmonitor` (run by status and diff), the `textconv` and
/// external diff programs (run to show a diff). Hooks of commits and pushes still run.
fn git(args: &[&str]) -> std::process::Command {
    let mut c = crate::os::command("git");
    // `git status` must not rewrite the index: that would look like a change made outside Florm
    // (see `stamp`), and could collide with a git command run by the user in a terminal
    c.env("GIT_OPTIONAL_LOCKS", "0");
    c.args(["-c", "core.fsmonitor=false"]).args(hardened(args));
    c
}

/// Commands that show diffs: they take `--no-textconv --no-ext-diff`.
const DIFF_COMMANDS: &[&str] = &["diff", "log", "show", "blame", "reflog", "format-patch"];

/// `args` with `--no-textconv --no-ext-diff` right after the command when it shows a diff (after
/// `stash show` for a stash).
fn hardened<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut out = args.to_vec();
    let i = subcommand_at(args);
    let at = match args.get(i) {
        Some(sub) if DIFF_COMMANDS.contains(sub) => Some(i + 1),
        Some(&"stash") if args.get(i + 1) == Some(&"show") => Some(i + 2),
        _ => None,
    };
    if let Some(at) = at {
        out.splice(at..at, ["--no-textconv", "--no-ext-diff"]);
    }
    out
}

/// Where the command is in `args`: the first word that is not a global option (`-c x=y` and
/// `-C dir` take a value).
fn subcommand_at(args: &[&str]) -> usize {
    let mut i = 0;
    while i < args.len() && args[i].starts_with('-') {
        i += if matches!(args[i], "-c" | "-C") { 2 } else { 1 };
    }
    i
}

fn subcommand<'a>(args: &[&'a str]) -> Option<&'a str> {
    args.get(subcommand_at(args)).copied()
}

/// Git turned off (Préférences → Git, or `git` not installed): nothing is run.
fn off() -> Option<String> {
    (!crate::features::on(crate::features::Feature::Git)).then(|| t!("Git est désactivé (Préférences → Git)", "Git is turned off (Preferences → Git)").to_string())
}

/// The trusted folders (settings `trusted_folders`): the configuration of a repository
/// (`.git/config`, filters of `.gitattributes`) can name commands that git runs (a `clean` filter
/// runs on `git status`), so git only runs in them. None: no limit (until the app sets it).
static TRUSTED: std::sync::RwLock<Option<Vec<PathBuf>>> = std::sync::RwLock::new(None);

/// The trusted folders, set by the app (and again when they change).
pub fn set_trusted(list: &[String]) {
    // tests make their repositories anywhere, from several threads: never limited there
    if cfg!(test) {
        return;
    }
    let dirs = list.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).map(PathBuf::from).collect();
    *TRUSTED.write().unwrap_or_else(|e| e.into_inner()) = Some(dirs);
}

/// git may run in `dir`: a trusted folder (or no limit set).
pub fn trusted(dir: &Path) -> bool {
    allowed(TRUSTED.read().unwrap_or_else(|e| e.into_inner()).as_deref(), dir, &[])
}

/// git may run `args` in `dir`: inside a trusted folder, or a command that reads no repository
/// configuration (`clone` and `init` make a new one).
fn allowed(trusted: Option<&[PathBuf]>, dir: &Path, args: &[&str]) -> bool {
    let Some(list) = trusted else { return true };
    matches!(subcommand(args), Some("clone" | "init")) || list.iter().any(|t| dir.starts_with(t))
}

/// Why git does not run in `dir` (not a trusted folder), if so.
fn untrusted(dir: &Path, args: &[&str]) -> Option<String> {
    (!allowed(TRUSTED.read().unwrap_or_else(|e| e.into_inner()).as_deref(), dir, args)).then(|| untrusted_error().to_string())
}

/// The error of git in a folder not trusted.
pub fn untrusted_error() -> &'static str {
    t!(
        "🔒 Dossier pas de confiance : Git n'y tourne pas (sa configuration peut lancer des commandes)",
        "🔒 Folder not trusted: Git does not run there (its configuration can run commands)"
    )
}

fn exec(dir: &Path, args: &[&str]) -> Result<(String, String), String> {
    if let Some(e) = off().or_else(|| untrusted(dir, args)) {
        return Err(e);
    }
    let out = git(args)
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
    if let Some(e) = off().or_else(|| untrusted(dir, args)) {
        return Err(e);
    }
    let mut child = git(args)
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

/// The git folder of `repo`: `.git`, or for a worktree / submodule the one its `.git` file names.
fn git_dir(repo: &Path) -> Option<PathBuf> {
    let dot = repo.join(".git");
    if !dot.is_file() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    Some(repo.join(text.trim().strip_prefix("gitdir:")?.trim()))
}

/// The branch checked out in `repo`, read from its HEAD without running git (cheap enough to
/// ask at every frame); the short hash of a detached HEAD.
pub fn head_branch(repo: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir(repo)?.join("HEAD")).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: refs/heads/") {
        Some(b) => b.to_string(),
        None => head.chars().take(8).collect(),
    })
}

/// Modification times of the files git rewrites on a commit, checkout, staging, fetch…: when it
/// changes, the repository was changed (possibly outside Florm, e.g. in a terminal).
pub fn stamp(repo: &Path) -> Vec<Option<std::time::SystemTime>> {
    let Some(dir) = git_dir(repo) else { return Vec::new() };
    ["HEAD", "index", "logs/HEAD", "FETCH_HEAD", "ORIG_HEAD", "MERGE_HEAD", "rebase-merge"]
        .iter()
        .map(|f| std::fs::metadata(dir.join(f)).and_then(|m| m.modified()).ok())
        .collect()
}

/// `stamp`, and the date of the configuration of `repo` (its remotes are written there).
pub fn config_stamp(repo: &Path) -> Vec<Option<std::time::SystemTime>> {
    let mut st = stamp(repo);
    st.push(git_dir(repo).and_then(|d| std::fs::metadata(d.join("config")).and_then(|m| m.modified()).ok()));
    st
}

/// Git repositories inside `root` (up to `max_depth` levels), plus the repository
/// containing `root` when it is a subfolder of one.
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

/// A rebase stopped half way (conflict, edit, interactive).
pub fn rebase_in_progress(repo: &Path) -> bool {
    git_path_exists(repo, "rebase-merge") || git_path_exists(repo, "rebase-apply")
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
    /// The commits of the current branch that it lacks, in the interactive rebase.
    RebaseInteractive,
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
    if let Some(e) = untrusted(dir, &[]) {
        return Err(e);
    }
    let mut child = git(&["blame", "--porcelain", "-L", &range, "--contents", "-", "--", name])
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

/// The blame of every line of `file` as it is in HEAD (the editor maps its lines through the
/// changes not committed: one `git blame` per commit, not per edit; it takes seconds on a long
/// history). With `contents`: of that text instead (lines not committed: a hash of zeros).
pub fn blame_file(file: &Path, contents: Option<&str>) -> Result<Vec<Blame>, String> {
    let dir = file.parent().ok_or(t!("chemin invalide", "invalid path"))?;
    let name = file.file_name().and_then(|n| n.to_str()).ok_or(t!("chemin invalide", "invalid path"))?;
    if let Some(e) = untrusted(dir, &[]) {
        return Err(e);
    }
    let args: Vec<&str> = match contents {
        Some(_) => vec!["blame", "--porcelain", "--contents", "-", "--", name],
        None => vec!["blame", "--porcelain", "HEAD", "--", name],
    };
    let mut child = git(&args)
        .current_dir(dir)
        .stdin(if contents.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let (Some(mut stdin), Some(contents)) = (child.stdin.take(), contents) {
        let contents = contents.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(contents.as_bytes());
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(parse_blame_porcelain(&String::from_utf8_lossy(&out.stdout)))
}

/// The lines of `git blame --porcelain`, in their order (a commit's details come once, at its
/// first line).
fn parse_blame_porcelain(text: &str) -> Vec<Blame> {
    use std::collections::HashMap;
    let mut known: HashMap<String, Blame> = HashMap::new();
    let mut out: Vec<Blame> = Vec::new();
    let mut cur: Option<(String, usize)> = None;
    for l in text.lines() {
        if let Some(code) = l.strip_prefix('\t') {
            let _ = code;
            if let Some((hash, line)) = cur.take() {
                let b = known.get(&hash).cloned().unwrap_or(Blame { hash: hash.clone(), author: String::new(), time: 0, summary: String::new() });
                if out.len() < line {
                    out.resize(line, Blame { hash: String::new(), author: String::new(), time: 0, summary: String::new() });
                }
                out[line - 1] = b;
            }
            continue;
        }
        let mut parts = l.split(' ');
        let first = parts.next().unwrap_or("");
        if first.len() == 40 && first.chars().all(|c| c.is_ascii_hexdigit()) {
            let line = parts.nth(1).and_then(|n| n.parse::<usize>().ok()).unwrap_or(out.len() + 1);
            known.entry(first.to_string()).or_insert_with(|| Blame { hash: first.to_string(), author: String::new(), time: 0, summary: String::new() });
            cur = Some((first.to_string(), line));
            continue;
        }
        let Some((hash, _)) = &cur else { continue };
        let Some(b) = known.get_mut(hash) else { continue };
        if let Some(v) = l.strip_prefix("author ") {
            b.author = v.to_string();
        } else if let Some(v) = l.strip_prefix("author-time ") {
            b.time = v.parse().unwrap_or(0);
        } else if let Some(v) = l.strip_prefix("summary ") {
            b.summary = v.to_string();
        }
    }
    out
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
    // git stops at the commit that wrote the lines: the creation of the file is worth seeing too
    // (through its renames), as Sublime Merge shows it
    let added = stdout(dir, &["log", "--follow", "--diff-filter=A", "--date=format:%Y-%m-%d %H:%M", "--format=%x1e%H%x1f%an%x1f%ad%x1f%s", "-p", "HEAD", "--", name]);
    if let Some(first) = added.ok().and_then(|out| parse_log(&out).pop()) {
        if !commits.iter().any(|c| c.hash == first.hash) {
            commits.push(first);
        }
    }
    Ok(commits)
}

/// The diff creates the file.
pub fn creates_file(diff: &str) -> bool {
    diff.lines().take(5).any(|l| l.starts_with("new file mode"))
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

    /// The blame of every line of a real repository, with a line not committed yet.
    #[test]
    fn blame_of_every_line() {
        let dir = std::env::temp_dir().join(format!("florm_blame_all_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = |args: &[&str]| assert!(std::process::Command::new("git").args(args).current_dir(&dir).output().unwrap().status.success(), "{args:?}");
        g(&["init", "-q"]);
        g(&["config", "user.email", "a@b.c"]);
        g(&["config", "user.name", "Alice"]);
        std::fs::write(dir.join("f.txt"), "one\ntwo\n").unwrap();
        g(&["add", "f.txt"]);
        g(&["commit", "-q", "-m", "first"]);
        let b = blame_file(&dir.join("f.txt"), Some("one\nnew\ntwo\n")).unwrap();
        assert_eq!(b.len(), 3);
        assert_eq!((b[0].author.as_str(), b[0].summary.as_str(), b[2].author.as_str()), ("Alice", "first", "Alice"));
        assert!(b[1].uncommitted() && !b[0].uncommitted() && b[0].hash == b[2].hash);
        // HEAD's lines, whatever the file on the disk
        std::fs::write(dir.join("f.txt"), "changed\n").unwrap();
        let h = blame_file(&dir.join("f.txt"), None).unwrap();
        assert!(h.len() == 2 && !h[1].uncommitted());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// git only in a trusted folder (and those inside), except to make a new repository.
    #[test]
    fn git_runs_only_in_a_trusted_folder() {
        let list = vec![PathBuf::from("/home/a/projet")];
        let ok = |dir: &str, args: &[&str]| allowed(Some(&list), Path::new(dir), args);
        assert!(ok("/home/a/projet", &["status"]));
        assert!(ok("/home/a/projet/addons/x", &["-c", "a=b", "log"]));
        assert!(!ok("/home/a/projet2", &["status"]));
        assert!(!ok("/tmp/recu", &["diff"]));
        assert!(ok("/tmp", &["clone", "https://x/y.git"]));
        assert!(ok("/tmp/nouveau", &["init"]));
        // nothing set yet: no limit
        assert!(allowed(None, Path::new("/tmp/recu"), &["status"]));
    }

    #[test]
    fn diffs_never_run_the_programs_of_the_repository() {
        let h = |a: &[&str]| hardened(a).join(" ");
        assert_eq!(h(&["diff", "--cached"]), "diff --no-textconv --no-ext-diff --cached");
        assert_eq!(h(&["-c", "color.ui=false", "log", "-p"]), "-c color.ui=false log --no-textconv --no-ext-diff -p");
        assert_eq!(h(&["stash", "show", "-p", "stash@{0}"]), "stash show --no-textconv --no-ext-diff -p stash@{0}");
        // the others do not take them
        for a in [&["status", "--porcelain=v2"][..], &["stash", "list"], &["rev-parse", "HEAD"], &["cat-file", "-p", "HEAD:f"], &["grep", "diff"]] {
            assert_eq!(hardened(a), a.to_vec());
        }
    }

    /// A repository whose configuration names programs (received in a zip): opening it and
    /// showing its diffs runs none of them.
    #[test]
    #[cfg(unix)]
    fn the_configuration_of_a_repository_runs_nothing() {
        let dir = std::env::temp_dir().join(format!("florm_git_trust_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mark = dir.join("ran");
        run(&dir, &["init", "-q"]).unwrap();
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        run(&dir, &["add", "a.txt"]).unwrap();
        run(&dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "a"]).unwrap();
        let cmd = format!("sh -c 'touch {}; echo x' --", mark.display());
        for (k, v) in [("core.fsmonitor", cmd.as_str()), ("diff.external", cmd.as_str()), ("diff.evil.textconv", cmd.as_str())] {
            run(&dir, &["config", k, v]).unwrap();
        }
        std::fs::write(dir.join(".gitattributes"), "*.txt diff=evil\n").unwrap();
        std::fs::write(dir.join("a.txt"), "b\n").unwrap();
        status(&dir).unwrap();
        for a in [&["diff"][..], &["log", "-p"], &["show"], &["diff", "HEAD", "--", "a.txt"]] {
            stdout(&dir, a).unwrap();
        }
        assert!(!mark.exists(), "a program of the repository ran");
        let _ = std::fs::remove_dir_all(&dir);
    }

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
}
