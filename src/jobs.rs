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
        log: Vec<git::LogEntry>,
        /// `git remote -v` of each repository.
        remotes: Vec<(PathBuf, String)>,
    },
    History(PathBuf, Result<history::Graph, String>),
    HistoryDetails(String, Result<history::Details, String>),
    Blame { uid: u64, line: usize, version: u64, result: Result<git::Blame, String> },
    LineHistory(Result<Vec<git::LineCommit>, String>),
    HistoryDiff { hash: String, path: Option<String>, result: Result<String, String> },
    GitBranches(Vec<String>),
    GitOp { label: String, result: Result<String, String> },
    NewTab { title: String, text: String, syntax: String },
    Search(u64, search::Outcome),
    /// Native file dialog closed: what it was for (0 folder, 1 file, 2 save as) and the choice.
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    Picked(u8, Option<PathBuf>),
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
