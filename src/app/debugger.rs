//! The debugger (as in Zed, through the Debug Adapter Protocol, see `dap.rs`): breakpoints in the
//! gutter (F9), F5 to start or go on, F10 / F11 / Maj+F11 to step, the call stack, the variables
//! and a console in the « Débogueur » view of the bottom panel.

use super::*;
use crate::dap::{self, LaunchCfg, Msg};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver, Sender};

mod panel;

/// What a thread made of a session to start: the adapter running, and the request that starts
/// the program (sent once the adapter answered `initialize`).
pub(crate) struct DebugStart {
    session: dap::Session,
    cfg: LaunchCfg,
    adapter_id: String,
    request: String,
    args: Value,
}

/// Where the session is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Phase {
    #[default]
    Idle,
    /// Its adapter is looked for, the program built, the adapter started.
    Starting,
    Running,
    Stopped,
}

/// What a request was for (its answer is handled accordingly).
enum Pending {
    Initialize,
    Launch,
    Threads,
    Stack,
    Scopes,
    Variables(i64),
    Evaluate,
    /// The value of the watched expression `k`.
    Watch(usize),
    Disconnect,
    Other,
}

#[derive(Clone)]
struct Frame {
    id: i64,
    name: String,
    path: Option<PathBuf>,
    line: usize,
}

#[derive(Clone)]
struct Var {
    name: String,
    value: String,
    kind: String,
    /// > 0: it has children (asked when opened).
    reference: i64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Line {
    Out,
    Err,
    /// Florm's own lines (started, stopped, the expressions typed and their value).
    Info,
}

pub(super) struct DebugUi {
    session: Option<dap::Session>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    phase: Phase,
    /// The configuration running (or last run: ⟳ starts it again).
    cfg: Option<LaunchCfg>,
    /// The file it was started from ({file} of the configuration).
    file: Option<PathBuf>,
    start_request: Option<(String, Value)>,
    caps: Value,
    pending: HashMap<i64, Pending>,
    thread: Option<i64>,
    threads: Vec<(i64, String)>,
    frames: Vec<Frame>,
    frame: usize,
    scopes: Vec<(String, i64)>,
    vars: HashMap<i64, Vec<Var>>,
    open: HashSet<i64>,
    console: Vec<(Line, String)>,
    input: String,
    reason: String,
    /// The configuration chosen in the panel: "cmd" the command typed, "" the file, "test" the test
    /// under the cursor, else the name of one of `.florm/debug.toml`.
    pub(super) choice: String,
    /// The command typed (as in a terminal) and the folder it runs in (relative to the project).
    pub(super) cmd: String,
    cmd_dir: String,
    /// The project whose last command was put in the fields.
    cmd_for: Option<PathBuf>,
    /// `.florm/debug.toml` of the project, read when the panel shows or F5 is pressed.
    configs: Vec<LaunchCfg>,
    configs_err: Option<String>,
    /// The breakpoints of the files closed (given back when they open again).
    saved: HashMap<PathBuf, Vec<usize>>,
    /// Their conditions (line → condition).
    saved_conds: HashMap<PathBuf, HashMap<usize, String>>,
    /// The watched expressions and their value at the last stop (Err: the adapter's error).
    pub(super) watches: Vec<(String, Option<Result<String, String>>)>,
    /// The field of a new watched expression.
    pub(super) watch_input: String,
    /// Started again once the session ends (⟳).
    restart: bool,
}

impl Default for DebugUi {
    fn default() -> Self {
        let (tx, rx) = channel();
        DebugUi {
            session: None,
            tx,
            rx,
            phase: Phase::Idle,
            cfg: None,
            file: None,
            start_request: None,
            caps: Value::Null,
            pending: HashMap::new(),
            thread: None,
            threads: Vec::new(),
            frames: Vec::new(),
            frame: 0,
            scopes: Vec::new(),
            vars: HashMap::new(),
            open: HashSet::new(),
            console: Vec::new(),
            input: String::new(),
            reason: String::new(),
            // the command first: the way most programs start (empty, F5 debugs the open file)
            choice: "cmd".into(),
            cmd: String::new(),
            cmd_dir: String::new(),
            cmd_for: None,
            configs: Vec::new(),
            configs_err: None,
            saved: HashMap::new(),
            saved_conds: HashMap::new(),
            watches: Vec::new(),
            watch_input: String::new(),
            restart: false,
        }
    }
}

/// The files Florm can debug without a configuration (and those of any configured project).
fn debuggable(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("py" | "go" | "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "zig" | "m"))
}

impl DebugUi {
    fn log(&mut self, kind: Line, text: impl Into<String>) {
        let text: String = text.into();
        for l in text.trim_end_matches('\n').split('\n') {
            self.console.push((kind, l.to_string()));
        }
        let n = self.console.len();
        if n > 5000 {
            self.console.drain(..n - 5000);
        }
    }

    fn request(&mut self, command: &str, args: Value, what: Pending) {
        if let Some(s) = self.session.as_mut() {
            let seq = s.request(command, args);
            self.pending.insert(seq, what);
        }
    }

    /// Back to no session (its adapter is stopped when dropped).
    fn ended(&mut self) {
        self.session = None;
        self.phase = Phase::Idle;
        self.pending.clear();
        self.frames.clear();
        self.scopes.clear();
        self.vars.clear();
        self.threads.clear();
        self.thread = None;
    }

    fn selected(&self) -> Option<&Frame> {
        self.frames.get(self.frame)
    }
}

impl App {
    /// The session for the status bar: None without one, Some(true) stopped, Some(false) running.
    pub(in crate::app) fn debug_status(&self) -> Option<bool> {
        match self.debug.phase {
            Phase::Idle => None,
            Phase::Stopped => Some(true),
            Phase::Running | Phase::Starting => Some(false),
        }
    }

    /// (the file can be debugged, the line where the debugger stopped in it), for the editor.
    pub(super) fn debug_marks(&self, i: usize) -> (bool, Option<usize>) {
        let Some(path) = self.docs.get(i).and_then(|d| d.path.as_deref()) else { return (false, None) };
        let can = debuggable(path) || !self.debug.configs.is_empty() || !self.docs[i].breakpoints.is_empty();
        let exec = self.debug.selected().filter(|f| self.debug.phase == Phase::Stopped && f.path.as_deref() == Some(path)).map(|f| f.line);
        (can, exec)
    }

    /// The breakpoints of the files, open or not: (file, lines from 0, their conditions).
    fn all_breakpoints(&self) -> Vec<(PathBuf, Vec<usize>, HashMap<usize, String>)> {
        let mut out: Vec<(PathBuf, Vec<usize>, HashMap<usize, String>)> =
            self.docs.iter().filter_map(|d| Some((d.path.clone()?, d.breakpoint_lines(), d.breakpoint_conditions()))).filter(|(_, l, _)| !l.is_empty()).collect();
        for (p, lines) in &self.debug.saved {
            if !self.docs.iter().any(|d| d.path.as_ref() == Some(p)) && !lines.is_empty() {
                out.push((p.clone(), lines.clone(), self.debug.saved_conds.get(p).cloned().unwrap_or_default()));
            }
        }
        out
    }

    /// Tells the adapter the breakpoints of `path` (all of them for the file: the protocol replaces
    /// them), with their condition (`log: …` a logpoint: the text written, `{x}` replaced).
    fn send_breakpoints(&mut self, path: &Path, lines: &[usize], conds: &HashMap<usize, String>) {
        let bps: Vec<Value> = lines.iter().map(|l| breakpoint_json(*l, conds.get(l).map(String::as_str))).collect();
        self.debug.request("setBreakpoints", json!({"source": {"path": path, "name": path.file_name().map(|n| n.to_string_lossy().into_owned())}, "breakpoints": bps}), Pending::Other);
    }

    /// The breakpoints of a closed file are kept, to be given back when it opens again.
    pub(super) fn debug_keep_breakpoints(&mut self, d: &Document) {
        if let Some(p) = d.path.clone() {
            let lines = d.breakpoint_lines();
            if lines.is_empty() {
                self.debug.saved.remove(&p);
                self.debug.saved_conds.remove(&p);
            } else {
                self.debug.saved.insert(p.clone(), lines);
                self.debug.saved_conds.insert(p, d.breakpoint_conditions());
            }
        }
    }

    /// Each frame: the breakpoints given back or changed, the messages of the adapter.
    pub(super) fn debug_frame(&mut self, _ctx: &egui::Context) {
        let live = self.debug.session.is_some() && matches!(self.debug.phase, Phase::Running | Phase::Stopped);
        // right click in the column of the breakpoints: the field of its condition
        if let Some((uid, line)) = self.docs.iter_mut().find_map(|d| d.bp_condition_request.take().map(|l| (d.uid, l))) {
            self.open_prompt(Prompt::BreakpointCondition(uid, line));
        }
        let mut changed = Vec::new();
        for d in self.docs.iter_mut() {
            if !d.breakpoints_loaded {
                d.breakpoints_loaded = true;
                if let Some(lines) = d.path.as_ref().and_then(|p| self.debug.saved.get(p)) {
                    d.set_breakpoint_lines(lines);
                }
                if let Some(conds) = d.path.as_ref().and_then(|p| self.debug.saved_conds.get(p)) {
                    d.set_breakpoint_conditions(conds);
                }
            }
            if std::mem::take(&mut d.breakpoints_changed) {
                if let Some(p) = d.path.clone() {
                    changed.push((p, d.breakpoint_lines(), d.breakpoint_conditions()));
                }
            }
        }
        for (p, lines, conds) in changed {
            if lines.is_empty() {
                self.debug.saved.remove(&p);
                self.debug.saved_conds.remove(&p);
            } else {
                self.debug.saved.insert(p.clone(), lines.clone());
                self.debug.saved_conds.insert(p.clone(), conds.clone());
            }
            if live {
                self.send_breakpoints(&p, &lines, &conds);
            }
        }
        while let Ok(m) = self.debug.rx.try_recv() {
            self.debug_message(m);
        }
    }

    fn debug_message(&mut self, m: Msg) {
        match m {
            Msg::Response { seq, ok, body, message, .. } => {
                let what = self.debug.pending.remove(&seq).unwrap_or(Pending::Other);
                if !ok {
                    let msg = if message.is_empty() { body["error"]["format"].as_str().unwrap_or("").to_string() } else { message };
                    match what {
                        Pending::Evaluate => self.debug.log(Line::Err, msg),
                        Pending::Watch(k) => {
                            if let Some(w) = self.debug.watches.get_mut(k) {
                                w.1 = Some(Err(msg));
                            }
                        }
                        Pending::Launch | Pending::Initialize => {
                            self.debug.log(Line::Err, tf!("Le programme n'a pas démarré : {msg}", "The program did not start: {msg}"));
                            self.set_error(tf!("Débogueur : {msg}", "Debugger: {msg}"));
                            self.debug.ended();
                        }
                        Pending::Disconnect => self.debug.ended(),
                        _ if !msg.is_empty() => self.debug.log(Line::Err, msg),
                        _ => {}
                    }
                    return;
                }
                match what {
                    Pending::Initialize => {
                        self.debug.caps = body;
                        if let Some((req, args)) = self.debug.start_request.take() {
                            self.debug.request(&req, args, Pending::Launch);
                        }
                    }
                    Pending::Launch => {}
                    Pending::Threads => {
                        self.debug.threads = body["threads"].as_array().map(|a| a.iter().map(|t| (t["id"].as_i64().unwrap_or(0), t["name"].as_str().unwrap_or("").to_string())).collect()).unwrap_or_default();
                    }
                    Pending::Stack => self.debug_stack(&body),
                    Pending::Scopes => {
                        self.debug.scopes = body["scopes"].as_array().map(|a| a.iter().map(|s| (s["name"].as_str().unwrap_or("").to_string(), s["variablesReference"].as_i64().unwrap_or(0))).collect()).unwrap_or_default();
                        self.debug.vars.clear();
                        // the first scope (the locals) open, the others when clicked
                        let first = self.debug.scopes.first().map(|s| s.1).filter(|r| *r > 0);
                        if let Some(r) = first {
                            self.debug.open.insert(r);
                            self.debug.request("variables", json!({"variablesReference": r}), Pending::Variables(r));
                        }
                        for (_, r) in self.debug.scopes.clone() {
                            if self.debug.open.contains(&r) && Some(r) != first {
                                self.debug.request("variables", json!({"variablesReference": r}), Pending::Variables(r));
                            }
                        }
                    }
                    Pending::Variables(r) => {
                        let vars = body["variables"].as_array().map(|a| a.iter().map(|v| Var {
                            name: v["name"].as_str().unwrap_or("").to_string(),
                            value: v["value"].as_str().unwrap_or("").to_string(),
                            kind: v["type"].as_str().unwrap_or("").to_string(),
                            reference: v["variablesReference"].as_i64().unwrap_or(0),
                        }).collect()).unwrap_or_default();
                        self.debug.vars.insert(r, vars);
                    }
                    Pending::Evaluate => {
                        let r = body["result"].as_str().unwrap_or("").to_string();
                        self.debug.log(Line::Out, r);
                    }
                    Pending::Watch(k) => {
                        if let Some(w) = self.debug.watches.get_mut(k) {
                            w.1 = Some(Ok(body["result"].as_str().unwrap_or("").to_string()));
                        }
                    }
                    Pending::Disconnect => self.debug.ended(),
                    Pending::Other => {}
                }
            }
            Msg::Event { event, body } => match event.as_str() {
                "initialized" => {
                    for (p, lines, conds) in self.all_breakpoints() {
                        self.send_breakpoints(&p, &lines, &conds);
                    }
                    if self.debug.caps["supportsConfigurationDoneRequest"].as_bool().unwrap_or(false) {
                        self.debug.request("configurationDone", json!({}), Pending::Other);
                    }
                    self.debug.phase = Phase::Running;
                }
                "stopped" => {
                    self.debug.phase = Phase::Stopped;
                    self.debug.reason = body["reason"].as_str().unwrap_or("").to_string();
                    let thread = body["threadId"].as_i64().or(self.debug.thread).or_else(|| self.debug.threads.first().map(|t| t.0)).unwrap_or(1);
                    self.debug.thread = Some(thread);
                    self.debug.request("threads", json!({}), Pending::Threads);
                    self.debug.request("stackTrace", json!({"threadId": thread, "startFrame": 0, "levels": 64}), Pending::Stack);
                    if let Some(text) = body["text"].as_str().or(body["description"].as_str()).filter(|t| !t.is_empty()) {
                        self.debug.log(Line::Info, format!("⏸ {text}"));
                    }
                }
                "continued" => {
                    self.debug.phase = Phase::Running;
                    self.debug.frames.clear();
                    self.debug.scopes.clear();
                    self.debug.vars.clear();
                }
                "thread" => {
                    let id = body["threadId"].as_i64().unwrap_or(0);
                    if body["reason"] == "started" && !self.debug.threads.iter().any(|t| t.0 == id) {
                        self.debug.threads.push((id, String::new()));
                    }
                }
                "output" => {
                    let kind = match body["category"].as_str().unwrap_or("console") {
                        "stderr" => Line::Err,
                        "telemetry" => return,
                        "stdout" => Line::Out,
                        _ => Line::Info,
                    };
                    self.debug.log(kind, body["output"].as_str().unwrap_or(""));
                }
                "exited" => {
                    let code = body["exitCode"].as_i64().unwrap_or(0);
                    self.debug.log(Line::Info, tf!("■ Le programme s'est terminé (code {code})", "■ The program ended (code {code})"));
                }
                "terminated" => {
                    self.debug.request("disconnect", json!({"terminateDebuggee": true}), Pending::Disconnect);
                    self.debug.phase = Phase::Idle;
                    self.debug.frames.clear();
                }
                _ => {}
            },
            Msg::Exited => {
                if self.debug.session.is_some() {
                    self.debug.ended();
                    self.debug.log(Line::Info, t!("■ Débogueur arrêté", "■ Debugger stopped"));
                }
                if std::mem::take(&mut self.debug.restart) {
                    if let Some(cfg) = self.debug.cfg.clone() {
                        let file = self.debug.file.clone();
                        self.debug_launch(cfg, file);
                    }
                }
            }
        }
    }

    /// The call stack of the stop: its first frame with a file shown in the editor, its variables asked.
    fn debug_stack(&mut self, body: &Value) {
        self.debug.frames = body["stackFrames"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|f| Frame {
                        id: f["id"].as_i64().unwrap_or(0),
                        name: f["name"].as_str().unwrap_or("").to_string(),
                        path: f["source"]["path"].as_str().filter(|p| !p.is_empty()).map(PathBuf::from),
                        line: (f["line"].as_u64().unwrap_or(1) as usize).saturating_sub(1),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let k = self.debug.frames.iter().position(|f| f.path.as_ref().is_some_and(|p| p.is_file())).unwrap_or(0);
        self.debug_select_frame(k);
    }

    /// Frame `k` of the stack chosen: its line shown, its variables asked.
    fn debug_select_frame(&mut self, k: usize) {
        self.debug.frame = k;
        let Some(f) = self.debug.frames.get(k).cloned() else { return };
        if let Some(p) = f.path.filter(|p| p.is_file()) {
            if self.open_path(p) {
                if let Some(d) = self.docs.get_mut(self.active) {
                    d.goto_line(f.line, 0);
                }
            }
        }
        self.debug.request("scopes", json!({"frameId": f.id}), Pending::Scopes);
        self.debug_eval_watches();
    }

    /// The watched expressions evaluated in the frame shown (at each stop, or a frame chosen).
    pub(super) fn debug_eval_watches(&mut self) {
        if self.debug.phase != Phase::Stopped {
            return;
        }
        let frame = self.debug.selected().map(|f| f.id);
        for k in 0..self.debug.watches.len() {
            let mut args = json!({"expression": self.debug.watches[k].0, "context": "watch"});
            if let Some(f) = frame {
                args["frameId"] = json!(f);
            }
            self.debug.request("evaluate", args, Pending::Watch(k));
        }
    }

    /// A watched expression added (the selection, the panel), evaluated at once when stopped.
    pub(super) fn debug_add_watch(&mut self, expr: String) {
        let expr = expr.trim().to_string();
        if expr.is_empty() || self.debug.watches.iter().any(|w| w.0 == expr) {
            return;
        }
        self.debug.watches.push((expr, None));
        self.debug_eval_watches();
    }

    /// The configurations of the project read again (`.florm/debug.toml`, in a trusted folder).
    fn debug_read_configs(&mut self) {
        let root = self.root.clone();
        let (configs, err) = match root.filter(|r| self.trusted(r)) {
            Some(r) => match dap::project_configs(&r) {
                Ok(c) => (c, None),
                Err(e) => (Vec::new(), Some(e)),
            },
            None => (Vec::new(), None),
        };
        self.debug.configs = configs;
        self.debug.configs_err = err;
    }

    /// The configuration for the file shown, made by Florm. Err: why it cannot.
    fn debug_file_config(&self) -> Result<(LaunchCfg, PathBuf), String> {
        let Some(path) = self.docs.get(self.active).and_then(|d| d.path.clone()) else {
            return Err(t!("Ouvre d'abord le fichier à déboguer", "Open the file to debug first").into());
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let base = |marks: &[&str]| path.ancestors().skip(1).find(|d| marks.iter().any(|m| d.join(m).is_file())).map(Path::to_path_buf);
        let cfg = match path.extension().and_then(|e| e.to_str()) {
            Some("py") => LaunchCfg { name, adapter: "python".into(), program: path.display().to_string(), ..Default::default() },
            Some("go") => {
                let dir = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                let mut extra = serde_json::Map::new();
                extra.insert("mode".into(), "debug".into());
                LaunchCfg { name, adapter: "go".into(), program: dir, extra, ..Default::default() }
            }
            Some("rs") => {
                let Some(dir) = base(&["Cargo.toml"]) else { return Err(t!("Pas de Cargo.toml au-dessus du fichier", "No Cargo.toml above the file").into()) };
                let text = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
                let pkg = text.parse::<toml::Table>().ok().and_then(|t| t.get("package")?.get("name")?.as_str().map(String::from));
                let Some(pkg) = pkg else {
                    return Err(t!("Cargo.toml sans [package] : décris le programme dans .florm/debug.toml", "Cargo.toml without [package]: describe the program in .florm/debug.toml").into());
                };
                let target = dir.join("target").join("debug").join(&pkg);
                LaunchCfg { name, adapter: "native".into(), build: "cargo build".into(), program: target.display().to_string(), cwd: dir.display().to_string(), ..Default::default() }
            }
            _ => return Err(t!("Pour ce fichier, décris le programme à lancer dans .florm/debug.toml (Débogueur : configurations du projet)", "For this file, describe the program to run in .florm/debug.toml (Debugger: configurations of the project)").into()),
        };
        Ok((cfg, path))
    }

    /// The command typed to debug, with its folder, when « Commande » is chosen.
    pub(super) fn debug_command_line(&self) -> Option<(String, PathBuf)> {
        (self.debug.choice == "cmd" && !self.debug.cmd.trim().is_empty()).then(|| (self.debug.cmd.clone(), self.debug_cmd_dir()))
    }

    /// The folder of the command: the field, relative to the project (empty: the project).
    fn debug_cmd_dir(&self) -> PathBuf {
        cmd_dir_of(self.root.as_deref(), &self.debug.cmd_dir)
    }

    /// A folder chosen in the dialog for the command: relative to the project when it is inside.
    pub(in crate::app) fn debug_dir_picked(&mut self, p: PathBuf) {
        self.debug.cmd_dir = match self.root.as_deref().and_then(|r| p.strip_prefix(r).ok()) {
            Some(rel) if rel.as_os_str().is_empty() => String::new(),
            Some(rel) => rel.display().to_string(),
            None => p.display().to_string(),
        };
    }
}

/// The folder of the command for the field's text `d`, from the project `root` (empty: the project).
pub(super) fn cmd_dir_of(root: Option<&Path>, d: &str) -> PathBuf {
    {
        let root = root.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let d = d.trim();
        if d.is_empty() || d == "." {
            root
        } else if let Some(rest) = d.strip_prefix("~/").or_else(|| d.strip_prefix("~\\")) {
            crate::config::home().join(rest)
        } else if d == "~" {
            crate::config::home()
        } else if Path::new(d).is_absolute() {
            PathBuf::from(d)
        } else {
            root.join(d)
        }
    }
}

impl App {
    /// The command typed, read into a configuration and kept in the history of the project.
    fn debug_command_config(&mut self) -> Result<LaunchCfg, String> {
        let dir = self.debug_cmd_dir();
        if !dir.is_dir() {
            return Err(tf!("Le dossier de la commande n'existe pas : {}", "The folder of the command does not exist: {}", dir.display()));
        }
        let cfg = dap::from_command(&self.debug.cmd, &dir)?;
        let project = self.root.as_ref().map(|r| r.display().to_string()).unwrap_or_default();
        self.state.remember_debug(project::DebugRun { cmd: self.debug.cmd.trim().to_string(), dir: self.debug.cmd_dir.trim().to_string(), project });
        project::save_state(&self.state);
        Ok(cfg)
    }

    /// The commands of the history run in this project: (command, folder), the last first.
    fn debug_history(&self) -> Vec<(String, String)> {
        let project = self.root.as_ref().map(|r| r.display().to_string()).unwrap_or_default();
        self.state.debug_history.iter().filter(|r| r.project == project).map(|r| (r.cmd.clone(), r.dir.clone())).collect()
    }

    /// Once per project: its last command in the fields, chosen when there is one.
    fn debug_prefill(&mut self) {
        if self.debug.cmd_for == self.root {
            return;
        }
        self.debug.cmd_for = self.root.clone();
        match self.debug_history().into_iter().next() {
            Some((cmd, dir)) => {
                (self.debug.cmd, self.debug.cmd_dir) = (cmd, dir);
                self.debug.choice = "cmd".into();
            }
            None => (self.debug.cmd, self.debug.cmd_dir) = (String::new(), String::new()),
        }
    }

    /// The configuration of the test under the cursor (pytest, go test, cargo test).
    fn debug_test_config(&self) -> Result<(LaunchCfg, PathBuf), String> {
        let d = self.docs.get(self.active).ok_or("")?;
        let path = d.path.clone().ok_or_else(|| t!("Ouvre d'abord un fichier de tests", "Open a test file first").to_string())?;
        let root = self.repo_for(&path).or_else(|| self.root.clone()).or_else(|| path.parent().map(Path::to_path_buf)).ok_or("")?;
        let line = d.rope.char_to_line(d.primary_sel().head);
        let cmd = crate::test_cmd::command_at(&path, &d.rope.to_string(), &root, line).ok_or_else(|| t!("Pas de test sous le curseur", "No test under the cursor").to_string())?;
        let (cd, argv) = crate::test_cmd::parse(&cmd).ok_or("")?;
        let cwd = cd.map(|c| root.join(c)).unwrap_or_else(|| root.clone());
        let name = tf!("Test : {}", "Test: {}", argv.last().cloned().unwrap_or_default());
        let cfg = match crate::test_cmd::runner_of(&path) {
            Some(crate::test_cmd::Runner::Pytest) => LaunchCfg { name, adapter: "python".into(), module: "pytest".into(), args: argv[1..].to_vec(), cwd: cwd.display().to_string(), ..Default::default() },
            Some(crate::test_cmd::Runner::Go) => {
                // go test <pkg> -run <pattern>
                let pkg = argv.get(2).cloned().unwrap_or_else(|| ".".into());
                let pattern = argv.iter().position(|a| a == "-run").and_then(|i| argv.get(i + 1)).cloned();
                let mut extra = serde_json::Map::new();
                extra.insert("mode".into(), "test".into());
                let args = pattern.map(|p| vec!["-test.run".to_string(), p]).unwrap_or_default();
                LaunchCfg { name, adapter: "go".into(), program: cwd.join(pkg).display().to_string(), args, cwd: cwd.display().to_string(), extra, ..Default::default() }
            }
            Some(crate::test_cmd::Runner::Cargo) => {
                let filter: Vec<String> = argv.iter().skip(2).cloned().collect();
                let mut args: Vec<String> = filter.iter().filter(|a| !a.starts_with('-')).cloned().collect();
                args.push("--nocapture".into());
                LaunchCfg { name, adapter: "native".into(), args, cwd: cwd.display().to_string(), cargo_test: Some(filter), ..Default::default() }
            }
            _ => return Err(t!("Ce type de test ne se débogue pas encore (pytest, go test, cargo test)", "This kind of test cannot be debugged yet (pytest, go test, cargo test)").into()),
        };
        Ok((cfg, path))
    }

    /// Starts `cfg` on a thread: its adapter found, its program built, the adapter started.
    fn debug_launch(&mut self, cfg: LaunchCfg, file: Option<PathBuf>) {
        if self.debug.session.is_some() || self.debug.phase == Phase::Starting {
            return;
        }
        let root = self.root.clone().or_else(|| file.as_ref().and_then(|f| f.parent().map(Path::to_path_buf))).unwrap_or_else(|| PathBuf::from("."));
        if !self.trusted(&root) {
            self.set_error(t!("Débogueur : le dossier n'est pas de confiance (🔒 Mode restreint)", "Debugger: the folder is not trusted (🔒 Restricted mode)"));
            return;
        }
        self.debug.cfg = Some(cfg.clone());
        self.debug.file = file.clone();
        self.debug.phase = Phase::Starting;
        self.debug.console.clear();
        self.debug.reason.clear();
        self.debug.log(Line::Info, tf!("▶ {} …", "▶ {} …", cfg.name));
        self.show_bottom(BottomTab::Debug, false);
        // the unsaved files are saved: the program runs what is on the disk
        for i in 0..self.docs.len() {
            if self.docs[i].is_dirty() && self.docs[i].path.is_some() {
                self.save(i);
            }
        }
        let (events, ctx) = (self.debug.tx.clone(), self.egui_ctx.clone());
        self.jobs.spawn(move || {
            let r = (|| {
                let mut cfg = cfg;
                let spec = dap::adapter_for(&cfg, &root)?;
                let cwd = if cfg.cwd.trim().is_empty() { root.clone() } else { PathBuf::from(dap::expand(&cfg.cwd, file.as_deref(), &root)) };
                if !cfg.build.trim().is_empty() {
                    dap::run_build(&dap::expand(&cfg.build, file.as_deref(), &root), &cwd)?;
                }
                if let Some(filter) = cfg.cargo_test.take() {
                    cfg.program = dap::cargo_test_executable(&cwd, &filter)?.display().to_string();
                }
                let (request, args) = dap::launch_args(&cfg, &spec, file.as_deref(), &root);
                let session = dap::Session::start(&spec, &cwd, events, move || ctx.request_repaint())?;
                Ok(DebugStart { session, cfg, adapter_id: spec.id.clone(), request, args })
            })();
            JobResult::DebugStarted(r)
        });
    }

    /// The thread of `debug_launch` is done.
    pub(super) fn debug_started(&mut self, r: Result<DebugStart, String>) {
        match r {
            Ok(st) => {
                self.debug.session = Some(st.session);
                self.debug.start_request = Some((st.request, st.args));
                self.debug.cfg = Some(st.cfg);
                let init = json!({
                    "clientID": "florm", "clientName": "Florm", "adapterID": st.adapter_id,
                    "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path",
                    "supportsVariableType": true, "supportsRunInTerminalRequest": false, "locale": if crate::i18n::en() { "en" } else { "fr" },
                });
                self.debug.request("initialize", init, Pending::Initialize);
            }
            Err(e) => {
                self.debug.phase = Phase::Idle;
                self.debug.log(Line::Err, e.clone());
                self.set_error(tf!("Débogueur : {}", "Debugger: {}", e.lines().next().unwrap_or("")));
            }
        }
    }

    /// F5: on when stopped, else starts the configuration chosen in the panel.
    fn debug_start_or_continue(&mut self) {
        match self.debug.phase {
            Phase::Stopped => self.debug_step("continue"),
            Phase::Running | Phase::Starting => self.set_status(t!("Le débogueur tourne déjà (Maj+F5 pour l'arrêter)", "The debugger is already running (Shift+F5 to stop it)")),
            Phase::Idle => {
                self.debug_read_configs();
                self.debug_prefill();
                // its debugger missing: installed first, then F5 goes on by itself
                if self.debug_needs_install() {
                    self.show_bottom(BottomTab::Debug, false);
                    return;
                }
                let choice = self.debug.choice.clone();
                let r = match choice.as_str() {
                    "cmd" if !self.debug.cmd.trim().is_empty() => self.debug_command_config().map(|c| (c, self.docs.get(self.active).and_then(|d| d.path.clone()))),
                    "" | "cmd" => self.debug_file_config().map(|(c, f)| (c, Some(f))),
                    "test" => self.debug_test_config().map(|(c, f)| (c, Some(f))),
                    name => match self.debug.configs.iter().find(|c| c.name == name).cloned() {
                        Some(c) => Ok((c, self.docs.get(self.active).and_then(|d| d.path.clone()))),
                        None => self.debug_file_config().map(|(c, f)| (c, Some(f))),
                    },
                };
                match r {
                    Ok((cfg, file)) => self.debug_launch(cfg, file),
                    Err(e) => {
                        self.show_bottom(BottomTab::Debug, false);
                        self.debug.log(Line::Err, e.clone());
                        self.set_error(e);
                    }
                }
            }
        }
    }

    /// continue, next, stepIn, stepOut on the thread stopped.
    fn debug_step(&mut self, command: &str) {
        if self.debug.phase != Phase::Stopped {
            return;
        }
        let Some(thread) = self.debug.thread else { return };
        self.debug.request(command, json!({"threadId": thread}), Pending::Other);
        self.debug.phase = Phase::Running;
        self.debug.frames.clear();
    }

    fn debug_stop(&mut self) {
        if self.debug.session.is_some() {
            self.debug.request("disconnect", json!({"terminateDebuggee": true}), Pending::Disconnect);
            self.debug.log(Line::Info, t!("■ Arrêt…", "■ Stopping…"));
        }
    }

    /// An expression of the console, in the frame shown.
    fn debug_evaluate(&mut self, expr: String) {
        self.debug.log(Line::Info, format!("> {expr}"));
        let frame = self.debug.selected().map(|f| f.id);
        let mut args = json!({"expression": expr, "context": "repl"});
        if let Some(f) = frame {
            args["frameId"] = json!(f);
        }
        self.debug.request("evaluate", args, Pending::Evaluate);
    }

    /// The commands of the debugger (palette, shortcuts).
    pub(super) fn debug_command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::DebugStart => self.debug_start_or_continue(),
            Cmd::DebugStop => self.debug_stop(),
            Cmd::DebugRestart => {
                if self.debug.session.is_some() {
                    self.debug.restart = true;
                    self.debug_stop();
                } else if let Some(cfg) = self.debug.cfg.clone() {
                    let file = self.debug.file.clone();
                    self.debug_launch(cfg, file);
                } else {
                    self.debug_start_or_continue();
                }
            }
            Cmd::DebugPause => {
                if self.debug.phase == Phase::Running {
                    let thread = self.debug.thread.or_else(|| self.debug.threads.first().map(|t| t.0)).unwrap_or(1);
                    self.debug.request("pause", json!({"threadId": thread}), Pending::Other);
                }
            }
            Cmd::DebugStepOver => self.debug_step("next"),
            Cmd::DebugStepInto => self.debug_step("stepIn"),
            Cmd::DebugStepOut => self.debug_step("stepOut"),
            Cmd::DebugToggleBreakpoint => {
                if let Some(d) = self.docs.get_mut(self.active).filter(|d| d.path.is_some() && d.view == document::View::Text) {
                    let line = d.rope.char_to_line(d.primary_sel().head);
                    let on = d.toggle_breakpoint(line);
                    self.set_status(if on { tf!("● Point d'arrêt ligne {}", "● Breakpoint line {}", line + 1) } else { tf!("Point d'arrêt enlevé ligne {}", "Breakpoint taken away line {}", line + 1) });
                }
            }
            Cmd::DebugBreakpointCondition => {
                if let Some(d) = self.docs.get(self.active).filter(|d| d.path.is_some() && d.view == document::View::Text) {
                    let line = d.rope.char_to_line(d.primary_sel().head);
                    let uid = d.uid;
                    self.open_prompt(Prompt::BreakpointCondition(uid, line));
                }
            }
            Cmd::DebugWatchSelection => {
                let Some(d) = self.docs.get(self.active) else { return };
                let s = d.primary_sel();
                let expr = if s.is_empty() { d.symbol_at(s.head).map(|(a, b)| d.slice(a, b)).unwrap_or_default() } else { d.slice(s.start(), s.end()) };
                if expr.trim().is_empty() {
                    self.set_status(t!("Rien à surveiller : sélectionne une expression", "Nothing to watch: select an expression"));
                } else {
                    self.set_status(tf!("Surveillée : {}", "Watched: {}", expr.trim()));
                    self.debug_add_watch(expr);
                }
            }
            Cmd::DebugClearBreakpoints => {
                for d in self.docs.iter_mut().filter(|d| !d.breakpoints.is_empty()) {
                    d.breakpoints.clear();
                    d.breakpoints_changed = true;
                }
                let saved: Vec<PathBuf> = self.debug.saved.keys().cloned().collect();
                self.debug.saved.clear();
                self.debug.saved_conds.clear();
                if self.debug.session.is_some() {
                    for p in saved {
                        self.send_breakpoints(&p, &[], &HashMap::new());
                    }
                }
            }
            Cmd::DebugTest => {
                if self.debug.session.is_some() {
                    self.set_status(t!("Le débogueur tourne déjà (Maj+F5 pour l'arrêter)", "The debugger is already running (Shift+F5 to stop it)"));
                    return;
                }
                match self.debug_test_config() {
                    Ok((cfg, file)) => self.debug_launch(cfg, Some(file)),
                    Err(e) => self.set_error(e),
                }
            }
            Cmd::DebugShow => {
                self.debug_read_configs();
                self.show_bottom(BottomTab::Debug, false);
            }
            Cmd::DebugConfigs => {
                let Some(root) = self.root.clone() else {
                    self.set_status(t!("Ouvre d'abord un dossier", "Open a folder first"));
                    return;
                };
                let p = root.join(".florm").join("debug.toml");
                if !p.exists() {
                    let _ = std::fs::create_dir_all(root.join(".florm"));
                    if let Err(e) = std::fs::write(&p, dap::DEBUG_TOML_EXAMPLE) {
                        self.set_error(e.to_string());
                        return;
                    }
                }
                self.open_path(p);
            }
            Cmd::DebugInstallDebugpy => {
                self.set_status(t!("Installation de debugpy (pip)…", "Installing debugpy (pip)…"));
                self.log(format!("pip install --target {} debugpy", lint::tools_dir().display()));
                self.jobs.spawn(|| JobResult::ToolInstalled(lint::install_python_tool("debugpy")));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A whole session through the app, with debugpy (`cargo test -- --ignored`): F5 on a file
    /// with a breakpoint stops there, shows its line and its variables; F10 goes to the next line;
    /// F5 runs to the end.
    #[test]
    #[ignore]
    fn the_app_debugs_a_python_file() {
        let dir = std::env::temp_dir().join(format!("florm_debug_app_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.toml"), format!("check_updates = false\nlsp = false\ntrusted_folders = [{:?}]\n", dir.display().to_string())).unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let file = dir.join("prog.py");
        std::fs::write(&file, "def f(n):\n    total = n * 2\n    return total\n\nprint(f(21))\nprint('fin')\n").unwrap();
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(dir.clone()));
        app.open_path(file.clone());
        let i = app.active;
        app.docs[i].toggle_breakpoint(2);
        // started by a command typed as in a terminal
        app.debug.cmd_for = app.root.clone();
        app.debug.choice = "cmd".into();
        app.debug.cmd = "python3 prog.py".into();
        let pump = |app: &mut App, until: &dyn Fn(&App) -> bool| {
            let t = Instant::now();
            while !until(app) {
                assert!(t.elapsed() < Duration::from_secs(20), "console: {:?}", app.debug.console);
                app.process_jobs();
                app.debug_frame(&ctx);
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        app.debug_command(Cmd::DebugStart);
        pump(&mut app, &|a| a.debug.phase == Phase::Stopped && a.debug.vars.values().any(|v| v.iter().any(|x| x.name == "total")));
        assert_eq!(app.debug_marks(app.active), (true, Some(2)));
        let total = app.debug.vars.values().flatten().find(|v| v.name == "total").unwrap().value.clone();
        assert_eq!(total, "42");
        app.debug_command(Cmd::DebugStepOver);
        pump(&mut app, &|a| a.debug.phase == Phase::Stopped && a.debug.selected().is_some_and(|f| f.line != 2));
        app.debug_command(Cmd::DebugStart);
        pump(&mut app, &|a| a.debug.session.is_none());
        assert!(app.debug.console.iter().any(|(_, l)| l == "42"), "{:?}", app.debug.console);
        assert_eq!(app.debug_history(), [("python3 prog.py".to_string(), String::new())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The commands run stay in the history of their project: the last one is in the fields
    /// when the project opens again, the commands of another project are not shown.
    #[test]
    fn commands_are_remembered_per_project() {
        let dir = std::env::temp_dir().join(format!("florm_debug_hist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("odoo-bin"), "#!/usr/bin/env python3\n").unwrap();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(a.clone()));
        app.debug_prefill();
        // the command row shows from the start, even with no history
        assert!(app.debug.cmd.is_empty() && app.debug.choice == "cmd");
        for c in ["./odoo-bin -d test -u sale", "./odoo-bin -d test -u stock"] {
            app.debug.cmd = c.into();
            let cfg = app.debug_command_config().unwrap();
            assert_eq!(cfg.adapter, "python");
        }
        // another project: not its commands
        app.root = Some(b.clone());
        app.debug_prefill();
        assert!(app.debug.cmd.is_empty() && app.debug_history().is_empty());
        // back (as after a restart: the state read again from the disk)
        app.state = project::load_state();
        app.root = Some(a.clone());
        app.debug_prefill();
        assert_eq!((app.debug.cmd.as_str(), app.debug.choice.as_str()), ("./odoo-bin -d test -u stock", "cmd"));
        assert_eq!(app.debug_history().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A breakpoint for the adapter: its line (from 1), its condition, or the text of a logpoint
/// (`log: total = {total}`).
fn breakpoint_json(line: usize, cond: Option<&str>) -> Value {
    let mut bp = json!({"line": line + 1});
    match cond.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => match c.strip_prefix("log:") {
            Some(text) => bp["logMessage"] = json!(text.trim()),
            None => bp["condition"] = json!(c),
        },
        None => {}
    }
    bp
}

#[cfg(test)]
mod breakpoint_tests {
    use super::*;

    #[test]
    fn conditions_and_logpoints() {
        assert_eq!(breakpoint_json(4, None), json!({"line": 5}));
        assert_eq!(breakpoint_json(4, Some(" x > 3 ")), json!({"line": 5, "condition": "x > 3"}));
        assert_eq!(breakpoint_json(0, Some("log: total = {total}")), json!({"line": 1, "logMessage": "total = {total}"}));
    }
}
