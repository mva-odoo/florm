//! Background work (git, linters, plugins, file indexing) on threads; results
//! come back to the UI thread through a channel polled every frame.

use crate::git;
use crate::history;
use crate::lint::Diagnostic;
use crate::plugins::Output;
use crate::search;
use crate::split;
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

pub enum JobResult {
    /// Changes since the start of the review of Claude's work (a diff).
    ClaudeReview(Result<String, String>),
    /// Commit message written by Claude.
    CommitMessage(Result<String, String>),
    /// The repository pull requests go to, and its default branch.
    GhBase(PathBuf, Result<(String, String), String>),
    /// Pull requests of a branch (repository, branch).
    GhBranchPrs(PathBuf, String, Result<Vec<crate::github::PrSummary>, String>),
    /// Pull requests containing a commit.
    GhCommitPrs(String, Result<Vec<crate::github::LinkedPr>, String>),
    /// Branch pushed and pull request created: its address.
    PrCreated(PathBuf, Result<String, String>),
    /// Pull requests of a repository (`gh pr list`).
    PrList(PathBuf, Result<Vec<crate::github::PrSummary>, String>),
    PrDetail(PathBuf, u64, Result<crate::github::PrDetail, String>),
    /// Diff of a commit of the pull request shown (its hash).
    PrCommitDiff(String, Result<String, String>),
    /// `gh pr checkout`, a comment sent…: the pull request to read again, the draft sent.
    PrOp { label: String, result: Result<String, String>, reload: Option<u64>, draft: Option<String> },
    Analyzed {
        uid: u64,
        version: u64,
        reload: bool,
        /// diagnostics and the linter that produced them
        diags: Option<(Vec<Diagnostic>, String)>,
        /// File content in HEAD (None: untracked / no repository)
        base: Option<String>,
        log: Vec<String>,
    },
    FileIndex(PathBuf, Vec<PathBuf>),
    /// Live lint of the buffer while typing.
    Lint { uid: u64, version: u64, diags: Vec<Diagnostic>, linter: String },
    ToolInstalled(Result<String, String>),
    GitStatus {
        selected: PathBuf,
        statuses: Vec<(PathBuf, Result<git::Status, String>)>,
        /// `git remote -v` of each repository.
        remotes: Vec<(PathBuf, String)>,
    },
    History(PathBuf, u64, Result<history::Graph, String>),
    HistoryDetails(String, Result<history::Details, String>),
    Blame { uid: u64, line: usize, version: u64, result: Result<git::Blame, String> },
    LineHistory(Result<Vec<git::LineCommit>, String>),
    HistoryDiff { hash: String, path: Option<String>, result: Result<String, String> },
    GitBranches(Vec<String>),
    /// `status`: the repository read again right after, in the same job (shown at once).
    GitOp { label: String, result: Result<String, String>, status: Option<(PathBuf, Result<git::Status, String>)> },
    NewTab { title: String, text: String, syntax: String },
    Search(u64, search::Outcome),
    /// Native file dialog closed: what it was for (0 folder, 1 file, 2 save as) and the choice.
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    Picked(crate::os::Pick, Result<Option<PathBuf>, String>),
    /// Fonts installed on the system (preferences).
    Fonts(Vec<crate::fonts::FontInfo>),
    /// Git repositories found in the opened folder.
    Repos(PathBuf, Vec<PathBuf>),
    /// Symbols of the whole project (go to definition, completion).
    SymbolIndex(PathBuf, crate::index::Index),
    /// Commit split started (or resumed): the commit and its changes.
    SplitStarted(PathBuf, Result<(split::Info, Vec<split::FileChange>), String>),
    /// Changes left after a refresh or a partial commit (with its short hash and subject).
    SplitChanges { result: Result<Vec<split::FileChange>, String>, committed: Option<(String, String)> },
    SplitEnded { aborted: bool, result: Result<String, String> },
    Plugin { uid: u64, version: u64, title: String, output: Output, result: Result<Vec<String>, String> },
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

    pub fn spawn(&self, f: impl FnOnce() -> JobResult + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ctx.request_repaint();
        });
    }
}
