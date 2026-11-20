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
    let mut vars: Vec<Var> = body["variables"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|v| {
                    let name = v["name"].as_str().unwrap_or("").to_string();
                    let expr = v["evaluateName"].as_str().map(String::from).or_else(|| parent_expr.is_none().then(|| name.clone()));
                    Var { value: v["value"].as_str().unwrap_or("").to_string(), ty: v["type"].as_str().unwrap_or("").to_string(), reference: v["variablesReference"].as_i64().unwrap_or(0), expr, name }
                })
                .collect()
        })
        .unwrap_or_default();
    // the dunder attributes and the special groups of debugpy last (it puts its groups first):
    // the fields of a record are seen at once; the order of the others is kept
    vars.sort_by_key(|v| is_special(&v.name));
    vars
}

/// `__init__`, or a group of debugpy ("special variables", "function variables"…).
fn is_special(name: &str) -> bool {
    (name.len() > 4 && name.starts_with("__") && name.ends_with("__")) || name.ends_with(" variables")
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
                } else if command == "startDebugging" {
                    self.debug_child_session(seq, arguments);
                } else if let Some(s) = self.debug.session.as_mut() {
                    s.client.respond(seq, &command, false, json!({}));
                }
            }
            // while waiting (container stopped…): written once, not at each try
            Incoming::Stderr(l) if self.debug.session.as_ref().is_some_and(|s| !s.connected) => self.debug_auto_note(Cat::Err, l),
            Incoming::Stderr(l) => self.debug_note(Cat::Err, l),
            Incoming::Log(l) => self.debug_note(Cat::Info, l),
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

    /// js-debug runs the program in a session of its own, opened at its port: Florm goes on with
    /// it (the breakpoints, the pauses, the stop), the first one kept for the adapter to live.
    pub(super) fn debug_child_session(&mut self, seq: i64, args: Value) {
        let ctx = self.egui_ctx.clone();
        let Some(s) = self.debug.session.as_mut() else { return };
        let Some(port) = s.server else {
            s.client.respond(seq, "startDebugging", false, json!({}));
            return;
        };
        s.client.respond(seq, "startDebugging", true, json!({}));
        let child = match dap::Client::connect("127.0.0.1", port, move || ctx.request_repaint()) {
            Ok(c) => c,
            Err(e) => {
                self.debug_note(Cat::Err, e);
                return;
            }
        };
        let parent = std::mem::replace(&mut s.client, child);
        s.parents.push(parent);
        s.request = if args["request"] == "attach" { "attach" } else { "launch" };
        s.args = args["configuration"].clone();
        s.configured = false;
        s.pending.clear();
        let init = json!({
            "clientID": "florm", "clientName": "Florm", "adapterID": s.cfg.lang.adapter_id(),
            "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path",
            "supportsVariableType": true, "supportsRunInTerminalRequest": true, "supportsStartDebuggingRequest": true,
        });
        let seq = s.client.request("initialize", init);
        s.pending.insert(seq, Req::Initialize);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::net::{TcpListener, TcpStream};

    /// The other end of the session: what Florm sends to the debugger, read as it comes.
    struct Adapter(BufReader<TcpStream>);

    impl Adapter {
        /// The next message: (its type, the command, its arguments or body).
        fn next(&mut self) -> (String, String, Value) {
            let mut len = 0;
            loop {
                let mut line = String::new();
                self.0.read_line(&mut line).expect("a message");
                let line = line.trim();
                if line.is_empty() {
                    break;
                }
                if let Some(n) = line.strip_prefix("Content-Length:") {
                    len = n.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            self.0.read_exact(&mut body).unwrap();
            let m: Value = serde_json::from_slice(&body).unwrap();
            let args = if m["arguments"].is_null() { m["body"].clone() } else { m["arguments"].clone() };
            (m["type"].as_str().unwrap_or("").into(), m["command"].as_str().unwrap_or("").into(), args)
        }

        /// The commands of the next `n` requests.
        fn requests(&mut self, n: usize) -> Vec<(String, Value)> {
            (0..n).map(|_| self.next()).map(|(_, c, a)| (c, a)).collect()
        }
    }

    /// An app without a window, with a session connected to a fake debugger. `FLORM_TEST_APP=1`.
    fn session(name: &str) -> Option<(App, Adapter, std::sync::MutexGuard<'static, ()>, PathBuf)> {
        std::env::var_os("FLORM_TEST_APP")?;
        let dir = std::env::temp_dir().join(format!("florm_dap_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let mut app = App::with_ctx(&egui::Context::default(), Some(proj.clone()));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = dap::Client::connect("127.0.0.1", port, || {}).unwrap();
        let (stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let cfg = dap::Config { lang: Lang::Python, target: Target::Local, mode: Mode::Attach, command: String::new(), cwd: proj, python: String::new(), host: "127.0.0.1".into(), port, just_my_code: true, wait: false };
        app.debug.session = Some(Session {
            client, cfg, title: "test".into(), request: "attach", args: json!({}), mounts: Vec::new(), pending: HashMap::new(), configured: true, paused: None,
            threads: Vec::new(), stacks: HashMap::new(), frame: None, scopes: Vec::new(), vars: HashMap::new(), watch: HashMap::new(), hover: None,
            remote_docs: HashMap::new(), stopping: None, exception: None, connected: true, auto: false, server: None, parents: Vec::new(),
        });
        Some((app, Adapter(BufReader::new(stream)), lock, dir))
    }

    fn s(app: &mut App) -> &mut Session {
        app.debug.session.as_mut().unwrap()
    }

    /// Stopped on an exception: the threads, the stack of the thread and the exception asked.
    #[test]
    fn a_pause_asks_what_to_show() {
        let Some((mut app, mut ad, _lock, dir)) = session("pause") else { return };
        app.debug_event("stopped", json!({ "threadId": 4, "reason": "exception", "text": "ZeroDivisionError" }));
        let r = ad.requests(3);
        assert_eq!(r.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["threads", "stackTrace", "exceptionInfo"]);
        assert_eq!(r[1].1["threadId"], 4);
        let p = s(&mut app).paused.as_ref().unwrap();
        assert_eq!((p.thread, p.reason.as_str(), p.text.as_str()), (4, "exception", "ZeroDivisionError"));
        // the threads: the paused one first
        app.debug_response(Req::Threads, "threads", true, String::new(), json!({ "threads": [{ "id": 1, "name": "main" }, { "id": 4, "name": "worker" }] }));
        assert_eq!(s(&mut app).threads[0], (4, "worker".to_string()));
        // the exception, also in the console
        app.debug_response(Req::ExceptionInfo, "exceptionInfo", true, String::new(), json!({ "exceptionId": "ZeroDivisionError", "description": "division by zero" }));
        assert_eq!(s(&mut app).exception.as_deref(), Some("ZeroDivisionError : division by zero"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The stack of the paused thread: the first frame with a file is shown (not debugpy's), its
    /// scopes asked, then the variables of the first scope only (the others stay folded).
    #[test]
    fn the_first_frame_with_a_file_is_shown() {
        let Some((mut app, mut ad, _lock, dir)) = session("frames") else { return };
        app.debug_event("stopped", json!({ "threadId": 1, "reason": "breakpoint" }));
        ad.requests(2);
        let body = json!({ "stackFrames": [{ "id": 10, "name": "<debugpy>", "line": 1, "source": {} }, { "id": 11, "name": "f", "line": 3, "source": { "path": "/nowhere/a.py" } }] });
        app.debug_response(Req::Stack(1), "stackTrace", true, String::new(), body);
        assert_eq!(s(&mut app).frame, Some((1, 1)));
        let (cmd, args) = ad.requests(1).remove(0);
        assert_eq!((cmd.as_str(), &args["frameId"]), ("scopes", &json!(11)));
        app.debug_response(Req::Scopes(11), "scopes", true, String::new(), json!({ "scopes": [{ "name": "Locals", "variablesReference": 5 }, { "name": "Globals", "variablesReference": 6 }] }));
        let (cmd, args) = ad.requests(1).remove(0);
        assert_eq!((cmd.as_str(), &args["variablesReference"]), ("variables", &json!(5)));
        assert!(s(&mut app).pending.values().all(|r| !matches!(r, Req::Vars(6, _))));
        // the scopes of a frame no longer shown are dropped
        app.debug_response(Req::Scopes(99), "scopes", true, String::new(), json!({ "scopes": [{ "name": "Old", "variablesReference": 7 }] }));
        assert_eq!(s(&mut app).scopes.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What the debugger says of the breakpoints of a file goes on them, in order; an answer
    /// that does not match them (changed meanwhile) is left aside.
    #[test]
    fn breakpoints_get_their_state() {
        let Some((mut app, _ad, _lock, dir)) = session("bps") else { return };
        let path = PathBuf::from("/nowhere/a.py");
        app.debug.bps.insert(path.clone(), vec![Bp::new(2), Bp::new(9)]);
        let body = json!({ "breakpoints": [{ "verified": true }, { "verified": false, "message": "pas de code ici" }] });
        app.debug_response(Req::Bps(path.clone()), "setBreakpoints", true, String::new(), body);
        let states: Vec<_> = app.debug.bps[&path].iter().map(|b| b.state.clone()).collect();
        assert_eq!(states, [Some((true, String::new())), Some((false, "pas de code ici".to_string()))]);
        app.debug.bps.insert(path.clone(), vec![Bp::new(2)]);
        app.debug_response(Req::Bps(path.clone()), "setBreakpoints", true, String::new(), json!({ "breakpoints": [{ "verified": true }, { "verified": true }] }));
        assert!(app.debug.bps[&path][0].state.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A request of the debugger Florm does not know is refused; a failed start ends the session.
    #[test]
    fn unknown_requests_refused_and_failed_start_ends() {
        let Some((mut app, mut ad, _lock, dir)) = session("refuse") else { return };
        app.debug_handle(Incoming::Request { seq: 3, command: "handshake".into(), arguments: json!({}) });
        let (kind, cmd, _) = ad.next();
        assert_eq!((kind.as_str(), cmd.as_str()), ("response", "handshake"));
        // a watch that fails is shown as such, without ending anything
        app.debug_response(Req::Eval(EvalFor::Watch("x".into())), "evaluate", false, "name 'x' is not defined".into(), json!({}));
        assert!(matches!(s(&mut app).watch.get("x"), Some(Err(e)) if e.contains("not defined")));
        app.debug_response(Req::Start, "attach", false, "refused".into(), json!({}));
        assert!(app.debug.session.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn special_variables_last() {
        let names = ["special variables", "function variables", "__dict__", "partner_id", "__", "amount", "0"];
        let body = json!({ "variables": names.iter().map(|n| json!({ "name": n, "value": "" })).collect::<Vec<_>>() });
        let order: Vec<String> = parse_vars(&body, None).into_iter().map(|v| v.name).collect();
        assert_eq!(order, ["partner_id", "__", "amount", "0", "special variables", "function variables", "__dict__"]);
    }

    #[test]
    fn variables_without_evaluate_name() {
        // at the top, the name is the expression; under a variable, none (it cannot be guessed)
        let body = json!({ "variables": [{ "name": "total", "value": "3", "type": "int", "variablesReference": 0 }] });
        assert_eq!(parse_vars(&body, None)[0].expr.as_deref(), Some("total"));
        assert_eq!(parse_vars(&body, Some("")).first().and_then(|v| v.expr.clone()), None);
        assert!(parse_vars(&json!({}), None).is_empty());
        // a frame without line nor name still shows
        let f = parse_frames(&json!({ "stackFrames": [{ "id": 1 }] }), &Target::Local, &[]);
        assert_eq!((f[0].name.as_str(), f[0].line, f[0].local.is_none()), ("?", 1, true));
    }
}
