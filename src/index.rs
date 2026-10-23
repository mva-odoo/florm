//! Index of the project's symbols (like Sublime Text's): built in the background, updated when a
//! file is saved; used by "go to definition" and the completion when no language server answers.
//!
//! Millions of symbols in a big project (Odoo: 1.7 million): each name and detail is kept once
//! (`Strings`) and a symbol is 16 bytes.

use crate::symbols::{self, Kind};
use crate::walk::{self, Exclusions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

const MAX_FILE: u64 = 1_500_000;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    /// 0-based line.
    pub line: usize,
    pub detail: String,
}

/// A symbol in the index: its name and detail are numbers in `Strings`.
#[derive(Clone, Copy)]
struct Sym {
    name: u32,
    detail: u32,
    line: u32,
    kind: Kind,
}

/// Each distinct string once, by number (0: the empty string).
#[derive(Clone)]
struct Strings {
    all: Vec<Arc<str>>,
    ids: HashMap<Arc<str>, u32>,
}

impl Default for Strings {
    fn default() -> Self {
        let empty: Arc<str> = Arc::from("");
        Strings { all: vec![empty.clone()], ids: HashMap::from([(empty, 0)]) }
    }
}

impl Strings {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.ids.get(s) {
            return id;
        }
        let a: Arc<str> = Arc::from(s);
        let id = self.all.len() as u32;
        self.all.push(a.clone());
        self.ids.insert(a, id);
        id
    }

    fn get(&self, id: u32) -> &str {
        &self.all[id as usize]
    }
}

#[derive(Default, Clone)]
pub struct Index {
    files: HashMap<PathBuf, Box<[Sym]>>,
    strings: Strings,
    /// Distinct names (string numbers), sorted by text (completion by prefix).
    names: Vec<u32>,
}

fn entries(text: &str, syntax: &str) -> Vec<symbols::Symbol> {
    let mut v = symbols::extract_for_index(text, syntax);
    v.retain(|s| s.kind != Kind::Heading);
    v
}

impl Index {
    /// Indexes the source files of `root` (several threads), the folders of `first` before the
    /// rest (the repositories being worked on), at most `max_files` files: in a folder holding
    /// many repositories, only the ones far from the work are left out.
    #[cfg(test)]
    pub fn build(root: &Path, first: &[PathBuf], ex: &Exclusions, max_files: usize) -> Index {
        Index::build_in_steps(root, first, ex, max_files, |_| {})
    }

    /// Like `build`, and `first_done` gets the index of the `first` folders as soon as they are
    /// walked: go to definition works there long before a big folder is done. Half the processor
    /// cores at most, the interface keeps the others.
    pub fn build_in_steps(root: &Path, first: &[PathBuf], ex: &Exclusions, max_files: usize, first_done: impl FnOnce(Index)) -> Index {
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).div_ceil(2);
        let mut first_done = Some(first_done);
        let mut idx = Index::default();
        let count = AtomicUsize::new(0);
        let mut first: Vec<PathBuf> = first.iter().filter(|d| d.starts_with(root) && d.as_path() != root).cloned().collect();
        first.sort();
        first.dedup();
        // a folder inside another one of the list is walked with it
        let outer: Vec<PathBuf> = first.iter().filter(|d| !first.iter().any(|o| o != *d && d.starts_with(o))).cloned().collect();
        let walks = outer.iter().map(|d| (d.clone(), Vec::new())).chain([(root.to_path_buf(), outer.clone())]);
        for (dir, skip) in walks {
            if count.load(Ordering::Relaxed) >= max_files {
                break;
            }
            if dir.as_path() == root && !outer.is_empty() {
                if let Some(f) = first_done.take() {
                    let mut part = idx.clone();
                    part.rebuild_names();
                    f(part);
                }
            }
            // the symbols found by the walking threads are kept by this one: what stays in memory is
            // not scattered in the memory pools of short-lived threads, between freed file texts
            let (tx, rx) = mpsc::sync_channel::<(PathBuf, Vec<symbols::Symbol>)>(256);
            std::thread::scope(|s| {
                let count = &count;
                s.spawn(move || {
                    walk::walker(&dir, ex, skip).threads(threads).build_parallel().run(|| {
                        let tx = tx.clone();
                        Box::new(move |e| {
                            let Ok(e) = e else { return ignore::WalkState::Continue };
                            if count.load(Ordering::Relaxed) >= max_files {
                                return ignore::WalkState::Quit;
                            }
                            if !e.file_type().is_some_and(|t| t.is_file()) {
                                return ignore::WalkState::Continue;
                            }
                            let p = e.path();
                            let Some(syntax) = symbols::syntax_for(p).filter(|s| *s != "Markdown") else { return ignore::WalkState::Continue };
                            if e.metadata().map_or(true, |m| m.len() > MAX_FILE) {
                                return ignore::WalkState::Continue;
                            }
                            count.fetch_add(1, Ordering::Relaxed);
                            let Ok(text) = std::fs::read_to_string(p) else { return ignore::WalkState::Continue };
                            let v = entries(&text, syntax);
                            if !v.is_empty() && tx.send((p.to_path_buf(), v)).is_err() {
                                return ignore::WalkState::Quit;
                            }
                            ignore::WalkState::Continue
                        })
                    });
                });
                for (p, v) in rx {
                    idx.insert(p, v);
                }
            });
        }
        idx.files.shrink_to_fit();
        idx.rebuild_names();
        idx
    }

    fn insert(&mut self, path: PathBuf, v: Vec<symbols::Symbol>) {
        let syms: Box<[Sym]> = v
            .iter()
            .map(|s| Sym { name: self.strings.id(&s.name), detail: self.strings.id(&s.detail), line: s.line.min(u32::MAX as usize) as u32, kind: s.kind })
            .collect();
        self.files.insert(path, syms);
    }

    fn rebuild_names(&mut self) {
        let mut used = vec![false; self.strings.all.len()];
        for s in self.files.values().flat_map(|v| v.iter()) {
            used[s.name as usize] = true;
        }
        let mut n: Vec<u32> = (0..used.len() as u32).filter(|&i| used[i as usize]).collect();
        n.sort_unstable_by(|&a, &b| self.strings.get(a).cmp(self.strings.get(b)));
        self.names = n;
    }

    /// A file changed (saved): its symbols are extracted again.
    pub fn update(&mut self, path: &Path, text: &str) {
        let Some(syntax) = symbols::syntax_for(path) else { return };
        let v = entries(text, syntax);
        if v.is_empty() {
            self.files.remove(path);
        } else {
            self.insert(path.to_path_buf(), v);
        }
        self.rebuild_names();
    }

    fn entry(&self, s: &Sym) -> Entry {
        Entry { name: self.strings.get(s.name).to_string(), kind: s.kind, line: s.line as usize, detail: self.strings.get(s.detail).to_string() }
    }

    /// Definitions of `name`. "module.xml_id" also finds the record "xml_id", and "sale.order" the
    /// model declared with `_name = 'sale.order'`.
    pub fn lookup(&self, name: &str) -> Vec<(PathBuf, Entry)> {
        let short = name.rsplit('.').next().unwrap_or(name);
        let id = self.strings.ids.get(name).copied();
        let short_id = if short != name { self.strings.ids.get(short).copied() } else { None };
        if id.is_none() && short_id.is_none() {
            return Vec::new();
        }
        let mut out: Vec<(PathBuf, Entry)> = self
            .files
            .iter()
            .flat_map(|(p, v)| v.iter().map(move |e| (p, e)))
            .filter(|(_, e)| Some(e.name) == id || (e.kind == Kind::Record && Some(e.name) == short_id))
            .map(|(p, e)| (p.clone(), self.entry(e)))
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
        let start = self.names.partition_point(|&n| self.strings.get(n) < prefix);
        self.names[start..].iter().map(|&n| self.strings.get(n)).take_while(|n| n.starts_with(prefix)).take(limit).collect()
    }

    pub fn len(&self) -> usize {
        self.files.values().map(|v| v.len()).sum()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
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
        let mut idx = Index::build(&root, &[], &Exclusions::none(), usize::MAX);
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

    #[test]
    fn repositories_being_worked_on_first() {
        let root = std::env::temp_dir().join(format!("florm_index_first_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (dir, name) in [("a", "alpha"), ("b", "beta"), ("c", "gamma")] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join("m.py"), format!("def {name}():\n    pass\n")).unwrap();
        }
        // room for one file: the one of the repository given first
        let idx = Index::build(&root, &[root.join("b")], &Exclusions::none(), 1);
        assert_eq!(idx.file_count(), 1);
        assert_eq!(idx.lookup("beta").len(), 1);
        let all = Index::build(&root, &[root.join("b")], &Exclusions::new(&root, &["c/".into()]), usize::MAX);
        assert_eq!((all.lookup("alpha").len(), all.lookup("beta").len(), all.lookup("gamma").len()), (1, 1, 0));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `FLORM_BENCH_INDEX=<folder> cargo test --release bench_index -- --nocapture`
    #[test]
    fn bench_index() {
        let Ok(dir) = std::env::var("FLORM_BENCH_INDEX") else { return };
        let t = std::time::Instant::now();
        let idx = Index::build(Path::new(&dir), &[], &Exclusions::none(), usize::MAX);
        let rss = || std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split(' ').nth(1)?.parse::<usize>().ok()).map_or(0, |p| p * 4096 / 1_000_000);
        eprintln!("{} symboles, {} fichiers, {:?}, mémoire {} Mo", idx.len(), idx.file_count(), t.elapsed(), rss());
        crate::os::release_memory();
        eprintln!("après release_memory : {} Mo", rss());
        let t = std::time::Instant::now();
        let n = idx.lookup("sale.order").len();
        eprintln!("lookup sale.order : {n} résultat(s) en {:?}", t.elapsed());
    }
}
