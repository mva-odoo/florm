//! Language Server Protocol client (completion, go to definition, rename): one server process
//! per language family and root folder, JSON-RPC over stdio, read and written on threads.

use ropey::Rope;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// A server nobody asked for this long, with no document open on it, is stopped (it restarts when needed).
const IDLE_STOP: Duration = Duration::from_secs(10 * 60);
/// How often the servers are looked at for that.
const IDLE_CHECK: Duration = Duration::from_secs(60);

/// Language family (one server each) and LSP language id of a file.
pub fn language_of(path: &Path) -> Option<(&'static str, &'static str)> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    Some(match ext.as_str() {
        "py" | "pyi" => ("python", "python"),
        "js" | "mjs" | "cjs" => ("typescript", "javascript"),
        "jsx" => ("typescript", "javascriptreact"),
        "ts" | "mts" | "cts" => ("typescript", "typescript"),
        "tsx" => ("typescript", "typescriptreact"),
        "rs" => ("rust", "rust"),
        "go" => ("go", "go"),
        "c" | "h" => ("c", "c"),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => ("c", "cpp"),
        _ => return None,
    })
}

/// Executable in PATH.
pub fn in_path(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat", ""] } else { &[""] };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|d| {
        exts.iter().map(|e| d.join(format!("{name}{e}"))).find(|p| p.is_file())
    })
}

/// Settings given to the servers. Florm shows the problems of its linters (flake8, ruff…), not
/// the servers' own: they are asked not to compute them (less CPU and memory, no duplicates).
pub fn light_settings() -> Value {
    let off = json!({"enabled": false});
    let analysis = json!({"diagnosticMode": "openFilesOnly", "typeCheckingMode": "off", "autoSearchPaths": true});
    json!({
        "pylsp": {"plugins": {
            "pycodestyle": off, "pyflakes": off, "mccabe": off, "pylint": off, "flake8": off,
            "pydocstyle": off, "autopep8": off, "yapf": off
        }},
        "python": {"analysis": analysis},
        "basedpyright": {"analysis": analysis}
    })
}

/// The value of a section ("python.analysis") of `light_settings`, for `workspace/configuration`.
fn setting(section: &str) -> Value {
    let mut v = light_settings();
    for key in section.split('.').filter(|k| !k.is_empty()) {
        v = v.get(key).cloned().unwrap_or(Value::Null);
    }
    v
}

/// Command line (program, args, extra env) of the server for a family.
pub struct ServerSpec {
    pub name: String,
    pub cmd: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// pylsp installed by Florm in its private tools folder.
pub fn private_pylsp() -> bool {
    crate::lint::tools_dir().join("pylsp").is_dir()
}

pub fn find_server(family: &str, overrides: &HashMap<String, String>) -> Option<ServerSpec> {
    if let Some(cmd) = overrides.get(family).filter(|c| !c.trim().is_empty()) {
        let cmd: Vec<String> = cmd.split_whitespace().map(String::from).collect();
        return Some(ServerSpec { name: cmd[0].clone(), cmd, env: Vec::new() });
    }
    let simple = |bin: &str, args: &[&str]| {
        in_path(bin).map(|p| ServerSpec {
            name: bin.to_string(),
            cmd: std::iter::once(p.display().to_string()).chain(args.iter().map(|s| s.to_string())).collect(),
            env: Vec::new(),
        })
    };
    match family {
        "python" => simple("pyright-langserver", &["--stdio"])
            .or_else(|| simple("basedpyright-langserver", &["--stdio"]))
            .or_else(|| simple("pylsp", &[]))
            .or_else(|| {
                private_pylsp().then(|| ServerSpec {
                    name: "pylsp".into(),
                    cmd: vec![crate::lint::python().into(), "-m".into(), "pylsp".into()],
                    env: vec![("PYTHONPATH".into(), crate::lint::tools_dir().display().to_string())],
                })
            }),
        "typescript" => simple("typescript-language-server", &["--stdio"]),
        "rust" => simple("rust-analyzer", &[]),
        "go" => simple("gopls", &[]),
        "c" => simple("clangd", &[]),
        _ => None,
    }
}

// ------------------------------------------------------------------ positions and URIs

pub fn path_to_uri(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    let mut out = String::from(if s.starts_with('/') { "file://" } else { "file:///" });
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let s = String::from_utf8(out).ok()?;
    // "/C:/x" on Windows
    let s = if s.len() > 2 && s.as_bytes()[0] == b'/' && s.as_bytes()[2] == b':' { s[1..].to_string() } else { s };
    Some(PathBuf::from(s))
}

/// LSP position (line, UTF-16 column) of a char index.
pub fn to_position(rope: &Rope, ch: usize) -> Value {
    let ch = ch.min(rope.len_chars());
    let line = rope.char_to_line(ch);
    let col: usize = rope.slice(rope.line_to_char(line)..ch).chars().map(char::len_utf16).sum();
    json!({ "line": line, "character": col })
}

/// Char index of an LSP position (clamped to the line).
pub fn from_position(rope: &Rope, pos: &Value) -> usize {
    let line = (pos["line"].as_u64().unwrap_or(0) as usize).min(rope.len_lines().saturating_sub(1));
    let mut left = pos["character"].as_u64().unwrap_or(0) as usize;
    let start = rope.line_to_char(line);
    let mut idx = start;
    for c in rope.line(line).chars() {
        if left == 0 || c == '\n' {
            break;
        }
        left = left.saturating_sub(c.len_utf16());
        idx += 1;
    }
    idx
}

// ------------------------------------------------------------------ client

/// Message from a server, for the UI thread.
pub enum Incoming {
    Response { id: i64, result: Result<Value, String> },
    /// The server asks for a workspace edit to be made (after a command of a code action).
    ApplyEdit(Value),
    Exited(String),
}

pub struct Client {
    pub family: String,
    pub root: PathBuf,
    pub name: String,
    pub ready: bool,
    out: Sender<String>,
    next_id: i64,
    /// Messages waiting for the end of `initialize`.
    queue: Vec<String>,
    init_id: i64,
    /// Document uid -> (version sent, file).
    pub opened: HashMap<u64, (u64, PathBuf)>,
    /// Last request or change sent to it.
    used: Instant,
    child: Child,
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.out.send(json!({"jsonrpc": "2.0", "method": "exit"}).to_string());
        let _ = self.child.kill();
    }
}

impl Client {
    fn spawn(idx: usize, family: &str, root: &Path, spec: ServerSpec, events: Sender<(usize, Incoming)>, repaint: impl Fn() + Send + 'static + Clone) -> Result<Client, String> {
        let mut cmd = crate::os::command(&spec.cmd[0]);
        cmd.args(&spec.cmd[1..])
            .envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|e| format!("{} : {e}", spec.name))?;
        let mut stdin = child.stdin.take().ok_or("stdin")?;
        let stdout = child.stdout.take().ok_or("stdout")?;
        let (out, out_rx) = channel::<String>();
        std::thread::spawn(move || {
            for msg in out_rx {
                let frame = format!("Content-Length: {}\r\n\r\n{msg}", msg.len());
                if stdin.write_all(frame.as_bytes()).and_then(|_| stdin.flush()).is_err() {
                    break;
                }
            }
        });
        let reply = out.clone();
        let name = spec.name.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let Some(msg) = read_message(&mut r) else { break };
                let Ok(v) = serde_json::from_str::<Value>(&msg) else { continue };
                match (v.get("id"), v.get("method")) {
                    // request from the server: answer so that it does not wait
                    (Some(id), Some(m)) => {
                        let result = match m.as_str().unwrap_or("") {
                            "workspace/configuration" => {
                                let items = v["params"]["items"].as_array().cloned().unwrap_or_default();
                                Value::Array(items.iter().map(|it| setting(it["section"].as_str().unwrap_or(""))).collect())
                            }
                            "workspace/workspaceFolders" => Value::Null,
                            "workspace/applyEdit" => {
                                let _ = events.send((idx, Incoming::ApplyEdit(v["params"]["edit"].clone())));
                                repaint();
                                json!({"applied": true})
                            }
                            _ => Value::Null,
                        };
                        let _ = reply.send(json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string());
                    }
                    (Some(id), None) => {
                        let id = id.as_i64().unwrap_or(-1);
                        let result = match v.get("error") {
                            Some(e) => Err(e["message"].as_str().unwrap_or(t!("erreur du serveur", "server error")).to_string()),
                            None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = events.send((idx, Incoming::Response { id, result }));
                        repaint();
                    }
                    _ => {}
                }
            }
            let _ = events.send((idx, Incoming::Exited(name)));
            repaint();
        });
        let mut c = Client {
            family: family.to_string(),
            root: root.to_path_buf(),
            name: spec.name,
            ready: false,
            out,
            next_id: 1,
            queue: Vec::new(),
            init_id: 0,
            opened: HashMap::new(),
            used: Instant::now(),
            child,
        };
        let root_uri = path_to_uri(root);
        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "rootPath": root.display().to_string(),
            "workspaceFolders": [{ "uri": root_uri, "name": root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() }],
            "clientInfo": { "name": "florm" },
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] },
                "workspace": { "workspaceEdit": { "documentChanges": true }, "configuration": true, "workspaceFolders": true, "applyEdit": true, "executeCommand": {} },
                "textDocument": {
                    "synchronization": { "didSave": true },
                    "completion": {
                        "completionItem": { "snippetSupport": false, "documentationFormat": ["plaintext"], "insertReplaceSupport": false },
                        "contextSupport": true
                    },
                    "definition": { "linkSupport": true },
                    "rename": { "prepareSupport": false },
                    "codeAction": {
                        "codeActionLiteralSupport": { "codeActionKind": { "valueSet": ["", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source", "source.organizeImports", "source.fixAll"] } },
                        "resolveSupport": { "properties": ["edit"] },
                        "dataSupport": true,
                        "isPreferredSupport": true
                    }
                }
            }
        });
        c.init_id = c.send_now("initialize", params);
        Ok(c)
    }

    fn send_now(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        let _ = self.out.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string());
        id
    }

    /// Sends a request (queued until the server is initialized); returns its id.
    pub fn request(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        if self.ready {
            let _ = self.out.send(msg);
        } else {
            self.queue.push(msg);
        }
        id
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params}).to_string();
        if self.ready {
            let _ = self.out.send(msg);
        } else {
            self.queue.push(msg);
        }
    }

    /// Opens or updates a document on the server (whole text).
    pub fn sync(&mut self, uid: u64, path: &Path, language: &str, version: u64, text: impl FnOnce() -> String) {
        match self.opened.get(&uid).map(|o| o.0) {
            Some(v) if v == version => {}
            Some(_) => {
                self.notify(
                    "textDocument/didChange",
                    json!({ "textDocument": { "uri": path_to_uri(path), "version": version }, "contentChanges": [{ "text": text() }] }),
                );
                self.opened.insert(uid, (version, path.to_path_buf()));
            }
            None => {
                self.notify(
                    "textDocument/didOpen",
                    json!({ "textDocument": { "uri": path_to_uri(path), "languageId": language, "version": version, "text": text() } }),
                );
                self.opened.insert(uid, (version, path.to_path_buf()));
            }
        }
    }

    /// Tells the server that the documents not in `alive` were closed.
    pub fn close_missing(&mut self, alive: &HashSet<u64>) {
        let gone: Vec<u64> = self.opened.keys().filter(|u| !alive.contains(u)).copied().collect();
        for uid in gone {
            if let Some((_, path)) = self.opened.remove(&uid) {
                self.notify("textDocument/didClose", json!({ "textDocument": { "uri": path_to_uri(&path) } }));
            }
        }
    }
}

fn read_message(r: &mut impl BufRead) -> Option<String> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let t = line.trim_end();
        if t.is_empty() {
            break;
        }
        if let Some(v) = t.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse::<usize>().ok();
        }
    }
    let mut buf = vec![0; len?];
    r.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

/// All the servers of the session.
pub struct Manager {
    pub clients: Vec<Option<Client>>,
    events_tx: Sender<(usize, Incoming)>,
    events: Receiver<(usize, Incoming)>,
    /// (family, root) whose server is missing or crashed: not retried until `reset`.
    pub failed: HashSet<(String, PathBuf)>,
    /// When the idle servers were last looked at.
    checked: Instant,
}

impl Default for Manager {
    fn default() -> Self {
        let (events_tx, events) = channel();
        Manager { clients: Vec::new(), events_tx, events, failed: HashSet::new(), checked: Instant::now() }
    }
}

impl Manager {
    /// Index of the running server for (family, root), started if needed. Err: why there is none.
    pub fn ensure(&mut self, family: &str, root: &Path, overrides: &HashMap<String, String>, ctx: &eframe::egui::Context) -> Result<usize, String> {
        if let Some(i) = self.clients.iter().position(|c| c.as_ref().is_some_and(|c| c.family == family && c.root == root)) {
            return Ok(i);
        }
        let key = (family.to_string(), root.to_path_buf());
        if self.failed.contains(&key) {
            return Err(String::new());
        }
        let Some(spec) = find_server(family, overrides) else {
            self.failed.insert(key);
            return Err(tf!("aucun serveur de langage pour {family}", "no language server for {family}"));
        };
        let idx = self.clients.len();
        let ctx = ctx.clone();
        match Client::spawn(idx, family, root, spec, self.events_tx.clone(), move || ctx.request_repaint()) {
            Ok(c) => {
                self.clients.push(Some(c));
                Ok(idx)
            }
            Err(e) => {
                self.failed.insert(key);
                Err(e)
            }
        }
    }

    pub fn client(&mut self, i: usize) -> Option<&mut Client> {
        let c = self.clients.get_mut(i)?.as_mut()?;
        c.used = Instant::now();
        Some(c)
    }

    /// Stops the servers unused for `IDLE_STOP` and with no document open: they use memory for
    /// nothing. The slot stays (the indexes of the others do not move); `ensure` starts it again.
    fn stop_idle(&mut self) {
        if self.checked.elapsed() < IDLE_CHECK {
            return;
        }
        self.checked = Instant::now();
        for slot in self.clients.iter_mut() {
            if slot.as_ref().is_some_and(|c| c.opened.is_empty() && c.used.elapsed() >= IDLE_STOP) {
                *slot = None;
            }
        }
    }

    /// Responses received since the last call (the end of `initialize` is handled here).
    pub fn poll(&mut self) -> Vec<(usize, Incoming)> {
        self.stop_idle();
        let mut out = Vec::new();
        while let Ok((i, msg)) = self.events.try_recv() {
            match &msg {
                Incoming::Response { id, .. } => {
                    if let Some(c) = self.clients.get_mut(i).and_then(|c| c.as_mut()) {
                        if *id == c.init_id {
                            c.ready = true;
                            let _ = c.out.send(json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}).to_string());
                            let settings = json!({"jsonrpc": "2.0", "method": "workspace/didChangeConfiguration", "params": {"settings": light_settings()}});
                            let _ = c.out.send(settings.to_string());
                            for m in std::mem::take(&mut c.queue) {
                                let _ = c.out.send(m);
                            }
                            continue;
                        }
                    }
                }
                Incoming::ApplyEdit(_) => {}
                Incoming::Exited(_) => {
                    if let Some(c) = self.clients.get_mut(i).and_then(|c| c.take()) {
                        self.failed.insert((c.family.clone(), c.root.clone()));
                    }
                }
            }
            out.push((i, msg));
        }
        out
    }

    /// Forget the failures (after installing a server, or changing the settings).
    pub fn reset(&mut self) {
        self.failed.clear();
        self.clients.clear();
    }
}

// ------------------------------------------------------------------ results

#[derive(Clone, Debug)]
pub struct CompletionItem {
    pub label: String,
    pub detail: String,
    pub kind: u64,
    pub filter: String,
    pub sort: String,
    pub insert: String,
    /// Range replaced (from the server), else the word before the cursor.
    pub range: Option<Value>,
    /// Body of a Florm template, expanded when accepted (variables, indentation, `$0`).
    pub template: Option<String>,
}

impl CompletionItem {
    pub fn icon(&self) -> &'static str {
        match self.kind {
            2 | 3 | 4 => "ƒ",
            5 | 10 => "·",
            6 => "x",
            7 | 8 | 22 => "C",
            9 => "M",
            14 => "k",
            21 => "π",
            15 => "»",
            _ => "·",
        }
    }
}

/// Items of a completion response; the bool says the list is incomplete (ask again while typing).
pub fn parse_completion(v: &Value) -> (Vec<CompletionItem>, bool) {
    let (items, incomplete) = match v {
        Value::Array(a) => (a.clone(), false),
        Value::Object(_) => (v["items"].as_array().cloned().unwrap_or_default(), v["isIncomplete"].as_bool().unwrap_or(false)),
        _ => (Vec::new(), false),
    };
    let items = items
        .iter()
        .map(|it| {
            let label = it["label"].as_str().unwrap_or("").to_string();
            let (insert, range) = match &it["textEdit"] {
                Value::Object(te) => (
                    te.get("newText").and_then(|t| t.as_str()).unwrap_or(&label).to_string(),
                    te.get("range").or_else(|| te.get("replace")).cloned(),
                ),
                _ => (it["insertText"].as_str().unwrap_or(&label).to_string(), None),
            };
            CompletionItem {
                detail: it["detail"].as_str().unwrap_or("").to_string(),
                kind: it["kind"].as_u64().unwrap_or(0),
                filter: it["filterText"].as_str().unwrap_or(&label).to_string(),
                sort: it["sortText"].as_str().unwrap_or(&label).to_string(),
                insert,
                range,
                label,
                template: None,
            }
        })
        .collect();
    (items, incomplete)
}

/// First location of a definition response: (file, LSP position).
pub fn parse_location(v: &Value) -> Option<(PathBuf, Value)> {
    let first = match v {
        Value::Array(a) => a.first()?.clone(),
        Value::Object(_) => v.clone(),
        _ => return None,
    };
    let uri = first.get("uri").or_else(|| first.get("targetUri"))?.as_str()?;
    let range = first.get("targetSelectionRange").or_else(|| first.get("range"))?;
    Some((uri_to_path(uri)?, range["start"].clone()))
}

/// Every location of a list (references): (path, start position).
pub fn parse_locations(v: &Value) -> Vec<(PathBuf, Value)> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|l| {
                    let uri = l.get("uri").or_else(|| l.get("targetUri"))?.as_str()?;
                    let range = l.get("targetSelectionRange").or_else(|| l.get("range"))?;
                    Some((uri_to_path(uri)?, range["start"].clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Text of a hover answer (markdown, plain text or the older "marked strings"), without the
/// code fences.
pub fn parse_hover(v: &Value) -> String {
    fn text(c: &Value) -> String {
        match c {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.iter().map(text).filter(|t| !t.trim().is_empty()).collect::<Vec<_>>().join("\n\n"),
            Value::Object(o) => o.get("value").and_then(Value::as_str).unwrap_or("").to_string(),
            _ => String::new(),
        }
    }
    let raw = text(&v["contents"]);
    raw.lines().filter(|l| !l.trim_start().starts_with("```")).collect::<Vec<_>>().join("\n").trim().to_string()
}

/// Text edits of a workspace edit, per file: (path, [(range, new text)]).
pub fn parse_workspace_edit(v: &Value) -> Vec<(PathBuf, Vec<(Value, String)>)> {
    let mut out: Vec<(PathBuf, Vec<(Value, String)>)> = Vec::new();
    let mut add = |uri: &str, edits: &Value| {
        let Some(p) = uri_to_path(uri) else { return };
        let list: Vec<(Value, String)> = edits
            .as_array()
            .map(|a| a.iter().map(|e| (e["range"].clone(), e["newText"].as_str().unwrap_or("").to_string())).collect())
            .unwrap_or_default();
        match out.iter_mut().find(|(q, _)| *q == p) {
            Some((_, l)) => l.extend(list),
            None => out.push((p, list)),
        }
    };
    if let Some(changes) = v["changes"].as_object() {
        for (uri, edits) in changes {
            add(uri, edits);
        }
    }
    if let Some(dc) = v["documentChanges"].as_array() {
        for d in dc {
            if let Some(uri) = d["textDocument"]["uri"].as_str() {
                add(uri, &d["edits"]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_and_references_answers() {
        let md = serde_json::json!({ "contents": { "kind": "markdown", "value": "```python\ndef f(a: int) -> str\n```\nDoes things." } });
        assert_eq!(parse_hover(&md), "def f(a: int) -> str\nDoes things.");
        let old = serde_json::json!({ "contents": [{ "language": "python", "value": "x: int" }, "doc"] });
        assert_eq!(parse_hover(&old), "x: int\n\ndoc");
        assert_eq!(parse_hover(&serde_json::json!(null)), "");
        let refs = serde_json::json!([
            { "uri": "file:///a/b.py", "range": { "start": { "line": 3, "character": 4 }, "end": { "line": 3, "character": 7 } } },
            { "uri": "file:///a/c.py", "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } } }
        ]);
        let l = parse_locations(&refs);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].1["line"], 3);
    }

    #[test]
    fn positions_are_utf16() {
        let rope = Rope::from_str("é😀x\nab\n");
        // 😀 is 2 UTF-16 units: "x" is at column 3
        assert_eq!(to_position(&rope, 2), json!({"line": 0, "character": 3}));
        assert_eq!(from_position(&rope, &json!({"line": 0, "character": 3})), 2);
        assert_eq!(from_position(&rope, &json!({"line": 1, "character": 1})), 5);
        assert_eq!(from_position(&rope, &json!({"line": 1, "character": 99})), 6);
    }

    #[test]
    fn uris() {
        let p = Path::new("/home/a b/é.py");
        let u = path_to_uri(p);
        assert_eq!(u, "file:///home/a%20b/%C3%A9.py");
        assert_eq!(uri_to_path(&u).unwrap(), p);
    }

    #[test]
    fn parse_results() {
        let (items, inc) = parse_completion(&json!({"isIncomplete": true, "items": [
            {"label": "append", "kind": 2, "detail": "list.append"},
            {"label": "x", "textEdit": {"newText": "xyz", "range": {"start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 2}}}}
        ]}));
        assert!(inc);
        assert_eq!((items[0].insert.as_str(), items[0].icon()), ("append", "ƒ"));
        assert_eq!(items[1].insert, "xyz");
        assert!(items[1].range.is_some());
        let (p, pos) = parse_location(&json!([{"targetUri": "file:///a/b.py", "targetRange": {}, "targetSelectionRange": {"start": {"line": 4, "character": 2}}}])).unwrap();
        assert_eq!((p, pos), (PathBuf::from("/a/b.py"), json!({"line": 4, "character": 2})));
        let e = parse_workspace_edit(&json!({"documentChanges": [{"textDocument": {"uri": "file:///a.py", "version": 1}, "edits": [{"range": {}, "newText": "n"}]}]}));
        assert_eq!(e[0].0, PathBuf::from("/a.py"));
        assert_eq!(e[0].1[0].1, "n");
    }

    #[test]
    fn framing() {
        let mut r = std::io::Cursor::new(b"Content-Length: 7\r\nContent-Type: x\r\n\r\n{\"a\":1}Content-Length: 2\r\n\r\n{}".to_vec());
        assert_eq!(read_message(&mut r).as_deref(), Some("{\"a\":1}"));
        assert_eq!(read_message(&mut r).as_deref(), Some("{}"));
        assert!(read_message(&mut r).is_none());
    }

    /// Real server: `FLORM_TEST_PYLSP=<folder with pylsp installed by pip --target> cargo test`.
    #[test]
    fn pylsp_end_to_end() {
        let Ok(dir) = std::env::var("FLORM_TEST_PYLSP") else { return };
        let root = std::env::temp_dir().join(format!("florm_lsp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let src = "def helper(x):\n    return x\n\n\nclass Thing:\n    def method(self):\n        pass\n\n\nhelper(1)\nt = Thing()\nt.me\n";
        let file = root.join("a.py");
        std::fs::write(&file, src).unwrap();
        let rope = Rope::from_str(src);
        let spec = ServerSpec { name: "pylsp".into(), cmd: vec!["python3".into(), "-m".into(), "pylsp".into()], env: vec![("PYTHONPATH".into(), dir)] };
        let (tx, rx) = channel();
        let mut c = Client::spawn(0, "python", &root, spec, tx, || {}).unwrap();
        let wait = |rx: &Receiver<(usize, Incoming)>, want: i64| -> Value {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                match rx.recv_timeout(left).expect("pas de réponse du serveur") {
                    (_, Incoming::Response { id, result }) if id == want => return result.unwrap(),
                    (_, Incoming::Exited(n)) => panic!("{n} s'est arrêté"),
                    _ => {}
                }
            }
        };
        wait(&rx, c.init_id);
        c.ready = true;
        c.notify("initialized", json!({}));
        c.sync(1, &file, "python", 1, || src.to_string());
        let doc = json!({ "uri": path_to_uri(&file) });
        // completion after "t.me"
        let end = rope.line_to_char(11) + 4;
        let id = c.request("textDocument/completion", json!({ "textDocument": doc, "position": to_position(&rope, end) }));
        let (items, _) = parse_completion(&wait(&rx, id));
        assert!(items.iter().any(|i| i.label.starts_with("method")), "{:?}", items.iter().map(|i| &i.label).collect::<Vec<_>>());
        // definition of "helper" in "helper(1)"
        let at = rope.line_to_char(9) + 2;
        let id = c.request("textDocument/definition", json!({ "textDocument": doc, "position": to_position(&rope, at) }));
        let (p, pos) = parse_location(&wait(&rx, id)).expect("pas de définition");
        assert_eq!(p.canonicalize().unwrap(), file.canonicalize().unwrap());
        assert_eq!(pos["line"], 0);
        // rename "helper" -> "aide": the definition and the call
        let id = c.request("textDocument/rename", json!({ "textDocument": doc, "position": to_position(&rope, at), "newName": "aide" }));
        let edits = parse_workspace_edit(&wait(&rx, id));
        assert_eq!(edits.len(), 1);
        let new: String = {
            // apply like the editor: from the end so that positions stay valid
            let mut r = rope.clone();
            let mut list: Vec<(usize, usize, String)> = edits[0]
                .1
                .iter()
                .map(|(range, t)| (from_position(&r, &range["start"]), from_position(&r, &range["end"]), t.clone()))
                .collect();
            list.sort_by_key(|e| std::cmp::Reverse(e.0));
            for (a, b, t) in list {
                r.remove(a..b);
                r.insert(a, &t);
            }
            r.to_string()
        };
        assert!(new.starts_with("def aide(x):") && new.contains("\naide(1)\n") && !new.contains("helper"), "{new}");
        drop(c);
        let _ = std::fs::remove_dir_all(&root);
    }
}
