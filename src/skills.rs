//! The skills of the user, in one folder of their choice (a git repository of their own, say),
//! given to every conversation of Claude whatever its account, so that no project has to hold
//! them. Any layout: every `SKILL.md` found in the folder, in sub-folders by theme or not, is a
//! skill. Claude Code wants a plugin (`skills/<name>/SKILL.md`): Florm makes one in its data
//! folder, of links to the skills, and gives it with `--plugin-dir`; Claude sees them as
//! `florm:<name>`. Without a folder chosen: `skills/` of the shared configuration, else of the
//! configuration folder of Florm.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Skill {
    pub name: String,
    pub description: String,
    /// Its SKILL.md.
    pub path: PathBuf,
    /// The folder it is in, from the folder of the skills ("" at the top, "odoo/tests"…).
    pub group: String,
}

/// `skills_dir` of the settings (set by `config::load` and the preferences).
static CHOSEN: Mutex<String> = Mutex::new(String::new());

pub fn set_dir(dir: &str) {
    *CHOSEN.lock().unwrap_or_else(|e| e.into_inner()) = dir.trim().to_string();
}

/// The folder of the skills.
pub fn dir() -> PathBuf {
    let chosen = CHOSEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if !chosen.is_empty() {
        return match chosen.strip_prefix("~/").or_else(|| chosen.strip_prefix("~\\")) {
            Some(rest) => crate::config::home().join(rest),
            None => PathBuf::from(chosen),
        };
    }
    crate::shared::sub("skills").unwrap_or_else(|| crate::config::config_dir().join("skills"))
}

pub fn list() -> Vec<Skill> {
    list_in(&dir())
}

/// Every SKILL.md under `root` (not in hidden folders nor `node_modules`), by group then name.
pub fn list_in(root: &Path) -> Vec<Skill> {
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out.sort_by(|a, b| (&a.group, &a.name).cmp(&(&b.group, &b.name)));
    out
}

fn walk(root: &Path, d: &Path, depth: usize, out: &mut Vec<Skill>) {
    if depth > 8 {
        return;
    }
    let file = d.join("SKILL.md");
    if d != root {
        if let Ok(text) = std::fs::read_to_string(&file) {
            let (name, description) = front(&text);
            let folder = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let group = d.parent().and_then(|p| p.strip_prefix(root).ok()).map(|g| g.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            // `skills/` of the layout of Claude Code is no group
            let group = group.strip_suffix("skills").map(|g| g.trim_end_matches('/').to_string()).unwrap_or(group);
            out.push(Skill { name: name.unwrap_or(folder), description: description.unwrap_or_default(), path: file, group });
            // a skill's own folders (scripts, references) hold no other skill
            return;
        }
    }
    let Ok(rd) = std::fs::read_dir(d) else { return };
    let mut dirs: Vec<PathBuf> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
    dirs.sort();
    for sub in dirs {
        let name = sub.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !name.starts_with('.') && name != "node_modules" {
            walk(root, &sub, depth + 1, out);
        }
    }
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

/// The plugin given to Claude, made again from the folder of the skills; None when there is none.
pub fn plugin_dir() -> Option<PathBuf> {
    make_plugin(&list(), &crate::config::data_dir().join("skills-plugin"))
}

/// The plugin at `out`: its manifest, and in `skills/` one link per skill (a copy where links
/// cannot be made); two skills of the same name get the name of their group in front.
fn make_plugin(skills: &[Skill], out: &Path) -> Option<PathBuf> {
    let links = out.join("skills");
    // the links only go: what they point to stays
    let _ = std::fs::remove_dir_all(&links);
    if skills.is_empty() {
        return None;
    }
    std::fs::create_dir_all(&links).ok()?;
    std::fs::create_dir_all(out.join(".claude-plugin")).ok()?;
    std::fs::write(out.join(".claude-plugin").join("plugin.json"), "{\n  \"name\": \"florm\",\n  \"description\": \"Les skills de Florm / Florm's skills\"\n}\n").ok()?;
    let mut used: Vec<String> = Vec::new();
    for s in skills {
        let slug = |t: &str| t.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect::<String>();
        let mut name = slug(&s.name);
        if used.contains(&name) {
            name = slug(&format!("{}-{}", s.group.replace('/', "-"), s.name));
        }
        if name.is_empty() || used.contains(&name) {
            continue;
        }
        let Some(src) = s.path.parent() else { continue };
        let dst = links.join(&name);
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(src, &dst).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(src, &dst).is_ok();
        if !linked && copy_dir(src, &dst).is_err() {
            continue;
        }
        used.push(name);
    }
    Some(out.to_path_buf())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()))?;
        } else {
            std::fs::copy(&p, to.join(e.file_name()))?;
        }
    }
    Ok(())
}

/// A new skill `name` (lowercase, dashes) at the top of `root`, with a SKILL.md to fill; returns
/// its file (the one there already, when it exists).
pub fn create_in(root: &Path, name: &str) -> Result<PathBuf, String> {
    let slug: String = name.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        return Err(crate::t!("Nom vide", "Empty name").into());
    }
    let folder = root.join(&slug);
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
    fn skills_in_any_layout_given_as_a_plugin() {
        let base = std::env::temp_dir().join(format!("florm_skills_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, out) = (base.join("mes-skills"), base.join("plugin"));
        // no skill: no plugin given to Claude
        assert!(make_plugin(&list_in(&root), &out).is_none());
        create_in(&root, "Notes").unwrap();
        // by theme, deeper, the layout of Claude Code, and two of the same name
        for (dir, name) in [("odoo/tests-odoo", "tests-odoo"), ("odoo/migration/scripts-mig", "scripts-mig"), ("perso/skills/rapport", "rapport"), ("perso/notes", "notes")] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join("SKILL.md"), format!("---\nname: {name}\ndescription: d\n---\nx")).unwrap();
        }
        // a folder of a skill (its scripts) and a hidden one are not looked into
        std::fs::create_dir_all(root.join("odoo/tests-odoo/sub")).unwrap();
        std::fs::write(root.join("odoo/tests-odoo/sub/SKILL.md"), "---\nname: no\n---").unwrap();
        std::fs::create_dir_all(root.join(".git/x")).unwrap();
        std::fs::write(root.join(".git/x/SKILL.md"), "---\nname: no\n---").unwrap();
        let s = list_in(&root);
        let names: Vec<(String, String)> = s.iter().map(|s| (s.group.clone(), s.name.clone())).collect();
        assert_eq!(names, [("", "notes"), ("odoo", "tests-odoo"), ("odoo/migration", "scripts-mig"), ("perso", "notes"), ("perso", "rapport")].map(|(g, n)| (g.to_string(), n.to_string())));
        assert_eq!(make_plugin(&s, &out), Some(out.clone()));
        assert!(std::fs::read_to_string(out.join(".claude-plugin/plugin.json")).unwrap().contains("\"florm\""));
        for n in ["notes", "perso-notes", "tests-odoo", "scripts-mig", "rapport"] {
            assert!(out.join("skills").join(n).join("SKILL.md").is_file(), "{n}");
        }
        // made again: a skill removed goes, the skills themselves stay
        std::fs::remove_dir_all(root.join("perso")).unwrap();
        make_plugin(&list_in(&root), &out);
        assert!(!out.join("skills/rapport").exists() && root.join("odoo/tests-odoo/SKILL.md").is_file());
        assert_eq!(front("---\nname: a\ndescription: \"b c\"\n---\nx"), (Some("a".into()), Some("b c".into())));
        let _ = std::fs::remove_dir_all(&base);
    }
}
