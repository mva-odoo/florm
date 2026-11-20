//! What Florm gives Claude, written into its field: lines, files, notes, commits, pictures.

use super::*;

/// Something given to Claude: lines of a file, a file / folder, a text (the changes not
/// committed, the problems of a file…) or a picture.
pub(in crate::app) struct Attach {
    pub(super) path: PathBuf,
    /// Lines (1-based, first and last) and their text; None: the whole file or folder.
    pub(super) lines: Option<(usize, usize, String)>,
    /// A text and what it is (instead of a file).
    pub(super) note: Option<(String, String)>,
    /// A picture (a pasted image, a file, a PDF): what it is, and its file.
    pub(super) image: Option<(String, PathBuf)>,
}

impl Attach {

    /// An image (`image`: what it is, its PNG) given with a message of the conversation in `dir`.
    pub(in crate::app) fn image(dir: PathBuf, image: (String, PathBuf)) -> Self {
        Attach { path: dir, lines: None, note: None, image: Some(image) }
    }

    pub(in crate::app) fn label(&self, dir: Option<&Path>) -> String {
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

/// What can be given from « + Contexte ».
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

/// The path of `path` relative to the folder `dir` of a conversation (None outside it).
pub(super) fn rel_path(dir: Option<&Path>, path: &Path) -> Option<String> {
    dir.and_then(|d| path.strip_prefix(d).ok()).map(|r| r.display().to_string())
}

/// Whether `given` (what messages gave) holds the file `rel`, or a part of it (`rel:line`).
pub(super) fn gives(given: &[String], rel: &str) -> bool {
    let prefix = format!("{rel}:");
    given.iter().any(|g| g == rel || g.starts_with(&prefix))
}

impl App {
    /// Lines `a..=b` (0-based) of document `i` given to Claude.
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

    /// A text given to Claude (`label`: what it is).
    pub(super) fn claude_attach_note(&mut self, label: String, text: String) {
        let path = self.claude.dir.clone().unwrap_or_default();
        self.claude_attach(Attach { path, lines: None, note: Some((label, text)), image: None });
    }

    /// An image (PNG) given to Claude; `label`: what it is.
    pub(in crate::app) fn claude_attach_image(&mut self, png: Vec<u8>, label: String) {
        let saved = match claude::save_image(&png) {
            Ok(p) => p,
            Err(e) => return self.set_error(tf!("Image non enregistrée : {e}", "Image not saved: {e}")),
        };
        // its path written into its field: Claude Code reads the image
        let a = Attach::image(self.claude.dir.clone().unwrap_or_default(), (label.clone(), saved));
        let text = term::term_text(&a, self.claude.dir.as_deref());
        self.claude_term_give(text, &label);
    }

    /// A PDF given to Claude: a copy is kept.
    pub(in crate::app) fn claude_attach_pdf(&mut self, path: &Path) {
        let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::metadata(path) {
            Ok(m) if m.len() > claude::PDF_MAX => {
                return self.set_error(tf!("{label} : trop gros pour Claude ({} Mo, 32 au plus)", "{label}: too big for Claude ({} MB, 32 at most)", m.len() / 1024 / 1024));
            }
            Err(e) => return self.set_error(tf!("{label} : {e}", "{label}: {e}")),
            Ok(_) => {}
        }
        let saved = match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| claude::save_media(&b, "pdf")) {
            Ok(p) => p,
            Err(e) => return self.set_error(tf!("PDF non enregistré : {e}", "PDF not saved: {e}")),
        };
        let a = Attach::image(self.claude.dir.clone().unwrap_or_default(), (label.clone(), saved));
        let text = term::term_text(&a, self.claude.dir.as_deref());
        self.claude_term_give(text, &label);
    }

    /// An image file given to Claude (read and made smaller in the background); a PDF as it is.
    pub(in crate::app) fn claude_attach_image_file(&mut self, path: &Path) {
        if claude::is_pdf(path) {
            return self.claude_attach_pdf(path);
        }
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

    /// A commit given to Claude: its message, the files it touched and its
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

    /// What « + Contexte » asked for.
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
        // a file or its lines: « Reprendre la conversation de ce fichier » finds it again
        if a.note.is_none() && a.image.is_none() && !self.claude.given.contains(&label) {
            self.claude.given.push(label.clone());
        }
        let text = term::term_text(&a, self.claude.dir.as_deref());
        self.claude_term_give(text, &label);
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
}
