//! Breakpoints written in the code (`import ipdb; ipdb.set_trace()`, `debugger;`) rather than a
//! debugger: the lines that are one, the line F9 writes, where pdb / ipdb stopped (from what it
//! prints in a terminal), the ones a diff adds (not to commit them), and the tracebacks of Python.

use std::path::{Path, PathBuf};

/// The languages whose breakpoints Florm knows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Python,
    /// JavaScript and TypeScript: `debugger;`.
    Js,
}

/// Line written by F9 in Python when the template `ipdb` has none of one line.
pub const PYTHON: &str = "import ipdb; ipdb.set_trace()";

pub fn lang_of(path: &Path) -> Option<Lang> {
    match path.extension()?.to_str()? {
        "py" => Some(Lang::Python),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Some(Lang::Js),
        _ => None,
    }
}

/// `line` stops the program (`ipdb.set_trace()`, `pdb.set_trace()`, `breakpoint()`…; `debugger;`),
/// and is not a comment.
pub fn is_trace(lang: Lang, line: &str) -> bool {
    let t = line.trim();
    match lang {
        Lang::Python => {
            // pdb, ipdb, pudb, pdbpp: `import ipdb; ipdb.set_trace()`, `__import__('ipdb').set_trace()`
            !t.starts_with('#') && !t.starts_with("def ") && (t.starts_with("breakpoint(") || (t.contains(".set_trace(") && (t.contains("pdb") || t.contains("pudb"))))
        }
        Lang::Js => t == "debugger" || t == "debugger;",
    }
}

/// The line F9 writes: in Python the template triggered by `ipdb` (one line, the user's own if
/// they changed it), else `PYTHON`.
pub fn statement(lang: Lang) -> String {
    match lang {
        Lang::Python => {
            let all = crate::templates::load_all();
            let body = all.iter().find(|t| t.ext == "py" && t.trigger == "ipdb").map(|t| t.body.replace("$0", "")).unwrap_or_default();
            let body = body.trim();
            if body.is_empty() || body.contains('\n') { PYTHON.to_string() } else { body.to_string() }
        }
        Lang::Js => "debugger;".to_string(),
    }
}

/// `path(12)` at the start of `w`, as pdb writes where it is (`/a/b.py(12)f()`): the path, the line.
pub fn pdb_location(w: &str) -> Option<(&str, usize)> {
    let open = w.find('(')?;
    let (path, rest) = (&w[..open], &w[open + 1..]);
    let close = rest.find(')')?;
    let line = rest[..close].parse().ok()?;
    // a file: a name with an extension (not `f(12)` in some code)
    let name = path.rsplit(['/', '\\']).next()?;
    name.rsplit_once('.').filter(|(stem, ext)| !stem.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()) && !ext.is_empty())?;
    Some((path, line))
}

/// The prompt of pdb or ipdb waiting for a command.
pub fn is_prompt(line: &str) -> bool {
    let t = line.trim_start();
    ["ipdb>", "(Pdb)", "(Pdb++)", "(pdb)", "pdb>"].iter().any(|p| t.starts_with(p))
}

/// Where pdb / ipdb waits, from the visible lines of a terminal: the last `> file(line)function()`
/// when its prompt is the last line written. `None`: no debugger waits there.
pub fn stop_in(lines: &[String]) -> Option<(String, usize)> {
    let mut rows = lines.iter().map(|l| l.trim_end()).filter(|l| !l.is_empty()).rev();
    if !is_prompt(rows.next()?) {
        return None;
    }
    rows.find_map(|l| pdb_location(l.strip_prefix("> ")?)).map(|(p, n)| (p.to_string(), n))
}

/// A traceback line: `File "/path/x.py", line 12` → (path, line).
pub fn traceback_location(text: &str) -> Option<(String, usize)> {
    let i = text.find("File \"")?;
    let rest = &text[i + 6..];
    let end = rest.find('"')?;
    let path = &rest[..end];
    let after = rest[end + 1..].trim_start().strip_prefix(',')?.trim_start().strip_prefix("line ")?;
    let n: usize = after.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()?;
    Some((path.to_string(), n))
}

/// The file of `path` printed by a program (maybe in a container: `/opt/odoo/addons/sale/x.py`),
/// here: as it is, else its end under one of `dirs` (`sale/x.py` in the project), the longest end
/// first and at least a folder and the file.
pub fn find_file(path: &str, dirs: &[&Path]) -> Option<PathBuf> {
    let p = Path::new(path);
    if p.is_absolute() && p.is_file() {
        return Some(p.to_path_buf());
    }
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty() && *s != ".").collect();
    if !p.is_absolute() {
        if let Some(f) = dirs.iter().map(|d| d.join(path)).find(|f| f.is_file()) {
            return Some(f);
        }
    }
    for k in (2..=parts.len()).rev() {
        let end: PathBuf = parts[parts.len() - k..].iter().collect();
        if let Some(f) = dirs.iter().map(|d| d.join(&end)).find(|f| f.is_file()) {
            return Some(f);
        }
    }
    None
}

/// The breakpoints a unified diff adds (`git diff -U0`): (file, 1-based line).
pub fn added_in_diff(diff: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let (mut file, mut lang, mut line) = (String::new(), None, 0usize);
    for l in diff.lines() {
        if let Some(f) = l.strip_prefix("+++ ") {
            file = f.strip_prefix("b/").unwrap_or(f).to_string();
            lang = lang_of(Path::new(&file));
        } else if let Some(h) = l.strip_prefix("@@ ") {
            // @@ -a,b +c,d @@
            line = h.split_whitespace().find_map(|w| w.strip_prefix('+')).and_then(|w| w.split(',').next()?.parse().ok()).unwrap_or(0);
        } else if let Some(text) = l.strip_prefix('+') {
            if lang.is_some_and(|g| is_trace(g, text)) {
                out.push((file.clone(), line));
            }
            line += 1;
        } else if l.starts_with(' ') {
            line += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoint_lines() {
        for l in ["import ipdb; ipdb.set_trace()", "        import pdb; pdb.set_trace()", "breakpoint()", "__import__('ipdb').set_trace()", "from pudb import set_trace; pudb.set_trace()"] {
            assert!(is_trace(Lang::Python, l), "{l}");
        }
        for l in ["# import ipdb; ipdb.set_trace()", "x = 1", "def set_trace(self):", "\"\"\"ipdb\"\"\""] {
            assert!(!is_trace(Lang::Python, l), "{l}");
        }
        assert!(is_trace(Lang::Js, "    debugger;"));
        assert!(!is_trace(Lang::Js, "// debugger;"));
        assert_eq!(lang_of(Path::new("a/b.tsx")), Some(Lang::Js));
        assert_eq!(lang_of(Path::new("a/b.rs")), None);
    }

    #[test]
    fn where_pdb_waits() {
        let screen = |s: &str| s.lines().map(String::from).collect::<Vec<_>>();
        let s = screen("$ odoo -d db\n> /home/me/odoo/addons/sale/models/sale.py(120)action_confirm()\n    119         import ipdb; ipdb.set_trace()\n--> 120         self.write({})\n\nipdb> \n\n");
        assert_eq!(stop_in(&s), Some(("/home/me/odoo/addons/sale/models/sale.py".into(), 120)));
        let s = screen("> /a/b.py(3)<module>()\n-> x = 1\n(Pdb) n\n> /a/b.py(4)<module>()\n-> y = 2\n(Pdb) ");
        assert_eq!(stop_in(&s), Some(("/a/b.py".into(), 4)));
        // it went on: the prompt is no longer the last line
        let s = screen("> /a/b.py(4)<module>()\n(Pdb) c\nINFO server running");
        assert_eq!(stop_in(&s), None);
        assert_eq!(pdb_location("/a/b.py(12)f()"), Some(("/a/b.py", 12)));
        assert_eq!(pdb_location("print(12)"), None);
    }

    #[test]
    fn files_of_a_container_found_here() {
        let dir = std::env::temp_dir().join(format!("florm_trace_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("addons/sale/models")).unwrap();
        std::fs::write(dir.join("addons/sale/models/sale.py"), "").unwrap();
        let want = Some(dir.join("addons/sale/models/sale.py"));
        assert_eq!(find_file("/opt/odoo/addons/sale/models/sale.py", &[&dir]), want);
        assert_eq!(find_file("addons/sale/models/sale.py", &[&dir]), want);
        assert_eq!(find_file("/opt/odoo/other.py", &[&dir]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn breakpoints_added_by_a_diff() {
        let diff = "diff --git a/m.py b/m.py\n--- a/m.py\n+++ b/m.py\n@@ -10,0 +11,2 @@ def f():\n+    import ipdb; ipdb.set_trace()\n+    x = 1\n@@ -40 +42 @@\n-    y\n+    breakpoint()\n--- a/w.js\n+++ b/w.js\n@@ -1,0 +2 @@\n+debugger;\n--- a/r.md\n+++ b/r.md\n@@ -1,0 +1 @@\n+breakpoint()\n";
        assert_eq!(added_in_diff(diff), [("m.py".into(), 11), ("m.py".into(), 42), ("w.js".into(), 2)]);
        assert_eq!(traceback_location(r#"  File "/a/b.py", line 120, in f"#), Some(("/a/b.py".into(), 120)));
    }
}
