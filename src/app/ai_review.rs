//! AI review by Claude, of a pull request or of the changes not committed yet. Its remarks are
//! only proposals, under the lines of the diff: each one is edited, kept (checked) or dismissed.
//! Nothing is sent by itself: a button publishes the kept ones on the pull request (one GitHub
//! review), or writes them in the Claude tab for it to fix them.

use super::*;
use crate::claude::{self, ReviewNote};
use crate::github;

pub(super) struct AiNote {
    pub(super) note: ReviewNote,
    pub(super) keep: bool,
    /// Its text shown as a field to change it (else rendered).
    editing: bool,
    /// Who it is, for the answers of Claude about it.
    id: u64,
    /// The talk with Claude about it: (written by the developer, text).
    thread: Vec<(bool, String)>,
    /// The field of the talk is shown, and what is being written in it.
    chat: bool,
    draft: String,
    /// Claude is answering.
    asking: bool,
    /// Its text before Claude rewrote it (↶ brings it back).
    before: Option<String>,
    /// The field of the talk takes the keyboard (once, as it opens).
    focus: bool,
}

impl AiNote {
    fn new(note: ReviewNote) -> Self {
        static N: AtomicU64 = AtomicU64::new(1);
        AiNote { note, keep: false, editing: false, id: N.fetch_add(1, Ordering::Relaxed), thread: Vec::new(), chat: false, draft: String::new(), asking: false, before: None, focus: false }
    }
}

#[derive(Default)]
pub(super) struct AiReview {
    /// What is reviewed: an answer is for it only if nothing else was asked since.
    key: String,
    /// Repository reviewed.
    pub(super) repo: Option<PathBuf>,
    pub(super) notes: Vec<AiNote>,
    pub(super) loading: bool,
    error: Option<String>,
    /// The answer arrived (no remark: nothing to say).
    done: bool,
    /// What the changes are, their diff (None: read again, the changes not committed) and the
    /// language of the remarks: for the talks about a remark.
    about: String,
    diff: Option<String>,
    language: String,
}

/// What a remark asked for during the frame.
pub(super) enum NoteAct {
    /// Open the file at this line.
    Open(String, usize),
    Dismiss(usize),
    /// Send what is written in the talk of a remark to Claude.
    Ask(usize),
    /// Close the review (its remarks are forgotten).
    Close,
}

impl AiReview {
    fn start(&mut self, repo: PathBuf, what: &str) {
        // a counter in the key: a late answer to an earlier review is ignored
        static N: AtomicU64 = AtomicU64::new(0);
        let key = format!("{what}#{}", N.fetch_add(1, Ordering::Relaxed));
        *self = AiReview { key, repo: Some(repo), loading: true, ..Default::default() };
    }

    pub(super) fn shown(&self) -> bool {
        self.loading || self.done || self.error.is_some()
    }

    pub(super) fn kept(&self) -> impl Iterator<Item = &ReviewNote> {
        self.notes.iter().filter(|n| n.keep).map(|n| &n.note)
    }

    /// Row of the diff each remark goes under (None: not on a line of the diff).
    pub(super) fn rows(&self, diff: Option<&HistoryDiff>) -> Vec<Option<usize>> {
        self.notes.iter().map(|n| diff.and_then(|d| diff_row(d, &n.note.path, n.note.line, &n.note.side))).collect()
    }

    /// Dismisses or closes; returns the place to open.
    pub(super) fn apply(&mut self, act: NoteAct) -> Option<(String, usize)> {
        match act {
            NoteAct::Open(path, line) => return Some((path, line)),
            NoteAct::Dismiss(k) if k < self.notes.len() => {
                self.notes.remove(k);
            }
            NoteAct::Dismiss(_) | NoteAct::Ask(_) => {}
            NoteAct::Close => *self = AiReview::default(),
        }
        None
    }
}

/// Row of `diff` showing line `line` of `path` (`side` "LEFT": of the old version).
pub(super) fn diff_row(diff: &HistoryDiff, path: &str, line: usize, side: &str) -> Option<usize> {
    let f = diff.files.iter().position(|f| f.path == path)?;
    let end = diff.files.get(f + 1).map_or(diff.lines.len(), |n| n.start);
    (diff.files[f].start..end).find(|&i| {
        let l = &diff.lines[i];
        match side {
            "LEFT" => l.kind != history::DiffKind::Added && l.kind != history::DiffKind::Hunk && l.old == Some(line),
            _ => l.kind != history::DiffKind::Removed && l.kind != history::DiffKind::Hunk && l.new == Some(line),
        }
    })
}

/// Label and color of a kind of remark.
fn severity(s: &str, accent: Color32) -> (&'static str, Color32) {
    match s {
        "bug" => ("Bug", err()),
        "risk" => (t!("Risque", "Risk"), warn()),
        "nit" => (t!("Détail", "Nit"), Color32::GRAY),
        _ => ("Suggestion", accent),
    }
}

/// A small colored label: « Bug », « 3 gardées »…
fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    let bg = icons::mix(ui.visuals().panel_fill, color, 0.22);
    ui.label(RichText::new(format!(" {text} ")).small().strong().color(color).background_color(bg));
}

/// A remark like a review comment: an edge of the color of its kind, its kind and place, the
/// buttons (keep, edit, dismiss), then its text rendered (or a field while it is edited), its
/// suggested code apart. `in_diff`: shown under its line (the file is not repeated).
pub(super) fn note_card(ui: &mut egui::Ui, cache: &mut egui_commonmark::CommonMarkCache, k: usize, n: &mut AiNote, in_diff: bool, accent: Color32, act: &mut Option<NoteAct>) {
    let (label, color) = severity(&n.note.severity, accent);
    let v = ui.visuals().clone();
    ui.add_space(5.0);
    let frame = egui::Frame::NONE
        .fill(icons::mix(v.panel_fill, color, if n.keep { 0.10 } else { 0.035 }))
        .stroke(egui::Stroke::new(1.0_f32, icons::mix(v.panel_fill, color, if n.keep { 0.7 } else { 0.25 })))
        .corner_radius(8.0)
        .inner_margin(egui::Margin { left: 16, right: 10, top: 8, bottom: 10 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("✳").color(color));
                pill(ui, label, color);
                if !n.note.path.is_empty() {
                    let place = match (in_diff, n.note.line) {
                        (true, l) => tf!("ligne {l}", "line {l}"),
                        (false, 0) => n.note.path.clone(),
                        (false, l) => format!("{}:{l}", n.note.path),
                    };
                    if ui.link(RichText::new(place).monospace().small()).on_hover_text(t!("Ouvrir le fichier à cette ligne", "Open the file at this line")).clicked() {
                        *act = Some(NoteAct::Open(n.note.path.clone(), n.note.line.max(1)));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text(t!("Écarter cette remarque", "Drop this remark")).clicked() {
                        *act = Some(NoteAct::Dismiss(k));
                    }
                    let talk = if n.thread.is_empty() { "💬".to_string() } else { format!("💬 {}", n.thread.len()) };
                    if ui.small_button(talk).on_hover_text(t!("En discuter avec Claude pour l'affiner (plus court, en anglais, tu es sûr ?…)", "Talk it over with Claude to refine it (shorter, in English, are you sure?…)")).clicked() {
                        n.chat = !n.chat;
                        n.focus = n.chat;
                    }
                    let (edit, tip) = if n.editing { (t!("✔ Fini", "✔ Done"), t!("Revenir au rendu", "Back to the rendering")) } else { ("✏", t!("Modifier le texte avant de le garder", "Edit the text before keeping it")) };
                    if ui.small_button(edit).on_hover_text(tip).clicked() {
                        n.editing = !n.editing;
                    }
                    let keep = if n.keep {
                        egui::Button::new(RichText::new(t!("✔ Gardée", "✔ Kept")).small().strong().color(text_on(color))).fill(color)
                    } else {
                        egui::Button::new(RichText::new(t!("Garder", "Keep")).small().color(color)).fill(Color32::TRANSPARENT).stroke(egui::Stroke::new(1.0_f32, color))
                    };
                    if ui.add(keep.corner_radius(10.0)).on_hover_text(if n.keep { t!("Ne plus la garder", "Stop keeping it") } else { t!("La garder", "Keep it") }).clicked() {
                        n.keep = !n.keep;
                    }
                });
            });
            ui.add_space(4.0);
            if n.editing {
                ui.add(egui::TextEdit::multiline(&mut n.note.body).desired_rows(3).desired_width(f32::INFINITY).hint_text(t!("Markdown ; ```suggestion pour proposer du code", "Markdown; ```suggestion to suggest code")));
                return;
            }
            for part in github::parts(&n.note.body) {
                match part {
                    github::Part::Text(t) => super::md::markdown(ui, cache, None, &t),
                    github::Part::Suggestion(code) => suggestion(ui, &code),
                }
            }
            if n.before.is_some() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t!("✳ Réécrite par Claude", "✳ Rewritten by Claude")).small().color(color));
                    if ui.small_button(t!("↶ Version d'avant", "↶ Previous version")).clicked() {
                        if let Some(b) = n.before.take() {
                            n.note.body = b;
                        }
                    }
                });
            }
            if n.chat || n.asking {
                talk(ui, cache, k, n, color, act);
            }
        });
    // the edge, of the color of its kind
    let r = frame.response.rect;
    let edge = egui::Rect::from_min_size(r.min, egui::vec2(4.0, r.height()));
    ui.painter().rect_filled(edge, egui::CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 }, color);
    ui.add_space(5.0);
}

/// The talk with Claude about a remark: what was said, then the field (Enter sends).
fn talk(ui: &mut egui::Ui, cache: &mut egui_commonmark::CommonMarkCache, k: usize, n: &mut AiNote, color: Color32, act: &mut Option<NoteAct>) {
    let weak = ui.visuals().weak_text_color();
    ui.add_space(6.0);
    ui.separator();
    for (dev, text) in &n.thread {
        if *dev {
            ui.label(RichText::new(format!("{} {text}", t!("Toi :", "You:"))).strong());
        } else {
            ui.horizontal_top(|ui| {
                ui.label(RichText::new("✳").color(color));
                ui.vertical(|ui| super::md::markdown(ui, cache, None, text));
            });
        }
    }
    if n.asking {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(t!("Claude réfléchit…", "Claude is thinking…")).small().color(weak));
        });
        return;
    }
    ui.horizontal(|ui| {
        let r = crate::input::add(ui, egui::TextEdit::singleline(&mut n.draft).desired_width((ui.available_width() - 80.0).max(120.0)).hint_text(t!("Plus court, en anglais, tu es sûr ? regarde…", "Shorter, in English, are you sure? look at…")));
        if std::mem::take(&mut n.focus) {
            r.request_focus();
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if (enter || ui.add_enabled(!n.draft.trim().is_empty(), egui::Button::new(t!("Envoyer", "Send"))).clicked()) && !n.draft.trim().is_empty() {
            *act = Some(NoteAct::Ask(k));
        }
    });
}

/// The code a remark proposes instead of its line, in green, with « Copier ».
fn suggestion(ui: &mut egui::Ui, code: &str) {
    let v = ui.visuals().clone();
    let base = v.extreme_bg_color;
    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(base)
        .stroke(egui::Stroke::new(1.0_f32, icons::mix(base, ok(), 0.45)))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("Changement suggéré", "Suggested change")).small().strong().color(ok()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    super::md::copy_button(ui, code, t!("Copier", "Copy"));
                });
            });
            ui.spacing_mut().item_spacing.y = 0.0;
            for l in code.split('\n') {
                let text = RichText::new(format!("+ {}", l.replace('\t', "    "))).monospace().size(12.5).color(ok()).background_color(icons::mix(base, ok(), 0.14));
                ui.add(egui::Label::new(text).extend());
            }
        });
    ui.add_space(4.0);
}

/// The state of the review above the diff, the buttons of the caller (given the number of kept
/// remarks), then the remarks that are not on a line of the diff.
pub(super) fn review_bar(
    ui: &mut egui::Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    r: &mut AiReview,
    rows: &[Option<usize>],
    accent: Color32,
    act: &mut Option<NoteAct>,
    actions: impl FnOnce(&mut egui::Ui, usize),
) {
    if !r.shown() {
        return;
    }
    let v = ui.visuals().clone();
    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(icons::mix(v.panel_fill, accent, 0.06))
        .stroke(egui::Stroke::new(1.0_f32, icons::mix(v.panel_fill, accent, 0.25)))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // title and state on the left, what to do on the right
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("✳ Revue IA", "✳ AI review")).strong().size(14.0).color(accent));
                if r.loading {
                    spinner(ui);
                    ui.label(RichText::new(t!("Claude relit les changements… (une à quelques minutes)", "Claude is reading over the changes… (one to a few minutes)")).weak());
                } else if r.error.is_none() && !r.notes.is_empty() {
                    let kept = r.notes.iter().filter(|n| n.keep).count();
                    for (key, label) in [("bug", "bug"), ("risk", t!("risque", "risk")), ("suggestion", "suggestion"), ("nit", t!("détail", "nit"))] {
                        let n = r.notes.iter().filter(|x| severity(&x.note.severity, accent).0 == severity(key, accent).0).count();
                        if n > 0 {
                            pill(ui, &format!("{n} {label}{}", if n > 1 { "s" } else { "" }), severity(key, accent).1);
                        }
                    }
                    pill(ui, &tf!("{kept} gardée(s)", "{kept} kept"), if kept > 0 { ok() } else { Color32::GRAY });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text(t!("Fermer et oublier cette revue", "Close and forget this review")).clicked() {
                        *act = Some(NoteAct::Close);
                    }
                    if !r.loading && r.error.is_none() && !r.notes.is_empty() {
                        let kept = r.notes.iter().filter(|n| n.keep).count();
                        actions(ui, kept);
                        let all = kept == r.notes.len();
                        if ui.small_button(if all { t!("Tout retirer", "Remove all") } else { t!("Tout garder", "Keep all") }).clicked() {
                            r.notes.iter_mut().for_each(|n| n.keep = !all);
                        }
                    }
                });
            });
            if let Some(e) = &r.error {
                ui.label(RichText::new(format!("⚠ {e}")).color(err()));
            } else if r.done && r.notes.is_empty() {
                ui.label(RichText::new(t!("✔ Rien à redire.", "✔ Nothing to say.")).color(ok()));
            } else if !r.loading {
                ui.label(RichText::new(t!("Rien n'est envoyé tout seul : garde les remarques utiles (✏ pour les retoucher), écarte les autres.", "Nothing is sent by itself: keep the useful remarks (✏ to touch them up), drop the others.")).small().weak());
            }
            for (k, n) in r.notes.iter_mut().enumerate() {
                if rows.get(k).copied().flatten().is_none() {
                    note_card(ui, cache, k, n, false, accent, act);
                }
            }
        });
    ui.add_space(4.0);
}

impl App {
    pub(super) fn ai_review_loaded(&mut self, key: String, r: Result<Vec<ReviewNote>, String>) {
        let rev = if self.pr.ai.key == key {
            &mut self.pr.ai
        } else if self.git.ai.key == key {
            &mut self.git.ai
        } else {
            return;
        };
        rev.loading = false;
        let status = match r {
            Ok(notes) => {
                rev.done = true;
                let n = notes.len();
                rev.notes = notes.into_iter().map(AiNote::new).collect();
                if n == 0 {
                    t!("Revue IA : rien à redire", "AI review: nothing to say").to_string()
                } else {
                    tf!("Revue IA : {n} remarque(s) proposée(s), à trier (rien n'est envoyé)", "AI review: {n} remark(s) suggested, to sort (nothing is sent)")
                }
            }
            Err(e) => {
                let first = e.lines().next().unwrap_or("").to_string();
                rev.error = Some(e);
                tf!("Revue IA : échec — {first}", "AI review: failed — {first}")
            }
        };
        self.set_status(status);
        self.pr_update_notes();
    }

    /// Claude reviews the changes not committed yet (indexed or not, new files too) of the
    /// repository of the Git view.
    pub(super) fn ai_review_wip(&mut self) {
        let Some(repo) = self.history.repo.clone().or_else(|| self.repo.clone()) else { return };
        let wish = self.claude.wish.clone().filter(|_| self.claude.dir.as_ref() == Some(&repo)).unwrap_or_default();
        self.git.ai.start(repo.clone(), "wip");
        self.git.ai.language = t!("français", "English").into();
        self.git.ai.about = if wish.trim().is_empty() { String::new() } else { tf!("Ce que le développeur voulait faire : {wish}", "What the developer meant to do: {wish}") };
        let key = self.git.ai.key.clone();
        self.set_status(t!("Revue IA des changements non commités…", "AI review of the uncommitted changes…"));
        self.jobs.spawn(move || {
            let r = match super::git_view::worktree_diff(&repo, &["diff", "HEAD"]) {
                Ok(d) if d.trim().is_empty() => Err(t!("Aucun changement à relire.", "No change to read over.").to_string()),
                Ok(d) => {
                    let about = if wish.trim().is_empty() { String::new() } else { tf!("Ce que le développeur voulait faire : {wish}", "What the developer meant to do: {wish}") };
                    claude::review(&repo, &d, &about, t!("français", "English"))
                }
                Err(e) => Err(e),
            };
            JobResult::AiReview(key, r)
        });
    }

    /// Claude reviews pull request `d` (its title and description say what it is about).
    pub(super) fn ai_review_pr(&mut self, repo: PathBuf, d: &github::PrDetail) {
        let (n, diff, about) = (d.number, d.diff.clone(), tf!("Pull request « {} »\n{}", "Pull request “{}”\n{}", d.title, d.body));
        self.pr.ai.start(repo.clone(), &format!("pr{n}"));
        self.pr.ai.about = about.clone();
        self.pr.ai.diff = Some(diff.clone());
        self.pr.ai.language = t!("celle du titre et de la description de la pull request (l'anglais s'ils sont en anglais)", "the one of the title and description of the pull request").into();
        let key = self.pr.ai.key.clone();
        self.set_status(tf!("Revue IA de la PR #{n}…", "AI review of PR #{n}…"));
        self.jobs.spawn(move || {
            let r = if diff.trim().is_empty() {
                Err(t!("La pull request ne change rien.", "The pull request changes nothing.").to_string())
            } else {
                claude::review(&repo, &diff, &about, t!("celle du titre et de la description de la pull request (l'anglais s'ils sont en anglais)", "the one of the title and description of the pull request"))
            };
            JobResult::AiReview(key, r)
        });
    }

    /// The kept remarks of the review of the changes not committed, written in the Claude tab
    /// (to read and send) for it to fix them.
    pub(super) fn ai_fix_with_claude(&mut self) {
        let Some(repo) = self.git.ai.repo.clone() else { return };
        let points: Vec<String> = self
            .git
            .ai
            .kept()
            .map(|n| {
                let place = if n.path.is_empty() { String::new() } else if n.line > 0 { format!("{}:{} — ", n.path, n.line) } else { format!("{} — ", n.path) };
                format!("- {place}{}", n.body.trim())
            })
            .collect();
        if points.is_empty() {
            return;
        }
        self.claude_prefill(repo, tf!("Corrige ces points relevés à la revue de mes changements non commités :\n{}", "Fix these points raised in the review of my uncommitted changes:\n{}", points.join("\n")));
    }

    /// A remark asked something in the view of the changes not committed.
    /// What is written in the talk of remark `k` (of the review of the pull request, else of the
    /// changes not committed) sent to Claude; its answer comes in `ai_refined`.
    pub(super) fn ai_ask(&mut self, pr: bool, k: usize) {
        let rev = if pr { &mut self.pr.ai } else { &mut self.git.ai };
        let Some(repo) = rev.repo.clone() else { return };
        let Some(n) = rev.notes.get_mut(k) else { return };
        let ask = std::mem::take(&mut n.draft).trim().to_string();
        if ask.is_empty() || n.asking {
            return;
        }
        n.thread.push((true, ask));
        n.asking = true;
        let (note, thread, id) = (n.note.clone(), n.thread.clone(), n.id);
        let (key, about, diff, language) = (rev.key.clone(), rev.about.clone(), rev.diff.clone(), rev.language.clone());
        self.jobs.spawn(move || {
            let diff = match diff {
                Some(d) => Ok(d),
                None => super::git_view::worktree_diff(&repo, &["diff", "HEAD"]),
            };
            let result = diff.and_then(|d| claude::refine_note(&repo, &note, &claude::diff_of_file(&d, &note.path), &about, &thread, &language));
            JobResult::AiRefined { key, id, result }
        });
    }

    /// Claude's answer about a remark: in its talk, and its new text in place of the old one.
    pub(super) fn ai_refined(&mut self, key: String, id: u64, result: Result<(String, Option<String>), String>) {
        let rev = if self.pr.ai.key == key { &mut self.pr.ai } else if self.git.ai.key == key { &mut self.git.ai } else { return };
        let Some(n) = rev.notes.iter_mut().find(|n| n.id == id) else { return };
        n.asking = false;
        match result {
            Ok((reply, body)) => {
                if !reply.is_empty() {
                    n.thread.push((false, reply));
                }
                if let Some(b) = body.filter(|b| *b != n.note.body) {
                    n.before = Some(std::mem::replace(&mut n.note.body, b));
                }
            }
            Err(e) => n.thread.push((false, format!("⚠ {}", e.lines().next().unwrap_or("")))),
        }
        self.pr_update_notes();
    }

    pub(super) fn ai_wip_act(&mut self, a: NoteAct) {
        if let NoteAct::Ask(k) = a {
            self.ai_ask(false, k);
            return;
        }
        let repo = self.git.ai.repo.clone();
        if let (Some((path, line)), Some(repo)) = (self.git.ai.apply(a), repo) {
            let Some(path) = crate::os::inside_repo(&repo, &path) else {
                self.set_error(tf!("Remarque refusée : « {path} » n'est pas un fichier du dépôt", "Remark refused: “{path}” is not a file of the repository"));
                return;
            };
            if self.open_path(path) {
                if let Some(d) = self.docs.get_mut(self.active) {
                    d.goto_line(line.saturating_sub(1), 0);
                }
                self.pending_focus = true;
            }
        }
        self.pr_update_notes();
    }

    /// Remarks of the review of the changes not committed, in the margin of the editor: file → (line, text).
    pub(super) fn ai_editor_notes(&self, notes: &mut HashMap<PathBuf, Vec<(usize, String)>>) {
        let Some(repo) = &self.git.ai.repo else { return };
        for n in self.git.ai.notes.iter().map(|n| &n.note).filter(|n| !n.path.is_empty() && n.line > 0 && n.side == "RIGHT") {
            let (label, _) = severity(&n.severity, Color32::GRAY);
            let body: String = n.body.lines().take(6).collect::<Vec<_>>().join("\n");
            notes.entry(repo.join(&n.path)).or_default().push((n.line - 1, tf!("✳ Revue IA · {label} :\n{body}", "✳ AI review · {label}:\n{body}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remarks_under_their_line() {
        let text = "diff --git a/x.py b/x.py\n--- a/x.py\n+++ b/x.py\n@@ -1,3 +1,3 @@\n a = 1\n-b = 2\n+b = 3\n c = 4".to_string();
        let lines = history::parse_diff(&text);
        let files = history::diff_files(&lines);
        let d = HistoryDiff { hash: String::new(), path: None, text, lines, files };
        assert_eq!(diff_row(&d, "x.py", 2, "RIGHT"), Some(6));
        assert_eq!(diff_row(&d, "x.py", 2, "LEFT"), Some(5));
        assert_eq!(diff_row(&d, "x.py", 9, "RIGHT"), None);
        assert_eq!(diff_row(&d, "y.py", 2, "RIGHT"), None);
        let mut r = AiReview::default();
        assert!(!r.shown());
        r.start(PathBuf::from("/r"), "wip");
        let first = r.key.clone();
        r.start(PathBuf::from("/r"), "wip");
        assert_ne!(first, r.key);
        let note = |line| AiNote { keep: line == 2, ..AiNote::new(ReviewNote { path: "x.py".into(), line, side: "RIGHT".into(), severity: "bug".into(), body: "?".into() }) };
        r.notes = vec![note(2), note(9)];
        assert_eq!(r.rows(Some(&d)), vec![Some(6), None]);
        assert_eq!(r.kept().count(), 1);
        assert_eq!(r.apply(NoteAct::Dismiss(0)), None);
        assert_eq!(r.notes.len(), 1);
        assert_eq!(r.apply(NoteAct::Open("x.py".into(), 9)), Some(("x.py".to_string(), 9)));
        r.apply(NoteAct::Close);
        assert!(!r.shown() && r.notes.is_empty());
    }

    /// Claude's answer about a remark: said in its talk, the remark rewritten, ↶ possible; an
    /// answer for a review gone is dropped.
    #[test]
    fn a_remark_refined() {
        let dir = std::env::temp_dir().join(format!("florm_ai_refine_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        app.pr.ai.start(proj.clone(), "pr1");
        app.pr.ai.notes = vec![AiNote::new(ReviewNote { path: "x.py".into(), line: 2, side: "RIGHT".into(), severity: "nit".into(), body: "Long texte.".into() })];
        let (key, id) = (app.pr.ai.key.clone(), app.pr.ai.notes[0].id);
        app.pr.ai.notes[0].asking = true;
        app.ai_refined(key.clone(), id, Ok(("Voilà, plus court.".into(), Some("Court.".into()))));
        let n = &app.pr.ai.notes[0];
        assert!(!n.asking);
        assert_eq!(n.note.body, "Court.");
        assert_eq!(n.before.as_deref(), Some("Long texte."));
        assert_eq!(n.thread, vec![(false, "Voilà, plus court.".to_string())]);
        // an answer that keeps the remark as it is
        app.ai_refined(key, id, Ok(("Elle est juste.".into(), None)));
        assert_eq!(app.pr.ai.notes[0].note.body, "Court.");
        // for an earlier review: dropped
        app.ai_refined("pr1#old".into(), id, Ok(("x".into(), Some("y".into()))));
        assert_eq!(app.pr.ai.notes[0].thread.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With the real Claude Code: `cargo test a_remark_talked_over_with_claude -- --ignored`.
    #[test]
    #[ignore]
    fn a_remark_talked_over_with_claude() {
        let dir = std::env::temp_dir();
        let diff = "diff --git a/calc.py b/calc.py\n--- a/calc.py\n+++ b/calc.py\n@@ -1,2 +1,2 @@\n def mean(xs):\n-    return sum(xs) / len(xs)\n+    return sum(xs) / max(len(xs), 1)\n";
        let note = ReviewNote { path: "calc.py".into(), line: 2, side: "RIGHT".into(), severity: "risk".into(), body: "Une liste vide donne 0 au lieu d'une erreur : est-ce voulu ? Ça peut cacher un bug en amont.".into() };
        let thread = vec![(true, "Réécris-la en anglais, en une phrase.".to_string())];
        let (reply, body) = claude::refine_note(&dir, &note, diff, "", &thread, "français").unwrap();
        eprintln!("reply: {reply}\nbody: {body:?}");
        let body = body.expect("rewritten");
        assert!(body.to_lowercase().contains("empty"), "{body}");
    }
}
