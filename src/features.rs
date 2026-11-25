//! Parts of Florm that can be turned off in the preferences: Git, GitHub (pull requests),
//! Claude. Off, their views, commands, menus, icons and buttons go away.

use crate::commands::Cmd;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    GitHub,
    Claude,
    /// Off (or `git` not installed): no git command is run.
    Git,
}

static OFF: [AtomicBool; 3] = [AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)];

pub fn on(f: Feature) -> bool {
    // the pull requests need git
    !OFF[f as usize].load(Ordering::Relaxed) && (f != Feature::GitHub || on(Feature::Git))
}

/// The three of them from the settings (`git`: on and installed).
pub fn set_all(github: bool, claude: bool, git: bool) {
    for (f, yes) in [(Feature::GitHub, github), (Feature::Claude, claude), (Feature::Git, git)] {
        OFF[f as usize].store(!yes, Ordering::Relaxed);
    }
}

/// The part a command belongs to (None: always there).
pub fn of_cmd(cmd: Cmd) -> Option<Feature> {
    use Cmd::*;
    Some(match cmd {
        PullRequests | CreatePr | CopyGithubLink => Feature::GitHub,
        ShowGit | GitAddVersion | GitAmend | GitBlame | GitCheckout | GitClean | GitCommit | GitDeleteBranch | GitDiffFile | GitFetch | GitFileHistory | GitForcePush | GitHistory | GitLineHistory | GitMerge | GitNewBranch | GitNextChange | GitPrevChange | GitPull | GitPullRebase | GitPush | GitPushTags | GitRebaseOnto | GitFetchRebase | GitRebaseInteractive | GitReflog | GitRefresh | GitRenameBranch | GitRevertChange | GitRevertFile | GitSearchHistory | GitStageAll | GitStash | GitStashPop | GitVersions => Feature::Git,
        Claude | SuggestCommit | ClaudeAddSelection | ClaudeAddFile | ClaudeResumeFile | ClaudeAsk | ClaudeNextWaiting | GitAiReview | GitExplain | GitCommitRules | GitMyCommitRules => Feature::Claude,
        _ => return None,
    })
}

/// The command is there: its part is on (or it has none).
pub fn cmd_on(cmd: Cmd) -> bool {
    of_cmd(cmd).is_none_or(on)
}

pub fn name(f: Feature) -> &'static str {
    match f {
        Feature::GitHub => "GitHub",
        Feature::Claude => "Claude",
        Feature::Git => "Git",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_follow_their_part() {
        // (the switches themselves are global: other tests run at the same time, they stay on)
        assert_eq!(of_cmd(Cmd::PullRequests), Some(Feature::GitHub));
        assert_eq!(of_cmd(Cmd::ClaudeAsk), Some(Feature::Claude));
        assert_eq!(of_cmd(Cmd::GitPush), Some(Feature::Git));
        assert_eq!(of_cmd(Cmd::Save), None);
        assert!(cmd_on(Cmd::Save));
    }
}
