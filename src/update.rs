//! New versions of Florm, from the GitHub releases (read with `curl`, present on Linux, macOS and
//! Windows 10+: no HTTP client in the binary). Florm shows the update; the download goes through
//! the browser and the user installs it like the first time (the package replaces the old one).

use serde::Deserialize;
use std::process::Stdio;

const RELEASES: &str = "https://api.github.com/repos/mva-odoo/florm/releases?per_page=20";

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub version: String,
    /// Release page (notes and every file).
    pub page: String,
    /// The file for this system: (name, download address).
    pub file: Option<(String, String)>,
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
    let file = r.assets.iter().find(|a| a.name.ends_with(kind)).map(|a| (a.name.clone(), a.browser_download_url.clone()));
    Some(Update { version: r.tag_name.trim_start_matches('v').to_string(), page: r.html_url, file })
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
    fn distribution_package() {
        assert_eq!(linux_kind("NAME=\"Arch Linux\"\nID=arch\n"), ".pkg.tar.zst");
        assert_eq!(linux_kind("ID=endeavouros\nID_LIKE=arch\n"), ".pkg.tar.zst");
        assert_eq!(linux_kind("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"), ".deb");
        assert_eq!(linux_kind("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n"), ".rpm");
        assert_eq!(linux_kind("ID=fedora\n"), ".rpm");
        assert_eq!(linux_kind("ID=nixos\n"), "-linux-x86_64");
    }
}
