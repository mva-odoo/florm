//! Local history: a copy of a file at each save, kept apart from git (in the data folder), to
//! compare with or go back to; and the comparison of two files or folders (`git diff --no-index`,
//! which works outside any repository).

use std::path::{Path, PathBuf};

/// Versions kept per file, and the largest file kept.
const KEEP: usize = 50;
const MAX_SIZE: u64 = 2_000_000;

fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

fn dir_of(path: &Path) -> PathBuf {
    crate::config::data_dir().join("history").join(format!("{:016x}", hash(&path.to_string_lossy())))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The versions of `path`, newest first: (when, in ms since 1970; the copy).
pub fn versions(path: &Path) -> Vec<(u64, PathBuf)> {
    versions_in(&dir_of(path))
}

fn versions_in(dir: &Path) -> Vec<(u64, PathBuf)> {
    let mut v: Vec<(u64, PathBuf)> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| Some((e.file_name().to_str()?.strip_suffix(".txt")?.parse().ok()?, e.path()))).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v
}

/// Keeps the content of `path` on disk as a version, unless it is the last one kept (to call
/// before saving, for the version replaced, and after).
pub fn keep(path: &Path) {
    keep_in(&dir_of(path), path, now_ms());
}

fn keep_in(dir: &Path, path: &Path, at: u64) {
    if std::fs::metadata(path).map_or(true, |m| !m.is_file() || m.len() > MAX_SIZE) {
        return;
    }
    let Ok(text) = std::fs::read(path) else { return };
    let old = versions_in(dir);
    if old.first().and_then(|(_, p)| std::fs::read(p).ok()).is_some_and(|last| last == text) {
        return;
    }
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    // the same millisecond twice (before and after a save): the next one
    let at = old.first().map_or(at, |(t, _)| at.max(t + 1));
    let _ = std::fs::write(dir.join(format!("{at}.txt")), &text);
    let _ = std::fs::write(dir.join("path"), path.to_string_lossy().as_bytes());
    for (_, p) in old.iter().skip(KEEP - 1) {
        let _ = std::fs::remove_file(p);
    }
}

/// Unified diff of two files or two folders, the paths shown as `la` and `lb`; empty when they
/// are the same.
pub fn diff(a: &Path, b: &Path, la: &str, lb: &str) -> Result<String, String> {
    let out = crate::os::command("git")
        .args(["diff", "--no-index", "--no-color", "--no-ext-diff", "--"])
        .arg(a)
        .arg(b)
        .output()
        .map_err(|_| t!("La commande git est introuvable (elle sert à comparer).", "The git command was not found (it is used to compare).").to_string())?;
    // 1: they differ
    if !matches!(out.status.code(), Some(0 | 1)) {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let strip = |p: &Path| p.to_string_lossy().trim_start_matches('/').replace('\\', "/");
    Ok(text.replace(&strip(a), la).replace(&strip(b), lb))
}

/// A text written in a file of its own (the editor or the clipboard, to compare it).
pub fn temp_file(name: &str, text: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("florm-compare-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let p = dir.join(name);
    std::fs::write(&p, text).map_err(|e| e.to_string())?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_kept_once_newest_first() {
        let base = std::env::temp_dir().join(format!("florm_local_history_{}", std::process::id()));
        let (dir, file) = (base.join("h"), base.join("a.py"));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(&file, "x = 1\n").unwrap();
        keep_in(&dir, &file, 10);
        keep_in(&dir, &file, 20);
        std::fs::write(&file, "x = 2\n").unwrap();
        keep_in(&dir, &file, 20);
        let v = versions_in(&dir);
        assert_eq!(v.iter().map(|x| x.0).collect::<Vec<_>>(), [20, 10]);
        assert_eq!(std::fs::read_to_string(&v[1].1).unwrap(), "x = 1\n");
        let d = diff(&v[1].1, &file, "avant", "maintenant").unwrap();
        assert!(d.contains("-x = 1") && d.contains("+x = 2") && d.contains("avant"), "{d}");
        assert!(diff(&file, &file, "a", "b").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }
}
