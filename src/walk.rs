//! Walking the project's files the same way everywhere (go to file, symbol index, find in
//! files): `.gitignore` applies, `.git` is skipped, and so are the user's exclusions.

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The `exclude` setting ("__pycache__/", "*.pyc"…, gitignore syntax) for a folder.
#[derive(Clone)]
pub struct Exclusions(Arc<Gitignore>);

impl Exclusions {
    pub fn new(root: &Path, patterns: &[String]) -> Exclusions {
        let mut b = GitignoreBuilder::new(root);
        for p in patterns.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
            let _ = b.add_line(None, p);
        }
        Exclusions(Arc::new(b.build().unwrap_or_else(|_| Gitignore::empty())))
    }

    #[cfg(test)]
    pub fn none() -> Exclusions {
        Exclusions(Arc::new(Gitignore::empty()))
    }

    pub fn excludes(&self, path: &Path, is_dir: bool) -> bool {
        self.0.matched(path, is_dir).is_ignore()
    }
}

/// Walker of `root` without `.git`, the exclusions and the folders of `skip` (already walked).
pub fn walker(root: &Path, ex: &Exclusions, skip: Vec<PathBuf>) -> WalkBuilder {
    let mut b = WalkBuilder::new(root);
    let ex = ex.clone();
    b.hidden(false).filter_entry(move |e| {
        let dir = e.file_type().is_some_and(|t| t.is_dir());
        e.file_name() != ".git" && !ex.excludes(e.path(), dir) && !(dir && skip.iter().any(|s| s == e.path()))
    });
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_prune_folders_and_files() {
        let root = std::env::temp_dir().join(format!("florm_walk_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["a/__pycache__", "b"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["a/x.py", "a/x.pyc", "a/__pycache__/x.cpython-312.pyc", "b/y.py"] {
            std::fs::write(root.join(f), "").unwrap();
        }
        let ex = Exclusions::new(&root, &["__pycache__/".into(), "*.pyc".into()]);
        let list = |skip: Vec<PathBuf>| {
            let mut v: Vec<String> = walker(&root, &ex, skip)
                .build()
                .flatten()
                .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
                .map(|e| e.path().strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"))
                .collect();
            v.sort();
            v
        };
        assert_eq!(list(Vec::new()), ["a/x.py", "b/y.py"]);
        assert_eq!(list(vec![root.join("b")]), ["a/x.py"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
