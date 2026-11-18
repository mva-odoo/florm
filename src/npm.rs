//! A package of npm's registry downloaded into a folder of Florm (the Copilot server, the word
//! lists of the spell checker), checked against the SHA-512 npm gives, unpacked with `tar`.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// `package` at `version` ("latest", "2.0.0") unpacked into `dir` (emptied first); returns the
/// folder of its files (`dir/package`). `max_secs`: the longest the download may take.
pub fn fetch(package: &str, version: &str, dir: &Path, max_secs: u32) -> Result<PathBuf, String> {
    let meta: Value = serde_json::from_str(&curl_text(&format!("https://registry.npmjs.org/{package}/{version}"))?).map_err(|e| e.to_string())?;
    let tarball = meta["dist"]["tarball"].as_str().ok_or("npm : pas de fichier")?.to_string();
    let integrity = meta["dist"]["integrity"].as_str().unwrap_or("").to_string();
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = dir.join("package.tgz");
    let out = crate::os::command("curl")
        .args(["-fsSL", "--max-time", &max_secs.to_string(), "-o"])
        .arg(&file)
        .arg(&tarball)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| crate::t!("curl est introuvable", "curl was not found").to_string())?;
    if !out.status.success() {
        return Err(crate::tf!("Téléchargement impossible : {}", "Cannot download: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    // the SHA-512 npm gives ("sha512-<base64>"): a broken or changed download is not used
    let want = integrity.strip_prefix("sha512-").and_then(crate::claude::base64_decode).ok_or_else(|| crate::t!("npm ne donne pas d'empreinte pour ce fichier", "npm gives no checksum for this file").to_string())?;
    use sha2::{Digest, Sha512};
    let got = Sha512::digest(std::fs::read(&file).map_err(|e| e.to_string())?);
    if got.as_slice() != want.as_slice() {
        let _ = std::fs::remove_dir_all(dir);
        return Err(crate::t!("Le fichier téléchargé est abîmé (empreinte différente) : réessaie", "The downloaded file is damaged (different checksum): try again").into());
    }
    let out = crate::os::command("tar").arg("-xzf").arg(&file).arg("-C").arg(dir).stdin(Stdio::null()).output().map_err(|_| crate::t!("tar est introuvable", "tar was not found").to_string())?;
    let _ = std::fs::remove_file(&file);
    if !out.status.success() {
        return Err(crate::tf!("Décompression impossible : {}", "Cannot unpack: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(dir.join("package"))
}

fn curl_text(url: &str) -> Result<String, String> {
    let out = crate::os::command("curl")
        .args(["-fsSL", "--max-time", "30", url])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| crate::t!("curl est introuvable", "curl was not found").to_string())?;
    if !out.status.success() {
        return Err(crate::tf!("Le registre npm ne répond pas : {}", "npm's registry does not answer: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
