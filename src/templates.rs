//! Templates (snippets): blocks of code inserted at the cursor, e.g. a Python breakpoint.
//! Built-in ones plus the files of the `templates` folder of the configuration (file name = name
//! of the template, extension = language). `$0` is where the cursor goes.
//! A template with a trigger (`ipdb`) is also offered by the completion, and Tab after the
//! word expands it, like the snippets of Sublime Text.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Template {
    pub name: String,
    /// Extension of the files it is meant for ("xml", "py"…); empty: any.
    pub ext: String,
    /// Word that expands it ("ipdb"); empty: only in the list of templates.
    pub trigger: String,
    pub body: String,
    /// File of a user template (None: built-in).
    pub path: Option<PathBuf>,
}

/// (name, extension, trigger, body)
const BUILTIN: &[(&str, &str, &str, &str)] = &[
    ("Python : ipdb", "py", "ipdb", "import ipdb; ipdb.set_trace()$0"),
    ("Python : pdb", "py", "pdb", "import pdb; pdb.set_trace()$0"),
];

const README: &str = "# Modèles Florm\n\n\
Chaque fichier de ce dossier est un modèle : son nom devient le nom du modèle, son extension\n\
le type de fichier pour lequel il est proposé en premier (ex. « Mon test.py »).\n\n\
Un nom d'un seul mot (lettres, chiffres, _) sert aussi de déclencheur : avec « ipdb.py », taper\n\
`ipdb` dans un fichier .py le propose dans les suggestions, et Tab le développe.\n\n\
Pour remplacer un modèle intégré, donne au fichier le même nom, avec « · » à la place de « : »\n\
(interdit dans un nom de fichier sous Windows) : « Python · pdb.py » remplace « Python : pdb ».\n\n\
`$0` marque la position du curseur après l'insertion.\n";

/// Where new templates are written: the shared configuration when there is one.
pub fn dir() -> PathBuf {
    crate::shared::sub("templates").unwrap_or_else(|| crate::config::config_dir().join("templates"))
}

/// Built-in, local and shared templates (the shared ones win).
pub fn load_all() -> Vec<Template> {
    let dirs: Vec<PathBuf> = std::iter::once(crate::config::config_dir().join("templates")).chain(crate::shared::sub("templates")).collect();
    load_dirs(&dirs)
}

/// Creates the folder with its README the first time.
pub fn install_readme(dir: &Path) {
    if !dir.exists() && std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(dir.join("README.md"), README);
    }
}

/// Built-in templates, then the user's (a user template with the same name replaces the built-in one).
#[cfg(test)]
pub fn load(dir: &Path) -> Vec<Template> {
    load_dirs(&[dir.to_path_buf()])
}

/// Built-in templates, then those of `dirs` in order (a later one with the same name wins).
fn load_dirs(dirs: &[PathBuf]) -> Vec<Template> {
    let mut v: Vec<Template> = BUILTIN
        .iter()
        .map(|(n, e, tr, b)| Template { name: n.to_string(), ext: e.to_string(), trigger: tr.to_string(), body: b.to_string(), path: None })
        .collect();
    for rd in dirs.iter().filter_map(|d| std::fs::read_dir(d).ok()) {
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for p in files {
            if p.file_name().is_some_and(|n| n == "README.md") {
                continue;
            }
            let Ok(body) = std::fs::read_to_string(&p) else { continue };
            let name = name_of(&p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            let ext = p.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            let trigger = if is_word(&name) { name.clone() } else { String::new() };
            // a user template with a built-in's trigger replaces it too
            v.retain(|t| t.name != name && (trigger.is_empty() || t.trigger != trigger));
            v.push(Template { name, ext, trigger, body, path: Some(p) });
        }
    }
    v
}

/// The file name of template `name`: « : » (refused in a file name on Windows) becomes « · ».
pub fn file_stem(name: &str) -> String {
    name.replace(':', "·")
}

/// The name of the template of file stem `stem` (« Python · pdb » or, written by hand on Linux
/// or macOS, « Python : pdb »).
fn name_of(stem: &str) -> String {
    stem.replace('·', ":")
}

fn is_word(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Templates whose trigger starts with `prefix` (any case), for a file of extension `ext`
/// (none: a new file, all of them).
pub fn with_trigger<'a>(all: &'a [Template], ext: &str, prefix: &str) -> impl Iterator<Item = &'a Template> {
    let (lower, ext) = (prefix.to_lowercase(), ext.to_string());
    all.iter().filter(move |t| {
        !t.trigger.is_empty() && (ext.is_empty() || t.ext.is_empty() || t.ext == ext) && t.trigger.to_lowercase().starts_with(&lower)
    })
}

/// The text to insert: every line after the first indented with `indent`. Returns it with the
/// position of `$0` (in chars), or its end.
pub fn expand(body: &str, indent: &str) -> (String, usize) {
    let t = body.replace("\r\n", "\n");
    // inserted on a line that already has its indentation: only the next lines get it
    let t = t.strip_suffix('\n').map(str::to_string).unwrap_or(t);
    let t: String = t.split('\n').enumerate().map(|(i, l)| if i == 0 || l.is_empty() { l.to_string() } else { format!("{indent}{l}") }).collect::<Vec<_>>().join("\n");
    match t.find("$0") {
        Some(b) => {
            let cursor = t[..b].chars().count();
            (t.replacen("$0", "", 1), cursor)
        }
        None => {
            let n = t.chars().count();
            (t, n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_with_indent_and_cursor() {
        let (t, c) = expand("<item id=\"a\"\n    parent=\"$0\"/>\n", "    ");
        assert_eq!(t, "<item id=\"a\"\n        parent=\"\"/>");
        assert_eq!(&t.chars().take(c).collect::<String>(), "<item id=\"a\"\n        parent=\"");
        // the built-in ones all have a cursor
        for tpl in load(Path::new("/nonexistent")) {
            let (t, _) = expand(&tpl.body, "");
            assert!(!t.contains("$0"), "{}", tpl.name);
        }
    }

    #[test]
    fn user_templates_replace_builtin_ones() {
        let dir = std::env::temp_dir().join(format!("florm_tpl_user_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        install_readme(&dir);
        // « · » for « : », refused in a file name on Windows
        std::fs::write(dir.join("Python · pdb.py"), "breakpoint()").unwrap();
        std::fs::write(dir.join("Card.html"), "<div/>").unwrap();
        let all = load(&dir);
        assert_eq!(all.iter().filter(|t| t.name == "Python : pdb").count(), 1);
        assert_eq!(all.iter().find(|t| t.name == "Python : pdb").unwrap().body, "breakpoint()");
        // the files written with « : » on Linux or macOS still replace it
        if cfg!(unix) {
            std::fs::write(dir.join("Python : ipdb.py"), "breakpoint()").unwrap();
            let all = load(&dir);
            assert_eq!(all.iter().filter(|t| t.name == "Python : ipdb").map(|t| t.body.as_str()).collect::<Vec<_>>(), ["breakpoint()"]);
            std::fs::remove_file(dir.join("Python : ipdb.py")).unwrap();
        }
        assert!(all.iter().any(|t| t.name == "Card" && t.ext == "html"));
        assert!(!all.iter().any(|t| t.name == "README"));
        assert_eq!(all.iter().find(|t| t.name == "Card").unwrap().trigger, "Card");
        std::fs::write(dir.join("ipdb.py"), "breakpoint()").unwrap();
        let all = load(&dir);
        let hits: Vec<&Template> = with_trigger(&all, "py", "ipd").collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].body, "breakpoint()");
        assert_eq!(with_trigger(&all, "xml", "ipdb").count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
