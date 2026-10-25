//! The debugger, like VS Code's: breakpoints in the gutter (F9), the program paused there, its
//! variables, call stack and a console to evaluate Python in it, F10 / F11 / Maj+F11 to step.
//! For Odoo on this computer or in a Docker container (see dap.rs): F5 starts the program set in
//! the view, Alt+F6 (or a right click on the ▶ of a test) debugs the test under the cursor.

use super::*;
use crate::dap::{self, Incoming, Mode, Target};
use crate::project::SessionBreakpoint;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Program launched in a container when none is set: a second Odoo next to the one of the
/// container (its port is taken), in one process so that every request stops at the breakpoints.
pub(super) const DOCKER_COMMAND: &str = "odoo --workers 0 --http-port 8070";

/// Lines kept in the console.
const CONSOLE_MAX: usize = 5000;
/// Frames read from the call stack of a thread.
const STACK_LEVELS: usize = 60;

/// Files that can have breakpoints.
pub(crate) fn debuggable(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "py")
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Bp {
    /// 0-based.
    line: usize,
    condition: String,
    /// What the debugger said of it: verified, why not.
    state: Option<(bool, String)>,
}

impl Bp {
    fn new(line: usize) -> Self {
        Bp { line, condition: String::new(), state: None }
    }
}

/// The breakpoints of a file once its lines moved or changed: the conditions follow the lines
/// in order when they only moved, else stay on the lines that are still there.
fn merge_bps(old: &[Bp], lines: &[usize]) -> Vec<Bp> {
    if old.len() == lines.len() {
        return old.iter().zip(lines).map(|(b, &l)| Bp { line: l, ..b.clone() }).collect();
    }
    lines.iter().map(|&l| old.iter().find(|b| b.line == l).cloned().unwrap_or_else(|| Bp::new(l))).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Cat {
    Out,
    Err,
    Info,
    Input,
    Result,
}

struct Frame {
    id: i64,
    name: String,
    /// The file as the debugger sees it (in the container).
    remote: Option<String>,
    /// 1-based.
    line: usize,
    /// The file here (it may not exist: a file of the image of a container).
    local: Option<PathBuf>,
}

struct Scope {
    name: String,
    reference: i64,
}

#[derive(Clone)]
struct Var {
    name: String,
    value: String,
    ty: String,
    reference: i64,
    /// What to evaluate to find it again (the watch of a variable).
    expr: Option<String>,
}

enum EvalFor {
    Repl,
    Watch(String),
    /// A word of document `uid` under the mouse: its range, the expression.
    Hover(u64, (usize, usize), String),
}

/// What a request was for.
enum Req {
    Initialize,
    Start,
    Bps(PathBuf),
    Other,
    Threads,
    Stack(i64),
    Scopes(i64),
    /// The children of a variable (or scope): its reference and its path in the tree.
    Vars(i64, String),
    Eval(EvalFor),
    ExceptionInfo,
    Disconnect,
}

struct Paused {
    thread: i64,
    reason: String,
    text: String,
}

struct Session {
    client: dap::Client,
    cfg: dap::Config,
    title: String,
    request: &'static str,
    args: Value,
    mounts: Vec<(String, PathBuf)>,
    pending: HashMap<i64, Req>,
    /// Breakpoints sent (`initialized` came): the changes are sent as they happen.
    configured: bool,
    paused: Option<Paused>,
    threads: Vec<(i64, String)>,
    stacks: HashMap<i64, Vec<Frame>>,
    /// The frame shown: (thread, index in its stack).
    frame: Option<(i64, usize)>,
    scopes: Vec<Scope>,
    vars: HashMap<i64, Vec<Var>>,
    /// Value of each watched expression in the frame shown.
    watch: HashMap<String, Result<String, String>>,
    /// The hovered word whose value was asked.
    hover: Option<(u64, (usize, usize))>,
    /// Files of a container shown read only here (they are not on this computer): path there → document.
    remote_docs: HashMap<String, u64>,
    /// `disconnect` was sent at this moment.
    stopping: Option<Instant>,
    /// The exception the program is stopped on.
    exception: Option<String>,
    /// The program answered: attached (until then, Florm waits for it to listen).
    connected: bool,
    /// Started by itself (attach mode with breakpoints), not by F5.
    auto: bool,
}

#[derive(Default)]
pub(super) struct DebugUi {
    /// Breakpoints of the project, by file (kept in its session).
    bps: BTreeMap<PathBuf, Vec<Bp>>,
    /// Files whose breakpoints changed since they were sent.
    dirty: HashSet<PathBuf>,
    session: Option<Session>,
    /// A session is being prepared (its title).
    preparing: Option<String>,
    failure: Option<dap::Failure>,
    console: Vec<(Cat, String)>,
    repl: String,
    repl_hist: Vec<String>,
    repl_pos: Option<usize>,
    pub(super) focus_console: bool,
    watch: Vec<String>,
    watch_draft: String,
    /// Terminal tab of the program of the last session.
    term: Option<u64>,
    /// The last session started (Ctrl+Maj+F5 starts it again).
    last: Option<(dap::Config, String)>,
    /// Started once the session ends (restart, another test).
    next: Option<(dap::Config, String)>,
    /// Variables opened / closed in the tree, by path (they stay so from one stop to the next).
    open: HashSet<String>,
    shut: HashSet<String>,
    /// The odoo-bin of the folder: (folder, its command).
    odoo_bin: Option<(PathBuf, Option<String>)>,
    /// The running containers, and when they were read.
    pub(super) containers: Option<(Instant, Vec<String>)>,
    /// The settings of the session are unfolded (else one line sums them up).
    editing: bool,
    containers_asked: Option<Instant>,
    /// Stopped by hand (Shift+F5): Florm no longer waits for the program by itself, until F5.
    auto_off: bool,
    /// The session being prepared was started by itself.
    preparing_auto: bool,
    /// Number of the session being prepared: the result of an earlier one is left aside.
    prep_id: u64,
    /// Not before: the waiting failed (container stopped…), it is tried again a bit later.
    auto_retry: Option<Instant>,
    /// The last message of the waiting, written once (not at each try).
    auto_note: Option<String>,
}

/// Time before waiting again for the program after a failure (container not running…).
const AUTO_RETRY: Duration = Duration::from_secs(3);

/// Something clicked in the view, done once it is drawn.
enum DebugAct {
    Start,
    Stop,
    Restart,
    Step(&'static str),
    Toggle(String, i64, bool),
    Frame(i64, usize),
    LoadStack(i64),
    Open(PathBuf, usize),
    RemoveBp(PathBuf, usize),
    ConditionChanged(PathBuf),
    ClearBps,
    Watch(String),
    Unwatch(usize),
    Eval(String),
    Copy(String),
    Install(Vec<String>),
    Exceptions,
    Target(String),
    Move(bool),
}

/// Open by default: the first scope (the local variables), not the others nor the variables.
fn is_open(open: &HashSet<String>, shut: &HashSet<String>, path: &str, default: bool) -> bool {
    open.contains(path) || (default && !shut.contains(path))
}

fn parse_frames(body: &Value, target: &Target, mounts: &[(String, PathBuf)]) -> Vec<Frame> {
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

fn parse_vars(body: &Value, parent_expr: Option<&str>) -> Vec<Var> {
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
    pub(super) fn open_debug(&mut self) {
        if self.bottom == Some(BottomTab::Debug) {
            self.bottom = None;
        }
        self.open_view_tab(document::View::Debug, t!("Débogage", "Debug").into());
    }

    /// The debug view on screen (in the bottom panel when it is not in a tab).
    fn debug_show(&mut self) {
        if !self.docs.iter().any(|d| d.view == document::View::Debug) && self.bottom != Some(BottomTab::Debug) {
            self.bottom = Some(BottomTab::Debug);
        }
    }

    /// Paused: the view under the file of the breakpoint, in the bottom panel. In an editor tab,
    /// the file would hide it (same panel), unless the tab is in the other panel of a split.
    fn debug_show_paused(&mut self) {
        let beside = self.is_split() && self.docs.iter().any(|d| d.view == document::View::Debug && d.pane != self.focused_pane);
        if !beside {
            self.show_bottom(BottomTab::Debug, false);
        }
    }

    fn debug_note(&mut self, cat: Cat, text: impl Into<String>) {
        let c = &mut self.debug.console;
        for l in text.into().lines() {
            c.push((cat, l.to_string()));
        }
        if c.len() > CONSOLE_MAX {
            c.drain(..c.len() - CONSOLE_MAX);
        }
    }

    // ------------------------------------------------------------ breakpoints

    pub(super) fn debug_session_breakpoints(&self) -> Vec<SessionBreakpoint> {
        self.debug.bps.iter().flat_map(|(p, v)| v.iter().map(|b| SessionBreakpoint { path: p.clone(), line: b.line, condition: b.condition.clone() })).collect()
    }

    pub(super) fn debug_set_breakpoints(&mut self, list: Vec<SessionBreakpoint>) {
        self.debug.bps.clear();
        for b in list {
            let v = self.debug.bps.entry(b.path).or_default();
            if !v.iter().any(|x| x.line == b.line) {
                v.push(Bp { line: b.line, condition: b.condition, state: None });
            }
        }
        for v in self.debug.bps.values_mut() {
            v.sort_by_key(|b| b.line);
        }
    }

    /// The breakpoints of the open files follow their edits; a click in the gutter (or F9) adds
    /// or removes one.
    fn debug_sync_bps(&mut self) {
        let ui = &mut self.debug;
        for d in self.docs.iter_mut().filter(|d| d.view == document::View::Text) {
            let Some(path) = d.path.clone().filter(|p| debuggable(p)) else { continue };
            if d.breakpoints.is_none() {
                let lines: Vec<usize> = ui.bps.get(&path).map(|v| v.iter().map(|b| b.line).collect()).unwrap_or_default();
                d.set_breakpoint_lines(&lines);
            }
            let mut lines = d.breakpoint_lines().unwrap_or_default();
            if let Some(l) = d.bp_pick.take() {
                match lines.iter().position(|&x| x == l) {
                    Some(k) => {
                        lines.remove(k);
                    }
                    None => lines.push(l),
                }
                lines.sort_unstable();
                d.set_breakpoint_lines(&lines);
                lines = d.breakpoint_lines().unwrap_or_default();
            }
            let old = ui.bps.get(&path).map_or(&[][..], |v| &v[..]);
            if old.len() == lines.len() && old.iter().zip(&lines).all(|(b, l)| b.line == *l) {
                continue;
            }
            let new = merge_bps(old, &lines);
            if new.is_empty() {
                ui.bps.remove(&path);
            } else {
                ui.bps.insert(path.clone(), new);
            }
            ui.dirty.insert(path);
        }
    }

    pub(super) fn debug_toggle_breakpoint(&mut self) {
        let Some(d) = self.docs.get_mut(self.active).filter(|d| d.view == document::View::Text) else { return };
        if !d.path.as_deref().is_some_and(debuggable) {
            self.set_status(t!("Points d'arrêt : dans les fichiers Python", "Breakpoints: in Python files"));
            return;
        }
        d.bp_pick = Some(d.rope.char_to_line(d.primary_sel().head.min(d.rope.len_chars())));
    }

    pub(super) fn debug_clear_breakpoints(&mut self) {
        let paths: Vec<PathBuf> = self.debug.bps.keys().cloned().collect();
        self.debug.dirty.extend(paths);
        self.debug.bps.clear();
        for d in self.docs.iter_mut().filter(|d| d.breakpoints.is_some()) {
            d.set_breakpoint_lines(&[]);
        }
        self.set_status(t!("Points d'arrêt retirés", "Breakpoints removed"));
    }

    fn debug_remove_bp(&mut self, path: PathBuf, line: usize) {
        if let Some(v) = self.debug.bps.get_mut(&path) {
            v.retain(|b| b.line != line);
            if v.is_empty() {
                self.debug.bps.remove(&path);
            }
        }
        let lines: Vec<usize> = self.debug.bps.get(&path).map(|v| v.iter().map(|b| b.line).collect()).unwrap_or_default();
        if let Some(d) = self.docs.iter_mut().find(|d| d.path.as_ref() == Some(&path) && d.breakpoints.is_some()) {
            d.set_breakpoint_lines(&lines);
        }
        self.debug.dirty.insert(path);
    }

    fn debug_send_bps(&mut self, path: &Path) {
        let list = self.debug.bps.get(path).cloned().unwrap_or_default();
        let Some(s) = self.debug.session.as_mut().filter(|s| s.configured) else { return };
        let remote = dap::remote_path(path, &s.cfg.target, &s.mounts);
        let bps: Vec<Value> = list
            .iter()
            .map(|b| {
                let mut v = json!({ "line": b.line + 1 });
                if !b.condition.trim().is_empty() {
                    v["condition"] = json!(b.condition.trim());
                }
                v
            })
            .collect();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let seq = s.client.request("setBreakpoints", json!({ "source": { "path": remote, "name": name }, "breakpoints": bps }));
        s.pending.insert(seq, Req::Bps(path.to_path_buf()));
    }

    fn debug_send_exceptions(&mut self) {
        let filters: Vec<String> = self.settings.debug_exceptions.iter().filter(|f| ["raised", "uncaught", "userUnhandled"].contains(&f.as_str())).cloned().collect();
        let Some(s) = self.debug.session.as_mut() else { return };
        let seq = s.client.request("setExceptionBreakpoints", json!({ "filters": filters }));
        s.pending.insert(seq, Req::Other);
    }

    // ------------------------------------------------------------ what is started

    fn debug_odoo_bin(&mut self) -> Option<String> {
        let root = self.root.clone()?;
        if self.debug.odoo_bin.as_ref().is_none_or(|(r, _)| *r != root) {
            self.debug.odoo_bin = Some((root.clone(), dap::find_odoo_bin(&root)));
        }
        self.debug.odoo_bin.as_ref().and_then(|(_, c)| c.clone())
    }

    /// The program launched here when none is set: the odoo-bin of the folder.
    fn debug_default_local(&mut self) -> String {
        self.debug_odoo_bin().map(|b| format!("{b} --workers 0")).unwrap_or_default()
    }

    fn debug_local_command(&mut self) -> String {
        let c = self.settings.debug_command.trim().to_string();
        if c.is_empty() { self.debug_default_local() } else { c }
    }

    fn debug_config(&mut self) -> dap::Config {
        let target = if self.settings.debug_target.is_empty() { Target::Local } else { Target::Docker(self.settings.debug_target.clone()) };
        let command = match &target {
            Target::Local => self.debug_local_command(),
            Target::Docker(_) => Some(self.settings.debug_docker_command.trim()).filter(|c| !c.is_empty()).unwrap_or(DOCKER_COMMAND).to_string(),
        };
        let s = &self.settings;
        dap::Config {
            target,
            mode: if s.debug_mode == "attach" { Mode::Attach } else { Mode::Launch },
            command,
            cwd: self.root.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            python: Some(s.debug_python.trim()).filter(|p| !p.is_empty()).unwrap_or(if cfg!(windows) { "python" } else { "python3" }).to_string(),
            host: "127.0.0.1".into(),
            port: s.debug_port,
            just_my_code: s.debug_just_my_code,
            // attach: Florm waits for the program to listen (it may be started after)
            wait: s.debug_mode == "attach",
        }
    }

    fn debug_title(cfg: &dap::Config) -> String {
        let place = match &cfg.target {
            Target::Local => String::new(),
            Target::Docker(c) => format!(" ({c})"),
        };
        match cfg.mode {
            Mode::Attach => tf!("attaché au port {}{place}", "attached to port {}{place}", cfg.port),
            Mode::Launch => {
                let words = crate::odoo_test::split(&cfg.command);
                let prog = words.iter().find(|w| !w.starts_with('-') && !w.starts_with("python")).map_or("?", |w| w.rsplit('/').next().unwrap_or(w));
                format!("{prog}{place}")
            }
        }
    }

    /// F5: continues when paused, else starts the program set in the view.
    pub(super) fn debug_start_or_continue(&mut self) {
        match &self.debug.session {
            Some(s) if s.paused.is_some() => self.debug_step("continue"),
            Some(s) if s.connected => self.set_status(t!("Le programme tourne (F6 : pause, Maj+F5 : arrêter)", "The program is running (F6: pause, Shift+F5: stop)")),
            None if self.debug.preparing.is_some() && !self.debug.preparing_auto => {}
            // nothing yet, or only waiting for a program: F5 starts the one of the view
            _ => {
                self.debug.auto_off = false;
                self.debug.auto_retry = None;
                let cfg = self.debug_config();
                let title = Self::debug_title(&cfg);
                self.debug_launch(cfg, title);
            }
        }
    }

    /// The Odoo test `tags` under the debugger: in the container of the debugger, else in the one
    /// of the tests when there is no odoo-bin here.
    pub(super) fn debug_test(&mut self, tags: String) {
        let mut cfg = self.debug_config();
        cfg.mode = Mode::Launch;
        if cfg.target == Target::Local && cfg.command.trim().is_empty() && !self.settings.odoo_test_container.is_empty() {
            cfg.target = Target::Docker(self.settings.odoo_test_container.clone());
        }
        let quoted = if tags.contains(char::is_whitespace) { format!("'{tags}'") } else { tags.clone() };
        cfg.command = match &cfg.target {
            Target::Docker(_) => {
                let t = Some(self.settings.odoo_test_command.trim()).filter(|t| !t.is_empty()).unwrap_or(crate::odoo_test::DEFAULT_COMMAND);
                t.replace("{tags}", &quoted)
            }
            Target::Local if cfg.command.trim().is_empty() => String::new(),
            Target::Local => format!("{} --stop-after-init --test-enable --test-tags {quoted}", cfg.command),
        };
        let title = tf!("Test {tags}", "Test {tags}");
        if self.debug.session.is_some() {
            self.debug.next = Some((cfg, title));
            self.debug_stop();
            return;
        }
        self.debug_launch(cfg, title);
    }

    fn debug_launch(&mut self, cfg: dap::Config, title: String) {
        self.debug_launch_as(cfg, title, false);
    }

    /// `auto`: Florm waits for the program by itself: the view is not opened, and a failure is
    /// tried again later without an error each time.
    fn debug_launch_as(&mut self, cfg: dap::Config, title: String, auto: bool) {
        // asked by hand (F5, a test): the waiting for a program gives way
        if !auto {
            if self.debug.preparing_auto {
                self.debug.preparing = None;
                self.debug.preparing_auto = false;
            }
            if self.debug.session.as_ref().is_some_and(|s| s.auto && !s.connected) {
                self.debug.session = None;
            }
        }
        if self.debug.session.is_some() || self.debug.preparing.is_some() {
            return;
        }
        self.debug.prep_id += 1;
        let id = self.debug.prep_id;
        self.debug.preparing = Some(title.clone());
        self.debug.preparing_auto = auto;
        self.debug.last = Some((cfg.clone(), title.clone()));
        if !auto {
            self.debug.failure = None;
            self.debug_note(Cat::Info, tf!("── {title} ──", "── {title} ──"));
            self.set_status(tf!("Débogage de {title}…", "Debugging {title}…"));
            self.debug_show();
        }
        self.jobs.spawn(move || JobResult::DebugReady { id, result: dap::prepare(&cfg), cfg, title });
    }

    /// A breakpoint set: Florm waits by itself (no F5, whatever the mode) for a program started
    /// with debugpy (a terminal, a container), attaches to it, and waits again after each run of
    /// it, until it is stopped by hand.
    fn debug_auto(&mut self) {
        let d = &self.debug;
        let want = !d.auto_off && d.bps.values().any(|v| !v.is_empty());
        if !want {
            // nothing to wait for any more (breakpoints removed): the waiting stops
            if d.session.as_ref().is_some_and(|s| s.auto && !s.connected) {
                self.debug.session = None;
            }
            if self.debug.preparing_auto {
                self.debug.preparing = None;
                self.debug.preparing_auto = false;
            }
            return;
        }
        if d.session.is_some() || d.preparing.is_some() {
            return;
        }
        if let Some(t) = d.auto_retry {
            if t > Instant::now() {
                self.egui_ctx.request_repaint_after(t - Instant::now());
                return;
            }
        }
        self.debug.auto_retry = None;
        let mut cfg = self.debug_config();
        cfg.mode = Mode::Attach;
        cfg.wait = true;
        let title = Self::debug_title(&cfg);
        self.debug_launch_as(cfg, title, true);
    }

    /// A message of the waiting, written once.
    fn debug_auto_note(&mut self, cat: Cat, text: String) {
        if self.debug.auto_note.as_deref() != Some(text.as_str()) {
            self.debug.auto_note = Some(text.clone());
            self.debug_note(cat, text);
        }
    }

    /// The program answered: the debugger is attached and stops at the breakpoints.
    fn debug_connected(&mut self) {
        let Some(s) = self.debug.session.as_mut() else { return };
        s.connected = true;
        let title = s.title.clone();
        self.debug.failure = None;
        self.debug.auto_note = None;
        self.debug_note(Cat::Info, tf!("Connecté : {title}", "Connected: {title}"));
        self.set_status(tf!("Débogueur connecté ({title}) : il s'arrête aux points d'arrêt", "Debugger connected ({title}): it stops at the breakpoints"));
    }

    /// The command that starts the program so that Florm can attach to it (to copy into a
    /// terminal, or into the command of the container).
    fn debug_listen_command(&mut self) -> String {
        let port = self.settings.debug_port;
        if self.settings.debug_target.is_empty() {
            let cmd = self.debug_local_command();
            let cmd = if cmd.is_empty() { "./odoo-bin --workers 0".to_string() } else { cmd };
            let python = Some(self.settings.debug_python.trim()).filter(|p| !p.is_empty()).unwrap_or(if cfg!(windows) { "python" } else { "python3" }).to_string();
            format!("{python} -m debugpy --listen {port} {cmd}")
        } else {
            format!("python3 -m debugpy --listen 0.0.0.0:{port} /usr/bin/odoo --workers 0")
        }
    }

    pub(super) fn debug_ready(&mut self, id: u64, cfg: dap::Config, title: String, result: Result<dap::Prepared, dap::Failure>) {
        // stopped meanwhile, or replaced by another one
        if id != self.debug.prep_id || self.debug.preparing.take().is_none() {
            return;
        }
        let auto = std::mem::take(&mut self.debug.preparing_auto);
        let p = match result {
            Ok(p) => p,
            Err(f) => {
                let msg = match &f {
                    dap::Failure::NoDebugpy { place, .. } => tf!("debugpy n'est pas installé {place}", "debugpy is not installed {place}"),
                    dap::Failure::Other(e) => e.clone(),
                };
                if auto {
                    // the container is maybe not started yet: tried again in a moment
                    self.debug_auto_note(Cat::Err, msg);
                    self.debug.failure = Some(f);
                    self.debug.auto_retry = Some(Instant::now() + AUTO_RETRY);
                    self.egui_ctx.request_repaint_after(AUTO_RETRY);
                    return;
                }
                self.debug_note(Cat::Err, msg.clone());
                self.set_error(tf!("Débogage impossible : {msg}", "Cannot debug: {msg}"));
                self.debug.failure = Some(f);
                self.debug.next = None;
                return;
            }
        };
        let ctx = self.egui_ctx.clone();
        let repaint = move || ctx.request_repaint();
        let client = match &p.transport {
            dap::Transport::Spawn { argv, cwd } => dap::Client::spawn(argv, cwd.as_deref(), repaint),
            dap::Transport::Tcp(host, port) => dap::Client::connect(host, *port, repaint),
        };
        let mut client = match client {
            Ok(c) => c,
            Err(e) => {
                let e = if cfg.mode == Mode::Attach { tf!("{e} (le programme écoute-t-il avec debugpy --listen ?)", "{e} (is the program listening with debugpy --listen?)") } else { e };
                if auto {
                    self.debug_auto_note(Cat::Err, e);
                    self.debug.auto_retry = Some(Instant::now() + AUTO_RETRY);
                    self.egui_ctx.request_repaint_after(AUTO_RETRY);
                    return;
                }
                self.debug_note(Cat::Err, e.clone());
                self.set_error(tf!("Débogage impossible : {e}", "Cannot debug: {e}"));
                return;
            }
        };
        let seq = client.request(
            "initialize",
            json!({
                "clientID": "florm", "clientName": "Florm", "adapterID": "debugpy", "locale": if crate::i18n::en() { "en" } else { "fr" },
                "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path",
                "supportsVariableType": true, "supportsRunInTerminalRequest": true, "supportsProgressReporting": false,
            }),
        );
        let mut pending = HashMap::new();
        pending.insert(seq, Req::Initialize);
        if cfg.wait {
            let place = match &cfg.target {
                Target::Local => String::new(),
                Target::Docker(c) => tf!(" dans {c}", " in {c}"),
            };
            let port = cfg.port;
            self.debug_auto_note(Cat::Info, tf!("En attente du programme sur le port {port}{place}…", "Waiting for the program on port {port}{place}…"));
        }
        self.debug.session = Some(Session {
            client,
            title,
            request: p.request,
            args: p.args,
            mounts: p.mounts,
            pending,
            configured: false,
            paused: None,
            threads: Vec::new(),
            stacks: HashMap::new(),
            frame: None,
            scopes: Vec::new(),
            vars: HashMap::new(),
            watch: HashMap::new(),
            hover: None,
            remote_docs: HashMap::new(),
            stopping: None,
            exception: None,
            connected: false,
            auto,
            cfg,
        });
    }

    /// Shift+F5, ■: Florm no longer waits for the program by itself (F5 waits again).
    pub(super) fn debug_stop_by_hand(&mut self) {
        self.debug.auto_off = true;
        self.debug_stop();
    }

    pub(super) fn debug_stop(&mut self) {
        if self.debug.preparing.take().is_some() {
            self.set_status(t!("Débogage annulé", "Debugging cancelled"));
            return;
        }
        // still waiting for the program: nothing to say to it
        if self.debug.session.as_ref().is_some_and(|s| !s.connected) {
            self.debug.session = None;
            self.debug.auto_note = None;
            self.set_status(t!("Attente du programme arrêtée (F5 pour attendre à nouveau)", "Waiting for the program stopped (F5 to wait again)"));
            if let Some((cfg, title)) = self.debug.next.take() {
                self.debug_launch(cfg, title);
            }
            return;
        }
        let Some(s) = self.debug.session.as_mut() else { return };
        if s.stopping.is_some() {
            self.debug_end(t!("Débogage arrêté", "Debugging stopped").into());
            return;
        }
        let launched = s.request == "launch";
        let seq = s.client.request("disconnect", json!({ "terminateDebuggee": launched, "restart": false }));
        s.pending.insert(seq, Req::Disconnect);
        s.stopping = Some(Instant::now());
    }

    pub(super) fn debug_restart(&mut self) {
        match (&self.debug.session, self.debug.last.clone()) {
            (Some(_), last) => {
                self.debug.next = last;
                self.debug.auto_off = false;
                self.debug_stop();
            }
            (None, Some((cfg, title))) => self.debug_launch(cfg, title),
            (None, None) => self.debug_start_or_continue(),
        }
    }

    /// The session is over (the program ended, or it was stopped).
    fn debug_end(&mut self, msg: String) {
        let Some(s) = self.debug.session.take() else { return };
        let waiting = !s.connected && s.cfg.wait;
        drop(s);
        if waiting {
            // the waiting ended without the program (container stopped…): tried again in a moment
            self.debug.auto_retry = Some(Instant::now() + AUTO_RETRY);
            self.egui_ctx.request_repaint_after(AUTO_RETRY);
            if let Some((cfg, title)) = self.debug.next.take() {
                self.debug_launch(cfg, title);
            }
            return;
        }
        for d in &mut self.docs {
            d.debug_value = None;
        }
        self.debug_note(Cat::Info, msg.clone());
        self.set_status(msg);
        if let Some((cfg, title)) = self.debug.next.take() {
            self.debug_launch(cfg, title);
        }
    }

    /// Continue, pause and the steps: `continue`, `pause`, `next`, `stepIn`, `stepOut`.
    pub(super) fn debug_step(&mut self, cmd: &'static str) {
        let Some(s) = self.debug.session.as_mut() else {
            if cmd == "continue" {
                self.debug_start_or_continue();
            } else {
                self.set_status(t!("Pas de débogage en cours (F5 pour démarrer)", "No debugging session (F5 to start)"));
            }
            return;
        };
        let thread = match (&s.paused, cmd) {
            (Some(p), "pause") => {
                let _ = p;
                self.set_status(t!("Déjà en pause", "Already paused"));
                return;
            }
            (None, "pause") => s.threads.first().map_or(0, |t| t.0),
            (Some(p), _) => p.thread,
            (None, _) => {
                self.set_status(t!("Le programme n'est pas en pause", "The program is not paused"));
                return;
            }
        };
        let seq = s.client.request(cmd, json!({ "threadId": thread }));
        s.pending.insert(seq, Req::Other);
        if cmd != "pause" {
            self.debug_resumed();
        }
    }

    /// The program runs again: nothing paused to show.
    fn debug_resumed(&mut self) {
        let Some(s) = self.debug.session.as_mut() else { return };
        s.paused = None;
        s.stacks.clear();
        s.frame = None;
        s.scopes.clear();
        s.vars.clear();
        s.watch.clear();
        s.hover = None;
        s.exception = None;
        for d in &mut self.docs {
            d.debug_value = None;
        }
    }

    // ------------------------------------------------------------ what the adapter says

    /// Every frame: the messages of the adapter, the breakpoints changed, the hovered value.
    pub(super) fn debug_frame(&mut self) {
        self.debug_sync_bps();
        self.debug_auto();
        let Some(s) = &self.debug.session else { return };
        for m in s.client.poll() {
            // the first answer of the adapter: the program listens, the debugger is attached
            if matches!(m, Incoming::Response { .. } | Incoming::Event { .. }) && self.debug.session.as_ref().is_some_and(|s| !s.connected) {
                self.debug_connected();
            }
            self.debug_handle(m);
        }
        if self.debug.session.as_ref().is_some_and(|s| s.configured) && !self.debug.dirty.is_empty() {
            for p in std::mem::take(&mut self.debug.dirty) {
                self.debug_send_bps(&p);
            }
        }
        if self.debug.session.as_ref().and_then(|s| s.stopping).is_some_and(|t| t.elapsed() > Duration::from_secs(3)) {
            self.debug_end(t!("Débogage arrêté", "Debugging stopped").into());
        }
        self.debug_hover();
    }

    fn debug_handle(&mut self, m: Incoming) {
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

    fn debug_response(&mut self, req: Req, command: &str, success: bool, message: String, body: Value) {
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

    fn debug_event(&mut self, event: &str, body: Value) {
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
    fn debug_run_in_terminal(&mut self, seq: i64, args: Value) {
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

    fn debug_close_term(&mut self) {
        let Some(old) = self.debug.term.take() else { return };
        match self.docs.iter().position(|d| d.view == document::View::Terminal(old)) {
            Some(i) => self.close_tab(i, true),
            None => self.term.close(old),
        }
    }

    /// Shows frame `k` of `thread`: its line in the editor, its variables, the watches.
    fn debug_select_frame(&mut self, thread: i64, k: usize) {
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

    pub(super) fn debug_source(&mut self, remote: String, line: usize, result: Result<String, String>) {
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

    /// Where the debugger is paused in document `i`, for the editor.
    pub(super) fn debug_exec_line(&self, i: usize) -> Option<(usize, bool)> {
        let s = self.debug.session.as_ref()?;
        s.paused.as_ref()?;
        let (t, k) = s.frame?;
        let f = s.stacks.get(&t)?.get(k)?;
        let d = self.docs.get(i)?;
        let here = match (&f.local, &d.path) {
            (Some(l), Some(p)) if l == p => true,
            _ => f.remote.as_ref().is_some_and(|r| s.remote_docs.get(r) == Some(&d.uid)),
        };
        here.then(|| (f.line.saturating_sub(1), k == 0))
    }

    /// Paused: the value of the word under the mouse (with what leads to it: `self.partner_id`).
    fn debug_hover(&mut self) {
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

    fn debug_eval(&mut self, expr: String) {
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

    fn debug_watch_eval(&mut self, e: &str) {
        let Some(s) = self.debug.session.as_mut() else { return };
        let Some(f) = s.frame.and_then(|(t, k)| s.stacks.get(&t)?.get(k)).map(|f| f.id) else { return };
        let seq = s.client.request("evaluate", json!({ "expression": e, "frameId": f, "context": "watch" }));
        s.pending.insert(seq, Req::Eval(EvalFor::Watch(e.to_string())));
    }

    // ------------------------------------------------------------ the view

    pub(super) fn debug_ui(&mut self, ui: &mut egui::Ui) {
        let mut act: Option<DebugAct> = None;
        let fill = ui.visuals().panel_fill;
        let weak = ui.visuals().weak_text_color();
        let big = self.docs.iter().any(|d| d.view == document::View::Debug);
        egui::TopBottomPanel::top("debug_bar")
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 6.0, bottom: 6.0 }))
            .show_inside(ui, |ui| self.debug_bar(ui, big, &mut act));
        egui::SidePanel::left("debug_side")
            .resizable(true)
            .default_width(360.0)
            .width_range(220.0..=900.0)
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 8.0, right: 8.0, top: 6.0, bottom: 6.0 }))
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.debug_side(ui, weak, &mut act));
            });
        egui::CentralPanel::default().frame(egui::Frame::none().fill(self.hl.colors.bg).inner_margin(egui::Margin::same(6.0))).show_inside(ui, |ui| self.debug_console(ui, &mut act));
        if let Some(a) = act {
            self.debug_act(a);
        }
    }

    fn debug_bar(&mut self, ui: &mut egui::Ui, big: bool, act: &mut Option<DebugAct>) {
        let weak = ui.visuals().weak_text_color();
        ui.horizontal(|ui| {
            crate::view_bar::title(ui, t!("Débogage", "Debug"));
            ui.add_space(6.0);
            let attach = self.settings.debug_mode == "attach";
            let listen = Some(self.debug_listen_command());
            let live = self.debug.session.as_ref();
            let paused = live.is_some_and(|s| s.paused.is_some());
            match live {
                None => {
                    if self.debug.preparing.is_some() {
                        spinner(ui);
                        if ui.button(t!("■ Annuler", "■ Cancel")).clicked() {
                            *act = Some(DebugAct::Stop);
                        }
                    } else if attach {
                        if ui.add(filled_button(t!("▶ Attendre le programme", "▶ Wait for the program"), self.hl.colors.accent)).on_hover_text(t!("Attendre que le programme écoute avec debugpy, puis s'y attacher (F5)", "Wait for the program to listen with debugpy, then attach to it (F5)")).clicked() {
                            *act = Some(DebugAct::Start);
                        }
                    } else if ui.add(filled_button(t!("▶ Démarrer", "▶ Start"), self.hl.colors.accent)).on_hover_text(t!("Lancer le programme sous le débogueur (F5)", "Run the program under the debugger (F5)")).clicked() {
                        *act = Some(DebugAct::Start);
                    }
                    if !self.debug.bps.values().any(|v| !v.is_empty()) {
                        ui.label(RichText::new(t!("Pose un point d'arrêt (F9) : Florm attend tout seul le programme lancé avec debugpy.", "Put a breakpoint (F9): Florm waits by itself for the program started with debugpy.")).small().color(weak));
                    } else if self.debug.auto_off {
                        ui.label(RichText::new(t!("Attente arrêtée à la main (F5 pour reprendre)", "Waiting stopped by hand (F5 to resume)")).small().color(weak));
                    }
                }
                Some(s) if !s.connected => {
                    // waiting for a program started with debugpy; F5 can still launch the one of the view
                    if !attach && ui.add(filled_button(t!("▶ Démarrer", "▶ Start"), self.hl.colors.accent)).on_hover_text(t!("Lancer le programme sous le débogueur (F5) au lieu d'attendre", "Run the program under the debugger (F5) instead of waiting")).clicked() {
                        *act = Some(DebugAct::Start);
                    }
                    spinner(ui);
                    if ui.button(t!("■ Arrêter l'attente", "■ Stop waiting")).on_hover_text(t!("Ne plus attendre le programme (F5 pour attendre à nouveau)", "No longer wait for the program (F5 to wait again)")).clicked() {
                        *act = Some(DebugAct::Stop);
                    }
                }
                Some(_) => {
                    let b = |ui: &mut egui::Ui, label: &str, tip: &str, enabled: bool| ui.add_enabled(enabled, egui::Button::new(label)).on_hover_text(tip).on_disabled_hover_text(tip).clicked();
                    if paused {
                        if b(ui, t!("▶ Continuer", "▶ Continue"), t!("Continuer jusqu'au prochain point d'arrêt (F5)", "Continue to the next breakpoint (F5)"), true) {
                            *act = Some(DebugAct::Step("continue"));
                        }
                    } else if b(ui, "‖ Pause", t!("Mettre le programme en pause là où il est (F6)", "Pause the program where it is (F6)"), true) {
                        *act = Some(DebugAct::Step("pause"));
                    }
                    for (label, cmd, tip) in [
                        ("↷", "next", t!("Pas à pas principal : la ligne suivante, sans entrer dans les appels (F10)", "Step over: the next line, without entering the calls (F10)")),
                        ("↓", "stepIn", t!("Pas à pas détaillé : entrer dans l'appel (F11)", "Step into: enter the call (F11)")),
                        ("↑", "stepOut", t!("Pas à pas sortant : finir la fonction (Maj+F11)", "Step out: finish the function (Shift+F11)")),
                    ] {
                        if b(ui, label, tip, paused) {
                            *act = Some(DebugAct::Step(cmd));
                        }
                    }
                    if b(ui, "⟳", t!("Redémarrer (Ctrl+Maj+F5)", "Restart (Ctrl+Shift+F5)"), true) {
                        *act = Some(DebugAct::Restart);
                    }
                    if b(ui, t!("■ Arrêter", "■ Stop"), t!("Arrêter le débogage (Maj+F5)", "Stop debugging (Shift+F5)"), true) {
                        *act = Some(DebugAct::Stop);
                    }
                }
            }
            if let Some(s) = live {
                let state = match &s.paused {
                    Some(p) => {
                        let mut t = tf!("{} : en pause, {}", "{}: paused, {}", s.title, reason_label(&p.reason));
                        if !p.text.is_empty() && p.reason != "exception" {
                            t.push_str(&format!(" ({})", p.text));
                        }
                        RichText::new(t).color(highlight::yellow())
                    }
                    None if s.stopping.is_some() => RichText::new(t!("arrêt…", "stopping…")).color(weak),
                    None if !s.connected => RichText::new(tf!("en attente du programme (port {})…", "waiting for the program (port {})…", s.cfg.port)).color(weak),
                    None => RichText::new(tf!("{} : en cours", "{}: running", s.title)).color(highlight::green()),
                };
                ui.label(state);
            } else if let Some(t) = &self.debug.preparing {
                ui.label(RichText::new(tf!("préparation de {t}…", "preparing {t}…")).color(weak));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(crate::view_bar::BarAct::Move(b)) = crate::view_bar::controls(ui, big, None, None) {
                    *act = Some(DebugAct::Move(b));
                }
                // attach: the command that starts the program under debugpy, to paste in a terminal
                if let Some(cmd) = listen.filter(|_| !self.debug.session.as_ref().is_some_and(|s| s.connected)) {
                    if ui.button(t!("Copier la commande", "Copy the command")).on_hover_text(tf!("{cmd}\n\nÀ lancer dans un terminal (celui de Florm ou le tien) ; dans un conteneur, comme commande d'Odoo (command: du docker-compose.yml).", "{cmd}\n\nTo run in a terminal (Florm's or yours); in a container, as the command of Odoo (command: of docker-compose.yml).")).clicked() {
                        *act = Some(DebugAct::Copy(cmd));
                    }
                }
            });
        });
        if let Some(e) = self.debug.session.as_ref().and_then(|s| s.exception.clone()) {
            ui.label(RichText::new(format!("⚠ {e}")).color(highlight::red()).monospace());
        }
        // what is debugged: only between two sessions
        let locked = self.debug.session.is_some() || self.debug.preparing.is_some();
        // folded into one line: where, how, what (Modifier… unfolds it)
        let summary = self.debug_summary();
        ui.horizontal(|ui| {
            ui.label(RichText::new(summary).small().monospace().color(ui.visuals().weak_text_color()));
            if !locked {
                let label = if self.debug.editing { t!("✔ Fermer", "✔ Close") } else { t!("Modifier…", "Edit…") };
                if ui.small_button(label).on_hover_text(t!("Où le programme tourne, lancer ou s'attacher, la commande", "Where the program runs, launch or attach, the command")).clicked() {
                    self.debug.editing = !self.debug.editing;
                }
            }
        });
        if self.debug.editing && !locked {
            self.debug_settings_row(ui, act);
        }
        match &self.debug.failure {
            Some(dap::Failure::NoDebugpy { install, place }) => {
                let (install, place) = (install.clone(), place.clone());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tf!("debugpy n'est pas installé {place} : il fait le lien entre Python et le débogueur.", "debugpy is not installed {place}: it links Python to the debugger.")).color(highlight::red()));
                    if ui.button(t!("Installer debugpy", "Install debugpy")).on_hover_text(install.join(" ")).clicked() {
                        *act = Some(DebugAct::Install(install));
                    }
                });
            }
            Some(dap::Failure::Other(e)) => {
                ui.label(RichText::new(e).color(highlight::red()));
            }
            None => {}
        }
    }

    fn debug_settings_row(&mut self, ui: &mut egui::Ui, act: &mut Option<DebugAct>) {
        let weak = ui.visuals().weak_text_color();
        let here = t!("Cet ordinateur", "This computer").to_string();
        let containers = if crate::docker::available() { self.debug_containers() } else { Some(Vec::new()) };
        let target = self.settings.debug_target.clone();
        let mode_attach = self.settings.debug_mode == "attach";
        let default_local = self.debug_default_local();
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("Où", "Where")).small().color(weak));
            let mut items = vec![here.clone()];
            let names: Vec<String> = containers.clone().unwrap_or_default();
            items.extend(names.iter().map(|n| format!("🐳 {n}")));
            // the container chosen, even when it is not running now
            if !target.is_empty() && !names.contains(&target) {
                items.push(tf!("🐳 {target} (arrêté ?)", "🐳 {target} (stopped?)"));
            }
            let current = if target.is_empty() { Some(0) } else { items.iter().position(|i| i.strip_prefix("🐳 ").is_some_and(|n| n == target || n.starts_with(&format!("{target} ")))) };
            let shown = current.and_then(|k| items.get(k)).cloned().unwrap_or_else(|| here.clone());
            if let Some(k) = crate::pick::combo(ui, "debug_target", shown, &items, current) {
                let name = if k == 0 { String::new() } else { items[k].trim_start_matches("🐳 ").split(" (").next().unwrap_or("").to_string() };
                *act = Some(DebugAct::Target(name));
            }
            if containers.is_none() {
                spinner(ui);
            }
            ui.add_space(6.0);
            let mut attach = mode_attach;
            ui.selectable_value(&mut attach, false, t!("Lancer", "Launch")).on_hover_text(t!("Florm lance le programme sous le débogueur", "Florm runs the program under the debugger"));
            ui.selectable_value(&mut attach, true, t!("S'attacher", "Attach")).on_hover_text(t!("Le programme tourne déjà avec debugpy --listen : Florm s'y connecte", "The program already runs with debugpy --listen: Florm connects to it"));
            if attach != mode_attach {
                self.settings.debug_mode = if attach { "attach".into() } else { "launch".into() };
                self.persist("debug_mode", config::toml_str(&self.settings.debug_mode));
            }
            ui.add_space(6.0);
            if attach {
                ui.label(RichText::new("Port").small().color(weak));
                let mut port = self.settings.debug_port as u32;
                if ui.add(egui::DragValue::new(&mut port).range(1..=65535)).changed() {
                    self.settings.debug_port = port as u16;
                    self.persist("debug_port", port.to_string());
                }
            } else {
                ui.label(RichText::new(t!("Commande", "Command")).small().color(weak));
                let (value, hint, key) = if target.is_empty() {
                    (&mut self.settings.debug_command, if default_local.is_empty() { "./odoo-bin -c odoo.conf -d ma_base --workers 0".to_string() } else { default_local.clone() }, "debug_command")
                } else {
                    (&mut self.settings.debug_docker_command, DOCKER_COMMAND.to_string(), "debug_docker_command")
                };
                let r = ui.add(egui::TextEdit::singleline(value).hint_text(hint).font(egui::TextStyle::Monospace).desired_width((ui.available_width() - 190.0).max(160.0)));
                if r.lost_focus() {
                    let v = config::toml_str(value);
                    self.persist(key, v);
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        *act = Some(DebugAct::Start);
                    }
                }
            }
            let mut mine = self.settings.debug_just_my_code;
            if ui.checkbox(&mut mine, t!("Mon code seulement", "My code only")).on_hover_text(t!("Ne pas entrer dans le code d'Odoo ni des bibliothèques (justMyCode) : les points d'arrêt n'y sont pas pris non plus", "Do not step into the code of Odoo nor of the libraries (justMyCode): the breakpoints there are not taken either")).changed() {
                self.settings.debug_just_my_code = mine;
                self.persist("debug_just_my_code", mine.to_string());
            }
        });
        let help = match (target.is_empty(), mode_attach) {
            (true, false) => t!("Lance la commande ici, dans le panneau du bas, sous le débogueur. --workers 0 : un seul processus, chaque requête passe par les points d'arrêt.", "Runs the command here, in the bottom panel, under the debugger. --workers 0: one process, every request goes through the breakpoints.").to_string(),
            (false, false) => t!("Lance la commande dans le conteneur, sous le débogueur, à côté de l'Odoo qui y tourne (un autre port). Ses fichiers montés s'ouvrent ici, les autres en lecture seule. Clic droit sur le ▶ d'un test (ou Alt+F6) : déboguer ce test.", "Runs the command in the container, under the debugger, next to the Odoo running there (another port). Its mounted files open here, the others read only. Right click on the ▶ of a test (or Alt+F6): debug this test.").to_string(),
            (true, true) => tf!("Avec un point d'arrêt, Florm attend tout seul que le programme écoute, puis s'y attache, à chaque lancement. Lancez-le avec « Copier la commande » (python3 -m debugpy --listen {} ./odoo-bin … --workers 0), dans le terminal de Florm ou le vôtre.", "With a breakpoint, Florm waits by itself for the program to listen, then attaches to it, at each run. Start it with “Copy the command” (python3 -m debugpy --listen {} ./odoo-bin … --workers 0), in Florm's terminal or yours.", self.settings.debug_port),
            (false, true) => tf!("Dans le conteneur, Odoo doit tourner avec debugpy (command: du docker-compose.yml) : python3 -m debugpy --listen 0.0.0.0:{} /usr/bin/odoo … --workers 0. Florm s'y connecte par docker exec : aucun port à publier.", "In the container, Odoo must run with debugpy (command: of docker-compose.yml): python3 -m debugpy --listen 0.0.0.0:{} /usr/bin/odoo … --workers 0. Florm connects to it through docker exec: no port to publish.", self.settings.debug_port),
        };
        ui.label(RichText::new(help).small().color(weak));
    }

    fn debug_side(&mut self, ui: &mut egui::Ui, weak: Color32, act: &mut Option<DebugAct>) {
        let mono = egui::TextStyle::Monospace;
        let header = |ui: &mut egui::Ui, id: &str, title: &str, open: bool, add: &mut dyn FnMut(&mut egui::Ui)| {
            egui::CollapsingHeader::new(RichText::new(title).strong()).id_salt(id).default_open(open).show(ui, |ui| add(ui));
        };
        let d = &self.debug;
        let s = d.session.as_ref();
        let paused = s.is_some_and(|s| s.paused.is_some());

        // ------------------------------------------------ variables
        header(ui, "debug_vars", t!("Variables", "Variables"), true, &mut |ui| {
            let Some(s) = s.filter(|_| paused) else {
                ui.label(RichText::new(t!("Visibles quand le programme est en pause.", "Shown when the program is paused.")).small().color(weak));
                return;
            };
            if s.scopes.is_empty() {
                spinner(ui);
            }
            for (k, sc) in s.scopes.iter().enumerate() {
                let open = is_open(&d.open, &d.shut, &sc.name, k == 0);
                if tree_row(ui, 0, Some(open), &sc.name, "", "", weak).clicked() {
                    *act = Some(DebugAct::Toggle(sc.name.clone(), sc.reference, !open));
                }
                if open {
                    var_rows(ui, s, &d.open, &d.shut, sc.reference, &sc.name, 1, weak, act);
                }
            }
        });

        // ------------------------------------------------ watch
        let (watch, draft) = (&self.debug.watch, &mut self.debug.watch_draft);
        let session = self.debug.session.as_ref();
        egui::CollapsingHeader::new(RichText::new(t!("Espion", "Watch")).strong()).id_salt("debug_watch").default_open(true).show(ui, |ui| {
            for (k, e) in watch.iter().enumerate() {
                ui.horizontal(|ui| {
                    let v = session.and_then(|s| s.watch.get(e));
                    ui.label(RichText::new(e).text_style(mono.clone()).color(highlight::blue()));
                    match v {
                        Some(Ok(v)) => {
                            ui.add(egui::Label::new(RichText::new(format!("= {v}")).text_style(mono.clone())).truncate()).on_hover_text(v);
                        }
                        Some(Err(e)) => {
                            ui.add(egui::Label::new(RichText::new(e).small().color(highlight::red())).truncate());
                        }
                        None if paused => {
                            spinner(ui);
                        }
                        None => {}
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text(t!("Ne plus espionner", "Stop watching")).clicked() {
                            *act = Some(DebugAct::Unwatch(k));
                        }
                    });
                });
            }
            let r = ui.add(egui::TextEdit::singleline(draft).hint_text(t!("+ expression à espionner (self.state…)", "+ expression to watch (self.state…)")).font(egui::TextStyle::Small).desired_width(f32::INFINITY));
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !draft.trim().is_empty() {
                *act = Some(DebugAct::Watch(std::mem::take(draft).trim().to_string()));
            }
        });
        let d = &self.debug;
        let s = d.session.as_ref();

        // ------------------------------------------------ call stack
        header(ui, "debug_stack", t!("Pile d'appels", "Call stack"), true, &mut |ui| {
            let Some(s) = s.filter(|_| paused) else {
                ui.label(RichText::new(t!("Visible quand le programme est en pause.", "Shown when the program is paused.")).small().color(weak));
                return;
            };
            let p = s.paused.as_ref().expect("paused");
            let mut threads = s.threads.clone();
            if !threads.iter().any(|(t, _)| *t == p.thread) {
                threads.insert(0, (p.thread, String::new()));
            }
            for (t, name) in threads {
                let stack = s.stacks.get(&t);
                let label = if t == p.thread { tf!("{name} — en pause ({})", "{name} — paused ({})", reason_label(&p.reason)) } else { name.clone() };
                let r = tree_row(ui, 0, Some(stack.is_some()), &label, "", "", weak);
                if r.clicked() && stack.is_none() {
                    *act = Some(DebugAct::LoadStack(t));
                }
                let Some(frames) = stack else { continue };
                for (k, f) in frames.iter().enumerate() {
                    let sel = s.frame == Some((t, k));
                    let file = f.remote.as_deref().map_or(String::new(), |r| format!("{}:{}", r.rsplit('/').next().unwrap_or(r), f.line));
                    let r = ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        let r = ui.selectable_label(sel, RichText::new(&f.name).text_style(mono.clone()));
                        ui.label(RichText::new(&file).small().color(weak));
                        r
                    });
                    let tip = f.remote.clone().unwrap_or_default();
                    if r.inner.on_hover_text(tip).clicked() {
                        *act = Some(DebugAct::Frame(t, k));
                    }
                }
            }
        });

        // ------------------------------------------------ breakpoints
        let n: usize = d.bps.values().map(Vec::len).sum();
        egui::CollapsingHeader::new(RichText::new(tf!("Points d'arrêt ({n})", "Breakpoints ({n})")).strong()).id_salt("debug_bps").default_open(true).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(t!("Exceptions :", "Exceptions:")).small().color(weak));
                for (key, label, tip) in [
                    ("uncaught", t!("non interceptées", "uncaught"), t!("S'arrêter sur une exception que rien n'intercepte", "Stop on an exception nothing catches")),
                    ("raised", t!("toutes", "all raised"), t!("S'arrêter sur chaque exception levée, même interceptée (Odoo en lève beaucoup)", "Stop on every exception raised, even caught (Odoo raises many)")),
                    ("userUnhandled", t!("hors de mon code", "leaving my code"), t!("S'arrêter quand une exception sort du code du projet", "Stop when an exception leaves the code of the project")),
                ] {
                    let mut on = self.settings.debug_exceptions.iter().any(|e| e == key);
                    if ui.checkbox(&mut on, RichText::new(label).small()).on_hover_text(tip).changed() {
                        self.settings.debug_exceptions.retain(|e| e != key);
                        if on {
                            self.settings.debug_exceptions.push(key.to_string());
                        }
                        *act = Some(DebugAct::Exceptions);
                    }
                }
            });
            if n == 0 {
                ui.label(RichText::new(t!("Clic à gauche des numéros de ligne (ou F9) pour en mettre un.", "Click left of the line numbers (or F9) to put one.")).small().color(weak));
            } else if ui.small_button(t!("Tout retirer", "Remove all")).clicked() {
                *act = Some(DebugAct::ClearBps);
            }
            let root = self.root.clone();
            for (path, list) in self.debug.bps.iter_mut() {
                let rel = root.as_ref().and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path).display().to_string();
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                for b in list.iter_mut() {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), Sense::hover());
                        match &b.state {
                            Some((false, _)) => {
                                ui.painter().circle_stroke(rect.center(), 4.0, egui::Stroke::new(1.5_f32, highlight::red()));
                            }
                            _ => {
                                ui.painter().circle_filled(rect.center(), 4.0, highlight::red());
                            }
                        }
                        let tip = match &b.state {
                            Some((false, m)) if !m.is_empty() => format!("{rel}:{}\n{m}", b.line + 1),
                            Some((false, _)) => tf!("{rel}:{}\nPas encore pris par le débogueur", "{rel}:{}\nNot taken by the debugger yet", b.line + 1),
                            _ => format!("{rel}:{}", b.line + 1),
                        };
                        if ui.link(format!("{name}:{}", b.line + 1)).on_hover_text(tip).clicked() {
                            *act = Some(DebugAct::Open(path.clone(), b.line));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text(t!("Retirer", "Remove")).clicked() {
                                *act = Some(DebugAct::RemoveBp(path.clone(), b.line));
                            }
                            let r = ui.add(egui::TextEdit::singleline(&mut b.condition).hint_text(t!("condition", "condition")).font(egui::TextStyle::Small).desired_width(ui.available_width()));
                            let r = r.on_hover_text(t!("S'arrêter seulement quand elle est vraie, ex. self.id == 7 (Entrée)", "Stop only when it is true, e.g. self.id == 7 (Enter)"));
                            if r.lost_focus() {
                                *act = Some(DebugAct::ConditionChanged(path.clone()));
                            }
                        });
                    });
                }
            }
        });
    }

    fn debug_console(&mut self, ui: &mut egui::Ui, act: &mut Option<DebugAct>) {
        let mono = egui::TextStyle::Monospace;
        egui::TopBottomPanel::bottom("debug_repl").frame(egui::Frame::none().inner_margin(egui::Margin { left: 0.0, right: 0.0, top: 4.0, bottom: 0.0 })).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("›").text_style(mono.clone()).color(self.hl.colors.accent));
                let paused = self.debug.session.as_ref().is_some_and(|s| s.paused.is_some());
                let hint = if paused { t!("Python dans le cadre en pause : self.env['res.partner'].search([]), x = 2…", "Python in the paused frame: self.env['res.partner'].search([]), x = 2…") } else { t!("Python, une fois le programme en pause", "Python, once the program is paused") };
                let r = ui.add(egui::TextEdit::singleline(&mut self.debug.repl).hint_text(hint).font(mono.clone()).desired_width(ui.available_width() - 60.0));
                if std::mem::take(&mut self.debug.focus_console) {
                    r.request_focus();
                }
                if r.has_focus() {
                    if let Some(h) = history_arrows(ui, &self.debug.repl_hist, &mut self.debug.repl_pos) {
                        self.debug.repl = h;
                    }
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.debug.repl_pos = None;
                    *act = Some(DebugAct::Eval(std::mem::take(&mut self.debug.repl)));
                    r.request_focus();
                }
                if ui.small_button(t!("Effacer", "Clear")).clicked() {
                    self.debug.console.clear();
                }
            });
        });
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let rh = ui.fonts(|f| f.row_height(&font));
        let n = self.debug.console.len();
        let colors = self.hl.colors;
        egui::ScrollArea::both().id_salt("debug_console").auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, rh, n, |ui, rows| {
            for (cat, text) in &self.debug.console[rows] {
                let c = match cat {
                    Cat::Out => colors.fg,
                    Cat::Err => highlight::red(),
                    Cat::Info => colors.gutter_fg,
                    Cat::Input => colors.accent,
                    Cat::Result => highlight::green(),
                };
                let r = ui.add(egui::Label::new(RichText::new(text).text_style(mono.clone()).color(c)).extend().sense(Sense::click()));
                r.context_menu(|ui| {
                    if ui.button(t!("Copier", "Copy")).clicked() {
                        *act = Some(DebugAct::Copy(text.clone()));
                        ui.close_menu();
                    }
                });
            }
        });
    }

    fn debug_act(&mut self, a: DebugAct) {
        match a {
            DebugAct::Start => self.debug_start_or_continue(),
            DebugAct::Stop => self.debug_stop_by_hand(),
            DebugAct::Restart => self.debug_restart(),
            DebugAct::Step(c) => self.debug_step(c),
            DebugAct::Toggle(path, reference, open) => {
                let d = &mut self.debug;
                if open {
                    d.shut.remove(&path);
                    d.open.insert(path.clone());
                } else {
                    d.open.remove(&path);
                    d.shut.insert(path.clone());
                }
                if let Some(s) = d.session.as_mut().filter(|s| open && reference > 0 && !s.vars.contains_key(&reference)) {
                    let seq = s.client.request("variables", json!({ "variablesReference": reference }));
                    s.pending.insert(seq, Req::Vars(reference, path));
                }
            }
            DebugAct::Frame(t, k) => self.debug_select_frame(t, k),
            DebugAct::LoadStack(t) => {
                if let Some(s) = self.debug.session.as_mut() {
                    let seq = s.client.request("stackTrace", json!({ "threadId": t, "startFrame": 0, "levels": STACK_LEVELS }));
                    s.pending.insert(seq, Req::Stack(t));
                }
            }
            DebugAct::Open(p, line) => self.open_at(p, line),
            DebugAct::RemoveBp(p, line) => self.debug_remove_bp(p, line),
            DebugAct::ConditionChanged(p) => {
                self.debug.dirty.insert(p);
            }
            DebugAct::ClearBps => self.debug_clear_breakpoints(),
            DebugAct::Watch(e) => {
                if !self.debug.watch.contains(&e) {
                    self.debug.watch.push(e.clone());
                    self.debug_watch_eval(&e);
                }
            }
            DebugAct::Unwatch(k) => {
                if k < self.debug.watch.len() {
                    self.debug.watch.remove(k);
                }
            }
            DebugAct::Eval(e) => self.debug_eval(e),
            DebugAct::Copy(t) => self.egui_ctx.output_mut(|o| o.copied_text = t),
            DebugAct::Install(argv) => {
                let ctx = self.egui_ctx.clone();
                match self.term.open_command_panel(&ctx, &argv, self.root.clone()) {
                    Ok(id) => {
                        let title = t!("Installation de debugpy", "Installing debugpy").to_string();
                        self.term.set_name(id, title);
                        self.bottom = Some(BottomTab::Terminal);
                        self.debug.failure = None;
                        self.set_status(t!("Une fois debugpy installé : F5", "Once debugpy is installed: F5"));
                    }
                    Err(e) => self.set_error(tf!("Terminal impossible à ouvrir : {e}", "Cannot open a terminal: {e}")),
                }
            }
            DebugAct::Exceptions => {
                let v = config::toml_str_array(&self.settings.debug_exceptions);
                self.persist("debug_exceptions", v);
                if self.debug.session.as_ref().is_some_and(|s| s.configured) {
                    self.debug_send_exceptions();
                }
            }
            DebugAct::Target(name) => self.debug_set_target(name),
            DebugAct::Move(true) => self.show_big(BottomTab::Debug),
            DebugAct::Move(false) => self.show_bottom(BottomTab::Debug, false),
        }
    }

    /// Where, how and what is debugged, in one line.
    fn debug_summary(&mut self) -> String {
        let cfg = self.debug_config();
        let place = match &cfg.target {
            Target::Local => t!("cet ordinateur", "this computer").to_string(),
            Target::Docker(c) => format!("🐳 {c}"),
        };
        let what = match cfg.mode {
            Mode::Attach => tf!("s'attacher au port {}", "attach to port {}", cfg.port),
            Mode::Launch if cfg.command.trim().is_empty() => t!("lancer : commande à indiquer", "launch: command to enter").to_string(),
            Mode::Launch => tf!("lancer {}", "launch {}", cfg.command),
        };
        format!("{place} · {what}")
    }

    /// Names of the running containers (`None` until they are read), read again every few
    /// seconds while they are shown.
    fn debug_containers(&mut self) -> Option<Vec<String>> {
        let d = &mut self.debug;
        let stale = d.containers.as_ref().is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(10));
        if stale && d.containers_asked.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            d.containers_asked = Some(Instant::now());
            self.jobs.spawn(|| JobResult::DebugContainers(crate::docker::list().map(|l| l.into_iter().filter(|c| c.running()).map(|c| c.name).collect())));
        }
        self.egui_ctx.request_repaint_after(Duration::from_secs(10));
        self.debug.containers.as_ref().map(|(_, l)| l.clone())
    }

    /// The container the debugger works in (empty: this computer).
    pub(super) fn debug_set_target(&mut self, name: String) {
        if self.settings.debug_target != name {
            self.settings.debug_target = name.clone();
            self.persist("debug_target", config::toml_str(&name));
        }
        self.debug.failure = None;
    }
}

fn reason_label(reason: &str) -> String {
    match reason {
        "breakpoint" => t!("point d'arrêt", "breakpoint").into(),
        "step" => t!("pas à pas", "step").into(),
        "exception" => t!("exception", "exception").into(),
        "pause" => t!("pause", "pause").into(),
        "entry" => t!("début", "entry").into(),
        "goto" => "goto".into(),
        r => r.to_string(),
    }
}

/// A row of a tree: indent, ▸ / ▾ when it opens, name, value, type (on hover).
fn tree_row(ui: &mut egui::Ui, depth: usize, open: Option<bool>, name: &str, value: &str, ty: &str, weak: Color32) -> egui::Response {
    let r = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(depth as f32 * 14.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), Sense::hover());
        if let Some(o) = open {
            icons::paint_chevron(ui.painter(), rect.center(), o, weak);
        }
        let mono = egui::TextStyle::Monospace;
        if value.is_empty() {
            ui.label(RichText::new(name).strong());
        } else {
            ui.label(RichText::new(name).text_style(mono.clone()).color(highlight::blue()));
            ui.add(egui::Label::new(RichText::new(value).text_style(mono)).truncate());
        }
    });
    let r = ui.interact(r.response.rect, ui.id().with(("tree_row", depth, name, r.response.rect.top() as i32)), Sense::click());
    let r = if ty.is_empty() { r } else { r.on_hover_text(format!("{name} : {ty}\n{value}")) };
    if open.is_some() && r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// The children of `reference` in the variables tree, and theirs when they are open.
#[allow(clippy::too_many_arguments)]
fn var_rows(ui: &mut egui::Ui, s: &Session, open: &HashSet<String>, shut: &HashSet<String>, reference: i64, path: &str, depth: usize, weak: Color32, act: &mut Option<DebugAct>) {
    let Some(vars) = s.vars.get(&reference) else {
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0 + 16.0);
            spinner(ui);
        });
        return;
    };
    if vars.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0 + 16.0);
            ui.label(RichText::new(t!("(vide)", "(empty)")).small().color(weak));
        });
    }
    for v in vars {
        let p = format!("{path}\u{1}{}", v.name);
        let is = v.reference > 0 && is_open(open, shut, &p, false);
        let r = tree_row(ui, depth, (v.reference > 0).then_some(is), &v.name, &v.value, &v.ty, weak);
        if r.clicked() && v.reference > 0 {
            *act = Some(DebugAct::Toggle(p.clone(), v.reference, !is));
        }
        r.context_menu(|ui| {
            if ui.button(t!("Copier la valeur", "Copy the value")).clicked() {
                *act = Some(DebugAct::Copy(v.value.clone()));
                ui.close_menu();
            }
            if let Some(e) = &v.expr {
                if ui.button(t!("Copier l'expression", "Copy the expression")).clicked() {
                    *act = Some(DebugAct::Copy(e.clone()));
                    ui.close_menu();
                }
                if ui.button(t!("Espionner", "Watch")).clicked() {
                    *act = Some(DebugAct::Watch(e.clone()));
                    ui.close_menu();
                }
            }
        });
        if is {
            var_rows(ui, s, open, shut, v.reference, &p, depth + 1, weak, act);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_keep_their_conditions() {
        let old = vec![Bp { line: 3, condition: "x".into(), state: None }, Bp::new(8)];
        // lines moved: the conditions follow in order
        let moved = merge_bps(&old, &[5, 10]);
        assert_eq!((moved[0].line, moved[0].condition.as_str()), (5, "x"));
        // one added: the others keep theirs where they are
        let added = merge_bps(&old, &[1, 3, 8]);
        assert_eq!(added.iter().map(|b| (b.line, b.condition.as_str())).collect::<Vec<_>>(), [(1, ""), (3, "x"), (8, "")]);
        assert!(merge_bps(&old, &[]).is_empty());
    }

    /// The whole session in an app without a window, with a real debugpy:
    /// `FLORM_TEST_DEBUGPY=<python with debugpy> cargo test debug_session`.
    #[test]
    fn debug_session_end_to_end() {
        let Ok(python) = std::env::var("FLORM_TEST_DEBUGPY") else { return };
        let dir = std::env::temp_dir().join(format!("florm_debug_app_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), format!("check_updates = false\nlsp = false\ndebug_python = {}\ndebug_command = \"t.py\"\n", config::toml_str(&python))).unwrap();
        let file = proj.join("t.py");
        std::fs::write(&file, "def f(x):\n    y = x * 2\n    return y\n\nprint('result', f(21))\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        assert!(app.open_path(file.clone()));
        let uid = app.docs[app.active].uid;
        let doc = |app: &App| app.docs.iter().position(|d| d.uid == uid).unwrap();
        // a click in the gutter on "return y"
        let i = doc(&app);
        app.docs[i].bp_pick = Some(2);
        app.debug_frame();
        assert_eq!(app.debug.bps.get(&app.docs[i].path.clone().unwrap()).map(|v| v.len()), Some(1));
        app.debug_start_or_continue();
        let wait = |app: &mut App, what: &str, done: &dyn Fn(&App) -> bool| {
            let until = Instant::now() + Duration::from_secs(40);
            while !done(app) {
                assert!(Instant::now() < until, "trop long : {what}\n{:?}", app.debug.console.iter().map(|c| c.1.clone()).collect::<Vec<_>>());
                app.process_jobs();
                app.debug_frame();
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let locals = |app: &App| -> Option<Vec<(String, String)>> {
            let s = app.debug.session.as_ref()?;
            let r = s.scopes.first()?.reference;
            Some(s.vars.get(&r)?.iter().map(|v| (v.name.clone(), v.value.clone())).collect())
        };
        wait(&mut app, "arrêt au point d'arrêt", &|app| locals(app).is_some_and(|v| v.iter().any(|(n, _)| n == "y")));
        let vars = locals(&app).unwrap();
        assert!(vars.contains(&("x".into(), "21".into())) && vars.contains(&("y".into(), "42".into())), "{vars:?}");
        // the editor shows the line where it is paused (the file stays the one shown)
        let i = doc(&app);
        assert_eq!(app.debug_exec_line(i), Some((2, true)));
        assert_eq!(app.docs[app.active].uid, uid);
        // the view and the editor drawn while paused (with the variables unfolded)
        for _ in 0..3 {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() }, |ctx| {
                egui::TopBottomPanel::bottom("t_debug").exact_height(400.0).show(ctx, |ui| app.debug_ui(ui));
                egui::CentralPanel::default().show(ctx, |ui| {
                    let i = doc(&app);
                    app.doc_body(ui, i, editor_id(), true);
                });
            });
        }
        // the console evaluates in the frame
        app.debug_eval("y + 1".into());
        wait(&mut app, "évaluation", &|app| app.debug.console.iter().any(|(c, t)| *c == Cat::Result && t == "43"));
        // the value under the mouse
        let i = doc(&app);
        let ls = app.docs[i].rope.line_to_char(2);
        app.docs[i].hover_word = Some((ls + 11, ls + 12));
        wait(&mut app, "survol", &|app| app.docs[doc(app)].debug_value.as_ref().is_some_and(|(_, t)| t.starts_with("y") && t.ends_with("= 42")));
        // one step out: back in the caller
        app.debug_step("stepOut");
        wait(&mut app, "pas à pas sortant", &|app| app.debug.session.as_ref().and_then(|s| s.frame.map(|(t, k)| s.stacks.get(&t).map(|v| v[k].line)))== Some(Some(5)));
        app.debug_step("continue");
        wait(&mut app, "fin du programme", &|app| app.debug.session.is_none());
        let out = app.debug.term.and_then(|id| app.term.output(id)).unwrap_or_default();
        assert!(out.contains("result 42"), "{out}");
        // the breakpoint is kept in the session of the project
        assert_eq!(app.debug_session_breakpoints().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A breakpoint, nothing else (launch mode, no F5): Florm waits by itself, the program is started
    /// afterwards outside of Florm with debugpy, it stops at the breakpoint, and once it ends Florm
    /// waits again: `FLORM_TEST_DEBUGPY=<python with debugpy> cargo test debug_waits`.
    #[test]
    fn debug_waits_for_the_program_and_attaches_by_itself() {
        let Ok(python) = std::env::var("FLORM_TEST_DEBUGPY") else { return };
        let dir = std::env::temp_dir().join(format!("florm_debug_wait_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        // a free port
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        std::fs::write(conf.join("settings.toml"), format!("check_updates = false\nlsp = false\ndebug_python = {}\ndebug_port = {port}\n", config::toml_str(&python))).unwrap();
        let file = proj.join("t.py");
        // a moment for Florm to attach before the code with the breakpoint runs
        let result = proj.join("out.txt");
        std::fs::write(&file, "import sys, time\ntime.sleep(2)\ndef f(x):\n    y = x * 2\n    return y\n\nopen(sys.argv[1], 'a').write('result %d\\n' % f(21))\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        assert!(app.open_path(file.clone()));
        let i = app.active;
        // a click in the gutter on "return y"
        app.docs[i].bp_pick = Some(4);
        let wait = |app: &mut App, what: &str, done: &dyn Fn(&App) -> bool| {
            let until = Instant::now() + Duration::from_secs(40);
            while !done(app) {
                assert!(Instant::now() < until, "trop long : {what}\n{:?}", app.debug.console.iter().map(|c| c.1.clone()).collect::<Vec<_>>());
                app.process_jobs();
                app.debug_frame();
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        // no F5: the breakpoint is enough to wait
        wait(&mut app, "attente du programme", &|app| app.debug.session.as_ref().is_some_and(|s| s.auto && !s.connected));
        // its output in a file: debugpy's adapter (another process) would keep a pipe open
        let run = || std::process::Command::new(&python).args(["-m", "debugpy", "--listen", &port.to_string()]).arg(&file).arg(&result).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let ended = |mut p: std::process::Child| {
            let until = Instant::now() + Duration::from_secs(30);
            while p.try_wait().unwrap().is_none() {
                assert!(Instant::now() < until, "le programme ne s'est pas terminé");
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        let program = run();
        let paused_on_y = |app: &App| {
            let Some(s) = app.debug.session.as_ref() else { return false };
            let Some(r) = s.scopes.first().map(|s| s.reference) else { return false };
            s.vars.get(&r).is_some_and(|v| v.iter().any(|v| v.name == "y" && v.value == "42"))
        };
        wait(&mut app, "arrêt au point d'arrêt", &paused_on_y);
        app.debug_step("continue");
        ended(program);
        assert_eq!(std::fs::read_to_string(&result).unwrap_or_default(), "result 42\n");
        // the program ended: Florm waits again, for the next run
        wait(&mut app, "nouvelle attente", &|app| app.debug.session.as_ref().is_some_and(|s| !s.connected));
        let program = run();
        wait(&mut app, "arrêt au deuxième lancement", &paused_on_y);
        // stopped by hand: no more waiting, until F5
        app.debug_stop_by_hand();
        wait(&mut app, "arrêt à la main", &|app| app.debug.session.is_none());
        for _ in 0..20 {
            app.process_jobs();
            app.debug_frame();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.debug.session.is_none() && app.debug.preparing.is_none());
        // detached, the program goes on to its end
        ended(program);
        assert_eq!(std::fs::read_to_string(&result).unwrap_or_default(), "result 42\nresult 42\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// In a running container with debugpy and a folder mounted from here:
    /// `FLORM_TEST_DEBUG_CONTAINER=<name> cargo test debug_in_docker`. Launched there, then
    /// attached to a debugpy listening there.
    #[test]
    fn debug_in_docker_end_to_end() {
        let Ok(container) = std::env::var("FLORM_TEST_DEBUG_CONTAINER") else { return };
        let mounts = crate::docker::running_mounts(&container).unwrap();
        let (remote_dir, proj) = mounts.iter().find(|(_, s)| s.is_dir()).cloned().expect("un dossier monté");
        let dir = std::env::temp_dir().join(format!("florm_debug_docker_{}", std::process::id()));
        let conf = dir.join("conf");
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::write(conf.join("settings.toml"), format!("check_updates = false\nlsp = false\ndebug_target = {}\ndebug_docker_command = \"florm_t.py\"\n", config::toml_str(&container))).unwrap();
        let file = proj.join("florm_t.py");
        std::fs::write(&file, "import textwrap\n\ndef f(x):\n    y = textwrap.dedent('  a')\n    return y\n\nprint('result', f(21))\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        assert!(app.open_path(file.clone()));
        let uid = app.docs[app.active].uid;
        let i = app.active;
        app.docs[i].bp_pick = Some(3);
        app.debug_frame();
        app.debug_start_or_continue();
        let wait = |app: &mut App, what: &str, done: &dyn Fn(&App) -> bool| {
            let until = Instant::now() + Duration::from_secs(40);
            while !done(app) {
                assert!(Instant::now() < until, "trop long : {what}\n{:?}", app.debug.console.iter().map(|c| c.1.clone()).collect::<Vec<_>>());
                app.process_jobs();
                app.debug_frame();
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let frame = |app: &App| -> Option<(Option<String>, usize)> {
            let s = app.debug.session.as_ref()?;
            let (t, k) = s.frame?;
            let f = s.stacks.get(&t)?.get(k)?;
            Some((f.remote.clone(), f.line))
        };
        wait(&mut app, "arrêt dans le conteneur", &|app| frame(app).is_some());
        // the file of the container is the one here, through the mount
        assert_eq!(frame(&app), Some((Some(format!("{}/florm_t.py", remote_dir.trim_end_matches('/'))), 4)));
        let i = app.docs.iter().position(|d| d.uid == uid).unwrap();
        assert_eq!(app.debug_exec_line(i), Some((3, true)));
        // into textwrap, a file of the image only: read from the container, read only
        app.debug_step("stepIn");
        wait(&mut app, "fichier de l'image", &|app| app.docs[app.active].title_override.as_deref().is_some_and(|t| t.starts_with("textwrap.py")));
        assert!(app.docs[app.active].read_only);
        assert!(app.debug_exec_line(app.active).is_some());
        app.debug_step("continue");
        wait(&mut app, "fin du programme", &|app| app.debug.session.is_none());
        let out = app.debug.term.and_then(|id| app.term.output(id)).unwrap_or_default();
        assert!(out.contains("result a"), "{out}");

        // attach: a program listening with debugpy in the container, reached through docker exec
        let remote = format!("{}/florm_t.py", remote_dir.trim_end_matches('/'));
        let st = std::process::Command::new("docker").args(["exec", "-d", &container, "python3", "-m", "debugpy", "--listen", "127.0.0.1:5679", "--wait-for-client", &remote]).status().unwrap();
        assert!(st.success());
        std::thread::sleep(Duration::from_secs(2));
        app.settings.debug_mode = "attach".into();
        app.settings.debug_port = 5679;
        app.debug_start_or_continue();
        wait(&mut app, "arrêt après s'être attaché", &|app| frame(app).is_some_and(|f| f.1 == 4));
        app.debug_step("continue");
        wait(&mut app, "fin du programme attaché", &|app| app.debug.session.is_none());
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn variables_and_frames() {
        let body = json!({ "variables": [{ "name": "self", "value": "sale.order(7,)", "type": "sale.order", "variablesReference": 12, "evaluateName": "self" }, { "name": "x", "value": "2", "variablesReference": 0 }] });
        let v = parse_vars(&body, None);
        assert_eq!((v[0].reference, v[0].expr.as_deref(), v[1].expr.as_deref()), (12, Some("self"), Some("x")));
        let mounts = vec![("/mnt/addons".to_string(), PathBuf::from("/nowhere/addons"))];
        let body = json!({ "stackFrames": [{ "id": 3, "name": "action_confirm", "line": 120, "source": { "path": "/mnt/addons/sale/models/sale.py" } }, { "id": 4, "name": "<frozen>", "line": 1, "source": {} }] });
        let f = parse_frames(&body, &Target::Docker("web".into()), &mounts);
        assert_eq!(f[0].local.as_deref(), Some(Path::new("/nowhere/addons/sale/models/sale.py")));
        assert_eq!((f[0].line, f[1].remote.is_none()), (120, true));
    }
}
