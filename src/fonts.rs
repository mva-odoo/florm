//! Fonts installed on the system: listing (for the preferences) and loading the chosen editor /
//! interface fonts into egui (in front of the built-in ones, which stay as fallbacks).

use eframe::egui::{FontData, FontDefinitions, FontFamily};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FontInfo {
    pub family: String,
    pub path: PathBuf,
    /// Face index in a collection (.ttc).
    pub index: u32,
    pub mono: bool,
}

/// Large files (CJK fonts…) are skipped: slow to read and never wanted for code.
const MAX_FONT_FILE: u64 = 25_000_000;

fn font_dirs() -> Vec<PathBuf> {
    let home = crate::config::home();
    if cfg!(windows) {
        let mut v = vec![PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join("Fonts")];
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            v.push(PathBuf::from(local).join(r"Microsoft\Windows\Fonts"));
        }
        v
    } else if cfg!(target_os = "macos") {
        vec!["/System/Library/Fonts".into(), "/Library/Fonts".into(), home.join("Library/Fonts")]
    } else {
        vec!["/usr/share/fonts".into(), "/usr/local/share/fonts".into(), home.join(".local/share/fonts"), home.join(".fonts")]
    }
}

fn font_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                font_files(&p, depth - 1, out);
            }
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_lowercase().as_str(), "ttf" | "otf" | "ttc")) {
            out.push(p);
        }
    }
}

/// Faces of a font file: (family, index, monospace, regular).
fn faces(data: &[u8]) -> Vec<(String, u32, bool, bool)> {
    let n = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    (0..n)
        .filter_map(|i| {
            let face = ttf_parser::Face::parse(data, i).ok()?;
            let names = face.names();
            let name = |id: u16| names.into_iter().filter(|n| n.name_id == id && n.is_unicode()).find_map(|n| n.to_string());
            let family = name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| name(ttf_parser::name_id::FAMILY))?;
            Some((family, i, face.is_monospaced(), face.is_regular()))
        })
        .collect()
}

/// One entry per family (its regular face when there is one), sorted by name.
pub fn scan() -> Vec<FontInfo> {
    let mut files = Vec::new();
    for d in font_dirs() {
        font_files(&d, 4, &mut files);
    }
    let mut by_family: BTreeMap<String, (FontInfo, bool)> = BTreeMap::new();
    for p in files {
        if std::fs::metadata(&p).map_or(true, |m| m.len() > MAX_FONT_FILE) {
            continue;
        }
        let Ok(data) = std::fs::read(&p) else { continue };
        for (family, index, mono, regular) in faces(&data) {
            let info = FontInfo { family: family.clone(), path: p.clone(), index, mono };
            match by_family.get(&family) {
                Some((_, true)) => {}
                Some(_) if !regular => {}
                _ => {
                    by_family.insert(family, (info, regular));
                }
            }
        }
    }
    by_family.into_values().map(|(f, _)| f).collect()
}

fn cache_path() -> PathBuf {
    crate::config::config_dir().join("fonts.toml")
}

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    #[serde(default)]
    fonts: Vec<FontInfo>,
}

/// Usual file of a few common fonts (avoids scanning the whole system at the first start).
fn known_file(family: &str) -> Option<PathBuf> {
    let file = match family {
        "Consolas" => "consola.ttf",
        "Cascadia Mono" => "CascadiaMono.ttf",
        "Cascadia Code" => "CascadiaCode.ttf",
        "Segoe UI" => "segoeui.ttf",
        _ => return None,
    };
    font_dirs().into_iter().map(|d| d.join(file)).find(|p| p.is_file())
}

/// The font of a family: from the cache of the last choices, else by scanning the system.
pub fn find(family: &str) -> Option<FontInfo> {
    if let Some(p) = known_file(family) {
        return Some(FontInfo { family: family.to_string(), path: p, index: 0, mono: family != "Segoe UI" });
    }
    let mut cache: Cache = std::fs::read_to_string(cache_path()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();
    if let Some(f) = cache.fonts.iter().find(|f| f.family == family && f.path.is_file()) {
        return Some(f.clone());
    }
    let f = scan().into_iter().find(|f| f.family.eq_ignore_ascii_case(family))?;
    cache.fonts.retain(|c| c.family != f.family);
    cache.fonts.push(f.clone());
    if let Ok(text) = toml::to_string(&cache) {
        let _ = std::fs::create_dir_all(crate::config::config_dir());
        let _ = std::fs::write(cache_path(), text);
    }
    Some(f)
}

/// Fonts used when the settings name none: the system's screen fonts on Windows, else the built-in ones.
pub fn default_editor_font() -> &'static [&'static str] {
    if cfg!(windows) {
        &["Cascadia Mono", "Consolas"]
    } else {
        &[]
    }
}

pub fn default_ui_font() -> &'static [&'static str] {
    if cfg!(windows) {
        &["Segoe UI"]
    } else {
        &[]
    }
}

/// A Nerd Font installed on the system, for the icons of shell prompts (starship, powerline
/// arrows, logos…) that are in the private use area of Unicode: added after the other fonts,
/// only the glyphs they lack come from it. Found by file name (no font is read): the symbols
/// only font first, then a monospaced one.
fn nerd_font_file() -> Option<PathBuf> {
    let mut files = Vec::new();
    for d in font_dirs() {
        font_files(&d, 4, &mut files);
    }
    let rank = |p: &PathBuf| {
        let n = p.file_name()?.to_string_lossy().to_lowercase();
        if !n.contains("nerdfont") || !n.contains("-regular.") || n.contains("propo") {
            return None;
        }
        Some(if n.starts_with("symbolsnerdfontmono") {
            0
        } else if n.starts_with("symbolsnerdfont") {
            1
        } else if n.contains("nerdfontmono-") {
            2
        } else {
            3
        })
    };
    files.into_iter().filter_map(|p| Some((rank(&p)?, p))).min().map(|(_, p)| p)
}

/// egui fonts with the chosen editor (monospace) and interface fonts first.
pub fn definitions(editor: &str, ui: &str) -> FontDefinitions {
    let mut defs = crate::app::font_definitions();
    let load = |names: Vec<&str>| names.into_iter().filter(|n| !n.trim().is_empty()).find_map(|n| {
        let f = find(n.trim())?;
        let bytes = std::fs::read(&f.path).ok()?;
        let mut data = FontData::from_owned(bytes);
        data.index = f.index;
        Some(data)
    });
    let editor_names: Vec<&str> = if editor.trim().is_empty() { default_editor_font().to_vec() } else { vec![editor] };
    if let Some(d) = load(editor_names) {
        defs.font_data.insert("user_mono".into(), d);
        defs.families.entry(FontFamily::Monospace).or_default().insert(0, "user_mono".into());
    }
    let ui_names: Vec<&str> = if ui.trim().is_empty() { default_ui_font().to_vec() } else { vec![ui] };
    if let Some(d) = load(ui_names) {
        defs.font_data.insert("user_ui".into(), d);
        defs.families.entry(FontFamily::Proportional).or_default().insert(0, "user_ui".into());
    }
    if let Some(bytes) = nerd_font_file().and_then(|p| std::fs::read(p).ok()) {
        defs.font_data.insert("nerd_symbols".into(), FontData::from_owned(bytes));
        for family in [FontFamily::Monospace, FontFamily::Proportional] {
            defs.families.entry(family).or_default().push("nerd_symbols".into());
        }
    }
    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_nerd_font_by_its_file_name() {
        // only checks the choice when the system has one
        if let Some(p) = nerd_font_file() {
            let n = p.file_name().unwrap().to_string_lossy().to_lowercase();
            assert!(n.contains("nerdfont") && n.contains("-regular.") && !n.contains("propo"), "{n}");
        }
    }

    #[test]
    fn reads_font_names() {
        // the fonts shipped with egui: Hack is monospaced, Ubuntu is not
        let dir = std::env::var("CARGO_HOME").map(PathBuf::from).unwrap_or_else(|_| crate::config::home().join(".cargo"));
        let mut files = Vec::new();
        font_files(&dir.join("registry/src"), 3, &mut files);
        let hack = files.iter().find(|p| p.ends_with("Hack-Regular.ttf") && p.to_string_lossy().contains("epaint_default_fonts"));
        let Some(hack) = hack else { return };
        let f = faces(&std::fs::read(hack).unwrap());
        assert_eq!(f[0].0, "Hack");
        assert!(f[0].2 && f[0].3);
        let ubuntu = hack.with_file_name("Ubuntu-Light.ttf");
        let f = faces(&std::fs::read(ubuntu).unwrap());
        assert!(f[0].0.starts_with("Ubuntu") && !f[0].2);
    }
}

#[cfg(test)]
mod bench {
    /// `FLORM_BENCH_FONTS=1 cargo test --release bench_fonts -- --nocapture`
    #[test]
    fn bench_fonts() {
        if std::env::var("FLORM_BENCH_FONTS").is_err() {
            return;
        }
        let t = std::time::Instant::now();
        let v = super::scan();
        eprintln!("{} familles ({} à chasse fixe) en {:?}", v.len(), v.iter().filter(|f| f.mono).count(), t.elapsed());
        eprintln!("{:?}", v.iter().map(|f| f.family.as_str()).take(12).collect::<Vec<_>>());
    }
}

#[cfg(test)]
mod windows_fonts {
    /// `FLORM_WIN_FONTS=/mnt/c/Windows/Fonts cargo test --release windows_fonts -- --nocapture`
    #[test]
    fn windows_default_fonts() {
        let Ok(dir) = std::env::var("FLORM_WIN_FONTS") else { return };
        for (file, family, mono) in [("consola.ttf", "Consolas", true), ("segoeui.ttf", "Segoe UI", false)] {
            let data = std::fs::read(std::path::Path::new(&dir).join(file)).unwrap();
            let f = super::faces(&data);
            eprintln!("{file}: {:?}", f);
            assert_eq!((f[0].0.as_str(), f[0].2), (family, mono));
        }
    }
}
