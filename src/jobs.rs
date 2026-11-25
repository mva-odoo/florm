//! Background work (git, linters, file indexing) on threads; results
//! come back to the UI thread through a channel polled every frame.

use crate::git;
use crate::history;
use crate::lint::Diagnostic;
use crate::search;
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

#[cfg(test)]
mod par_tests {
    #[test]
    fn par_map_keeps_the_order() {
        let items: Vec<u64> = (0..50).collect();
        let out = super::par_map(&items, 8, |n| {
            std::thread::sleep(std::time::Duration::from_millis(50 - n));
            n * 2
        });
        assert_eq!(out, items.iter().map(|n| n * 2).collect::<Vec<_>>());
        assert!(super::par_map(&Vec::<u8>::new(), 8, |n| *n).is_empty());
    }
}

pub enum JobResult {
    /// A commit read for Claude (repository): its label and its text.
    ClaudeCommit(PathBuf, Result<(String, String), String>),
    /// The request « Résoudre avec Claude » written (repository, number of files, text).
    ConflictsForClaude(PathBuf, usize, String),
    /// The stashes of a repository.
    Stashes(PathBuf, Vec<git::Stash>),
    /// The dates read on the disk every 2 s: the files of git of each repository, and each file
    /// open (uid, the date it had when asked).
    DiskCheck { dirs: Vec<(PathBuf, crate::sidebar::Scanned)>, stamps: Vec<(PathBuf, Vec<Option<std::time::SystemTime>>)>, files: Vec<(u64, std::time::SystemTime, Result<std::time::SystemTime, std::io::ErrorKind>)> },
    /// The diff against HEAD of a document (uid) at a version, for the gutter.
    Diffed { uid: u64, version: u64, hunks: Vec<crate::diff::Hunk> },
    /// A Python document (uid) at a version formatted or fixed by ruff (the lines formatted, if
    /// only some), and saved after when it was Ctrl+S.
    Rewritten { uid: u64, version: u64, what: crate::lint::Rewrite, lines: Option<(usize, usize)>, then_save: bool, result: Result<String, String> },
    /// The last commits of the folder of a Claude conversation (`git log` lines).
    ClaudeCommits(u64, String),
    /// What `claude auth status` says of the Claude account named so.
    ClaudeAuth(String, Result<crate::claude_account::Status, String>),
    /// The commits of a pull request fetched to show one of them in the Git view: repository,
    /// commit, and what git said.
    PrCommitFetched(std::path::PathBuf, String, Result<String, String>),
    /// Commit message written by Claude.
    CommitMessage(Result<String, String>),
    /// Commit after a task of the queue of conversation (id): the first line of its message.
    /// Remarks of an AI review (key of what was reviewed: a pull request, the changes not committed).
    AiReview(String, Result<Vec<crate::claude::ReviewNote>, String>),
    /// The words a document (uid) at a version does not spell right.
    Spelled { uid: u64, version: u64, list: Vec<crate::spell::Misspelled> },
    /// Claude's answer about a remark of a review: (key of the review, id of the remark).
    AiRefined { key: String, id: u64, result: Result<(String, Option<String>), String> },
    /// Suggestion of the AI completion (number of the request); empty: nothing to suggest.
    AiComplete(u64, Result<String, String>),
    /// Where GitHub Copilot stands ("OK", "NotSignedIn"…), after a check, an installation or a sign-in.
    CopilotState(Result<String, String>),
    /// The code to type on GitHub to sign Copilot in, and the page to type it in.
    CopilotCode(Result<(String, String), String>),
    /// The repository pull requests go to, and its default branch.
    GhBase(PathBuf, Result<(String, String), String>),
    /// What the window « Créer une pull request » needs from git, read off the UI thread:
    /// repository, branch, upstream remote, remotes (name, GitHub owner), last commit's subject and
    /// body, every remote branch ("remote/branch").
    PrCreateInfo { repo: PathBuf, branch: String, upstream: Option<String>, remotes: Vec<(String, String)>, title: String, body: String, refs: Vec<String> },
    /// Pull requests of a branch (repository, branch).
    GhBranchPrs(PathBuf, String, Result<Vec<crate::github::PrSummary>, String>),
    /// Pull requests containing a commit.
    GhCommitPrs(String, Result<Vec<crate::github::LinkedPr>, String>),
    /// Branch pushed and pull request created: its address.
    PrCreated(PathBuf, Result<String, String>),
    /// The description of the pull request being created, written by Claude.
    PrBody(Result<String, String>),
    /// Pull requests of a repository (`gh pr list`).
    PrList(PathBuf, Result<Vec<crate::github::PrSummary>, String>),
    PrDetail(PathBuf, u64, Result<crate::github::PrDetail, String>),
    /// Diff of a commit of the pull request shown (its hash).
    PrCommitDiff(String, Result<String, String>),
    /// `gh pr checkout`, a comment sent…: the pull request to read again, the draft sent.
    PrOp { label: String, result: Result<String, String>, reload: Option<u64>, draft: Option<String> },
    /// Two files or folders compared: the title of the tab, the diff.
    Compare(String, Result<String, String>),
    /// Log of a check (its name).
    PrLog(String, Result<String, String>),
    Analyzed {
        uid: u64,
        /// diagnostics and the linter that produced them
        diags: Option<(Vec<Diagnostic>, String)>,
        /// File content in HEAD (None: untracked / no repository)
        base: Option<String>,
    },
    /// Go to file scored in the background: the query and its number, every match (to narrow
    /// the next letter) and the best ones, in order.
    FileMatches { gen: u64, query: String, matched: Vec<usize>, top: Vec<usize> },
    /// The files of the project (sorted) and their go to file labels; `None`: unchanged.
    FileIndex(PathBuf, Option<crate::filelist::FileList>),
    /// Live lint of the buffer while typing.
    Lint { uid: u64, version: u64, diags: Vec<Diagnostic>, linter: String },
    ToolInstalled(Result<String, String>),
    /// The language packs, and for each of their tools whether it is there.
    PackStatus(Vec<crate::packs::Pack>, Vec<Vec<bool>>),
    /// Tools of a language pack installed (or the log of what failed).
    PackInstalled(Result<String, String>),
    /// The blame of every line of a document (for its version).
    BlameLines { uid: u64, version: u64, result: Result<Vec<crate::git::Blame>, String> },
    /// Python downloaded for the extensions (or why not).
    PythonReady(Result<(), String>),
    /// The catalogue repositories cloned again (the errors).
    CatalogFetched(Vec<String>),
    /// A debug session started on a thread (its adapter found, the program built): the session
    /// and its `launch` / `attach` request, or why it could not.
    DebugStarted(Result<crate::app::DebugStart, String>),
    GitStatus {
        selected: PathBuf,
        /// Of the repositories looked at (all of them, or those asked).
        statuses: Vec<(PathBuf, Result<git::Status, String>)>,
        /// `git remote -v` of the repositories looked at, read again when their git files changed,
        /// with the dates of these files.
        remotes: Vec<(PathBuf, Vec<Option<std::time::SystemTime>>, String)>,
        /// Stashes of the selected repository, when it was looked at.
        stashes: Option<Vec<git::Stash>>,
    },
    History(PathBuf, u64, Result<history::Graph, String>),
    HistoryDetails(String, Result<history::Details, String>),
    Blame { uid: u64, line: usize, version: u64, result: Result<git::Blame, String> },
    LineHistory(Result<Vec<git::LineCommit>, String>),
    /// Diff of a commit of the history window (its hash), read when it is chosen.
    LineHistoryDiff(String, Result<String, String>),
    /// Message of the last commit, to amend it.
    LastCommitMessage(Result<String, String>),
    /// `git clean -n -d`: what would be deleted.
    CleanPreview(Result<String, String>),
    HistoryDiff { hash: String, path: Option<String>, result: Result<String, String> },
    /// The branches, what to do with the one chosen, and the one already chosen if any.
    GitBranches(Vec<git::Branch>, git::BranchPick, Option<String>),
    /// The interactive rebase prepared: its first commit and the commits up to HEAD.
    RebasePlan(PathBuf, Result<(String, Vec<crate::rebase::Step>), String>),
    /// `status`: the repository read again right after, in the same job (shown at once).
    GitOp { label: String, result: Result<String, String>, status: Option<(PathBuf, Result<git::Status, String>)> },
    NewTab { title: String, text: String, syntax: String },
    Search(u64, search::Outcome),
    /// Native file dialog closed: what it was for (0 folder, 1 file, 2 save as) and the choice.
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    Picked(crate::os::Pick, Result<Vec<PathBuf>, String>),
    /// An image file for Claude, as PNG.
    ClaudeImage { label: String, png: Result<Vec<u8>, String> },
    /// Fonts installed on the system (preferences).
    Fonts(Vec<crate::fonts::FontInfo>),
    /// Git repositories found in the opened folder.
    Repos(PathBuf, Vec<PathBuf>),
    /// XML / JSON checked (document uid, version checked).
    LintedInProcess { uid: u64, version: u64, diags: Vec<Diagnostic> },
    /// `git init` in a folder of the tree: its output, then the repositories of the project again.
    RepoInit { root: PathBuf, dir: PathBuf, result: Result<String, String>, repos: Vec<PathBuf> },
    /// Symbols of the whole project (go to definition, completion).
    /// `bool`: only the repositories being worked on so far, the whole index follows.
    SymbolIndex(PathBuf, crate::index::Index, bool),
    Update { manual: bool, result: Result<Option<crate::update::Update>, String> },
    /// Update downloaded and installed: the program to start for the restart.
    UpdateInstalled { version: String, result: Result<std::path::PathBuf, String> },
    /// Shared configuration synchronized, published or created: its `shared_config` and the message.
    Shared(crate::shared::Op, Result<(String, String), String>),
}

/// `f` on each item, `threads` at a time, the results in the order of the items: for the same
/// slow command run on many repositories (`git status` on each one).
pub fn par_map<T: Sync, R: Send>(items: &[T], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    let mut out: Vec<(usize, R)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads.clamp(1, items.len().max(1)))
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(k) else { break };
                        done.push((k, f(item)));
                    }
                    done
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
    });
    out.sort_by_key(|(k, _)| *k);
    out.into_iter().map(|(_, r)| r).collect()
}

pub struct Jobs {
    tx: Sender<JobResult>,
    pub rx: Receiver<JobResult>,
    ctx: egui::Context,
}

impl Jobs {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        Jobs { tx, rx, ctx }
    }

    /// Sends a result before the end of a job (a first part of the work).
    pub fn sender(&self) -> impl Fn(JobResult) + Send + 'static {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        move |r| {
            let _ = tx.send(r);
            ctx.request_repaint();
        }
    }

    pub fn spawn(&self, f: impl FnOnce() -> JobResult + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ctx.request_repaint();
        });
    }
}
