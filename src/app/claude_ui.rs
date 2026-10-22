//! The Claude tab: a conversation with Claude Code on the left, and on the right the review of
//! what it changed in the files since the review started, block by block (kept or rejected).
//!
//! The review compares the files with a private index (`.git/florm-review-index`) holding them as
//! they were: keeping a block adds it to that index, rejecting it takes it out of the file.

use super::*;
use crate::claude::{self, Event};
use history::{DiffAct, DiffClick};

/// Something given to Claude with the next message: lines of a file, or a file / folder.
#[derive(Clone)]
pub(super) struct Attach {
    path: PathBuf,
    /// Lines (1-based, first and last) and their text; None: the whole file or folder.
    lines: Option<(usize, usize, String)>,
}

impl Attach {
    fn label(&self, dir: Option<&Path>) -> String {
        let rel = dir.and_then(|d| self.path.strip_prefix(d).ok()).unwrap_or(&self.path).display().to_string();
        match &self.lines {
            Some((a, b, _)) if a == b => format!("{rel}:{a}"),
            Some((a, b, _)) => format!("{rel}:{a}-{b}"),
            None => rel,
        }
    }

    /// How it is said to Claude.
    fn prompt(&self, dir: Option<&Path>) -> String {
        let label = self.label(dir);
        match &self.lines {
            Some((_, _, text)) => {
                let lang = self.path.extension().and_then(|e| e.to_str()).unwrap_or("");
                format!("Extrait de {label} :\n```{lang}\n{text}\n```")
            }
            None if self.path.is_dir() => format!("Dossier à prendre en compte : {label}"),
            None => format!("Fichier à prendre en compte : {label}"),
        }
    }
}

/// A line of the conversation.
enum Item {
    /// A message, with what was given with it.
    User(String, Vec<String>),
    /// The answer, being written (pieces) or whole.
    Text(String),
    Streaming(String),
    Tool { id: String, name: String, summary: String, result: Option<(String, bool)> },
    Info(String),
    Error(String),
}

#[derive(Default)]
pub(super) struct ClaudeUi {
    /// Folder of the conversation (the selected repository, else the project).
    dir: Option<PathBuf>,
    session: Option<claude::Session>,
    items: Vec<Item>,
    input: String,
    busy: bool,
    model: usize,
    mode: usize,
    cost: f64,
    /// Tokens of the conversation read by the model for its last message, and its window.
    context: (u64, u64),
    /// Use of the plan's quotas (5 hours, 7 days): kept between conversations.
    limits: Vec<(String, f32, i64)>,
    /// The file being edited and its selection are given with the message.
    with_context: bool,
    /// Lines and files given with the next message.
    pub(super) attach: Vec<Attach>,
    /// First message of the conversation: the wish behind the changes (for the commit message).
    pub(super) wish: Option<String>,
    /// The review: its index and the changes since.
    review_index: Option<PathBuf>,
    review: Option<HistoryDiff>,
    review_loading: bool,
    collapsed: HashSet<usize>,
    jump: Option<history::FileJump>,
    /// "Tout rejeter" clicked once: a second click does it.
    confirm_reject: bool,
    pub(super) focus: bool,
}

/// Review index of a repository (inside .git: not seen by git status, not committed).
fn review_index_of(repo: &Path) -> PathBuf {
    let p = git::stdout(repo, &["rev-parse", "--git-path", "florm-review-index"]).unwrap_or_default();
    let p = PathBuf::from(p.trim());
    if p.is_absolute() {
        p
    } else {
        repo.join(p)
    }
}

/// git with the review index instead of the real one.
fn rgit(repo: &Path, index: &Path, args: &[&str], input: Option<&str>) -> Result<String, String> {
    git::run_with(repo, args, &[("GIT_INDEX_FILE", &index.to_string_lossy())], input)
}

/// The changes since the review started: modified files, then new ones.
fn review_diff(repo: &Path, index: &Path) -> Result<String, String> {
    let mut text = rgit(repo, index, &["-c", "core.quotePath=false", "diff"], None)?;
    let others = rgit(repo, index, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"], None).unwrap_or_default();
    for p in others.lines().filter(|p| !p.is_empty()).take(200) {
        if std::fs::metadata(repo.join(p)).is_ok_and(|m| m.len() > 2_000_000) {
            continue;
        }
        let d = match git::run(repo, &["diff", "--no-index", "--", "/dev/null", p]) {
            Err(e) if e.starts_with("diff --git") => e,
            Ok(o) => o,
            Err(_) => continue,
        };
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&d);
    }
    Ok(text)
}

impl App {
    /// Shows Claude in the bottom panel, or in an editor tab (`claude_in_tab`).
    pub(super) fn open_claude(&mut self) {
        if self.settings.claude_in_tab {
            if self.bottom == Some(BottomTab::Claude) {
                self.bottom = None;
            }
            self.open_view_tab(document::View::Claude, "Claude".into());
        } else {
            self.bottom = Some(BottomTab::Claude);
        }
        self.claude_prepare();
    }

    /// Its folder, the keyboard and the review, wherever Claude is shown.
    pub(super) fn claude_prepare(&mut self) {
        if self.claude.dir.is_none() {
            self.claude.dir = self.repo.clone().or_else(|| self.root.clone());
        }
        self.claude.focus = true;
        self.claude_reload_review();
    }

    /// What Claude sent since the last frame.
    pub(super) fn claude_poll(&mut self) {
        let Some(s) = &self.claude.session else { return };
        let events = s.poll();
        if events.is_empty() {
            return;
        }
        let mut reload = false;
        for e in events {
            let items = &mut self.claude.items;
            match e {
                Event::Init { .. } => {}
                Event::Delta(t) => match items.last_mut() {
                    Some(Item::Streaming(s)) => s.push_str(&t),
                    _ => items.push(Item::Streaming(t)),
                },
                Event::Text(t) => match items.last_mut() {
                    Some(it @ Item::Streaming(_)) => *it = Item::Text(t),
                    _ => items.push(Item::Text(t)),
                },
                Event::Tool { id, name, summary } => {
                    if let Some(Item::Streaming(s)) = items.last() {
                        let s = s.clone();
                        *items.last_mut().unwrap() = Item::Text(s);
                    }
                    items.push(Item::Tool { id, name, summary, result: None });
                }
                Event::ToolResult { id, text, error } => {
                    for it in items.iter_mut().rev() {
                        if let Item::Tool { id: i, name, result, .. } = it {
                            if *i == id {
                                reload |= claude::changes_files(name);
                                *result = Some((text, error));
                                break;
                            }
                        }
                    }
                }
                Event::Context(tokens) => self.claude.context.0 = tokens,
                Event::Limits(l) => self.claude.limits = l,
                Event::Done { error, text, cost, window } => {
                    self.claude.busy = false;
                    self.claude.cost += cost;
                    if window > 0 {
                        self.claude.context.1 = window;
                    }
                    if let Some(Item::Streaming(s)) = items.last() {
                        let s = s.clone();
                        *items.last_mut().unwrap() = Item::Text(s);
                    }
                    if error && !text.is_empty() {
                        items.push(Item::Error(text));
                    }
                    reload = true;
                }
                Event::Exited(err) => {
                    self.claude.busy = false;
                    self.claude.session = None;
                    let msg = if err.is_empty() { "Claude s'est arrêté : un nouveau message démarre une nouvelle conversation.".to_string() } else { err };
                    items.push(Item::Error(msg));
                    break;
                }
            }
        }
        if reload {
            self.claude_reload_review();
            // the files changed: read again at once
            self.last_disk_check = Instant::now() - Duration::from_secs(10);
            self.git_refresh();
        }
    }

    fn claude_send(&mut self) {
        let text = self.claude.input.trim().to_string();
        let Some(dir) = self.claude.dir.clone() else {
            self.set_status("Aucun dossier ouvert");
            return;
        };
        if text.is_empty() || self.claude.busy {
            return;
        }
        // the review starts with the first message: everything changed since is Claude's
        if self.claude.review_index.is_none() {
            self.claude_start_review();
        }
        let mut message = text.clone();
        let attach = std::mem::take(&mut self.claude.attach);
        let given: Vec<String> = attach.iter().map(|a| a.label(Some(&dir))).collect();
        if !attach.is_empty() {
            let parts: Vec<String> = attach.iter().map(|a| a.prompt(Some(&dir))).collect();
            message = format!("{}\n\n{message}", parts.join("\n\n"));
        }
        if self.claude.with_context {
            if let Some(ctx) = self.editor_context(&dir) {
                message = format!("{ctx}\n\n{message}");
            }
        }
        if self.claude.session.is_none() {
            let (model, mode) = (claude::MODELS[self.claude.model].0, claude::MODES[self.claude.mode].0);
            let ctx = self.egui_ctx.clone();
            match claude::Session::start(&dir, model, mode, move || ctx.request_repaint()) {
                Ok(s) => self.claude.session = Some(s),
                Err(e) => {
                    self.claude.items.push(Item::Error(e));
                    self.claude.attach = attach;
                    return;
                }
            }
        }
        let Some(s) = self.claude.session.as_mut() else { return };
        match s.send(&message) {
            Ok(()) => {
                self.claude.items.push(Item::User(text.clone(), given));
                self.claude.wish.get_or_insert(text);
                self.claude.input.clear();
                self.claude.busy = true;
            }
            Err(e) => {
                self.claude.session = None;
                self.claude.items.push(Item::Error(e));
                self.claude.attach = attach;
            }
        }
    }

    /// Lines `a..=b` (0-based) of document `i` given to Claude with the next message.
    pub(super) fn claude_attach_lines(&mut self, i: usize, a: usize, b: usize) {
        let Some(d) = self.docs.get(i) else { return };
        let Some(path) = d.path.clone() else {
            self.set_status("Enregistre d'abord le fichier pour le donner à Claude");
            return;
        };
        let b = b.min(d.rope.len_lines().saturating_sub(1));
        let text: String = (a..=b).map(|l| d.line_text(l)).collect::<Vec<_>>().join("\n");
        self.claude_attach(Attach { path, lines: Some((a + 1, b + 1, text)) });
    }

    pub(super) fn claude_attach_path(&mut self, path: PathBuf) {
        self.claude_attach(Attach { path, lines: None });
    }

    fn claude_attach(&mut self, a: Attach) {
        if self.claude.dir.is_none() {
            self.claude.dir = self.repo_for(&a.path).or_else(|| self.repo.clone()).or_else(|| self.root.clone());
        }
        let label = a.label(self.claude.dir.as_deref());
        if !self.claude.attach.iter().any(|x| x.path == a.path && x.lines.as_ref().map(|l| (l.0, l.1)) == a.lines.as_ref().map(|l| (l.0, l.1))) {
            self.claude.attach.push(a);
        }
        self.set_status(format!("✳ {label} ajouté au contexte de Claude ({} élément(s)) — Ctrl+Alt+C pour lui écrire", self.claude.attach.len()));
    }

    /// The selection (else the line of the cursor) of the current file for Claude; `ask`: and
    /// the Claude tab opened to write the question.
    pub(super) fn claude_attach_selection(&mut self, ask: bool) {
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
    pub(super) fn claude_take_picks(&mut self) {
        let picks: Vec<(usize, (usize, usize))> = self.docs.iter_mut().enumerate().filter_map(|(i, d)| d.claude_pick.take().map(|p| (i, p))).collect();
        for (i, (a, b)) in picks {
            self.claude_attach_lines(i, a, b);
        }
    }

    /// The file being edited, the line of the cursor and the selection, said to Claude.
    fn editor_context(&self, dir: &Path) -> Option<String> {
        let d = self.docs.iter().rev().find(|d| d.view == document::View::Text && d.path.is_some())?;
        let path = d.path.as_ref()?;
        let rel = path.strip_prefix(dir).unwrap_or(path).display().to_string();
        let sel = d.primary_sel();
        let (line, _) = d.line_col(sel.head);
        let mut s = format!("(Fichier ouvert dans l'éditeur : {rel}, ligne {}.", line + 1);
        if !sel.is_empty() {
            let text: String = d.slice(sel.start(), sel.end()).chars().take(8000).collect();
            s.push_str(&format!(" Texte sélectionné :\n```\n{text}\n```"));
        }
        s.push(')');
        Some(s)
    }

    /// The review starts from the files as they are now.
    fn claude_start_review(&mut self) {
        let Some(repo) = self.claude.dir.clone().filter(|d| d.join(".git").exists()) else { return };
        let index = review_index_of(&repo);
        let r = rgit(&repo, &index, &["read-tree", "HEAD"], None).and_then(|_| rgit(&repo, &index, &["add", "-A"], None));
        match r {
            Ok(_) => {
                self.claude.review_index = Some(index);
                self.claude.review = None;
                self.claude.collapsed.clear();
            }
            Err(e) => self.claude.items.push(Item::Error(format!("Revue impossible : {e}"))),
        }
    }

    fn claude_reload_review(&mut self) {
        let (Some(repo), Some(index)) = (self.claude.dir.clone(), self.claude.review_index.clone()) else { return };
        self.claude.review_loading = true;
        self.jobs.spawn(move || JobResult::ClaudeReview(review_diff(&repo, &index)));
    }

    pub(super) fn claude_review_loaded(&mut self, r: Result<String, String>) {
        self.claude.review_loading = false;
        let text = r.unwrap_or_else(|e| e);
        let lines = history::parse_diff(&text);
        let files = history::diff_files(&lines);
        self.claude.review = Some(HistoryDiff { hash: String::new(), path: None, text, lines, files });
    }

    /// Keeps or rejects a block or a file of the review.
    fn claude_review_action(&mut self, click: DiffClick) {
        let (Some(repo), Some(index), Some(d)) = (self.claude.dir.clone(), self.claude.review_index.clone(), self.claude.review.as_ref()) else { return };
        let idx = match click {
            DiffClick::Line(i) | DiffClick::Hunk(i, _) | DiffClick::File(i, _) => i,
        };
        let Some(path) = history::file_at(&d.text, idx) else { return };
        let new_file = history::file_patch(&d.text, idx).is_some_and(|f| f.contains("\n--- /dev/null"));
        let text = d.text.clone();
        let r = match click {
            DiffClick::Line(_) | DiffClick::File(_, DiffAct::Open) => {
                let line = history::target_line(&d.lines, idx);
                self.pr_open_path(&repo.join(&path), line);
                return;
            }
            DiffClick::File(_, DiffAct::Stage) => rgit(&repo, &index, &["add", "-A", "--", &path], None),
            DiffClick::Hunk(_, DiffAct::Stage) if new_file => rgit(&repo, &index, &["add", "--", &path], None),
            DiffClick::Hunk(i, DiffAct::Stage) => match history::hunk_patch(&text, i) {
                Some(p) => rgit(&repo, &index, &["apply", "--cached", "--recount", "-"], Some(&p)),
                None => return,
            },
            DiffClick::File(..) | DiffClick::Hunk(..) if new_file => std::fs::remove_file(repo.join(&path)).map(|_| String::new()).map_err(|e| e.to_string()),
            DiffClick::File(..) => {
                // the file as it was (or removed again when Claude deleted it… restored)
                rgit(&repo, &index, &["checkout-index", "-f", "--", &path], None)
            }
            DiffClick::Hunk(i, _) => match history::hunk_patch(&text, i) {
                Some(p) => git::run_with(&repo, &["apply", "-R", "--recount", "-"], &[], Some(&p)),
                None => return,
            },
        };
        // shown at once without it, git's new diff follows
        if let Some(d) = self.claude.review.as_mut() {
            d.text = history::without(&d.text, idx, matches!(click, DiffClick::File(..)));
            d.lines = history::parse_diff(&d.text);
            d.files = history::diff_files(&d.lines);
        }
        if let Err(e) = r {
            self.claude.items.push(Item::Error(format!("{path} : {e}")));
        }
        self.last_disk_check = Instant::now() - Duration::from_secs(10);
        self.claude_reload_review();
        self.git_refresh();
    }

    /// Keeps everything (the review starts again from here) or rejects everything.
    fn claude_review_all(&mut self, keep: bool) {
        let (Some(repo), Some(index)) = (self.claude.dir.clone(), self.claude.review_index.clone()) else { return };
        let r = if keep {
            rgit(&repo, &index, &["add", "-A"], None)
        } else {
            let news = rgit(&repo, &index, &["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"], None).unwrap_or_default();
            for p in news.lines().filter(|p| !p.is_empty()) {
                let _ = std::fs::remove_file(repo.join(p));
            }
            rgit(&repo, &index, &["checkout-index", "-a", "-f"], None)
        };
        if let Err(e) = r {
            self.claude.items.push(Item::Error(e));
        }
        self.claude.confirm_reject = false;
        self.last_disk_check = Instant::now() - Duration::from_secs(10);
        self.claude_reload_review();
        self.git_refresh();
    }

    fn pr_open_path(&mut self, path: &Path, line: usize) {
        if self.open_path(path.to_path_buf()) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.goto_line(line.saturating_sub(1), 0);
            }
            self.pending_focus = true;
        }
    }

    /// Claude into an editor tab (⇱, more room) or back into the bottom panel (⇲), remembered.
    pub(super) fn move_claude(&mut self, to_tab: bool) {
        if !to_tab {
            if let Some(i) = self.docs.iter().position(|d| d.view == document::View::Claude) {
                self.close_tab(i, true);
            }
        }
        self.settings.claude_in_tab = to_tab;
        self.persist("claude_in_tab", to_tab.to_string());
        self.open_claude();
    }

    pub(super) fn claude_ui(&mut self, ui: &mut egui::Ui, in_tab: bool) {
        let accent = self.hl.colors.accent;
        let fill = ui.visuals().panel_fill;
        let dir_name = self.claude.dir.as_deref().map(|d| self.repo_label(d)).unwrap_or_default();
        let dirs: Vec<(PathBuf, String)> = self.repos.iter().map(|r| (r.clone(), self.repo_label(r))).collect();
        let (mut send, mut stop, mut new_conv, mut review_click, mut all) = (false, false, false, None, None);
        let (mut chosen_dir, mut move_to) = (None, None);
        let c = &mut self.claude;
        let cache = &mut self.md_cache;

        // ------------------------------------------------ settings of the conversation
        egui::TopBottomPanel::top("claude_bar")
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 8.0, bottom: 6.0 }))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("✳ Claude").strong().size(15.0).color(accent));
                    let running = c.session.is_some();
                    ui.add_enabled_ui(!running, |ui| {
                        if dirs.len() > 1 {
                            let labels: Vec<String> = dirs.iter().map(|(_, l)| l.clone()).collect();
                            let current = dirs.iter().position(|(d, _)| c.dir.as_ref() == Some(d));
                            if let Some(k) = crate::pick::combo(ui, "claude_dir", &dir_name, &labels, current) {
                                chosen_dir = Some(dirs[k].0.clone());
                            }
                        } else {
                            ui.label(RichText::new(&dir_name).weak());
                        }
                        egui::ComboBox::from_id_salt("claude_model").selected_text(claude::MODELS[c.model].1).show_ui(ui, |ui| {
                            for (k, (_, label)) in claude::MODELS.iter().enumerate() {
                                ui.selectable_value(&mut c.model, k, *label);
                            }
                        });
                        egui::ComboBox::from_id_salt("claude_mode").selected_text(claude::MODES[c.mode].1).show_ui(ui, |ui| {
                            for (k, (_, label, tip)) in claude::MODES.iter().enumerate() {
                                ui.selectable_value(&mut c.mode, k, *label).on_hover_text(*tip);
                            }
                        });
                    })
                    .response
                    .on_disabled_hover_text("Pour changer : « Nouvelle conversation »");
                    if ui.button("Nouvelle conversation").on_hover_text("Oublie la conversation (la revue des changements reste)").clicked() {
                        new_conv = true;
                    }
                    if c.cost > 0.0 {
                        ui.label(RichText::new(format!("{:.2} $", c.cost)).weak().small()).on_hover_text("Coût estimé de la conversation");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if in_tab {
                            if ui.small_button("⇲ Panneau du bas").on_hover_text("Afficher Claude dans le panneau du bas").clicked() {
                                move_to = Some(false);
                            }
                        } else if ui.small_button("⇱ Onglet").on_hover_text("Afficher Claude dans un onglet de l'éditeur (plus de place)").clicked() {
                            move_to = Some(true);
                        }
                        ui.add_space(8.0);
                        // right to left: the plan's quotas, then the context
                        for (window, used, reset) in c.limits.iter().rev() {
                            let (label, what) = match window.as_str() {
                                "five_hour" => ("5 h", "sur 5 heures"),
                                _ => ("7 j", "sur 7 jours"),
                            };
                            let tip = format!("Quota de ton abonnement {what} : {:.0} % utilisé, remis à zéro {}", used * 100.0, until(*reset));
                            gauge(ui, label, *used, accent, tip);
                        }
                        let (tokens, window) = c.context;
                        if tokens > 0 && window > 0 {
                            let used = (tokens as f32 / window as f32).min(1.0);
                            let tip = format!(
                                "Contexte de la conversation : {} / {} tokens.\nPlein, Claude résume le début de la conversation ; « Nouvelle conversation » repart de zéro.",
                                thousands(tokens),
                                thousands(window)
                            );
                            gauge(ui, "Contexte", used, accent, tip);
                        }
                    });
                });
            });

        // ------------------------------------------------ the review, on the right
        egui::SidePanel::right("claude_review").resizable(true).default_width(ui.available_width() * 0.5).width_range(320.0..=1400.0).show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("CE QUE CLAUDE A CHANGÉ").small().strong().color(accent));
                let n = c.review.as_ref().map_or(0, |d| d.files.len());
                if n > 0 {
                    ui.label(RichText::new(format!("{n} fichier(s)")).small().weak());
                }
                if c.review_loading {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if n > 0 {
                        let label = if c.confirm_reject { "Confirmer : tout rejeter" } else { "↺ Tout rejeter" };
                        if ui.add(egui::Button::new(RichText::new(label).color(err())).small()).on_hover_text("Remettre les fichiers comme au début de la revue (les nouveaux fichiers sont supprimés)").clicked() {
                            if c.confirm_reject {
                                all = Some(false);
                            } else {
                                c.confirm_reject = true;
                            }
                        }
                        if ui.small_button("✔ Tout garder").on_hover_text("Tout accepter : la revue repart de là").clicked() {
                            all = Some(true);
                        }
                    }
                });
            });
            ui.separator();
            match &c.review {
                _ if c.review_index.is_none() => {
                    ui.label(RichText::new("Les fichiers modifiés par Claude apparaîtront ici, bloc par bloc : tu gardes ou tu rejettes chaque partie.").weak());
                }
                Some(d) if d.files.is_empty() => {
                    ui.label(RichText::new("Aucun changement à revoir.").weak());
                }
                Some(d) => {
                    let hunk_buttons: &[history::DiffButton] = &[
                        ("✔ Garder", "Accepter ce bloc (il quitte la revue, il reste dans le fichier)", DiffAct::Stage),
                        ("↺ Rejeter", "Retirer ce bloc du fichier", DiffAct::Discard),
                    ];
                    let file_buttons: &[history::DiffButton] = &[
                        ("Ouvrir", "Ouvrir le fichier", DiffAct::Open),
                        ("✔ Garder", "Accepter tout le fichier", DiffAct::Stage),
                        ("↺ Rejeter", "Remettre le fichier comme avant (supprimé s'il est nouveau)", DiffAct::Discard),
                    ];
                    let jump = c.jump.take();
                    review_click = history::stacked_diff_view(ui, &d.lines, &d.files, &mut c.collapsed, "claude_review", hunk_buttons, file_buttons, jump, None);
                }
                None => {
                    ui.spinner();
                }
            }
        });

        // ------------------------------------------------ the conversation
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::TopBottomPanel::bottom("claude_input").show_inside(ui, |ui| {
                ui.add_space(6.0);
                if !c.attach.is_empty() {
                    let mut remove = None;
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Contexte :").small().weak());
                        for (k, a) in c.attach.iter().enumerate() {
                            let chip = egui::Button::new(RichText::new(format!("✳ {}  ×", a.label(c.dir.as_deref()))).small().color(accent))
                                .fill(icons::mix(fill, accent, 0.12))
                                .rounding(10.0);
                            let tip = a.lines.as_ref().map_or_else(|| a.path.display().to_string(), |(_, _, t)| t.lines().take(20).collect::<Vec<_>>().join("\n"));
                            if ui.add(chip).on_hover_text(format!("{tip}\n\nClic : retirer")).clicked() {
                                remove = Some(k);
                            }
                        }
                    });
                    if let Some(k) = remove {
                        c.attach.remove(k);
                    }
                }
                let rows = match (c.input.lines().count() > 2, in_tab) {
                    (true, true) => 6,
                    (true, false) => 4,
                    (false, true) => 3,
                    (false, false) => 2,
                };
                let resp = ui.add(
                    egui::TextEdit::multiline(&mut c.input)
                        .hint_text("Demande à Claude… (Ctrl+Entrée pour envoyer)")
                        .desired_rows(rows)
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut c.focus) {
                    resp.request_focus();
                }
                if resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter)) {
                    send = true;
                }
                ui.horizontal(|ui| {
                    if c.busy {
                        if ui.button("■ Arrêter").on_hover_text("Arrêter la réponse en cours (la conversation continue)").clicked() {
                            stop = true;
                        }
                        ui.spinner();
                        ui.label(RichText::new("Claude travaille…").weak());
                    } else if ui.add_enabled(!c.input.trim().is_empty(), filled_button("Envoyer", accent)).clicked() {
                        send = true;
                    }
                    ui.checkbox(&mut c.with_context, "Fichier ouvert et sélection").on_hover_text("Dit à Claude quel fichier tu édites, la ligne du curseur et le texte sélectionné");
                });
                ui.add_space(4.0);
            });
            egui::ScrollArea::vertical().id_salt("claude_chat").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(900.0));
                if c.items.is_empty() {
                    ui.add_space(20.0);
                    ui.label(RichText::new("Demande quelque chose à Claude : il lit le projet et modifie les fichiers. Tu revois ses changements à droite, bloc par bloc.").weak());
                    ui.add_space(6.0);
                    ui.label(RichText::new("Florm utilise la commande claude (Claude Code) et ta connexion : pas de clé à configurer.").weak().small());
                }
                for it in &c.items {
                    ui.add_space(6.0);
                    match it {
                        Item::User(t, given) => {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                egui::Frame::none()
                                    .fill(icons::mix(fill, accent, 0.16))
                                    .rounding(egui::Rounding { nw: 10.0, ne: 2.0, sw: 10.0, se: 10.0 })
                                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                    .show(ui, |ui| {
                                        ui.set_max_width(ui.available_width() * 0.8);
                                        ui.vertical(|ui| {
                                            ui.add(egui::Label::new(t.as_str()).wrap());
                                            if !given.is_empty() {
                                                ui.label(RichText::new(format!("✳ {}", given.join(", "))).small().weak());
                                            }
                                        });
                                    });
                            });
                        }
                        Item::Text(t) | Item::Streaming(t) => {
                            egui_commonmark::CommonMarkViewer::new().show(ui, cache, t);
                        }
                        Item::Tool { name, summary, result, .. } => {
                            let (icon, color) = match name.as_str() {
                                "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => ("✏", warn()),
                                "Bash" => ("$", accent),
                                _ => ("·", ui.visuals().weak_text_color()),
                            };
                            let error = result.as_ref().is_some_and(|(_, e)| *e);
                            let head = RichText::new(format!("{icon} {name}  {summary}")).monospace().size(12.0).color(if error { err() } else { color });
                            match result.as_ref().filter(|(t, _)| !t.trim().is_empty() && (name == "Bash" || error)) {
                                Some((t, _)) => {
                                    egui::CollapsingHeader::new(head).id_salt(ui.next_auto_id()).show(ui, |ui| {
                                        let t: String = t.lines().take(80).collect::<Vec<_>>().join("\n");
                                        ui.label(RichText::new(t).monospace().size(11.5).weak());
                                    });
                                }
                                None => {
                                    ui.horizontal(|ui| {
                                        ui.label(head);
                                        if result.is_none() {
                                            ui.spinner();
                                        }
                                    });
                                }
                            }
                        }
                        Item::Info(t) => {
                            ui.label(RichText::new(t).weak().italics());
                        }
                        Item::Error(t) => {
                            ui.label(RichText::new(format!("⚠ {t}")).color(err()));
                        }
                    }
                }
                ui.add_space(10.0);
            });
        });

        if let Some(t) = move_to {
            self.move_claude(t);
        }
        if let Some(d) = chosen_dir {
            self.claude.dir = Some(d);
            self.claude.review_index = None;
            self.claude.review = None;
        }
        if new_conv {
            self.claude.session = None;
            self.claude.busy = false;
            self.claude.wish = None;
            self.claude.cost = 0.0;
            self.claude.context.0 = 0;
            self.claude.items.push(Item::Info("— nouvelle conversation —".into()));
        }
        if stop {
            if let Some(s) = self.claude.session.as_mut() {
                let _ = s.interrupt();
            }
        }
        if send {
            self.claude_send();
        }
        if let Some(c) = review_click {
            self.claude_review_action(c);
        }
        if let Some(keep) = all {
            self.claude_review_all(keep);
        }
    }

    /// A commit message proposed by Claude for what is staged (else for all the changes).
    pub(super) fn suggest_commit_message(&mut self) {
        let Some(repo) = self.repo.clone() else { return };
        let staged = self.git.status.as_ref().is_some_and(|s| s.entries.iter().any(|e| e.staged()));
        let wish = self.claude.wish.clone().filter(|_| self.claude.dir.as_ref() == Some(&repo)).unwrap_or_default();
        self.git.suggesting = true;
        self.jobs.spawn(move || {
            let diff = if staged { git::run(&repo, &["diff", "--cached"]) } else { git::run(&repo, &["diff", "HEAD"]) }.unwrap_or_default();
            let recent = git::stdout(&repo, &["log", "-12", "--format=%s"]).unwrap_or_default();
            let r = if diff.trim().is_empty() { Err("Rien à décrire : aucun changement".to_string()) } else { claude::commit_message(&repo, &diff, &recent, &wish) };
            JobResult::CommitMessage(r)
        });
    }

    pub(super) fn commit_message_ready(&mut self, r: Result<String, String>) {
        self.git.suggesting = false;
        match r {
            Ok(m) => {
                self.git.commit_msg = m;
                self.set_status("Message de commit proposé par Claude — à relire");
            }
            Err(e) => self.set_status(format!("Pas de message proposé : {}", e.lines().next().unwrap_or(""))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        git::run(dir, args).unwrap();
    }

    #[test]
    fn what_is_given_to_claude() {
        let dir = Path::new("/p");
        let a = Attach { path: "/p/src/m.py".into(), lines: Some((3, 4, "a\nb".into())) };
        assert_eq!(a.label(Some(dir)), "src/m.py:3-4");
        assert_eq!(a.prompt(Some(dir)), "Extrait de src/m.py:3-4 :\n```py\na\nb\n```");
        let f = Attach { path: "/p/src/m.py".into(), lines: None };
        assert_eq!(f.prompt(Some(dir)), "Fichier à prendre en compte : src/m.py");
    }

    #[test]
    fn review_keeps_and_rejects() {
        let dir = std::env::temp_dir().join(format!("florm_review_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["init", "-q"]);
        let lines: String = (1..=30).map(|i| format!("l{i}\n")).collect();
        std::fs::write(dir.join("a.txt"), &lines).unwrap();
        sh(&dir, &["add", "."]);
        sh(&dir, &["-c", "user.email=a@b", "-c", "user.name=a", "commit", "-qm", "init"]);
        // a change of the user before the review: not Claude's
        std::fs::write(dir.join("mine.txt"), "à moi\n").unwrap();
        let index = review_index_of(&dir);
        rgit(&dir, &index, &["read-tree", "HEAD"], None).unwrap();
        rgit(&dir, &index, &["add", "-A"], None).unwrap();
        assert_eq!(review_diff(&dir, &index).unwrap(), "");
        // Claude changes two places and adds a file
        std::fs::write(dir.join("a.txt"), lines.replace("l2\n", "DEUX\n").replace("l28\n", "VINGT-HUIT\n")).unwrap();
        std::fs::write(dir.join("new.txt"), "nouveau\n").unwrap();
        let d = review_diff(&dir, &index).unwrap();
        assert!(d.contains("+DEUX") && d.contains("+VINGT-HUIT") && d.contains("+nouveau") && !d.contains("à moi"), "{d}");
        // keep the first block, reject the second
        let hunks: Vec<usize> = d.split('\n').enumerate().filter(|(_, l)| l.starts_with("@@")).map(|(i, _)| i).collect();
        rgit(&dir, &index, &["apply", "--cached", "--recount", "-"], Some(&history::hunk_patch(&d, hunks[0]).unwrap())).unwrap();
        git::run_with(&dir, &["apply", "-R", "--recount", "-"], &[], Some(&history::hunk_patch(&d, hunks[1]).unwrap())).unwrap();
        let a = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(a.contains("DEUX") && a.contains("l28") && !a.contains("VINGT"));
        let d = review_diff(&dir, &index).unwrap();
        assert!(!d.contains("a.txt") && d.contains("new.txt"), "{d}");
        // the real index was not touched
        assert_eq!(git::run(&dir, &["diff", "--cached", "--name-only"]).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A small gauge (right to left: the bar, then its label), red when nearly full.
fn gauge(ui: &mut egui::Ui, label: &str, used: f32, accent: Color32, tip: String) {
    let color = if used >= 0.9 { err() } else if used >= 0.7 { warn() } else { accent };
    let (rect, r) = ui.allocate_exact_size(egui::vec2(46.0, 14.0), Sense::hover());
    let bar = egui::Rect::from_center_size(rect.center(), egui::vec2(46.0, 6.0));
    ui.painter().rect_filled(bar, 3.0, ui.visuals().widgets.inactive.bg_fill);
    ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * used, bar.height())), 3.0, color);
    let text = ui.label(RichText::new(format!("{label} {:.0} %", used * 100.0)).small().weak());
    r.union(text).on_hover_text(tip);
    ui.add_space(6.0);
}

/// "dans 2 h 05", "dans 3 j" until a unix time.
fn until(ts: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let s = (ts - now).max(0);
    match s {
        0..=59 => "maintenant".into(),
        60..=3599 => format!("dans {} min", s / 60),
        3600..=86_399 => format!("dans {} h {:02}", s / 3600, s % 3600 / 60),
        _ => format!("dans {} j {} h", s / 86_400, s % 86_400 / 3600),
    }
}

/// 1234567 → "1 234 567".
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}
