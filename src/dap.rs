//! Debug Adapter Protocol client, as in Zed: one adapter process per session (debugpy for Python,
//! `dlv dap` for Go, `lldb-dap` or `gdb -i dap` for Rust / C / C++), JSON messages framed like the
//! LSP ones, over stdio or TCP, read and written on threads. The launch configurations: the file
//! or the test under the cursor, and those of the project (`.florm/debug.toml`).

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

/// What the adapter sent.
#[derive(Debug)]
pub enum Msg {
    /// The answer to the request `seq`.
    Response { seq: i64, ok: bool, body: Value, message: String },
    Event { event: String, body: Value },
    /// The adapter is gone (its output closed).
    Exited,
}

/// A launch configuration: `[[config]]` of `.florm/debug.toml`, or made by Florm for the file or
/// the test under the cursor. `{file}`, `{dir}` (its folder) and `{root}` are replaced in the
/// paths and arguments.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct LaunchCfg {
    pub name: String,
    /// "python", "go", "lldb", "gdb" (or "rust", "c", "cpp": lldb-dap, else gdb).
    pub adapter: String,
    /// "launch" (the program is started by the debugger) or "attach" (to a running one: debugpy
    /// listening on host:port, in a container for instance).
    pub request: String,
    pub program: String,
    /// Python: `python -m module` instead of a program.
    pub module: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: BTreeMap<String, String>,
    pub stop_on_entry: bool,
    /// Python: also stop in the libraries (false: only in the code of the project).
    pub all_code: bool,
    /// attach: where debugpy listens.
    pub host: String,
    pub port: u16,
    /// attach: folder seen by the program (in the container) = folder here.
    pub path_mappings: BTreeMap<String, String>,
    /// A command run before (`cargo build`…), in `cwd`.
    pub build: String,
    /// More fields given as they are to the adapter (its own options).
    pub extra: serde_json::Map<String, Value>,
    /// A Rust test: its executable is built first (`cargo test --no-run` with these arguments).
    #[serde(skip)]
    pub cargo_test: Option<Vec<String>>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct DebugFile {
    config: Vec<LaunchCfg>,
}

/// The configurations of `.florm/debug.toml` in `root` (empty without the file; an error said).
pub fn project_configs(root: &Path) -> Result<Vec<LaunchCfg>, String> {
    let p = root.join(".florm").join("debug.toml");
    let Ok(text) = std::fs::read_to_string(&p) else { return Ok(Vec::new()) };
    let f: DebugFile = toml::from_str(&text).map_err(|e| format!("{} : {e}", p.display()))?;
    Ok(f.config.into_iter().filter(|c| !c.name.trim().is_empty()).collect())
}

/// The text of a new `.florm/debug.toml`, with examples.
pub const DEBUG_TOML_EXAMPLE: &str = r#"# Configurations du débogueur de Florm (F5), une par [[config]].
# {file} = le fichier ouvert, {dir} = son dossier, {root} = le dossier du projet.

[[config]]
name = "Lancer main.py"
adapter = "python"          # python, go, lldb, gdb
program = "{root}/main.py"
args = []
cwd = "{root}"
# env = { DEBUG = "1" }
# module = "flask"          # python -m flask, à la place de program
# all_code = true           # s'arrêter aussi dans les bibliothèques

# Un programme Python qui tourne déjà (dans un conteneur : python -m debugpy --listen 0.0.0.0:5678 ...)
# [[config]]
# name = "Attacher (conteneur)"
# adapter = "python"
# request = "attach"
# host = "localhost"
# port = 5678
# path_mappings = { "/app" = "{root}" }

# Rust / C / C++ : compilé avant, puis lancé dans lldb-dap ou gdb
# [[config]]
# name = "Mon binaire"
# adapter = "lldb"
# build = "cargo build"
# program = "{root}/target/debug/mon-binaire"
"#;

/// `{file}`, `{dir}`, `{root}` replaced.
pub fn expand(s: &str, file: Option<&Path>, root: &Path) -> String {
    let dir = file.and_then(Path::parent).map(|d| d.display().to_string()).unwrap_or_default();
    s.replace("{file}", &file.map(|f| f.display().to_string()).unwrap_or_default()).replace("{dir}", &dir).replace("{root}", &root.display().to_string())
}

/// The family of an adapter name.
fn family(adapter: &str) -> &str {
    match adapter {
        "py" | "python" | "debugpy" => "python",
        "go" | "dlv" | "delve" => "go",
        "lldb" | "lldb-dap" | "codelldb" => "lldb",
        "gdb" => "gdb",
        "rust" | "c" | "cpp" | "c++" | "native" => "native",
        other => other,
    }
}

/// The Python of the project's virtual environment (`.venv`, `venv`, `env`), else the system's.
pub fn project_python(root: &Path) -> String {
    for venv in [".venv", "venv", "env"] {
        let bin = if cfg!(windows) { root.join(venv).join("Scripts").join("python.exe") } else { root.join(venv).join("bin").join("python") };
        if bin.is_file() {
            return bin.display().to_string();
        }
    }
    crate::lint::python().to_string()
}

/// How to start an adapter.
#[derive(Clone, Debug)]
pub struct AdapterSpec {
    pub name: String,
    pub cmd: Vec<String>,
    pub env: Vec<(String, String)>,
    /// It listens on TCP: `{port}` in `cmd` is replaced by a free port, then Florm connects.
    pub tcp: bool,
    /// Its `adapterID` in `initialize`.
    pub id: String,
}

fn python_has_debugpy(python: &str, pythonpath: Option<&Path>) -> bool {
    let mut c = crate::os::command(python);
    c.args(["-c", "import debugpy"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(p) = pythonpath {
        c.env("PYTHONPATH", p);
    }
    c.status().is_ok_and(|s| s.success())
}

/// The adapter of `cfg` (looked for on the disk: run on a thread). Err: what to install.
pub fn adapter_for(cfg: &LaunchCfg, root: &Path) -> Result<AdapterSpec, String> {
    let which = |b: &str| crate::lsp::in_path(b).map(|p| p.display().to_string());
    match family(&cfg.adapter) {
        "python" => {
            // the project's Python, the system's, then the one Florm installed in its tools
            let venv = project_python(root);
            for py in [venv.as_str(), crate::lint::python()] {
                if python_has_debugpy(py, None) {
                    return Ok(AdapterSpec { name: "debugpy".into(), cmd: vec![py.into(), "-m".into(), "debugpy.adapter".into()], env: Vec::new(), tcp: false, id: "debugpy".into() });
                }
            }
            let tools = crate::lint::tools_dir();
            if tools.join("debugpy").is_dir() && python_has_debugpy(crate::lint::python(), Some(&tools)) {
                let env = vec![("PYTHONPATH".to_string(), tools.display().to_string())];
                return Ok(AdapterSpec { name: "debugpy".into(), cmd: vec![crate::lint::python().into(), "-m".into(), "debugpy.adapter".into()], env, tcp: false, id: "debugpy".into() });
            }
            Err(t!("debugpy n'est pas installé : « Débogueur : installer debugpy » (palette), ou pip install debugpy", "debugpy is not installed: “Debugger: install debugpy” (palette), or pip install debugpy").into())
        }
        "go" => match which("dlv") {
            Some(dlv) => Ok(AdapterSpec { name: "dlv".into(), cmd: vec![dlv, "dap".into(), "--listen".into(), "127.0.0.1:{port}".into()], env: Vec::new(), tcp: true, id: "go".into() }),
            None => Err(t!("dlv n'est pas installé : go install github.com/go-delve/delve/cmd/dlv@latest", "dlv is not installed: go install github.com/go-delve/delve/cmd/dlv@latest").into()),
        },
        f @ ("lldb" | "gdb" | "native") => {
            let lldb = || which("lldb-dap").or_else(|| which("lldb-vscode")).map(|p| AdapterSpec { name: "lldb-dap".into(), cmd: vec![p], env: Vec::new(), tcp: false, id: "lldb-dap".into() });
            let gdb = || which("gdb").map(|p| AdapterSpec { name: "gdb".into(), cmd: vec![p, "-i".into(), "dap".into()], env: Vec::new(), tcp: false, id: "gdb".into() });
            let found = match f {
                "lldb" => lldb(),
                "gdb" => gdb(),
                _ => lldb().or_else(gdb),
            };
            found.ok_or_else(|| t!("ni lldb-dap ni gdb (14 ou plus) : paquet lldb ou gdb", "neither lldb-dap nor gdb (14 or later): lldb or gdb package").into())
        }
        other => Err(tf!("adaptateur inconnu : « {other} » (python, go, lldb, gdb)", "unknown adapter: “{other}” (python, go, lldb, gdb)")),
    }
}

/// The arguments of `launch` / `attach` for `cfg` and its adapter.
pub fn launch_args(cfg: &LaunchCfg, spec: &AdapterSpec, file: Option<&Path>, root: &Path) -> (String, Value) {
    let x = |s: &str| expand(s, file, root);
    let request = if cfg.request == "attach" { "attach" } else { "launch" };
    let cwd = if cfg.cwd.trim().is_empty() { root.display().to_string() } else { x(&cfg.cwd) };
    let args: Vec<String> = cfg.args.iter().map(|a| x(a)).collect();
    let env: serde_json::Map<String, Value> = cfg.env.iter().map(|(k, v)| (k.clone(), Value::String(x(v)))).collect();
    let mut a = serde_json::Map::new();
    match spec.id.as_str() {
        "debugpy" => {
            a.insert("type".into(), "python".into());
            a.insert("justMyCode".into(), (!cfg.all_code).into());
            if request == "attach" {
                let host = if cfg.host.is_empty() { "localhost".to_string() } else { cfg.host.clone() };
                a.insert("connect".into(), json!({"host": host, "port": if cfg.port == 0 { 5678 } else { cfg.port }}));
                let maps: Vec<Value> = cfg.path_mappings.iter().map(|(remote, local)| json!({"remoteRoot": remote, "localRoot": x(local)})).collect();
                if !maps.is_empty() {
                    a.insert("pathMappings".into(), maps.into());
                }
            } else {
                if cfg.module.trim().is_empty() {
                    a.insert("program".into(), x(&cfg.program).into());
                } else {
                    a.insert("module".into(), cfg.module.clone().into());
                }
                a.insert("args".into(), args.into());
                a.insert("cwd".into(), cwd.into());
                a.insert("env".into(), env.into());
                a.insert("python".into(), project_python(root).into());
                a.insert("console".into(), "internalConsole".into());
                a.insert("redirectOutput".into(), true.into());
                a.insert("stopOnEntry".into(), cfg.stop_on_entry.into());
                a.insert("showReturnValue".into(), true.into());
            }
        }
        "go" => {
            let mode = cfg.extra.get("mode").and_then(Value::as_str).unwrap_or("debug").to_string();
            a.insert("mode".into(), mode.into());
            a.insert("program".into(), x(&cfg.program).into());
            a.insert("args".into(), args.into());
            a.insert("cwd".into(), cwd.into());
            a.insert("env".into(), env.into());
            a.insert("stopOnEntry".into(), cfg.stop_on_entry.into());
        }
        id => {
            a.insert("program".into(), x(&cfg.program).into());
            a.insert("args".into(), args.into());
            a.insert("cwd".into(), cwd.into());
            if id == "gdb" {
                a.insert("stopAtBeginningOfMainSubprogram".into(), cfg.stop_on_entry.into());
            } else {
                // lldb-dap: a list of "KEY=value"
                let list: Vec<Value> = cfg.env.iter().map(|(k, v)| Value::String(format!("{k}={}", x(v)))).collect();
                a.insert("env".into(), list.into());
                a.insert("stopOnEntry".into(), cfg.stop_on_entry.into());
            }
        }
    }
    for (k, v) in &cfg.extra {
        a.insert(k.clone(), v.clone());
    }
    (request.to_string(), Value::Object(a))
}

/// A running adapter: requests go to its writing thread, its messages come on `events`.
pub struct Session {
    out: Sender<String>,
    seq: i64,
    child: Option<Child>,
    pub name: String,
}

/// A free TCP port on this computer.
fn free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok().map(|a| a.port())
}

impl Session {
    /// Starts the adapter (and connects to it over TCP when it listens). Blocking up to a few
    /// seconds: called on a thread.
    pub fn start(spec: &AdapterSpec, cwd: &Path, events: Sender<Msg>, repaint: impl Fn() + Send + Clone + 'static) -> Result<Session, String> {
        let port = if spec.tcp { free_port().ok_or("port")? } else { 0 };
        let argv: Vec<String> = spec.cmd.iter().map(|a| a.replace("{port}", &port.to_string())).collect();
        let mut cmd = crate::os::command(&argv[0]);
        cmd.args(&argv[1..]).envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str()))).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("{} : {e}", spec.name))?;
        // what the adapter writes on stderr goes to the console (its own errors)
        if let Some(mut err) = child.stderr.take() {
            let (events, repaint) = (events.clone(), repaint.clone());
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let _ = events.send(Msg::Event { event: "output".into(), body: json!({"category": "stderr", "output": text}) });
                    repaint();
                }
            });
        }
        let (reader, mut writer): (Box<dyn Read + Send>, Box<dyn Write + Send>) = if spec.tcp {
            let stream = connect(port, &mut child)?;
            let r = stream.try_clone().map_err(|e| e.to_string())?;
            (Box::new(r), Box::new(stream))
        } else {
            (Box::new(child.stdout.take().ok_or("stdout")?), Box::new(child.stdin.take().ok_or("stdin")?))
        };
        let (out, out_rx) = channel::<String>();
        std::thread::spawn(move || {
            for msg in out_rx {
                let frame = format!("Content-Length: {}\r\n\r\n{msg}", msg.len());
                if writer.write_all(frame.as_bytes()).and_then(|_| writer.flush()).is_err() {
                    break;
                }
            }
        });
        let reply = out.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(reader);
            while let Some(text) = crate::lsp::read_message(&mut r) {
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                let msg = match v["type"].as_str().unwrap_or("") {
                    "response" => Msg::Response {
                        seq: v["request_seq"].as_i64().unwrap_or(-1),
                        ok: v["success"].as_bool().unwrap_or(false),
                        body: v.get("body").cloned().unwrap_or(Value::Null),
                        message: v["message"].as_str().unwrap_or("").into(),
                    },
                    "event" => Msg::Event { event: v["event"].as_str().unwrap_or("").into(), body: v.get("body").cloned().unwrap_or(Value::Null) },
                    // a request of the adapter (runInTerminal, startDebugging): not supported,
                    // said at once so that it does not wait
                    "request" => {
                        let answer = json!({"type": "response", "seq": 0, "request_seq": v["seq"], "success": false, "command": v["command"], "message": "not supported by Florm"});
                        let _ = reply.send(answer.to_string());
                        continue;
                    }
                    _ => continue,
                };
                let _ = events.send(msg);
                repaint();
            }
            let _ = events.send(Msg::Exited);
            repaint();
        });
        Ok(Session { out, seq: 0, child: Some(child), name: spec.name.clone() })
    }

    /// Sends `command`; its answer comes with this number.
    pub fn request(&mut self, command: &str, arguments: Value) -> i64 {
        self.seq += 1;
        let msg = json!({"seq": self.seq, "type": "request", "command": command, "arguments": arguments});
        let _ = self.out.send(msg.to_string());
        self.seq
    }
}

/// Connects to an adapter listening on `port` (it takes a moment to start).
fn connect(port: u16, child: &mut Child) -> Result<std::net::TcpStream, String> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => return Ok(s),
            Err(e) => {
                if let Ok(Some(st)) = child.try_wait() {
                    return Err(tf!("l'adaptateur s'est arrêté ({st})", "the adapter stopped ({st})"));
                }
                if Instant::now() > until {
                    return Err(e.to_string());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// The first test executable `cargo test --no-run` builds in `cwd` (with its filter arguments).
pub fn cargo_test_executable(cwd: &Path, filter: &[String]) -> Result<PathBuf, String> {
    let out = crate::os::command("cargo")
        .args(["test", "--no-run", "--message-format=json"])
        .args(filter.iter().filter(|a| a.starts_with('-')))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cargo : {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v["reason"] == "compiler-artifact" && v["profile"]["test"] == true {
            if let Some(exe) = v["executable"].as_str() {
                return Ok(PathBuf::from(exe));
            }
        }
    }
    Err(format!("cargo test --no-run : {}", String::from_utf8_lossy(&out.stderr).lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("")))
}

/// Runs `cmd` (a `build` of a configuration) in `cwd`: its output when it failed.
pub fn run_build(cmd: &str, cwd: &Path) -> Result<(), String> {
    let argv = crate::test_cmd::split(cmd);
    let Some(prog) = argv.first() else { return Ok(()) };
    let out = crate::os::command(prog).args(&argv[1..]).current_dir(cwd).stdin(Stdio::null()).output().map_err(|e| format!("{cmd} : {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{cmd} :\n{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;

    #[test]
    fn configurations_of_the_project() {
        let f: DebugFile = toml::from_str(DEBUG_TOML_EXAMPLE).unwrap();
        assert_eq!(f.config.len(), 1);
        assert_eq!(f.config[0].adapter, "python");
        let text = "[[config]]\nname = \"Attacher\"\nadapter = \"python\"\nrequest = \"attach\"\nport = 5679\npath_mappings = { \"/app\" = \"{root}\" }\nextra = { subProcess = true }\n";
        let f: DebugFile = toml::from_str(text).unwrap();
        let c = &f.config[0];
        let spec = AdapterSpec { name: "debugpy".into(), cmd: vec![], env: vec![], tcp: false, id: "debugpy".into() };
        let (req, a) = launch_args(c, &spec, None, Path::new("/p"));
        assert_eq!(req, "attach");
        assert_eq!(a["connect"], json!({"host": "localhost", "port": 5679}));
        assert_eq!(a["pathMappings"], json!([{"remoteRoot": "/app", "localRoot": "/p"}]));
        assert_eq!(a["subProcess"], json!(true));
        // a launch: {file}, {dir} and {root} replaced
        let c = LaunchCfg { adapter: "gdb".into(), program: "{dir}/a.out".into(), args: vec!["{file}".into()], ..Default::default() };
        let spec = AdapterSpec { name: "gdb".into(), cmd: vec![], env: vec![], tcp: false, id: "gdb".into() };
        let (req, a) = launch_args(&c, &spec, Some(Path::new("/p/src/m.c")), Path::new("/p"));
        assert_eq!((req.as_str(), a["program"].as_str(), a["args"][0].as_str(), a["cwd"].as_str()), ("launch", Some("/p/src/a.out"), Some("/p/src/m.c"), Some("/p")));
    }

    /// Waits for the message `pick` keeps (the others are skipped), up to 20 s.
    fn wait_for<T>(rx: &Receiver<Msg>, mut pick: impl FnMut(&Msg) -> Option<T>) -> T {
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            let m = rx.recv_timeout(until.saturating_duration_since(Instant::now())).expect("adapter message");
            if let Some(t) = pick(&m) {
                return t;
            }
        }
    }

    /// A whole session with a real adapter: initialize, launch, a breakpoint, stopped there, the
    /// stack and the variables, continue to the end.
    fn session_to_a_breakpoint(cfg: LaunchCfg, file: &Path, line: usize, var: &str) {
        let root = file.parent().unwrap();
        let spec = adapter_for(&cfg, root).expect("adapter installed");
        let (tx, rx) = channel();
        let mut s = Session::start(&spec, root, tx, || {}).unwrap();
        let init = s.request("initialize", json!({"clientID": "florm", "adapterID": spec.id, "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path"}));
        wait_for(&rx, |m| matches!(m, Msg::Response { seq, ok: true, .. } if *seq == init).then_some(()));
        let (req, args) = launch_args(&cfg, &spec, Some(file), root);
        s.request(&req, args);
        wait_for(&rx, |m| matches!(m, Msg::Event { event, .. } if event == "initialized").then_some(()));
        s.request("setBreakpoints", json!({"source": {"path": file}, "breakpoints": [{"line": line}]}));
        s.request("configurationDone", json!({}));
        let thread = wait_for(&rx, |m| match m {
            Msg::Event { event, body } if event == "stopped" => Some(body["threadId"].as_i64().unwrap_or(1)),
            _ => None,
        });
        let st = s.request("stackTrace", json!({"threadId": thread, "levels": 20}));
        let frame = wait_for(&rx, |m| match m {
            Msg::Response { seq, body, .. } if *seq == st => Some(body["stackFrames"][0].clone()),
            _ => None,
        });
        assert_eq!(frame["line"].as_u64(), Some(line as u64), "{frame}");
        let sc = s.request("scopes", json!({"frameId": frame["id"]}));
        let r = wait_for(&rx, |m| match m {
            Msg::Response { seq, body, .. } if *seq == sc => Some(body["scopes"][0]["variablesReference"].as_i64().unwrap()),
            _ => None,
        });
        let vr = s.request("variables", json!({"variablesReference": r}));
        let names = wait_for(&rx, |m| match m {
            Msg::Response { seq, body, .. } if *seq == vr => Some(body["variables"].as_array().unwrap().iter().map(|v| v["name"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>()),
            _ => None,
        });
        assert!(names.iter().any(|n| n == var), "{names:?}");
        s.request("continue", json!({"threadId": thread}));
        wait_for(&rx, |m| matches!(m, Msg::Event { event, .. } if event == "terminated" || event == "exited").then_some(()));
    }

    /// `cargo test -- --ignored`: Python with debugpy.
    #[test]
    #[ignore]
    fn python_with_debugpy() {
        let dir = std::env::temp_dir().join(format!("florm_dap_py_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("prog.py");
        std::fs::write(&file, "def f(n):\n    total = n * 2\n    return total\n\nprint(f(21))\n").unwrap();
        let cfg = LaunchCfg { adapter: "python".into(), program: "{file}".into(), ..Default::default() };
        session_to_a_breakpoint(cfg, &file, 3, "total");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `cargo test -- --ignored`: C with gdb (14 or later) and gcc.
    #[test]
    #[ignore]
    fn c_with_gdb() {
        let dir = std::env::temp_dir().join(format!("florm_dap_c_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("prog.c");
        std::fs::write(&file, "#include <stdio.h>\nint main(void) {\n    int total = 21 * 2;\n    printf(\"%d\\n\", total);\n    return 0;\n}\n").unwrap();
        let exe = dir.join("prog");
        assert!(std::process::Command::new("gcc").args(["-g", "-O0", "-o"]).arg(&exe).arg(&file).status().unwrap().success());
        let cfg = LaunchCfg { adapter: "gdb".into(), program: exe.display().to_string(), ..Default::default() };
        session_to_a_breakpoint(cfg, &file, 4, "total");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
