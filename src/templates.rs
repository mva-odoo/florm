//! Templates (snippets): blocks of code inserted at the cursor, e.g. a Python breakpoint.
//! Built-in ones plus the files of the `templates` folder of the configuration (file name = name
//! of the template, extension = language). The body is a snippet as in VS Code: `$1`, `$2`… (or
//! `${1:text}`, `${1|a,b|}`) are the places Tab goes to in turn, the same number at several places
//! typed at once, `$0` where the cursor ends.
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
    ("Python : main", "py", "ifmain", "if __name__ == \"__main__\":\n    ${1:main()}$0"),
    // Odoo: Tab goes from one place to fill to the next
    (
        "Odoo : modèle",
        "py",
        "omodel",
        "class ${1:Name}(models.Model):\n    _name = \"${2:module.name}\"\n    _description = \"${3:Description}\"\n\n    name = fields.Char(required=True)\n    $0",
    ),
    ("Odoo : champ Many2one", "py", "om2o", "${1:partner_id} = fields.Many2one(\"${2:res.partner}\", string=\"${3:Partner}\")$0"),
    (
        "Odoo : champ calculé",
        "py",
        "ocompute",
        "${1:total} = fields.${2|Float,Integer,Char,Monetary,Boolean|}(compute=\"_compute_$1\")\n\n@api.depends(\"${3:line_ids}\")\ndef _compute_$1(self):\n    for rec in self:\n        rec.$1 = $0",
    ),
    (
        "Odoo : vue formulaire",
        "xml",
        "oform",
        "<record id=\"${1:model}_view_form\" model=\"ir.ui.view\">\n    <field name=\"name\">${2:model.name}.form</field>\n    <field name=\"model\">$2</field>\n    <field name=\"arch\" type=\"xml\">\n        <form>\n            <sheet>\n                <group>\n                    <field name=\"${3:name}\"/>$0\n                </group>\n            </sheet>\n        </form>\n    </field>\n</record>",
    ),
];

const README: &str = "# Modèles Florm\n\n\
Chaque fichier de ce dossier est un modèle : son nom devient le nom du modèle, son extension\n\
le type de fichier pour lequel il est proposé en premier (ex. « Mon test.py »).\n\n\
Un nom d'un seul mot (lettres, chiffres, _) sert aussi de déclencheur : avec « ipdb.py », taper\n\
`ipdb` dans un fichier .py le propose dans les suggestions, et Tab le développe.\n\n\
Pour remplacer un modèle intégré, donne au fichier le même nom, avec « · » à la place de « : »\n\
(interdit dans un nom de fichier sous Windows) : « Python · pdb.py » remplace « Python : pdb ».\n\n\
`$1`, `$2`… marquent les endroits à remplir, dans l'ordre : Tab passe au suivant, Maj+Tab revient.\n\
`${1:texte}` y met un texte déjà sélectionné, `${1|a,b|}` le premier choix. Le même numéro à\n\
plusieurs endroits s'écrit partout à la fois. `$0` marque la position du curseur à la fin.\n\
`$TM_FILENAME_BASE` donne le nom du fichier sans extension, `$TM_FILENAME` avec ;\n\
`\\$` écrit un $ (les autres `$NOM` restent tels quels).\n";

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

/// A snippet ready to insert: its text and its tab stops, in the order Tab visits them (`$1`,
/// `$2`… then `$0`, or the end, last). Each stop is the char ranges of its number in the text.
#[derive(Debug, PartialEq, Default)]
pub struct Expanded {
    pub text: String,
    pub stops: Vec<Vec<(usize, usize)>>,
}

impl Expanded {
    /// Where the cursor goes first: the first stop (its text selected).
    pub fn first(&self) -> (usize, usize) {
        self.stops.first().and_then(|s| s.first()).copied().unwrap_or_else(|| {
            let n = self.text.chars().count();
            (n, n)
        })
    }

    /// Whether Tab has places to go to after the first one.
    pub fn has_steps(&self) -> bool {
        self.stops.len() > 1
    }
}

/// What a snippet's variables give: the name of the file (with and without its extension).
#[derive(Default, Clone)]
pub struct Vars {
    pub file_name: String,
}

/// The text to insert: every line after the first indented with `indent`, the snippet syntax
/// read (see `Expanded`).
pub fn expand(body: &str, indent: &str, vars: &Vars) -> Expanded {
    let t = body.replace("\r\n", "\n");
    // inserted on a line that already has its indentation: only the next lines get it
    let t = t.strip_suffix('\n').map(str::to_string).unwrap_or(t);
    let t: String = t.split('\n').enumerate().map(|(i, l)| if i == 0 || l.is_empty() { l.to_string() } else { format!("{indent}{l}") }).collect::<Vec<_>>().join("\n");
    let chars: Vec<char> = t.chars().collect();
    // the text of each number (its first placeholder with one), copied by its bare mirrors
    let mut defaults = std::collections::HashMap::new();
    let mut probe = Parser { chars: &chars, i: 0, out: String::new(), n: 0, marks: Vec::new(), defaults: &std::collections::HashMap::new(), vars };
    probe.run(None);
    for (num, a, b) in &probe.marks {
        if b > a {
            defaults.entry(*num).or_insert_with(|| probe.out.chars().skip(*a).take(b - a).collect::<String>());
        }
    }
    let mut p = Parser { chars: &chars, i: 0, out: String::new(), n: 0, marks: Vec::new(), defaults: &defaults, vars };
    p.run(None);
    let mut nums: Vec<u32> = p.marks.iter().map(|m| m.0).filter(|&n| n > 0).collect();
    nums.sort_unstable();
    nums.dedup();
    let mut stops: Vec<Vec<(usize, usize)>> = nums.iter().map(|n| p.marks.iter().filter(|m| m.0 == *n).map(|m| (m.1, m.2)).collect()).collect();
    let end = p.marks.iter().find(|m| m.0 == 0).map_or((p.n, p.n), |m| (m.1, m.2));
    stops.push(vec![end]);
    Expanded { text: p.out, stops }
}

/// Reads the snippet syntax of VS Code (a subset: tab stops, placeholders, choices, a few variables).
struct Parser<'a> {
    chars: &'a [char],
    i: usize,
    out: String,
    /// Chars written to `out`.
    n: usize,
    /// (number, start, end) of each tab stop, in chars of `out`.
    marks: Vec<(u32, usize, usize)>,
    defaults: &'a std::collections::HashMap<u32, String>,
    vars: &'a Vars,
}

impl Parser<'_> {
    fn push(&mut self, c: char) {
        self.out.push(c);
        self.n += 1;
    }

    fn push_str(&mut self, s: &str) {
        for c in s.chars() {
            self.push(c);
        }
    }

    fn peek(&self, k: usize) -> Option<char> {
        self.chars.get(self.i + k).copied()
    }

    /// Up to the end, or to the `}` closing a placeholder (`close`), which it consumes.
    fn run(&mut self, close: Option<char>) {
        while let Some(c) = self.peek(0) {
            if Some(c) == close {
                self.i += 1;
                return;
            }
            match c {
                '\\' if matches!(self.peek(1), Some('$' | '}' | '\\')) => {
                    self.push(self.chars[self.i + 1]);
                    self.i += 2;
                }
                '$' => self.dollar(),
                _ => {
                    self.push(c);
                    self.i += 1;
                }
            }
        }
    }

    fn digits(&mut self) -> Option<u32> {
        let start = self.i;
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        (self.i > start).then(|| self.chars[start..self.i].iter().collect::<String>().parse().unwrap_or(0))
    }

    fn name(&mut self) -> String {
        let start = self.i;
        while self.peek(0).is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            self.i += 1;
        }
        self.chars[start..self.i].iter().collect()
    }

    /// The value of a known variable (None: unknown, kept as written).
    fn var(&self, name: &str) -> Option<String> {
        let f = &self.vars.file_name;
        match name {
            "TM_FILENAME" => Some(f.clone()),
            "TM_FILENAME_BASE" => Some(f.split('.').next().unwrap_or("").to_string()),
            "TM_SELECTED_TEXT" | "SELECTION" => Some(String::new()),
            _ => None,
        }
    }

    fn dollar(&mut self) {
        let at = self.i;
        self.i += 1;
        let start = self.n;
        if let Some(num) = self.digits() {
            // a bare mirror: the text of its number
            if let Some(d) = self.defaults.get(&num).cloned() {
                self.push_str(&d);
            }
            self.marks.push((num, start, self.n));
            return;
        }
        if self.peek(0) == Some('{') {
            self.i += 1;
            if let Some(num) = self.digits() {
                match self.peek(0) {
                    Some('}') => {
                        self.i += 1;
                        if let Some(d) = self.defaults.get(&num).cloned() {
                            self.push_str(&d);
                        }
                    }
                    Some(':') => {
                        self.i += 1;
                        self.run(Some('}'));
                    }
                    Some('|') => {
                        // a choice: the first one
                        self.i += 1;
                        let rest: String = self.chars[self.i..].iter().collect();
                        let end = rest.find("|}").map_or(rest.chars().count(), |b| rest[..b].chars().count());
                        let first: String = self.chars[self.i..self.i + end].iter().collect::<String>().split(',').next().unwrap_or("").to_string();
                        self.push_str(&first);
                        self.i = (self.i + end + 2).min(self.chars.len());
                    }
                    _ => {}
                }
                self.marks.push((num, start, self.n));
                return;
            }
            let name = self.name();
            if let Some(v) = self.var(&name).filter(|_| !name.is_empty()) {
                match self.peek(0) {
                    Some('}') => {
                        self.i += 1;
                        self.push_str(&v);
                    }
                    Some(':') => {
                        self.i += 1;
                        let before = self.n;
                        let mut sub = Parser { chars: self.chars, i: self.i, out: String::new(), n: 0, marks: Vec::new(), defaults: self.defaults, vars: self.vars };
                        sub.run(Some('}'));
                        self.i = sub.i;
                        self.push_str(if v.is_empty() { &sub.out } else { &v });
                        let _ = before;
                    }
                    _ => {}
                }
                return;
            }
        } else {
            let name = self.name();
            if let Some(v) = self.var(&name).filter(|_| !name.is_empty()) {
                self.push_str(&v);
                return;
            }
        }
        // not a snippet thing: the $ and what follows stay as written
        self.i = at + 1;
        self.push('$');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_with_indent_and_cursor() {
        let e = expand("<item id=\"a\"\n    parent=\"$0\"/>\n", "    ", &Vars::default());
        assert_eq!(e.text, "<item id=\"a\"\n        parent=\"\"/>");
        let (c, _) = e.first();
        assert_eq!(&e.text.chars().take(c).collect::<String>(), "<item id=\"a\"\n        parent=\"");
        assert!(!e.has_steps());
        // the built-in ones all have a cursor
        for tpl in load(Path::new("/nonexistent")) {
            let e = expand(&tpl.body, "", &Vars::default());
            assert!(!e.text.contains("$0"), "{}", tpl.name);
        }
    }

    #[test]
    fn snippet_stops_in_order_with_mirrors() {
        let v = Vars { file_name: "res_partner.py".into() };
        let e = expand("class ${1:Name}(${2|models.Model,models.TransientModel|}):\n    _name = '$1'\n    $0", "", &v);
        assert_eq!(e.text, "class Name(models.Model):\n    _name = 'Name'\n    ");
        // $1 at two places (typed at once), then $2, then $0 at the end
        assert_eq!(e.stops, vec![vec![(6, 10), (39, 43)], vec![(11, 23)], vec![(49, 49)]]);
        assert_eq!(e.first(), (6, 10));
        assert!(e.has_steps());
        // variables, escapes; an unknown $NAME (shell, PHP) stays as written
        let e = expand("# $TM_FILENAME_BASE: \\$1 costs $PRICE ${HOME}$", "", &v);
        assert_eq!(e.text, "# res_partner: $1 costs $PRICE ${HOME}$");
        assert_eq!(e.stops, vec![vec![(39, 39)]]);
        // a placeholder inside a placeholder, without $0: the end last
        let e = expand("f(${1:a, ${2:b}})", "", &v);
        assert_eq!(e.text, "f(a, b)");
        assert_eq!(e.stops, vec![vec![(2, 6)], vec![(5, 6)], vec![(7, 7)]]);
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
