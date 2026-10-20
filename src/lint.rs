//! Linters: XML/JSON in-process, Python and JavaScript through external tools
//! (ruff → flake8 → pyflakes → ast, eslint → node --check).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub line: usize,
    pub col: usize,
    pub severity: Severity,
    pub message: String,
    pub source: String,
    /// Linter rule code (E501, F401, W0611…) when known.
    pub code: Option<String>,
}

pub fn lang_of(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    Some(match ext.as_str() {
        "py" | "pyw" => "python",
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => "javascript",
        "xml" | "svg" | "xsd" | "xsl" | "xslt" | "csproj" | "plist" | "xaml" | "rng" | "kml" => "xml",
        "json" => "json",
        _ => return None,
    })
}

pub fn is_in_process(lang: &str) -> bool {
    matches!(lang, "xml" | "json")
}

pub fn lint_in_process(lang: &str, text: &str) -> Vec<Diagnostic> {
    match lang {
        "xml" => lint_xml(text),
        "json" => lint_json(text),
        _ => Vec::new(),
    }
}

/// Which Python linter to use and its extra arguments (from the settings).
#[derive(Clone, Debug, Default)]
pub struct LintConfig {
    /// "auto" (ruff → flake8 → pyflakes), or "flake8", "ruff", "pyflakes", "pylint".
    pub python_linter: String,
    pub python_args: Vec<String>,
}

/// Runs an external linter. Python gets the buffer on stdin (unsaved changes are checked);
/// JavaScript runs on the saved file. `None` when no linter applies.
pub fn lint_external(path: &Path, text: &str, root: Option<&Path>, cfg: &LintConfig) -> Option<(Vec<Diagnostic>, String)> {
    match lang_of(path)? {
        "python" => Some(lint_python(path, text, root, cfg)),
        "javascript" => Some((lint_js(path, root), "eslint".into())),
        _ => None,
    }
}

fn diag_at(text: &str, byte: usize, severity: Severity, message: String, source: &str) -> Diagnostic {
    let mut b = byte.min(text.len());
    while !text.is_char_boundary(b) {
        b -= 1;
    }
    let before = &text[..b];
    let line = before.matches('\n').count();
    let col = before.rsplit('\n').next().unwrap_or("").chars().count();
    Diagnostic { line, col, severity, message, source: source.into() , code: None}
}

fn lint_xml(text: &str) -> Vec<Diagnostic> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    let mut r = Reader::from_str(text);
    let mut stack: Vec<(String, usize)> = Vec::new();
    loop {
        let before = r.buffer_position() as usize;
        match r.read_event() {
            Ok(Event::Start(e)) => stack.push((String::from_utf8_lossy(e.name().as_ref()).into_owned(), before)),
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                let p = r.buffer_position() as usize;
                return vec![diag_at(text, p, Severity::Error, format!("XML : {e}"), "xml")];
            }
        }
    }
    match stack.last() {
        Some((name, p)) => vec![diag_at(text, *p, Severity::Error, format!("Balise <{name}> non fermée"), "xml")],
        None => Vec::new(),
    }
}

fn lint_json(text: &str) -> Vec<Diagnostic> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(_) => Vec::new(),
        Err(e) => vec![Diagnostic {
            line: e.line().saturating_sub(1),
            col: e.column().saturating_sub(1),
            severity: Severity::Error,
            message: format!("JSON : {e}"),
            source: "json".into(),
            code: None,
        }],
    }
}

/// Runs a tool; `None` if it could not be started (not installed).
fn run_tool(prog: &str, args: &[&str], dir: Option<&Path>) -> Option<String> {
    let mut c = crate::os::command(prog);
    c.args(args).stdin(Stdio::null());
    if let Some(d) = dir {
        c.current_dir(d);
    }
    let o = c.output().ok()?;
    Some(format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

/// Rule code at the start of a message: "E501 line too long" -> "E501".
fn code_of(msg: &str) -> Option<String> {
    let first = msg.split_whitespace().next()?.trim_end_matches(':');
    let letters = first.chars().take_while(|c| c.is_ascii_uppercase()).count();
    let digits = first.chars().skip(letters).take_while(|c| c.is_ascii_digit()).count();
    (letters >= 1 && letters <= 4 && digits >= 2 && letters + digits == first.len()).then(|| first.to_string())
}

fn classify(msg: &str, code: Option<&str>) -> Severity {
    let m = msg.trim_start();
    if m.contains("SyntaxError") || m.contains("invalid-syntax") || m.to_lowercase().starts_with("error") {
        return Severity::Error;
    }
    match code {
        // pyflakes / pycodestyle: syntax errors, undefined names, invalid statements
        Some(c) if c.starts_with("E9") || c.starts_with("F82") || c.starts_with("F63") || c.starts_with("F7") => Severity::Error,
        // pylint: E (error) and F (fatal) use 4 digits
        Some(c) if c.len() == 5 && (c.starts_with('E') || c.starts_with('F')) => Severity::Error,
        _ => Severity::Warning,
    }
}

/// Parses `path:line:col: message` lines (ruff, flake8, pyflakes, pylint template).
fn parse_colon_format(out: &str, source: &str) -> Vec<Diagnostic> {
    let re = regex::Regex::new(r"^(.*?):(\d+):(\d+):?\s*(.*)$").unwrap();
    out.lines()
        .filter_map(|l| {
            let c = re.captures(l)?;
            let line: usize = c[2].parse().ok()?;
            let col: usize = c[3].parse().ok()?;
            let message = c[4].trim().to_string();
            let code = code_of(&message);
            Some(Diagnostic {
                line: line.saturating_sub(1),
                col: col.saturating_sub(1),
                severity: classify(&message, code.as_deref()),
                message,
                source: source.into(),
                code,
            })
        })
        .collect()
}

/// Syntax check on stdin when no linter is installed.
const PY_SYNTAX: &str = r#"
import ast, sys
src = sys.stdin.read()
try:
    ast.parse(src, sys.argv[1])
except SyntaxError as e:
    print(f"{sys.argv[1]}:{e.lineno or 1}:{e.offset or 1}: E999 SyntaxError: {e.msg}")
"#;

/// Private folder where Florm installs Python tools (`pip install --target`), so that
/// nothing is installed in the system Python.
pub fn tools_dir() -> PathBuf {
    crate::config::data_dir().join("pytools")
}

pub fn python() -> &'static str {
    static PY: OnceLock<&'static str> = OnceLock::new();
    PY.get_or_init(|| {
        if crate::os::command("python3").arg("--version").output().is_ok() {
            "python3"
        } else {
            "python"
        }
    })
}

#[derive(Clone)]
enum Runner {
    /// Executable found in PATH or in the tools folder.
    Bin(PathBuf),
    /// `python -m <module>` with the tools folder on PYTHONPATH.
    Module(&'static str),
}

fn runners() -> &'static Mutex<HashMap<String, Option<Runner>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Runner>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Forget which linters exist (after installing one).
pub fn reset_linter_cache() {
    runners().lock().unwrap().clear();
}

fn find_runner(name: &str) -> Option<Runner> {
    if let Some(r) = runners().lock().unwrap().get(name) {
        return r.clone();
    }
    let module: Option<&'static str> = match name {
        "flake8" => Some("flake8"),
        "pyflakes" => Some("pyflakes"),
        "pylint" => Some("pylint"),
        _ => None,
    };
    let in_path = crate::os::command(name).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok();
    let tool_bin = tools_dir().join("bin").join(name);
    let r = if in_path {
        Some(Runner::Bin(PathBuf::from(name)))
    } else if tool_bin.exists() && name == "ruff" {
        Some(Runner::Bin(tool_bin))
    } else {
        module.filter(|m| tools_dir().join(m).is_dir()).map(Runner::Module)
    };
    runners().lock().unwrap().insert(name.to_string(), r.clone());
    r
}

/// Whether a Python linter ("flake8", "ruff", "pyflakes", "pylint") can run.
pub fn is_installed(name: &str) -> bool {
    find_runner(name).is_some()
}

fn run_with_stdin(runner: &Runner, args: &[String], input: &str, cwd: &Path) -> Option<String> {
    let mut c = match runner {
        Runner::Bin(p) => crate::os::command(p),
        Runner::Module(m) => {
            let mut c = crate::os::command(python());
            c.arg("-m").arg(m).env("PYTHONPATH", tools_dir());
            c
        }
    };
    c.args(args).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let o = child.wait_with_output().ok()?;
    Some(format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

/// Lints Python source `text` (the buffer) as if it were `path`. Runs in the project /
/// repository root so the project configuration (setup.cfg, .flake8, tox.ini,
/// pyproject.toml) applies. Returns the diagnostics and the linter used.
pub fn lint_python(path: &Path, text: &str, root: Option<&Path>, cfg: &LintConfig) -> (Vec<Diagnostic>, String) {
    let cwd = root.or(path.parent()).unwrap_or(Path::new("."));
    let display = path.strip_prefix(cwd).unwrap_or(path).to_string_lossy().into_owned();
    let wanted: Vec<&str> = match cfg.python_linter.as_str() {
        "" | "auto" => vec!["ruff", "flake8", "pyflakes"],
        other => vec![other],
    };
    let extra = cfg.python_args.iter().cloned();
    for name in wanted {
        let Some(runner) = find_runner(name) else { continue };
        let args: Vec<String> = match name {
            "ruff" => ["check", "--output-format=concise", "--no-cache", "--quiet", "--stdin-filename", &display]
                .iter()
                .map(|s| s.to_string())
                .chain(extra.clone())
                .chain(["-".to_string()])
                .collect(),
            "flake8" => ["--stdin-display-name".to_string(), display.clone()].into_iter().chain(extra.clone()).chain(["-".to_string()]).collect(),
            "pyflakes" => vec!["-".to_string()],
            "pylint" => [
                "--from-stdin".to_string(),
                "--score=n".to_string(),
                "--msg-template={path}:{line}:{column}: {msg_id} {msg} ({symbol})".to_string(),
            ]
            .into_iter()
            .chain(extra.clone())
            .chain([display.clone()])
            .collect(),
            _ => continue,
        };
        if let Some(out) = run_with_stdin(&runner, &args, text, cwd) {
            let mut diags = parse_colon_format(&out, name);
            if name == "pylint" {
                for d in &mut diags {
                    d.col += 1; // pylint columns are 0-based
                }
            }
            return (diags, name.to_string());
        }
    }
    let args = vec!["-c".to_string(), PY_SYNTAX.to_string(), display];
    let out = run_with_stdin(&Runner::Bin(PathBuf::from(python())), &args, text, cwd).unwrap_or_default();
    (parse_colon_format(&out, "python"), "syntaxe".into())
}

/// Installs a linter into the private tools folder (`pip install --target`).
pub fn install_python_tool(name: &str) -> Result<String, String> {
    let dir = tools_dir();
    let _ = std::fs::create_dir_all(&dir);
    let out = crate::os::command(python())
        .args(["-m", "pip", "install", "--upgrade", "--quiet", "--target"])
        .arg(&dir)
        .arg(name)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("pip introuvable : {e}"))?;
    reset_linter_cache();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(format!("{name} installé dans {}", dir.display()))
    } else {
        Err(text)
    }
}

fn lint_js(path: &Path, root: Option<&Path>) -> Vec<Diagnostic> {
    let p = path.to_string_lossy();
    let dir = root.or(path.parent());
    let mut candidates = Vec::new();
    if let Some(r) = root {
        let local = r.join("node_modules").join(".bin").join("eslint");
        if local.exists() {
            candidates.push(local.to_string_lossy().into_owned());
        }
    }
    candidates.push("eslint".into());
    for eslint in candidates {
        let Some(out) = run_tool(&eslint, &["--format", "json", &p], dir) else { continue };
        if let Some(d) = parse_eslint_json(&out) {
            return d;
        }
        // eslint present but not configured: fall back to a syntax check.
        break;
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if matches!(ext, "js" | "mjs" | "cjs") {
        if let Some(out) = run_tool("node", &["--check", &p], dir) {
            return parse_node_check(&out);
        }
    }
    Vec::new()
}

fn parse_eslint_json(out: &str) -> Option<Vec<Diagnostic>> {
    let start = out.find('[')?;
    let v: serde_json::Value = serde_json::from_str(&out[start..]).ok()?;
    let mut diags = Vec::new();
    for file in v.as_array()? {
        for m in file["messages"].as_array().into_iter().flatten() {
            let rule = m["ruleId"].as_str().unwrap_or("");
            diags.push(Diagnostic {
                line: (m["line"].as_u64().unwrap_or(1) as usize).saturating_sub(1),
                col: (m["column"].as_u64().unwrap_or(1) as usize).saturating_sub(1),
                severity: if m["severity"].as_u64() == Some(2) { Severity::Error } else { Severity::Warning },
                message: if rule.is_empty() {
                    m["message"].as_str().unwrap_or("").to_string()
                } else {
                    format!("{} ({rule})", m["message"].as_str().unwrap_or(""))
                },
                source: "eslint".into(),
                code: (!rule.is_empty()).then(|| rule.to_string()),
            });
        }
    }
    Some(diags)
}

/// node --check prints `file:LINE`, the source line, a caret line, then the error.
fn parse_node_check(out: &str) -> Vec<Diagnostic> {
    let lines: Vec<&str> = out.lines().collect();
    let Some(i) = lines.iter().position(|l| l.rsplit_once(':').is_some_and(|(_, n)| n.parse::<usize>().is_ok()))
    else {
        return Vec::new();
    };
    let line: usize = lines[i].rsplit_once(':').unwrap().1.parse().unwrap_or(1);
    let col = lines.get(i + 2).and_then(|l| l.find('^')).unwrap_or(0);
    let message = lines.iter().find(|l| l.contains("Error:")).map(|l| l.trim().to_string()).unwrap_or_default();
    vec![Diagnostic { line: line.saturating_sub(1), col, severity: Severity::Error, message, source: "node".into() , code: None}]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_errors() {
        assert!(lint_in_process("xml", "<a><b/></a>").is_empty());
        let d = lint_in_process("xml", "<a>\n  <b></c>\n</a>");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 1);
        let d = lint_in_process("xml", "<odoo>\n<record>");
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn json_errors() {
        assert!(lint_in_process("json", "{\"a\": 1}").is_empty());
        let d = lint_in_process("json", "{\n\"a\": ,}");
        assert_eq!(d[0].line, 1);
    }

    #[test]
    fn colon_format() {
        let d = parse_colon_format("/tmp/x.py:3:5: F821 Undefined name `y`\nFound 1 error.", "ruff");
        assert_eq!(d[0].code.as_deref(), Some("F821"));
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].line, d[0].col), (2, 4));
        assert_eq!(d[0].severity, Severity::Error);
    }

    #[test]
    fn python_buffer_syntax_error() {
        let p = std::env::temp_dir().join("florm_lint_test.py");
        let cfg = LintConfig { python_linter: "none".into(), python_args: Vec::new() };
        let (d, used) = lint_python(&p, "def f(:\n    pass\n", None, &cfg);
        assert_eq!(used, "syntaxe");
        assert!(!d.is_empty(), "no diagnostic");
        assert_eq!((d[0].line, d[0].severity), (0, Severity::Error));
    }

    #[test]
    fn codes_and_severity() {
        let d = parse_colon_format("x.py:1:1: F401 'os' imported but unused\nx.py:4:12: F821 undefined name 'y'\nx.py:2:80: E501 line too long (90 > 79 characters)\nx.py:3:1: E0602 Undefined variable 'z' (undefined-variable)", "flake8");
        assert_eq!(d.iter().map(|d| d.code.clone().unwrap()).collect::<Vec<_>>(), ["F401", "F821", "E501", "E0602"]);
        assert_eq!(d.iter().map(|d| d.severity).collect::<Vec<_>>(), [Severity::Warning, Severity::Error, Severity::Warning, Severity::Error]);
    }
}
