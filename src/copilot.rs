//! GitHub Copilot's completion (the grey text after the cursor), through its language server
//! `copilot-language-server`: one process kept running, spoken to in LSP. Florm downloads the
//! server itself (its standalone build, from npm's registry, checked against the registry's
//! SHA-512) into its data folder: no npm, no node. The sign-in to GitHub is kept by the server.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The build of the server for this system, as named on npm (`None`: no build for it).
fn platform() -> Option<&'static str> {
    let os = if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return None;
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        return None;
    };
    Some(match (os, arch) {
        ("win32", "x64") => "win32-x64",
        ("win32", _) => "win32-arm64",
        ("darwin", "x64") => "darwin-x64",
        ("darwin", _) => "darwin-arm64",
        ("linux", "x64") => "linux-x64",
        _ => "linux-arm64",
    })
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "copilot-language-server.exe"
    } else {
        "copilot-language-server"
    }
}

/// Where Florm puts the server it downloads.
fn install_dir() -> PathBuf {
    crate::config::data_dir().join("copilot")
}

/// The server: the one Florm downloaded, else one on the PATH (installed with npm).
pub fn program() -> Option<PathBuf> {
    let own = install_dir().join("package").join(exe_name());
    if own.is_file() {
        return Some(own);
    }
    crate::tools::locate("copilot-language-server").then(|| PathBuf::from("copilot-language-server"))
}

/// Downloads the standalone server for this system from npm's registry (about 160 MB), checks
/// it and unpacks it into Florm's data folder.
pub fn install() -> Result<(), String> {
    let plat = platform().ok_or_else(|| t!("Copilot n'existe pas pour ce système", "Copilot does not exist for this system").to_string())?;
    let meta_url = format!("https://registry.npmjs.org/@github/copilot-language-server-{plat}/latest");
    let meta: Value = serde_json::from_str(&curl_text(&meta_url)?).map_err(|e| e.to_string())?;
    let tarball = meta["dist"]["tarball"].as_str().ok_or("npm : pas de fichier")?.to_string();
    let integrity = meta["dist"]["integrity"].as_str().unwrap_or("").to_string();
    let dir = install_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("server.tgz");
    let out = crate::os::command("curl")
        .args(["-fsSL", "--max-time", "900", "-o"])
        .arg(&file)
        .arg(&tarball)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| t!("curl est introuvable", "curl was not found").to_string())?;
    if !out.status.success() {
        return Err(tf!("Téléchargement impossible : {}", "Cannot download: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    // the SHA-512 npm gives ("sha512-<base64>"): a broken or changed download is not run
    if let Some(want) = integrity.strip_prefix("sha512-").and_then(base64) {
        use sha2::{Digest, Sha512};
        let got = Sha512::digest(std::fs::read(&file).map_err(|e| e.to_string())?);
        if got.as_slice() != want.as_slice() {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(t!("Le fichier téléchargé est abîmé (empreinte différente) : réessaie", "The downloaded file is damaged (different checksum): try again").into());
        }
    }
    let out = crate::os::command("tar").arg("-xzf").arg(&file).arg("-C").arg(&dir).stdin(Stdio::null()).output().map_err(|_| t!("tar est introuvable", "tar was not found").to_string())?;
    let _ = std::fs::remove_file(&file);
    if !out.status.success() {
        return Err(tf!("Décompression impossible : {}", "Cannot unpack: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let exe = dir.join("package").join(exe_name());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    }
    if !exe.is_file() {
        return Err(t!("Le serveur Copilot est introuvable dans le fichier téléchargé", "The Copilot server is not in the downloaded file").into());
    }
    // a server started before (another version) is left: the next request starts this one
    *SERVER.lock().unwrap() = None;
    Ok(())
}

fn curl_text(url: &str) -> Result<String, String> {
    let out = crate::os::command("curl")
        .args(["-fsSL", "--max-time", "30", url])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| t!("curl est introuvable", "curl was not found").to_string())?;
    if !out.status.success() {
        return Err(tf!("Le registre npm ne répond pas : {}", "npm's registry does not answer: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Standard base64 decoded (`None`: not base64).
fn base64(text: &str) -> Option<Vec<u8>> {
    crate::claude::base64_decode(text)
}

/// The server running: requests sent from any thread, answers dispatched by id.
struct Server {
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Arc<Mutex<HashMap<u64, Sender<Value>>>>,
    next: AtomicU64,
    /// Documents opened in the server: their version.
    docs: Mutex<HashMap<String, i64>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.lock().unwrap().kill();
    }
}

static SERVER: Mutex<Option<Arc<Server>>> = Mutex::new(None);

fn write_msg(stdin: &Mutex<ChildStdin>, msg: &Value) -> Result<(), String> {
    let body = msg.to_string();
    let mut s = stdin.lock().unwrap();
    write!(s, "Content-Length: {}\r\n\r\n{body}", body.len()).and_then(|_| s.flush()).map_err(|e| e.to_string())
}

impl Server {
    fn start() -> Result<Arc<Server>, String> {
        let exe = program().ok_or_else(|| t!("Copilot n'est pas installé", "Copilot is not installed").to_string())?;
        let mut child = crate::os::command(exe.to_string_lossy().as_ref())
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| tf!("Copilot ne démarre pas : {e}", "Copilot does not start: {e}"))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("stdin")?));
        let stdout = child.stdout.take().ok_or("stdout")?;
        let pending: Arc<Mutex<HashMap<u64, Sender<Value>>>> = Arc::default();
        let (p, w) = (pending.clone(), stdin.clone());
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        // the server ended: whoever waits gets an error
                        p.lock().unwrap().clear();
                        return;
                    }
                    let line = line.trim();
                    if line.is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; len];
                if r.read_exact(&mut body).is_err() {
                    p.lock().unwrap().clear();
                    return;
                }
                let Ok(msg) = serde_json::from_slice::<Value>(&body) else { continue };
                match (msg.get("id"), msg.get("method")) {
                    // a request of the server (configuration, a message to show…): answered empty
                    (Some(id), Some(_)) => {
                        let _ = write_msg(&w, &json!({"jsonrpc": "2.0", "id": id, "result": null}));
                    }
                    (Some(id), None) => {
                        if let Some(tx) = id.as_u64().and_then(|id| p.lock().unwrap().remove(&id)) {
                            let _ = tx.send(msg);
                        }
                    }
                    _ => {}
                }
            }
        });
        let s = Arc::new(Server { child: Mutex::new(child), stdin, pending, next: AtomicU64::new(1), docs: Mutex::default() });
        let version = env!("CARGO_PKG_VERSION");
        s.request(
            "initialize",
            json!({"processId": std::process::id(), "rootUri": null, "capabilities": {"workspace": {"workspaceFolders": true}},
                "initializationOptions": {"editorInfo": {"name": "Florm", "version": version}, "editorPluginInfo": {"name": "florm", "version": version}}}),
            Duration::from_secs(30),
        )?;
        s.notify("initialized", json!({}))?;
        Ok(s)
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        write_msg(&self.stdin, &json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    fn request(&self, method: &str, params: Value, wait: Duration) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);
        write_msg(&self.stdin, &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let msg = rx.recv_timeout(wait).map_err(|_| {
            self.pending.lock().unwrap().remove(&id);
            t!("Copilot ne répond pas", "Copilot does not answer").to_string()
        })?;
        if let Some(e) = msg.get("error") {
            return Err(e["message"].as_str().unwrap_or("Copilot").to_string());
        }
        Ok(msg["result"].clone())
    }
}

/// The server, started when first needed (again if it ended).
fn server() -> Result<Arc<Server>, String> {
    let mut slot = SERVER.lock().unwrap();
    if let Some(s) = slot.as_ref() {
        if s.child.lock().unwrap().try_wait().ok().flatten().is_none() {
            return Ok(s.clone());
        }
    }
    let s = Server::start()?;
    *slot = Some(s.clone());
    Ok(s)
}

/// Where Copilot stands: "OK" (signed in), "NotSignedIn", "NotAuthorized" (no Copilot plan)…
pub fn status() -> Result<String, String> {
    let r = server()?.request("checkStatus", json!({}), Duration::from_secs(30))?;
    Ok(r["status"].as_str().unwrap_or("").to_string())
}

/// Starts the sign-in to GitHub: the code to type and the page to type it in.
pub fn sign_in() -> Result<(String, String), String> {
    let r = server()?.request("signIn", json!({}), Duration::from_secs(30))?;
    if r["status"] == "AlreadySignedIn" || r["status"] == "OK" {
        return Ok((String::new(), String::new()));
    }
    let code = r["userCode"].as_str().ok_or("Copilot : pas de code")?.to_string();
    let uri = r["verificationUri"].as_str().unwrap_or("https://github.com/login/device").to_string();
    Ok((code, uri))
}

/// Waits for the code to be typed on GitHub (15 minutes at most), then says where Copilot stands.
pub fn finish_sign_in() -> Result<String, String> {
    let s = server()?;
    s.request("workspace/executeCommand", json!({"command": "github.copilot.finishDeviceFlow", "arguments": []}), Duration::from_secs(900))?;
    status()
}

pub fn sign_out() -> Result<(), String> {
    server()?.request("signOut", json!({}), Duration::from_secs(30)).map(|_| ())
}

/// The LSP name of a language of the editor (its syntax name).
fn language_id(syntax: &str) -> String {
    let s = syntax.to_ascii_lowercase();
    match s.as_str() {
        "bourne again shell (bash)" | "shell-unix-generic" | "bash" => "shellscript".into(),
        "c++" => "cpp".into(),
        "c#" => "csharp".into(),
        "javascript (babel)" => "javascript".into(),
        "typescriptreact" | "tsx" => "typescriptreact".into(),
        _ => s.split_whitespace().next().unwrap_or("plaintext").to_string(),
    }
}

/// What Copilot suggests to insert at `line`, `col` (UTF-16 units, as LSP counts) of the file
/// `path` holding `text`; empty when nothing.
pub fn complete(path: &Path, syntax: &str, text: &str, line: usize, col: usize, tab_size: usize, spaces: bool) -> Result<String, String> {
    let s = server()?;
    let uri = file_uri(path);
    let version = {
        let mut docs = s.docs.lock().unwrap();
        let v = docs.entry(uri.clone()).or_insert(0);
        *v += 1;
        *v
    };
    if version == 1 {
        s.notify("textDocument/didOpen", json!({"textDocument": {"uri": uri, "languageId": language_id(syntax), "version": version, "text": text}}))?;
    } else {
        s.notify("textDocument/didChange", json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]}))?;
    }
    let r = s.request(
        "textDocument/inlineCompletion",
        json!({"textDocument": {"uri": uri, "version": version}, "position": {"line": line, "character": col},
            "context": {"triggerKind": 2}, "formattingOptions": {"tabSize": tab_size, "insertSpaces": spaces}}),
        Duration::from_secs(15),
    )?;
    let items = r.get("items").unwrap_or(&r);
    let Some(item) = items.as_array().and_then(|a| a.first()) else { return Ok(String::new()) };
    let insert = item["insertText"].as_str().unwrap_or("");
    let current = text.split('\n').nth(line).unwrap_or("");
    let start = item["range"]["start"]["character"].as_u64().map(|c| c as usize).filter(|_| item["range"]["start"]["line"].as_u64() == Some(line as u64));
    Ok(after_cursor(insert, current, start, col))
}

/// The part of `insert` after the cursor: Copilot gives the text from `start` (often the start of
/// the line), what is already typed up to the cursor (`col`) included.
fn after_cursor(insert: &str, line: &str, start: Option<usize>, col: usize) -> String {
    let Some(start) = start.filter(|s| *s < col) else { return insert.to_string() };
    let units: Vec<u16> = line.encode_utf16().collect();
    let typed = String::from_utf16_lossy(&units[start.min(units.len())..col.min(units.len())]);
    insert.strip_prefix(typed.as_str()).unwrap_or(insert).to_string()
}

fn file_uri(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    let p = if p.starts_with('/') { p } else { format!("/{p}") };
    let mut out = String::from("file://");
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_of_npm() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("Zm9vYmFy").unwrap(), b"foobar");
        assert!(base64("pas*base64").is_none());
    }

    #[test]
    fn the_text_typed_is_not_given_again() {
        // from the start of the line: what is typed goes
        assert_eq!(after_cursor("    return a + b", "    return ", Some(0), 11), "a + b");
        // from the cursor: all of it
        assert_eq!(after_cursor("a + b", "    return ", Some(11), 11), "a + b");
        assert_eq!(after_cursor("a + b", "x", None, 1), "a + b");
        // characters outside the BMP count twice in UTF-16
        assert_eq!(after_cursor("é😀 ok", "é😀", Some(0), 3), " ok");
    }

    #[test]
    fn uris_and_languages() {
        assert_eq!(file_uri(Path::new("/home/u/mon projet/a.py")), "file:///home/u/mon%20projet/a.py");
        assert_eq!(language_id("Python"), "python");
        assert_eq!(language_id("Bourne Again Shell (bash)"), "shellscript");
        assert_eq!(language_id("C++"), "cpp");
    }
}
