//! Debugging beyond Python, on this computer: Node (js-debug, the debugger of VS Code, downloaded
//! once), Rust and C (lldb-dap, else the debug adapter built into gdb 14+), Go (delve). The
//! program is set by a command like in a terminal: `cargo run`, `cargo test x`, `go run .`,
//! `go test ./pkg -run TestX`, `node app.js`, `npm start`, `npx jest file -t name`.

use crate::dap::{Config, Failure, Mode, Prepared, Target, Transport};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lang {
    Python,
    Node,
    /// Rust, C, C++: a program compiled here.
    Native,
    Go,
}

impl Lang {
    pub fn name(self) -> &'static str {
        match self {
            Lang::Python => "Python",
            Lang::Node => "Node",
            Lang::Native => "Rust / C",
            Lang::Go => "Go",
        }
    }

    /// The `adapterID` given to the adapter.
    pub fn adapter_id(self) -> &'static str {
        match self {
            Lang::Python => "debugpy",
            Lang::Node => "pwa-node",
            Lang::Native => "lldb",
            Lang::Go => "go",
        }
    }
}

fn split(s: &str) -> Vec<String> {
    crate::odoo_test::split(s.trim())
}

fn stem(word: &str) -> String {
    let name = Path::new(word).file_name().and_then(|n| n.to_str()).unwrap_or(word);
    name.strip_suffix(".exe").unwrap_or(name).to_string()
}

/// The language of `command`, when its program tells it.
pub fn of_command(command: &str) -> Option<Lang> {
    let name = stem(split(command).first()?);
    Some(match name.as_str() {
        "node" | "npm" | "npx" | "yarn" | "pnpm" | "bun" | "tsx" | "ts-node" => Lang::Node,
        "cargo" => Lang::Native,
        "go" => Lang::Go,
        "pytest" | "odoo-bin" | "odoo" => Lang::Python,
        n if n.starts_with("python") || n.ends_with(".py") => Lang::Python,
        n if [".js", ".mjs", ".cjs", ".ts"].iter().any(|e| n.ends_with(e)) => Lang::Node,
        _ => return None,
    })
}

/// The language of the project at `root`, from its files.
pub fn of_project(root: &Path) -> Lang {
    if root.join("Cargo.toml").is_file() {
        Lang::Native
    } else if root.join("go.mod").is_file() {
        Lang::Go
    } else if root.join("package.json").is_file() && !root.join("odoo-bin").is_file() {
        Lang::Node
    } else {
        Lang::Python
    }
}

/// The command debugged when none is set, for a project in `lang` (Python: see `find_odoo_bin`).
pub fn default_command(lang: Lang, root: &Path) -> String {
    match lang {
        Lang::Native if root.join("Cargo.toml").is_file() => "cargo run".into(),
        Lang::Go if root.join("go.mod").is_file() => "go run .".into(),
        Lang::Node => {
            let pkg: Value = std::fs::read_to_string(root.join("package.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
            if let Some(main) = pkg["main"].as_str().filter(|m| root.join(m).is_file()) {
                format!("node {main}")
            } else if pkg["scripts"]["start"].is_string() {
                "npm start".into()
            } else if root.join("index.js").is_file() {
                "node index.js".into()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

/// An example of command, shown in the empty field.
pub fn example(lang: Lang) -> &'static str {
    match lang {
        Lang::Python => "python3 main.py",
        Lang::Node => "node app.js",
        Lang::Native => "cargo run",
        Lang::Go => "go run .",
    }
}

/// Checks what is needed and finds the program (in the background: it may build it, download
/// js-debug).
pub fn prepare(cfg: &Config) -> Result<Prepared, Failure> {
    let lang = cfg.lang.name();
    if cfg.target != Target::Local || cfg.mode != Mode::Launch {
        return Err(Failure::Other(crate::tf!(
            "{lang} : le débogueur lance le programme sur cet ordinateur (pas de conteneur ni de « s'attacher » pour ce langage)",
            "{lang}: the debugger runs the program on this computer (no container nor “attach” for this language)"
        )));
    }
    let words = split(&cfg.command);
    if words.is_empty() {
        return Err(Failure::Other(crate::tf!("Indique la commande à lancer (ex. {})", "Enter the command to run (e.g. {})", example(cfg.lang))));
    }
    match cfg.lang {
        Lang::Native => native(&words, &cfg.cwd),
        Lang::Go => go(&words, &cfg.cwd),
        Lang::Node => node(&words, &cfg.cwd),
        Lang::Python => Err(Failure::Other("Python".into())),
    }
}

fn in_path(name: &str) -> Option<String> {
    crate::lsp::in_path(name).map(|p| p.to_string_lossy().into_owned())
}

/// A free port of this computer, for an adapter started as a server.
fn free_port() -> Result<u16, Failure> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| Failure::Other(e.to_string()))?;
    Ok(l.local_addr().map_err(|e| Failure::Other(e.to_string()))?.port())
}

/// `rel` from the folder `cwd`, as an absolute path.
fn absolute(rel: &str, cwd: &Path) -> String {
    let p = Path::new(rel);
    if p.is_absolute() { rel.to_string() } else { cwd.join(p).to_string_lossy().into_owned() }
}

/// The command that installs a package of the system with the package manager there is.
fn system_install(pkgs: &[(&str, &str)]) -> Vec<String> {
    if cfg!(windows) {
        return ["winget", "install", "LLVM.LLVM"].map(String::from).to_vec();
    }
    if cfg!(target_os = "macos") {
        return ["xcode-select", "--install"].map(String::from).to_vec();
    }
    let sh: Vec<String> = pkgs
        .iter()
        .map(|(pm, pkg)| match *pm {
            "pacman" => format!("command -v pacman >/dev/null && sudo pacman -S --needed {pkg}"),
            "apt-get" => format!("command -v apt-get >/dev/null && sudo apt-get install -y {pkg}"),
            "dnf" => format!("command -v dnf >/dev/null && sudo dnf install -y {pkg}"),
            _ => format!("command -v zypper >/dev/null && sudo zypper install -y {pkg}"),
        })
        .collect();
    ["sh".to_string(), "-c".into(), sh.join(" || ")].to_vec()
}

// ------------------------------------------------------------------ Rust, C

/// lldb-dap (or its old name lldb-vscode), else gdb when it has its adapter (version 14 and up).
fn native_adapter() -> Result<Vec<String>, Failure> {
    for name in ["lldb-dap", "lldb-vscode"] {
        if let Some(p) = in_path(name) {
            return Ok(vec![p]);
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(out) = crate::os::command("xcrun").args(["-f", "lldb-dap"]).stdin(Stdio::null()).output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !p.is_empty() {
            return Ok(vec![p]);
        }
    }
    if let Some(gdb) = in_path("gdb") {
        let version = crate::os::command(&gdb).arg("--version").stdin(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        if gdb_has_dap(&version) {
            return Ok(vec![gdb, "--interpreter=dap".into(), "--quiet".into()]);
        }
    }
    Err(Failure::Missing {
        tool: "gdb".into(),
        why: crate::t!("il débogue les programmes Rust et C (lldb-dap convient aussi)", "it debugs the Rust and C programs (lldb-dap works too)").into(),
        install: system_install(&[("pacman", "gdb"), ("apt-get", "gdb"), ("dnf", "gdb"), ("zypper", "gdb")]),
    })
}

/// "GNU gdb (GDB) 18.1": a version with the debug adapter (14 and up).
fn gdb_has_dap(version: &str) -> bool {
    let first = version.lines().next().unwrap_or("");
    let v = first.split_whitespace().rev().find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit())).unwrap_or("");
    v.split('.').next().and_then(|m| m.parse::<u32>().ok()).is_some_and(|m| m >= 14)
}

fn native(words: &[String], cwd: &Path) -> Result<Prepared, Failure> {
    let adapter = native_adapter()?;
    let (program, args) = if stem(&words[0]) == "cargo" { cargo_build(words, cwd)? } else { (absolute(&words[0], cwd), words[1..].to_vec()) };
    let args = json!({ "program": program, "args": args, "cwd": cwd.to_string_lossy() });
    Ok(Prepared { transport: Transport::Spawn { argv: adapter, cwd: Some(cwd.to_path_buf()) }, request: "launch", args, mounts: Vec::new() })
}

/// The options of cargo followed by a value.
const CARGO_VALUED: [&str; 12] = ["--bin", "--example", "--test", "--bench", "-p", "--package", "--features", "-F", "--target", "--profile", "--manifest-path", "-j"];

/// `cargo run …` / `cargo test …`: the program built (`cargo build`, `cargo test --no-run`) and
/// its arguments (after `--`; for the tests, the filter and one thread).
fn cargo_build(words: &[String], cwd: &Path) -> Result<(String, Vec<String>), Failure> {
    let test = words.get(1).map(String::as_str) == Some("test");
    let (opts, after) = match words.iter().position(|w| w == "--") {
        Some(k) => (&words[2.min(words.len())..k], words[k + 1..].to_vec()),
        None => (&words[2.min(words.len())..], Vec::new()),
    };
    let mut cargo: Vec<String> = if test { vec!["test".into(), "--no-run".into()] } else { vec!["build".into()] };
    let mut filters = Vec::new();
    let mut k = 0;
    while k < opts.len() {
        let w = &opts[k];
        if CARGO_VALUED.contains(&w.as_str()) {
            cargo.push(w.clone());
            if let Some(v) = opts.get(k + 1) {
                cargo.push(v.clone());
            }
            k += 2;
            continue;
        }
        if w.starts_with('-') { cargo.push(w.clone()) } else { filters.push(w.clone()) }
        k += 1;
    }
    cargo.push("--message-format=json-render-diagnostics".into());
    let out = crate::os::command(&words[0]).args(&cargo).current_dir(cwd).stdin(Stdio::null()).output().map_err(|e| Failure::Other(format!("cargo : {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).collect();
        return Err(Failure::Other(crate::tf!("La compilation a échoué :\n{}", "The build failed:\n{}", tail[tail.len().saturating_sub(15)..].join("\n"))));
    }
    let exe = pick_artifact(&String::from_utf8_lossy(&out.stdout), test).ok_or_else(|| Failure::Other(crate::t!("cargo n'a produit aucun programme à lancer", "cargo made no program to run").into()))?;
    let mut args = filters;
    if test {
        args.push("--test-threads=1".into());
    }
    args.extend(after);
    Ok((exe, args))
}

/// The program among what cargo built (its JSON messages): a binary, the tests of the library
/// first (then of a binary, then the others).
fn pick_artifact(json_lines: &str, test: bool) -> Option<String> {
    let mut found: Vec<(u8, String)> = Vec::new();
    for line in json_lines.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v["reason"] != "compiler-artifact" {
            continue;
        }
        let Some(exe) = v["executable"].as_str() else { continue };
        if v["profile"]["test"].as_bool().unwrap_or(false) != test {
            continue;
        }
        let kinds: Vec<&str> = v["target"]["kind"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let rank = if kinds.contains(&"lib") { 0 } else if kinds.contains(&"bin") { 1 } else { 2 };
        found.push((rank, exe.to_string()));
    }
    found.sort_by_key(|(r, _)| *r);
    found.into_iter().next().map(|(_, e)| e)
}

// ------------------------------------------------------------------ Go

/// delve: in the PATH, else where `go install` puts it (see `lsp::in_path`).
fn find_dlv() -> Option<String> {
    in_path("dlv")
}

fn go(words: &[String], cwd: &Path) -> Result<Prepared, Failure> {
    let dlv = find_dlv().ok_or_else(|| Failure::Missing {
        tool: "delve".into(),
        why: crate::t!("c'est le débogueur de Go", "it is the debugger of Go").into(),
        install: ["go", "install", "github.com/go-delve/delve/cmd/dlv@latest"].map(String::from).to_vec(),
    })?;
    let args = go_launch(words, cwd)?;
    let port = free_port()?;
    let argv = vec![dlv, "dap".into(), "--listen".into(), format!("127.0.0.1:{port}")];
    Ok(Prepared { transport: Transport::Serve { argv, cwd: Some(cwd.to_path_buf()), port }, request: "launch", args, mounts: Vec::new() })
}

/// `go run <package> args`, `go test <package> -run X -v`, or a program built: the launch of delve.
fn go_launch(words: &[String], cwd: &Path) -> Result<Value, Failure> {
    let cwd_s = cwd.to_string_lossy();
    if stem(&words[0]) != "go" {
        return Ok(json!({ "mode": "exec", "program": absolute(&words[0], cwd), "args": &words[1..], "cwd": cwd_s }));
    }
    let sub = words.get(1).map(String::as_str).unwrap_or("");
    let rest = &words[2.min(words.len())..];
    // the package: the first word that is not an option (`.` when there is none)
    let pkg_at = rest.iter().position(|w| !w.starts_with('-'));
    let pkg = pkg_at.map_or(".".to_string(), |k| rest[k].clone());
    let others: Vec<String> = rest.iter().enumerate().filter(|(k, _)| Some(*k) != pkg_at).map(|(_, w)| w.clone()).collect();
    match sub {
        "run" => {
            // the options before the package are for the build, the words after it for the program
            let (flags, args) = match pkg_at {
                Some(k) => (rest[..k].to_vec(), rest[k + 1..].to_vec()),
                None => (rest.to_vec(), Vec::new()),
            };
            let mut v = json!({ "mode": "debug", "program": absolute(&pkg, cwd), "args": args, "cwd": cwd_s });
            if !flags.is_empty() {
                v["buildFlags"] = json!(flags.join(" "));
            }
            Ok(v)
        }
        "test" => {
            // the flags of `go test` given to the test binary: -run X → -test.run X
            let mut args = Vec::new();
            let mut k = 0;
            while k < others.len() {
                let w = others[k].trim_start_matches('-');
                let (name, value) = w.split_once('=').map_or((w, None), |(n, v)| (n, Some(v.to_string())));
                match name {
                    "run" | "count" | "timeout" | "bench" | "skip" => {
                        args.push(format!("-test.{name}"));
                        match value {
                            Some(v) => args.push(v),
                            None => {
                                if let Some(v) = others.get(k + 1) {
                                    args.push(v.clone());
                                }
                                k += 1;
                            }
                        }
                    }
                    "v" => args.push("-test.v".into()),
                    _ => {}
                }
                k += 1;
            }
            Ok(json!({ "mode": "test", "program": absolute(&pkg, cwd), "args": args, "cwd": cwd_s }))
        }
        _ => Err(Failure::Other(crate::tf!("« go {sub} » ne se débogue pas : go run ou go test", "“go {sub}” cannot be debugged: go run or go test"))),
    }
}

// ------------------------------------------------------------------ Node

/// The latest js-debug (the debugger of VS Code, as a server), downloaded once into the data
/// folder of Florm and checked against the SHA-256 GitHub gives.
fn js_debug() -> Result<PathBuf, Failure> {
    let dir = crate::config::data_dir().join("js-debug");
    let server = dir.join("js-debug").join("src").join("dapDebugServer.js");
    if server.is_file() {
        return Ok(server);
    }
    let err = |e: String| Failure::Other(crate::tf!("js-debug (le débogueur de Node) ne se télécharge pas : {e}", "js-debug (the debugger of Node) cannot be downloaded: {e}"));
    let api = curl(&["-sSfL", "https://api.github.com/repos/microsoft/vscode-js-debug/releases/latest"]).map_err(err)?;
    let rel: Value = serde_json::from_slice(&api).map_err(|e| err(e.to_string()))?;
    let asset = rel["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"].as_str().is_some_and(|n| n.starts_with("js-debug-dap-") && n.ends_with(".tar.gz")))).ok_or_else(|| err("asset ?".into()))?;
    let url = asset["browser_download_url"].as_str().ok_or_else(|| err("url ?".into()))?;
    let digest = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).ok_or_else(|| err("sha256 ?".into()))?.to_lowercase();
    let bytes = curl(&["-sSfL", url]).map_err(err)?;
    use sha2::{Digest, Sha256};
    let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if got != digest {
        return Err(err(crate::t!("le fichier reçu ne correspond pas à son empreinte", "the file received does not match its checksum").into()));
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| err(e.to_string()))?;
    let file = dir.join("js-debug.tar.gz");
    std::fs::write(&file, &bytes).map_err(|e| err(e.to_string()))?;
    let out = crate::os::command("tar").arg("-xzf").arg(&file).arg("-C").arg(&dir).stdin(Stdio::null()).output().map_err(|e| err(format!("tar : {e}")))?;
    let _ = std::fs::remove_file(&file);
    if !out.status.success() || !server.is_file() {
        return Err(err(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(server)
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = crate::os::command("curl").args(args).args(["-A", "florm"]).stdin(Stdio::null()).output().map_err(|e| format!("curl : {e}"))?;
    if out.status.success() { Ok(out.stdout) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

fn node(words: &[String], cwd: &Path) -> Result<Prepared, Failure> {
    let node = in_path("node").ok_or_else(|| Failure::Missing {
        tool: "Node.js".into(),
        why: crate::t!("il fait tourner le programme et son débogueur", "it runs the program and its debugger").into(),
        install: system_install(&[("pacman", "nodejs"), ("apt-get", "nodejs"), ("dnf", "nodejs"), ("zypper", "nodejs")]),
    })?;
    let args = node_launch(words, cwd);
    let server = js_debug()?;
    let port = free_port()?;
    let argv = vec![node, server.to_string_lossy().into_owned(), port.to_string(), "127.0.0.1".into()];
    Ok(Prepared { transport: Transport::Serve { argv, cwd: Some(cwd.to_path_buf()), port }, request: "launch", args, mounts: Vec::new() })
}

/// The launch of js-debug: `node [options] file args`, a runner (`npm start`, `npx jest …`), or a file.
fn node_launch(words: &[String], cwd: &Path) -> Value {
    let mut v = json!({ "type": "pwa-node", "name": "Florm", "request": "launch", "cwd": cwd.to_string_lossy(), "console": "internalConsole" });
    let first = stem(&words[0]);
    if first == "node" {
        let k = words[1..].iter().position(|w| !w.starts_with('-')).map(|k| k + 1);
        if let Some(k) = k {
            v["program"] = json!(absolute(&words[k], cwd));
            v["runtimeArgs"] = json!(&words[1..k]);
            v["args"] = json!(&words[k + 1..]);
            return v;
        }
    }
    if ["npm", "npx", "yarn", "pnpm", "bun", "tsx", "ts-node"].contains(&first.as_str()) {
        let mut rest = words[1..].to_vec();
        // the tests of jest in one process: the breakpoints are reached
        if rest.iter().any(|w| w == "jest") && !rest.iter().any(|w| w == "--runInBand" || w == "-i") {
            rest.push("--runInBand".into());
        }
        v["runtimeExecutable"] = json!(words[0]);
        v["runtimeArgs"] = json!(rest);
        return v;
    }
    v["program"] = json!(absolute(&words[0], cwd));
    v["args"] = json!(&words[1..]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        split(s)
    }

    #[test]
    fn languages_from_the_command_and_the_project() {
        let dir = std::env::temp_dir().join(format!("florm_debug_langs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(of_project(&dir), Lang::Python);
        assert_eq!(of_command(""), None);
        assert_eq!(of_command("npx jest a.test.ts"), Some(Lang::Node));
        assert_eq!(of_command("cargo test x"), Some(Lang::Native));
        assert_eq!(of_command("go test ./calc -run '^TestAdd$'"), Some(Lang::Go));
        assert_eq!(of_command("./odoo/20.0/odoo-bin --workers 0"), Some(Lang::Python));
        std::fs::write(dir.join("Cargo.toml"), "").unwrap();
        assert_eq!(of_project(&dir), Lang::Native);
        assert_eq!(default_command(Lang::Native, &dir), "cargo run");
        std::fs::remove_file(dir.join("Cargo.toml")).unwrap();
        std::fs::write(dir.join("package.json"), r#"{"scripts": {"start": "node s.js"}}"#).unwrap();
        assert_eq!(of_project(&dir), Lang::Node);
        assert_eq!(default_command(Lang::Node, &dir), "npm start");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gdb_versions() {
        assert!(gdb_has_dap("GNU gdb (GDB) 18.1\nCopyright"));
        assert!(gdb_has_dap("GNU gdb (Ubuntu 15.0.50.20240403-0ubuntu1) 15.0.50.20240403-git"));
        assert!(!gdb_has_dap("GNU gdb (GDB) Red Hat Enterprise Linux 12.1-2.el9"));
    }

    #[test]
    fn the_artifact_of_cargo() {
        let lines = [
            r#"{"reason":"compiler-artifact","target":{"kind":["lib"]},"profile":{"test":false},"executable":null}"#,
            r#"{"reason":"compiler-artifact","target":{"kind":["bin"]},"profile":{"test":false},"executable":"/t/debug/app"}"#,
            r#"{"reason":"compiler-artifact","target":{"kind":["bin"]},"profile":{"test":true},"executable":"/t/debug/deps/app-1"}"#,
            r#"{"reason":"compiler-artifact","target":{"kind":["lib"]},"profile":{"test":true},"executable":"/t/debug/deps/lib-2"}"#,
            r#"{"reason":"build-finished","success":true}"#,
        ]
        .join("\n");
        assert_eq!(pick_artifact(&lines, false).as_deref(), Some("/t/debug/app"));
        assert_eq!(pick_artifact(&lines, true).as_deref(), Some("/t/debug/deps/lib-2"));
    }

    #[test]
    fn launches_of_go_and_node() {
        let cwd = Path::new("/p");
        let v = go_launch(&w("go test ./calc -run '^TestAdd$' -v"), cwd).unwrap();
        assert_eq!((v["mode"].as_str(), v["program"].as_str()), (Some("test"), Some("/p/./calc")));
        assert_eq!(v["args"], json!(["-test.run", "^TestAdd$", "-test.v"]));
        let v = go_launch(&w("go run . --port 8080"), cwd).unwrap();
        assert_eq!((v["mode"].as_str(), v["args"].clone()), (Some("debug"), json!(["--port", "8080"])));
        let v = node_launch(&w("node --inspect-brk app.js a b"), cwd);
        assert_eq!((v["program"].as_str(), v["args"].clone(), v["runtimeArgs"].clone()), (Some("/p/app.js"), json!(["a", "b"]), json!(["--inspect-brk"])));
        let v = node_launch(&w("npx jest src/a.test.ts -t adds"), cwd);
        assert_eq!(v["runtimeExecutable"], "npx");
        assert_eq!(v["runtimeArgs"], json!(["jest", "src/a.test.ts", "-t", "adds", "--runInBand"]));
    }
}
