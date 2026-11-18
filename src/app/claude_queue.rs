//! Messages waiting for Claude: written while it answers (⏭ Après, Ctrl+Maj+Entrée), each sent
//! whole, with what was given with it, when it has answered the one before; shown above the
//! field until it leaves.

use super::*;
use super::claude_ui::Attach;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Queue {
    pub(super) tasks: Vec<Task>,
    /// The next message goes when Claude is free (not kept between sessions).
    #[serde(skip)]
    pub(super) running: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Task {
    pub(super) text: String,
    pub(super) state: TaskState,
    /// What went wrong.
    #[serde(default)]
    pub(super) note: String,
    /// What was given with the message (images, files, texts) when it was written: sent with it.
    #[serde(default)]
    pub(super) attach: Vec<Attach>,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub(super) enum TaskState {
    Waiting,
    /// (`Committing`: a commit after a task, of the task queue of before.)
    #[serde(alias = "Committing")]
    Running,
    /// Sent and answered (kept by the task queue of before; dropped when loaded).
    Done,
    Failed,
}

/// What the list asked for during the frame.
pub(super) enum QueueAct {
    Remove(usize),
    /// Waiting again (one that could not be sent), and the messages go on.
    Retry(usize),
    /// The list closed: all its messages go.
    Close,
}

impl Queue {
    /// A message written while Claude answers, with what was given with it.
    pub(super) fn push_message(&mut self, text: &str, attach: Vec<Attach>) {
        self.tasks.push(Task { text: text.to_string(), state: TaskState::Waiting, note: String::new(), attach });
    }

    /// The message being sent.
    pub(super) fn current(&self) -> Option<usize> {
        self.tasks.iter().position(|t| t.state == TaskState::Running)
    }

    pub(super) fn next_waiting(&self) -> Option<usize> {
        self.tasks.iter().position(|t| t.state == TaskState::Waiting)
    }

    /// After a restart of Florm, nothing is being sent any more (the ones answered go).
    pub(super) fn settle(&mut self) {
        self.tasks.retain(|t| t.state != TaskState::Done);
        for t in &mut self.tasks {
            if t.state == TaskState::Running {
                t.state = TaskState::Waiting;
            }
        }
        self.running = false;
    }
}

/// Width of the column of the buttons at the end of a row (the same for every row).
const ROW_BUTTONS_W: f32 = 90.0;

/// The list above the field of the chat: the messages waiting, their state, × to drop them.
pub(super) fn queue_ui(ui: &mut egui::Ui, q: &mut Queue, accent: Color32) -> Option<QueueAct> {
    let mut act = None;
    let sending = q.current().is_some();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(tf!("⏭ EN ATTENTE · {}", "⏭ WAITING · {}", q.tasks.len())).small().strong().color(accent))
            .on_hover_text(t!("Chacun part quand Claude a répondu au précédent", "Each one goes when Claude has answered the one before"));
        let close = ui.add_enabled(!sending, egui::Button::new("×").small());
        if close.on_hover_text(t!("Retirer tous les messages en attente", "Remove all the waiting messages")).on_disabled_hover_text(t!("Un message part en ce moment", "A message is being sent")).clicked() {
            act = Some(QueueAct::Close);
        }
    });
    let row_fill = icons::mix(ui.visuals().panel_fill, accent, 0.10);
    let mut number = 0;
    egui::ScrollArea::vertical().id_salt("claude_queue").auto_shrink([false, true]).max_height(170.0).show(ui, |ui| {
        for (k, t) in q.tasks.iter_mut().enumerate() {
            number += 1;
            let running = t.state == TaskState::Running;
            let frame = egui::Frame::none().inner_margin(egui::Margin::symmetric(4.0, 2.0)).rounding(4.0);
            frame.fill(if running { row_fill } else { Color32::TRANSPARENT }).show(ui, |ui| ui.horizontal(|ui| {
                // a column of its own for the state: the texts start at the same place
                ui.allocate_ui_with_layout(egui::vec2(22.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_width(22.0);
                    match t.state {
                        TaskState::Running => {
                            spinner(ui);
                        }
                        TaskState::Failed => {
                            ui.label(RichText::new("⚠").color(err())).on_hover_text(&t.note);
                        }
                        _ => {
                            ui.label(RichText::new(format!("{number}.")).weak());
                        }
                    }
                });
                let w = (ui.available_width() - ROW_BUTTONS_W).max(80.0);
                // what goes with it (an image…), under its text: it left the field of the message
                let given = (!t.attach.is_empty()).then(|| format!("✳ {}", t.attach.iter().map(|a| a.label(None)).collect::<Vec<_>>().join(", ")));
                ui.allocate_ui_with_layout(egui::vec2(w, 18.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(w);
                    // still waiting: it can be changed before it goes
                    if t.state == TaskState::Waiting {
                        ui.add(egui::TextEdit::multiline(&mut t.text).desired_rows(1).desired_width(w).layouter(&mut layout_cutting));
                    } else {
                        let tip = if t.note.is_empty() { t.text.clone() } else { format!("{}\n\n{}", t.text, t.note) };
                        ui.add(egui::Label::new(RichText::new(t.text.trim())).wrap()).on_hover_text(tip);
                    }
                    if let Some(g) = &given {
                        ui.add(egui::Label::new(RichText::new(g).small().weak()).truncate());
                    }
                });
                ui.allocate_ui_with_layout(egui::vec2(ROW_BUTTONS_W, 18.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !running && ui.small_button("×").on_hover_text(t!("Retirer", "Remove")).clicked() {
                        act = Some(QueueAct::Remove(k));
                    }
                    if t.state == TaskState::Failed && ui.small_button(t!("Réessayer", "Retry")).clicked() {
                        act = Some(QueueAct::Retry(k));
                    }
                });
            }));
        }
    });
    ui.add_space(4.0);
    act
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_waiting_for_claude() {
        let mut q = Queue::default();
        q.push_message("Fais ceci :\n- a\n- b", Vec::new());
        q.push_message("puis cela", Vec::new());
        // a list stays one message
        assert_eq!(q.tasks.len(), 2);
        q.tasks[0].state = TaskState::Running;
        assert_eq!((q.current(), q.next_waiting()), (Some(0), Some(1)));
        q.settle();
        assert_eq!((q.current(), q.next_waiting()), (None, Some(0)));
        // a queue of tasks of before: its fields of then are left, the ones done go, the
        // commit being made waits again
        let old: Queue = serde_json::from_str(r#"{"tasks":[{"text":"a","state":"Done"},{"text":"b","state":"Committing","model":"opus","plain":false}],"commit_each":true,"model":1}"#).unwrap();
        let mut old = old;
        old.settle();
        assert_eq!(old.tasks.len(), 1);
        assert_eq!((old.tasks[0].text.as_str(), old.tasks[0].state), ("b", TaskState::Waiting));
    }
}
