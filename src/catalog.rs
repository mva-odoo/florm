//! The catalogue of extensions and language packs: those shipped with Florm (`assets/extensions`),
//! those of the shared configuration (its `extensions/` and `packs/`), and those of the catalogue
//! repositories of the settings (`extension_catalogs`: "owner/repo" or a git address), each
//! cloned in Florm's data folder. A catalogue is a folder: one folder per extension (with its
//! extension.toml), and `packs/*.toml`. Installing copies into `~/.config/florm/extensions/` or
//! `~/.config/florm/packs/`.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Extension,
    Pack,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    pub name: String,
    pub description: String,
    pub version: String,
    /// Where it comes from: « Florm », « configuration partagée », « owner/repo ».
    pub source: String,
    /// Shipped with Florm (no code from elsewhere).
    pub builtin: bool,
    /// Its folder (extension) or file (pack) in the catalogue.
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Missing,
    Installed,
    /// Installed, but the catalogue has another version.
    Differs,
}

/// The files of the extensions shipped with Florm.
const BUILTIN: &[(&str, &str)] = &[
    ("odoo/extension.toml", include_str!("../assets/extensions/odoo/extension.toml")),
    ("odoo/main.py", include_str!("../assets/extensions/odoo/main.py")),
    ("odoo/odoo_lib.py", include_str!("../assets/extensions/odoo/odoo_lib.py")),
];

/// The shipped catalogue, written in the data folder (again when Florm changed it).
pub fn builtin_dir() -> PathBuf {
    let d = crate::config::data_dir().join("catalog").join("florm");
    for (rel, text) in BUILTIN {
        let p = d.join(rel);
        if std::fs::read_to_string(&p).ok().as_deref() != Some(*text) {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&p, text);
        }
    }
    d
}

/// "owner/repo" or a git address: (address to clone, "owner/repo" on GitHub).
pub fn resolve(spec: &str) -> Option<(String, Option<String>)> {
    let s = spec.trim().trim_end_matches('/');
    if s.contains("://") || s.starts_with("git@") {
        return crate::shared::resolve(s);
    }
    let mut parts = s.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(o), Some(r), None) if !o.is_empty() && !r.is_empty() => crate::shared::resolve(s),
        _ => None,
    }
}

/// The folder a catalogue repository is cloned in.
pub fn clone_dir(spec: &str) -> PathBuf {
    let name: String = spec.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect();
    crate::config::data_dir().join("catalog").join("repos").join(name)
}

/// Clones a catalogue repository again (on a thread): a fresh copy, no git run in a folder of
/// someone else's configuration.
pub fn fetch(spec: &str) -> Result<(), String> {
    let (url, slug) = resolve(spec).ok_or_else(|| crate::tf!("« {spec} » : owner/repo ou une adresse git", "“{spec}”: owner/repo or a git address"))?;
    let dir = clone_dir(spec);
    let tmp = dir.with_extension("new");
    let _ = std::fs::remove_dir_all(&tmp);
    crate::shared::clone(&url, slug.as_deref(), &tmp).map_err(|e| format!("{spec} : {e}"))?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&tmp, &dir).map_err(|e| format!("{spec} : {e}"))
}

#[derive(Deserialize)]
struct Head {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    version: String,
}

/// The entries of a catalogue folder.
pub fn scan(dir: &Path, source: &str, builtin: bool) -> Vec<Entry> {
    let mut out = Vec::new();
    let head = |p: &Path| std::fs::read_to_string(p).ok().and_then(|t| toml::from_str::<Head>(&t).ok());
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.join("extension.toml").is_file()).collect();
    dirs.sort();
    for d in dirs {
        if let Some(h) = head(&d.join("extension.toml")) {
            let name = if h.name.trim().is_empty() { d.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string() } else { h.name };
            out.push(Entry { kind: Kind::Extension, name, description: h.description, version: h.version, source: source.to_string(), builtin, path: d });
        }
    }
    let mut packs: Vec<PathBuf> = std::fs::read_dir(dir.join("packs")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect();
    packs.sort();
    for p in packs {
        if let Some(h) = head(&p) {
            let name = if h.name.trim().is_empty() { p.file_stem().and_then(|n| n.to_str()).unwrap_or("?").to_string() } else { h.name };
            out.push(Entry { kind: Kind::Pack, name, description: h.description, version: h.version, source: source.to_string(), builtin, path: p });
        }
    }
    out
}

/// All the entries: Florm's, the shared configuration's, the catalogue repositories' (cloned).
pub fn all(catalogs: &[String]) -> Vec<Entry> {
    let mut out = scan(&builtin_dir(), "Florm", true);
    if let Some(d) = crate::shared::dir() {
        out.extend(scan(&d.join("extensions"), crate::t!("configuration partagée", "shared configuration"), false));
        // its packs/ are next to extensions/, not inside
        out.extend(scan(&d, crate::t!("configuration partagée", "shared configuration"), false).into_iter().filter(|e| e.kind == Kind::Pack));
    }
    for c in catalogs {
        out.extend(scan(&clone_dir(c), c, false));
    }
    out
}

/// Where an entry is installed.
pub fn target(e: &Entry) -> PathBuf {
    let name = e.path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    match e.kind {
        Kind::Extension => crate::ext::dir().join(name),
        Kind::Pack => crate::packs::user_dir().join(name),
    }
}

/// The files of a folder, relative to it (no .git, no state the extension keeps).
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        for e in std::fs::read_dir(dir.join(&rel)).into_iter().flatten().flatten() {
            let name = e.file_name();
            let n = name.to_string_lossy();
            if n.starts_with('.') || n == "__pycache__" || n == "state.json" {
                continue;
            }
            let r = rel.join(&name);
            if e.path().is_dir() {
                stack.push(r);
            } else {
                out.push(r);
            }
        }
    }
    out.sort();
    out
}

pub fn state(e: &Entry) -> State {
    let t = target(e);
    let same = |a: &Path, b: &Path| std::fs::read(a).ok().is_some_and(|x| std::fs::read(b).ok().is_some_and(|y| x == y));
    match e.kind {
        Kind::Pack if !t.is_file() => State::Missing,
        Kind::Pack => if same(&e.path, &t) { State::Installed } else { State::Differs },
        Kind::Extension if !t.join("extension.toml").is_file() => State::Missing,
        Kind::Extension => {
            if files(&e.path).iter().all(|r| same(&e.path.join(r), &t.join(r))) {
                State::Installed
            } else {
                State::Differs
            }
        }
    }
}

/// Installs (or updates) an entry: its files copied over (what the extension keeps, like its
/// state.json, stays).
pub fn install(e: &Entry) -> Result<PathBuf, String> {
    let t = target(e);
    match e.kind {
        Kind::Pack => {
            std::fs::create_dir_all(t.parent().unwrap_or(&t)).map_err(|x| x.to_string())?;
            std::fs::copy(&e.path, &t).map_err(|x| x.to_string())?;
        }
        Kind::Extension => {
            for r in files(&e.path) {
                let to = t.join(&r);
                std::fs::create_dir_all(to.parent().unwrap_or(&t)).map_err(|x| x.to_string())?;
                std::fs::copy(e.path.join(&r), &to).map_err(|x| format!("{} : {x}", to.display()))?;
            }
        }
    }
    Ok(t)
}

/// Removes what an entry installed.
pub fn uninstall(e: &Entry) -> Result<(), String> {
    let t = target(e);
    match e.kind {
        Kind::Pack => std::fs::remove_file(&t),
        Kind::Extension => std::fs::remove_dir_all(&t),
    }
    .map_err(|x| x.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Odoo extension is in Florm's catalogue; installed, then the same, then changed.
    #[test]
    fn install_from_the_catalogue() {
        let dir = std::env::temp_dir().join(format!("florm_catalog_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _lock = crate::config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let all = all(&[]);
        let odoo = all.iter().find(|e| e.name == "Odoo").expect("odoo").clone();
        assert!(odoo.builtin && odoo.kind == Kind::Extension && odoo.version == "1.0");
        assert_eq!(state(&odoo), State::Missing);
        let t = install(&odoo).unwrap();
        assert!(t.join("odoo_lib.py").is_file());
        assert_eq!(state(&odoo), State::Installed);
        // its own state is not a difference; a changed file is
        std::fs::write(t.join("state.json"), "{}").unwrap();
        assert_eq!(state(&odoo), State::Installed);
        std::fs::write(t.join("main.py"), "changed").unwrap();
        assert_eq!(state(&odoo), State::Differs);
        install(&odoo).unwrap();
        assert!(t.join("state.json").is_file() && state(&odoo) == State::Installed);
        uninstall(&odoo).unwrap();
        assert_eq!(state(&odoo), State::Missing);
        // a catalogue folder with a pack
        let cat = dir.join("cat");
        std::fs::create_dir_all(cat.join("packs")).unwrap();
        std::fs::write(cat.join("packs").join("nim.toml"), "name = \"Nim\"\ndescription = \"x\"\next = [\"nim\"]\n").unwrap();
        let e = scan(&cat, "me/cat", false);
        assert_eq!((e.len(), e[0].kind, e[0].name.as_str()), (1, Kind::Pack, "Nim"));
        install(&e[0]).unwrap();
        assert!(crate::packs::user_dir().join("nim.toml").is_file());
        assert!(resolve("me/cat").is_some() && resolve("me").is_none() && resolve("a/b/c").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
