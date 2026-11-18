//! The models of Claude Code (their names, asked once a session) and what Florm tells Claude of the
//! project when a conversation starts.

use super::*;
use crate::claude;

impl App {
    /// What Florm knows of the project of `dir`, for Claude's system prompt (`claude::florm_context`):
    /// the tests container and command, the containers, the commit rules, the branch.
    pub(super) fn florm_context(&self, dir: &Path) -> String {
        let repo = self.repo_for(dir).unwrap_or_else(|| dir.to_path_buf());
        let branch = git::head_branch(dir).map(|b| (self.repo_label(&repo), b));
        let ctx = claude::FlormContext {
            commit_rules: claude::commit_rules(dir),
            repo: branch,
            version: self.version.clone(),
        };
        claude::florm_context(&ctx)
    }

    /// Asks Claude Code once per session of Florm which models it offers (kept on disk with its
    /// commands: the list of the last time is shown meanwhile).
    pub(super) fn claude_ask_models(&mut self) {
        if self.claude_models_asked {
            return;
        }
        self.claude_models_asked = true;
        let dir = self.claude.dir.clone().or_else(|| self.root.clone()).unwrap_or_else(std::env::temp_dir);
        let account = self.claude_account_dir(&self.claude.account);
        self.jobs.spawn(move || JobResult::ClaudeModels(claude::list_models(&dir, account.as_deref())));
    }

    /// The models given by Claude Code.
    pub(super) fn claude_models_listed(&mut self, r: Result<Vec<claude::ModelInfo>, String>) {
        // not installed, offline…: the list of the last time (or the aliases) stays
        let Ok(models) = r else { return };
        if models != self.claude_commands.models {
            self.claude_commands.models = models;
            claude_ui::write_commands(&self.claude_commands);
        }
    }

}
