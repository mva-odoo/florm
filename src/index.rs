//! Index of the project's symbols (like Sublime Text's): built in the background, updated when a
//! file is saved; used by "go to definition" and the completion when no language server answers.
//!
//! Millions of symbols in a big project (Odoo: 1.7 million): each name and detail is kept once
//! (`Strings`) and a symbol is 16 bytes.

use crate::symbols::{self, Kind};
use crate::walk::{self, Exclusions};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

const MAX_FILE: u64 = 1_500_000;
/// First bytes of the file of the index kept between two runs (and its version: change it when the
/// layout or the symbols change).
const MAGIC: &[u8; 8] = b"FLIDX\0\0\x01";

/// Size and modification time of a file: when both are the same, its symbols are the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    mtime: u128,
}

fn stamp_of(m: &std::fs::Metadata) -> Option<Stamp> {
    let mtime = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    Some(Stamp { len: m.len(), mtime })
}

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
    /// Every indexable file of the last walk (the ones without symbols too), with its stamp: what
    /// `save` writes and what a later build reuses when the file did not change.
    stamps: HashMap<PathBuf, Stamp>,
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
        Index::build_in_steps(root, first, ex, max_files, None, |_| {})
    }

    /// Like `build`, and `first_done` gets the index of the `first` folders as soon as they are
    /// walked: go to definition works there long before a big folder is done. Half the processor
    /// cores at most, the interface keeps the others. `cache`: the index of the last run; a file
    /// whose size and modification time did not change is not read again.
    pub fn build_in_steps(root: &Path, first: &[PathBuf], ex: &Exclusions, max_files: usize, cache: Option<&Index>, first_done: impl FnOnce(Index)) -> Index {
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
            let (tx, rx) = mpsc::sync_channel::<(PathBuf, Option<Stamp>, Option<Vec<symbols::Symbol>>)>(256);
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
                            let Ok(meta) = e.metadata() else { return ignore::WalkState::Continue };
                            if meta.len() > MAX_FILE {
                                return ignore::WalkState::Continue;
                            }
                            count.fetch_add(1, Ordering::Relaxed);
                            let stamp = stamp_of(&meta);
                            // the same size and date as in the last run: its symbols are kept, not read again
                            if let (Some(old), Some(s)) = (cache, stamp) {
                                if old.stamps.get(p) == Some(&s) {
                                    return if tx.send((p.to_path_buf(), stamp, None)).is_err() { ignore::WalkState::Quit } else { ignore::WalkState::Continue };
                                }
                            }
                            let Ok(text) = std::fs::read_to_string(p) else { return ignore::WalkState::Continue };
                            let v = entries(&text, syntax);
                            if tx.send((p.to_path_buf(), stamp, Some(v))).is_err() {
                                return ignore::WalkState::Quit;
                            }
                            ignore::WalkState::Continue
                        })
                    });
                });
                for (p, stamp, v) in rx {
                    match v {
                        Some(v) if !v.is_empty() => idx.insert(p.clone(), v),
                        Some(_) => {}
                        None => {
                            if let Some(old) = cache {
                                idx.adopt(old, &p);
                            }
                        }
                    }
                    if let Some(s) = stamp {
                        idx.stamps.insert(p, s);
                    }
                }
            });
        }
        idx.files.shrink_to_fit();
        idx.rebuild_names();
        idx
    }

    /// Takes the symbols of `path` from `from` (the index of the last run), numbers renumbered.
    fn adopt(&mut self, from: &Index, path: &Path) {
        let Some(syms) = from.files.get(path) else { return };
        let syms: Box<[Sym]> = syms
            .iter()
            .map(|s| Sym { name: self.strings.id(from.strings.get(s.name)), detail: self.strings.id(from.strings.get(s.detail)), line: s.line, kind: s.kind })
            .collect();
        self.files.insert(path.to_path_buf(), syms);
    }

    /// Writes the index to `path` (through a temporary file: a stop in the middle leaves the old one).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("tmp");
        let mut w = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        w.write_all(MAGIC)?;
        // a path that is not text cannot be kept: its file is read again next time
        let files: Vec<(&str, &PathBuf, &Stamp)> = self.stamps.iter().filter_map(|(p, s)| Some((p.to_str()?, p, s))).collect();
        w.write_all(&(files.len() as u64).to_le_bytes())?;
        for (name, p, s) in files {
            put_str(&mut w, name)?;
            w.write_all(&s.len.to_le_bytes())?;
            w.write_all(&s.mtime.to_le_bytes())?;
            let syms: &[Sym] = self.files.get(p).map_or(&[], |v| &v[..]);
            w.write_all(&(syms.len() as u32).to_le_bytes())?;
            for sym in syms {
                put_str(&mut w, self.strings.get(sym.name))?;
                put_str(&mut w, self.strings.get(sym.detail))?;
                w.write_all(&sym.line.to_le_bytes())?;
                w.write_all(&[kind_code(sym.kind)])?;
            }
        }
        w.flush()?;
        drop(w);
        std::fs::rename(&tmp, path)
    }

    /// The index written by `save`; None when there is none or it is not readable (then the
    /// folder is walked and read as before).
    pub fn load(path: &Path) -> Option<Index> {
        let data = std::fs::read(path).ok()?;
        let mut r = Reader { data: &data, pos: 0 };
        if r.take(MAGIC.len())? != MAGIC {
            return None;
        }
        let mut idx = Index::default();
        for _ in 0..r.u64()? {
            let p = PathBuf::from(r.str()?);
            let stamp = Stamp { len: r.u64()?, mtime: r.u128()? };
            let mut syms = Vec::new();
            for _ in 0..r.u32()? {
                let name = idx.strings.id(r.str()?);
                let detail = idx.strings.id(r.str()?);
                let line = r.u32()?;
                syms.push(Sym { name, detail, line, kind: kind_of(r.u8()?)? });
            }
            if !syms.is_empty() {
                idx.files.insert(p.clone(), syms.into_boxed_slice());
            }
            idx.stamps.insert(p, stamp);
        }
        idx.rebuild_names();
        Some(idx)
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

fn kind_code(k: Kind) -> u8 {
    match k {
        Kind::Class => 0,
        Kind::Function => 1,
        Kind::Method => 2,
        Kind::Record => 3,
        Kind::Heading => 4,
        Kind::Other => 5,
    }
}

fn kind_of(c: u8) -> Option<Kind> {
    [Kind::Class, Kind::Function, Kind::Method, Kind::Record, Kind::Heading, Kind::Other].get(c as usize).copied()
}

/// A text as its length (u32) and its bytes.
fn put_str(w: &mut impl Write, s: &str) -> std::io::Result<()> {
    w.write_all(&(s.len() as u32).to_le_bytes())?;
    w.write_all(s.as_bytes())
}

/// Reads what `Index::save` wrote; None at the end or on anything unexpected.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.data.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn u128(&mut self) -> Option<u128> {
        Some(u128::from_le_bytes(self.take(16)?.try_into().ok()?))
    }

    fn str(&mut self) -> Option<&'a str> {
        let n = self.u32()? as usize;
        std::str::from_utf8(self.take(n)?).ok()
    }
}

/// Where the index of the folder `root` is kept between two runs of Florm.
pub fn cache_path(root: &Path) -> PathBuf {
    // FNV-1a: the same name in every run (the standard hasher may change between Rust versions)
    let h = root.to_string_lossy().bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    crate::config::data_dir().join("index").join(format!("{h:016x}.bin"))
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

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn saved_index_is_reused_and_changed_files_read_again() {
        let dir = std::env::temp_dir().join(format!("florm-index-cache-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.py"), "class Foo:\n    pass\n").unwrap();
        std::fs::write(dir.join("sub/b.py"), "def bar():\n    pass\n").unwrap();
        let file = dir.join("cache.bin");

        let first = Index::build(&dir, &[], &Exclusions::none(), usize::MAX);
        first.save(&file).unwrap();
        let old = Index::load(&file).unwrap();
        assert_eq!(old.lookup("Foo").len(), 1);
        assert_eq!(old.file_count(), first.file_count());

        // a file of another length is read again; the others come from the cache
        std::fs::write(dir.join("a.py"), "class Quux:\n    pass\n").unwrap();
        let again = Index::build_in_steps(&dir, &[], &Exclusions::none(), usize::MAX, Some(&old), |_| {});
        assert_eq!(again.lookup("bar").len(), 1);
        assert_eq!(again.lookup("Quux").len(), 1);
        assert_eq!(again.lookup("Foo").len(), 0);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
