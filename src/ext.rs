//! Extensions: programs that add commands to Florm and react to what happens in it. Each one is a
//! folder of `~/.config/florm/extensions/` with an `extension.toml` (its name, the command that
//! starts it, its commands, the events it wants) and its code, in any language: Florm talks to
//! it in JSON-RPC over stdin/stdout, framed like a language server (`Content-Length`). A small
//! Python library (`florm.py`, on the PYTHONPATH of the extensions) makes one a few lines.
//!
//! Florm → extension: `initialize` (request), `command` {id, context} (request, answered when
//! done), `event` {name, context} (notification).
//! Extension → Florm: `status`, `message`, `output`, `editor/text`, `editor/replace_selection`,
//! `editor/insert`, `editor/set_text`, `open`, `pick`, `input`, `run`, `terminal` (see
//! `app/extensions.rs`).

use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};

/// The events an extension can ask for.
pub const EVENTS: &[&str] = &["open", "save", "close", "focus"];

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ExtCommand {
    pub id: String,
    pub title: String,
    /// A shortcut ("ctrl+alt+h").
    #[serde(default)]
    pub key: String,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The command that starts it, run in its folder ("python3 main.py").
    pub cmd: String,
    #[serde(default, rename = "command")]
    pub commands: Vec<ExtCommand>,
    /// open, save, close, focus.
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub version: String,
    #[serde(skip)]
    pub dir: PathBuf,
}

/// The folder of the extensions.
pub fn dir() -> PathBuf {
    crate::config::config_dir().join("extensions")
}

/// The extensions of `dir` (a folder each, with its extension.toml), and those that cannot be read.
pub fn load_all(dir: &Path) -> (Vec<Manifest>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return (out, errors) };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("extension.toml").is_file()).collect();
    dirs.sort();
    for d in dirs {
        let file = d.join("extension.toml");
        match std::fs::read_to_string(&file).map_err(|e| e.to_string()).and_then(|t| toml::from_str::<Manifest>(&t).map_err(|e| e.to_string())) {
            Ok(mut m) => {
                if m.name.trim().is_empty() {
                    m.name = d.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
                }
                if let Some(bad) = m.events.iter().find(|e| !EVENTS.contains(&e.as_str())) {
                    errors.push(crate::tf!("{} : événement inconnu « {bad} » ({})", "{}: unknown event “{bad}” ({})", file.display(), EVENTS.join(", ")));
                }
                m.dir = d;
                out.push(m);
            }
            Err(e) => errors.push(format!("{} : {}", file.display(), e.lines().next().unwrap_or(""))),
        }
    }
    (out, errors)
}

/// The folder where Florm puts its library for the extensions (`florm.py`).
pub fn lib_dir() -> PathBuf {
    crate::config::data_dir().join("extensions-lib")
}

pub const FLORM_PY: &str = include_str!("../assets/florm_ext.py");

/// `florm.py` written (again when Florm changed it).
fn write_lib() -> PathBuf {
    let d = lib_dir();
    let f = d.join("florm.py");
    if std::fs::read_to_string(&f).ok().as_deref() != Some(FLORM_PY) {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(&f, FLORM_PY);
    }
    d
}

/// What comes from the extensions, read on their threads.
pub enum Incoming {
    Msg(usize, Value),
    /// A line it wrote on stderr (its print(), its errors).
    Log(usize, String),
    Exited(usize),
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
}

/// The extensions and their processes (started when first needed).
pub struct Host {
    pub list: Vec<Manifest>,
    pub errors: Vec<String>,
    procs: Vec<Option<Proc>>,
    next_id: i64,
    tx: Sender<Incoming>,
    rx: Receiver<Incoming>,
}

impl Default for Host {
    fn default() -> Self {
        let (tx, rx) = channel();
        Host { list: Vec::new(), errors: Vec::new(), procs: Vec::new(), next_id: 1, tx, rx }
    }
}

impl Host {
    /// Reads the folder again (the running ones are stopped).
    pub fn reload(&mut self, dir: &Path) {
        self.stop_all();
        let (list, errors) = load_all(dir);
        self.procs = list.iter().map(|_| None).collect();
        self.list = list;
        self.errors = errors;
    }

    pub fn running(&self, k: usize) -> bool {
        self.procs.get(k).is_some_and(|p| p.is_some())
    }

    /// Starts extension `k` if it is not running. Err: why it cannot.
    pub fn ensure(&mut self, k: usize, ctx: &eframe::egui::Context) -> Result<(), String> {
        if self.running(k) {
            return Ok(());
        }
        let m = self.list.get(k).ok_or("?")?.clone();
        let mut argv = crate::test_cmd::split(&m.cmd);
        if argv.is_empty() {
            return Err(crate::tf!("{} : cmd est vide dans extension.toml", "{}: cmd is empty in extension.toml", m.name));
        }
        // "python3" is "python" on Windows
        if matches!(argv[0].as_str(), "python" | "python3") {
            argv[0] = crate::lint::python().to_string();
        }
        let lib = write_lib();
        let pythonpath = match std::env::var_os("PYTHONPATH") {
            Some(p) => std::env::join_paths(std::iter::once(lib.clone()).chain(std::env::split_paths(&p))).unwrap_or_else(|_| lib.clone().into_os_string()),
            None => lib.clone().into_os_string(),
        };
        let prog = crate::lsp::in_path(&argv[0]).or_else(|| Some(m.dir.join(&argv[0])).filter(|p| p.is_file())).unwrap_or_else(|| PathBuf::from(&argv[0]));
        let mut child = crate::os::command(prog.to_str().unwrap_or(&argv[0]))
            .args(&argv[1..])
            .current_dir(&m.dir)
            .env("PYTHONPATH", pythonpath)
            .env("PYTHONUNBUFFERED", "1")
            .env("FLORM_EXTENSION", &m.name)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| crate::tf!("{} : « {} » ne démarre pas : {e}", "{}: “{}” does not start: {e}", m.name, m.cmd))?;
        let stdout = child.stdout.take().ok_or("stdout")?;
        let stderr = child.stderr.take().ok_or("stderr")?;
        let stdin = child.stdin.take().ok_or("stdin")?;
        let (tx, c) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            while let Some(text) = crate::lsp::read_message(&mut r) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if tx.send(Incoming::Msg(k, v)).is_err() {
                        return;
                    }
                    c.request_repaint();
                }
            }
            let _ = tx.send(Incoming::Exited(k));
            c.request_repaint();
        });
        let (tx, c) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut r = BufReader::new(stderr);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match std::io::BufRead::read_until(&mut r, b'\n', &mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                        if tx.send(Incoming::Log(k, line)).is_err() {
                            return;
                        }
                        c.request_repaint();
                    }
                }
            }
        });
        if let Some(slot) = self.procs.get_mut(k) {
            *slot = Some(Proc { child, stdin });
        }
        let lang = if crate::i18n::en() { "en" } else { "fr" };
        self.request(k, "initialize", json!({"florm": env!("CARGO_PKG_VERSION"), "lang": lang, "name": m.name}));
        Ok(())
    }

    fn send(&mut self, k: usize, msg: Value) {
        let Some(Some(p)) = self.procs.get_mut(k) else { return };
        let body = msg.to_string();
        let frame = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        if p.stdin.write_all(frame.as_bytes()).and_then(|_| p.stdin.flush()).is_err() {
            self.stop(k);
        }
    }

    pub fn notify(&mut self, k: usize, method: &str, params: Value) {
        self.send(k, json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    pub fn request(&mut self, k: usize, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(k, json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    pub fn respond(&mut self, k: usize, id: Value, result: Result<Value, String>) {
        let msg = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": e}}),
        };
        self.send(k, msg);
    }

    /// What came since the last frame.
    pub fn poll(&mut self) -> Vec<Incoming> {
        let v: Vec<Incoming> = self.rx.try_iter().collect();
        for m in &v {
            if let Incoming::Exited(k) = m {
                if let Some(slot) = self.procs.get_mut(*k) {
                    if let Some(mut p) = slot.take() {
                        let _ = p.child.wait();
                    }
                }
            }
        }
        v
    }

    pub fn stop(&mut self, k: usize) {
        if let Some(slot) = self.procs.get_mut(k) {
            if let Some(mut p) = slot.take() {
                let _ = p.child.kill();
                let _ = p.child.wait();
            }
        }
    }

    pub fn stop_all(&mut self) {
        for k in 0..self.procs.len() {
            self.stop(k);
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop_all();
    }
}

/// An error answered by an extension: its message (and the Python trace under it, for the log).
pub fn error_text(v: &Value) -> String {
    let e = &v["error"];
    let msg = e["message"].as_str().map(String::from).unwrap_or_else(|| e.to_string());
    match e["data"]["traceback"].as_str() {
        Some(tb) => format!("{msg}\n{tb}"),
        None => msg,
    }
}

/// The example `+ Nouvelle extension` writes: its extension.toml and its main.py.
pub const EXAMPLE_TOML: &str = r#"# An extension of Florm: its name, the command that starts it (in this folder), its commands
# (in the palette, Ctrl+Shift+P; key: a shortcut), the events it wants (open, save, close, focus).
name = "Mon extension"
description = "Exemple : met la sélection en majuscules, compte les lignes à l'enregistrement"
cmd = "python3 main.py"
events = ["save"]

[[command]]
id = "upper"
title = "Mettre la sélection en majuscules"
key = "ctrl+alt+u"

[[command]]
id = "pick"
title = "Choisir un mot à insérer"
"#;

pub const EXAMPLE_PY: &str = r#"# An extension of Florm in Python: florm.py (given by Florm) does the talking.
# print() goes to Output (bottom panel). After a change: Preferences → Extensions → ⟳.
import florm


@florm.command("upper")
def upper(ctx):
    if not ctx.selection:
        florm.message("Sélectionne du texte d'abord", "warning")
        return
    florm.replace_selection(ctx.selection.upper())


@florm.command("pick")
def pick(ctx):
    words = ["TODO", "FIXME", "NOTE"]
    k = florm.pick(words, "Mot à insérer")
    if k is not None:
        florm.insert(words[k])


@florm.on("save")
def saved(ctx):
    lines = florm.text().count("\n") + 1
    florm.status(f"{ctx.file} : {lines} lignes")


florm.main()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Extensions read from their folders; a broken one said, the others kept.
    #[test]
    fn extensions_read() {
        let dir = std::env::temp_dir().join(format!("florm_ext_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, toml) in [("a", EXAMPLE_TOML), ("b", "name = ["), ("c", "name = \"C\"\ncmd = \"x\"\nevents = [\"boom\"]\n")] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
            std::fs::write(dir.join(name).join("extension.toml"), toml).unwrap();
        }
        std::fs::create_dir_all(dir.join("not_one")).unwrap();
        let (list, errors) = load_all(&dir);
        assert_eq!(list.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Mon extension", "C"]);
        assert_eq!(list[0].commands.len(), 2);
        assert_eq!(list[0].commands[0].key, "ctrl+alt+u");
        assert_eq!(errors.len(), 2, "{errors:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
