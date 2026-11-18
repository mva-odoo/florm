//! Folders of a WSL distribution opened as projects from Windows: Windows shows the distributions
//! as `\\wsl.localhost\<distro>\…`, and Florm keeps working on these ordinary paths (files, git,
//! search, linters run here). The terminal of such a project is a shell over there
//! (`wsl.exe --cd`) in the same folder: a path is enough to know where it is, nothing else is kept.

use std::path::{Path, PathBuf};
use std::process::Stdio;

/// A machine to connect to, from the palette.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A WSL distribution (Windows).
    Wsl(String),
}

impl Target {
    pub fn label(&self) -> &str {
        match self {
            Target::Wsl(d) => d,
        }
    }
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

/// A folder of a WSL distribution (shown in the title).
pub fn describe(path: &Path) -> Option<String> {
    wsl_location(path).map(|(d, _)| format!("WSL : {d}"))
}

/// The path over there of a folder of a WSL distribution.
pub fn remote_path(path: &Path) -> Option<String> {
    wsl_location(path).map(|(_, p)| p)
}

/// Program and arguments of a terminal in `cwd` when it is a folder of a WSL distribution: a shell
/// over there, in the same folder.
pub fn terminal_argv(cwd: &Path) -> Option<Vec<String>> {
    let (distro, path) = wsl_location(cwd)?;
    Some(vec!["wsl.exe".into(), "-d".into(), distro, "--cd".into(), path])
}

/// The WSL distributions installed (Windows; none elsewhere).
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

/// Runs a command with nothing to type in, its output or its error.
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

/// The home folder of the user in the distribution, as a path Windows reads.
pub fn connect(target: &Target) -> Result<PathBuf, String> {
    let Target::Wsl(distro) = target;
    let mut c = crate::os::command("wsl.exe");
    c.args(["-d", distro, "--cd", "~", "-e", "pwd"]);
    let home = run(c, "wsl.exe").map_err(|e| tf!("WSL {distro} : {e}", "WSL {distro}: {e}"))?;
    let dir = PathBuf::from(format!(r"\\wsl.localhost\{distro}"));
    Ok(home.split('/').filter(|s| !s.is_empty()).fold(dir, |p, s| p.join(s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_paths() {
        assert_eq!(wsl_location(Path::new(r"\\wsl.localhost\Ubuntu\home\me\src")), Some(("Ubuntu".into(), "/home/me/src".into())));
        assert_eq!(wsl_location(Path::new(r"\\WSL$\Debian")), Some(("Debian".into(), "/".into())));
        assert_eq!(wsl_location(Path::new(r"C:\Users\me")), None);
    }

    #[test]
    fn terminal_in_wsl() {
        let wsl = terminal_argv(Path::new(r"\\wsl.localhost\Ubuntu\home\me")).unwrap();
        assert_eq!(wsl, ["wsl.exe", "-d", "Ubuntu", "--cd", "/home/me"]);
        assert_eq!(terminal_argv(Path::new("/home/me")), None);
    }

    #[test]
    fn wsl_list_in_utf16() {
        let bytes: Vec<u8> = "Ubuntu\r\ndocker-desktop\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(parse_wsl_list(&bytes), ["Ubuntu", "docker-desktop"]);
    }
}
