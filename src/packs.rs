//! Language packs: per language, its tools (language server, linter, formatter, debugger) and how
//! to install them in Florm's own tools folder (pip in a virtual environment, npm, go install,
//! rustup), or the command to copy when only the system can (gdb, marksman…).
//! The packs Florm ships are in `assets/packs.toml`; `~/.config/florm/packs/*.toml` adds some or
//! replaces one (same name). A pack may also declare a language server or a linter Florm does
//! not know (`cmd`), used for its files.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{OnceLock, RwLock};

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Tool {
    /// lsp, lint, format, debug.
    pub kind: String,
    /// The program looked for.
    pub bin: String,
    #[serde(default)]
    pub pip: String,
    #[serde(default)]
    pub npm: String,
    #[serde(default)]
    pub go: String,
    #[serde(default)]
    pub rustup: String,
    /// A GitHub repository whose latest release has the program ("rust-lang/rust-analyzer")…
    #[serde(default)]
    pub github: String,
    /// …in the file named for each platform (linux-x64, linux-arm64, macos-x64, macos-arm64,
    /// windows-x64, windows-arm64; `{version}`: the tag without « v »).
    #[serde(default)]
    pub asset: std::collections::BTreeMap<String, String>,
    /// The command to copy when Florm cannot install it (hint_linux, hint_macos, hint_windows:
    /// for one system).
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub hint_linux: String,
    #[serde(default)]
    pub hint_macos: String,
    #[serde(default)]
    pub hint_windows: String,
    /// A Python module whose presence is enough (debugpy).
    #[serde(default)]
    pub module: String,
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub optional: bool,
    /// lsp: the command of a server Florm does not know (`nimlangserver --stdio`); lint: the
    /// command of a linter (`mylint {file}`, its output `file:line:col: message`).
    #[serde(default)]
    pub cmd: String,
    /// lint: a regex with (?P<line>…) (?P<col>…) (?P<msg>…) when the output is another format.
    #[serde(default)]
    pub pattern: String,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Pack {
    pub name: String,
    /// Extensions ("py"), or whole file names ("Dockerfile").
    #[serde(default)]
    pub ext: Vec<String>,
    #[serde(default, rename = "tool")]
    pub tools: Vec<Tool>,
    /// The tree-sitter grammars compiled into Florm for this language (`syntax_tree::names`):
    /// precise highlighting and selection by syntax node, turned off together in the pack.
    #[serde(default)]
    pub tree_sitter: Vec<String>,
    /// From ~/.config/florm/packs (not shipped with Florm).
    #[serde(skip)]
    pub own: bool,
}

#[derive(Deserialize)]
struct Shipped {
    pack: Vec<Pack>,
}

impl Tool {
    /// How Florm installs it: ("pip", "ruff"); None: by hand (hint) or nothing to do.
    pub fn method(&self) -> Option<(&'static str, &str)> {
        let github = !self.github.trim().is_empty() && self.asset.contains_key(&crate::runtimes::platform());
        for (m, v, ok) in [("pip", &self.pip, true), ("npm", &self.npm, true), ("go", &self.go, true), ("github", &self.github, github), ("rustup", &self.rustup, true)] {
            if ok && !v.trim().is_empty() {
                return Some((m, v.trim()));
            }
        }
        None
    }

    /// The command to copy for this system.
    pub fn hint(&self) -> &str {
        let own = if cfg!(windows) { &self.hint_windows } else if cfg!(target_os = "macos") { &self.hint_macos } else { &self.hint_linux };
        if own.trim().is_empty() { &self.hint } else { own }
    }

    /// The kind as shown: LSP, Lint, Format, Debug.
    pub fn kind_label(&self) -> &'static str {
        match self.kind.as_str() {
            "lsp" => "LSP",
            "lint" => "Lint",
            "format" => crate::t!("Format", "Format"),
            "debug" => crate::t!("Débogueur", "Debugger"),
            _ => "?",
        }
    }
}

impl Pack {
    pub fn matches(&self, path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        self.ext.iter().map(|e| e.trim().trim_start_matches('.')).any(|e| e == name || (!ext.is_empty() && e.eq_ignore_ascii_case(ext)))
    }
}

/// The packs Florm ships.
pub fn shipped() -> Vec<Pack> {
    toml::from_str::<Shipped>(include_str!("../assets/packs.toml")).map(|s| s.pack).unwrap_or_default()
}

/// The folder of the user's packs.
pub fn user_dir() -> PathBuf {
    crate::config::config_dir().join("packs")
}

/// The user's packs, and the files that cannot be read (with why).
pub fn user_packs(dir: &Path) -> (Vec<Pack>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return (out, errors) };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect();
    files.sort();
    for f in files {
        match std::fs::read_to_string(&f).map_err(|e| e.to_string()).and_then(|t| toml::from_str::<Pack>(&t).map_err(|e| e.to_string())) {
            Ok(mut p) => {
                if p.name.trim().is_empty() {
                    p.name = f.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
                }
                p.own = true;
                out.push(p);
            }
            Err(e) => errors.push(format!("{} : {}", f.display(), e.lines().next().unwrap_or(""))),
        }
    }
    (out, errors)
}

/// The shipped packs, the user's replacing those of the same name and added after.
pub fn merge(shipped: Vec<Pack>, user: Vec<Pack>) -> Vec<Pack> {
    let mut out: Vec<Pack> = shipped.into_iter().filter(|p| !user.iter().any(|u| u.name.eq_ignore_ascii_case(&p.name))).collect();
    // in the order of the shipped ones where they replace one
    for u in user {
        out.push(u);
    }
    out
}

struct Loaded {
    packs: Vec<Pack>,
    errors: Vec<String>,
    /// The language servers they declare (`cmd`): (family, command, extensions).
    servers: Vec<(String, String, Vec<String>)>,
}

fn load() -> Loaded {
    let (user, errors) = user_packs(&user_dir());
    let packs = merge(shipped(), user);
    let servers = packs
        .iter()
        .flat_map(|p| {
            let family = p.name.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-");
            p.tools.iter().filter(|t| t.kind == "lsp" && !t.cmd.trim().is_empty()).map(move |t| (family.clone(), t.cmd.clone(), p.ext.clone()))
        })
        .collect();
    Loaded { packs, errors, servers }
}

fn cell() -> &'static RwLock<Loaded> {
    static C: OnceLock<RwLock<Loaded>> = OnceLock::new();
    C.get_or_init(|| RwLock::new(load()))
}

/// All the packs (read once; `reload` after a change of the folder).
pub fn all() -> Vec<Pack> {
    cell().read().map(|c| c.packs.clone()).unwrap_or_default()
}

/// The name of the pack of file `path` (its language), without copying the packs.
pub fn name_for(path: &Path) -> Option<String> {
    cell().read().ok()?.packs.iter().find(|p| p.matches(path)).map(|p| p.name.clone())
}

/// The files of the user's packs that could not be read.
pub fn errors() -> Vec<String> {
    cell().read().map(|c| c.errors.clone()).unwrap_or_default()
}

pub fn reload() {
    let l = load();
    if let Ok(mut c) = cell().write() {
        *c = l;
    }
}

/// The language servers the packs declare (`cmd`): (family, command, extensions). Often empty:
/// read at each frame (the language of the current file).
pub fn servers() -> Vec<(String, String, Vec<String>)> {
    cell().read().map(|c| c.servers.clone()).unwrap_or_default()
}

/// The linters the packs declare (`cmd`): (name, command, pattern) for `path`.
pub fn linters_for(path: &Path) -> Vec<(String, String, String)> {
    all()
        .into_iter()
        .filter(|p| p.matches(path))
        .flat_map(|p| p.tools.into_iter().filter(|t| t.kind == "lint" && !t.cmd.trim().is_empty()).map(|t| (t.bin, t.cmd, t.pattern)))
        .collect()
}

// ------------------------------------------------------------------ where the tools go

/// Florm's tools folder (`~/.local/share/florm/tools`).
pub fn tools_root() -> PathBuf {
    crate::config::data_dir().join("tools")
}

/// The virtual environment of the Python tools.
pub fn py_venv() -> PathBuf {
    tools_root().join("python")
}

pub fn py_venv_bin() -> PathBuf {
    py_venv().join(if cfg!(windows) { "Scripts" } else { "bin" })
}

/// Its Python, when it exists.
pub fn py_venv_python() -> Option<PathBuf> {
    let p = py_venv_bin().join(if cfg!(windows) { "python.exe" } else { "python" });
    p.is_file().then_some(p)
}

fn node_dir() -> PathBuf {
    tools_root().join("node")
}

/// The folders where Florm's installs put programs (looked into after the PATH).
pub fn bin_dirs() -> Vec<PathBuf> {
    vec![py_venv_bin(), node_dir().join("node_modules").join(".bin"), tools_root().join("bin")]
}

/// The Python module `m` is installed for Florm: in its virtual environment or its old tools
/// folder (`pip --target`).
fn module_installed(m: &str) -> bool {
    if crate::lint::tools_dir().join(m).is_dir() {
        return true;
    }
    let lib = if cfg!(windows) { py_venv().join("Lib").join("site-packages").join(m).is_dir() } else { false };
    lib || std::fs::read_dir(py_venv().join("lib")).into_iter().flatten().flatten().any(|e| e.path().join("site-packages").join(m).is_dir())
}

/// Is the tool there? (looks on the disk, and asks the system's Python for a module: on a thread)
pub fn installed(t: &Tool) -> bool {
    if t.builtin {
        return true;
    }
    if !t.module.is_empty() {
        return module_installed(&t.module)
            || crate::os::command(crate::lint::python()).args(["-c", &format!("import {}", t.module)]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    }
    crate::lsp::in_path(&t.bin).is_some()
}

/// For each pack, whether each of its tools is there (on a thread).
pub fn status(packs: &[Pack]) -> Vec<Vec<bool>> {
    packs.iter().map(|p| p.tools.iter().map(installed).collect()).collect()
}

/// The tools of `pack` an « Installer » installs: missing, not optional, that Florm can install.
#[cfg(test)]
pub fn to_install(pack: &Pack, status: &[bool]) -> Vec<Tool> {
    to_install_kinds(pack, status, &["lsp", "lint", "format", "debug"].map(String::from))
}

/// `to_install` for the kinds of tools wanted (lsp, lint, format, debug).
pub fn to_install_kinds(pack: &Pack, status: &[bool], kinds: &[String]) -> Vec<Tool> {
    let mut out: Vec<Tool> = Vec::new();
    for (t, ok) in pack.tools.iter().zip(status) {
        if !ok && !t.optional && kinds.contains(&t.kind) && t.method().is_some() && !out.iter().any(|o| o.method() == t.method()) {
            out.push(t.clone());
        }
    }
    out
}

/// Runs a command, its output kept for the log. Err: the output when it fails.
fn run(mut c: std::process::Command, what: &str, log: &mut String) -> Result<(), String> {
    log.push_str(&format!("$ {what}\n"));
    let out = c.stdin(Stdio::null()).output().map_err(|e| format!("{what} : {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    log.push_str(&text);
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{what}\n{}", text.trim()))
    }
}

/// The virtual environment of the Python tools, made when missing (or broken: the Python it was
/// made with was updated or removed). Python is downloaded if the system has none. Err: why.
fn ensure_venv(log: &mut String) -> Result<PathBuf, String> {
    if let Some(p) = py_venv_python() {
        if crate::os::command(&p).arg("--version").stdin(Stdio::null()).output().is_ok_and(|o| o.status.success()) {
            return Ok(p);
        }
        let _ = std::fs::remove_dir_all(py_venv());
    }
    let _ = std::fs::create_dir_all(tools_root());
    let py = crate::runtimes::python()?;
    let mut c = crate::os::command(&py);
    c.args(["-m", "venv"]).arg(py_venv());
    run(c, &format!("{py} -m venv {}", py_venv().display()), log)?;
    py_venv_python().ok_or_else(|| crate::t!("l'environnement virtuel n'a pas de Python", "the virtual environment has no Python").to_string())
}

/// Installs `tools` (on a thread): a message for the status bar, or the log of what failed.
pub fn install(tools: &[Tool]) -> Result<String, String> {
    let mut log = String::new();
    let mut done: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let by = |method: &str| -> Vec<String> {
        let mut v: Vec<String> = tools.iter().filter_map(|t| t.method().filter(|(m, _)| *m == method).map(|(_, p)| p.to_string())).flat_map(|p| p.split_whitespace().map(String::from).collect::<Vec<_>>()).collect();
        v.dedup();
        v
    };
    let pip = by("pip");
    let npm = by("npm");
    let go = by("go");
    let rustup = by("rustup");
    let github: Vec<&Tool> = tools.iter().filter(|t| t.method().is_some_and(|(m, _)| m == "github")).collect();
    if !pip.is_empty() {
        let r = match ensure_venv(&mut log) {
            Ok(py) => {
                let mut c = crate::os::command(py.to_str().unwrap_or("python"));
                c.args(["-m", "pip", "install", "--upgrade", "--quiet", "--disable-pip-version-check"]).args(&pip);
                run(c, &format!("pip install {}", pip.join(" ")), &mut log)
            }
            // no venv module (Debian without python3-venv): in the old tools folder
            Err(_) => pip.iter().try_for_each(|p| crate::lint::install_python_tool(p).map(|m| log.push_str(&format!("{m}\n")))),
        };
        match r {
            Ok(()) => done.extend(pip.iter().cloned()),
            Err(e) => failed.push(e),
        }
    }
    if !npm.is_empty() {
        // the system's npm, else Florm's Node.js (downloaded once)
        match crate::runtimes::npm() {
            Err(e) => failed.push(crate::tf!("Node.js pas disponible ({e}) pour : {}", "Node.js not available ({e}) for: {}", npm.join(", "))),
            Ok(bin) => {
                let _ = std::fs::create_dir_all(node_dir());
                let mut c = crate::os::command(bin.to_str().unwrap_or("npm"));
                c.args(["install", "--no-fund", "--no-audit", "--prefix"]).arg(node_dir()).args(&npm);
                match run(c, &format!("npm install {}", npm.join(" ")), &mut log) {
                    Ok(()) => done.extend(npm.iter().cloned()),
                    Err(e) => failed.push(e),
                }
            }
        }
    }
    for g in go {
        match crate::runtimes::go() {
            Err(e) => failed.push(crate::tf!("Go pas disponible ({e}) pour : {g}", "Go not available ({e}) for: {g}")),
            Ok(bin) => {
                let _ = std::fs::create_dir_all(tools_root().join("bin"));
                let mut c = crate::os::command(bin.to_str().unwrap_or("go"));
                c.args(["install", &g]).env("GOBIN", tools_root().join("bin"));
                match run(c, &format!("go install {g}"), &mut log) {
                    Ok(()) => done.push(g.rsplit('/').next().unwrap_or(&g).split('@').next().unwrap_or("").to_string()),
                    Err(e) => failed.push(e),
                }
            }
        }
    }
    for r in rustup {
        match crate::lsp::in_path("rustup") {
            None => failed.push(crate::tf!("rustup introuvable pour : {r}", "rustup not found for: {r}")),
            Some(bin) => {
                let mut c = crate::os::command(bin.to_str().unwrap_or("rustup"));
                c.args(["component", "add", &r]);
                match run(c, &format!("rustup component add {r}"), &mut log) {
                    Ok(()) => done.push(r.clone()),
                    Err(e) => failed.push(e),
                }
            }
        }
    }
    for t in github {
        let asset = t.asset.get(&crate::runtimes::platform()).cloned().unwrap_or_default();
        log.push_str(&format!("$ github {} {asset}\n", t.github));
        match crate::runtimes::install_github(&t.github, &asset, &t.bin) {
            Ok(()) => done.push(t.bin.clone()),
            Err(e) => failed.push(e),
        }
    }
    crate::runtimes::refresh();
    crate::lint::reset_linter_cache();
    if failed.is_empty() {
        Ok(crate::tf!("Installé : {}", "Installed: {}", done.join(", ")))
    } else {
        Err(format!("{log}\n{}", failed.join("\n\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packs Florm ships read, each tool with a kind, a program and a way to get it.
    #[test]
    fn shipped_packs_read() {
        let packs = shipped();
        assert!(packs.len() >= 15, "{}", packs.len());
        for p in &packs {
            assert!(!p.ext.is_empty(), "{}", p.name);
            for t in &p.tools {
                assert!(["lsp", "lint", "format", "debug"].contains(&t.kind.as_str()), "{} {}", p.name, t.kind);
                assert!(t.builtin || t.method().is_some() || !t.hint().is_empty(), "{} {}", p.name, t.bin);
                // a GitHub program has its file for each platform
                if !t.github.is_empty() {
                    for plat in ["linux-x64", "linux-arm64", "macos-x64", "macos-arm64", "windows-x64"] {
                        assert!(t.asset.contains_key(plat) || !t.hint().is_empty(), "{} {plat}", t.bin);
                    }
                }
            }
        }
        let py = packs.iter().find(|p| p.name == "Python").unwrap();
        assert!(py.matches(Path::new("/x/a.py")) && !py.matches(Path::new("/x/a.pyc")));
        let docker = packs.iter().find(|p| p.name == "Dockerfile").unwrap();
        assert!(docker.matches(Path::new("/x/Dockerfile")));
    }

    /// One install per way (ruff is the linter and the formatter: once); the optional left out.
    #[test]
    fn what_an_install_installs() {
        let py = shipped().into_iter().find(|p| p.name == "Python").unwrap();
        let none = vec![false; py.tools.len()];
        let t = to_install(&py, &none);
        let bins: Vec<&str> = t.iter().map(|t| t.bin.as_str()).collect();
        assert_eq!(bins, ["pylsp", "ruff", "debugpy"]);
        let all = vec![true; py.tools.len()];
        assert!(to_install(&py, &all).is_empty());
        // only the kinds wanted: the linter without the language server; ruff once (lint, format)
        let kinds = |k: &[&str]| k.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let lint: Vec<String> = to_install_kinds(&py, &none, &kinds(&["lint", "format"])).into_iter().map(|t| t.bin).collect();
        assert_eq!(lint, ["ruff"]);
        assert!(to_install_kinds(&py, &none, &kinds(&[])).is_empty());
    }

    /// A pack of the user replaces the one of the same name, adds a server and a linter.
    #[test]
    fn user_packs_replace_and_add() {
        let dir = std::env::temp_dir().join(format!("florm_packs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("nim.toml"), "name = \"Nim\"\next = [\"nim\"]\n[[tool]]\nkind = \"lsp\"\nbin = \"nimlangserver\"\ncmd = \"nimlangserver --stdio\"\n[[tool]]\nkind = \"lint\"\nbin = \"nimcheck\"\ncmd = \"nim check {file}\"\n").unwrap();
        std::fs::write(dir.join("python.toml"), "name = \"python\"\next = [\"py\"]\n").unwrap();
        std::fs::write(dir.join("broken.toml"), "name = [").unwrap();
        let (user, errors) = user_packs(&dir);
        assert_eq!((user.len(), errors.len()), (2, 1));
        let packs = merge(shipped(), user);
        let py: Vec<&Pack> = packs.iter().filter(|p| p.name.eq_ignore_ascii_case("python")).collect();
        assert!(py.len() == 1 && py[0].own && py[0].tools.is_empty());
        let nim = packs.iter().find(|p| p.name == "Nim").unwrap();
        assert_eq!(nim.tools[0].cmd, "nimlangserver --stdio");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `cargo test -- --ignored`: real installs in Florm's tools folder (a test one): yamllint
    /// with pip in the virtual environment, bash-language-server with npm; then found.
    #[test]
    #[ignore]
    fn installs_for_real() {
        let dir = std::env::temp_dir().join(format!("florm_packs_install_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _lock = crate::config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let yaml = shipped().into_iter().find(|p| p.name == "YAML").unwrap();
        let shell = shipped().into_iter().find(|p| p.name == "Shell").unwrap();
        let tools: Vec<Tool> = vec![yaml.tools[1].clone(), shell.tools[0].clone()];
        assert!(!tools.iter().any(installed));
        let r = install(&tools);
        assert!(r.is_ok(), "{r:?}");
        for t in &tools {
            let p = crate::lsp::in_path(&t.bin).unwrap();
            assert!(p.starts_with(&dir), "{} {}", t.bin, p.display());
        }
        let out = crate::os::command(crate::lsp::in_path("yamllint").unwrap().to_str().unwrap()).arg("--version").output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
