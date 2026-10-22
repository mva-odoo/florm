//! A to-do list for Claude: a long request cut into tasks, sent one after the other in the same
//! conversation, each when the one before is finished (and committed, if asked).

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Queue {
    pub(super) tasks: Vec<Task>,
    /// The next task goes when the current one is finished (not kept between sessions).
    #[serde(skip)]
    pub(super) running: bool,
    /// A commit of all the changes after each task, its message written by Claude.
    pub(super) commit_each: bool,
    /// Each task in a conversation of its own (the history is not read again: fewer tokens),
    /// with what the one before did said in a few lines.
    #[serde(default)]
    pub(super) fresh_each: bool,
    /// Model of the conversation when the queue was started: the tasks without a model of their own.
    #[serde(skip)]
    pub(super) base_model: Option<usize>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Task {
    pub(super) text: String,
    pub(super) state: TaskState,
    /// What went wrong, or the first line of its commit.
    #[serde(default)]
    pub(super) note: String,
    /// Model for this task (index in `claude::MODELS`); None: the one of the conversation.
    #[serde(default)]
    pub(super) model: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub(super) enum TaskState {
    Waiting,
    Running,
    Committing,
    Done,
    Failed,
}

/// What the list asked for during the frame.
pub(super) enum QueueAct {
    Start,
    Pause,
    Remove(usize),
    Up(usize),
    /// Waiting again (a failed task).
    Retry(usize),
    ClearDone,
}

/// The tasks of a request: each item of its list ("- …", "* …", "1. …", "2) …", the lines under
/// an item going with it), else the whole text as one task.
pub(super) fn split_tasks(text: &str) -> Vec<String> {
    let item = |l: &str| -> Option<String> {
        let t = l.trim_start();
        if let Some(r) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("• ")) {
            return Some(r.to_string());
        }
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        let rest = &t[digits..];
        (digits > 0 && (rest.starts_with(". ") || rest.starts_with(") "))).then(|| rest[2..].to_string())
    };
    let mut tasks: Vec<String> = Vec::new();
    let mut intro: Vec<&str> = Vec::new();
    for l in text.lines() {
        match (item(l), tasks.last_mut()) {
            (Some(t), _) => tasks.push(t),
            // the lines under an item: part of it
            (None, Some(last)) if !l.trim().is_empty() => {
                last.push('\n');
                last.push_str(l.trim());
            }
            (None, None) => intro.push(l),
            _ => {}
        }
    }
    let tasks: Vec<String> = tasks.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    if tasks.len() < 2 {
        let whole = text.trim();
        return if whole.is_empty() { Vec::new() } else { vec![whole.to_string()] };
    }
    // a sentence before the list ("Dans florm :") is said with each task
    let intro = intro.join("\n").trim().to_string();
    if intro.is_empty() {
        tasks
    } else {
        tasks.into_iter().map(|t| format!("{intro}\n{t}")).collect()
    }
}

impl Queue {
    pub(super) fn add(&mut self, text: &str) -> usize {
        let new = split_tasks(text);
        let n = new.len();
        self.tasks.extend(new.into_iter().map(|text| Task { text, state: TaskState::Waiting, note: String::new(), model: None }));
        n
    }

    /// The task being done or committed.
    pub(super) fn current(&self) -> Option<usize> {
        self.tasks.iter().position(|t| matches!(t.state, TaskState::Running | TaskState::Committing))
    }

    pub(super) fn next_waiting(&self) -> Option<usize> {
        self.tasks.iter().position(|t| t.state == TaskState::Waiting)
    }

    /// After a restart of Florm, nothing is running any more.
    pub(super) fn settle(&mut self) {
        for t in &mut self.tasks {
            if matches!(t.state, TaskState::Running | TaskState::Committing) {
                t.state = TaskState::Waiting;
            }
        }
        self.running = false;
    }
}

/// The list above the field of the chat: the tasks and their state, the buttons of the queue.
pub(super) fn queue_ui(ui: &mut egui::Ui, q: &mut Queue, busy: bool, accent: Color32) -> Option<QueueAct> {
    let mut act = None;
    let done = q.tasks.iter().filter(|t| t.state == TaskState::Done).count();
    let waiting = q.tasks.iter().filter(|t| t.state == TaskState::Waiting).count();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("FILE DE TÂCHES · {done}/{}", q.tasks.len())).small().strong().color(accent));
        if q.running {
            if ui.small_button("Pause").on_hover_text("La tâche en cours se termine, la suivante attend").clicked() {
                act = Some(QueueAct::Pause);
            }
        } else if waiting > 0 {
            let tip = "Envoie la première tâche en attente, puis chaque suivante quand Claude a fini la précédente (même conversation : il garde le contexte)";
            if ui.add_enabled(!busy, filled_button("Lancer", accent)).on_hover_text(tip).on_disabled_hover_text("Claude répond encore").clicked() {
                act = Some(QueueAct::Start);
            }
        }
        ui.checkbox(&mut q.commit_each, RichText::new("Commit après chaque tâche").small())
            .on_hover_text("Quand une tâche est finie : git add -A, message écrit par Claude, git commit, puis la tâche suivante.\nTout ce qui est modifié dans le dépôt part dans le commit.");
        ui.checkbox(&mut q.fresh_each, RichText::new("Conversation neuve par tâche").small()).on_hover_text(
            "Chaque tâche repart d'une conversation vide : Claude ne relit pas tout ce qui a été dit avant (beaucoup moins de tokens sur une longue file).\nLa dernière réponse de la tâche d'avant lui est donnée en quelques lignes.",
        );
        if done > 0 && ui.small_button("Retirer les terminées").clicked() {
            act = Some(QueueAct::ClearDone);
        }
    });
    egui::ScrollArea::vertical().id_salt("claude_queue").max_height(170.0).show(ui, |ui| {
        for (k, t) in q.tasks.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                match t.state {
                    TaskState::Running | TaskState::Committing => {
                        ui.spinner();
                    }
                    TaskState::Done => {
                        ui.label(RichText::new("✔").color(ok()));
                    }
                    TaskState::Failed => {
                        ui.label(RichText::new("⚠").color(err())).on_hover_text(&t.note);
                    }
                    TaskState::Waiting => {
                        ui.label(RichText::new(format!("{}.", k + 1)).weak());
                    }
                }
                let editable = t.state == TaskState::Waiting;
                let w = (ui.available_width() - 190.0).max(120.0);
                if editable {
                    ui.add(egui::TextEdit::multiline(&mut t.text).desired_rows(1).desired_width(w).layouter(&mut layout_cutting));
                } else {
                    let mut text = RichText::new(t.text.lines().next().unwrap_or(""));
                    if t.state == TaskState::Done {
                        text = text.weak();
                    }
                    let tip = if t.note.is_empty() { t.text.clone() } else { format!("{}\n\n{}", t.text, t.note) };
                    ui.add_sized([w, 18.0], egui::Label::new(text).truncate()).on_hover_text(tip);
                }
                if t.state == TaskState::Committing {
                    ui.label(RichText::new("commit…").small().weak());
                }
                if t.state == TaskState::Failed && ui.small_button("Réessayer").clicked() {
                    act = Some(QueueAct::Retry(k));
                }
                if editable {
                    // a smaller model for a small task: cheaper and faster
                    let shown = t.model.map_or("Modèle de la conversation", |m| crate::claude::MODELS[m.min(crate::claude::MODELS.len() - 1)].1);
                    egui::ComboBox::from_id_salt(("task_model", k)).selected_text(RichText::new(shown).small()).width(96.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut t.model, None, "Modèle de la conversation");
                        for (m, (_, label)) in crate::claude::MODELS.iter().enumerate().skip(1) {
                            ui.selectable_value(&mut t.model, Some(m), *label);
                        }
                    });
                    if k > 0 && ui.small_button("↑").on_hover_text("Plus tôt").clicked() {
                        act = Some(QueueAct::Up(k));
                    }
                    if ui.small_button("×").on_hover_text("Retirer").clicked() {
                        act = Some(QueueAct::Remove(k));
                    }
                }
            });
        }
    });
    ui.add_space(4.0);
    act
}

/// `git add -A`, a message written by Claude for `wish`, `git commit`; returns the first line of
/// the message (or says there was nothing to commit).
pub(super) fn commit_task(dir: &Path, wish: &str) -> Result<String, String> {
    git::run(dir, &["add", "-A"])?;
    let diff = git::run(dir, &["diff", "--cached"])?;
    if diff.trim().is_empty() {
        return Ok("rien à commiter".into());
    }
    let recent = git::stdout(dir, &["log", "-12", "--format=%s"]).unwrap_or_default();
    let msg = crate::claude::commit_message(dir, &diff, &recent, wish)?;
    git::run(dir, &["commit", "-q", "-m", &msg])?;
    Ok(msg.lines().next().unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_cut_into_tasks() {
        assert_eq!(split_tasks("corrige le bug du tri"), ["corrige le bug du tri"]);
        assert_eq!(split_tasks("- le 2\n- le 3\n  avec un détail\n\n- le 6"), ["le 2", "le 3\navec un détail", "le 6"]);
        assert_eq!(split_tasks("Dans florm :\n1. le retour à la ligne\n2) le stash"), ["Dans florm :\nle retour à la ligne", "Dans florm :\nle stash"]);
        // one item only: the whole text
        assert_eq!(split_tasks("Fais ceci :\n- une chose"), ["Fais ceci :\n- une chose"]);
        assert!(split_tasks("  \n").is_empty());
        // a queue saved before the models and the fresh conversations
        let old: Queue = serde_json::from_str(r#"{"tasks":[{"text":"a","state":"Waiting"}],"commit_each":true}"#).unwrap();
        assert_eq!((old.tasks[0].model, old.fresh_each, old.commit_each), (None, false, true));
        let mut q = Queue::default();
        assert_eq!(q.add("- a\n- b"), 2);
        q.tasks[0].state = TaskState::Running;
        assert_eq!((q.current(), q.next_waiting()), (Some(0), Some(1)));
        q.settle();
        assert_eq!((q.current(), q.next_waiting()), (None, Some(0)));
    }
}
