//! The skills of the user, in one folder that Florm gives to every conversation of Claude,
//! whatever its account: a plugin named "florm" (`claude --plugin-dir`), each skill in
//! `skills/<name>/SKILL.md`. In the shared configuration when there is one (the same skills on
//! every computer), else in the configuration folder of Florm.

use std::path::{Path, PathBuf};

pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

/// The folder of the skills (the root of the plugin).
pub fn dir() -> PathBuf {
    crate::shared::sub("skills").unwrap_or_else(|| crate::config::config_dir().join("skills"))
}

/// The skills of `root`, by name.
pub fn list_in(root: &Path) -> Vec<Skill> {
    let mut out: Vec<Skill> = std::fs::read_dir(root.join("skills"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path().join("SKILL.md");
            let text = std::fs::read_to_string(&path).ok()?;
            let (name, description) = front(&text);
            Some(Skill { name: name.unwrap_or_else(|| e.file_name().to_string_lossy().into_owned()), description: description.unwrap_or_default(), path })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn list() -> Vec<Skill> {
    list_in(&dir())
}

/// `name` and `description` of the front matter of a SKILL.md.
fn front(text: &str) -> (Option<String>, Option<String>) {
    let Some(rest) = text.strip_prefix("---") else { return (None, None) };
    let block = rest.split("\n---").next().unwrap_or("");
    let get = |key: &str| {
        block.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':').map(|v| v.trim().trim_matches(['"', '\'']).to_string())).filter(|v| !v.is_empty())
    };
    (get("name"), get("description"))
}

/// The folder given to Claude as a plugin (its manifest written when missing); None when it
/// holds no skill.
pub fn plugin_dir() -> Option<PathBuf> {
    plugin_in(&dir())
}

fn plugin_in(root: &Path) -> Option<PathBuf> {
    if list_in(root).is_empty() {
        return None;
    }
    let manifest = root.join(".claude-plugin").join("plugin.json");
    if !manifest.is_file() {
        std::fs::create_dir_all(manifest.parent()?).ok()?;
        std::fs::write(&manifest, "{\n  \"name\": \"florm\",\n  \"description\": \"Les skills de Florm / Florm's skills\"\n}\n").ok()?;
    }
    Some(root.to_path_buf())
}

/// A new skill `name` (lowercase, dashes) with a SKILL.md to fill; returns its file.
pub fn create_in(root: &Path, name: &str) -> Result<PathBuf, String> {
    let slug: String = name.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        return Err(crate::t!("Nom vide", "Empty name").into());
    }
    let folder = root.join("skills").join(&slug);
    let file = folder.join("SKILL.md");
    if file.exists() {
        return Ok(file);
    }
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let title = name.trim();
    let body = crate::tf!(
        "---\nname: {slug}\ndescription: Ce que fait ce skill et quand Claude doit s'en servir (une ou deux phrases : c'est ce qu'il lit pour le choisir).\n---\n\n# {title}\n\nLes consignes pour Claude : les commandes à lancer, où sont les choses, ce qu'il faut éviter.\n",
        "---\nname: {slug}\ndescription: What this skill does and when Claude should use it (one or two sentences: it is what it reads to pick it).\n---\n\n# {title}\n\nThe instructions for Claude: the commands to run, where things are, what to avoid.\n",
    );
    std::fs::write(&file, body).map_err(|e| e.to_string())?;
    Ok(file)
}

pub fn create(name: &str) -> Result<PathBuf, String> {
    create_in(&dir(), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_of_a_folder_as_a_plugin() {
        let root = std::env::temp_dir().join(format!("florm_skills_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // no skill: no plugin given to Claude
        assert!(plugin_in(&root).is_none());
        let file = create_in(&root, "Tests Odoo").unwrap();
        assert_eq!(file, root.join("skills/tests-odoo/SKILL.md"));
        let s = list_in(&root);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "tests-odoo");
        assert!(!s[0].description.is_empty());
        assert_eq!(plugin_in(&root), Some(root.clone()));
        assert!(std::fs::read_to_string(root.join(".claude-plugin/plugin.json")).unwrap().contains("\"florm\""));
        assert_eq!(front("---\nname: a\ndescription: \"b c\"\n---\nx"), (Some("a".into()), Some("b c".into())));
        let _ = std::fs::remove_dir_all(&root);
    }
}
