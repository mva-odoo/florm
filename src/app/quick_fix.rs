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
    /// The chars a..b of the document replaced (a word misspelled by its correction).
    Replace(usize, usize, String),
    /// A word added to the dictionary of the user.
    AddWord(String),
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
        // a word the spell checker does not know, under the cursor: its corrections, or into the dictionary
        let col = d.primary_sel().head - d.rope.line_to_char(line);
        if let Some(m) = d.spelling.iter().find(|m| m.line == line && (m.col..=m.col + m.len).contains(&col)) {
            let a = d.rope.line_to_char(line) + m.col;
            if let Some(dict) = crate::spell::dict(|| {}) {
                for w in dict.suggest(&m.word, 5) {
                    if !d.read_only {
                        out.push((tf!("Orthographe : {w}", "Spelling: {w}"), Fix::Replace(a, a + m.len, w)));
                    }
                }
            }
            out.push((tf!("Ajouter « {} » au dictionnaire", "Add “{}” to the dictionary", m.word), Fix::AddWord(m.word.clone())));
        }
        // a name used without its import: the import, from the project index or the standard library
        if python && !d.read_only {
            let at = d.rope.line_to_char(import_line(d));
            let mut seen = HashSet::new();
            for name in problems.iter().filter_map(|g| undefined_name(&g.message)) {
                if !seen.insert(name.clone()) {
                    continue;
                }
                if STD_MODULES.contains(&name.as_str()) {
                    out.push((tf!("Importer {name} : import {name}", "Import {name}: import {name}"), Fix::Insert(at, format!("import {name}\n"))));
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
                    out.push((tf!("Importer {name} : from {m} import {name}", "Import {name}: from {m} import {name}"), Fix::Insert(at, format!("from {m} import {name}\n"))));
                }
            }
        }
        let codes = noqa_codes(d, line);
        if !codes.is_empty() && !d.read_only {
            out.push((tf!("Ignorer {} sur cette ligne (# noqa)", "Ignore {} on this line (# noqa)", codes.join(", ")), Fix::Cmd(Cmd::NoqaLine)));
        }
        if python && !d.read_only && lint::is_installed("ruff") && problems.iter().any(|g| matches!(g.source.as_str(), "ruff" | "flake8" | "pyflakes")) {
            out.push((t!("Corriger tout ce qui peut l'être dans le fichier (ruff --fix)", "Fix everything that can be in the file (ruff --fix)").into(), Fix::Cmd(Cmd::FixDocument)));
        }
        if let Some(g) = problems.first().filter(|_| crate::features::on(crate::features::Feature::Claude)) {
            let ask = tf!("Corrige ce problème signalé par {} : {}", "Fix this problem reported by {}: {}", g.source, g.message);
            out.push((tf!("✳ Corriger avec Claude : {}", "✳ Fix with Claude: {}", g.message), Fix::Claude(ask)));
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
            // the server's own problems of the line as it sent them, then the linters' ones (ruff server…)
            let line_of = |it: &serde_json::Value| it["range"]["start"]["line"].as_u64().map(|l| l as usize);
            let raw: Vec<serde_json::Value> = self.lsp.raw_diagnostics.get(&uid).map_or(Vec::new(), |v| v.iter().filter(|it| line_of(it) == Some(line)).cloned().collect());
            let mut diagnostics: Vec<serde_json::Value> = d
                .lint_diagnostics()
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
            // the cursor away from the problem: the range of the problem (rust-analyzer only fixes
            // the problems the range touches)
            let cur = lsp::to_position(&d.rope, s.head);
            let touches = |it: &serde_json::Value| {
                let (a, b) = (&it["range"]["start"], &it["range"]["end"]);
                let key = |p: &serde_json::Value| (p["line"].as_u64().unwrap_or(0), p["character"].as_u64().unwrap_or(0));
                key(a) <= key(&cur) && key(&cur) <= key(b)
            };
            let range = match raw.first() {
                Some(first) if s.start() == s.end() && !raw.iter().any(touches) => first["range"].clone(),
                _ => serde_json::json!({ "start": lsp::to_position(&d.rope, s.start()), "end": lsp::to_position(&d.rope, s.end()) }),
            };
            diagnostics.splice(0..0, raw);
            if let (Some(uri), Some(cl)) = (uri, self.lsp.manager.client(ci)) {
                let params = serde_json::json!({ "textDocument": { "uri": uri }, "range": range, "context": { "diagnostics": diagnostics, "triggerKind": 1 } });
                let id = cl.request("textDocument/codeAction", params);
                self.lsp.requests.insert((ci, id), LspReq::CodeActions { uid });
                waiting = true;
            }
        }
        if items.is_empty() && !waiting {
            self.set_status(t!("Aucune correction rapide ici", "No quick fix here"));
            return;
        }
        self.quick_fix = Some(QuickFix { uid, at, items, sel: 0, waiting });
    }

    /// The code actions of the server, added after Florm's own.
    pub(super) fn code_actions_loaded(&mut self, ci: usize, uid: u64, v: serde_json::Value) {
        let Some(q) = self.quick_fix.as_mut().filter(|q| q.uid == uid) else { return };
        q.waiting = false;
        // the fixes of the problem first (the preferred one on top), the refactors and the rest after
        let rank = |a: &serde_json::Value| match (a["isPreferred"].as_bool() == Some(true), a["kind"].as_str().unwrap_or("")) {
            (true, _) => 0,
            (_, k) if k.starts_with("quickfix") => 1,
            (_, k) if k.starts_with("source.organizeImports") || k.starts_with("source.fixAll") => 2,
            (_, k) if k.starts_with("refactor") => 3,
            _ => 4,
        };
        let mut actions: Vec<&serde_json::Value> = v.as_array().into_iter().flatten().collect();
        actions.sort_by_key(|a| rank(a));
        for a in actions {
            if a.get("disabled").is_some() {
                continue;
            }
            let Some(title) = a["title"].as_str() else { continue };
            let star = if a["isPreferred"].as_bool() == Some(true) { "★ " } else { "" };
            q.items.push((format!("{star}{title}"), Fix::Lsp(ci, a.clone())));
        }
        if q.items.is_empty() {
            self.quick_fix = None;
            self.set_status(t!("Aucune correction rapide ici", "No quick fix here"));
        }
    }

    fn quick_fix_apply(&mut self, fix: Fix, ctx: &egui::Context) {
        match fix {
            Fix::Insert(at, text) => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    d.replace_keep(EditKind::Other, vec![(at, at, text.clone())]);
                    self.set_status(tf!("Ajouté : {}", "Added: {}", text.trim()));
                }
            }
            Fix::Replace(a, b, text) => {
                if let Some(d) = self.docs.get_mut(self.active) {
                    d.replace_keep(EditKind::Other, vec![(a, b, text)]);
                }
            }
            Fix::AddWord(w) => match crate::spell::add_word(&w) {
                Ok(()) => {
                    // checked again everywhere with the new word
                    for d in &mut self.docs {
                        d.spell_version = u64::MAX;
                    }
                    self.set_status(tf!("« {w} » ajouté au dictionnaire ({})", "“{w}” added to the dictionary ({})", crate::spell::my_words_path().display()));
                }
                Err(e) => self.set_error(tf!("Dictionnaire : {e}", "Dictionary: {e}")),
            },
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
            self.set_status(tf!("{} : {n} modification(s) dans {files} fichier(s)", "{}: {n} change(s) in {files} file(s)", a["title"].as_str().unwrap_or(t!("Correction", "Fix"))));
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
                ui.label(RichText::new(t!("Corrections rapides", "Quick fixes")).small().strong().color(accent));
                for (k, (label, _)) in q.items.iter().enumerate() {
                    if ui.selectable_label(k == q.sel, label.as_str()).clicked() {
                        act = Some(QfAct::Apply(k));
                    }
                }
                if q.waiting {
                    ui.horizontal(|ui| {
                        spinner(ui);
                        ui.label(RichText::new(t!("le serveur de langage cherche…", "the language server is looking…")).small().weak());
                    });
                }
                ui.label(RichText::new(t!("↑ ↓ Entrée · Échap pour fermer", "↑ ↓ Enter · Escape to close")).small().weak());
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

#[cfg(test)]
mod lsp_tests {
    use super::*;

    /// With gopls (when installed): the cursor away from a name used without its import, the
    /// server's fix is offered all the same, and applying it adds the import.
    #[test]
    fn the_import_of_a_language_server() {
        if !crate::tools::locate("gopls") {
            return;
        }
        let dir = std::env::temp_dir().join(format!("florm_qf_gopls_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = true\n").unwrap();
        std::fs::write(proj.join("go.mod"), "module t\n\ngo 1.21\n").unwrap();
        let file = proj.join("main.go");
        std::fs::write(&file, "package main\n\nfunc main() { println(strings.ToUpper(\"a\")) }\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        assert!(app.open_path(file.clone()));
        let wait = |app: &mut App, what: &str, done: &dyn Fn(&App) -> bool| {
            let until = Instant::now() + Duration::from_secs(60);
            while !done(app) {
                assert!(Instant::now() < until, "trop long : {what}");
                app.lsp_frame(&ctx);
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        wait(&mut app, "l'erreur de gopls", &|a| a.docs[a.active].diagnostics.iter().any(|d| d.line == 2));
        // the cursor at the start of the line, away from `strings`
        let at = app.docs[app.active].rope.line_to_char(2);
        let d = &mut app.docs[app.active];
        d.sels = vec![Sel::range(at, at)];
        d.primary = 0;
        app.quick_fix_open(&ctx);
        wait(&mut app, "les corrections de gopls", &|a| a.quick_fix.as_ref().is_some_and(|q| !q.waiting));
        let titles: Vec<String> = app.quick_fix.as_ref().map(|q| q.items.iter().map(|i| i.0.clone()).collect()).unwrap_or_default();
        let k = titles.iter().position(|t| t.contains("\"strings\"")).unwrap_or_else(|| panic!("{titles:?}"));
        // the fix of the problem before the other actions of gopls
        assert!(titles[..k].iter().all(|t| !t.starts_with("Browse")), "{titles:?}");
        let fix = app.quick_fix.take().unwrap().items.swap_remove(k).1;
        app.quick_fix_apply(fix, &ctx);
        wait(&mut app, "l'import ajouté", &|a| a.docs[a.active].rope.to_string().contains("import \"strings\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A word misspelled in a Markdown file: dotted, Ctrl+. offers its correction and to add it
    /// to the dictionary; added, it is no longer underlined.
    #[test]
    fn a_misspelled_word_corrected_or_added() {
        let dir = std::env::temp_dir().join(format!("florm_qf_spell_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let file = proj.join("notes.md");
        std::fs::write(&file, "Le fichierr et flormesque.\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        crate::spell::use_words(&["le", "et", "fichier", "fichiers"]);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        assert!(app.open_path(file.clone()));
        // checked on a thread: its answer read like the app does at each frame
        let check = |app: &mut App| {
            app.docs[app.active].last_change = Instant::now() - Duration::from_secs(1);
            app.periodic(&ctx);
            for _ in 0..60 {
                app.process_jobs();
                std::thread::sleep(Duration::from_millis(5));
            }
            app.docs[app.active].spelling.iter().map(|m| m.word.clone()).collect::<Vec<_>>()
        };
        assert_eq!(check(&mut app), ["fichierr", "flormesque"]);
        // Ctrl+. on « fichierr »: its correction first, then the dictionary
        let d = &mut app.docs[app.active];
        d.sels = vec![Sel::range(5, 5)];
        d.primary = 0;
        let fixes = app.local_fixes(app.active);
        assert_eq!(fixes[0].0, t!("Orthographe : fichier", "Spelling: fichier"));
        let fix = app.local_fixes(app.active).swap_remove(0).1;
        app.quick_fix_apply(fix, &ctx);
        assert_eq!(app.docs[app.active].rope.to_string(), "Le fichier et flormesque.\n");
        // « flormesque » into the dictionary: no longer underlined
        let d = &mut app.docs[app.active];
        d.sels = vec![Sel::range(16, 16)];
        let add = app.local_fixes(app.active).into_iter().find(|f| matches!(f.1, Fix::AddWord(_))).unwrap().1;
        app.quick_fix_apply(add, &ctx);
        assert!(check(&mut app).is_empty());
        assert!(std::fs::read_to_string(crate::spell::my_words_path()).unwrap().contains("flormesque"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
