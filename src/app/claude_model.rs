//! « Modèle d'abord » : before a message starts, Haiku looks for the model that suits it, and the
//! message waits until the user chooses (see `ModelHold`). The tasks of the queue wait the same way.

use super::*;
use super::claude_queue::TaskState;
use crate::claude;

/// A message held before it starts, while the model suited to it is checked (`suggestion` None),
/// or until the choice is made (`suggestion`: the --model value and the reason).
#[derive(Clone)]
pub(super) struct ModelHold {
    pub(super) text: String,
    pub(super) ask: bool,
    pub(super) suggestion: Option<(String, String)>,
    /// A task of the queue: cancelled, it waits again and the queue is paused.
    pub(super) from_queue: bool,
    /// What a task of the queue was given with (the field of the message keeps its own).
    pub(super) attach: Vec<claude_ui::Attach>,
}

/// What the user chooses for a held message.
#[derive(Clone)]
pub(super) enum HoldAct {
    Switch(String),
    Keep,
    Cancel,
}

impl App {
    /// Holds `text` and asks Haiku which model suits it (see `claude::model_check`).
    pub(super) fn claude_check_model(&mut self, text: String, ask: bool, from_queue: bool, attach: Vec<claude_ui::Attach>) {
        let Some(dir) = self.claude.dir.clone() else {
            self.set_status(t!("Aucun dossier ouvert", "No folder open"));
            if from_queue {
                self.claude_queue_failed(t!("Pas pu être envoyée", "Could not be sent"));
            }
            return;
        };
        let id = self.claude.id;
        // the default one: its real name, so that a suggestion of the same family is not made
        let current = if self.claude.model.is_empty() { self.claude.model_id.clone() } else { self.claude.model.clone() };
        let label = self.claude_model_label(&self.claude.model);
        self.claude.model_hold = Some(ModelHold { text: text.clone(), ask, suggestion: None, from_queue, attach });
        self.jobs.spawn(move || JobResult::ClaudeModelCheck(id, claude::model_check(&dir, &text, &current, &label)));
    }

    /// The model checked for the message held of conversation `id`.
    pub(super) fn claude_model_checked(&mut self, id: u64, r: Result<Option<(String, String)>, String>) {
        let Some(k) = self.claude_others.iter().position(|c| c.id == id) else {
            if self.claude.id == id {
                self.claude_model_checked_done(r);
            }
            return;
        };
        // a conversation not shown: made the current one the time to send its message
        std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
        self.claude_model_checked_done(r);
        std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
    }

    fn claude_model_checked_done(&mut self, r: Result<Option<(String, String)>, String>) {
        let Some(hold) = self.claude.model_hold.as_mut() else { return };
        match r {
            Ok(Some(suggestion)) => hold.suggestion = Some(suggestion),
            // the current model suits, or the check failed: the message goes as it is
            Ok(None) => self.claude_hold_act(HoldAct::Keep),
            Err(e) => {
                self.set_status(tf!("Modèle non vérifié ({e}) : le message part avec le modèle choisi", "Model not checked ({e}): the message goes with the model chosen"));
                self.claude_hold_act(HoldAct::Keep);
            }
        }
    }

    /// What the user chose for the held message: the model changed if asked, then it is sent.
    pub(super) fn claude_hold_act(&mut self, act: HoldAct) {
        let Some(hold) = self.claude.model_hold.take() else { return };
        if hold.from_queue && matches!(act, HoldAct::Cancel) {
            let q = &mut self.claude.queue;
            if let Some(k) = q.current() {
                q.tasks[k].state = TaskState::Waiting;
            }
            q.running = false;
            self.set_status(t!("Tâche remise en attente : la file est en pause", "Task waiting again: the queue is paused"));
            return;
        }
        match act.clone() {
            HoldAct::Cancel => {
                self.claude.input = hold.text;
                return;
            }
            HoldAct::Switch(m) if m != self.claude.model => {
                // another model: the process starts again, the conversation goes on (--resume)
                self.claude.model = m;
                self.claude.session = None;
            }
            _ => {}
        }
        if !self.claude_send_given(hold.text.clone(), hold.ask, hold.attach) {
            if hold.from_queue {
                self.claude_queue_failed(t!("Pas pu être envoyée", "Could not be sent"));
            } else {
                self.claude.input = hold.text;
            }
        }
    }

    /// What Florm knows of the project of `dir`, for Claude's system prompt (`claude::florm_context`):
    /// the tests container and command, the containers, the commit rules, the branch.
    pub(super) fn florm_context(&self, dir: &Path) -> String {
        let s = &self.settings;
        let tests = (!s.odoo_test_container.is_empty()).then(|| (s.odoo_test_container.clone(), s.odoo_test_command.clone(), s.odoo_test_history.clone()));
        let repo = self.repo_for(dir).unwrap_or_else(|| dir.to_path_buf());
        let branch = git::head_branch(dir).map(|b| (self.repo_label(&repo), b));
        let ctx = claude::FlormContext {
            tests,
            containers: self.docker_containers(),
            commit_rules: claude::commit_rules(dir),
            repo: branch,
            version: self.version.clone(),
        };
        claude::florm_context(&ctx)
    }

    /// The name of the model `value` (see `claude_ui::model_label`).
    pub(super) fn claude_model_label(&self, value: &str) -> String {
        claude_ui::model_label(&self.claude_commands.models, value)
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

    /// The task of the queue being done failed with `note`: the queue is paused.
    pub(super) fn claude_queue_failed(&mut self, note: &str) {
        let q = &mut self.claude.queue;
        if let Some(k) = q.current() {
            q.tasks[k].state = TaskState::Failed;
            q.tasks[k].note = note.into();
        }
        q.running = false;
    }
}
