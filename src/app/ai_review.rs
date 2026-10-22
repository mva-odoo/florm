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
}

/// What a remark asked for during the frame.
pub(super) enum NoteAct {
    /// Open the file at this line.
    Open(String, usize),
    Dismiss(usize),
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
            NoteAct::Dismiss(_) => {}
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
        "risk" => ("Risque", warn()),
        "nit" => ("Détail", Color32::GRAY),
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
    let frame = egui::Frame::none()
        .fill(icons::mix(v.panel_fill, color, if n.keep { 0.10 } else { 0.035 }))
        .stroke(egui::Stroke::new(1.0_f32, icons::mix(v.panel_fill, color, if n.keep { 0.7 } else { 0.25 })))
        .rounding(8.0)
        .inner_margin(egui::Margin { left: 16.0, right: 10.0, top: 8.0, bottom: 10.0 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("✳").color(color));
                pill(ui, label, color);
                if !n.note.path.is_empty() {
                    let place = match (in_diff, n.note.line) {
                        (true, l) => format!("ligne {l}"),
                        (false, 0) => n.note.path.clone(),
                        (false, l) => format!("{}:{l}", n.note.path),
                    };
                    if ui.link(RichText::new(place).monospace().small()).on_hover_text("Ouvrir le fichier à cette ligne").clicked() {
                        *act = Some(NoteAct::Open(n.note.path.clone(), n.note.line.max(1)));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text("Écarter cette remarque").clicked() {
                        *act = Some(NoteAct::Dismiss(k));
                    }
                    let (edit, tip) = if n.editing { ("✔ Fini", "Revenir au rendu") } else { ("✏", "Modifier le texte avant de le garder") };
                    if ui.small_button(edit).on_hover_text(tip).clicked() {
                        n.editing = !n.editing;
                    }
                    let keep = if n.keep {
                        egui::Button::new(RichText::new("✔ Gardée").small().strong().color(text_on(color))).fill(color)
                    } else {
                        egui::Button::new(RichText::new("Garder").small().color(color)).fill(Color32::TRANSPARENT).stroke(egui::Stroke::new(1.0_f32, color))
                    };
                    if ui.add(keep.rounding(10.0)).on_hover_text(if n.keep { "Ne plus la garder" } else { "La garder" }).clicked() {
                        n.keep = !n.keep;
                    }
                });
            });
            ui.add_space(4.0);
            if n.editing {
                ui.add(egui::TextEdit::multiline(&mut n.note.body).desired_rows(3).desired_width(f32::INFINITY).hint_text("Markdown ; ```suggestion pour proposer du code"));
                return;
            }
            for part in github::parts(&n.note.body) {
                match part {
                    github::Part::Text(t) => super::md::markdown(ui, cache, &t),
                    github::Part::Suggestion(code) => suggestion(ui, &code),
                }
            }
        });
    // the edge, of the color of its kind
    let r = frame.response.rect;
    let edge = egui::Rect::from_min_size(r.min, egui::vec2(4.0, r.height()));
    ui.painter().rect_filled(edge, egui::Rounding { nw: 8.0, sw: 8.0, ne: 0.0, se: 0.0 }, color);
    ui.add_space(5.0);
}

/// The code a remark proposes instead of its line, in green, with « Copier ».
fn suggestion(ui: &mut egui::Ui, code: &str) {
    let v = ui.visuals().clone();
    let base = v.extreme_bg_color;
    ui.add_space(4.0);
    egui::Frame::none()
        .fill(base)
        .stroke(egui::Stroke::new(1.0_f32, icons::mix(base, ok(), 0.45)))
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Changement suggéré").small().strong().color(ok()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    super::md::copy_button(ui, code, "Copier");
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
    egui::Frame::none()
        .fill(icons::mix(v.panel_fill, accent, 0.06))
        .stroke(egui::Stroke::new(1.0_f32, icons::mix(v.panel_fill, accent, 0.25)))
        .rounding(8.0)
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // title and state on the left, what to do on the right
            ui.horizontal(|ui| {
                ui.label(RichText::new("✳ Revue IA").strong().size(14.0).color(accent));
                if r.loading {
                    ui.spinner();
                    ui.label(RichText::new("Claude relit les changements… (une à quelques minutes)").weak());
                } else if r.error.is_none() && !r.notes.is_empty() {
                    let kept = r.notes.iter().filter(|n| n.keep).count();
                    for (key, label) in [("bug", "bug"), ("risk", "risque"), ("suggestion", "suggestion"), ("nit", "détail")] {
                        let n = r.notes.iter().filter(|x| severity(&x.note.severity, accent).0 == severity(key, accent).0).count();
                        if n > 0 {
                            pill(ui, &format!("{n} {label}{}", if n > 1 { "s" } else { "" }), severity(key, accent).1);
                        }
                    }
                    pill(ui, &format!("{kept} gardée(s)"), if kept > 0 { ok() } else { Color32::GRAY });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text("Fermer et oublier cette revue").clicked() {
                        *act = Some(NoteAct::Close);
                    }
                    if !r.loading && r.error.is_none() && !r.notes.is_empty() {
                        let kept = r.notes.iter().filter(|n| n.keep).count();
                        actions(ui, kept);
                        let all = kept == r.notes.len();
                        if ui.small_button(if all { "Tout retirer" } else { "Tout garder" }).clicked() {
                            r.notes.iter_mut().for_each(|n| n.keep = !all);
                        }
                    }
                });
            });
            if let Some(e) = &r.error {
                ui.label(RichText::new(format!("⚠ {e}")).color(err()));
            } else if r.done && r.notes.is_empty() {
                ui.label(RichText::new("✔ Rien à redire.").color(ok()));
            } else if !r.loading {
                ui.label(RichText::new("Rien n'est envoyé tout seul : garde les remarques utiles (✏ pour les retoucher), écarte les autres.").small().weak());
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
                rev.notes = notes.into_iter().map(|note| AiNote { note, keep: false, editing: false }).collect();
                if n == 0 {
                    "Revue IA : rien à redire".to_string()
                } else {
                    format!("Revue IA : {n} remarque(s) proposée(s), à trier (rien n'est envoyé)")
                }
            }
            Err(e) => {
                let first = e.lines().next().unwrap_or("").to_string();
                rev.error = Some(e);
                format!("Revue IA : échec — {first}")
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
        let key = self.git.ai.key.clone();
        self.set_status("Revue IA des changements non commités…");
        self.jobs.spawn(move || {
            let r = match super::git_view::worktree_diff(&repo, &["diff", "HEAD"]) {
                Ok(d) if d.trim().is_empty() => Err("Aucun changement à relire.".to_string()),
                Ok(d) => {
                    let about = if wish.trim().is_empty() { String::new() } else { format!("Ce que le développeur voulait faire : {wish}") };
                    claude::review(&repo, &d, &about, "français")
                }
                Err(e) => Err(e),
            };
            JobResult::AiReview(key, r)
        });
    }

    /// Claude reviews pull request `d` (its title and description say what it is about).
    pub(super) fn ai_review_pr(&mut self, repo: PathBuf, d: &github::PrDetail) {
        let (n, diff, about) = (d.number, d.diff.clone(), format!("Pull request « {} »\n{}", d.title, d.body));
        self.pr.ai.start(repo.clone(), &format!("pr{n}"));
        let key = self.pr.ai.key.clone();
        self.set_status(format!("Revue IA de la PR #{n}…"));
        self.jobs.spawn(move || {
            let r = if diff.trim().is_empty() {
                Err("La pull request ne change rien.".to_string())
            } else {
                claude::review(&repo, &diff, &about, "celle du titre et de la description de la pull request (l'anglais s'ils sont en anglais)")
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
        self.claude_prefill(repo, format!("Corrige ces points relevés à la revue de mes changements non commités :\n{}", points.join("\n")));
    }

    /// A remark asked something in the view of the changes not committed.
    pub(super) fn ai_wip_act(&mut self, a: NoteAct) {
        let repo = self.git.ai.repo.clone();
        if let (Some((path, line)), Some(repo)) = (self.git.ai.apply(a), repo) {
            let path = repo.join(path);
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
            notes.entry(repo.join(&n.path)).or_default().push((n.line - 1, format!("✳ Revue IA · {label} :\n{body}")));
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
        let note = |line| AiNote { note: ReviewNote { path: "x.py".into(), line, side: "RIGHT".into(), severity: "bug".into(), body: "?".into() }, keep: line == 2, editing: false };
        r.notes = vec![note(2), note(9)];
        assert_eq!(r.rows(Some(&d)), vec![Some(6), None]);
        assert_eq!(r.kept().count(), 1);
        assert_eq!(r.apply(NoteAct::Dismiss(0)), None);
        assert_eq!(r.notes.len(), 1);
        assert_eq!(r.apply(NoteAct::Open("x.py".into(), 9)), Some(("x.py".to_string(), 9)));
        r.apply(NoteAct::Close);
        assert!(!r.shown() && r.notes.is_empty());
    }
}
