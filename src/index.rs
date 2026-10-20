//! Index of the project's symbols (like Sublime Text's): built in the background, updated when a
//! file is saved; used by "go to definition" and the completion when no language server answers.

use crate::symbols::{self, Kind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MAX_FILE: u64 = 1_500_000;
const MAX_FILES: usize = 60_000;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    /// 0-based line.
    pub line: usize,
    pub detail: String,
}

#[derive(Default)]
pub struct Index {
    pub files: HashMap<PathBuf, Vec<Entry>>,
    /// Distinct names, sorted (completion by prefix).
    names: Vec<String>,
}

fn entries(text: &str, syntax: &str) -> Vec<Entry> {
    symbols::extract_for_index(text, syntax)
        .into_iter()
        .filter(|s| s.kind != Kind::Heading)
        .map(|s| Entry { name: s.name, kind: s.kind, line: s.line, detail: s.detail })
        .collect()
}

impl Index {
    pub fn build(root: &Path) -> Index {
        let mut idx = Index::default();
        let walker = ignore::WalkBuilder::new(root).hidden(false).filter_entry(|e| e.file_name() != ".git").build();
        for e in walker.flatten().filter(|e| e.file_type().is_some_and(|t| t.is_file())).take(MAX_FILES) {
            let p = e.path();
            let Some(syntax) = symbols::syntax_for(p).filter(|s| *s != "Markdown") else { continue };
            if e.metadata().map_or(true, |m| m.len() > MAX_FILE) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(p) else { continue };
            let v = entries(&text, syntax);
            if !v.is_empty() {
                idx.files.insert(p.to_path_buf(), v);
            }
        }
        idx.rebuild_names();
        idx
    }

    fn rebuild_names(&mut self) {
        let mut n: Vec<String> = self.files.values().flatten().map(|e| e.name.clone()).collect();
        n.sort_unstable();
        n.dedup();
        self.names = n;
    }

    /// A file changed (saved): its symbols are extracted again.
    pub fn update(&mut self, path: &Path, text: &str) {
        let Some(syntax) = symbols::syntax_for(path) else { return };
        let v = entries(text, syntax);
        if v.is_empty() {
            self.files.remove(path);
        } else {
            self.files.insert(path.to_path_buf(), v);
        }
        self.rebuild_names();
    }

    /// Definitions of `name`. "module.xml_id" also finds the record "xml_id", and "sale.order" the
    /// model declared with `_name = 'sale.order'`.
    pub fn lookup(&self, name: &str) -> Vec<(PathBuf, Entry)> {
        let short = name.rsplit('.').next().unwrap_or(name);
        let mut out: Vec<(PathBuf, Entry)> = self
            .files
            .iter()
            .flat_map(|(p, v)| v.iter().map(move |e| (p, e)))
            .filter(|(_, e)| e.name == name || (short != name && e.kind == Kind::Record && e.name == short))
            .map(|(p, e)| (p.clone(), e.clone()))
            .collect();
        // classes and functions before the rest (fields…), then by file
        let rank = |k: Kind| match k {
            Kind::Class | Kind::Record => 0,
            Kind::Function | Kind::Method => 1,
            _ => 2,
        };
        out.sort_by(|a, b| rank(a.1.kind).cmp(&rank(b.1.kind)).then_with(|| a.0.cmp(&b.0)).then(a.1.line.cmp(&b.1.line)));
        out
    }

    /// Names starting with `prefix` (case-sensitive, at most `limit`).
    pub fn names_with_prefix(&self, prefix: &str, limit: usize) -> Vec<&str> {
        let start = self.names.partition_point(|n| n.as_str() < prefix);
        self.names[start..].iter().take_while(|n| n.starts_with(prefix)).take(limit).map(String::as_str).collect()
    }

    pub fn len(&self) -> usize {
        self.files.values().map(Vec::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odoo_project() {
        let root = std::env::temp_dir().join(format!("florm_index_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("syndic/models")).unwrap();
        std::fs::create_dir_all(root.join("syndic/views")).unwrap();
        std::fs::write(
            root.join("syndic/models/budget.py"),
            "from odoo import fields, models\n\n\nclass SyndicBudget(models.Model):\n    _name = 'syndic.budget'\n\n    amount = fields.Float()\n\n    def compute_total(self):\n        return 1\n",
        )
        .unwrap();
        std::fs::write(root.join("syndic/views/budget.xml"), "<odoo>\n    <record id=\"view_budget_form\" model=\"ir.ui.view\">\n    </record>\n</odoo>\n").unwrap();
        let mut idx = Index::build(&root);
        let one = |n: &str| idx.lookup(n).into_iter().map(|(p, e)| (p.file_name().unwrap().to_string_lossy().into_owned(), e.line)).collect::<Vec<_>>();
        assert_eq!(one("compute_total"), [("budget.py".to_string(), 8)]);
        assert_eq!(one("SyndicBudget"), [("budget.py".to_string(), 3)]);
        assert_eq!(one("syndic.budget"), [("budget.py".to_string(), 4)]);
        assert_eq!(one("amount"), [("budget.py".to_string(), 6)]);
        assert_eq!(one("syndic.view_budget_form"), [("budget.xml".to_string(), 1)]);
        assert_eq!(idx.names_with_prefix("comp", 10), ["compute_total"]);
        // saving a file updates it
        let p = root.join("syndic/models/budget.py");
        idx.update(&p, "def compute_other():\n    pass\n");
        assert!(idx.lookup("compute_total").is_empty());
        assert_eq!(idx.names_with_prefix("compute", 10), ["compute_other"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `FLORM_BENCH_INDEX=<folder> cargo test --release bench_index -- --nocapture`
    #[test]
    fn bench_index() {
        let Ok(dir) = std::env::var("FLORM_BENCH_INDEX") else { return };
        let t = std::time::Instant::now();
        let idx = Index::build(Path::new(&dir));
        eprintln!("{} symboles, {} fichiers, {:?}", idx.len(), idx.files.len(), t.elapsed());
        let t = std::time::Instant::now();
        let n = idx.lookup("sale.order").len();
        eprintln!("lookup sale.order : {n} résultat(s) en {:?}", t.elapsed());
    }
}
