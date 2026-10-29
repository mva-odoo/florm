//! What goes to Claude with a message: lines, files, notes, commits, and the definitions of
//! the names the message cites.

use super::*;

/// Something given to Claude with the next message: lines of a file, a file / folder, or a text
/// (the changes not committed, the problems of a file…).
#[derive(Clone)]
pub(in crate::app) struct Attach {
    pub(super) path: PathBuf,
    /// Lines (1-based, first and last) and their text; None: the whole file or folder.
    pub(super) lines: Option<(usize, usize, String)>,
    /// A text and what it is (instead of a file).
    pub(super) note: Option<(String, String)>,
    /// An image sent with the message (a pasted image, a file): what it is, and
    /// its PNG, saved to be shown again in the message.
    pub(super) image: Option<(String, PathBuf)>,
}

impl Attach {
    /// A text (`note`: what it is, the text) given with a message of the conversation in `dir`.
    pub(in crate::app) fn note(dir: PathBuf, note: (String, String)) -> Self {
        Attach { path: dir, lines: None, note: Some(note), image: None }
    }

    /// An image (`image`: what it is, its PNG) given with a message of the conversation in `dir`.
    pub(in crate::app) fn image(dir: PathBuf, image: (String, PathBuf)) -> Self {
        Attach { path: dir, lines: None, note: None, image: Some(image) }
    }

    pub(super) fn label(&self, dir: Option<&Path>) -> String {
        if let Some((label, _)) = &self.image {
            return label.clone();
        }
        if let Some((label, _)) = &self.note {
            return label.clone();
        }
        let rel = dir.and_then(|d| self.path.strip_prefix(d).ok()).unwrap_or(&self.path).display().to_string();
        match &self.lines {
            Some((a, b, _)) if a == b => format!("{rel}:{a}"),
            Some((a, b, _)) => format!("{rel}:{a}-{b}"),
            None => rel,
        }
    }

    /// How it is said to Claude.
    pub(super) fn prompt(&self, dir: Option<&Path>) -> String {
        let label = self.label(dir);
        if self.image.is_some() {
            return tf!("Image jointe ({label}), envoyée avec le message", "Image attached ({label}), sent with the message");
        }
        if let Some((_, text)) = &self.note {
            return format!("{label} :\n```\n{text}\n```");
        }
        match &self.lines {
            Some((_, _, text)) => {
                let lang = self.path.extension().and_then(|e| e.to_str()).unwrap_or("");
                tf!("Extrait de {label} :\n```{lang}\n{text}\n```", "Excerpt of {label}:\n```{lang}\n{text}\n```")
            }
            None if self.path.is_dir() => tf!("Dossier à prendre en compte : {label}", "Folder to take into account: {label}"),
            None => tf!("Fichier à prendre en compte : {label}", "File to take into account: {label}"),
        }
    }
}

/// Most characters of a diff given to Claude as context (the rest: Claude reads it with git itself).
pub(super) const MAX_DIFF_CHARS: usize = 20_000;

/// What can be added to the context from the chat (« + Contexte »).
#[derive(Clone)]
pub(super) enum CtxAdd {
    /// A commit (its hash): its message and its diff.
    Commit(String),
    File,
    Selection,
    Tabs,
    Diff,
    Problems,
    /// An image file, chosen in the file chooser.
    Image,
}

/// The names of a message that look like code: in backquotes, followed by "(", CamelCase, or
/// with an underscore — not the words of the sentence.
/// The path of `path` relative to the folder `dir` of a conversation (None outside it).
pub(super) fn rel_path(dir: Option<&Path>, path: &Path) -> Option<String> {
    dir.and_then(|d| path.strip_prefix(d).ok()).map(|r| r.display().to_string())
}

/// Whether a message of a conversation gave the file `rel`, or a part of it (`rel:line`).
pub(super) fn gives_file(items: &[Item], rel: &str) -> bool {
    items.iter().any(|i| matches!(i, Item::User(_, given, ..) if gives(given, rel)))
}

/// Whether `given` (what messages gave) holds the file `rel`, or a part of it (`rel:line`).
pub(super) fn gives(given: &[String], rel: &str) -> bool {
    let prefix = format!("{rel}:");
    given.iter().any(|g| g == rel || g.starts_with(&prefix))
}

pub(super) fn cited_names(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |w: &str| {
        let w = w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_'));
        if w.chars().count() >= 3 && w.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && !out.iter().any(|o| o == w) {
            out.push(w.to_string());
        }
    };
    let quoted: Vec<&str> = text.split('`').skip(1).step_by(2).collect();
    for q in &quoted {
        for w in q.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            push(w);
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphanumeric() || chars[i] == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let w: String = chars[start..i].iter().collect();
            let call = chars.get(i) == Some(&'(');
            let camel = w.chars().next().is_some_and(|c| c.is_uppercase()) && w.chars().skip(1).any(|c| c.is_uppercase()) && w.chars().any(|c| c.is_lowercase());
            let snake = w.contains('_') && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if call || camel || snake {
                push(&w);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The lines of a definition starting at `line`: until the code comes back to its indentation
/// (the next definition), at most `max`.
pub(super) fn definition_lines(src: &str, line: usize, max: usize) -> String {
    let lines: Vec<&str> = src.lines().skip(line).collect();
    let Some(first) = lines.first() else { return String::new() };
    let indent = first.len() - first.trim_start().len();
    let mut out = vec![*first];
    for l in lines.iter().skip(1).take(max - 1) {
        let ind = l.len() - l.trim_start().len();
        // decorators and blank lines do not end it; code at its indentation or less does
        if !l.trim().is_empty() && ind <= indent && !l.trim_start().starts_with(')') {
            break;
        }
        out.push(l);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.join("\n")
}

impl App {
    /// Lines `a..=b` (0-based) of document `i` given to Claude with the next message.
    pub(in crate::app) fn claude_attach_lines(&mut self, i: usize, a: usize, b: usize) {
        let Some(d) = self.docs.get(i) else { return };
        let Some(path) = d.path.clone() else {
            self.set_status(t!("Enregistre d'abord le fichier pour le donner à Claude", "Save the file first to give it to Claude"));
            return;
        };
        let b = b.min(d.rope.len_lines().saturating_sub(1));
        let text: String = (a..=b).map(|l| d.line_text(l)).collect::<Vec<_>>().join("\n");
        self.claude_attach(Attach { path, lines: Some((a + 1, b + 1, text)), note: None, image: None });
    }

    pub(in crate::app) fn claude_attach_path(&mut self, path: PathBuf) {
        self.claude_attach(Attach { path, lines: None, note: None, image: None });
    }

    /// A text given to Claude with the next message (`label`: what it is).
    pub(super) fn claude_attach_note(&mut self, label: String, text: String) {
        let path = self.claude.dir.clone().unwrap_or_default();
        self.claude_attach(Attach { path, lines: None, note: Some((label, text)), image: None });
    }

    /// An image (PNG) given to Claude with the next message; `label`: what it is.
    pub(in crate::app) fn claude_attach_image(&mut self, png: Vec<u8>, label: String) {
        let saved = match claude::save_image(&png) {
            Ok(p) => p,
            Err(e) => return self.set_error(tf!("Image non enregistrée : {e}", "Image not saved: {e}")),
        };
        if self.claude.attach.iter().any(|a| a.image.as_ref().is_some_and(|(_, p)| *p == saved)) {
            return;
        }
        // « Image collée », « Image collée 2 »…: each its own name in the message
        let same = self.claude.attach.iter().filter(|a| a.image.as_ref().is_some_and(|(l, _)| l.starts_with(&label))).count();
        let label = if same == 0 { label } else { format!("{label} {}", same + 1) };
        let path = self.claude.dir.clone().unwrap_or_default();
        self.claude.attach.push(Attach { path, lines: None, note: None, image: Some((label.clone(), saved)) });
        self.claude.focus = true;
        self.set_status(tf!("✳ {label} ajoutée au contexte de Claude, envoyée avec le prochain message", "✳ {label} added to Claude's context, sent with the next message"));
    }

    /// An image file given to Claude (read and made smaller in the background).
    pub(in crate::app) fn claude_attach_image_file(&mut self, path: &Path) {
        let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let path = path.to_path_buf();
        self.jobs.spawn(move || {
            let png = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| claude::png_of_file_bytes(&b));
            JobResult::ClaudeImage { label, png }
        });
    }

    /// The text document Claude's context is about: the active one, else the last one opened.
    pub(super) fn claude_editor_doc(&self) -> Option<usize> {
        let ok = |d: &Document| d.view == document::View::Text && d.path.is_some();
        self.docs.get(self.active).filter(|d| ok(*d)).map(|_| self.active).or_else(|| self.docs.iter().rposition(ok))
    }

    /// What the « + Contexte » menu of the chat asked for.
    /// A commit given to Claude with the next message: its message, the files it touched and its
    /// diff (cut when very large), to start from or to compare with.
    pub(in crate::app) fn claude_attach_commit(&mut self, repo: &Path, hash: &str) {
        match git::run(repo, &["show", "--stat", "--patch", "--format=commit %H%nAuteur : %an, %ad%n%n%B", "--date=short", hash]) {
            Ok(text) => {
                let subject = git::stdout(repo, &["log", "-1", "--format=%s", hash]).unwrap_or_default();
                let mut text: String = text.chars().take(MAX_DIFF_CHARS).collect();
                if text.chars().count() >= MAX_DIFF_CHARS {
                    text.push_str(t!("\n[… diff coupé, trop long]", "\n[… diff cut, too long]"));
                }
                self.claude.dir.get_or_insert_with(|| repo.to_path_buf());
                let short = &hash[..hash.len().min(8)];
                self.claude_attach_note(tf!("Commit {short} : {}", "Commit {short}: {}", subject.trim()), text);
            }
            Err(e) => self.set_error(format!("git show : {}", e.lines().next().unwrap_or(""))),
        }
    }

    pub(super) fn claude_add_context(&mut self, what: CtxAdd) {
        let doc = self.claude_editor_doc();
        match what {
            CtxAdd::Commit(hash) => {
                if let Some(dir) = self.claude.dir.clone() {
                    self.claude_attach_commit(&dir, &hash);
                }
            }
            CtxAdd::File => match doc.and_then(|i| self.docs[i].path.clone()) {
                Some(p) => self.claude_attach_path(p),
                None => self.set_status(t!("Aucun fichier ouvert", "No file open")),
            },
            CtxAdd::Selection => match doc {
                Some(i) => {
                    let d = &self.docs[i];
                    let s = d.primary_sel();
                    let a = d.rope.char_to_line(s.start());
                    let b = d.rope.char_to_line(s.end().saturating_sub(usize::from(s.end() > s.start())));
                    self.claude_attach_lines(i, a, b.max(a));
                }
                None => self.set_status(t!("Aucun fichier ouvert", "No file open")),
            },
            CtxAdd::Tabs => {
                let paths: Vec<PathBuf> = self.docs.iter().filter(|d| d.view == document::View::Text).filter_map(|d| d.path.clone()).collect();
                for p in paths {
                    self.claude_attach_path(p);
                }
            }
            CtxAdd::Diff => {
                let Some(dir) = self.claude.dir.clone() else { return };
                match super::git_view::worktree_diff(&dir, &["diff", "HEAD"]) {
                    Ok(d) if !d.trim().is_empty() => {
                        let d: String = d.chars().take(MAX_DIFF_CHARS).collect();
                        self.claude_attach_note(t!("Changements non commités", "Uncommitted changes").into(), d);
                    }
                    Ok(_) => self.set_status(t!("Aucun changement non commité", "No uncommitted change")),
                    Err(e) => self.set_error(format!("git diff : {}", e.lines().next().unwrap_or(""))),
                }
            }
            CtxAdd::Image => self.file_dialog(crate::os::Pick::Image),
            CtxAdd::Problems => {
                let Some(i) = doc else {
                    self.set_status(t!("Aucun fichier ouvert", "No file open"));
                    return;
                };
                let d = &self.docs[i];
                let name = d.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let list: Vec<String> = d
                    .diagnostics
                    .iter()
                    .map(|g| tf!("ligne {} : {}{} ({})", "line {}: {}{} ({})", g.line + 1, g.code.as_ref().map(|c| format!("{c} ")).unwrap_or_default(), g.message, g.source))
                    .collect();
                if list.is_empty() {
                    self.set_status(tf!("Aucun problème dans {name}", "No problem in {name}"));
                } else {
                    self.claude_attach_note(tf!("Problèmes de {name}", "Problems of {name}"), list.join("\n"));
                }
            }
        }
    }

    pub(super) fn claude_attach(&mut self, a: Attach) {
        if self.claude.dir.is_none() {
            self.claude.dir = self.repo_for(&a.path).or_else(|| self.repo.clone()).or_else(|| self.root.clone());
        }
        let label = a.label(self.claude.dir.as_deref());
        let same = |x: &Attach| {
            x.path == a.path && x.lines.as_ref().map(|l| (l.0, l.1)) == a.lines.as_ref().map(|l| (l.0, l.1)) && x.note.as_ref().map(|n| &n.0) == a.note.as_ref().map(|n| &n.0)
        };
        if let Some(x) = self.claude.attach.iter_mut().find(|x| same(x)) {
            // the same diff or problems again: the newest text
            x.note = a.note;
        } else {
            self.claude.attach.push(a);
        }
        self.set_status(tf!("✳ {label} ajouté au contexte de Claude ({} élément(s)) — Ctrl+Alt+C pour lui écrire", "✳ {label} added to Claude's context ({} item(s)) — Ctrl+Alt+C to write to it", self.claude.attach.len()));
    }

    /// The selection (else the line of the cursor) of the current file for Claude; `ask`: and
    /// the Claude tab opened to write the question.
    pub(in crate::app) fn claude_attach_selection(&mut self, ask: bool) {
        let Some(d) = self.docs.get(self.active).filter(|d| d.view == document::View::Text) else { return };
        let s = d.primary_sel();
        let a = d.rope.char_to_line(s.start());
        let b = d.rope.char_to_line(s.end().saturating_sub(usize::from(s.end() > s.start())));
        self.claude_attach_lines(self.active, a, b.max(a));
        if ask {
            self.open_claude();
        }
    }

    /// Lines picked with ✳ in the gutter of an editor.
    pub(in crate::app) fn claude_take_picks(&mut self) {
        let picks: Vec<(usize, (usize, usize))> = self.docs.iter_mut().enumerate().filter_map(|(i, d)| d.claude_pick.take().map(|p| (i, p))).collect();
        for (i, (a, b)) in picks {
            self.claude_attach_lines(i, a, b);
        }
    }

    /// The file being edited, the line of the cursor and the selection, said to Claude.
    /// The definitions in the project of the names `text` cites (at most 4 names, each defined in
    /// at most 2 places: a name found everywhere says nothing), with their first lines; and the
    /// names given.
    pub(super) fn cited_definitions(&self, text: &str, dir: &Path) -> Option<(String, Vec<String>)> {
        const MAX_NAMES: usize = 4;
        const MAX_LINES: usize = 40;
        let ix = self.index.symbols.as_ref()?;
        let (mut parts, mut names) = (Vec::new(), Vec::new());
        for name in cited_names(text) {
            if names.len() >= MAX_NAMES {
                break;
            }
            let found: Vec<(PathBuf, crate::index::Entry)> = ix
                .lookup(&name)
                .into_iter()
                .filter(|(_, e)| e.name == name && matches!(e.kind, crate::symbols::Kind::Class | crate::symbols::Kind::Function | crate::symbols::Kind::Method))
                .collect();
            if found.is_empty() || found.len() > 2 {
                continue;
            }
            for (p, e) in &found {
                let Ok(src) = std::fs::read_to_string(p) else { continue };
                let rel = p.strip_prefix(dir).unwrap_or(p).display().to_string();
                let lang = p.extension().and_then(|x| x.to_str()).unwrap_or("");
                parts.push(format!("{rel}:{} :\n```{lang}\n{}\n```", e.line + 1, definition_lines(&src, e.line, MAX_LINES)));
            }
            names.push(name);
        }
        (!parts.is_empty()).then(|| (tf!("(Définitions trouvées dans le projet pour les noms cités :\n{})", "(Definitions found in the project for the names mentioned:\n{})", parts.join("\n")), names))
    }

    /// The file open in the editor, said to Claude: the text, and the path of the file.
    pub(super) fn editor_context(&self, dir: &Path) -> Option<(String, String)> {
        let d = self.docs.iter().rev().find(|d| d.view == document::View::Text && d.path.is_some())?;
        let path = d.path.as_ref()?;
        let rel = path.strip_prefix(dir).unwrap_or(path).display().to_string();
        let sel = d.primary_sel();
        let (line, _) = d.line_col(sel.head);
        let mut s = tf!("(Fichier ouvert dans l'éditeur : {rel}, ligne {}.", "(File open in the editor: {rel}, line {}.", line + 1);
        if !sel.is_empty() {
            let text: String = d.slice(sel.start(), sel.end()).chars().take(8000).collect();
            s.push_str(&tf!(" Texte sélectionné :\n```\n{text}\n```", " Selected text:\n```\n{text}\n```"));
        }
        s.push(')');
        Some((s, rel))
    }
}
