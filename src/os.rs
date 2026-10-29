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

/// Where the window buttons (─ □ ✕) of Florm's title bar are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controls {
    /// macOS: round buttons at the left, as the system draws them.
    Mac,
    /// Windows and Linux desktops: square buttons at the right.
    Side,
    /// Tiling window managers (Hyprland, Sway, i3, niri): the keyboard does it, no buttons.
    None,
}

pub fn window_controls() -> Controls {
    if cfg!(target_os = "macos") {
        Controls::Mac
    } else if ["HYPRLAND_INSTANCE_SIGNATURE", "SWAYSOCK", "I3SOCK", "NIRI_SOCKET"].iter().any(|v| std::env::var_os(v).is_some()) {
        Controls::None
    } else {
        Controls::Side
    }
}

/// Florm draws a title bar: there is something in it (buttons, or menus).
pub fn draws_title_bar(show_menu_bar: bool) -> bool {
    window_controls() != Controls::None || show_menu_bar
}

/// Gives back to the system the memory freed after a big job (the symbol index: its threads read
/// hundreds of thousands of files): glibc keeps it in its pools otherwise.
pub fn release_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // SAFETY: only returns free pages of the allocator's pools to the system.
        unsafe {
            malloc_trim(0);
        }
    }
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
            .ok_or(t!("wslpath a échoué", "wslpath failed"))?;
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
    r.map_err(|e| tf!("Impossible d'ouvrir le gestionnaire de fichiers : {e}", "Cannot open the file manager: {e}"))
}

/// `a` and `b` are the same entry of the disk (`readme` and `README` on a disk that ignores case).
pub fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (a.symlink_metadata(), b.symlink_metadata()) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        a.symlink_metadata().is_ok() && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
}

/// Moves `path` (file or folder) to the trash of the system: it can be got back from there.
pub fn trash(path: &Path) -> Result<(), String> {
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    let r = if cfg!(target_os = "windows") {
        // the Recycle Bin through .NET (no extra dependency); the path goes in a variable, never
        // inside the script
        let script = "$p = $env:FLORM_TRASH; Add-Type -AssemblyName Microsoft.VisualBasic; \
            if (Test-Path -LiteralPath $p -PathType Container) { [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($p, 'OnlyErrorDialogs', 'SendToRecycleBin') } \
            else { [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p, 'OnlyErrorDialogs', 'SendToRecycleBin') }";
        run_ok(command("powershell").args(["-NoProfile", "-NonInteractive", "-Command", script]).env("FLORM_TRASH", &path))
    } else if cfg!(target_os = "macos") {
        let script = "on run argv\ntell application \"Finder\" to delete (POSIX file (item 1 of argv) as alias)\nend run";
        run_ok(command("osascript").args(["-e", script]).arg(&path))
    } else {
        // the home trash of the freedesktop specification (what the file managers show);
        // on another disk, gio (GLib) finds the trash of that disk
        #[cfg(unix)]
        let r = freedesktop_trash(&path);
        #[cfg(not(unix))]
        let r = Err(String::new());
        r.or_else(|_| run_ok(command("gio").arg("trash").arg(&path)))
    };
    r.map_err(|e| if e.is_empty() { t!("pas de corbeille disponible", "no trash available").to_string() } else { e })
}

/// Runs the command; its error output when it fails.
fn run_ok(c: &mut Command) -> Result<(), String> {
    let o = c.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

#[cfg(unix)]
fn freedesktop_trash(path: &Path) -> Result<(), String> {
    use std::io::Write;
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| crate::config::home().join(".local/share")).join("Trash");
    let (files, info) = (base.join("files"), base.join("info"));
    std::fs::create_dir_all(&files).and_then(|_| std::fs::create_dir_all(&info)).map_err(|e| e.to_string())?;
    let name = path.file_name().ok_or_else(|| t!("chemin sans nom", "path without a name").to_string())?.to_string_lossy().into_owned();
    // a free name: "a.py", "a.2.py", "a.3.py"…, reserved by creating its .trashinfo
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    for k in 1..1000 {
        let candidate = if k == 1 { name.clone() } else { format!("{stem}.{k}{ext}") };
        let info_path = info.join(format!("{candidate}.trashinfo"));
        let Ok(mut f) = std::fs::OpenOptions::new().write(true).create_new(true).open(&info_path) else { continue };
        if files.join(&candidate).symlink_metadata().is_ok() {
            let _ = std::fs::remove_file(&info_path);
            continue;
        }
        let body = format!("[Trash Info]\nPath={}\nDeletionDate={}\n", percent_encode(path), utc_date(secs));
        let moved = f.write_all(body.as_bytes()).map_err(|e| e.to_string()).and_then(|_| std::fs::rename(path, files.join(&candidate)).map_err(|e| e.to_string()));
        if moved.is_err() {
            let _ = std::fs::remove_file(&info_path);
        }
        return moved;
    }
    Err(t!("corbeille pleine de noms identiques", "trash full of identical names").into())
}

/// Path for a .trashinfo: every byte but the unreserved ones and '/' as %XX.
#[cfg(unix)]
fn percent_encode(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str()
        .as_bytes()
        .iter()
        .map(|&b| if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// `2026-10-23T14:05:09` (UTC) for `secs` since 1970.
#[cfg(unix)]
fn utc_date(secs: u64) -> String {
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // civil date from the number of days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rest / 3600, rest / 60 % 60, rest % 60)
}

/// What a file chooser asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Folder,
    File,
    /// An image to give to Claude.
    Image,
    SaveAs,
}

/// File chooser of the desktop (GTK, KDE…) through the xdg portal. Blocks until the dialog is
/// closed; `Ok(None)` when cancelled.
#[cfg(target_os = "linux")]
pub fn portal_pick(kind: Pick, dir: &Path, name: Option<&str>) -> Result<Option<PathBuf>, String> {
    use std::collections::HashMap;
    use std::os::unix::ffi::OsStrExt;
    use zbus::zvariant::Value;
    let mut folder = dir.as_os_str().as_bytes().to_vec();
    folder.push(0);
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("current_folder", Value::from(folder));
    let (method, title) = match kind {
        Pick::Folder => ("OpenFile", t!("Ouvrir un dossier", "Open a folder")),
        Pick::File => ("OpenFile", t!("Ouvrir un fichier", "Open a file")),
        Pick::Image => ("OpenFile", t!("Une image pour Claude", "An image for Claude")),
        Pick::SaveAs => ("SaveFile", t!("Enregistrer sous", "Save as")),
    };
    if kind == Pick::Folder {
        opts.insert("directory", Value::from(true));
    }
    if kind == Pick::Image {
        // a filter: (name, [(0: glob pattern, pattern)])
        let globs: Vec<(u32, &str)> = IMAGE_EXTS.iter().map(|e| (0, *e)).collect();
        opts.insert("filters", Value::from(vec![(t!("Images", "Images"), globs.clone())]));
        opts.insert("current_filter", Value::from((t!("Images", "Images"), globs)));
    }
    if let Some(n) = name {
        opts.insert("current_name", Value::from(n));
    }
    let uri = portal_request("org.freedesktop.portal.FileChooser", method, Some(title), opts)
        .map_err(|e| tf!("Sélecteur de fichiers du bureau indisponible : {e}", "Desktop file chooser unavailable: {e}"))?;
    Ok(uri.and_then(|u| file_uri_path(&u)))
}

/// The patterns of the image files Claude can be given.
#[cfg(target_os = "linux")]
const IMAGE_EXTS: [&str; 10] = ["*.png", "*.jpg", "*.jpeg", "*.gif", "*.webp", "*.bmp", "*.PNG", "*.JPG", "*.JPEG", "*.WEBP"];

/// Asks the xdg portal (`interface`, `method`) and waits for its answer: the file it gives (its
/// uri), None when cancelled. `title`: the dialog's, for the methods that take one.
#[cfg(target_os = "linux")]
fn portal_request(interface: &str, method: &str, title: Option<&str>, mut opts: std::collections::HashMap<&str, zbus::zvariant::Value>) -> Result<Option<String>, zbus::Error> {
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, Value};
    let conn = zbus::blocking::Connection::session()?;
    // the request object path is known in advance: listen to its answer before asking
    let token = format!("florm{}", std::process::id());
    let sender = conn.unique_name().map(|n| n.trim_start_matches(':').replace('.', "_")).unwrap_or_default();
    let handle = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let request = zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", handle.as_str(), "org.freedesktop.portal.Request")?;
    let mut answers = request.receive_signal("Response")?;
    opts.insert("handle_token", Value::from(token.clone()));
    let portal = zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", interface)?;
    let _: OwnedObjectPath = match title {
        Some(t) => portal.call(method, &("", t, opts))?,
        None => portal.call(method, &("", opts))?,
    };
    let Some(msg) = answers.next() else { return Ok(None) };
    let body = msg.body();
    let (code, results): (u32, HashMap<String, Value>) = body.deserialize()?;
    if code != 0 {
        return Ok(None);
    }
    Ok(match results.get("uris") {
        Some(Value::Array(a)) => a.iter().find_map(|v| if let Value::Str(s) = v { Some(s.to_string()) } else { None }),
        _ => None,
    })
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
    fn trash_info_fields() {
        assert_eq!(utc_date(0), "1970-01-01T00:00:00");
        assert_eq!(utc_date(1_792_764_309), "2026-10-23T14:05:09");
        assert_eq!(percent_encode(Path::new("/home/a b/été.py")), "/home/a%20b/%C3%A9t%C3%A9.py");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn trash_keeps_the_file() {
        let dir = std::env::temp_dir().join(format!("florm-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_DATA_HOME", dir.join("data"));
        for _ in 0..2 {
            std::fs::write(dir.join("a.py"), "x").unwrap();
            freedesktop_trash(&dir.join("a.py")).unwrap();
            assert!(!dir.join("a.py").exists());
        }
        let trash = dir.join("data/Trash");
        assert_eq!(std::fs::read_to_string(trash.join("files/a.py")).unwrap(), "x");
        assert!(trash.join("files/a.2.py").exists());
        let info = std::fs::read_to_string(trash.join("info/a.2.py.trashinfo")).unwrap();
        assert!(info.contains(&format!("Path={}/a.py", dir.display())), "{info}");
        let _ = std::fs::remove_dir_all(&dir);
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
