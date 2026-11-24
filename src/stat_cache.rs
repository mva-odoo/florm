//! What the UI asks of the disk at each frame (a folder still there, the branch of a repository),
//! read again every 2 s only: on `\\wsl$` or a network folder each read is slow.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(2);

#[derive(Clone)]
enum Value {
    Bool(bool),
    Text(Option<String>),
}

thread_local! {
    static CACHE: RefCell<HashMap<(u8, PathBuf), (Instant, Value)>> = RefCell::new(HashMap::new());
}

fn cached(kind: u8, p: &Path, read: impl FnOnce() -> Value) -> Value {
    let key = (kind, p.to_path_buf());
    if let Some(v) = CACHE.with(|c| c.borrow().get(&key).filter(|(t, _)| t.elapsed() < TTL).map(|(_, v)| v.clone())) {
        return v;
    }
    let v = read();
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        // the old entries go now and then
        if c.len() > 512 {
            c.retain(|_, (t, _)| t.elapsed() < TTL);
        }
        c.insert(key, (Instant::now(), v.clone()));
    });
    v
}

pub fn is_dir(p: &Path) -> bool {
    matches!(cached(0, p, || Value::Bool(p.is_dir())), Value::Bool(true))
}

pub fn exists(p: &Path) -> bool {
    matches!(cached(1, p, || Value::Bool(p.exists())), Value::Bool(true))
}

/// `git::head_branch`.
pub fn head_branch(p: &Path) -> Option<String> {
    match cached(2, p, || Value::Text(crate::git::head_branch(p))) {
        Value::Text(t) => t,
        Value::Bool(_) => None,
    }
}
