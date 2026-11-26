//! The environment of the user's login shell (Linux, macOS), as in Zed and VS Code: Florm started
//! from the desktop does not get what the shell's configuration sets (pyenv, nvm, a virtual
//! environment, ~/bin…), so the programs it runs (the program debugged, linters, language
//! servers, tests) would not find the same Python or node as in a terminal. Read once at
//! startup on a thread, then given to every command (`os::command`).

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

static ENV: OnceLock<HashMap<String, String>> = OnceLock::new();

/// 0: not asked (the tests), 1: being read, 2: done (read or not).
static STATE: AtomicU8 = AtomicU8::new(0);

/// The environment of the shell is known (or will never be): what was looked for in the PATH
/// before can be looked for again.
pub fn settled() -> bool {
    STATE.load(Ordering::SeqCst) != 1
}

/// Waits (at most 6 seconds) for the environment of the shell, before looking for programs whose
/// answer is kept (the tools of the languages). On a thread, never the UI's.
pub fn wait() {
    let t = std::time::Instant::now();
    while !settled() && t.elapsed() < std::time::Duration::from_secs(6) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// What separates the output of the shell's configuration (a greeting) from the environment.
const MARK: &str = "__FLORM_SHELL_ENV__";

/// Reads the environment of `$SHELL -l -i` (at most 5 seconds). On a thread.
pub fn load() {
    STATE.store(1, Ordering::SeqCst);
    read();
    STATE.store(2, Ordering::SeqCst);
    // the linters looked for meanwhile (with Florm's own PATH) are looked for again
    crate::lint::reset_linter_cache();
}

fn read() {
    #[cfg(unix)]
    {
        let Some(shell) = std::env::var_os("SHELL") else { return };
        let Ok(mut child) = std::process::Command::new(&shell)
            .args(["-l", "-i", "-c", &format!("echo {MARK}; env")])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            return;
        };
        // read while it writes (a long environment would fill the pipe and block the shell)
        let (tx, rx) = std::sync::mpsc::channel();
        if let Some(mut so) = child.stdout.take() {
            std::thread::spawn(move || {
                use std::io::Read;
                let mut buf = Vec::new();
                let _ = so.read_to_end(&mut buf);
                let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
            });
        }
        let out = match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(out) => out,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        };
        let _ = child.wait();
        let env = parse(&out);
        if env.contains_key("PATH") {
            let _ = ENV.set(env);
        }
    }
}

/// The variables printed by `env` after the mark (a value over several lines is left out).
fn parse(out: &str) -> HashMap<String, String> {
    let Some((_, after)) = out.split_once(MARK) else { return HashMap::new() };
    after
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The PATH of the shell once read, else Florm's own.
pub fn path() -> Option<OsString> {
    ENV.get().and_then(|e| e.get("PATH")).map(OsString::from).or_else(|| std::env::var_os("PATH"))
}

/// The variables of the shell Florm does not have (PYENV_ROOT, NVM_DIR, VIRTUAL_ENV…), for the
/// commands it runs. Not those of the shell itself.
pub fn missing() -> Vec<(String, String)> {
    const SHELL_OWN: &[&str] = &["PATH", "PWD", "OLDPWD", "SHLVL", "_", "TERM", "COLUMNS", "LINES"];
    ENV.get()
        .map(|e| e.iter().filter(|(k, _)| !SHELL_OWN.contains(&k.as_str()) && std::env::var_os(k).is_none()).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

/// The program behind a pyenv shim (`~/.pyenv/shims/flake8` → `~/.pyenv/versions/3.12.12/bin/flake8`),
/// for the tools Florm runs again and again (linters, its Python): a shim costs ~160 ms per run.
/// The global version of pyenv (PYENV_VERSION, else `~/.pyenv/version`); any other path as is.
pub fn unshim(p: std::path::PathBuf) -> std::path::PathBuf {
    let (Some(dir), Some(name)) = (p.parent(), p.file_name()) else { return p };
    if dir.file_name().is_none_or(|n| n != "shims") || !dir.parent().is_some_and(|r| r.join("versions").is_dir()) {
        return p;
    }
    let root = dir.parent().unwrap();
    let var = |k: &str| ENV.get().and_then(|e| e.get(k).cloned()).or_else(|| std::env::var(k).ok());
    let wanted = var("PYENV_VERSION").unwrap_or_else(|| std::fs::read_to_string(root.join("version")).unwrap_or_default());
    let installed: Vec<String> = std::fs::read_dir(root.join("versions")).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str().map(String::from)).collect();
    // « 3.12 » is the newest 3.12.x installed
    let key = |v: &str| v.split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    for v in wanted.split(|c: char| c == ':' || c.is_whitespace()).filter(|v| !v.is_empty() && *v != "system") {
        let best = installed.iter().filter(|i| *i == v || i.starts_with(&format!("{v}."))).max_by_key(|i| key(i));
        if let Some(bin) = best.map(|b| root.join("versions").join(b).join("bin").join(name)).filter(|b| b.is_file()) {
            return bin;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pyenv shim: the program of the global version (« 3.12 » → the newest 3.12.x).
    #[test]
    fn the_program_behind_a_shim() {
        let root = std::env::temp_dir().join(format!("florm_pyenv_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for v in ["3.12.3", "3.12.12", "3.11.9"] {
            std::fs::create_dir_all(root.join("versions").join(v).join("bin")).unwrap();
            std::fs::write(root.join("versions").join(v).join("bin").join("flake8"), "").unwrap();
        }
        std::fs::create_dir_all(root.join("shims")).unwrap();
        std::fs::write(root.join("version"), "3.12\n").unwrap();
        let shim = root.join("shims").join("flake8");
        if std::env::var_os("PYENV_VERSION").is_none() {
            assert_eq!(unshim(shim.clone()), root.join("versions/3.12.12/bin/flake8"));
        }
        assert_eq!(unshim(root.join("shims").join("nothing")), root.join("shims").join("nothing"));
        assert_eq!(unshim("/usr/bin/flake8".into()), std::path::PathBuf::from("/usr/bin/flake8"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_environment_after_the_greeting() {
        let out = "Welcome to fish\nPATH=/x\n__FLORM_SHELL_ENV__\nPATH=/home/u/.pyenv/shims:/usr/bin\nPYENV_ROOT=/home/u/.pyenv\nnot a variable\n";
        let e = parse(out);
        assert_eq!(e.get("PATH").map(String::as_str), Some("/home/u/.pyenv/shims:/usr/bin"));
        assert_eq!(e.get("PYENV_ROOT").map(String::as_str), Some("/home/u/.pyenv"));
        assert_eq!(e.len(), 2);
        assert!(parse("no mark").is_empty());
    }
}
