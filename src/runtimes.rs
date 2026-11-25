//! What Florm downloads itself so that nothing else has to be installed: Python (for the
//! extensions and the Python tools of the language packs), Node.js (npm tools), Go (go install),
//! and the programs of GitHub releases (rust-analyzer, marksman…). Each archive is checked
//! against a SHA-256 sum (pinned here for the runtimes, given by GitHub for the releases), then
//! unpacked with the system's `curl` and `tar` (present on Linux, macOS and Windows 10+).
//! A runtime of the system is used when there is a real one (python3 that answers « Python 3 »,
//! npm, go): nothing is downloaded then.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// The platform: (os, arch) as « linux-x64 », « macos-arm64 », « windows-x64 »…
pub fn platform() -> String {
    let os = if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" };
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    format!("{os}-{arch}")
}

/// A runtime Florm can download: per platform, the archive and its SHA-256.
struct Runtime {
    name: &'static str,
    version: &'static str,
    /// (platform, url, sha256)
    files: &'static [(&'static str, &'static str, &'static str)],
    /// Where its programs are once unpacked (relative to its folder), per OS family.
    bin_unix: &'static str,
    bin_windows: &'static str,
}

const PY_BASE: &str = "https://github.com/astral-sh/python-build-standalone/releases/download/20261124/";

const PYTHON: Runtime = Runtime {
    name: "python",
    version: "3.12.16",
    files: &[
        ("linux-x64", "cpython-3.12.16+20261124-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz", "7e1adaea26c959d13aa1120be1ae62a1ea1bb52adcb382679fffe9217e22adff"),
        ("linux-arm64", "cpython-3.12.16+20261124-aarch64-unknown-linux-gnu-install_only_stripped.tar.gz", "518e24b37e356775d82d5609cd9d48e27e211381a39ae416381ba696bf76c25c"),
        ("macos-x64", "cpython-3.12.16+20261124-x86_64-apple-darwin-install_only_stripped.tar.gz", "6d7ef4efa1adf65f312e6e865040ce03ef8a4bd7e1b45415193295c9ffc0dbdd"),
        ("macos-arm64", "cpython-3.12.16+20261124-aarch64-apple-darwin-install_only_stripped.tar.gz", "9d6d9d0d85feda3f9cb4eb3e048f0fed21ed280feda087927eaa4fbc7124826f"),
        ("windows-x64", "cpython-3.12.16+20261124-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", "96694b50dcfde13d56e40346ddaa552a9cd650290e9b933cecf83c1d82226960"),
        ("windows-arm64", "cpython-3.12.16+20261124-aarch64-pc-windows-msvc-install_only_stripped.tar.gz", "005f88ec67b0821df5e90406b561c7698a9fba8387c1e5298a862b6639ad65b9"),
    ],
    bin_unix: "python/bin",
    bin_windows: "python",
};

const NODE: Runtime = Runtime {
    name: "node",
    version: "26.13.1",
    files: &[
        ("linux-x64", "https://nodejs.org/dist/v26.13.1/node-v26.13.1-linux-x64.tar.gz", "518674de2b621728a5e4486f5fb3b7d2900bb3cc4aa97969edaa463db99f9778"),
        ("linux-arm64", "https://nodejs.org/dist/v26.13.1/node-v26.13.1-linux-arm64.tar.gz", "80a330b35415382878df18d41afbd106ea89bfb3be0087d6608e289a14c5cc7d"),
        ("macos-x64", "https://nodejs.org/dist/v26.13.1/node-v26.13.1-darwin-x64.tar.gz", "82bb96078a24ca50b9c9a38f2379e6be6d87171fcd400fc14c8934b3bce96e2f"),
        ("macos-arm64", "https://nodejs.org/dist/v26.13.1/node-v26.13.1-darwin-arm64.tar.gz", "aec2514ea1ceb2ba52455e9ec65986f6749a06dd759e5db2a72e6889668c9735"),
        ("windows-x64", "https://nodejs.org/dist/v26.13.1/node-v26.13.1-win-x64.zip", "e2edda75bfb089654705cf65a13b58933bfd5f57148e8ebda916f58a2474f1f7"),
        ("windows-arm64", "https://nodejs.org/dist/v26.13.1/node-v26.13.1-win-arm64.zip", "181ebe431b90787113a9e4fc80d38ce86d37888a353a0386de44d2872e48cb97"),
    ],
    bin_unix: "*/bin",
    bin_windows: "*",
};

const GO: Runtime = Runtime {
    name: "go",
    version: "1.27.4",
    files: &[
        ("linux-x64", "https://go.dev/dl/go1.27.4.linux-amd64.tar.gz", "7226736c2339719cda28fbf3a54d71aa63bcefe376e98cef00034268dab24840"),
        ("linux-arm64", "https://go.dev/dl/go1.27.4.linux-arm64.tar.gz", "3392582988a273a87ab5043ee1b2c9c17a2c3c00580bf9326c47a2e0aaaeac92"),
        ("macos-x64", "https://go.dev/dl/go1.27.4.darwin-amd64.tar.gz", "2b799377704ef12db39bd254995d5c348c9cc8d86562ca817724390e56c38f43"),
        ("macos-arm64", "https://go.dev/dl/go1.27.4.darwin-arm64.tar.gz", "91a44a2b8a716600ad94a4f143e156116620db8828086a6e3602291be9bd0bb4"),
        ("windows-x64", "https://go.dev/dl/go1.27.4.windows-amd64.zip", "178ab98950ac69ab25069307dc6ade79f3816e414cce95b4da166a115cde1e7e"),
        ("windows-arm64", "https://go.dev/dl/go1.27.4.windows-arm64.zip", "9e7a72565792b8ab370b37ac9ac109799237abacb39ff2f541c438b963c0cc64"),
    ],
    bin_unix: "go/bin",
    bin_windows: "go/bin",
};

fn root() -> PathBuf {
    crate::config::data_dir().join("runtimes")
}

fn dir_of(r: &Runtime) -> PathBuf {
    root().join(format!("{}-{}", r.name, r.version))
}

/// The folder of a runtime's programs, when it is downloaded.
fn bin_of(r: &Runtime) -> Option<PathBuf> {
    let d = dir_of(r);
    let rel = if cfg!(windows) { r.bin_windows } else { r.bin_unix };
    let path = match rel.split_once('/') {
        // "*/bin": the one folder the archive made (node-v26…/bin)
        Some(("*", rest)) => std::fs::read_dir(&d).ok()?.flatten().map(|e| e.path()).find(|p| p.is_dir())?.join(rest),
        _ if rel == "*" => std::fs::read_dir(&d).ok()?.flatten().map(|e| e.path()).find(|p| p.is_dir())?,
        _ => d.join(rel),
    };
    (d.join(".done").is_file() && path.is_dir()).then_some(path)
}

fn dirs_cell() -> &'static std::sync::RwLock<Option<Vec<PathBuf>>> {
    static C: std::sync::OnceLock<std::sync::RwLock<Option<Vec<PathBuf>>>> = std::sync::OnceLock::new();
    C.get_or_init(|| std::sync::RwLock::new(None))
}

/// The folders of the downloaded runtimes and GitHub programs: added to the PATH of every
/// program Florm runs (`os::command`), so that npm finds node and a server installed by npm too.
/// Read once; `refresh` after a download.
pub fn bin_dirs() -> Vec<PathBuf> {
    if let Some(v) = dirs_cell().read().ok().and_then(|c| c.clone()) {
        return v;
    }
    let mut v: Vec<PathBuf> = [&PYTHON, &NODE, &GO].into_iter().filter_map(bin_of).collect();
    v.extend(github_dirs());
    if let Ok(mut c) = dirs_cell().write() {
        *c = Some(v.clone());
    }
    v
}

/// Forget the folders read (after a download or an install).
pub fn refresh() {
    if let Ok(mut c) = dirs_cell().write() {
        *c = None;
    }
}

/// The PATH for the programs Florm runs: the process's, with the folders of `bin_dirs` after it.
pub fn path_env() -> Option<std::ffi::OsString> {
    let extra = bin_dirs();
    if extra.is_empty() {
        return None;
    }
    let cur = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::env::split_paths(&cur).chain(extra)).ok()
}

/// `exe` with « .exe » on Windows.
fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// Downloads `url` to `to` and checks its SHA-256 (hex). Err: why.
pub fn download_checked(url: &str, to: &Path, sha256: &str) -> Result<(), String> {
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let out = crate::os::command("curl")
        .args(["-fsSL", "--retry", "2", "--max-time", "900", "-o"])
        .arg(to)
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| crate::t!("curl est introuvable", "curl was not found").to_string())?;
    if !out.status.success() {
        return Err(crate::tf!("Téléchargement de {url} impossible : {}", "Cannot download {url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let bytes = std::fs::read(to).map_err(|e| e.to_string())?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(sha256.trim()) {
        let _ = std::fs::remove_file(to);
        return Err(crate::tf!("{url} : somme SHA-256 différente de celle attendue (fichier refusé)", "{url}: SHA-256 sum different from the expected one (file refused)"));
    }
    Ok(())
}

/// Unpacks an archive (.tar.gz, .tgz, .zip) into `dir` with the system's tar.
fn unpack(archive: &Path, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let out = crate::os::command("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| crate::t!("tar est introuvable", "tar was not found").to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(crate::tf!("Archive {} illisible : {}", "Unreadable archive {}: {}", archive.display(), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Downloads and unpacks a runtime if it is not there (on a thread). Ok: the folder of its programs.
fn ensure(r: &Runtime) -> Result<PathBuf, String> {
    if let Some(b) = bin_of(r) {
        return Ok(b);
    }
    let plat = platform();
    let (_, file, sha) = r.files.iter().find(|(p, ..)| *p == plat).ok_or_else(|| crate::tf!("{} n'est pas disponible pour {plat}", "{} is not available for {plat}", r.name))?;
    let url = if file.starts_with("https://") { file.to_string() } else { format!("{PY_BASE}{}", file.replace('+', "%2B")) };
    let d = dir_of(r);
    let _ = std::fs::remove_dir_all(&d);
    let archive = root().join(format!("{}-{}.download", r.name, r.version));
    download_checked(&url, &archive, sha)?;
    let r2 = unpack(&archive, &d);
    let _ = std::fs::remove_file(&archive);
    r2?;
    std::fs::write(d.join(".done"), "").map_err(|e| e.to_string())?;
    refresh();
    bin_of(r).ok_or_else(|| crate::tf!("{} : l'archive n'a pas la forme attendue", "{}: the archive does not have the expected layout", r.name))
}

/// A real Python 3 of the system (≥ 3.9): it answers « Python 3.x » (the Microsoft Store's
/// python3 alias does not).
pub fn system_python() -> Option<String> {
    for name in ["python3", "python"] {
        let Some(p) = crate::lsp::in_path(name) else { continue };
        // the Store's aliases live in WindowsApps: they open the Store instead of running
        if p.to_string_lossy().contains("WindowsApps") {
            continue;
        }
        let Ok(out) = crate::os::command(&p).arg("--version").stdin(Stdio::null()).output() else { continue };
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let minor = text.trim().strip_prefix("Python 3.").and_then(|v| v.split('.').next()?.parse::<u32>().ok());
        if out.status.success() && minor.is_some_and(|m| m >= 9) {
            return Some(p.display().to_string());
        }
    }
    None
}

fn managed_python() -> Option<PathBuf> {
    bin_of(&PYTHON).map(|b| b.join(if cfg!(windows) { "python.exe" } else { "python3" })).filter(|p| p.is_file())
}

/// The Python of Florm's tools and extensions, if there is one without downloading.
pub fn python_ready() -> Option<String> {
    managed_python().map(|p| p.display().to_string()).or_else(system_python)
}

/// The Python of Florm's tools and extensions: the system's, else Florm's own (downloaded once,
/// ~30 MB). On a thread.
pub fn python() -> Result<String, String> {
    if let Some(p) = python_ready() {
        return Ok(p);
    }
    ensure(&PYTHON)?;
    managed_python().map(|p| p.display().to_string()).ok_or_else(|| "python".to_string())
}

/// npm: the system's, else the one of Florm's Node.js (downloaded once). On a thread.
pub fn npm() -> Result<PathBuf, String> {
    if let Some(p) = crate::lsp::in_path("npm") {
        return Ok(p);
    }
    let b = ensure(&NODE)?;
    let p = b.join(if cfg!(windows) { "npm.cmd" } else { "npm" });
    if p.is_file() { Ok(p) } else { Err("npm".into()) }
}

/// go: the system's, else Florm's (downloaded once). On a thread.
pub fn go() -> Result<PathBuf, String> {
    if let Some(p) = crate::lsp::in_path("go") {
        return Ok(p);
    }
    let b = ensure(&GO)?;
    let p = b.join(exe("go"));
    if p.is_file() { Ok(p) } else { Err("go".into()) }
}

// ------------------------------------------------------------------ programs of GitHub releases

fn github_root() -> PathBuf {
    crate::config::data_dir().join("tools").join("github")
}

/// The folders of the programs installed from GitHub releases (written at each install).
fn github_dirs() -> Vec<PathBuf> {
    std::fs::read_to_string(github_root().join("paths.txt")).unwrap_or_default().lines().map(PathBuf::from).filter(|p| p.is_dir()).collect()
}

/// The folder under `dir` holding the program `bin` (searched a few levels down).
fn find_bin(dir: &Path, bin: &str) -> Option<PathBuf> {
    let want = exe(bin);
    let mut level = vec![dir.to_path_buf()];
    for _ in 0..4 {
        let mut next = Vec::new();
        for d in &level {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    next.push(p);
                } else if p.file_name().is_some_and(|n| n.to_string_lossy() == want) {
                    return Some(d.clone());
                }
            }
        }
        level = next;
    }
    None
}

/// Installs program `bin` from the latest release of GitHub repository `repo`: the asset named
/// `asset` (`{version}` replaced by the release's tag without « v »), checked against the SHA-256
/// GitHub gives. A .gz is one program, a .tar.gz / .zip a folder, else the program itself.
pub fn install_github(repo: &str, asset: &str, bin: &str) -> Result<(), String> {
    let api = format!("https://api.github.com/repos/{repo}/releases/latest");
    let out = crate::os::command("curl")
        .args(["-fsSL", "--max-time", "30", "-H", "Accept: application/vnd.github+json", &api])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| crate::t!("curl est introuvable", "curl was not found").to_string())?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| crate::tf!("{repo} : pas de release", "{repo}: no release"))?;
    let tag = v["tag_name"].as_str().unwrap_or("");
    let name = asset.replace("{version}", tag.trim_start_matches('v'));
    let a = v["assets"].as_array().into_iter().flatten().find(|a| a["name"] == name.as_str()).ok_or_else(|| crate::tf!("{repo} {tag} : pas de fichier {name}", "{repo} {tag}: no file {name}"))?;
    let url = a["browser_download_url"].as_str().unwrap_or("");
    let sha = a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).ok_or_else(|| crate::tf!("{repo} : GitHub ne donne pas de somme SHA-256 pour {name} (refusé)", "{repo}: GitHub gives no SHA-256 sum for {name} (refused)"))?;
    let dir = github_root().join(bin);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(&name);
    download_checked(url, &file, sha)?;
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".zip") {
        unpack(&file, &dir)?;
        let _ = std::fs::remove_file(&file);
    } else if let Some(stem) = name.strip_suffix(".gz") {
        let _ = stem;
        let out = crate::os::command("gzip").arg("-df").arg(&file).stdin(Stdio::null()).output().map_err(|_| crate::t!("gzip est introuvable", "gzip was not found").to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        std::fs::rename(dir.join(&name[..name.len() - 3]), dir.join(exe(bin))).map_err(|e| e.to_string())?;
    } else if name != exe(bin) {
        std::fs::rename(&file, dir.join(exe(bin))).map_err(|e| e.to_string())?;
    }
    let at = find_bin(&dir, bin).ok_or_else(|| crate::tf!("{name} : pas de programme {bin} dedans", "{name}: no {bin} program inside"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(at.join(bin), std::fs::Permissions::from_mode(0o755));
    }
    let mut paths: Vec<PathBuf> = github_dirs().into_iter().filter(|p| !p.starts_with(&dir)).collect();
    paths.push(at);
    std::fs::write(github_root().join("paths.txt"), paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n")).map_err(|e| e.to_string())?;
    refresh();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every platform has its archive for each runtime, with a sum of 64 hex digits.
    #[test]
    fn runtimes_cover_the_platforms() {
        for r in [&PYTHON, &NODE, &GO] {
            for p in ["linux-x64", "linux-arm64", "macos-x64", "macos-arm64", "windows-x64", "windows-arm64"] {
                let (_, file, sha) = r.files.iter().find(|(x, ..)| *x == p).unwrap_or_else(|| panic!("{} {p}", r.name));
                assert!(sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit()), "{} {p}", r.name);
                assert!(file.ends_with(".tar.gz") || file.ends_with(".zip"), "{file}");
            }
        }
        assert!(["linux-x64", "linux-arm64", "macos-x64", "macos-arm64", "windows-x64", "windows-arm64"].contains(&platform().as_str()));
    }

    /// A download whose sum is not the expected one is refused (and removed).
    #[test]
    fn a_wrong_sum_is_refused() {
        let dir = std::env::temp_dir().join(format!("florm_rt_sum_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.txt");
        std::fs::write(&src, "hello").unwrap();
        let url = format!("file://{}", src.display());
        let good = format!("{:x}", Sha256::digest(b"hello"));
        if crate::lsp::in_path("curl").is_none() {
            return;
        }
        assert!(download_checked(&url, &dir.join("b"), &good).is_ok());
        assert!(download_checked(&url, &dir.join("c"), &"0".repeat(64)).is_err());
        assert!(!dir.join("c").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `cargo test -- --ignored`: Python and Node downloaded for real, checked, unpacked, run.
    #[test]
    #[ignore]
    fn runtimes_download_for_real() {
        let dir = std::env::temp_dir().join(format!("florm_rt_real_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _lock = crate::config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        ensure(&PYTHON).unwrap();
        let py = managed_python().unwrap();
        let out = crate::os::command(&py).arg("--version").output().unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("Python 3.12"), "{out:?}");
        let b = ensure(&NODE).unwrap();
        let out = crate::os::command(b.join(exe("node"))).arg("--version").output().unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("v26"), "{out:?}");
        install_github("artempyanykh/marksman", if cfg!(target_os = "macos") { "marksman-macos" } else { "marksman-linux-x64" }, "marksman").unwrap();
        assert!(bin_dirs().iter().any(|d| d.join("marksman").is_file()));
        // a .gz holding the program alone
        if platform() == "linux-x64" {
            install_github("rust-lang/rust-analyzer", "rust-analyzer-x86_64-unknown-linux-gnu.gz", "rust-analyzer").unwrap();
            let ra = bin_dirs().into_iter().map(|d| d.join("rust-analyzer")).find(|p| p.is_file()).unwrap();
            let out = crate::os::command(&ra).arg("--version").output().unwrap();
            assert!(String::from_utf8_lossy(&out.stdout).starts_with("rust-analyzer"), "{out:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
