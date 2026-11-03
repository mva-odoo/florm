//! Plugins: a folder with a `plugin.toml` declaring commands and hooks.
//! Each command is any executable run through the shell: it receives the
//! selection (or buffer) on stdin and context in FLORM_* environment variables,
//! and what it prints on stdout is put back into the editor.

use crate::terminal::shell_command;
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    #[default]
    Selection,
    Buffer,
    None,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    ReplaceSelection,
    ReplaceBuffer,
    Insert,
    #[default]
    Panel,
    NewTab,
    None,
}

#[derive(Deserialize, Clone, Debug)]
pub struct CommandDef {
    pub title: String,
    pub run: String,
    #[serde(default)]
    pub input: Input,
    #[serde(default)]
    pub output: Output,
    #[serde(default)]
    pub keys: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct HookDef {
    /// Only "on_save" for now.
    pub event: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    pub run: String,
    /// Reload the file after the hook (formatters).
    #[serde(default)]
    pub reload: bool,
}

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    commands: Vec<CommandDef>,
    #[serde(default)]
    hooks: Vec<HookDef>,
}

#[derive(Clone, Debug)]
pub struct Plugin {
    pub name: String,
    pub dir: PathBuf,
    pub commands: Vec<CommandDef>,
    pub hooks: Vec<HookDef>,
}

impl Plugin {
    pub fn hooks_for(&self, event: &str, path: &Path) -> impl Iterator<Item = &HookDef> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let event = event.to_string();
        self.hooks.iter().filter(move |h| h.event == event && (h.extensions.is_empty() || h.extensions.contains(&ext)))
    }
}

pub fn load_all(dirs: &[PathBuf]) -> (Vec<Plugin>, Vec<String>) {
    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    for base in dirs {
        let Ok(rd) = std::fs::read_dir(base) else { continue };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for dir in entries {
            let mf = dir.join("plugin.toml");
            if !mf.is_file() {
                continue;
            }
            let parsed = std::fs::read_to_string(&mf)
                .map_err(|e| e.to_string())
                .and_then(|s| toml::from_str::<Manifest>(&s).map_err(|e| e.to_string()));
            match parsed {
                Ok(m) => plugins.push(Plugin {
                    name: if m.name.is_empty() {
                        dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                    } else {
                        m.name
                    },
                    dir,
                    commands: m.commands,
                    hooks: m.hooks,
                }),
                Err(e) => errors.push(format!("{} : {e}", mf.display())),
            }
        }
    }
    (plugins, errors)
}

/// Whether `dir` holds a plugin (a folder with a `plugin.toml`).
pub fn has_plugins(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| rd.flatten().any(|e| e.path().join("plugin.toml").is_file()))
}

/// The folders whose plugins (`.florm/plugins`) may run: a cloned repository could otherwise run
/// its commands at the first file saved. Kept apart from the settings, which can be shared.
fn trusted_path() -> PathBuf {
    crate::config::config_dir().join("trusted_folders")
}

pub fn is_trusted(root: &Path) -> bool {
    let root = crate::os::canon(root);
    std::fs::read_to_string(trusted_path()).is_ok_and(|t| t.lines().any(|l| Path::new(l) == root))
}

/// Trust for the plugins of the shared configuration, kept apart: the repository can be pushed to
/// by others, so trust is given to what is in it now (its fingerprint), and a pull asks again.
fn shared_trusted_path() -> PathBuf {
    crate::config::config_dir().join("trusted_shared_plugins")
}

/// Fingerprint of the files of a plugins folder (names and contents, `.git` and links left out).
pub fn fingerprint(dir: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let path = e.path();
            let ty = e.file_type()?;
            if ty.is_symlink() || e.file_name() == ".git" {
                continue;
            }
            if ty.is_dir() {
                collect(&path, out)?;
            } else if ty.is_file() {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    collect(dir, &mut files).ok()?;
    files.sort();
    let mut h = Sha256::new();
    for f in files {
        let rel = f.strip_prefix(dir).ok()?.to_string_lossy().into_owned();
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(std::fs::read(&f).ok()?);
        h.update([0]);
    }
    Some(format!("{:x}", h.finalize()))
}

fn shared_trust_line(dir: &Path) -> Option<String> {
    Some(format!("{}\t{}", crate::os::canon(dir).to_string_lossy(), fingerprint(dir)?))
}

pub fn is_shared_trusted(dir: &Path) -> bool {
    let Some(line) = shared_trust_line(dir) else { return false };
    std::fs::read_to_string(shared_trusted_path()).is_ok_and(|t| t.lines().any(|l| l == line))
}

/// Trusts the plugins of the shared folder as they are now.
pub fn trust_shared(dir: &Path) -> std::io::Result<()> {
    let line = shared_trust_line(dir).ok_or_else(|| std::io::Error::other("plugins illisibles"))?;
    let canon = crate::os::canon(dir).to_string_lossy().into_owned();
    // the old fingerprint of this folder goes
    let mut lines: Vec<String> = std::fs::read_to_string(shared_trusted_path())
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with(&format!("{canon}\t")))
        .map(String::from)
        .collect();
    lines.push(line);
    std::fs::create_dir_all(crate::config::config_dir())?;
    std::fs::write(shared_trusted_path(), lines.join("\n") + "\n")
}

pub fn trust(root: &Path) -> std::io::Result<()> {
    let mut text = std::fs::read_to_string(trusted_path()).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&crate::os::canon(root).to_string_lossy());
    text.push('\n');
    std::fs::create_dir_all(crate::config::config_dir())?;
    std::fs::write(trusted_path(), text)
}

/// Runs a plugin command (always through `sh -c` / `cmd /C`) inside the plugin folder.
pub fn exec(dir: &Path, run: &str, input: &str, env: &[(String, String)]) -> Result<String, String> {
    let mut c = shell_command(run, None);
    c.current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    c.env("FLORM_PLUGIN_DIR", dir);
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = c.spawn().map_err(|e| format!("{run} : {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_string();
        // Write from another thread so a plugin producing lots of output cannot deadlock us.
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let so = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(so)
    } else {
        Err(tf!("{run} a échoué :\n{so}{}", "{run} failed:\n{so}{}", String::from_utf8_lossy(&out.stderr)))
    }
}

const EXAMPLE_TOML: &str = include_str!("../plugins/examples/plugin.toml");
const EXAMPLE_REVERSE: &str = include_str!("../plugins/examples/reverse_lines.py");
const EXAMPLE_README: &str = include_str!("../plugins/README.md");

/// Installs the bundled example plugin on first run.
pub fn install_examples(plugins_dir: &Path) {
    if plugins_dir.exists() {
        return;
    }
    let ex = plugins_dir.join("examples");
    if std::fs::create_dir_all(&ex).is_ok() {
        let _ = std::fs::write(ex.join("plugin.toml"), EXAMPLE_TOML);
        let _ = std::fs::write(ex.join("reverse_lines.py"), EXAMPLE_REVERSE);
        let _ = std::fs::write(plugins_dir.join("README.md"), EXAMPLE_README);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_plugins_need_trust() {
        let _lock = crate::config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = std::env::temp_dir().join(format!("florm_trust_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (conf, root) = (base.join("conf"), base.join("proj"));
        std::fs::create_dir_all(root.join(".florm/plugins/p")).unwrap();
        std::env::set_var("FLORM_CONFIG", &conf);
        assert!(!has_plugins(&root.join(".florm/plugins")));
        std::fs::write(root.join(".florm/plugins/p/plugin.toml"), "name = \"p\"\n").unwrap();
        assert!(has_plugins(&root.join(".florm/plugins")));
        assert!(!is_trusted(&root));
        trust(&root).unwrap();
        assert!(is_trusted(&root) && !is_trusted(&base));
        std::env::remove_var("FLORM_CONFIG");
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;

    #[test]
    fn a_change_of_the_plugins_changes_the_fingerprint() {
        let dir = std::env::temp_dir().join(format!("florm_fp_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("p")).unwrap();
        std::fs::write(dir.join("p/plugin.toml"), "name = \"p\"\n").unwrap();
        let before = fingerprint(&dir).unwrap();
        assert_eq!(before, fingerprint(&dir).unwrap());
        std::fs::write(dir.join("p/plugin.toml"), "name = \"p\"\nrun = \"rm -rf ~\"\n").unwrap();
        assert_ne!(before, fingerprint(&dir).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
