//! What the XML of an Odoo module gets wrong without being badly formed: a view or a template
//! defined twice in a file, a reference to an id found nowhere. Only the references that always
//! name something defined in XML are checked: the view or template inherited (`inherit_id`), a
//! template called (`t-call`), the parent and action of a menu, an action of a button (`%(…)d`), a
//! group (`groups`). A generic `ref` can name a record made by Python code or a CSV (a product
//! variant, the root user): it is left alone. A reference is only checked when the module it names
//! is in the project: no false alarm.

use crate::lint::{Diagnostic, Severity};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The Odoo module holding `path`: its name and folder (the nearest folder with a `__manifest__.py`).
pub fn module_of(path: &Path) -> Option<(String, PathBuf)> {
    let mut dir = path.parent();
    for _ in 0..6 {
        let d = dir?;
        if d.join("__manifest__.py").is_file() {
            return Some((d.file_name()?.to_string_lossy().into_owned(), d.to_path_buf()));
        }
        dir = d.parent();
    }
    None
}

/// The ids defined in `text`: (where the value starts, the id, a view or a template).
fn defined(text: &str) -> Vec<(usize, String, bool)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"<(record|template|menuitem|act_window|report)\b([^>]*?)\bid\s*=\s*"([^"]+)"([^>]*)"#).unwrap());
    re.captures_iter(text)
        .map(|c| {
            let attrs = format!("{} {}", &c[2], &c[4]);
            let view = &c[1] == "template" || attrs.contains(r#"model="ir.ui.view""#);
            let m = c.get(3).unwrap();
            (m.start(), m.as_str().to_string(), view)
        })
        .collect()
}

/// The references of `text`: (where the id starts, the id as written, "module.id" or "id").
fn references(text: &str) -> Vec<(usize, String)> {
    static ATTRS: OnceLock<Regex> = OnceLock::new();
    static PCT: OnceLock<Regex> = OnceLock::new();
    static CALL: OnceLock<Regex> = OnceLock::new();
    let attrs = ATTRS.get_or_init(|| Regex::new(r#"\b(?:parent|action|inherit_id|groups|t-call)\s*=\s*"([^"]*)""#).unwrap());
    let pct = PCT.get_or_init(|| Regex::new(r#"%\(([\w.]+)\)d"#).unwrap());
    // the view inherited: <field name="inherit_id" ref="…"/>
    let call = CALL.get_or_init(|| Regex::new(r#"<field\s+name\s*=\s*"inherit_id"\s+ref\s*=\s*"([\w.]+)""#).unwrap());
    let mut out = Vec::new();
    for c in attrs.captures_iter(text) {
        let m = c.get(1).unwrap();
        // `groups="a,!b"`: several ids; an expression (`{{ }}`, a space, a call) is not an id
        let mut at = m.start();
        for part in m.as_str().split(',') {
            let id = part.trim().trim_start_matches('!');
            if !id.is_empty() && id.chars().all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.') {
                out.push((at + part.find(id).unwrap_or(0), id.to_string()));
            }
            at += part.len() + 1;
        }
    }
    for re in [pct, call] {
        out.extend(re.captures_iter(text).filter_map(|c| c.get(1)).map(|m| (m.start(), m.as_str().to_string())));
    }
    out.sort();
    out
}

/// The problems of the XML `text` of the module `module`. `exists(id)`: a record, template or menu
/// named `id` is somewhere in the project; `indexed(module)`: that module is in the project.
pub fn check(text: &str, module: &str, exists: impl Fn(&str) -> bool, indexed: impl Fn(&str) -> bool) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let defs = defined(text);
    let mut seen: Vec<(&str, usize, bool)> = Vec::new();
    for (at, id, view) in &defs {
        let short = id.rsplit('.').next().unwrap_or(id);
        match seen.iter().find(|(s, _, _)| *s == short) {
            // a record of data declared again updates it (demo data does so); a view twice is a slip
            Some((_, first, was_view)) if *view && *was_view => out.push(diag(text, *at, Severity::Warning, tf!(
                "« {id} » est déjà défini plus haut dans ce fichier (ligne {}) : la seconde vue remplace la première",
                "“{id}” is already defined above in this file (line {}): the second view replaces the first",
                line_of(text, *first) + 1
            ))),
            Some(_) => {}
            None => seen.push((short, *at, *view)),
        }
    }
    for (at, id) in references(text) {
        let (owner, short) = match id.rsplit_once('.') {
            Some((m, s)) => (m.to_string(), s.to_string()),
            None => (module.to_string(), id.clone()),
        };
        // a model name written with dots (`sale.order`) is not a module: only "module.id" counts;
        // `model_…`, `field_…` and `…_ir_actions_server` (of a cron) are made by Odoo itself
        if owner.contains('.') || short.starts_with("model_") || short.starts_with("field_") || short.ends_with("_ir_actions_server") {
            continue;
        }
        let here = owner == module && seen.iter().any(|(s, _, _)| *s == short);
        // defined as "id" or as "module.id"
        if here || exists(&short) || exists(&format!("{owner}.{short}")) || !indexed(&owner) {
            continue;
        }
        out.push(diag(text, at, Severity::Warning, if owner == module {
            tf!("« {id} » introuvable dans le module {module} : aucun record, template ou menu de ce nom", "“{id}” not found in the module {module}: no record, template or menu of that name")
        } else {
            tf!("« {id} » introuvable : le module {owner} n'a aucun record, template ou menu de ce nom", "“{id}” not found: the module {owner} has no record, template or menu of that name")
        }));
    }
    out.sort_by_key(|d| (d.line, d.col));
    out
}

fn line_of(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].matches('\n').count()
}

fn diag(text: &str, byte: usize, severity: Severity, message: String) -> Diagnostic {
    let before = &text[..byte.min(text.len())];
    let line = before.matches('\n').count();
    let col = before.rsplit('\n').next().unwrap_or("").chars().count();
    Diagnostic { line, col, severity, message, source: "odoo".into(), code: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<odoo>
    <record id="view_form" model="ir.ui.view">
        <field name="inherit_id" ref="base.view_partner_form"/>
        <field name="groups_id" eval="[(4, ref('sale.group_sale'))]"/>
    </record>
    <menuitem id="menu_root" name="X" action="action_missing" groups="base.group_user,!sale.group_none"/>
    <record id="view_form" model="ir.ui.view"/>
    <menuitem id="menu_child" parent="menu_root" action="%(view_form)d"/>
    <field name="model">sale.order</field>
    <t t-call="web.layout"/>
</odoo>"#;

    #[test]
    fn duplicates_and_missing_references() {
        // the project has sale (without group_none), base, not web
        let exists = |id: &str| ["view_partner_form", "group_sale", "group_user"].contains(&id);
        let indexed = |m: &str| ["mymod", "base", "sale"].contains(&m);
        let d = check(XML, "mymod", exists, indexed);
        let msgs: Vec<String> = d.iter().map(|d| format!("{}:{}", d.line + 1, d.message)).collect();
        assert_eq!(d.len(), 3, "{msgs:?}");
        // the local action defined nowhere, the group missing from sale, the id defined twice
        assert!(msgs[0].starts_with("6:") && msgs[0].contains("action_missing"), "{msgs:?}");
        assert!(msgs[1].starts_with("6:") && msgs[1].contains("sale.group_none"), "{msgs:?}");
        assert!(msgs[2].starts_with("7:") && msgs[2].contains("ligne 2") || msgs[2].contains("line 2"), "{msgs:?}");
        // web is not in the project: t-call="web.layout" is not reported; menu_root / view_form are local
        assert!(!msgs.iter().any(|m| m.contains("web.layout") || m.contains("menu_root")));
    }

    #[test]
    fn the_module_of_a_file() {
        let dir = std::env::temp_dir().join(format!("florm_odoo_xml_{}", std::process::id()));
        let views = dir.join("mymod").join("views");
        std::fs::create_dir_all(&views).unwrap();
        std::fs::write(dir.join("mymod").join("__manifest__.py"), "{}").unwrap();
        assert_eq!(module_of(&views.join("a.xml")).map(|m| m.0), Some("mymod".to_string()));
        assert_eq!(module_of(&dir.join("b.xml")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore]
    fn live_odoo_master() {
        // an Odoo checkout of this computer: no false alarm on its sound XML
        let root = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("src/odoo/master");
        if !root.join("addons").is_dir() {
            return;
        }
        let ex = crate::walk::Exclusions::new(&root, &[]);
        let t = std::time::Instant::now();
        let ix = crate::index::Index::build(&root, &[], &ex, 400_000);
        eprintln!("index: {} files with symbols, {:.1} s", ix.file_count(), t.elapsed().as_secs_f32());
        let (mut files, mut found) = (0, Vec::new());
        for m in ["sale", "account", "stock", "website", "purchase", "hr", "crm", "project", "mrp", "point_of_sale"] {
            for e in walkdir(&root.join("addons").join(m)) {
                if e.extension().is_some_and(|x| x == "xml") {
                    files += 1;
                    let text = std::fs::read_to_string(&e).unwrap_or_default();
                    let t = std::time::Instant::now();
                    for d in check(&text, m, |id| ix.has_name(id), |md| ix.has_module(md)) {
                        found.push(format!("{}:{} {}", e.strip_prefix(&root).unwrap().display(), d.line + 1, d.message));
                    }
                    assert!(t.elapsed().as_millis() < 200, "slow on {}", e.display());
                }
            }
        }
        assert!(found.is_empty(), "{files} XML files, false alarms: {found:#?}");
    }

    fn walkdir(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() { out.extend(walkdir(&p)); } else { out.push(p); }
            }
        }
        out
    }
}
