//! What debugpy sends (DAP): responses, events, the frames and variables of the pause, the
//! sources of the files of a container, the evaluations (console, hover, watches).

use super::*;

pub(super) fn parse_frames(body: &Value, target: &Target, mounts: &[(String, PathBuf)]) -> Vec<Frame> {
    body["stackFrames"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|f| {
                    let remote = f["source"]["path"].as_str().map(String::from);
                    Frame {
                        id: f["id"].as_i64().unwrap_or(0),
                        name: f["name"].as_str().unwrap_or("?").to_string(),
                        local: remote.as_deref().map(|r| crate::os::canon(&dap::local_path(r, target, mounts))),
                        remote,
                        line: f["line"].as_u64().unwrap_or(1) as usize,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn parse_vars(body: &Value, parent_expr: Option<&str>) -> Vec<Var> {
    body["variables"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|v| {
                    let name = v["name"].as_str().unwrap_or("").to_string();
                    let expr = v["evaluateName"].as_str().map(String::from).or_else(|| parent_expr.is_none().then(|| name.clone()));
                    Var { value: v["value"].as_str().unwrap_or("").to_string(), ty: v["type"].as_str().unwrap_or("").to_string(), reference: v["variablesReference"].as_i64().unwrap_or(0), expr, name }
                })
                // the dunder attributes and the special groups of debugpy last
                .collect()
        })
        .unwrap_or_default()
}

impl App {
    pub(super) fn debug_handle(&mut self, m: Incoming) {
        match m {
            Incoming::Response { request_seq, command, success, message, body } => {
                let Some(req) = self.debug.session.as_mut().and_then(|s| s.pending.remove(&request_seq)) else { return };
                self.debug_response(req, &command, success, message, body);
            }
            Incoming::Event { event, body } => self.debug_event(&event, body),
            Incoming::Request { seq, command, arguments } => {
                if command == "runInTerminal" {
                    self.debug_run_in_terminal(seq, arguments);
                } else if let Some(s) = self.debug.session.as_mut() {
                    s.client.respond(seq, &command, false, json!({}));
                }
            }
            // while waiting (container stopped…): written once, not at each try
            Incoming::Stderr(l) if self.debug.session.as_ref().is_some_and(|s| !s.connected) => self.debug_auto_note(Cat::Err, l),
            Incoming::Stderr(l) => self.debug_note(Cat::Err, l),
            Incoming::Closed => self.debug_end(t!("Débogage terminé", "Debugging ended").into()),
        }
    }

    pub(super) fn debug_response(&mut self, req: Req, command: &str, success: bool, message: String, body: Value) {
        if !success && !matches!(req, Req::Eval(EvalFor::Hover(..)) | Req::Eval(EvalFor::Watch(_)) | Req::Disconnect) {
            let msg = if message.is_empty() { tf!("{command} a échoué", "{command} failed") } else { format!("{command} : {message}") };
            self.debug_note(Cat::Err, msg.clone());
            if matches!(req, Req::Initialize | Req::Start) {
                self.set_error(tf!("Débogage impossible : {msg}", "Cannot debug: {msg}"));
                self.debug_end(t!("Débogage terminé", "Debugging ended").into());
                return;
            }
        }
        let Some(s) = self.debug.session.as_mut() else { return };
        match req {
            Req::Initialize => {
                let seq = s.client.request(s.request, s.args.clone());
                s.pending.insert(seq, Req::Start);
            }
            Req::Start | Req::Other => {}
            Req::Bps(path) => {
                let states: Vec<(bool, String)> = body["breakpoints"].as_array().map_or(Vec::new(), |a| a.iter().map(|b| (b["verified"].as_bool().unwrap_or(false), b["message"].as_str().unwrap_or("").to_string())).collect());
                if let Some(v) = self.debug.bps.get_mut(&path).filter(|v| v.len() == states.len()) {
                    for (b, st) in v.iter_mut().zip(states) {
                        b.state = Some(st);
                    }
                }
            }
            Req::Threads => {
                s.threads = body["threads"].as_array().map_or(Vec::new(), |a| a.iter().map(|t| (t["id"].as_i64().unwrap_or(0), t["name"].as_str().unwrap_or("").to_string())).collect());
                // the paused thread first
                if let Some(p) = &s.paused {
                    let t = p.thread;
                    s.threads.sort_by_key(|(id, _)| *id != t);
                }
            }
            Req::Stack(thread) => {
                let frames = parse_frames(&body, &s.cfg.target, &s.mounts);
                let first = (s.paused.as_ref().is_some_and(|p| p.thread == thread) && s.frame.is_none()).then(|| {
                    // the first frame with a file (not one of debugpy's)
                    frames.iter().position(|f| f.remote.is_some()).unwrap_or(0)
                });
                s.stacks.insert(thread, frames);
                if let Some(k) = first {
                    self.debug_select_frame(thread, k);
                }
            }
            Req::Scopes(frame_id) => {
                if s.frame.and_then(|(t, k)| s.stacks.get(&t)?.get(k)).map(|f| f.id) != Some(frame_id) {
                    return;
                }
                s.scopes = body["scopes"].as_array().map_or(Vec::new(), |a| a.iter().map(|x| Scope { name: x["name"].as_str().unwrap_or("").to_string(), reference: x["variablesReference"].as_i64().unwrap_or(0) }).collect());
                let (open, shut) = (&self.debug.open, &self.debug.shut);
                let wanted: Vec<(i64, String)> = s.scopes.iter().enumerate().filter(|(k, sc)| sc.reference > 0 && is_open(open, shut, &sc.name, *k == 0)).map(|(_, sc)| (sc.reference, sc.name.clone())).collect();
                for (r, path) in wanted {
                    let seq = s.client.request("variables", json!({ "variablesReference": r }));
                    s.pending.insert(seq, Req::Vars(r, path));
                }
            }
            Req::Vars(reference, path) => {
                let top = !path.contains('\u{1}');
                let vars = parse_vars(&body, if top { None } else { Some("") });
                let (open, shut) = (&self.debug.open, &self.debug.shut);
                let wanted: Vec<(i64, String)> = vars.iter().filter(|v| v.reference > 0).map(|v| (v.reference, format!("{path}\u{1}{}", v.name))).filter(|(_, p)| is_open(open, shut, p, false)).collect();
                s.vars.insert(reference, vars);
                for (r, p) in wanted {
                    let seq = s.client.request("variables", json!({ "variablesReference": r }));
                    s.pending.insert(seq, Req::Vars(r, p));
                }
            }
            Req::Eval(EvalFor::Repl) => {
                if success {
                    let r = body["result"].as_str().unwrap_or("").to_string();
                    self.debug_note(Cat::Result, r);
                }
            }
            Req::Eval(EvalFor::Watch(e)) => {
                let v = if success { Ok(body["result"].as_str().unwrap_or("").to_string()) } else { Err(message) };
                s.watch.insert(e, v);
            }
            Req::Eval(EvalFor::Hover(uid, range, expr)) => {
                if success && s.paused.is_some() {
                    let ty = body["type"].as_str().filter(|t| !t.is_empty()).map_or(String::new(), |t| format!(" ({t})"));
                    let text = format!("{expr}{ty} = {}", body["result"].as_str().unwrap_or(""));
                    if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                        d.debug_value = Some((range, text));
                    }
                }
            }
            Req::ExceptionInfo => {
                let id = body["exceptionId"].as_str().unwrap_or("");
                let desc = body["description"].as_str().unwrap_or("");
                let text = if desc.is_empty() { id.to_string() } else { format!("{id} : {desc}") };
                s.exception = Some(text.clone());
                self.debug_note(Cat::Err, text);
            }
            Req::Disconnect => self.debug_end(t!("Débogage arrêté", "Debugging stopped").into()),
        }
    }

    pub(super) fn debug_event(&mut self, event: &str, body: Value) {
        let Some(s) = self.debug.session.as_mut() else { return };
        match event {
            "initialized" => {
                s.configured = true;
                let mut paths: Vec<PathBuf> = self.debug.bps.keys().cloned().collect();
                paths.extend(self.debug.dirty.drain());
                paths.sort();
                paths.dedup();
                for p in paths {
                    self.debug_send_bps(&p);
                }
                self.debug_send_exceptions();
                if let Some(s) = self.debug.session.as_mut() {
                    let seq = s.client.request("configurationDone", json!({}));
                    s.pending.insert(seq, Req::Other);
                }
            }
            "stopped" => {
                let thread = body["threadId"].as_i64().unwrap_or(0);
                let reason = body["reason"].as_str().unwrap_or("").to_string();
                let text = body["text"].as_str().or_else(|| body["description"].as_str()).unwrap_or("").to_string();
                s.paused = Some(Paused { thread, reason: reason.clone(), text });
                s.stacks.clear();
                s.frame = None;
                s.scopes.clear();
                s.vars.clear();
                s.watch.clear();
                s.exception = None;
                let seq = s.client.request("threads", json!({}));
                s.pending.insert(seq, Req::Threads);
                let seq = s.client.request("stackTrace", json!({ "threadId": thread, "startFrame": 0, "levels": STACK_LEVELS }));
                s.pending.insert(seq, Req::Stack(thread));
                if reason == "exception" {
                    let seq = s.client.request("exceptionInfo", json!({ "threadId": thread }));
                    s.pending.insert(seq, Req::ExceptionInfo);
                }
                let title = s.title.clone();
                self.set_status(tf!("{title} : en pause ({})", "{title}: paused ({})", reason_label(&reason)));
                self.debug_show_paused();
                // the window comes to the front (the breakpoint was reached from the browser)
                self.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            "continued" => {
                if body["allThreadsContinued"].as_bool().unwrap_or(true) || s.paused.as_ref().is_some_and(|p| Some(p.thread) == body["threadId"].as_i64()) {
                    self.debug_resumed();
                }
            }
            "output" => {
                let cat = body["category"].as_str().unwrap_or("console");
                if cat == "telemetry" {
                    return;
                }
                let text = body["output"].as_str().unwrap_or("").trim_end_matches('\n').to_string();
                let c = match cat {
                    "stderr" => Cat::Err,
                    "stdout" => Cat::Out,
                    _ => Cat::Info,
                };
                self.debug_note(c, text);
            }
            "exited" => {
                let code = body["exitCode"].as_i64().unwrap_or(0);
                self.debug_note(Cat::Info, tf!("Programme terminé (code {code})", "Program ended (code {code})"));
            }
            "terminated" => self.debug_end(t!("Débogage terminé", "Debugging ended").into()),
            "process" => {
                let name = body["name"].as_str().unwrap_or("").to_string();
                let pid = body["systemProcessId"].as_i64().map_or(String::new(), |p| format!(" (pid {p})"));
                self.debug_note(Cat::Info, tf!("Processus {name}{pid}", "Process {name}{pid}"));
            }
            _ => {}
        }
    }

    /// The adapter asks to run the program in a terminal: a tab of Florm (in the container with
    /// `docker exec -it`), so that its logs are read there and an `input()` or `ipdb` answered.
    pub(super) fn debug_run_in_terminal(&mut self, seq: i64, args: Value) {
        let Some(s) = self.debug.session.as_ref() else { return };
        let (argv, cwd, env) = dap::terminal_command(&s.cfg.target, &args);
        let (title, mounts) = (format!("Debug {}", s.title), s.mounts.clone());
        self.debug_close_term();
        let ctx = self.egui_ctx.clone();
        match self.term.open_once_panel(&ctx, &argv, cwd, &env) {
            Ok(id) => {
                self.term.set_name(id, title);
                self.term.set_mounts(id, mounts);
                // the bottom panel, the file being worked on stays shown
                self.bottom = Some(BottomTab::Terminal);
                self.debug.term = Some(id);
                if let Some(s) = self.debug.session.as_mut() {
                    s.client.respond(seq, "runInTerminal", true, json!({}));
                }
            }
            Err(e) => {
                self.debug_note(Cat::Err, e.clone());
                if let Some(s) = self.debug.session.as_mut() {
                    s.client.respond(seq, "runInTerminal", false, json!({ "error": { "id": 1, "format": e } }));
                }
            }
        }
    }

    pub(super) fn debug_close_term(&mut self) {
        let Some(old) = self.debug.term.take() else { return };
        match self.docs.iter().position(|d| d.view == document::View::Terminal(old)) {
            Some(i) => self.close_tab(i, true),
            None => self.term.close(old),
        }
    }

    /// Shows frame `k` of `thread`: its line in the editor, its variables, the watches.
    pub(super) fn debug_select_frame(&mut self, thread: i64, k: usize) {
        let watch = self.debug.watch.clone();
        let Some(s) = self.debug.session.as_mut() else { return };
        let Some(f) = s.stacks.get(&thread).and_then(|v| v.get(k)) else { return };
        let (id, line, local, remote) = (f.id, f.line, f.local.clone(), f.remote.clone());
        s.frame = Some((thread, k));
        s.scopes.clear();
        s.vars.clear();
        s.watch.clear();
        let seq = s.client.request("scopes", json!({ "frameId": id }));
        s.pending.insert(seq, Req::Scopes(id));
        for e in watch {
            let seq = s.client.request("evaluate", json!({ "expression": e, "frameId": id, "context": "watch" }));
            s.pending.insert(seq, Req::Eval(EvalFor::Watch(e)));
        }
        for d in &mut self.docs {
            d.debug_value = None;
        }
        let line0 = line.saturating_sub(1);
        match (local, remote) {
            (Some(p), _) if p.is_file() => self.open_at(p, line0),
            (_, Some(r)) => {
                let s = self.debug.session.as_ref().expect("session");
                if let Some(i) = s.remote_docs.get(&r).and_then(|u| self.docs.iter().position(|d| d.uid == *u)) {
                    self.activate_tab(i);
                    let d = &mut self.docs[i];
                    let ind = d.indent_len(line0.min(d.rope.len_lines().saturating_sub(1)));
                    d.goto_line(line0, ind);
                } else if let Target::Docker(c) = s.cfg.target.clone() {
                    // a file of the image: read from the container, shown read only
                    self.jobs.spawn(move || JobResult::DebugSource { result: crate::docker::exec_output(&c, &["cat", &r]), remote: r, line: line0 });
                } else {
                    self.set_status(tf!("Fichier introuvable : {r}", "File not found: {r}"));
                }
            }
            _ => {}
        }
    }

    pub(in crate::app) fn debug_source(&mut self, remote: String, line: usize, result: Result<String, String>) {
        let Some(s) = self.debug.session.as_ref() else { return };
        let container = match &s.cfg.target {
            Target::Docker(c) => c.clone(),
            Target::Local => return,
        };
        match result {
            Ok(text) => {
                let name = remote.rsplit('/').next().unwrap_or(&remote).to_string();
                self.new_scratch(tf!("{name} ({container})", "{name} ({container})"), text, "Python", true);
                let uid = self.docs[self.active].uid;
                if let Some(s) = self.debug.session.as_mut() {
                    s.remote_docs.insert(remote.clone(), uid);
                }
                let d = &mut self.docs[self.active];
                let ind = d.indent_len(line.min(d.rope.len_lines().saturating_sub(1)));
                d.goto_line(line, ind);
                self.set_status(tf!("{remote} : fichier du conteneur, en lecture seule", "{remote}: file of the container, read only"));
            }
            Err(e) => self.set_error(tf!("{remote} illisible dans {container} : {e}", "{remote} unreadable in {container}: {e}")),
        }
    }

    /// Paused: the value of the word under the mouse (with what leads to it: `self.partner_id`).
    pub(super) fn debug_hover(&mut self) {
        let Some(s) = self.debug.session.as_mut().filter(|s| s.paused.is_some()) else { return };
        let Some(frame_id) = s.frame.and_then(|(t, k)| s.stacks.get(&t)?.get(k)).map(|f| f.id) else { return };
        let Some(d) = self.docs.get(self.active).filter(|d| d.syntax == "Python") else { return };
        let Some(range) = d.hover_word else { return };
        if s.hover == Some((d.uid, range)) || d.debug_value.as_ref().is_some_and(|(r, _)| *r == range) {
            return;
        }
        s.hover = Some((d.uid, range));
        let line = d.rope.char_to_line(range.0);
        let ls = d.rope.line_to_char(line);
        let Some(expr) = dap::hover_expression(&d.line_text(line), range.0 - ls, range.1.saturating_sub(ls)) else { return };
        let seq = s.client.request("evaluate", json!({ "expression": expr, "frameId": frame_id, "context": "hover" }));
        s.pending.insert(seq, Req::Eval(EvalFor::Hover(d.uid, range, expr)));
    }

    pub(super) fn debug_eval(&mut self, expr: String) {
        let expr = expr.trim().to_string();
        if expr.is_empty() {
            return;
        }
        self.debug.repl_hist.retain(|h| *h != expr);
        self.debug.repl_hist.insert(0, expr.clone());
        self.debug.repl_hist.truncate(50);
        self.debug_note(Cat::Input, format!("› {expr}"));
        let Some(s) = self.debug.session.as_mut() else {
            self.debug_note(Cat::Err, t!("Pas de débogage en cours", "No debugging session"));
            return;
        };
        let frame = s.frame.and_then(|(t, k)| s.stacks.get(&t)?.get(k)).map(|f| f.id);
        let mut args = json!({ "expression": expr, "context": "repl" });
        if let Some(f) = frame {
            args["frameId"] = json!(f);
        }
        let seq = s.client.request("evaluate", args);
        s.pending.insert(seq, Req::Eval(EvalFor::Repl));
    }

    pub(super) fn debug_watch_eval(&mut self, e: &str) {
        let Some(s) = self.debug.session.as_mut() else { return };
        let Some(f) = s.frame.and_then(|(t, k)| s.stacks.get(&t)?.get(k)).map(|f| f.id) else { return };
        let seq = s.client.request("evaluate", json!({ "expression": e, "frameId": f, "context": "watch" }));
        s.pending.insert(seq, Req::Eval(EvalFor::Watch(e.to_string())));
    }
}
