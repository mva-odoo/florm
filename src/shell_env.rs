//! The environment of the user's login shell (Linux, macOS), as in Zed and VS Code: Florm started
//! from the desktop does not get what the shell's configuration sets (pyenv, nvm, a virtual
//! environment, ~/bin…), so the programs it runs (the program debugged, linters, language
//! servers, tests) would not find the same Python or node as in a terminal. Read once at
//! startup on a thread, then given to every command (`os::command`).

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::OnceLock;

static ENV: OnceLock<HashMap<String, String>> = OnceLock::new();

/// What separates the output of the shell's configuration (a greeting) from the environment.
const MARK: &str = "__FLORM_SHELL_ENV__";

/// Reads the environment of `$SHELL -l -i` (at most 5 seconds). On a thread.
pub fn load() {
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
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if start.elapsed() < std::time::Duration::from_secs(5) => std::thread::sleep(std::time::Duration::from_millis(20)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
        }
        let mut out = String::new();
        if let Some(mut so) = child.stdout.take() {
            use std::io::Read;
            let _ = so.read_to_string(&mut out);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
