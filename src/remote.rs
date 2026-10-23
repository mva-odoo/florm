//! Folders of another machine opened as projects: a server through SSH, a WSL distribution from
//! Windows. Florm keeps working on ordinary paths (files, git, search, linters run here):
//! - SSH: the server's `/` is mounted with `sshfs` in `<data>/ssh/<host>/` (one mount per host,
//!   every project of the host under it); on Windows, SSHFS-Win serves it as `\\sshfs.kr\…`;
//! - WSL: Windows shows the distributions as `\\wsl.localhost\<distro>\…`.
//!
//! The terminal of such a project is a shell over there (`ssh -t`, `wsl.exe --cd`) in the same
//! folder: a path is enough to know where it is, nothing else is kept.

use std::path::{Path, PathBuf};
use std::process::Stdio;

/// A machine to connect to, from the palette.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// `host` of ~/.ssh/config, or `[user@]host[:port]`.
    Ssh(String),
    /// A WSL distribution (Windows).
    Wsl(String),
}

impl Target {
    pub fn label(&self) -> &str {
        match self {
            Target::Ssh(h) | Target::Wsl(h) => h,
        }
    }
}

/// Where the hosts are mounted (Linux, macOS).
fn ssh_base() -> PathBuf {
    // as the project paths are (canonical): a data folder through a link would not match them
    crate::os::canon(&crate::config::data_dir().join("ssh"))
}

/// Folder name of a host under `ssh_base` (`:` of the port is not allowed in a file name on
/// every system; `#` can't be in a host name, so the name reads back).
fn host_dir(host: &str) -> String {
    host.replace(':', "#")
}

/// The host is something ssh takes: no path, no space, no option.
fn valid_host(host: &str) -> bool {
    !host.is_empty() && !host.starts_with('-') && !host.contains(['/', '\\', ' ', '\t', '\'', '"', '#'])
}

/// `host[:port]` → ssh arguments (`-p port`, host).
fn ssh_args(host: &str) -> Vec<String> {
    match host.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => vec!["-p".into(), p.into(), h.into()],
        _ => vec![host.into()],
    }
}

/// Remote host and absolute path of a folder: under the sshfs mounts, or an SSHFS-Win path
/// (`\\sshfs.kr\user@host!port\home\me`).
pub fn ssh_location(path: &Path) -> Option<(String, String)> {
    if let Some(rest) = unc_rest(path, &[r"\\sshfs.kr\", r"\\sshfs.k\", r"\\sshfs.r\", r"\\sshfs\"]) {
        let (host, rest) = rest.split_once('\\').unwrap_or((rest.as_str(), ""));
        let host = host.replace('!', ":");
        return Some((host, format!("/{}", rest.replace('\\', "/"))));
    }
    ssh_location_in(&ssh_base(), path)
}

fn ssh_location_in(base: &Path, path: &Path) -> Option<(String, String)> {
    let rel = path.strip_prefix(base).ok()?;
    let mut parts = rel.components();
    let host = parts.next()?.as_os_str().to_string_lossy().replace('#', ":");
    let rest: Vec<String> = parts.map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    Some((host, format!("/{}", rest.join("/"))))
}

/// WSL distribution and Linux path of a folder seen from Windows (`\\wsl.localhost\Ubuntu\home`).
pub fn wsl_location(path: &Path) -> Option<(String, String)> {
    let rest = unc_rest(path, &[r"\\wsl.localhost\", r"\\wsl$\"])?;
    let (distro, rest) = rest.split_once('\\').unwrap_or((rest.as_str(), ""));
    (!distro.is_empty()).then(|| (distro.to_string(), format!("/{}", rest.trim_end_matches('\\').replace('\\', "/"))))
}

/// What follows one of the UNC `prefixes` (case insensitive), with `/` read as `\`.
fn unc_rest(path: &Path, prefixes: &[&str]) -> Option<String> {
    let s = path.to_string_lossy().replace('/', "\\");
    let lower = s.to_lowercase();
    prefixes.iter().find(|p| lower.starts_with(*p)).map(|p| s[p.len()..].to_string())
}

/// A folder of another machine (shown in the title, offered to disconnect).
pub fn describe(path: &Path) -> Option<String> {
    if let Some((h, _)) = ssh_location(path) {
        return Some(format!("SSH : {h}"));
    }
    wsl_location(path).map(|(d, _)| format!("WSL : {d}"))
}

/// The path over there of a folder of another machine.
pub fn remote_path(path: &Path) -> Option<String> {
    ssh_location(path).or_else(|| wsl_location(path)).map(|(_, p)| p)
}

/// `'text'` for a POSIX shell (and fish).
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Program and arguments of a terminal in `cwd` when it is a folder of another machine: a login
/// shell over there, in the same folder.
pub fn terminal_argv(cwd: &Path) -> Option<Vec<String>> {
    if let Some((host, path)) = ssh_location(cwd) {
        let mut v = vec!["ssh".to_string(), "-t".to_string()];
        v.extend(ssh_args(&host));
        v.push(format!("cd {} 2>/dev/null; exec \"$SHELL\" -l", sh_quote(&path)));
        return Some(v);
    }
    let (distro, path) = wsl_location(cwd)?;
    Some(vec!["wsl.exe".into(), "-d".into(), distro, "--cd".into(), path])
}

/// The hosts of ~/.ssh/config (and the files it includes), without the patterns (`*`, `!`).
pub fn ssh_hosts() -> Vec<String> {
    let dir = crate::config::home().join(".ssh");
    let mut hosts = Vec::new();
    read_ssh_config(&dir, &dir.join("config"), &mut hosts, 0);
    let mut seen = std::collections::HashSet::new();
    hosts.retain(|h| seen.insert(h.clone()));
    hosts
}

fn read_ssh_config(dir: &Path, file: &Path, hosts: &mut Vec<String>, depth: usize) {
    let Ok(text) = std::fs::read_to_string(file) else { return };
    let (found, includes) = parse_ssh_config(&text);
    hosts.extend(found);
    if depth > 4 {
        return;
    }
    for inc in includes {
        let inc = match inc.strip_prefix("~/") {
            Some(rest) => crate::config::home().join(rest),
            None => dir.join(&inc),
        };
        for f in expand_glob(&inc) {
            read_ssh_config(dir, &f, hosts, depth + 1);
        }
    }
}

/// Host names and `Include` paths of an ssh_config.
fn parse_ssh_config(text: &str) -> (Vec<String>, Vec<String>) {
    let (mut hosts, mut includes) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let line = line.trim();
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k, v.trim_start_matches(|c: char| c.is_whitespace() || c == '=')),
            None => continue,
        };
        let words = value.split_whitespace().map(|w| w.trim_matches('"').to_string());
        if key.eq_ignore_ascii_case("host") {
            hosts.extend(words.filter(|w| !w.contains(['*', '?', '!'])));
        } else if key.eq_ignore_ascii_case("include") {
            includes.extend(words);
        }
    }
    (hosts, includes)
}

/// The files of a path with `*` in its last component (`config.d/*`), else the path.
fn expand_glob(p: &Path) -> Vec<PathBuf> {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let Some((before, after)) = name.split_once('*') else { return vec![p.to_path_buf()] };
    let Some(dir) = p.parent() else { return Vec::new() };
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|f| f.file_name().map(|n| n.to_string_lossy()).is_some_and(|n| n.starts_with(before) && n.ends_with(after)))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// The WSL distributions (none outside Windows).
pub fn wsl_distros() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let Ok(out) = crate::os::command("wsl.exe").args(["-l", "-q"]).stdin(Stdio::null()).output() else { return Vec::new() };
    parse_wsl_list(&out.stdout)
}

/// `wsl.exe -l -q` writes UTF-16 (little endian), a name per line.
fn parse_wsl_list(bytes: &[u8]) -> Vec<String> {
    let text = if (bytes.len() >= 2 && bytes[1] == 0) || bytes.starts_with(&[0xff, 0xfe]) {
        let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.lines().map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c == '\0').to_string()).filter(|l| !l.is_empty()).collect()
}

/// Runs a command with nothing to type in (ssh fails instead of asking a password), its output
/// or its error.
fn run(mut c: std::process::Command, what: &str) -> Result<String, String> {
    let out = c.stdin(Stdio::null()).output().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => tf!("{what} introuvable", "{what} not found"),
        _ => format!("{what} : {e}"),
    })?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout)).trim().to_string())
    }
}

/// Options of ssh: no question (Florm has no terminal for them), a short wait for a dead host.
const SSH_OPTS: [&str; 4] = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"];

/// The home folder of the user over there.
fn ssh_home(host: &str) -> Result<String, String> {
    let mut c = crate::os::command("ssh");
    c.args(SSH_OPTS).args(ssh_args(host)).arg("pwd");
    run(c, "ssh").map_err(|e| ssh_error(host, &e))
}

fn ssh_error(host: &str, e: &str) -> String {
    let hint = if e.contains("Permission denied") || e.contains("password") {
        t!(
            " · Florm se connecte sans mot de passe : ajoute ta clé sur le serveur (ssh-copy-id), ou charge-la dans ssh-agent.",
            " · Florm connects without a password: put your key on the server (ssh-copy-id), or load it in ssh-agent."
        )
    } else if e.contains("Host key verification failed") {
        t!(" · Connecte-toi une fois avec ssh dans un terminal pour accepter la clé du serveur.", " · Connect once with ssh in a terminal to accept the server's key.")
    } else {
        ""
    };
    tf!("Connexion à {host} impossible : {e}{hint}", "Cannot connect to {host}: {e}{hint}")
}

/// Whether `dir` is a mount point (another device than its parent); `Err` when the mount is
/// there but dead (the connection was lost and sshfs stopped).
#[cfg(unix)]
fn mounted(dir: &Path) -> Result<bool, ()> {
    use std::os::unix::fs::MetadataExt;
    let Some(parent) = dir.parent() else { return Ok(false) };
    match (std::fs::metadata(dir), std::fs::metadata(parent)) {
        (Ok(d), Ok(p)) => Ok(d.dev() != p.dev()),
        (Err(e), _) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        _ => Err(()),
    }
}

#[cfg(unix)]
fn unmount_dir(dir: &Path, lazy: bool) -> Result<(), String> {
    let mut c = if cfg!(target_os = "macos") {
        crate::os::command("umount")
    } else {
        let mut c = crate::os::command(if crate::tools::locate("fusermount3") { "fusermount3" } else { "fusermount" });
        c.arg(if lazy { "-uz" } else { "-u" });
        c
    };
    c.arg(dir);
    run(c, "umount").map(|_| ())
}

/// Whether the folder is on a host to mount first (`mount` before opening it).
pub fn needs_mount(path: &Path) -> bool {
    #[cfg(unix)]
    if let Some((host, _)) = ssh_location_in(&ssh_base(), path) {
        return mounted(&ssh_base().join(host_dir(&host))) != Ok(true);
    }
    let _ = path;
    false
}

/// Mounts the host of `path` (a folder under the mounts) if it is not.
pub fn mount_for(path: &Path) -> Result<(), String> {
    let (host, _) = ssh_location_in(&ssh_base(), path).ok_or_else(|| t!("Pas un dossier SSH", "Not an SSH folder").to_string())?;
    mount(&host).map(|_| ())
}

/// Mounts the `/` of the host (Linux, macOS: sshfs) and returns its folder here.
#[cfg(unix)]
fn mount(host: &str) -> Result<PathBuf, String> {
    let dir = ssh_base().join(host_dir(host));
    match mounted(&dir) {
        Ok(true) => return Ok(dir),
        Ok(false) => {}
        // dead mount: away with it, then mount again
        Err(()) => {
            let _ = unmount_dir(&dir, true);
        }
    }
    if !crate::tools::locate("sshfs") {
        return Err(sshfs_missing());
    }
    std::fs::create_dir_all(&dir).map_err(|e| tf!("Création de {} impossible : {e}", "Cannot create {}: {e}", dir.display()))?;
    let mut args = ssh_args(host);
    let name = args.pop().unwrap_or_default();
    let mut c = crate::os::command("sshfs");
    c.arg(format!("{name}:/")).arg(&dir).args(&args);
    // reconnect: a laptop going to sleep does not leave a dead mount; transform_symlinks: the
    // absolute links of the server point inside the mount
    let mut opts = "reconnect,ServerAliveInterval=15,ServerAliveCountMax=3,BatchMode=yes,ConnectTimeout=10,transform_symlinks,idmap=user".to_string();
    if cfg!(target_os = "macos") {
        opts.push_str(&format!(",volname={name}"));
    }
    c.args(["-o", &opts]);
    run(c, "sshfs").map_err(|e| ssh_error(host, &e))?;
    Ok(dir)
}

/// On Windows the folders of SSHFS-Win are reached by their path: nothing to mount.
#[cfg(windows)]
fn mount(host: &str) -> Result<PathBuf, String> {
    // user, address and port as ssh resolves them (`Host` of ~/.ssh/config): SSHFS-Win wants them
    let mut c = crate::os::command("ssh");
    c.arg("-G").args(ssh_args(host));
    let conf = run(c, "ssh")?;
    let field = |k: &str| conf.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix(' ')).map(str::to_string);
    let (user, name, port) = (field("user").unwrap_or_default(), field("hostname").unwrap_or_else(|| host.to_string()), field("port").unwrap_or_default());
    let port = if port.is_empty() || port == "22" { String::new() } else { format!("!{port}") };
    let dir = PathBuf::from(format!(r"\\sshfs.kr\{user}@{name}{port}"));
    if !dir.is_dir() {
        return Err(sshfs_missing());
    }
    Ok(dir)
}

fn sshfs_missing() -> String {
    use crate::tools::System;
    let how = match crate::tools::system() {
        System::Arch => "sudo pacman -S --needed sshfs",
        System::Debian => "sudo apt install -y sshfs",
        System::Fedora => "sudo dnf install -y fuse-sshfs",
        System::Suse => "sudo zypper install -y sshfs",
        System::Mac => "brew install --cask macfuse && brew install gromgit/fuse/sshfs-mac",
        System::Windows => "winget install -e --id WinFsp.WinFsp ; winget install -e --id SSHFS-Win.SSHFS-Win",
        System::OtherLinux => t!("le paquet sshfs de ta distribution", "the sshfs package of your distribution"),
    };
    tf!("Florm ouvre les dossiers SSH avec sshfs : installe-le ({how}), puis réessaie.", "Florm opens SSH folders with sshfs: install it ({how}), then try again.")
}

/// Connects to `target` and returns the folder to start from (the home folder over there).
pub fn connect(target: &Target) -> Result<PathBuf, String> {
    match target {
        Target::Ssh(host) => {
            let host = host.trim();
            if !valid_host(host) {
                return Err(tf!("Hôte SSH invalide : {host}", "Invalid SSH host: {host}"));
            }
            let home = ssh_home(host)?;
            let dir = mount(host)?;
            Ok(home.split('/').filter(|s| !s.is_empty()).fold(dir, |p, s| p.join(s)))
        }
        Target::Wsl(distro) => {
            let mut c = crate::os::command("wsl.exe");
            c.args(["-d", distro, "--cd", "~", "-e", "pwd"]);
            let home = run(c, "wsl.exe").map_err(|e| tf!("WSL {distro} : {e}", "WSL {distro}: {e}"))?;
            let dir = PathBuf::from(format!(r"\\wsl.localhost\{distro}"));
            Ok(home.split('/').filter(|s| !s.is_empty()).fold(dir, |p, s| p.join(s)))
        }
    }
}

/// Unmounts the host of `path` (SSH folders on Linux, macOS); `false`: nothing to unmount.
pub fn disconnect(path: &Path) -> Result<bool, String> {
    #[cfg(unix)]
    if let Some((host, _)) = ssh_location_in(&ssh_base(), path) {
        let dir = ssh_base().join(host_dir(&host));
        if mounted(&dir) == Ok(false) {
            return Ok(false);
        }
        return unmount_dir(&dir, false).map(|_| true);
    }
    let _ = path;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_folders_under_the_mounts() {
        let base = Path::new("/data/ssh");
        assert_eq!(ssh_location_in(base, Path::new("/data/ssh/prod/home/me/odoo")), Some(("prod".into(), "/home/me/odoo".into())));
        assert_eq!(ssh_location_in(base, Path::new("/data/ssh/me@srv#2222")), Some(("me@srv:2222".into(), "/".into())));
        assert_eq!(ssh_location_in(base, Path::new("/home/me")), None);
        assert_eq!(host_dir("me@srv:2222"), "me@srv#2222");
    }

    #[test]
    fn sshfs_win_paths() {
        assert_eq!(ssh_location(Path::new(r"\\sshfs.kr\me@srv!2222\home\me")), Some(("me@srv:2222".into(), "/home/me".into())));
    }

    #[test]
    fn wsl_paths() {
        assert_eq!(wsl_location(Path::new(r"\\wsl.localhost\Ubuntu\home\me\src")), Some(("Ubuntu".into(), "/home/me/src".into())));
        assert_eq!(wsl_location(Path::new(r"\\WSL$\Debian")), Some(("Debian".into(), "/".into())));
        assert_eq!(wsl_location(Path::new(r"C:\Users\me")), None);
    }

    #[test]
    fn terminal_over_ssh_and_wsl() {
        let wsl = terminal_argv(Path::new(r"\\wsl.localhost\Ubuntu\home\me")).unwrap();
        assert_eq!(wsl, ["wsl.exe", "-d", "Ubuntu", "--cd", "/home/me"]);
        let ssh = terminal_argv(Path::new(r"\\sshfs.kr\me@srv!2222\home\it's")).unwrap();
        assert_eq!(ssh, ["ssh", "-t", "-p", "2222", "me@srv", r#"cd '/home/it'\''s' 2>/dev/null; exec "$SHELL" -l"#]);
        assert_eq!(terminal_argv(Path::new("/home/me")), None);
    }

    #[test]
    fn hosts_of_ssh_config() {
        let text = "Host prod staging\n  HostName 10.0.0.1\nHost *.internal !x\nhost=dev\nInclude config.d/*\n# Host commented\n";
        let (hosts, includes) = parse_ssh_config(text);
        assert_eq!(hosts, ["prod", "staging", "dev"]);
        assert_eq!(includes, ["config.d/*"]);
    }

    #[test]
    fn wsl_list_in_utf16() {
        let bytes: Vec<u8> = "Ubuntu\r\ndocker-desktop\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(parse_wsl_list(&bytes), ["Ubuntu", "docker-desktop"]);
    }

    #[test]
    fn hosts_accepted() {
        assert!(valid_host("me@srv:2222"));
        assert!(!valid_host("-oProxyCommand=x"));
        assert!(!valid_host("srv /etc"));
        assert_eq!(ssh_args("srv:22"), ["-p", "22", "srv"]);
        assert_eq!(ssh_args("srv"), ["srv"]);
    }
}
