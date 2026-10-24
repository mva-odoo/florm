//! Debugger for Python (Odoo) through the Debug Adapter Protocol and debugpy, like VS Code.
//! The adapter runs here or in a Docker container (`docker exec -i <c> python3 -m
//! debugpy.adapter`): a program in a container is debugged without publishing any port, its
//! files are found here through the mounts of the container, and the program runs in a
//! terminal of the bottom panel (`runInTerminal`), so its logs and an `ipdb` in it are still there.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// What the adapter sends.
#[derive(Debug)]
pub enum Incoming {
    Response { request_seq: i64, command: String, success: bool, message: String, body: Value },
    Event { event: String, body: Value },
    /// A request of the adapter (`runInTerminal`).
    Request { seq: i64, command: String, arguments: Value },
    /// A line of its error output ("No module named debugpy"…).
    Stderr(String),
    /// The adapter is gone.
    Closed,
}

/// Connection to a debug adapter, read and written on threads.
pub struct Client {
    out: Sender<String>,
    rx: Receiver<Incoming>,
    seq: i64,
    child: Option<Child>,
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Client {
    /// The adapter is the program `argv`, spoken to on its input and output.
    pub fn spawn(argv: &[String], cwd: Option<&Path>, repaint: impl Fn() + Send + Clone + 'static) -> Result<Client, String> {
        let first = argv.first().ok_or("commande vide")?;
        let mut cmd = crate::os::command(first);
        cmd.args(&argv[1..]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(d) = cwd.filter(|d| d.is_dir()) {
            cmd.current_dir(d);
        }
        let mut child = cmd.spawn().map_err(|e| format!("{first} : {e}"))?;
        let stdin = child.stdin.take().ok_or("stdin")?;
        let stdout = child.stdout.take().ok_or("stdout")?;
        let stderr = child.stderr.take().ok_or("stderr")?;
        let (tx, rx) = channel();
        let (etx, erepaint) = (tx.clone(), repaint.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if etx.send(Incoming::Stderr(line)).is_err() {
                    break;
                }
                erepaint();
            }
        });
        let out = start(stdout, stdin, tx, repaint);
        Ok(Client { out, rx, seq: 0, child: Some(child) })
    }

    /// The adapter listens at `host:port` (`python -m debugpy --listen`).
    pub fn connect(host: &str, port: u16, repaint: impl Fn() + Send + Clone + 'static) -> Result<Client, String> {
        let addr = (host, port).to_socket_addrs().map_err(|e| format!("{host}:{port} : {e}"))?.next().ok_or_else(|| format!("{host}:{port} ?"))?;
        let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("{host}:{port} : {e}"))?;
        let r = s.try_clone().map_err(|e| e.to_string())?;
        let (tx, rx) = channel();
        let out = start(r, s, tx, repaint);
        Ok(Client { out, rx, seq: 0, child: None })
    }

    /// Sends a request; its sequence number, which its response carries.
    pub fn request(&mut self, command: &str, arguments: Value) -> i64 {
        self.seq += 1;
        let msg = json!({ "seq": self.seq, "type": "request", "command": command, "arguments": arguments });
        let _ = self.out.send(msg.to_string());
        self.seq
    }

    /// Answers a request of the adapter.
    pub fn respond(&mut self, request_seq: i64, command: &str, success: bool, body: Value) {
        self.seq += 1;
        let msg = json!({ "seq": self.seq, "type": "response", "request_seq": request_seq, "command": command, "success": success, "body": body });
        let _ = self.out.send(msg.to_string());
    }

    pub fn poll(&self) -> Vec<Incoming> {
        self.rx.try_iter().collect()
    }
}

fn start(reader: impl Read + Send + 'static, mut writer: impl Write + Send + 'static, tx: Sender<Incoming>, repaint: impl Fn() + Send + 'static) -> Sender<String> {
    let (out, out_rx) = channel::<String>();
    std::thread::spawn(move || {
        for msg in out_rx {
            let frame = format!("Content-Length: {}\r\n\r\n{msg}", msg.len());
            if writer.write_all(frame.as_bytes()).and_then(|_| writer.flush()).is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        let mut r = BufReader::new(reader);
        while let Some(msg) = crate::lsp::read_message(&mut r) {
            let Ok(v) = serde_json::from_str::<Value>(&msg) else { continue };
            if let Some(m) = parse(v) {
                if tx.send(m).is_err() {
                    return;
                }
                repaint();
            }
        }
        let _ = tx.send(Incoming::Closed);
        repaint();
    });
    out
}

fn parse(v: Value) -> Option<Incoming> {
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    Some(match v["type"].as_str()? {
        "response" => Incoming::Response {
            request_seq: v["request_seq"].as_i64()?,
            command: s("command"),
            success: v["success"].as_bool().unwrap_or(false),
            message: v["message"].as_str().or_else(|| v["body"]["error"]["format"].as_str()).unwrap_or("").to_string(),
            body: v["body"].clone(),
        },
        "event" => Incoming::Event { event: s("event"), body: v["body"].clone() },
        "request" => Incoming::Request { seq: v["seq"].as_i64()?, command: s("command"), arguments: v["arguments"].clone() },
        _ => return None,
    })
}

// ------------------------------------------------------------------ what is debugged

/// Where the program runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Local,
    /// In this Docker container.
    Docker(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Florm starts the program under the debugger.
    Launch,
    /// The program already runs with `python -m debugpy --listen <port>`.
    Attach,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub target: Target,
    pub mode: Mode,
    /// Launch: the program and its arguments ("./odoo-bin -c odoo.conf -d db").
    pub command: String,
    /// Launch here: the folder it runs in; in a container, the same folder there (mounts).
    pub cwd: PathBuf,
    /// Python of this machine, with debugpy.
    pub python: String,
    /// Attach: where debugpy listens (in the container, for Docker).
    pub host: String,
    pub port: u16,
    /// Only the code of the project (not Odoo's nor the libraries'), like VS Code's justMyCode.
    pub just_my_code: bool,
}

pub enum Transport {
    Spawn { argv: Vec<String>, cwd: Option<PathBuf> },
    Tcp(String, u16),
}

/// Ready to start: how to reach the adapter, and the request to send it.
pub struct Prepared {
    pub transport: Transport,
    /// "launch" or "attach".
    pub request: &'static str,
    pub args: Value,
    /// (folder in the container, folder here).
    pub mounts: Vec<(String, PathBuf)>,
}

#[derive(Debug)]
pub enum Failure {
    /// debugpy is not installed where the program runs: the command that installs it.
    NoDebugpy { install: Vec<String>, place: String },
    Other(String),
}

impl From<String> for Failure {
    fn from(s: String) -> Self {
        Failure::Other(s)
    }
}

/// Program run in a container that relays its input and output to debugpy listening there:
/// attaching to a program in a container needs no published port.
const RELAY: &str = r#"import os,socket,sys,threading
s=socket.create_connection(('127.0.0.1',int(sys.argv[1])))
def up():
    while True:
        d=os.read(0,65536)
        if not d:break
        s.sendall(d)
    s.shutdown(socket.SHUT_WR)
threading.Thread(target=up,daemon=True).start()
while True:
    d=s.recv(65536)
    if not d:break
    os.write(1,d)
"#;

/// Checks what is needed and finds the program (in the background: it runs `docker`).
pub fn prepare(cfg: &Config) -> Result<Prepared, Failure> {
    let common = |mut a: Value| {
        a["justMyCode"] = json!(cfg.just_my_code);
        a["showReturnValue"] = json!(true);
        a
    };
    match (&cfg.target, cfg.mode) {
        (Target::Local, Mode::Attach) => Ok(Prepared {
            transport: Transport::Tcp(if cfg.host.trim().is_empty() { "127.0.0.1".into() } else { cfg.host.trim().to_string() }, cfg.port),
            request: "attach",
            args: common(json!({})),
            mounts: Vec::new(),
        }),
        (Target::Docker(c), Mode::Attach) => {
            let mounts = crate::docker::running_mounts(c)?;
            let argv = ["docker", "exec", "-i", c, "python3", "-c", RELAY, &cfg.port.to_string()].map(String::from).to_vec();
            Ok(Prepared { transport: Transport::Spawn { argv, cwd: None }, request: "attach", args: common(json!({})), mounts })
        }
        (Target::Local, Mode::Launch) => {
            check(&[cfg.python.as_str(), "-c", "import debugpy"], || local_install(&cfg.python), t!("sur cet ordinateur", "on this computer"))?;
            let run = program(&cfg.command, |name| {
                let p = Path::new(name);
                let here = cfg.cwd.join(p);
                if p.is_absolute() {
                    Some(name.to_string())
                } else if here.is_file() {
                    Some(here.to_string_lossy().into_owned())
                } else {
                    crate::lsp::in_path(name).map(|p| p.to_string_lossy().into_owned())
                }
            })?;
            let args = common(run.launch_args(Some(&cfg.cwd.to_string_lossy())));
            let argv = vec![cfg.python.clone(), "-m".into(), "debugpy.adapter".into()];
            Ok(Prepared { transport: Transport::Spawn { argv, cwd: Some(cfg.cwd.clone()) }, request: "launch", args, mounts: Vec::new() })
        }
        (Target::Docker(c), Mode::Launch) => {
            let mounts = crate::docker::running_mounts(c)?;
            let docker = |args: &[&str]| -> Vec<String> { ["docker", "exec", c.as_str()].iter().chain(args).map(|s| s.to_string()).collect() };
            check(&docker(&["python3", "-c", "import debugpy"]).iter().map(String::as_str).collect::<Vec<_>>(), || docker_install(c), &tf!("dans le conteneur {c}", "in the container {c}"))?;
            let cwd = to_remote(&cfg.cwd, &mounts);
            let run = program(&cfg.command, |name| {
                if name.starts_with('/') {
                    return Some(name.to_string());
                }
                if name.contains('/') || name.ends_with(".py") {
                    return cwd.as_ref().map(|d| format!("{}/{}", d.trim_end_matches('/'), name.trim_start_matches("./")));
                }
                let found = crate::docker::exec_output(c, &["sh", "-c", &format!("command -v {name}")]).ok()?;
                Some(found.trim().to_string()).filter(|f| !f.is_empty())
            })?;
            let args = common(run.launch_args(cwd.as_deref()));
            let argv = ["docker", "exec", "-i", c, "python3", "-m", "debugpy.adapter"].map(String::from).to_vec();
            Ok(Prepared { transport: Transport::Spawn { argv, cwd: None }, request: "launch", args, mounts })
        }
    }
}

/// Runs `argv`; when it fails because debugpy is missing, says how to install it.
fn check(argv: &[&str], install: impl FnOnce() -> Vec<String>, place: &str) -> Result<(), Failure> {
    let out = crate::os::command(argv[0]).args(&argv[1..]).stdin(Stdio::null()).output().map_err(|e| Failure::Other(format!("{} : {e}", argv[0])))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if err.contains("No module named") {
        Err(Failure::NoDebugpy { install: install(), place: place.to_string() })
    } else {
        Err(Failure::Other(if err.is_empty() { format!("{} : code {:?}", argv.join(" "), out.status.code()) } else { err }))
    }
}

fn local_install(python: &str) -> Vec<String> {
    if cfg!(windows) {
        return [python, "-m", "pip", "install", "--user", "debugpy"].map(String::from).to_vec();
    }
    let py = format!("'{}'", python.replace('\'', r"'\''"));
    // a Python of its own (virtualenv, mise, pyenv…): into it, pip set up first if missing (no
    // --user: refused in a virtualenv). The system's: for the user with pip (which may refuse,
    // PEP 668: for the user only, all the same); without pip (Arch…), the package of the
    // distribution (its password asked in the terminal).
    let sh = format!(
        "case \"$(command -v {py})\" in \
         /usr/*) if {py} -m pip --version >/dev/null 2>&1; then {py} -m pip install --user debugpy || {py} -m pip install --user --break-system-packages debugpy; \
         elif command -v pacman >/dev/null; then sudo pacman -S --needed python-debugpy; \
         elif command -v apt-get >/dev/null; then sudo apt-get install -y python3-debugpy; \
         elif command -v dnf >/dev/null; then sudo dnf install -y python3-debugpy; \
         elif command -v zypper >/dev/null; then sudo zypper install -y python3-debugpy; \
         else {py} -m ensurepip --user && {py} -m pip install --user --break-system-packages debugpy; fi ;; \
         *) {{ {py} -m pip --version >/dev/null 2>&1 || {py} -m ensurepip; }} && {py} -m pip install debugpy ;; \
         esac"
    );
    ["sh", "-c", &sh].map(String::from).to_vec()
}

fn docker_install(container: &str) -> Vec<String> {
    // as root (the image's user often cannot write in Python's folders); pip may be missing from
    // an Odoo image: then the package of the distribution
    let sh = "python3 -m pip install debugpy || python3 -m pip install --break-system-packages debugpy || (apt-get update && apt-get install -y python3-debugpy)";
    ["docker", "exec", "-u", "root", "-it", container, "sh", "-c", sh].map(String::from).to_vec()
}

/// What to launch: a file or a module, its arguments, and the Python to run it with.
#[derive(Debug, PartialEq)]
pub struct Program {
    pub program: Option<String>,
    pub module: Option<String>,
    pub args: Vec<String>,
    /// The command starts with a Python of its own (a virtualenv).
    pub python: Option<String>,
}

impl Program {
    fn launch_args(&self, cwd: Option<&str>) -> Value {
        let mut a = json!({ "args": self.args, "console": "integratedTerminal" });
        if let Some(p) = &self.program {
            a["program"] = json!(p);
        }
        if let Some(m) = &self.module {
            a["module"] = json!(m);
        }
        if let Some(py) = &self.python {
            a["python"] = json!([py]);
        }
        if let Some(d) = cwd {
            a["cwd"] = json!(d);
        }
        a
    }
}

fn is_python(word: &str) -> bool {
    let name = Path::new(word).file_name().and_then(|n| n.to_str()).unwrap_or("");
    let name = name.strip_suffix(".exe").unwrap_or(name);
    name == "python" || name.strip_prefix("python").is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// The program of `command`: `python -m x …` is the module x, `python file …` and `file …` the
/// file, found by `find` (a name alone is looked for in the PATH).
pub fn program(command: &str, find: impl Fn(&str) -> Option<String>) -> Result<Program, String> {
    let mut words = crate::odoo_test::split(command.trim());
    if words.is_empty() {
        return Err(t!("Indiquez le programme à lancer (ex. ./odoo-bin -c odoo.conf)", "Enter the program to run (e.g. ./odoo-bin -c odoo.conf)").into());
    }
    let mut python = None;
    if is_python(&words[0]) {
        let py = words.remove(0);
        // the Python of this machine is the one debugpy uses already
        if py.contains('/') || py.contains('\\') {
            python = Some(py);
        }
        if words.first().map(String::as_str) == Some("-m") && words.len() >= 2 {
            let module = words[1].clone();
            return Ok(Program { program: None, module: Some(module), args: words[2..].to_vec(), python });
        }
        if words.is_empty() {
            return Err(t!("Python sans programme à lancer", "Python without a program to run").into());
        }
    }
    let name = words.remove(0);
    let path = find(&name).ok_or_else(|| tf!("Programme introuvable : {name}", "Program not found: {name}"))?;
    Ok(Program { program: Some(path), module: None, args: words, python })
}

/// The command that runs the program in a terminal (`runInTerminal`): there, or in the container
/// with `docker exec -it`; the folder and the variables to give it here.
pub fn terminal_command(target: &Target, args: &Value) -> (Vec<String>, Option<PathBuf>, Vec<(String, String)>) {
    let argv: Vec<String> = args["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let cwd = args["cwd"].as_str().filter(|c| !c.is_empty());
    let env: Vec<(String, String)> = args["env"].as_object().map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
    match target {
        Target::Local => (argv, cwd.map(PathBuf::from), env),
        Target::Docker(c) => {
            let mut a = vec!["docker".to_string(), "exec".into(), "-it".into()];
            if let Some(d) = cwd {
                a.extend(["-w".into(), d.to_string()]);
            }
            for (k, v) in env {
                a.extend(["-e".into(), format!("{k}={v}")]);
            }
            a.push(c.clone());
            a.extend(argv);
            (a, None, Vec::new())
        }
    }
}

/// A file or folder here, as the container sees it (through its mounts, the longest first).
pub fn to_remote(local: &Path, mounts: &[(String, PathBuf)]) -> Option<String> {
    let (dest, src) = mounts.iter().filter(|(_, src)| local.starts_with(src)).max_by_key(|(_, src)| src.as_os_str().len())?;
    let rest = local.strip_prefix(src).ok()?;
    let mut out = dest.trim_end_matches('/').to_string();
    for c in rest.components() {
        out.push('/');
        out.push_str(&c.as_os_str().to_string_lossy());
    }
    if out.is_empty() {
        out.push('/');
    }
    Some(out)
}

/// The path to give the debugger for a file here.
pub fn remote_path(local: &Path, target: &Target, mounts: &[(String, PathBuf)]) -> String {
    match target {
        Target::Docker(_) => to_remote(local, mounts).unwrap_or_else(|| local.to_string_lossy().into_owned()),
        Target::Local => local.to_string_lossy().into_owned(),
    }
}

/// A path of the debugger, here.
pub fn local_path(remote: &str, target: &Target, mounts: &[(String, PathBuf)]) -> PathBuf {
    match target {
        Target::Docker(_) => crate::odoo_test::map_path(remote, mounts),
        Target::Local => PathBuf::from(remote),
    }
}

/// The expression to show the value of for the word `start..end` (chars) of `line`: the word
/// with what leads to it ("self.partner_id.name" for "name").
pub fn hover_expression(line: &str, start: usize, end: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    // the word may be a dotted name already ("partner_id.name")
    if start >= end || end > chars.len() || !chars[start..end].iter().all(|&c| ident(c) || c == '.') || chars[start] == '.' || chars[end - 1] == '.' {
        return None;
    }
    let mut s = start;
    while s >= 2 && chars[s - 1] == '.' && ident(chars[s - 2]) {
        s -= 1;
        while s > 0 && ident(chars[s - 1]) {
            s -= 1;
        }
    }
    let expr: String = chars[s..end].iter().collect();
    const KEYWORDS: &[&str] = &["def", "class", "return", "import", "from", "if", "elif", "else", "for", "while", "in", "is", "not", "and", "or", "with", "as", "try", "except", "finally", "raise", "pass", "lambda", "yield", "None", "True", "False", "async", "await", "del", "global", "nonlocal", "assert", "break", "continue"];
    (!expr.starts_with(|c: char| c.is_ascii_digit()) && !KEYWORDS.contains(&expr.as_str())).then_some(expr)
}

/// `odoo-bin` in the folder or a folder under it (two levels): the command to launch Odoo with.
pub fn find_odoo_bin(root: &Path) -> Option<String> {
    let mut level = vec![root.to_path_buf()];
    for _ in 0..3 {
        let mut next = Vec::new();
        for d in &level {
            if d.join("odoo-bin").is_file() {
                let rel = d.strip_prefix(root).ok()?.join("odoo-bin");
                return Some(format!("./{}", rel.to_string_lossy().replace('\\', "/")));
            }
            if let Ok(rd) = std::fs::read_dir(d) {
                let mut subs: Vec<PathBuf> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.')).map(|e| e.path()).collect();
                subs.sort();
                next.extend(subs);
            }
        }
        level = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn debugpy_install_script_is_valid_shell() {
        for python in ["python3", "/home/x/.venv/bin/python", "/opt/it's/python"] {
            let argv = local_install(python);
            let ok = std::process::Command::new("sh").args(["-n", "-c", &argv[2]]).status().unwrap().success();
            assert!(ok, "{python}: {}", argv[2]);
        }
        assert!(local_install("python3")[2].contains("sudo pacman -S --needed python-debugpy"));
    }

    #[test]
    fn programs() {
        let find = |n: &str| Some(format!("/bin/{n}"));
        let p = program("./odoo-bin -c 'mon odoo.conf' -d db", find).unwrap();
        assert_eq!(p.program.as_deref(), Some("/bin/./odoo-bin"));
        assert_eq!(p.args, ["-c", "mon odoo.conf", "-d", "db"]);
        let p = program("python3 -m odoo --test-tags /sale", find).unwrap();
        assert_eq!((p.module.as_deref(), p.program, p.python), (Some("odoo"), None, None));
        assert_eq!(p.args, ["--test-tags", "/sale"]);
        let p = program("/venv/bin/python odoo-bin shell", find).unwrap();
        assert_eq!(p.python.as_deref(), Some("/venv/bin/python"));
        assert_eq!(p.program.as_deref(), Some("/bin/odoo-bin"));
        assert!(program("nothing", |_| None).is_err());
        assert!(program("  ", find).is_err());
        assert!(is_python("python3.12") && is_python("python.exe") && !is_python("pythonista"));
    }

    #[test]
    fn paths_through_mounts() {
        let mounts = vec![("/mnt/extra-addons".to_string(), PathBuf::from("/home/me/addons")), ("/opt/src".to_string(), PathBuf::from("/home/me"))];
        assert_eq!(to_remote(Path::new("/home/me/addons/sale/models/a.py"), &mounts).as_deref(), Some("/mnt/extra-addons/sale/models/a.py"));
        assert_eq!(to_remote(Path::new("/home/me/x.py"), &mounts).as_deref(), Some("/opt/src/x.py"));
        assert_eq!(to_remote(Path::new("/elsewhere/x.py"), &mounts), None);
        let t = Target::Docker("web".into());
        assert_eq!(remote_path(Path::new("/elsewhere/x.py"), &t, &mounts), "/elsewhere/x.py");
        assert_eq!(local_path("/mnt/extra-addons/sale/a.py", &t, &mounts), PathBuf::from("/home/me/addons/sale/a.py"));
        assert_eq!(local_path("/mnt/extra-addons/sale/a.py", &Target::Local, &mounts), PathBuf::from("/mnt/extra-addons/sale/a.py"));
    }

    #[test]
    fn terminal_in_the_container() {
        let args = json!({ "args": ["python3", "/x/launcher", "4242", "--", "/usr/bin/odoo"], "cwd": "/opt", "env": { "A": "1", "B": null } });
        let (argv, cwd, env) = terminal_command(&Target::Docker("web".into()), &args);
        assert_eq!(argv, ["docker", "exec", "-it", "-w", "/opt", "-e", "A=1", "web", "python3", "/x/launcher", "4242", "--", "/usr/bin/odoo"]);
        assert!(cwd.is_none() && env.is_empty());
        let (argv, cwd, env) = terminal_command(&Target::Local, &args);
        assert_eq!(argv[0], "python3");
        assert_eq!(cwd, Some(PathBuf::from("/opt")));
        assert_eq!(env, [("A".to_string(), "1".to_string())]);
    }

    #[test]
    fn hover_expressions() {
        let l = "        total = self.partner_id.name + x2";
        let at = |w: &str| {
            let s = l.find(w).unwrap();
            hover_expression(l, s, s + w.len())
        };
        assert_eq!(at("name").as_deref(), Some("self.partner_id.name"));
        assert_eq!(at("partner_id").as_deref(), Some("self.partner_id"));
        assert_eq!(at("total").as_deref(), Some("total"));
        assert_eq!(hover_expression("def f():", 0, 3), None);
        assert_eq!(hover_expression("x = 12", 4, 6), None);
    }

    #[test]
    fn odoo_bin_found() {
        let root = std::env::temp_dir().join(format!("florm_dap_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("odoo/18.0")).unwrap();
        assert_eq!(find_odoo_bin(&root), None);
        std::fs::write(root.join("odoo/18.0/odoo-bin"), "").unwrap();
        assert_eq!(find_odoo_bin(&root).as_deref(), Some("./odoo/18.0/odoo-bin"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Real debugpy: `FLORM_TEST_DEBUGPY=<python with debugpy> cargo test dap`.
    #[test]
    fn debugpy_end_to_end() {
        let Ok(python) = std::env::var("FLORM_TEST_DEBUGPY") else { return };
        let dir = std::env::temp_dir().join(format!("florm_dap_e2e_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("t.py");
        std::fs::write(&file, "def f(x):\n    y = x * 2\n    return y\n\nprint(f(21))\n").unwrap();
        let cfg = Config { target: Target::Local, mode: Mode::Launch, command: "t.py".into(), cwd: dir.clone(), python, host: String::new(), port: 5678, just_my_code: false };
        let p = prepare(&cfg).unwrap_or_else(|e| panic!("{e:?}"));
        let Transport::Spawn { argv, cwd } = p.transport else { panic!() };
        let mut c = Client::spawn(&argv, cwd.as_deref(), || {}).unwrap();
        c.request("initialize", json!({ "adapterID": "debugpy", "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path", "supportsRunInTerminalRequest": true }));
        let launch = c.request(p.request, p.args.clone());
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut child = None;
        let (mut stopped_line, mut value) = (None, None);
        while std::time::Instant::now() < deadline && value.is_none() {
            for m in c.poll() {
                match m {
                    Incoming::Request { seq, command, arguments } if command == "runInTerminal" => {
                        let (argv, cwd, env) = terminal_command(&Target::Local, &arguments);
                        let mut cmd = std::process::Command::new(&argv[0]);
                        cmd.args(&argv[1..]).envs(env).stdout(Stdio::null());
                        if let Some(d) = cwd {
                            cmd.current_dir(d);
                        }
                        child = Some(cmd.spawn().unwrap());
                        c.respond(seq, &command, true, json!({}));
                    }
                    Incoming::Event { event, .. } if event == "initialized" => {
                        c.request("setBreakpoints", json!({ "source": { "path": file.to_string_lossy() }, "breakpoints": [{ "line": 3 }] }));
                        c.request("configurationDone", json!({}));
                    }
                    Incoming::Event { event, body } if event == "stopped" => {
                        c.request("stackTrace", json!({ "threadId": body["threadId"] }));
                    }
                    Incoming::Response { command, body, .. } if command == "stackTrace" => {
                        let top = &body["stackFrames"][0];
                        stopped_line = top["line"].as_i64();
                        c.request("evaluate", json!({ "expression": "y + 1", "frameId": top["id"], "context": "repl" }));
                    }
                    Incoming::Response { command, body, .. } if command == "evaluate" => value = body["result"].as_str().map(String::from),
                    Incoming::Response { request_seq, success: false, message, .. } if request_seq == launch => panic!("launch : {message}"),
                    Incoming::Closed => panic!("adaptateur arrêté"),
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        c.request("disconnect", json!({ "terminateDebuggee": true }));
        std::thread::sleep(Duration::from_millis(300));
        if let Some(mut ch) = child {
            let _ = ch.kill();
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(stopped_line, Some(3));
        assert_eq!(value.as_deref(), Some("43"));
    }
}
