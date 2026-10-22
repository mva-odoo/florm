//! Shows a file in the system file manager (Explorer, Finder, Nautilus/Dolphin…).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `Command::new` that does not open a console window on Windows (Florm is a GUI program:
/// without this, every git or linter call would flash a black window).
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Absolute path with links resolved; on Windows without the `\\?\` prefix that canonicalize
/// adds (it shows in the UI, and git refuses it as working directory, e.g. for `\\wsl.localhost\…`).
pub fn canon(p: &Path) -> PathBuf {
    dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// A WSL folder seen from Windows (`\\wsl$\…`, `\\wsl.localhost\…`).
pub fn is_wsl_unc(p: &Path) -> bool {
    let s = p.to_string_lossy().to_lowercase();
    s.starts_with(r"\\wsl$\") || s.starts_with(r"\\wsl.localhost\")
}

fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/version").is_ok_and(|v| v.to_lowercase().contains("microsoft"))
}

/// Runs the command in the background (the file manager may stay attached to it).
fn spawn(mut c: Command) -> std::io::Result<()> {
    let mut child = c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Opens the file manager on the folder containing `path`, with `path` selected when possible.
pub fn reveal(path: &Path) -> Result<(), String> {
    let path = canon(path);
    let dir = if path.is_dir() { path.clone() } else { path.parent().unwrap_or(&path).to_path_buf() };
    let r = if cfg!(target_os = "windows") {
        let mut c = Command::new("explorer");
        c.arg(format!("/select,{}", path.display()));
        spawn(c)
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg("-R").arg(&path);
        spawn(c)
    } else if is_wsl() {
        let win = command("wslpath")
            .arg("-w")
            .arg(&path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .ok_or("wslpath a échoué")?;
        let mut c = Command::new("explorer.exe");
        c.arg(format!("/select,{win}"));
        spawn(c)
    } else {
        // FileManager1 selects the file (Nautilus, Dolphin, Nemo, Thunar…); otherwise open the folder.
        let uri = format!("file://{}", path.display());
        let shown = command("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.FileManager1",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{uri}"))
            .arg("string:")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if shown {
            Ok(())
        } else {
            let mut c = command("xdg-open");
            c.arg(&dir);
            spawn(c)
        }
    };
    r.map_err(|e| format!("Impossible d'ouvrir le gestionnaire de fichiers : {e}"))
}

/// What a file chooser asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Folder,
    File,
    SaveAs,
}

/// File chooser of the desktop (GTK, KDE…) through the xdg portal. Blocks until the dialog is
/// closed; `Ok(None)` when cancelled.
#[cfg(target_os = "linux")]
pub fn portal_pick(kind: Pick, dir: &Path, name: Option<&str>) -> Result<Option<PathBuf>, String> {
    use std::collections::HashMap;
    use std::os::unix::ffi::OsStrExt;
    use zbus::zvariant::{OwnedObjectPath, Value};
    let e = |e: zbus::Error| format!("Sélecteur de fichiers du bureau indisponible : {e}");
    let conn = zbus::blocking::Connection::session().map_err(e)?;
    // the request object path is known in advance: listen to its answer before asking
    let token = format!("florm{}", std::process::id());
    let sender = conn.unique_name().map(|n| n.trim_start_matches(':').replace('.', "_")).unwrap_or_default();
    let handle = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let request = zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", handle.as_str(), "org.freedesktop.portal.Request").map_err(e)?;
    let mut answers = request.receive_signal("Response").map_err(e)?;
    let mut folder = dir.as_os_str().as_bytes().to_vec();
    folder.push(0);
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("handle_token", Value::from(token.as_str()));
    opts.insert("current_folder", Value::from(folder));
    let (method, title) = match kind {
        Pick::Folder => ("OpenFile", "Ouvrir un dossier"),
        Pick::File => ("OpenFile", "Ouvrir un fichier"),
        Pick::SaveAs => ("SaveFile", "Enregistrer sous"),
    };
    if kind == Pick::Folder {
        opts.insert("directory", Value::from(true));
    }
    if let Some(n) = name {
        opts.insert("current_name", Value::from(n));
    }
    let chooser = zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.FileChooser").map_err(e)?;
    let _: OwnedObjectPath = chooser.call(method, &("", title, opts)).map_err(e)?;
    let Some(msg) = answers.next() else { return Ok(None) };
    let body = msg.body();
    let (code, results): (u32, HashMap<String, Value>) = body.deserialize().map_err(e)?;
    if code != 0 {
        return Ok(None);
    }
    let first = match results.get("uris") {
        Some(Value::Array(a)) => a.iter().find_map(|v| if let Value::Str(s) = v { Some(s.to_string()) } else { None }),
        _ => None,
    };
    Ok(first.and_then(|u| file_uri_path(&u)))
}

/// `file:///home/a%20b` → `/home/a b`.
#[cfg(target_os = "linux")]
fn file_uri_path(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = uri.strip_prefix("file://")?.as_bytes();
    let mut out = Vec::with_capacity(rest.len());
    let mut i = 0;
    while i < rest.len() {
        let hex = rest.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (rest[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

/// Started from the Finder, the Dock or Spotlight, a macOS app only gets the PATH of launchd
/// (/usr/bin:/bin:/usr/sbin:/sbin): claude, gh, ruff… installed with Homebrew or in ~/.local/bin
/// are not found. Like VS Code, Florm takes the PATH of the login shell, plus the usual folders
/// when the shell does not answer. Started from a terminal, the PATH is already the right one.
/// To call before any thread is started.
#[cfg(target_os = "macos")]
pub fn macos_path() {
    if std::env::var_os("TERM").is_some() {
        return;
    }
    let home = crate::config::home();
    let usual = [home.join(".local/bin"), "/opt/homebrew/bin".into(), "/opt/homebrew/sbin".into(), "/usr/local/bin".into(), home.join(".cargo/bin")];
    let current = std::env::var_os("PATH").unwrap_or_default();
    let path = merge_path(login_shell_path().as_deref(), &current, &usual);
    std::env::set_var("PATH", path);
}

/// PATH of the login shell (`$SHELL -l -i`, its configuration read like in a terminal).
#[cfg(target_os = "macos")]
fn login_shell_path() -> Option<String> {
    use std::io::Read;
    // around the value: what the configuration prints (greeting, prompt codes) is left out
    const MARK: &str = "_FLORM_PATH_";
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    // "$PATH" is joined with ':' by sh, bash, zsh and fish alike
    let script = format!("printf '{MARK}%s{MARK}' \"$PATH\"");
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // a configuration that waits for something must not hold the start of Florm
    let start = std::time::Instant::now();
    while child.try_wait().ok()?.is_none() {
        if start.elapsed() > std::time::Duration::from_secs(3) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    out.split(MARK).nth(1).filter(|p| !p.is_empty()).map(str::to_string)
}

/// The folders of the shell's PATH first, then those of `current` and the existing `usual` ones
/// it lacks, each once.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn merge_path(shell: Option<&str>, current: &std::ffi::OsStr, usual: &[PathBuf]) -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = shell.map(|s| std::env::split_paths(s).filter(|d| d.is_absolute()).collect()).unwrap_or_default();
    for d in std::env::split_paths(current).chain(usual.iter().filter(|d| d.is_dir()).cloned()) {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    std::env::join_paths(dirs).unwrap_or_else(|_| current.to_os_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn file_uri_decoding() {
        assert_eq!(file_uri_path("file:///home/a%20b/%C3%A9t%C3%A9.txt"), Some(PathBuf::from("/home/a b/été.txt")));
        assert_eq!(file_uri_path("https://x"), None);
    }

    #[test]
    fn wsl_paths_from_windows() {
        assert!(is_wsl_unc(Path::new(r"\\wsl.localhost\Ubuntu\home\odoo\src")));
        assert!(is_wsl_unc(Path::new(r"\\WSL$\Ubuntu\home")));
        assert!(!is_wsl_unc(Path::new(r"C:\Users\me")));
        assert!(!is_wsl_unc(Path::new("/home/odoo")));
        // no "\\?\" prefix, and a missing path is kept as is
        assert_eq!(canon(Path::new("/nonexistent/x")), PathBuf::from("/nonexistent/x"));
    }

    #[cfg(unix)]
    #[test]
    fn path_of_the_login_shell() {
        let tmp = std::env::temp_dir();
        let usual = [tmp.clone(), PathBuf::from("/nonexistent/bin")];
        let p = merge_path(Some("/opt/homebrew/bin:relative:/usr/bin"), "/usr/bin:/bin".as_ref(), &usual);
        let want = format!("/opt/homebrew/bin:/usr/bin:/bin:{}", tmp.display());
        assert_eq!(p, std::ffi::OsString::from(want));
        // the shell did not answer: the folders of launchd, then the usual ones that exist
        let p = merge_path(None, "/usr/bin:/bin".as_ref(), &usual);
        assert_eq!(p, std::ffi::OsString::from(format!("/usr/bin:/bin:{}", tmp.display())));
    }
}
