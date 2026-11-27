//! Formatters of the web projects: Biome or Prettier, when the project uses one (its configuration
//! file, or the tool in its node_modules). Run on the text of the editor (stdin), off the UI thread,
//! for Ctrl+Alt+F and format_on_save; before the language server, whose formatting differs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Clone, Debug, PartialEq)]
pub struct Formatter {
    /// "biome" or "prettier".
    pub name: &'static str,
    /// The program and its arguments (the text comes on stdin).
    pub cmd: Vec<String>,
    /// Where it runs: the folder of its configuration (the project).
    pub dir: PathBuf,
}

const PRETTIER_FILES: &[&str] = &[
    ".prettierrc", ".prettierrc.json", ".prettierrc.json5", ".prettierrc.yaml", ".prettierrc.yml", ".prettierrc.toml", ".prettierrc.js", ".prettierrc.cjs",
    ".prettierrc.mjs", ".prettierrc.ts", "prettier.config.js", "prettier.config.cjs", "prettier.config.mjs", "prettier.config.ts",
];

/// The extensions each one formats.
fn biome_ext(ext: &str) -> bool {
    matches!(ext, "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "json" | "jsonc" | "css" | "graphql" | "gql")
}

fn prettier_ext(ext: &str) -> bool {
    biome_ext(ext) || matches!(ext, "scss" | "less" | "html" | "htm" | "vue" | "svelte" | "astro" | "md" | "markdown" | "mdx" | "yaml" | "yml" | "handlebars" | "hbs")
}

/// The formatter of file `path`: the nearest folder (up to `root`, the repository) with the
/// configuration of Biome or Prettier, or with one of them in its node_modules.
pub fn find(path: &Path, root: Option<&Path>) -> Option<Formatter> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    if !prettier_ext(&ext) {
        return None;
    }
    let mut dir = path.parent();
    while let Some(d) = dir {
        let local = |tool: &str| {
            let bin = d.join("node_modules").join(".bin").join(if cfg!(windows) { format!("{tool}.cmd") } else { tool.to_string() });
            bin.is_file().then_some(bin)
        };
        let biome_cfg = d.join("biome.json").is_file() || d.join("biome.jsonc").is_file();
        if biome_ext(&ext) && (biome_cfg || local("biome").is_some()) {
            if let Some(bin) = local("biome").or_else(|| crate::lsp::in_path("biome")) {
                let file = format!("--stdin-file-path={}", path.display());
                return Some(Formatter { name: "biome", cmd: vec![bin.display().to_string(), "format".into(), file], dir: d.to_path_buf() });
            }
        }
        let prettier_cfg = PRETTIER_FILES.iter().any(|f| d.join(f).is_file()) || package_names_prettier(&d.join("package.json"));
        if prettier_cfg || local("prettier").is_some() {
            if let Some(bin) = local("prettier").or_else(|| crate::lsp::in_path("prettier")) {
                return Some(Formatter { name: "prettier", cmd: vec![bin.display().to_string(), "--stdin-filepath".into(), path.display().to_string()], dir: d.to_path_buf() });
            }
        }
        if root.is_some_and(|r| d == r) {
            break;
        }
        dir = d.parent();
    }
    None
}

/// package.json with a "prettier" key (its configuration) or prettier among its dependencies.
fn package_names_prettier(p: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(p) else { return false };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return false };
    v.get("prettier").is_some() || ["dependencies", "devDependencies"].iter().any(|k| v[k].get("prettier").is_some())
}

impl Formatter {
    /// `text` formatted (or the first line of its error).
    pub fn run(&self, text: &str) -> Result<String, String> {
        let mut child = crate::os::command(&self.cmd[0])
            .args(&self.cmd[1..])
            .current_dir(&self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{} : {e}", self.name))?;
        if let Some(mut stdin) = child.stdin.take() {
            let input = text.to_string();
            std::thread::spawn(move || {
                let _ = stdin.write_all(input.as_bytes());
            });
        }
        let o = child.wait_with_output().map_err(|e| format!("{} : {e}", self.name))?;
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        if o.status.success() && (!out.is_empty() || text.trim().is_empty()) {
            return Ok(out);
        }
        let err = String::from_utf8_lossy(&o.stderr);
        let first = err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or(t!("échec", "failed"));
        Err(format!("{} : {first}", self.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_only_where_the_project_uses_it() {
        let root = std::env::temp_dir().join(format!("florm_fmt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let app = root.join("web/src");
        std::fs::create_dir_all(&app).unwrap();
        let file = app.join("App.tsx");
        // nothing in the project: not formatted (even if prettier is installed elsewhere)
        assert_eq!(find(&file, Some(&root)), None);
        // prettier in node_modules of the project
        let bin = root.join("web/node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = |name: &str| {
            let p = bin.join(if cfg!(windows) { format!("{name}.cmd") } else { name.to_string() });
            std::fs::write(&p, "").unwrap();
            p
        };
        let prettier = exe("prettier");
        let f = find(&file, Some(&root)).unwrap();
        assert_eq!((f.name, f.dir.clone(), f.cmd[0].clone()), ("prettier", root.join("web"), prettier.display().to_string()));
        assert!(find(&app.join("x.py"), Some(&root)).is_none());
        // biome configured (and installed) wins for what it formats, prettier keeps the rest
        std::fs::write(root.join("web/biome.json"), "{}").unwrap();
        exe("biome");
        assert_eq!(find(&file, Some(&root)).unwrap().name, "biome");
        assert_eq!(find(&app.join("README.md"), Some(&root)).unwrap().name, "prettier");
        let _ = std::fs::remove_dir_all(&root);
    }
}
