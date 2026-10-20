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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_paths_from_windows() {
        assert!(is_wsl_unc(Path::new(r"\\wsl.localhost\Ubuntu\home\odoo\src")));
        assert!(is_wsl_unc(Path::new(r"\\WSL$\Ubuntu\home")));
        assert!(!is_wsl_unc(Path::new(r"C:\Users\me")));
        assert!(!is_wsl_unc(Path::new("/home/odoo")));
        // no "\\?\" prefix, and a missing path is kept as is
        assert_eq!(canon(Path::new("/nonexistent/x")), PathBuf::from("/nonexistent/x"));
    }
}
