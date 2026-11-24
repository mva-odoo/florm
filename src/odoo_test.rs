//! Odoo tests: the `--test-tags` of the code under the cursor, the tests of a file (for the ▶ of
//! the gutter), the command that runs them (the last one, with other tags) and its summary.

use std::path::Path;

/// Most test commands kept in the history.
pub const HISTORY: usize = 12;

/// A test file of an Odoo module: `tests/…/test_*.py`.
pub fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.starts_with("test") && name.ends_with(".py") && path.components().any(|c| c.as_os_str() == "tests")
}

/// The Odoo module of `path`: the folder holding `__manifest__.py`, from the file up.
pub fn module_of(path: &Path) -> Option<String> {
    path.ancestors().skip(1).find(|d| d.join("__manifest__.py").is_file()).and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned())
}

/// Test classes and methods of a Python text: (line, tag), `:Class` on the class line and
/// `:Class.test_x` on the method one.
pub fn tests_in(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut class: Option<(String, usize)> = None;
    for (i, line) in text.lines().enumerate() {
        let indent = line.len() - line.trim_start().len();
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if class.as_ref().is_some_and(|(_, ci)| indent <= *ci) {
            class = None;
        }
        if let Some(rest) = t.strip_prefix("class ") {
            let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            // a test class: named Test… / …Test(s) / …Case, or deriving from a …Case
            let bases = rest.split_once('(').map_or("", |(_, b)| b);
            if !name.is_empty() && (name.starts_with("Test") || name.ends_with("Test") || name.ends_with("Tests") || name.ends_with("Case") || bases.contains("Case")) {
                out.push((i, format!(":{name}")));
                class = Some((name, indent));
            }
            continue;
        }
        if let Some((cname, _)) = &class {
            let def = t.strip_prefix("async def ").or_else(|| t.strip_prefix("def "));
            if let Some(m) = def.filter(|d| d.starts_with("test")) {
                let m: String = m.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                out.push((i, format!(":{cname}.{m}")));
            }
        }
    }
    out
}

/// Tags for the code at `line`: the test method or class around it in a test file, else the
/// module of the file.
pub fn tags_at(path: &Path, text: &str, line: usize) -> Option<String> {
    if is_test_file(path) {
        let lines: Vec<&str> = text.lines().collect();
        let indent = |s: &str| s.len() - s.trim_start().len();
        // a block goes on while the lines are indented more than its first one
        let contains = |start: usize| {
            let i0 = indent(lines[start]);
            (start + 1..=line).all(|k| lines.get(k).is_none_or(|s| s.trim().is_empty() || indent(s) > i0))
        };
        // the nearest method, else class, around the line
        if let Some((_, tag)) = tests_in(text).into_iter().rev().filter(|(l, _)| *l <= line).find(|(l, _)| contains(*l)) {
            return Some(tag);
        }
    }
    module_of(path).map(|m| format!("/{m}"))
}

/// A test command of Odoo: it has `--test-tags`.
pub fn is_command(cmd: &str) -> bool {
    cmd.split_whitespace().any(|w| w == "--test-tags" || w.starts_with("--test-tags="))
}

/// Odoo test tags rather than a command: `/module`, `:Class.test_x`, `/module:Class`…
pub fn is_tags(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && !s.contains(char::is_whitespace) && (s.starts_with('/') || s.starts_with(':'))
}

/// The command that runs the Odoo tests `tags`: the last Odoo test command run with its
/// `--test-tags` changed (it keeps the database, `-u`, the container…), else `odoo-bin` (found in
/// the project) with them.
pub fn command(tags: &str, last: Option<&str>, odoo_bin: Option<&str>) -> String {
    if let Some(last) = last.filter(|l| is_command(l)) {
        let mut words: Vec<String> = Vec::new();
        let mut it = last.split(' ').peekable();
        while let Some(w) = it.next() {
            if w == "--test-tags" {
                words.push(w.to_string());
                // the value after it (the spaces between them kept as one)
                while it.peek() == Some(&"") {
                    it.next();
                }
                it.next();
                words.push(tags.to_string());
            } else if w.starts_with("--test-tags=") {
                words.push(format!("--test-tags={tags}"));
            } else {
                words.push(w.to_string());
            }
        }
        return words.join(" ");
    }
    format!("{} --stop-after-init --test-enable --test-tags {tags}", odoo_bin.unwrap_or("odoo"))
}

/// `odoo-bin` in `root` or a folder under it (two levels): how to run it from `root`.
pub fn find_odoo_bin(root: &Path) -> Option<String> {
    let mut dirs = vec![root.to_path_buf()];
    for _ in 0..2 {
        if let Some(d) = dirs.iter().find(|d| d.join("odoo-bin").is_file()) {
            let rel = d.strip_prefix(root).ok()?.join("odoo-bin");
            return Some(format!("./{}", rel.to_string_lossy().replace('\\', "/")));
        }
        let mut subs = Vec::new();
        for d in &dirs {
            let Ok(rd) = std::fs::read_dir(d) else { continue };
            let mut v: Vec<_> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.')).map(|e| e.path()).collect();
            v.sort();
            subs.extend(v);
        }
        dirs = subs;
    }
    dirs.iter().find(|d| d.join("odoo-bin").is_file()).and_then(|d| Some(format!("./{}", d.strip_prefix(root).ok()?.join("odoo-bin").to_string_lossy().replace('\\', "/"))))
}

/// The summary of an Odoo run, from its log: (all passed, « 0 failed, 0 error(s) of 12 tests »).
pub fn summary(out: &str) -> Option<(bool, String)> {
    let line = out.lines().rev().find(|l| l.contains("odoo.tests.result:"))?;
    let text = line.split("odoo.tests.result:").nth(1)?.trim();
    let text = text.split(" when ").next().unwrap_or(text).trim();
    let count = |what: &str| text.split(',').find(|p| p.contains(what)).and_then(|p| p.split_whitespace().next()?.parse::<usize>().ok()).unwrap_or(0);
    Some((count("failed") == 0 && count("error") == 0, text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
from odoo.tests import TransactionCase

class TestBilling(TransactionCase):

    def setUp(self):
        super().setUp()

    def test_renew(self):
        x = 1

        self.assertTrue(x)

    def helper(self):
        pass

class Other:
    def test_no(self):
        pass
";

    #[test]
    fn tests_of_a_file() {
        assert_eq!(tests_in(FILE), vec![(2, ":TestBilling".into()), (7, ":TestBilling.test_renew".into())]);
    }

    #[test]
    fn tags_under_the_cursor() {
        let p = Path::new("/nowhere/mod/tests/test_billing.py");
        assert_eq!(tags_at(p, FILE, 10).as_deref(), Some(":TestBilling.test_renew"));
        assert_eq!(tags_at(p, FILE, 9).as_deref(), Some(":TestBilling.test_renew"));
        assert_eq!(tags_at(p, FILE, 4).as_deref(), Some(":TestBilling"));
        assert_eq!(tags_at(p, FILE, 13).as_deref(), Some(":TestBilling"));
        assert_eq!(tags_at(p, FILE, 0), None);
        assert_eq!(tags_at(p, FILE, 17), None);
    }

    #[test]
    fn commands_of_odoo_tests() {
        assert!(is_tags("/sale:TestX.test_y") && is_tags(":TestX") && !is_tags("pytest a.py") && !is_tags("/a b"));
        assert!(is_command("./odoo-bin -d db --test-tags /sale") && is_command("odoo --test-tags=/sale") && !is_command("pytest"));
        // the last one keeps everything but its tags
        let last = "docker compose exec odoo odoo -d db -u sale --stop-after-init --test-tags  /sale:TestA --log-level=test";
        assert_eq!(command("/sale:TestB.test_c", Some(last), None), "docker compose exec odoo odoo -d db -u sale --stop-after-init --test-tags /sale:TestB.test_c --log-level=test");
        assert_eq!(command("/m", Some("odoo --test-tags=/x -d db"), None), "odoo --test-tags=/m -d db");
        assert_eq!(command("/m", Some("pytest a.py"), Some("./odoo-bin")), "./odoo-bin --stop-after-init --test-enable --test-tags /m");
        assert_eq!(command("/m", None, None), "odoo --stop-after-init --test-enable --test-tags /m");
    }

    #[test]
    fn summary_of_a_run() {
        let log = "2026-11-24 10:00:00,000 1 INFO db odoo.modules.loading: done\n2026-11-24 10:00:01,000 1 ERROR db odoo.tests.result: 1 failed, 0 error(s) of 12 tests when loading database 'db'\n";
        assert_eq!(summary(log), Some((false, "1 failed, 0 error(s) of 12 tests".into())));
        assert_eq!(summary("INFO db odoo.tests.result: 0 failed, 0 error(s) of 3 tests when loading database 'db'"), Some((true, "0 failed, 0 error(s) of 3 tests".into())));
        assert_eq!(summary("nothing"), None);
    }

    #[test]
    fn odoo_bin_of_the_project() {
        let dir = std::env::temp_dir().join(format!("florm_odoo_bin_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/odoo")).unwrap();
        assert_eq!(find_odoo_bin(&dir), None);
        std::fs::write(dir.join("src/odoo/odoo-bin"), "").unwrap();
        assert_eq!(find_odoo_bin(&dir).as_deref(), Some("./src/odoo/odoo-bin"));
        std::fs::write(dir.join("odoo-bin"), "").unwrap();
        assert_eq!(find_odoo_bin(&dir).as_deref(), Some("./odoo-bin"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
