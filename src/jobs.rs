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
    ClaudeReview(u64, Result<String, String>),
    /// Answer of a `/btw` of conversation (id).
    ClaudeBtw(u64, Result<String, String>),
    /// Model suggested for a message of conversation (id) before it starts; None: the current one.
    ClaudeModelCheck(u64, Result<Option<(usize, String)>, String>),
    /// The changes of one question of a Claude conversation (its message's index): a diff.
    ClaudeTurnDiff(u64, usize, Result<String, String>),
    /// The MCP servers of a Claude conversation (true: asked by /mcp).
    ClaudeMcp(u64, bool, Result<Vec<crate::claude::Mcp>, String>),
    /// A worktree made for a new Claude conversation: its folder, its branch, the main repository.
    ClaudeWorktree(Result<(std::path::PathBuf, String, std::path::PathBuf), String>),
    /// The changes not committed split by Claude into commits: the blocks, then each commit's
    /// message and blocks.
    CommitPlan(PathBuf, Result<(Vec<split::Block>, Vec<(String, Vec<usize>)>), String>),
    /// The commits of the plan made (short hash, subject), and the error that stopped them.
    CommitPlanDone(PathBuf, Vec<(String, String)>, Option<String>),
    /// Commit message written by Claude.
    CommitMessage(Result<String, String>),
    /// Commit after a task of the queue of conversation (id): the first line of its message.
    QueueCommit(u64, Result<String, String>),
    /// Remarks of an AI review (key of what was reviewed: a pull request, the changes not committed).
    AiReview(String, Result<Vec<crate::claude::ReviewNote>, String>),
    /// Answer of Claude on a selection in the editor (number of the question).
    InlineAi(u64, Result<crate::claude::Inline, String>),
    /// The repository pull requests go to, and its default branch.
    GhBase(PathBuf, Result<(String, String), String>),
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
    /// Labels and milestones of the repository of the pull requests.
    PrChoices(PathBuf, Result<(Vec<crate::github::Label>, Vec<String>), String>),
    /// A SQL query answered (its number).
    SqlDone(u64, Result<crate::sql::Outcome, String>),
    /// The databases of the PostgreSQL server.
    SqlDbs(Result<Vec<String>, String>),
    /// Two files or folders compared: the title of the tab, the diff.
    Compare(String, Result<String, String>),
    /// Log of a check (its name).
    PrLog(String, Result<String, String>),
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
    /// Go to file scored in the background: the query and its number, every match (to narrow
    /// the next letter) and the best ones, in order.
    FileMatches { gen: u64, query: String, matched: Vec<usize>, top: Vec<usize> },
    /// The files of the project (sorted) and their go to file labels; `None`: unchanged.
    FileIndex(PathBuf, Option<(Vec<PathBuf>, Vec<String>)>),
    /// Live lint of the buffer while typing.
    Lint { uid: u64, version: u64, diags: Vec<Diagnostic>, linter: String },
    ToolInstalled(Result<String, String>),
    GitStatus {
        selected: PathBuf,
        statuses: Vec<(PathBuf, Result<git::Status, String>)>,
        /// `git remote -v` of each repository.
        remotes: Vec<(PathBuf, String)>,
        /// Stashes of the selected repository.
        stashes: Vec<git::Stash>,
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
    /// `git init` in a folder of the tree: its output, then the repositories of the project again.
    RepoInit { root: PathBuf, dir: PathBuf, result: Result<String, String>, repos: Vec<PathBuf> },
    /// Symbols of the whole project (go to definition, completion).
    /// `bool`: only the repositories being worked on so far, the whole index follows.
    SymbolIndex(PathBuf, crate::index::Index, bool),
    /// Commit split started (or resumed): the commit and its changes.
    SplitStarted(PathBuf, Result<(split::Info, Vec<split::FileChange>), String>),
    /// Changes left after a refresh or a partial commit (with its short hash and subject).
    SplitChanges { result: Result<Vec<split::FileChange>, String>, committed: Option<(String, String)> },
    SplitEnded { aborted: bool, result: Result<String, String> },
    /// Newer release of Florm (`manual`: asked from the menu, so "up to date" is shown too).
    /// The containers, and the compose projects of the open folder.
    DockerList(Result<Vec<crate::docker::Container>, String>, Vec<crate::docker::Compose>),
    /// `None`: the container did not change since its logs were shown.
    DockerFollow { name: String, result: Result<Option<crate::docker::Follow>, String> },
    DockerDone { label: String, result: Result<(), String> },
    /// Odoo tests ready to run in a container.
    TestsReady { tags: String, result: Result<crate::docker::Exec, String> },
    /// The debugger checked what it needs and found the program (or not).
    /// A debugging session prepared (its number: a later one replaces it).
    DebugReady { id: u64, cfg: crate::dap::Config, title: String, result: Result<crate::dap::Prepared, crate::dap::Failure> },
    /// The running containers, for the debugger.
    DebugContainers(Result<Vec<String>, String>),
    /// A file of a container read for the debugger: its path there, the line to show.
    DebugSource { remote: String, line: usize, result: Result<String, String> },
    Update { manual: bool, result: Result<Option<crate::update::Update>, String> },
    /// Update downloaded and installed: the program to start for the restart.
    UpdateInstalled { version: String, result: Result<std::path::PathBuf, String> },
    /// Shared configuration synchronized, published or created: its `shared_config` and the message.
    Shared(crate::shared::Op, Result<(String, String), String>),
    /// Connected to another machine: the folder to start from there.
    RemoteConnected(crate::remote::Target, Result<PathBuf, String>),
    /// Host of an SSH project mounted: the project to open (`switch`: from another project).
    RemoteMounted { path: PathBuf, switch: bool, result: Result<(), String> },
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
