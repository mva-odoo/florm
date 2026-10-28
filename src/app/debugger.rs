//! The debugger, like VS Code's: breakpoints in the gutter (F9), the program paused there, its
//! variables, call stack and a console to evaluate Python in it, F10 / F11 / Maj+F11 to step.
//! For Odoo on this computer or in a Docker container (see dap.rs): F5 starts the program set in
//! the view, Alt+F6 (or a right click on the ▶ of a test) debugs the test under the cursor.

use super::*;
use crate::dap::{self, Incoming, Mode, Target};
use crate::project::SessionBreakpoint;
use serde_json::{json, Value};
use std::collections::BTreeMap;

mod breakpoints;
mod launch;
mod protocol;
mod ui;
use breakpoints::*;
use ui::*;

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

}

#[cfg(test)]
mod tests {
    use super::protocol::{parse_frames, parse_vars};
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
