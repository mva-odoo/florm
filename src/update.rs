//! New versions of Florm, from the GitHub releases (read with `curl`, present on Linux, macOS and
//! Windows 10+: no HTTP client in the binary). Florm downloads the file of this system, checks it
//! against the release's SHA256SUMS and puts it in place of the running one, then restarts:
//! - Linux package (/usr/bin): the package manager through `pkexec` (the system asks the password);
//! - Linux binary: the new binary renamed over the old one (`pkexec install` if the folder is root's);
//! - Windows: the running .exe renamed to .old (removed at the next start), the new one in its place;
//! - macOS: the .app of the .dmg copied over the installed one.
//! When that fails, the file can still be downloaded in the browser.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;

const RELEASES: &str = "https://api.github.com/repos/mva-odoo/florm/releases?per_page=20";

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub version: String,
    /// Release page (notes and every file).
    pub page: String,
    /// The file for this system: (name, download address).
    pub file: Option<(String, String)>,
    /// Every file of the release: (name, download address).
    pub assets: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The newest release if it is newer than this Florm, else `None`.
pub fn check() -> Result<Option<Update>, String> {
    let out = crate::os::command("curl")
        .args(["-fsSL", "--max-time", "20", "-H", "Accept: application/vnd.github+json", RELEASES])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "curl est introuvable : impossible de chercher une mise à jour".to_string())?;
    if !out.status.success() {
        return Err(format!("Recherche de mise à jour impossible : {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let releases: Vec<Release> = serde_json::from_slice(&out.stdout).map_err(|e| format!("Réponse de GitHub illisible : {e}"))?;
    Ok(newest(releases, current(), &file_kind()))
}

fn newest(releases: Vec<Release>, current: &str, kind: &str) -> Option<Update> {
    let r = releases
        .into_iter()
        .filter(|r| !r.draft && is_newer(&r.tag_name, current))
        .max_by(|a, b| version_key(&a.tag_name).cmp(&version_key(&b.tag_name)))?;
    let assets: Vec<(String, String)> = r.assets.into_iter().map(|a| (a.name, a.browser_download_url)).collect();
    let file = assets.iter().find(|(name, _)| name.ends_with(kind)).cloned();
    Some(Update { version: r.tag_name.trim_start_matches('v').to_string(), page: r.html_url, file, assets })
}

/// "v0.2.0-beta" → ([0, 2, 0], false): numbers, then a pre-version sorts before the version.
fn version_key(v: &str) -> (Vec<u64>, bool) {
    let v = v.trim().trim_start_matches('v');
    let (main, pre) = match v.split_once('-') {
        Some((m, _)) => (m, true),
        None => (v, false),
    };
    (main.split('.').map(|n| n.parse().unwrap_or(0)).collect(), !pre)
}

fn is_newer(tag: &str, current: &str) -> bool {
    version_key(tag) > version_key(current)
}

/// End of the name of the release file for this system.
fn file_kind() -> String {
    if cfg!(windows) {
        ".exe".into()
    } else if cfg!(target_os = "macos") {
        ".dmg".into()
    } else {
        linux_kind(&std::fs::read_to_string("/etc/os-release").unwrap_or_default()).into()
    }
}

/// The package of the distribution described by /etc/os-release (ID and ID_LIKE).
fn linux_kind(os_release: &str) -> &'static str {
    let ids: Vec<&str> = os_release
        .lines()
        .filter_map(|l| l.strip_prefix("ID=").or_else(|| l.strip_prefix("ID_LIKE=")))
        .flat_map(|v| v.trim_matches('"').split_whitespace())
        .collect();
    let any = |names: &[&str]| ids.iter().any(|i| names.contains(i));
    if any(&["arch"]) {
        ".pkg.tar.zst"
    } else if any(&["debian", "ubuntu"]) {
        ".deb"
    } else if any(&["fedora", "rhel", "centos", "suse", "opensuse"]) {
        ".rpm"
    } else {
        "-linux-x86_64"
    }
}

/// Downloads the update, checks it and puts it in place of this Florm. Returns the program to
/// start for the restart (the path of the running one, read before it is replaced: on Linux the
/// running program then reads as "… (deleted)").
pub fn install(u: &Update) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Emplacement de Florm inconnu : {e}"))?;
    let exe = crate::os::canon(&exe);
    if exe.components().any(|c| c.as_os_str() == "target") {
        return Err("Florm est lancé depuis ses sources (dossier target) : mettez-le à jour avec git pull et cargo build".into());
    }
    let dir = std::env::temp_dir().join(format!("florm-update-{}", u.version));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let r = install_from(u, &exe, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    r.map(|()| exe)
}

/// Removes what a Windows update leaves (the old .exe, still running during the update).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_exe(&exe));
    }
}

fn old_exe(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

#[cfg(target_os = "linux")]
fn install_from(u: &Update, exe: &Path, dir: &Path) -> Result<(), String> {
    let package = u.file.as_ref().filter(|(name, _)| !name.ends_with("-linux-x86_64"));
    if exe == Path::new("/usr/bin/florm") {
        if let Some((name, url)) = package {
            let file = download(u, name, url, dir)?;
            let file = file.to_string_lossy();
            let args: &[&str] = if name.ends_with(".pkg.tar.zst") {
                &["pacman", "-U", "--noconfirm", &file]
            } else if name.ends_with(".deb") {
                &["dpkg", "-i", &file]
            } else {
                &["rpm", "-U", "--oldpackage", &file]
            };
            return as_root(args);
        }
    }
    let (name, url) = u.assets.iter().find(|(name, _)| name.ends_with("-linux-x86_64")).ok_or("Pas de binaire Linux dans cette version")?;
    let file = download(u, name, url, dir)?;
    set_executable(&file)?;
    match replace(&file, exe) {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            as_root(&["install", "-m755", &file.to_string_lossy(), &exe.to_string_lossy()])
        }
        r => r.map_err(|e| format!("Remplacement de {} impossible : {e}", exe.display())),
    }
}

#[cfg(windows)]
fn install_from(u: &Update, exe: &Path, dir: &Path) -> Result<(), String> {
    let (name, url) = u.file.as_ref().ok_or("Pas de fichier Windows dans cette version")?;
    let file = download(u, name, url, dir)?;
    // a running .exe can't be overwritten, but it can be renamed
    let old = old_exe(exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| format!("Remplacement de {} impossible : {e}", exe.display()))?;
    if let Err(e) = std::fs::copy(&file, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(format!("Remplacement de {} impossible : {e}", exe.display()));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_from(u: &Update, exe: &Path, dir: &Path) -> Result<(), String> {
    // exe: …/Florm.app/Contents/MacOS/florm
    let app = exe.ancestors().nth(3).filter(|a| a.extension().is_some_and(|e| e == "app")).ok_or("Florm n'est pas dans une application .app")?;
    let parent = app.parent().ok_or("Dossier de Florm.app inconnu")?;
    let (name, url) = u.file.as_ref().ok_or("Pas de fichier macOS dans cette version")?;
    let dmg = download(u, name, url, dir)?;
    let mount = dir.join("mnt");
    run(crate::os::command("hdiutil").args(["attach", "-nobrowse", "-readonly", "-mountpoint"]).arg(&mount).arg(&dmg), "hdiutil attach")?;
    let new = parent.join(".Florm.app.new");
    let old = parent.join(".Florm.app.old");
    let _ = std::fs::remove_dir_all(&new);
    let _ = std::fs::remove_dir_all(&old);
    let copied = run(crate::os::command("ditto").arg(mount.join("Florm.app")).arg(&new), "ditto");
    let _ = run(crate::os::command("hdiutil").args(["detach", "-quiet"]).arg(&mount), "hdiutil detach");
    copied?;
    let moved = std::fs::rename(app, &old).and_then(|()| {
        std::fs::rename(&new, app).inspect_err(|_| {
            let _ = std::fs::rename(&old, app);
        })
    });
    let _ = std::fs::remove_dir_all(&new);
    let _ = std::fs::remove_dir_all(&old);
    moved.map_err(|e| format!("Remplacement de {} impossible : {e}", app.display()))
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn install_from(_u: &Update, _exe: &Path, _dir: &Path) -> Result<(), String> {
    Err("Mise à jour intégrée non disponible sur ce système".into())
}

/// Downloads a file of the release into `dir` and checks it against the release's SHA256SUMS.
fn download(u: &Update, name: &str, url: &str, dir: &Path) -> Result<PathBuf, String> {
    let (_, sums_url) = u
        .assets
        .iter()
        .find(|(n, _)| n == "SHA256SUMS")
        .ok_or("Cette version n'a pas de SHA256SUMS : le fichier ne peut pas être vérifié")?;
    let sums = dir.join("SHA256SUMS");
    curl(sums_url, &sums)?;
    let sums = std::fs::read_to_string(&sums).map_err(|e| format!("SHA256SUMS : {e}"))?;
    let expected = expected_sum(&sums, name).ok_or_else(|| format!("{name} n'est pas dans SHA256SUMS"))?;
    let file = dir.join(name);
    curl(url, &file)?;
    let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(format!("{name} est corrompu (somme SHA-256 différente de SHA256SUMS)"));
    }
    Ok(file)
}

/// The sum of `name` in a `sha256sum` output ("<hex>  <name>", or "<hex> *<name>" in binary mode).
fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (sum, file) = l.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name).then(|| sum.to_string())
    })
}

fn curl(url: &str, to: &Path) -> Result<(), String> {
    run(crate::os::command("curl").args(["-fsSL", "--max-time", "600", "-o"]).arg(to).arg(url), "curl")
}

fn run(c: &mut std::process::Command, what: &str) -> Result<(), String> {
    let out = c.stdin(Stdio::null()).output().map_err(|e| format!("{what} : {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{what} a échoué : {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Runs a command as root through polkit (the desktop asks the password).
#[cfg(target_os = "linux")]
fn as_root(args: &[&str]) -> Result<(), String> {
    let out = crate::os::command("pkexec")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "pkexec est introuvable : impossible d'obtenir les droits pour installer".to_string())?;
    match out.status.code() {
        Some(0) => Ok(()),
        Some(126) => Err("Mise à jour annulée (mot de passe non donné)".into()),
        Some(127) => Err("Droits refusés par pkexec (pas d'agent polkit pour demander le mot de passe ?)".into()),
        _ => Err(format!("{} a échoué : {}", args[0], String::from_utf8_lossy(&out.stderr).trim())),
    }
}

#[cfg(target_os = "linux")]
fn set_executable(file: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("{}: {e}", file.display()))
}

/// Puts `file` in place of `exe`: copied next to it, then renamed over it (the running program
/// keeps the old file until it quits).
#[cfg(target_os = "linux")]
fn replace(file: &Path, exe: &Path) -> std::io::Result<()> {
    let mut new = exe.as_os_str().to_owned();
    new.push(".new");
    let new = PathBuf::from(new);
    std::fs::copy(file, &new)?;
    std::fs::rename(&new, exe).inspect_err(|_| {
        let _ = std::fs::remove_file(&new);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, draft: bool, files: &[&str]) -> Release {
        Release {
            tag_name: tag.into(),
            html_url: format!("https://github.com/r/{tag}"),
            draft,
            assets: files.iter().map(|f| Asset { name: f.to_string(), browser_download_url: format!("https://dl/{f}") }).collect(),
        }
    }

    #[test]
    fn versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("v0.10.0", "0.9.3"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("alpha", "0.1.0"));
        assert!(is_newer("v0.2.0", "0.2.0-beta"));
        assert!(!is_newer("v0.2.0-beta", "0.2.0"));
    }

    #[test]
    fn newest_release_and_its_file() {
        let releases = vec![
            rel("v0.3.0", true, &["florm_0.3.0_amd64.deb"]),
            rel("v0.2.1", false, &["florm_0.2.1_amd64.deb", "florm-0.2.1.x86_64.rpm"]),
            rel("v0.2.0", false, &["florm_0.2.0_amd64.deb"]),
            rel("alpha", false, &[]),
        ];
        let u = newest(releases, "0.1.0", ".deb").unwrap();
        assert_eq!(u.version, "0.2.1");
        assert_eq!(u.file, Some(("florm_0.2.1_amd64.deb".into(), "https://dl/florm_0.2.1_amd64.deb".into())));
        assert_eq!(newest(vec![rel("v0.1.0", false, &[])], "0.1.0", ".deb"), None);
    }

    #[test]
    fn sums() {
        let sums = "abc123  florm_0.2.1_amd64.deb\ndef456 *florm-0.2.1-linux-x86_64\n";
        assert_eq!(expected_sum(sums, "florm_0.2.1_amd64.deb").as_deref(), Some("abc123"));
        assert_eq!(expected_sum(sums, "florm-0.2.1-linux-x86_64").as_deref(), Some("def456"));
        assert_eq!(expected_sum(sums, "florm-0.2.1.x86_64.rpm"), None);
    }

    #[test]
    fn distribution_package() {
        assert_eq!(linux_kind("NAME=\"Arch Linux\"\nID=arch\n"), ".pkg.tar.zst");
        assert_eq!(linux_kind("ID=endeavouros\nID_LIKE=arch\n"), ".pkg.tar.zst");
        assert_eq!(linux_kind("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"), ".deb");
        assert_eq!(linux_kind("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n"), ".rpm");
        assert_eq!(linux_kind("ID=fedora\n"), ".rpm");
        assert_eq!(linux_kind("ID=nixos\n"), "-linux-x86_64");
    }
}
