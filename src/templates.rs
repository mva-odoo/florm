//! Templates (snippets): blocks of code inserted at the cursor, e.g. an Odoo form view or menu.
//! Built-in ones plus the files of the `templates` folder of the configuration (file name = name
//! of the template, extension = language). Variables are filled from the current file:
//! `${model}` (sale.order), `${model_}` (sale_order), `${Model}` (SaleOrder), `${name}` (Sale
//! Order), `${module}` (folder with the `__manifest__.py`); `$0` is where the cursor goes.
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
    (
        "Odoo : vue formulaire",
        "xml",
        "odoo_form",
        r#"<record id="${model_}_view_form" model="ir.ui.view">
    <field name="name">${model}.view.form</field>
    <field name="model">${model}</field>
    <field name="arch" type="xml">
        <form string="${name}">
            <sheet>
                <group>
                    <field name="name"/>$0
                </group>
            </sheet>
        </form>
    </field>
</record>
"#,
    ),
    (
        "Odoo : vue liste",
        "xml",
        "odoo_list",
        r#"<record id="${model_}_view_list" model="ir.ui.view">
    <field name="name">${model}.view.list</field>
    <field name="model">${model}</field>
    <field name="arch" type="xml">
        <list string="${name}">
            <field name="name"/>$0
        </list>
    </field>
</record>
"#,
    ),
    (
        "Odoo : vue recherche",
        "xml",
        "odoo_search",
        r#"<record id="${model_}_view_search" model="ir.ui.view">
    <field name="name">${model}.view.search</field>
    <field name="model">${model}</field>
    <field name="arch" type="xml">
        <search string="${name}">
            <field name="name"/>$0
        </search>
    </field>
</record>
"#,
    ),
    (
        "Odoo : héritage de vue",
        "xml",
        "odoo_view_inherit",
        r#"<record id="${model_}_view_form_inherit_${module}" model="ir.ui.view">
    <field name="name">${model}.view.form.inherit.${module}</field>
    <field name="model">${model}</field>
    <field name="inherit_id" ref="$0"/>
    <field name="arch" type="xml">
        <xpath expr="//field[@name='name']" position="after">
        </xpath>
    </field>
</record>
"#,
    ),
    (
        "Odoo : action de fenêtre",
        "xml",
        "odoo_action",
        r#"<record id="${model_}_action" model="ir.actions.act_window">
    <field name="name">${name}</field>
    <field name="res_model">${model}</field>
    <field name="view_mode">list,form</field>
    <field name="help" type="html">
        <p class="o_view_nocontent_smiling_face">$0</p>
    </field>
</record>
"#,
    ),
    (
        "Odoo : menu",
        "xml",
        "odoo_menu",
        r#"<menuitem id="${model_}_menu"
          name="${name}"
          parent="$0"
          action="${model_}_action"
          sequence="10"/>
"#,
    ),
    (
        "Odoo : vues + action + menu",
        "xml",
        "odoo_views",
        r#"<record id="${model_}_view_list" model="ir.ui.view">
    <field name="name">${model}.view.list</field>
    <field name="model">${model}</field>
    <field name="arch" type="xml">
        <list string="${name}">
            <field name="name"/>
        </list>
    </field>
</record>

<record id="${model_}_view_form" model="ir.ui.view">
    <field name="name">${model}.view.form</field>
    <field name="model">${model}</field>
    <field name="arch" type="xml">
        <form string="${name}">
            <sheet>
                <group>
                    <field name="name"/>$0
                </group>
            </sheet>
        </form>
    </field>
</record>

<record id="${model_}_action" model="ir.actions.act_window">
    <field name="name">${name}</field>
    <field name="res_model">${model}</field>
    <field name="view_mode">list,form</field>
</record>

<menuitem id="${model_}_menu" name="${name}" action="${model_}_action" sequence="10"/>
"#,
    ),
    (
        "Odoo : fichier de données XML",
        "xml",
        "odoo_xml",
        r#"<?xml version="1.0" encoding="utf-8"?>
<odoo>
    $0
</odoo>
"#,
    ),
    (
        "Odoo : modèle",
        "py",
        "odoo_model",
        r#"from odoo import api, fields, models


class ${Model}(models.Model):
    _name = '${model}'
    _description = "${name}"

    name = fields.Char(required=True)$0
"#,
    ),
    (
        "Odoo : héritage de modèle",
        "py",
        "odoo_inherit",
        r#"from odoo import api, fields, models


class ${Model}(models.Model):
    _inherit = '${model}'

    $0
"#,
    ),
    (
        "Odoo : champ calculé",
        "py",
        "odoo_compute",
        r#"total = fields.Float(compute='_compute_total', store=True)

@api.depends($0)
def _compute_total(self):
    for record in self:
        record.total = 0
"#,
    ),
    (
        "Odoo : __manifest__.py",
        "py",
        "odoo_manifest",
        r#"{
    'name': "${name}",
    'version': '1.0',
    'depends': ['base'],
    'data': [
        'security/ir.model.access.csv',
        $0
    ],
    'license': 'LGPL-3',
}
"#,
    ),
    (
        "Odoo : droits d'accès (ir.model.access.csv)",
        "csv",
        "odoo_access",
        r#"id,name,model_id:id,group_id:id,perm_read,perm_write,perm_create,perm_unlink
access_${model_}_user,${model}.user,model_${model_},base.group_user,1,1,1,1$0
"#,
    ),
];

const README: &str = "# Modèles Florm\n\n\
Chaque fichier de ce dossier est un modèle : son nom devient le nom du modèle, son extension\n\
le type de fichier pour lequel il est proposé en premier (ex. « Ma vue kanban.xml »).\n\n\
Un nom d'un seul mot (lettres, chiffres, _) sert aussi de déclencheur : avec « ipdb.py », taper\n\
`ipdb` dans un fichier .py le propose dans les suggestions, et Tab le développe.\n\n\
Variables remplacées à l'insertion, d'après le fichier courant :\n\n\
- `${model}` : sale.order (d'après le nom du fichier : sale_order_views.xml, sale_order.py)\n\
- `${model_}` : sale_order\n\
- `${Model}` : SaleOrder\n\
- `${name}` : Sale Order\n\
- `${module}` : le module Odoo (dossier qui contient __manifest__.py)\n\
- `$0` : la position du curseur après l'insertion\n";

pub fn dir() -> PathBuf {
    crate::config::config_dir().join("templates")
}

/// Creates the folder with its README the first time.
pub fn install_readme(dir: &Path) {
    if !dir.exists() && std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(dir.join("README.md"), README);
    }
}

/// Built-in templates, then the user's (a user template with the same name replaces the built-in one).
pub fn load(dir: &Path) -> Vec<Template> {
    let mut v: Vec<Template> = BUILTIN
        .iter()
        .map(|(n, e, tr, b)| Template { name: n.to_string(), ext: e.to_string(), trigger: tr.to_string(), body: b.to_string(), path: None })
        .collect();
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for p in files {
            if p.file_name().is_some_and(|n| n == "README.md") {
                continue;
            }
            let Ok(body) = std::fs::read_to_string(&p) else { continue };
            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let ext = p.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            let trigger = if is_word(&name) { name.clone() } else { String::new() };
            // a user template with a built-in's trigger replaces it too
            v.retain(|t| t.name != name && (trigger.is_empty() || t.trigger != trigger));
            v.push(Template { name, ext, trigger, body, path: Some(p) });
        }
    }
    v
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

/// Model name guessed from a file name: "sale_order_views.xml" → "sale.order".
pub fn model_from_file(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut s = stem.as_str();
    for suffix in ["_views", "_view", "_menus", "_menu", "_actions", "_action", "_templates", "_data", "_security", "_report", "_wizard"] {
        if let Some(rest) = s.strip_suffix(suffix) {
            s = rest;
            break;
        }
    }
    if s.is_empty() || s == "__init__" || s == "__manifest__" || s == "ir.model.access" {
        return "my.model".into();
    }
    s.replace('_', ".")
}

/// Values of the variables for a template inserted in `path`.
pub fn variables(path: Option<&Path>) -> Vec<(&'static str, String)> {
    let model = path.map(model_from_file).unwrap_or_else(|| "my.model".into());
    let camel: String = model.split('.').map(|w| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }).collect();
    let name: String = model.split('.').map(|w| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }).collect::<Vec<_>>().join(" ");
    let module = path
        .and_then(|p| p.ancestors().skip(1).find(|d| d.join("__manifest__.py").is_file()))
        .and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "my_module".into());
    vec![("${model_}", model.replace('.', "_")), ("${model}", model), ("${Model}", camel), ("${name}", name), ("${module}", module)]
}

/// The text to insert: variables replaced, every line after the first indented with `indent`.
/// Returns it with the position of `$0` (in chars), or its end.
pub fn expand(body: &str, vars: &[(&str, String)], indent: &str) -> (String, usize) {
    let mut t = body.replace("\r\n", "\n");
    for (k, v) in vars {
        t = t.replace(k, v);
    }
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
    fn variables_from_the_file() {
        let dir = std::env::temp_dir().join(format!("florm_tpl_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("estate/views")).unwrap();
        std::fs::write(dir.join("estate/__manifest__.py"), "{}").unwrap();
        let f = dir.join("estate/views/estate_property_views.xml");
        let v = variables(Some(&f));
        let get = |k: &str| v.iter().find(|(n, _)| *n == k).unwrap().1.clone();
        assert_eq!(get("${model}"), "estate.property");
        assert_eq!(get("${model_}"), "estate_property");
        assert_eq!(get("${Model}"), "EstateProperty");
        assert_eq!(get("${name}"), "Estate Property");
        assert_eq!(get("${module}"), "estate");
        assert_eq!(model_from_file(Path::new("models/sale_order.py")), "sale.order");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expands_with_indent_and_cursor() {
        let vars = vec![("${model_}", "a_b".to_string()), ("${model}", "a.b".to_string())];
        let (t, c) = expand("<menuitem id=\"${model_}_menu\"\n    parent=\"$0\"/>\n", &vars, "    ");
        assert_eq!(t, "<menuitem id=\"a_b_menu\"\n        parent=\"\"/>");
        assert_eq!(&t.chars().take(c).collect::<String>(), "<menuitem id=\"a_b_menu\"\n        parent=\"");
        // the built-in ones all have a known model and a cursor
        for tpl in load(Path::new("/nonexistent")) {
            let (t, _) = expand(&tpl.body, &variables(Some(Path::new("x/sale_order_views.xml"))), "");
            assert!(!t.contains("${"), "{}: {t}", tpl.name);
            assert!(!t.contains("$0"), "{}", tpl.name);
        }
    }

    #[test]
    fn user_templates_replace_builtin_ones() {
        let dir = std::env::temp_dir().join(format!("florm_tpl_user_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        install_readme(&dir);
        std::fs::write(dir.join("Odoo : menu.xml"), "<menuitem/>").unwrap();
        std::fs::write(dir.join("Kanban.xml"), "<kanban/>").unwrap();
        let all = load(&dir);
        assert_eq!(all.iter().filter(|t| t.name == "Odoo : menu").count(), 1);
        assert_eq!(all.iter().find(|t| t.name == "Odoo : menu").unwrap().body, "<menuitem/>");
        assert!(all.iter().any(|t| t.name == "Kanban" && t.ext == "xml"));
        assert!(!all.iter().any(|t| t.name == "README"));
        assert_eq!(all.iter().find(|t| t.name == "Kanban").unwrap().trigger, "Kanban");
        std::fs::write(dir.join("ipdb.py"), "breakpoint()").unwrap();
        let all = load(&dir);
        let hits: Vec<&Template> = with_trigger(&all, "py", "ipd").collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].body, "breakpoint()");
        assert_eq!(with_trigger(&all, "xml", "ipdb").count(), 0);
        assert_eq!(with_trigger(&all, "py", "odoo_").filter(|t| t.ext != "py").count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
