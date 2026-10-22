//! Corrections rapides (Ctrl+., or the bulb at the end of a line with a problem): what can fix
//! the problem under the cursor, or act on the selection. Florm gives its own (import a name of
//! the project, # noqa, ruff --fix, Claude), and the language server adds its code actions.

use super::*;

pub(super) struct QuickFix {
    uid: u64,
    /// Where the list opens (under the cursor).
    at: egui::Pos2,
    items: Vec<(String, Fix)>,
    sel: usize,
    /// The answer of the language server is awaited.
    waiting: bool,
}

pub(super) enum Fix {
    /// Text inserted at a char position of the document (an import).
    Insert(usize, String),
    Cmd(Cmd),
    /// Claude in the editor on the line, with this request.
    Claude(String),
    /// A code action (or command) of the language server: (client, the action).
    Lsp(usize, serde_json::Value),
}

/// What the list asked for during the frame.
enum QfAct {
    Apply(usize),
    Close,
}

/// Python module of file `p`, for an import: its packages (folders with an __init__.py) and its
/// name; an Odoo module (folder with a __manifest__.py) is found under `odoo.addons`.
pub(super) fn python_module(p: &Path) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let stem = p.file_stem()?.to_string_lossy().into_owned();
    if stem != "__init__" {
        parts.push(stem);
    }
    let mut dir = p.parent();
    let mut odoo = false;
    while let Some(d) = dir.filter(|d| d.join("__init__.py").is_file()) {
        parts.insert(0, d.file_name()?.to_string_lossy().into_owned());
        if d.join("__manifest__.py").is_file() {
            odoo = true;
            break;
        }
        dir = d.parent();
    }
    if odoo {
        parts.insert(0, "odoo.addons".into());
    }
    (!parts.is_empty()).then(|| parts.join("."))
}

/// The name a problem says is not defined ("undefined name 'x'", "Undefined name `x`",
/// "Undefined variable 'x'").
fn undefined_name(message: &str) -> Option<String> {
    let lower = message.to_lowercase();
    let at = ["undefined name ", "undefined variable "].iter().find_map(|k| lower.find(k).map(|i| i + k.len()))?;
    let rest = &message[at..];
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '`' | '"'))?;
    let name: String = rest[1..].chars().take_while(|c| *c != quote).collect();
    (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')).then_some(name)
}

/// Line after which an import goes: after the last import at the top of the file, else after its
/// first comments and docstring.
fn import_line(d: &Document) -> usize {
    let n = d.rope.len_lines().min(400);
    let mut after = 0;
    let mut l = 0;
    // shebang, encoding, comments and the docstring of the module
    while l < n {
        let t = d.line_text(l);
        let t = t.trim();
        if t.starts_with('#') {
            l += 1;
            after = l;
        } else if t.starts_with("\"\"\"") || t.starts_with("'''") {
            let q = &t[..3];
            let mut e = l;
            if !(t.len() > 3 && t[3..].contains(q)) {
                e += 1;
                while e < n && !d.line_text(e).contains(q) {
                    e += 1;
                }
            }
            l = e + 1;
            after = l;
        } else {
            break;
        }
    }
    let mut l = after;
    while l < n {
        let t = d.line_text(l);
        if t.starts_with("import ") || t.starts_with("from ") {
            // an import in parentheses goes on until its ")"
            let mut e = l;
            if t.contains('(') && !t.contains(')') {
                while e + 1 < n && !d.line_text(e).contains(')') {
                    e += 1;
                }
            }
            l = e + 1;
            after = l;
        } else if t.trim().is_empty() || t.trim_start().starts_with('#') || t.starts_with("try:") || t.starts_with("except") || t.starts_with(' ') || t.starts_with('\t') {
            l += 1;
        } else {
            break;
        }
    }
    after
}

/// Modules of the standard library imported by their name.
const STD_MODULES: &[&str] = &[
    "os", "re", "sys", "json", "time", "math", "random", "logging", "datetime", "collections", "itertools", "functools", "subprocess",
    "pathlib", "typing", "base64", "hashlib", "uuid", "copy", "io", "csv", "shutil", "tempfile", "traceback", "contextlib", "operator",
    "string", "textwrap", "threading", "pprint", "inspect", "decimal", "calendar", "zipfile", "unittest",
];

impl App {
    /// Florm's own fixes for the cursor line of document `i`.
    fn local_fixes(&self, i: usize) -> Vec<(String, Fix)> {
        let d = &self.docs[i];
        let line = d.rope.char_to_line(d.primary_sel().head);
        let problems: Vec<&lint::Diagnostic> = d.diagnostics.iter().filter(|g| g.line == line).collect();
        let python = d.path.as_ref().is_some_and(|p| lint::lang_of(p) == Some("python"));
        let mut out: Vec<(String, Fix)> = Vec::new();
        // a name used without its import: the import, from the project index or the standard library
        if python && !d.read_only {
            let at = d.rope.line_to_char(import_line(d));
            let mut seen = HashSet::new();
            for name in problems.iter().filter_map(|g| undefined_name(&g.message)) {
                if !seen.insert(name.clone()) {
                    continue;
                }
                if STD_MODULES.contains(&name.as_str()) {
                    out.push((format!("Importer {name} : import {name}"), Fix::Insert(at, format!("import {name}\n"))));
                }
                let found = self.index.symbols.as_ref().map(|ix| ix.lookup(&name)).unwrap_or_default();
                let mut modules = Vec::new();
                for (p, e) in found {
                    let importable = e.name == name && matches!(e.kind, crate::symbols::Kind::Class | crate::symbols::Kind::Function);
                    if !importable || Some(&p) == d.path.as_ref() || lint::lang_of(&p) != Some("python") {
                        continue;
                    }
                    if let Some(m) = python_module(&p).filter(|m| !modules.contains(m)) {
                        modules.push(m);
                    }
                }
                for m in modules.into_iter().take(5) {
                    out.push((format!("Importer {name} : from {m} import {name}"), Fix::Insert(at, format!("from {m} import {name}\n"))));
                }
            }
        }
        let codes = noqa_codes(d, line);
        if !codes.is_empty() && !d.read_only {
            out.push((format!("Ignorer {} sur cette ligne (# noqa)", codes.join(", ")), Fix::Cmd(Cmd::NoqaLine)));
        }
        if python && !d.read_only && lint::is_installed("ruff") && problems.iter().any(|g| matches!(g.source.as_str(), "ruff" | "flake8" | "pyflakes")) {
            out.push(("Corriger tout ce qui peut l'être dans le fichier (ruff --fix)".into(), Fix::Cmd(Cmd::FixDocument)));
        }
        if let Some(g) = problems.first() {
            let ask = format!("Corrige ce problème signalé par {} : {}", g.source, g.message);
            out.push((format!("✳ Corriger avec Claude : {}", g.message), Fix::Claude(ask)));
        }
        out
    }

    /// The list of corrections for the cursor of the active editor.
    pub(super) fn quick_fix_open(&mut self, ctx: &egui::Context) {
        let i = self.active;
        let Some(d) = self.docs.get(i).filter(|d| d.view == document::View::Text) else { return };
        let uid = d.uid;
        let at = d.caret_screen.map_or_else(|| ctx.screen_rect().center(), |(x, y)| egui::pos2(x, y + 2.0));
        let items = self.local_fixes(i);
        let mut waiting = false;
        if let Some((ci, _)) = self.lsp_for(i, ctx, true) {
            let d = &self.docs[i];
            let s = d.primary_sel();
            let uri = d.path.as_ref().map(|p| lsp::path_to_uri(p));
            let line = d.rope.char_to_line(s.head);
            // the problems of the line, for the fixes of the server (ruff server…)
            let diagnostics: Vec<serde_json::Value> = d
                .diagnostics
                .iter()
                .filter(|g| g.line == line)
                .map(|g| {
                    let a = d.pos_of(g.line, g.col);
                    let b = d.word_at(a).1.max(a + 1).min(d.pos_of(g.line, d.line_len(g.line)).max(a));
                    serde_json::json!({
                        "range": { "start": lsp::to_position(&d.rope, a), "end": lsp::to_position(&d.rope, b) },
                        "message": g.message,
                        "severity": if g.severity == Severity::Error { 1 } else { 2 },
                        "code": g.code,
                        "source": g.source,
                    })
                })
                .collect();
            let range = serde_json::json!({ "start": lsp::to_position(&d.rope, s.start()), "end": lsp::to_position(&d.rope, s.end()) });
            if let (Some(uri), Some(cl)) = (uri, self.lsp.manager.client(ci)) {
                let params = serde_json::json!({ "textDocument": { "uri": uri }, "range": range, "context": { "diagnostics": diagnostics, "triggerKind": 1 } });
                let id = cl.request("textDocument/codeAction", params);
                self.lsp.requests.insert((ci, id), LspReq::CodeActions { uid });
                waiting = true;
            }
        }
        if items.is_empty() && !waiting {
            self.set_status("Aucune correction rapide ici");
            return;
        }
        self.quick_fix = Some(QuickFix { uid, at, items, sel: 0, waiting });
    }

    /// The code actions of the server, added after Florm's own.
    pub(super) fn code_actions_loaded(&mut self, ci: usize, uid: u64, v: serde_json::Value) {
        let Some(q) = self.quick_fix.as_mut().filter(|q| q.uid == uid) else { return };
        q.waiting = false;
        for a in v.as_array().into_iter().flatten() {
            if a.get("disabled").is_some() {
                continue;
            }
            let Some(title) = a["title"].as_str() else { continue };
            let star = if a["isPreferred"].as_bool() == Some(true) { "★ " } else { "" };
            q.items.push((format!("{star}{title}"), Fix::Lsp(ci, a.clone())));
        }
        if q.items.is_empty() {
            self.quick_fix = None;
            self.set_status("Aucune correction rapide ici");
        }
    }

    fn quick_fix_apply(&mut self, fix: Fix, ctx: &egui::Context) {
        match fix {
            Fix::Insert(at, text) => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    d.replace_keep(EditKind::Other, vec![(at, at, text.clone())]);
                    self.set_status(format!("Ajouté : {}", text.trim()));
                }
            }
            Fix::Cmd(cmd) => self.pending.push(cmd),
            Fix::Claude(ask) => {
                self.inline_ai_open();
                if let Some(st) = self.inline.as_mut() {
                    st.ask = ask;
                }
                self.inline_ai_send();
            }
            Fix::Lsp(ci, a) => self.run_code_action(ci, a, ctx),
        }
    }

    /// A code action: its edit, then its command; resolved first when the server sends it bare.
    pub(super) fn run_code_action(&mut self, ci: usize, a: serde_json::Value, _ctx: &egui::Context) {
        // a plain command (not a code action)
        if a["command"].is_string() {
            self.lsp_command(ci, &a);
            return;
        }
        if a.get("edit").is_none() && a.get("command").is_none() && a.get("data").is_some() {
            if let Some(cl) = self.lsp.manager.client(ci) {
                let id = cl.request("codeAction/resolve", a);
                self.lsp.requests.insert((ci, id), LspReq::ResolveAction);
            }
            return;
        }
        if let Some(edit) = a.get("edit") {
            let (n, files) = self.apply_workspace_edit(edit);
            self.set_status(format!("{} : {n} modification(s) dans {files} fichier(s)", a["title"].as_str().unwrap_or("Correction")));
        }
        if a["command"].is_object() {
            self.lsp_command(ci, &a["command"]);
        }
    }

    /// `workspace/executeCommand`: the server answers with a workspace/applyEdit if it changes text.
    fn lsp_command(&mut self, ci: usize, c: &serde_json::Value) {
        let params = serde_json::json!({ "command": c["command"], "arguments": c.get("arguments").cloned().unwrap_or(serde_json::json!([])) });
        if let Some(cl) = self.lsp.manager.client(ci) {
            let id = cl.request("workspace/executeCommand", params);
            self.lsp.requests.insert((ci, id), LspReq::Command);
        }
    }

    /// ↑ ↓ Entrée Échap go to the list while it is open (not to the editor).
    pub(super) fn quick_fix_keys(&mut self, ctx: &egui::Context) {
        let Some(q) = self.quick_fix.as_mut() else { return };
        let n = q.items.len().max(1);
        let mut act = None;
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                q.sel = (q.sel + 1) % n;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                q.sel = (q.sel + n - 1) % n;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Enter) {
                act = Some(QfAct::Apply(q.sel));
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                act = Some(QfAct::Close);
            }
        });
        self.quick_fix_act(act, ctx);
    }

    fn quick_fix_act(&mut self, act: Option<QfAct>, ctx: &egui::Context) {
        match act {
            Some(QfAct::Apply(k)) => {
                let Some(q) = self.quick_fix.take() else { return };
                if let Some((_, fix)) = q.items.into_iter().nth(k) {
                    self.quick_fix_apply(fix, ctx);
                }
            }
            Some(QfAct::Close) => self.quick_fix = None,
            None => {}
        }
    }

    pub(super) fn quick_fix_ui(&mut self, ctx: &egui::Context) {
        // a click on the bulb in the editor
        if let Some(d) = self.docs.iter_mut().find(|d| d.quick_fix_request) {
            d.quick_fix_request = false;
            self.quick_fix_open(ctx);
        }
        let Some(q) = self.quick_fix.as_ref() else { return };
        // the editor it was opened on is no longer the active one
        if self.docs.get(self.active).map(|d| d.uid) != Some(q.uid) {
            self.quick_fix = None;
            return;
        }
        let accent = self.hl.colors.accent;
        let mut act = None;
        let area = egui::Area::new(egui::Id::new("quick_fix")).order(egui::Order::Foreground).fixed_pos(q.at).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(320.0);
                ui.set_max_width(640.0);
                ui.label(RichText::new("Corrections rapides").small().strong().color(accent));
                for (k, (label, _)) in q.items.iter().enumerate() {
                    if ui.selectable_label(k == q.sel, label.as_str()).clicked() {
                        act = Some(QfAct::Apply(k));
                    }
                }
                if q.waiting {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("le serveur de langage cherche…").small().weak());
                    });
                }
                ui.label(RichText::new("↑ ↓ Entrée · Échap pour fermer").small().weak());
            });
        });
        // a click elsewhere closes it
        if act.is_none() && ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() {
            act = Some(QfAct::Close);
        }
        self.quick_fix_act(act, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_to_import() {
        assert_eq!(undefined_name("F821 undefined name 'Path'"), Some("Path".into()));
        assert_eq!(undefined_name("Undefined name `datetime`"), Some("datetime".into()));
        assert_eq!(undefined_name("E0602 Undefined variable 'np' (undefined-variable)"), Some("np".into()));
        assert_eq!(undefined_name("F401 'os' imported but unused"), None);
        let dir = std::env::temp_dir().join(format!("florm_qf_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in ["pkg/__init__.py", "pkg/sub/__init__.py", "pkg/sub/tools.py", "addons/sale_x/__init__.py", "addons/sale_x/__manifest__.py", "addons/sale_x/models/__init__.py", "addons/sale_x/models/order.py", "alone.py"] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "").unwrap();
        }
        assert_eq!(python_module(&dir.join("pkg/sub/tools.py")).as_deref(), Some("pkg.sub.tools"));
        assert_eq!(python_module(&dir.join("pkg/sub/__init__.py")).as_deref(), Some("pkg.sub"));
        assert_eq!(python_module(&dir.join("addons/sale_x/models/order.py")).as_deref(), Some("odoo.addons.sale_x.models.order"));
        assert_eq!(python_module(&dir.join("alone.py")).as_deref(), Some("alone"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn imports_go_after_the_others() {
        let d = |t: &str| {
            let mut d = Document::new(1, 4, true);
            d.rope = ropey::Rope::from_str(t);
            d
        };
        assert_eq!(import_line(&d("import os\nfrom a import (\n    b,\n)\n\nx = 1\n")), 4);
        assert_eq!(import_line(&d("#!/usr/bin/env python\n\"\"\"Doc\non two lines.\n\"\"\"\nx = 1\n")), 4);
        assert_eq!(import_line(&d("x = 1\n")), 0);
        assert_eq!(import_line(&d("# -*- coding: utf-8 -*-\nimport logging\n\n_logger = 1\n")), 2);
    }
}
