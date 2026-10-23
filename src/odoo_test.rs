//! Odoo tests run in a container: the `--test-tags` of the code under the cursor, the tests of
//! a file (for the ▶ of the gutter), the summary of a run, and the files of its tracebacks.

use std::path::{Path, PathBuf};

/// Command run in the container (`{tags}`: the tags asked).
pub const DEFAULT_COMMAND: &str = "odoo --stop-after-init --test-enable --test-tags {tags}";

/// Most tags kept in the history.
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

/// The command for `tags`, cut into arguments like a shell (quotes kept together).
pub fn command(template: &str, tags: &str) -> Vec<String> {
    let template = if template.trim().is_empty() { DEFAULT_COMMAND } else { template };
    split(&template.replace("{tags}", tags))
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

/// Result of a run, from the line Odoo writes at the end:
/// "odoo.tests.result: 1 failed, 0 error(s) of 12 tests when loading database 'x'".
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub failed: usize,
    pub errors: usize,
    pub total: usize,
}

pub fn summary(log: &str) -> Option<Summary> {
    let line = log.lines().rev().find(|l| l.contains(" failed, ") && l.contains(" error(s) of "))?;
    let num_before = |s: &str, pat: &str| -> Option<usize> {
        let head = &s[..s.find(pat)?];
        head.rsplit(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
    };
    let total = {
        let rest = &line[line.find(" error(s) of ")? + " error(s) of ".len()..];
        rest.split_whitespace().next()?.parse().ok()?
    };
    Some(Summary { failed: num_before(line, " failed, ")?, errors: num_before(line, " error(s) of ")?, total })
}

impl Summary {
    pub fn ok(&self) -> bool {
        self.failed == 0 && self.errors == 0
    }

    pub fn text(&self) -> String {
        if self.ok() {
            tf!("✓ {} test(s) réussi(s)", "✓ {} test(s) passed", self.total)
        } else {
            tf!("✗ {} échec(s), {} erreur(s) sur {} test(s)", "✗ {} failure(s), {} error(s) out of {} test(s)", self.failed, self.errors, self.total)
        }
    }
}

/// A traceback line in a logical line of the terminal: `File "/path/x.py", line 12` → (path, line).
pub fn traceback_location(text: &str) -> Option<(String, usize)> {
    let i = text.find("File \"")?;
    let rest = &text[i + 6..];
    let end = rest.find('"')?;
    let path = &rest[..end];
    let after = rest[end + 1..].trim_start().strip_prefix(',')?.trim_start().strip_prefix("line ")?;
    let n: usize = after.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()?;
    Some((path.to_string(), n))
}

/// A path seen in a container, on this machine: through the mounts (container folder → folder
/// here), the longest one first; else as it is.
pub fn map_path(path: &str, mounts: &[(String, PathBuf)]) -> PathBuf {
    let best = mounts
        .iter()
        .filter(|(dest, _)| path == dest || path.strip_prefix(dest.as_str()).is_some_and(|r| r.starts_with('/')))
        .max_by_key(|(dest, _)| dest.len());
    match best {
        Some((dest, src)) => src.join(path[dest.len()..].trim_start_matches('/')),
        None => PathBuf::from(path),
    }
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
    fn commands() {
        assert_eq!(command("", ":T"), ["odoo", "--stop-after-init", "--test-enable", "--test-tags", ":T"]);
        assert_eq!(command("odoo -d 'ma base' --test-tags {tags}", "/m"), ["odoo", "-d", "ma base", "--test-tags", "/m"]);
    }

    #[test]
    fn summaries() {
        let log = "2026-10-22 INFO db odoo.tests.result: 1 failed, 2 error(s) of 12 tests when loading database 'x'\n";
        assert_eq!(summary(log), Some(Summary { failed: 1, errors: 2, total: 12 }));
        assert!(summary("INFO db odoo.tests.result: 0 failed, 0 error(s) of 3 tests").unwrap().ok());
        assert_eq!(summary("nothing"), None);
    }

    #[test]
    fn tracebacks() {
        let l = r#"  File "/home/odoo/src/odoo/addons/sale/models/sale.py", line 120, in action_confirm"#;
        assert_eq!(traceback_location(l), Some(("/home/odoo/src/odoo/addons/sale/models/sale.py".into(), 120)));
        assert_eq!(traceback_location("File x"), None);
        let mounts = vec![("/opt/odoo".to_string(), PathBuf::from("/home/me/odoo")), ("/opt".to_string(), PathBuf::from("/srv"))];
        assert_eq!(map_path("/opt/odoo/addons/a.py", &mounts), PathBuf::from("/home/me/odoo/addons/a.py"));
        assert_eq!(map_path("/opt/other.py", &mounts), PathBuf::from("/srv/other.py"));
        assert_eq!(map_path("/opt/odoox/a.py", &mounts), PathBuf::from("/srv/odoox/a.py"));
        assert_eq!(map_path("/usr/lib/a.py", &mounts), PathBuf::from("/usr/lib/a.py"));
    }
}
