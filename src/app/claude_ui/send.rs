//! Sending: a message, a slash command, the queue of tasks run one after the other.

use super::*;

impl App {
    /// The message written in the field of the chat. While Claude works it goes into the answer
    /// being written, or, when `later`, waits in the queue until the answer is done.
    pub(super) fn claude_send(&mut self, later: bool) {
        let text = self.claude.input.trim().to_string();
        // « # note »: to remember (CLAUDE.md), not a message
        if let Some(note) = text.strip_prefix('#').filter(|n| !n.starts_with('#') && !n.trim().is_empty()) {
            self.claude.memory = Some(note.trim().to_string());
            self.claude.input.clear();
            return;
        }
        if let Some(clear) = text.starts_with('/').then(|| self.claude_command(&text)).flatten() {
            if clear {
                self.claude.input.clear();
            }
            return;
        }
        if later && self.claude.busy && !text.is_empty() {
            self.claude.queue.push_message(&text);
            self.claude.queue.running = true;
            self.claude.input.clear();
            self.set_status(t!("Message ajouté à la file : il partira quand Claude aura fini", "Message queued: it goes when Claude is done"));
            self.claude_save();
            return;
        }
        // the model of a message is being chosen: this one waits in the queue, after the answer
        if self.claude.model_hold.is_some() && !text.is_empty() {
            self.claude.queue.push_message(&text);
            self.claude.queue.running = true;
            self.claude.input.clear();
            self.set_status(t!("Message ajouté à la file : il partira après le choix du modèle et la réponse", "Message queued: it goes after the model choice and the answer"));
            self.claude_save();
            return;
        }
        let ask = self.claude.ask_first;
        // a message starting work: the model suited to it is checked first, when the option is on
        if self.claude.model_first && !self.claude.busy && !text.is_empty() {
            self.claude_check_model(text, ask, false);
            self.claude.input.clear();
            return;
        }
        if self.claude_send_text(text, ask) {
            self.claude.input.clear();
        }
    }

    /// The slash commands Florm answers itself (`/btw`, `/status`) or runs in a terminal (those
    /// of the terminal interface of Claude Code), and those that wait for Claude to be free.
    /// None: to send as a message; else whether the field is emptied.
    pub(super) fn claude_command(&mut self, text: &str) -> Option<bool> {
        let (name, rest) = text[1..].split_once(char::is_whitespace).map_or((&text[1..], ""), |(n, r)| (n, r.trim()));
        let dir = self.claude.dir.clone()?;
        let c = &self.claude;
        match name {
            "btw" if rest.is_empty() => {
                self.set_status(t!("/btw suivi de la question", "/btw followed by the question"));
                Some(false)
            }
            "btw" => {
                let (session, model, id, question) = (c.session_id.clone(), claude::MODELS[c.model].0.to_string(), c.id, rest.to_string());
                self.claude.side = Some((format!("/btw {rest}"), None));
                self.jobs.spawn(move || JobResult::ClaudeBtw(id, claude::btw(&dir, session.as_deref(), &model, &question)));
                Some(true)
            }
            "status" => {
                let version = &self.claude_commands.version;
                let line = |k: &str, v: &str| format!("- **{k}** : {v}\n");
                let mut s = String::new();
                s += &line("Claude Code", if version.is_empty() { "?" } else { version });
                s += &line(t!("Modèle", "Model"), claude::MODELS[c.model].1);
                s += &line(t!("Mode", "Mode"), claude::MODES[c.mode].1);
                s += &line(t!("Permissions", "Permissions"), &self.settings.claude_perms.join(", "));
                s += &line(t!("Dossier", "Folder"), &dir.display().to_string());
                s += &line("Session", c.session_id.as_deref().unwrap_or(t!("pas encore commencée", "not started yet")));
                s += &line(t!("Processus", "Process"), if c.session.is_some() { t!("en cours", "running") } else { t!("arrêté (le prochain message le relance)", "stopped (the next message starts it)") });
                if c.context.1 > 0 {
                    s += &line(t!("Contexte", "Context"), &format!("{} / {} tokens", thousands(c.context.0), thousands(c.context.1)));
                }
                s += &line(t!("Coût de la conversation", "Cost of the conversation"), &format!("{:.2} $", c.cost));
                self.claude.side = Some(("/status".into(), Some(Ok(s))));
                Some(true)
            }
            "rewind" => {
                let last = c.checkpoints.iter().rev().find(|p| matches!(c.items.get(p.item), Some(Item::User(..)))).map(|p| p.item);
                match last {
                    _ if c.busy => self.set_status(t!("Claude travaille : arrête-le d'abord (■ Arrêter)", "Claude is working: stop it first (■ Stop)")),
                    Some(k) => {
                        self.claude.confirm_rewind = Some(k);
                        self.claude.folded.remove(&k);
                    }
                    None => self.set_status(t!("Aucun message où revenir", "No message to go back to")),
                }
                Some(true)
            }
            "mcp" => {
                let id = c.id;
                self.claude.side = Some(("/mcp".into(), None));
                self.claude.tools.mcp_busy = true;
                self.jobs.spawn(move || JobResult::ClaudeMcp(id, true, claude::mcp_list(&dir)));
                Some(true)
            }
            "agents" => {
                self.claude_tool_act(ToolAct::ReadLists);
                let list: String = self.claude.tools.agents.iter().map(|(n, d, p)| format!("- **{n}** — {d} — `{}`\n", p.display())).collect();
                let text = if list.is_empty() { t!("Aucun sous-agent. Pour en créer un : ⋯ → Agents.", "No subagent. To create one: ⋯ → Agents.").to_string() } else { list };
                self.claude.side = Some(("/agents".into(), Some(Ok(text))));
                Some(true)
            }
            "permissions" => {
                let line = |k: &str, v: &str| format!("- **{k}** : {v}\n");
                let none = t!("aucune", "none");
                let join = |l: &[String]| if l.is_empty() { none.to_string() } else { l.iter().map(|r| format!("`{r}`")).collect::<Vec<_>>().join(", ") };
                let mut s = line(t!("Mode", "Mode"), claude::label(claude::MODES[c.mode].1));
                s += &line(t!("Cases", "Boxes"), &self.settings.claude_perms.join(", "));
                s += &line(t!("Règles autorisées", "Allowed rules"), &join(&self.settings.claude_allow));
                s += &line(t!("Règles interdites", "Refused rules"), &join(&self.settings.claude_deny));
                s += t!("\nPour les changer : 🔒 Permissions → Règles.", "\nTo change them: 🔒 Permissions → Rules.");
                self.claude.side = Some(("/permissions".into(), Some(Ok(s))));
                Some(true)
            }
            "memory" => {
                self.claude_open_file(dir.join("CLAUDE.md"), "");
                Some(true)
            }
            "add-dir" => {
                let p = PathBuf::from(rest);
                let p = if p.is_absolute() { p } else { dir.join(p) };
                if rest.is_empty() || !p.is_dir() {
                    self.set_status(t!("/add-dir suivi d'un dossier qui existe", "/add-dir followed by a folder that exists"));
                    return Some(false);
                }
                self.claude_tool_act(ToolAct::AddDir(p.clone()));
                self.set_status(tf!("Claude peut aussi utiliser {} (dès le prochain message)", "Claude may also use {} (from the next message on)", p.display()));
                Some(true)
            }
            _ if self.claude_commands.terminal.iter().any(|t| t == name) => {
                let ctx = self.egui_ctx.clone();
                match self.term.open_command_panel(&ctx, &["claude".to_string(), text.to_string()], Some(dir)) {
                    Ok(id) => {
                        self.term.set_name(id, format!("claude /{name}"));
                        self.bottom = Some(BottomTab::Terminal);
                    }
                    Err(e) => self.set_error(tf!("Terminal impossible à ouvrir : {e}", "Cannot open a terminal: {e}")),
                }
                Some(true)
            }
            _ if c.busy => {
                self.set_status(t!("Claude travaille : la commande partira quand il aura fini (/btw répond tout de suite)", "Claude is working: send the command once it is done (/btw answers at once)"));
                Some(false)
            }
            _ => None,
        }
    }

    /// The answer of a `/btw` of conversation `id`.
    pub(in crate::app) fn claude_btw_loaded(&mut self, id: u64, r: Result<String, String>) {
        let Some(c) = std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).find(|c| c.id == id) else { return };
        if let Some((_, answer @ None)) = &mut c.side {
            *answer = Some(r);
        }
    }

    /// Sends `text` (with the context given) to Claude, asking it for its questions first when
    /// `ask_first`; false when it could not be sent. A slash command (`/usage`…) goes alone: Claude
    /// Code reads it only at the start of the message. While Claude works, the message goes into
    /// the answer being written (Claude Code reads it between two steps).
    pub(in crate::app) fn claude_send_text(&mut self, text: String, ask_first: bool) -> bool {
        let Some(dir) = self.claude.dir.clone() else {
            self.set_status(t!("Aucun dossier ouvert", "No folder open"));
            return false;
        };
        // no text: the context alone (a pasted text…) is the message
        if text.is_empty() && self.claude.attach.is_empty() {
            return false;
        }
        let command = text.starts_with('/');
        let during = self.claude.busy;
        if during && (command || self.claude.session.is_none()) {
            return false;
        }
        let ask_first = ask_first && !during;
        // the model, the mode or the permissions changed while it ran: started again with them
        if self.claude.restart && !during {
            self.claude.session = None;
            self.claude.restart = false;
        }
        // the files cited with @ go with the message
        if !command {
            for p in super::claude_tools::mentioned_paths(&text, &dir) {
                if !self.claude.attach.iter().any(|a| a.path == p && a.lines.is_none() && a.note.is_none()) {
                    self.claude.attach.push(Attach { path: p, lines: None, note: None, image: None });
                }
            }
        }
        // the review starts with the first message: everything changed since is Claude's
        if self.claude.review_index.is_none() && !command {
            self.claude_start_review();
        }
        let mut message = text.clone();
        // a command leaves the files given for the next message
        let attach = if command { Vec::new() } else { std::mem::take(&mut self.claude.attach) };
        let mut given: Vec<String> = attach.iter().map(|a| a.label(Some(&dir))).collect();
        // the texts given (a pasted text…) stay readable in the message sent
        let notes: Vec<(String, String)> = attach.iter().filter_map(|a| a.note.clone()).collect();
        let image_files: Vec<(String, PathBuf)> = attach.iter().filter_map(|a| a.image.clone()).collect();
        if !attach.is_empty() {
            let parts: Vec<String> = attach.iter().map(|a| a.prompt(Some(&dir))).collect();
            message = format!("{}\n\n{message}", parts.join("\n\n"));
        }
        if self.claude.with_context && !command {
            if let Some((ctx, rel)) = self.editor_context(&dir) {
                message = format!("{ctx}\n\n{message}");
                // the file is given too: « reprendre la conversation de ce fichier » finds it
                if !given.contains(&rel) {
                    given.insert(0, rel);
                }
            }
        }
        // the classes and functions named in the message, found in the index: Claude does not
        // have to search for them
        if !self.claude.no_defs && !command {
            if let Some((defs, names)) = self.cited_definitions(&text, &dir) {
                message = format!("{message}\n\n{defs}");
                given.push(tf!("définitions : {}", "definitions: {}", names.join(", ")));
            }
        }
        if ask_first && !command {
            message = format!("{message}\n\n{}", claude_ask::instruction());
        }
        if self.claude.session.is_none() {
            let model = claude::MODELS[self.claude.model].0;
            let s = &self.settings;
            let perms = claude::permission_args(self.claude.mode, &s.claude_perms, &s.claude_allow, &s.claude_deny, s.claude_budget);
            let ctx = self.egui_ctx.clone();
            // a conversation of before (restart of Florm, process stopped): continued with its
            // context; after going back, from the message gone back to
            let resume = self.claude.session_id.clone();
            let at = self.claude.resume_at.clone().filter(|_| resume.is_some());
            self.claude.started = false;
            match claude::Session::start(&dir, model, &perms, resume.as_deref(), at.as_deref(), &self.claude.add_dirs, move || ctx.request_repaint()) {
                Ok(s) => {
                    self.claude.session = Some(s);
                    self.claude.resume_at = None;
                }
                Err(e) => {
                    self.claude.items.push(Item::Error(e));
                    self.claude.attach = attach;
                    return false;
                }
            }
        }
        // the files as they are before the message starts work: to see its changes, to go back
        let at = self.claude.items.len() - usize::from(matches!(self.claude.items.last(), Some(Item::Streaming(_))));
        let checkpoint = (!during && !command).then(|| self.claude_checkpoint(at));
        let Some(s) = self.claude.session.as_mut() else { return false };
        let images: Vec<Vec<u8>> = match image_files.iter().map(|(_, p)| std::fs::read(p)).collect() {
            Ok(images) => images,
            Err(e) => {
                self.set_error(tf!("Image jointe illisible : {e}", "Attached image unreadable: {e}"));
                self.claude.attach = attach;
                return false;
            }
        };
        let sent = if images.is_empty() { s.send(&message) } else { s.send_with_images(&message, &images) };
        match sent {
            Ok(()) => {
                // what goes with the message besides the text itself: the file, the definitions…
                let extra = message.len().saturating_sub(text.len());
                if extra > 0 && !during {
                    self.set_status(tf!("Contexte joint au message : environ {} Ko", "Context joined to the message: about {} KB", extra.div_ceil(1000)));
                }
                let items = &mut self.claude.items;
                // the text being written stays last: what follows of it is added to it
                items.insert(at, Item::User(text.clone(), given, notes, image_files));
                if let Some(cp) = checkpoint {
                    self.claude.checkpoints.push(cp);
                }
                if during {
                    self.set_status(t!("Message envoyé : Claude le prend en compte dans le travail en cours", "Message sent: Claude takes it into account in the work going on"));
                }
                // the conversation goes down to the message just sent, even if it was read up before
                let chat_id = egui::Id::new("claude_chat").with(self.claude.id);
                self.egui_ctx.data_mut(|d| d.insert_temp(egui::Id::new(("claude_to_end", chat_id)), true));
                let first = self.claude.wish.is_none() && !command;
                if !command {
                    self.claude.wish.get_or_insert(text);
                }
                self.claude.busy = true;
                // its tab is named after the first message
                if let (true, Some(uid)) = (first, self.claude.tab) {
                    let title = self.claude_title();
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                        d.title_override = Some(title);
                    }
                }
                self.claude_save();
                true
            }
            Err(e) => {
                self.claude.session = None;
                self.claude.items.push(Item::Error(e));
                self.claude.attach = attach;
                false
            }
        }
    }

    /// The task of the queue being done is finished: committed if asked, then the next one.
    pub(super) fn claude_task_finished(&mut self, r: Result<(), String>) {
        let q = &mut self.claude.queue;
        let Some(k) = q.current().filter(|k| q.tasks[*k].state == TaskState::Running) else {
            // an answer of the chat, not of a task: the messages waiting in the queue go on
            self.claude_queue_next();
            return;
        };
        // Claude asks questions: the task goes on once they are answered
        let asks = {
            let items = &self.claude.items;
            let last_user = items.iter().rposition(|i| matches!(i, Item::User(..))).unwrap_or(0);
            items[last_user..].iter().any(|i| matches!(i, Item::Text(t) if !claude_ask::parse(t).1.is_empty()))
        };
        match r {
            Err(e) => {
                q.tasks[k].state = TaskState::Failed;
                q.tasks[k].note = e;
                q.running = false;
                self.set_error(t!("File de tâches en pause : une tâche a échoué", "Task queue paused: a task failed"));
            }
            Ok(()) if asks => self.set_status(t!("Claude pose des questions : la file reprend après tes réponses", "Claude is asking questions: the queue resumes after your answers")),
            Ok(()) if q.commit_each && !q.tasks[k].plain => {
                q.tasks[k].state = TaskState::Committing;
                let (dir, wish, id) = (self.claude.dir.clone(), q.tasks[k].text.clone(), self.claude.id);
                let Some(dir) = dir else { return };
                self.jobs.spawn(move || JobResult::QueueCommit(id, claude_queue::commit_task(&dir, &wish)));
            }
            Ok(()) => {
                q.tasks[k].state = TaskState::Done;
                self.claude_queue_next();
            }
        }
    }

    /// The commit after a task of conversation `id` is made: the next task goes.
    pub(in crate::app) fn queue_commit_loaded(&mut self, id: u64, r: Result<String, String>) {
        let Some(k) = self.claude_others.iter().position(|c| c.id == id) else {
            if self.claude.id == id {
                self.queue_commit_done(r);
            }
            return;
        };
        // a conversation not shown: made the current one the time to send its next task
        std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
        self.queue_commit_done(r);
        std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
    }

    pub(super) fn queue_commit_done(&mut self, r: Result<String, String>) {
        let q = &mut self.claude.queue;
        let Some(k) = q.current().filter(|k| q.tasks[*k].state == TaskState::Committing) else { return };
        match r {
            Ok(first) => {
                q.tasks[k].state = TaskState::Done;
                q.tasks[k].note = tf!("Commit : {first}", "Commit: {first}");
                self.claude.items.push(Item::Info(format!("— commit : {first} —")));
                self.git_refresh();
                self.claude_queue_next();
            }
            Err(e) => {
                q.tasks[k].state = TaskState::Failed;
                q.tasks[k].note = tf!("Le commit a échoué : {e}", "The commit failed: {e}");
                q.running = false;
                self.claude.items.push(Item::Error(tf!("Commit après la tâche : {e}", "Commit after the task: {e}")));
            }
        }
        self.claude_save();
    }

    /// The next waiting task, if the queue runs and Claude is free.
    pub(super) fn claude_queue_next(&mut self) {
        let q = &self.claude.queue;
        if !q.running || self.claude.busy || self.claude.model_hold.is_some() || q.current().is_some() {
            return;
        }
        let Some(k) = q.next_waiting() else {
            self.claude.queue.running = false;
            self.set_status(t!("File de tâches terminée", "Task queue done"));
            return;
        };
        let mut text = q.tasks[k].text.trim().to_string();
        let model = q.model_of(k);
        let previous = q.tasks[..k].iter().rev().find(|t| t.state == TaskState::Done).map(|t| t.text.lines().next().unwrap_or("").to_string());
        let fresh = q.fresh_each;
        let c = &mut self.claude;
        // a conversation of its own: the history is not sent again, only what was just done
        if fresh && c.items.iter().any(|i| matches!(i, Item::User(..))) {
            let last: Option<String> = c.items.iter().rev().find_map(|i| if let Item::Text(t) = i { Some(t.clone()) } else { None });
            c.session = None;
            c.session_id = None;
            c.last_uuid = None;
            c.resume_at = None;
            c.context.0 = 0;
            c.items.push(Item::Info(t!("— conversation neuve pour cette tâche —", "— new conversation for this task —").into()));
            if let (Some(prev), Some(last)) = (previous, last) {
                let mut said: String = last.chars().take(1200).collect();
                if said.len() < last.len() {
                    said.push('…');
                }
                text = tf!("Pour info, la tâche d'avant (« {prev} ») vient d'être faite ; sa conclusion :\n{said}\n\nTâche :\n{text}", "For information, the previous task (“{prev}”) has just been done; its conclusion:\n{said}\n\nTask:\n{text}");
            }
        }
        // another model: the process starts again, the conversation goes on (--resume)
        if let Some(m) = model.filter(|m| *m != c.model && *m < claude::MODELS.len()) {
            c.model = m;
            c.session = None;
        }
        self.claude.queue.tasks[k].state = TaskState::Running;
        let ask = self.claude.ask_first;
        if self.claude.model_first && self.claude.queue.tasks[k].model.is_none() {
            // a task with its own model keeps it: only the others are checked
            self.claude_check_model(text, ask, true);
            return;
        }
        if !self.claude_send_text(text, ask) {
            let q = &mut self.claude.queue;
            q.tasks[k].state = TaskState::Failed;
            q.tasks[k].note = t!("Pas pu être envoyée", "Could not be sent").into();
            q.running = false;
        }
    }

    pub(super) fn claude_queue_act(&mut self, act: QueueAct) {
        let q = &mut self.claude.queue;
        match act {
            QueueAct::Start => {
                q.running = true;
                q.base_model = Some(self.claude.model);
                self.claude_queue_next();
            }
            QueueAct::Pause => q.running = false,
            QueueAct::Remove(k) => {
                q.tasks.remove(k);
            }
            QueueAct::Up(k) => q.tasks.swap(k, k - 1),
            QueueAct::Retry(k) => {
                q.tasks[k].state = TaskState::Waiting;
                q.tasks[k].note.clear();
            }
            QueueAct::ClearDone => q.tasks.retain(|t| t.state != TaskState::Done),
            QueueAct::Close => q.tasks.clear(),
        }
        self.claude_save();
    }
}
