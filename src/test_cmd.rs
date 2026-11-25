//! Tests of any project, run where it is (no container): pytest, jest (or vitest), `cargo test`,
//! `go test`. A test is a command line, shown and kept in the history as typed: `pytest
//! tests/test_a.py::TestX::test_y`, `cargo test app::x::tests::y`, `go test . -run '^TestX$'`,
//! `npx jest src/a.test.ts -t 'adds'`. A project in a sub-folder starts with `cd sub && `.

use std::path::{Path, PathBuf};

/// The runner of a test file.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Runner {
    Pytest,
    Jest,
    Cargo,
    Go,
}

/// The runner of `path`, from its name: `test_*.py` / `*_test.py`, `*.test.js` / `*.spec.ts`…,
/// `*_test.go`, a Rust file (its tests are inside it).
pub fn runner_of(path: &Path) -> Option<Runner> {
    let name = path.file_name()?.to_str()?;
    let ext = path.extension()?.to_str()?;
    match ext {
        "py" if name.starts_with("test_") || name.ends_with("_test.py") => Some(Runner::Pytest),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" if name.contains(".test.") || name.contains(".spec.") => Some(Runner::Jest),
        "go" if name.ends_with("_test.go") => Some(Runner::Go),
        "rs" => Some(Runner::Cargo),
        _ => None,
    }
}

/// The folder a run starts in: the nearest holding the file of the project (`Cargo.toml`,
/// `go.mod`, `package.json`; for pytest its settings, else the folder of the project).
fn base_of(path: &Path, runner: Runner, root: &Path) -> PathBuf {
    let marks: &[&str] = match runner {
        Runner::Cargo => &["Cargo.toml"],
        Runner::Go => &["go.mod"],
        Runner::Jest => &["package.json"],
        Runner::Pytest => &["pytest.ini", "pyproject.toml", "setup.cfg", "tox.ini", "conftest.py"],
    };
    path.ancestors()
        .skip(1)
        .take_while(|d| d.starts_with(root))
        .filter(|d| marks.iter().any(|m| d.join(m).is_file()))
        // pytest: the highest one (a conftest.py of a sub-folder is not the root)
        .reduce(|near, far| if runner == Runner::Pytest { far } else { near })
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf())
}

fn rel(path: &Path, base: &Path) -> String {
    let r = path.strip_prefix(base).unwrap_or(path).to_string_lossy().replace('\\', "/");
    if r.is_empty() { ".".into() } else { r }
}

/// `'text'` for the command line, read back by `split`.
fn quote(s: &str) -> String {
    if s.chars().all(|c| c.is_alphanumeric() || "_-./:".contains(c)) { s.to_string() } else { format!("'{}'", s.replace('\'', "")) }
}

/// The program of a run: the one of the project's virtual environment first (pytest), `npx`
/// with jest or vitest (as `package.json` says).
fn program(runner: Runner, base: &Path, root: &Path) -> String {
    match runner {
        Runner::Pytest => {
            for dir in [base, root] {
                for venv in [".venv", "venv", "env"] {
                    let bin = if cfg!(windows) { dir.join(venv).join("Scripts").join("pytest.exe") } else { dir.join(venv).join("bin").join("pytest") };
                    if bin.is_file() {
                        return rel(&bin, base);
                    }
                }
            }
            "pytest".into()
        }
        Runner::Jest => {
            let pkg = std::fs::read_to_string(base.join("package.json")).unwrap_or_default();
            if pkg.contains("\"vitest\"") && !pkg.contains("\"jest\"") { "npx vitest run".into() } else { "npx jest".into() }
        }
        Runner::Cargo => "cargo test".into(),
        Runner::Go => "go test".into(),
    }
}

/// The tests of the file `path` of the project `root`: (line, command), for the ▶ of the gutter.
pub fn tests_in(path: &Path, text: &str, root: &Path) -> Vec<(usize, String)> {
    let Some(runner) = runner_of(path) else { return Vec::new() };
    // the text first: the files asked to the disk only for a file with tests
    let names = match runner {
        Runner::Pytest => python_tests(text),
        Runner::Jest => js_tests(text),
        Runner::Cargo => rust_tests(text),
        Runner::Go => go_tests(text),
    };
    if names.is_empty() {
        return Vec::new();
    }
    let base = base_of(path, runner, root);
    let cd = if base == root { String::new() } else { format!("cd {} && ", quote(&rel(&base, root))) };
    let prog = program(runner, &base, root);
    let file = rel(path, &base);
    names
        .into_iter()
        .map(|(line, name)| {
            let cmd = match runner {
                Runner::Pytest => format!("{prog} {}", quote(&format!("{file}::{name}"))),
                Runner::Jest => format!("{prog} {} -t {}", quote(&file), quote(&regex_escape(&name))),
                Runner::Cargo => format!("{prog} {}", rust_filter(path, &base, &name)),
                Runner::Go => format!("{prog} {} -run {}", quote(&go_package(path, &base)), quote(&format!("^{name}$"))),
            };
            (line, format!("{cd}{cmd}"))
        })
        .collect()
}

/// The command of the test around `line` (the nearest one starting above it), else of the whole file.
pub fn command_at(path: &Path, text: &str, root: &Path, line: usize) -> Option<String> {
    let runner = runner_of(path)?;
    let tests = tests_in(path, text, root);
    if let Some((_, cmd)) = tests.iter().rev().find(|(l, _)| *l <= line) {
        return Some(cmd.clone());
    }
    if tests.is_empty() {
        return None;
    }
    let base = base_of(path, runner, root);
    let cd = if base == root { String::new() } else { format!("cd {} && ", quote(&rel(&base, root))) };
    let prog = program(runner, &base, root);
    Some(match runner {
        Runner::Pytest | Runner::Jest => format!("{cd}{prog} {}", quote(&rel(path, &base))),
        Runner::Cargo => format!("{cd}{prog}"),
        Runner::Go => format!("{cd}{prog} {}", quote(&go_package(path, &base))),
    })
}

/// `s` cut into arguments like a shell (quotes kept together).
pub fn split(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut quote, mut any) = (Vec::new(), String::new(), None, false);
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A command of these runners: its folder (relative to the project) and
/// its arguments.
pub fn parse(cmd: &str) -> Option<(Option<String>, Vec<String>)> {
    // cut like a shell first: a `&&` inside quotes (a folder named so) is part of a word
    let mut argv = split(cmd.trim());
    let dir = if argv.first().map(String::as_str) == Some("cd") && argv.get(2).map(String::as_str) == Some("&&") {
        let d = argv[1].clone();
        argv.drain(..3);
        Some(d)
    } else {
        None
    };
    let prog = argv.first()?;
    let name = Path::new(prog).file_stem()?.to_str()?;
    let known = ["pytest", "py.test", "python", "python3", "py", "uv", "cargo", "go", "npx", "npm", "pnpm", "yarn", "jest", "vitest", "bun", "deno"];
    known.contains(&name).then_some((dir, argv))
}

/// The summary of a run, from its last lines: (all passed, text).
pub fn summary(out: &str) -> Option<(bool, String)> {
    for line in out.lines().rev().map(str::trim) {
        // pytest: "==== 2 failed, 5 passed in 0.12s ===="
        if line.starts_with('=') && line.ends_with('=') && (line.contains(" passed") || line.contains(" failed") || line.contains(" error") || line.contains("no tests ran")) {
            let t = line.trim_matches(|c| c == '=' || c == ' ');
            return Some((!t.contains("failed") && !t.contains("error") && !t.contains("no tests"), t.to_string()));
        }
        // jest / vitest: "Tests:       1 failed, 3 passed, 4 total", "Tests  3 passed (3)"
        if let Some(t) = line.strip_prefix("Tests:").or_else(|| line.strip_prefix("Tests ")) {
            if t.contains("passed") || t.contains("failed") {
                return Some((!t.contains("failed"), t.trim().to_string()));
            }
        }
    }
    // cargo: one "test result:" per test binary, all added up
    let (mut passed, mut failed, mut any) = (0, 0, false);
    for line in out.lines().filter_map(|l| l.trim().strip_prefix("test result: ")) {
        any = true;
        // "ok. 4 passed; 0 failed; 0 ignored…"
        let count = |what: &str| {
            line.split(';').find_map(|p| {
                let mut w = p.split_whitespace().filter(|w| !w.ends_with('.'));
                let n = w.next()?.parse::<u32>().ok()?;
                (w.next()? == what).then_some(n)
            })
        };
        passed += count("passed").unwrap_or(0);
        failed += count("failed").unwrap_or(0);
    }
    if any {
        return Some((failed == 0, crate::tf!("{passed} réussis, {failed} en échec", "{passed} passed, {failed} failed")));
    }
    // go: "ok  	pkg	0.01s", "FAIL	pkg	0.01s", "--- FAIL: TestX"
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    let fails = lines.iter().filter(|l| l.starts_with("--- FAIL:")).count();
    let ok = lines.iter().any(|l| l.starts_with("ok ") || l.starts_with("ok\t"));
    if fails > 0 || lines.iter().any(|l| l.starts_with("FAIL\t") || *l == "FAIL") {
        return Some((false, crate::tf!("{fails} en échec", "{fails} failed")));
    }
    ok.then(|| (true, crate::t!("réussis", "passed").to_string()))
}

fn regex_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn ident(s: &str) -> String {
    s.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()
}

/// pytest: `test_…` functions, `Test…` classes and their `test_…` methods: `Class::method`.
fn python_tests(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut class: Option<(String, usize)> = None;
    for (i, line) in text.lines().enumerate() {
        let indent = line.len() - line.trim_start().len();
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') || t.starts_with('@') {
            continue;
        }
        if class.as_ref().is_some_and(|(_, ci)| indent <= *ci) {
            class = None;
        }
        if let Some(rest) = t.strip_prefix("class ") {
            let name = ident(rest);
            if name.starts_with("Test") || rest.split_once('(').is_some_and(|(_, b)| b.contains("TestCase")) {
                out.push((i, name.clone()));
                class = Some((name, indent));
            }
            continue;
        }
        let Some(def) = t.strip_prefix("async def ").or_else(|| t.strip_prefix("def ")) else { continue };
        let name = ident(def);
        if !name.starts_with("test") {
            continue;
        }
        match &class {
            Some((c, _)) => out.push((i, format!("{c}::{name}"))),
            None if indent == 0 => out.push((i, name)),
            None => {}
        }
    }
    out
}

/// jest: `describe(…)`, `it(…)`, `test(…)` with a written name (`.only`, `.skip`, `.each` kept out).
fn js_tests(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        let Some(rest) = ["describe(", "it(", "test(", "describe.only(", "it.only(", "test.only("].iter().find_map(|p| t.strip_prefix(p)) else { continue };
        let rest = rest.trim_start();
        let Some(q) = rest.chars().next().filter(|c| matches!(c, '\'' | '"' | '`')) else { continue };
        if let Some(end) = rest[1..].find(q) {
            let name = &rest[1..1 + end];
            if !name.is_empty() && !name.contains("${") {
                out.push((i, name.to_string()));
            }
        }
    }
    out
}

/// Rust: the `#[test]` functions (`#[tokio::test]`…), with the inline modules around them.
fn rust_tests(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    // (module, indent of its line)
    let mut mods: Vec<(String, usize)> = Vec::new();
    let mut attr = false;
    for (i, line) in text.lines().enumerate() {
        let indent = line.len() - line.trim_start().len();
        let t = line.trim_start();
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        if t.starts_with('}') {
            if mods.last().is_some_and(|(_, mi)| *mi == indent) {
                mods.pop();
            }
            continue;
        }
        if t.starts_with("#[") {
            let a = t.trim_start_matches("#[");
            if a.starts_with("test]") || a.starts_with("test ") || (a.contains("::test") && !a.contains("cfg")) {
                attr = true;
            }
            continue;
        }
        let t2 = t.strip_prefix("pub ").or_else(|| t.strip_prefix("pub(crate) ")).unwrap_or(t);
        if let Some(m) = t2.strip_prefix("mod ") {
            if m.trim_end().ends_with('{') {
                mods.push((ident(m), indent));
            }
            attr = false;
            continue;
        }
        if attr {
            let f = t2.strip_prefix("async fn ").or_else(|| t2.strip_prefix("fn "));
            if let Some(f) = f {
                let path: Vec<&str> = mods.iter().map(|(m, _)| m.as_str()).collect();
                let name = ident(f);
                out.push((i, if path.is_empty() { name } else { format!("{}::{name}", path.join("::")) }));
            }
            attr = false;
        }
    }
    out
}

/// The filter of `cargo test` for the test `name` of `path`: the module path of the file
/// before it (`src/app/x.rs` → `app::x::`), `--test file` for a file of `tests/`.
fn rust_filter(path: &Path, base: &Path, name: &str) -> String {
    let r = rel(path, base);
    if let Some(t) = r.strip_prefix("tests/") {
        let stem = t.trim_end_matches(".rs");
        if !stem.contains('/') {
            return format!("--test {} {}", quote(stem), quote(name));
        }
    }
    let inner = r.strip_prefix("src/").unwrap_or(&r).trim_end_matches(".rs");
    let mut parts: Vec<&str> = inner.split('/').collect();
    if matches!(parts.last(), Some(&"mod")) {
        parts.pop();
    }
    if parts.len() == 1 && matches!(parts[0], "lib" | "main") {
        parts.clear();
    }
    parts.push(name);
    quote(&parts.join("::"))
}

/// Go: the `func TestX(t *testing.T)`.
fn go_tests(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let f = l.strip_prefix("func Test")?;
            l.contains("*testing.T").then(|| (i, format!("Test{}", ident(f))))
        })
        .collect()
}

/// `./dir` of the package of `path`, from the folder of `go.mod` (`.` for its own).
fn go_package(path: &Path, base: &Path) -> String {
    match path.parent().map(|d| rel(d, base)) {
        Some(d) if d != "." => format!("./{d}"),
        _ => ".".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("florm_test_cmd_{}_{}", std::process::id(), files[0].0.replace('/', "_")));
        let _ = std::fs::remove_dir_all(&dir);
        for (f, text) in files {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        dir
    }

    #[test]
    fn pytest_tests_and_the_one_under_the_cursor() {
        let src = "import x\n\ndef test_one():\n    pass\n\nclass TestTwo:\n    def test_a(self):\n        pass\n    def helper(self):\n        pass\n\ndef other():\n    pass\n";
        let root = project(&[("pyproject.toml", ""), ("tests/test_x.py", src)]);
        let f = root.join("tests/test_x.py");
        let t = tests_in(&f, src, &root);
        let cmds: Vec<&str> = t.iter().map(|(_, c)| c.as_str()).collect();
        assert_eq!(cmds, ["pytest tests/test_x.py::test_one", "pytest tests/test_x.py::TestTwo", "pytest tests/test_x.py::TestTwo::test_a"]);
        assert_eq!(t[2].0, 6);
        assert_eq!(command_at(&f, src, &root, 7).as_deref(), Some("pytest tests/test_x.py::TestTwo::test_a"));
        assert_eq!(command_at(&f, src, &root, 0).as_deref(), Some("pytest tests/test_x.py"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn jest_in_a_sub_folder() {
        let src = "describe('math', () => {\n  it(\"adds (two)\", () => {});\n  test(`x ${a}`, () => {});\n});\n";
        let root = project(&[("web/package.json", "{\"devDependencies\": {\"jest\": \"1\"}}"), ("web/src/a.test.ts", src)]);
        let t = tests_in(&root.join("web/src/a.test.ts"), src, &root);
        let cmds: Vec<&str> = t.iter().map(|(_, c)| c.as_str()).collect();
        assert_eq!(cmds, ["cd web && npx jest src/a.test.ts -t math", "cd web && npx jest src/a.test.ts -t 'adds \\(two\\)'"]);
        let (dir, argv) = parse(cmds[1]).unwrap();
        assert_eq!(dir.as_deref(), Some("web"));
        assert_eq!(argv, ["npx", "jest", "src/a.test.ts", "-t", "adds \\(two\\)"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cargo_and_go_tests() {
        let rs = "fn f() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn adds() {}\n\n    mod deep {\n        #[tokio::test]\n        async fn waits() {}\n    }\n}\n";
        let go = "package calc\n\nimport \"testing\"\n\nfunc TestAdd(t *testing.T) {}\nfunc helper() {}\n";
        let root = project(&[("Cargo.toml", ""), ("src/app/calc.rs", rs), ("go.mod", ""), ("calc/calc_test.go", go)]);
        let t: Vec<String> = tests_in(&root.join("src/app/calc.rs"), rs, &root).into_iter().map(|(_, c)| c).collect();
        assert_eq!(t, ["cargo test app::calc::tests::adds", "cargo test app::calc::tests::deep::waits"]);
        let t: Vec<(usize, String)> = tests_in(&root.join("calc/calc_test.go"), go, &root);
        assert_eq!(t, [(4, "go test ./calc -run '^TestAdd$'".to_string())]);
        assert!(tests_in(&root.join("src/app/plain.rs"), "fn f() {}\n", &root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_named_with_and_and_stays_a_folder() {
        // a folder `x&&python3 evil.py` of a repository: quoted in the command, not a second command
        let (dir, argv) = parse("cd 'x&&python3 evil.py' && npx jest a.test.ts").unwrap();
        assert_eq!(dir.as_deref(), Some("x&&python3 evil.py"));
        assert_eq!(argv, ["npx", "jest", "a.test.ts"]);
        assert!(parse("cd 'x&&python3' evil.py").is_none());
    }

    #[test]
    fn only_runner_commands_are_commands() {
        assert!(parse("/sale:TestX.test_y").is_none());
        assert!(parse("cargo test x").is_some());
    }

    #[test]
    fn summaries() {
        assert_eq!(summary("...\n===== 1 failed, 4 passed in 0.20s =====\n"), Some((false, "1 failed, 4 passed in 0.20s".into())));
        assert_eq!(summary("Tests:       3 passed, 3 total\nTime: 1s\n").map(|s| s.0), Some(true));
        let cargo = "test result: ok. 4 passed; 0 failed; 0 ignored\n\ntest result: FAILED. 1 passed; 2 failed; 0 ignored\n";
        assert_eq!(summary(cargo).map(|s| s.0), Some(false));
        assert!(summary(cargo).unwrap().1.contains('5'));
        assert_eq!(summary("--- FAIL: TestAdd (0.00s)\nFAIL\nFAIL\tcalc\t0.01s\n").map(|s| s.0), Some(false));
        assert_eq!(summary("ok  \tcalc\t0.01s\n").map(|s| s.0), Some(true));
    }
}
