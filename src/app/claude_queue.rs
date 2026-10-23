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
    /// Model of the tasks without one of their own (index in `claude::MODELS`); None: the one of
    /// the conversation.
    #[serde(default)]
    pub(super) model: Option<usize>,
    /// Model of the conversation when the queue was started: what the tasks use when no model is chosen.
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
    /// Model for this task (index in `claude::MODELS`); None: the one of the queue.
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
    /// The list closed: all its tasks go.
    Close,
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

    /// The model task `k` is sent with: its own, else the one of the queue, else the one of the conversation.
    pub(super) fn model_of(&self, k: usize) -> Option<usize> {
        self.tasks[k].model.or(self.model).or(self.base_model)
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
pub(super) fn queue_ui(ui: &mut egui::Ui, q: &mut Queue, busy: bool, accent: Color32, id: egui::Id) -> Option<QueueAct> {
    let mut act = None;
    let done = q.tasks.iter().filter(|t| t.state == TaskState::Done).count();
    let waiting = q.tasks.iter().filter(|t| t.state == TaskState::Waiting).count();
    let working = q.current().is_some();
    // wrapped: a narrow panel stays narrow, the buttons go on a second line
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(tf!("FILE DE TÂCHES · {done}/{}", "TASK QUEUE · {done}/{}", q.tasks.len())).small().strong().color(accent));
        if done == q.tasks.len() {
            ui.label(RichText::new(t!("✔ terminée", "✔ done")).small().color(ok()));
        }
        if q.running {
            if ui.small_button("Pause").on_hover_text(t!("La tâche en cours se termine, la suivante attend", "The current task finishes, the next one waits")).clicked() {
                act = Some(QueueAct::Pause);
            }
        } else if waiting > 0 {
            let tip = t!("Envoie la première tâche en attente, puis chaque suivante quand Claude a fini la précédente (même conversation : il garde le contexte)", "Sends the first waiting task, then each next one when Claude is done with the previous one (same conversation: it keeps the context)");
            if ui.add_enabled(!busy, filled_button(t!("Lancer", "Run"), accent)).on_hover_text(tip).on_disabled_hover_text(t!("Claude répond encore", "Claude is still answering")).clicked() {
                act = Some(QueueAct::Start);
            }
        }
        // the model of the tasks without one of their own: a smaller one for small tasks is cheaper and faster
        let shown = q.model.map_or(t!("Modèle de la conversation", "Conversation's model"), model_label);
        egui::ComboBox::from_id_salt(id).selected_text(RichText::new(shown).small()).width(120.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut q.model, None, t!("Modèle de la conversation", "Conversation's model"));
            for (m, (_, label)) in crate::claude::MODELS.iter().enumerate().skip(1) {
                ui.selectable_value(&mut q.model, Some(m), crate::claude::label(label));
            }
        });
        ui.checkbox(&mut q.commit_each, RichText::new(t!("Commit après chaque tâche", "Commit after each task")).small())
            .on_hover_text(t!("Quand une tâche est finie : git add -A, message écrit par Claude, git commit, puis la tâche suivante.\nTout ce qui est modifié dans le dépôt part dans le commit.", "When a task is done: git add -A, message written by Claude, git commit, then the next task.\nEverything changed in the repository goes in the commit."));
        ui.checkbox(&mut q.fresh_each, RichText::new(t!("Conversation neuve par tâche", "New conversation per task")).small()).on_hover_text(
            t!("Chaque tâche repart d'une conversation vide : Claude ne relit pas tout ce qui a été dit avant (beaucoup moins de tokens sur une longue file).\nLa dernière réponse de la tâche d'avant lui est donnée en quelques lignes.", "Each task starts from an empty conversation: Claude does not read again all that was said before (far fewer tokens on a long queue).\nThe last answer of the previous task is given to it in a few lines."),
        );
        if done > 0 && ui.small_button(t!("Retirer les terminées", "Remove the finished ones")).clicked() {
            act = Some(QueueAct::ClearDone);
        }
        // closing the list: its tasks go, not while one of them runs
        let close = ui.add_enabled(!q.running && !working, egui::Button::new("×").small());
        if close.on_hover_text(t!("Fermer la file : retire toutes ses tâches", "Close the queue: removes all its tasks")).on_disabled_hover_text(t!("Une tâche tourne : Pause, puis attendre qu'elle finisse", "A task is running: Pause, then wait for it to finish")).clicked() {
            act = Some(QueueAct::Close);
        }
    });
    let queue_model = q.model.map_or(t!("Modèle de la conversation", "Conversation's model"), model_label);
    // the sentence before the list, said with each task: shown once, above them
    let intro = shared_intro(&q.tasks);
    if !intro.is_empty() {
        ui.label(RichText::new(intro.trim()).small().weak());
    }
    // the finished tasks folded in one line: a finished queue takes a line, not the whole list
    let show_done_id = id.with("show_done");
    let mut show_done = ui.data(|d| d.get_temp::<bool>(show_done_id)).unwrap_or(false);
    if done > 0 {
        let arrow = if show_done { "⏷" } else { "⏵" };
        let line = RichText::new(tf!("{arrow} ✔ {done} terminée(s)", "{arrow} ✔ {done} done")).small().color(ok());
        if ui.add(egui::Label::new(line).sense(egui::Sense::click())).on_hover_text(t!("Afficher ou masquer les tâches terminées", "Show or hide the finished tasks")).clicked() {
            show_done = !show_done;
            ui.data_mut(|d| d.insert_temp(show_done_id, show_done));
        }
    }
    let states: Vec<TaskState> = q.tasks.iter().map(|t| t.state).collect();
    let row_fill = icons::mix(ui.visuals().panel_fill, accent, 0.10);
    let mut number = 0;
    // the whole width, whatever the width of the tasks shown the frame before
    egui::ScrollArea::vertical().id_salt("claude_queue").auto_shrink([false, true]).max_height(170.0).show(ui, |ui| {
        for (k, t) in q.tasks.iter_mut().enumerate() {
            if t.state == TaskState::Done && !show_done {
                continue;
            }
            if t.state != TaskState::Done {
                number += 1;
            }
            let running = matches!(t.state, TaskState::Running | TaskState::Committing);
            let frame = egui::Frame::none().inner_margin(egui::Margin::symmetric(4.0, 2.0)).rounding(4.0);
            frame.fill(if running { row_fill } else { Color32::TRANSPARENT }).show(ui, |ui| ui.horizontal(|ui| {
                // a column of its own for the state: the texts start at the same place
                ui.allocate_ui_with_layout(egui::vec2(22.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_width(22.0);
                    match t.state {
                        TaskState::Running | TaskState::Committing => {
                            spinner(ui);
                        }
                        TaskState::Done => {
                            ui.label(RichText::new("✔").color(ok()));
                        }
                        TaskState::Failed => {
                            ui.label(RichText::new("⚠").color(err())).on_hover_text(&t.note);
                        }
                        TaskState::Waiting => {
                            ui.label(RichText::new(format!("{number}.")).weak());
                        }
                    }
                });
                let editable = t.state == TaskState::Waiting;
                // room for the buttons of the row, on the right
                let buttons = match t.state {
                    TaskState::Waiting => 170.0,
                    TaskState::Failed => 110.0,
                    TaskState::Committing => 70.0,
                    TaskState::Running => 10.0,
                    TaskState::Done => 30.0,
                };
                let w = (ui.available_width() - buttons).max(80.0);
                let body = &t.text[intro.len()..];
                if editable {
                    let mut edited = body.to_string();
                    if ui.add(egui::TextEdit::multiline(&mut edited).desired_rows(1).desired_width(w).layouter(&mut layout_cutting)).changed() {
                        t.text = format!("{intro}{edited}");
                    }
                } else {
                    let tip = if t.note.is_empty() { t.text.clone() } else { format!("{}\n\n{}", t.text, t.note) };
                    ui.allocate_ui_with_layout(egui::vec2(w, 18.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(w);
                        if t.state == TaskState::Done {
                            // done: its first line is enough, the rest on hover
                            let first = body.trim().lines().next().unwrap_or("");
                            ui.add(egui::Label::new(RichText::new(first).weak()).truncate()).on_hover_text(tip);
                        } else {
                            ui.add(egui::Label::new(RichText::new(body.trim())).wrap()).on_hover_text(tip);
                        }
                    });
                }
                if t.state == TaskState::Committing {
                    ui.label(RichText::new("commit…").small().weak());
                }
                if t.state == TaskState::Failed && ui.small_button(t!("Réessayer", "Retry")).clicked() {
                    act = Some(QueueAct::Retry(k));
                }
                if editable {
                    // a model for this task only: Haiku for a small one in a list sent with Opus
                    let shown = t.model.map_or(t!("Modèle de la file", "Queue's model"), model_label);
                    egui::ComboBox::from_id_salt(id.with(("task_model", k))).selected_text(RichText::new(shown).small()).width(96.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut t.model, None, t!("Modèle de la file", "Queue's model")).on_hover_text(queue_model);
                        for (m, (_, label)) in crate::claude::MODELS.iter().enumerate().skip(1) {
                            ui.selectable_value(&mut t.model, Some(m), crate::claude::label(label));
                        }
                    });
                }
                // above the waiting one before it (a finished one, folded, would not show it moved)
                if editable && k > 0 && states[k - 1] == TaskState::Waiting && ui.small_button("↑").on_hover_text(t!("Plus tôt", "Earlier")).clicked() {
                    act = Some(QueueAct::Up(k));
                }
                // any task can go, the one running excepted
                if !matches!(t.state, TaskState::Running | TaskState::Committing) && ui.small_button("×").on_hover_text(t!("Retirer", "Remove")).clicked() {
                    act = Some(QueueAct::Remove(k));
                }
            }));
        }
    });
    ui.add_space(4.0);
    act
}

/// The lines all the tasks begin with (the sentence before the list, said with each by
/// `split_tasks`), each task having more after them; "" with fewer than two tasks.
fn shared_intro(tasks: &[Task]) -> String {
    let [first, rest @ ..] = tasks else { return String::new() };
    if rest.is_empty() {
        return String::new();
    }
    let mut end = 0;
    for (i, _) in first.text.match_indices('\n') {
        let p = &first.text[..=i];
        if !tasks.iter().all(|t| t.text.starts_with(p) && !t.text[p.len()..].trim().is_empty()) {
            break;
        }
        end = i + 1;
    }
    first.text[..end].to_string()
}

fn model_label(m: usize) -> &'static str {
    crate::claude::label(crate::claude::MODELS[m.min(crate::claude::MODELS.len() - 1)].1)
}

/// `git add -A`, a message written by Claude for `wish`, `git commit`; returns the first line of
/// the message (or says there was nothing to commit).
pub(super) fn commit_task(dir: &Path, wish: &str) -> Result<String, String> {
    git::run(dir, &["add", "-A"])?;
    let diff = git::run(dir, &["diff", "--cached"])?;
    if diff.trim().is_empty() {
        return Ok(t!("rien à commiter", "nothing to commit").into());
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
        assert_eq!((old.model, old.tasks[0].model, old.fresh_each, old.commit_each), (None, None, false, true));
        let mut q = Queue::default();
        assert_eq!(q.add("- a\n- b"), 2);
        q.tasks[0].state = TaskState::Running;
        assert_eq!((q.current(), q.next_waiting()), (Some(0), Some(1)));
        q.settle();
        assert_eq!((q.current(), q.next_waiting()), (None, Some(0)));
        // the model of a task: its own, else the queue's, else the conversation's
        q.base_model = Some(1);
        assert_eq!(q.model_of(0), Some(1));
        q.model = Some(2);
        q.tasks[1].model = Some(3);
        assert_eq!((q.model_of(0), q.model_of(1)), (Some(2), Some(3)));
        // the sentence before the list, shown once above the tasks
        let mut q = Queue::default();
        q.add("Dans florm :\n- a\n- b");
        assert_eq!(shared_intro(&q.tasks), "Dans florm :\n");
        q.add("c");
        assert_eq!(shared_intro(&q.tasks), "");
        let mut q = Queue::default();
        q.add("seule tâche\navec un détail");
        assert_eq!(shared_intro(&q.tasks), "");
    }
}
