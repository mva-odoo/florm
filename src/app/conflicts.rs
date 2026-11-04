//! The conflicts assistant: a merge, rebase or cherry-pick stopped on conflicts. What is going
//! on (step, commit being applied, the two sides by their real names), each file in conflict
//! with its blocks side by side to choose from, or given to Claude; then continue, skip or abort.

use super::*;
use crate::conflict::{self, Block, Kind, OpInfo};
use crate::document::Resolution;
use crate::git::Operation;
use std::time::SystemTime;

/// Most lines of the commit's own changes given to Claude for a file.
const CLAUDE_DIFF_LINES: usize = 200;

/// Colors of the two sides, the ones of the conflicts in the editor.
fn first_color() -> Color32 {
    Color32::from_rgb(0x3f, 0xa3, 0x5b)
}
fn second_color() -> Color32 {
    Color32::from_rgb(0x3b, 0x82, 0xd6)
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_indentation_is_left_out() {
        let n = super::common_indent(["        a = 1\n            b\n", "        c\n", "\n"]);
        assert_eq!(n, 8);
        assert_eq!(super::dedent("        a = 1\n            b\n\n", n), "a = 1\n    b\n\n");
    }
}

#[derive(Default)]
pub(super) struct ConflictsUi {
    selected: Option<String>,
    /// The operation and what it is about, read again when `stale`.
    pub(super) info: Option<(Operation, OpInfo)>,
    pub(super) stale: bool,
    /// The operation the assistant was opened for by itself (repository, operation).
    pub(super) seen: Option<(PathBuf, Operation)>,
    /// The blocks of each file, with the time of the file they were read at.
    blocks: HashMap<String, (Option<SystemTime>, Vec<Block>)>,
}

enum ConfAct {
    Select(String),
    Resolve(String, Block, Resolution),
    /// The whole file from one side (true: the first one).
    Side(String, bool),
    Mark(String),
    MarkReady(Vec<String>),
    Remove(String),
    Open(String, usize),
    Claude(Vec<String>),
    Continue(Operation),
    Skip(Operation),
    Abort(Operation),
    Refresh,
}

impl App {
    /// The blocks of `rel` in the repository, read again when the file changed.
    pub(super) fn conflict_blocks(&mut self, repo: &Path, rel: &str) -> Vec<Block> {
        let path = repo.join(rel);
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let fresh = self.conflicts.blocks.get(rel).is_some_and(|(t, _)| *t == mtime);
        if !fresh {
            let b = std::fs::read_to_string(&path).map(|t| conflict::blocks(&t)).unwrap_or_default();
            self.conflicts.blocks.insert(rel.to_string(), (mtime, b));
        }
        self.conflicts.blocks[rel].1.clone()
    }

    pub(super) fn conflicts_ui(&mut self, ui: &mut egui::Ui) {
        let Some(repo) = self.repo.clone() else {
            ui.label(RichText::new(t!("Pas de dépôt git", "No git repository")).weak());
            return;
        };
        let Some(st) = self.git.status.clone() else { return };
        let accent = self.hl.colors.accent;
        let Some(op) = st.operation else {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(t!("✔ Aucune opération en cours : plus de conflit à résoudre.", "✔ No operation in progress: no conflict left to resolve.")).color(ok()).size(15.0));
                ui.add_space(8.0);
                if ui.button(t!("Fermer", "Close")).clicked() {
                    self.close_view(document::View::Conflicts);
                }
            });
            return;
        };
        if self.conflicts.stale || self.conflicts.info.as_ref().map(|(o, _)| *o) != Some(op) {
            self.conflicts.stale = false;
            self.conflicts.info = Some((op, conflict::info(&repo, op)));
        }
        let info = self.conflicts.info.as_ref().map(|(_, i)| i.clone()).unwrap_or_default();
        let (first_name, second_name) = conflict::sides(op, &info);
        let files: Vec<(String, Kind)> = st.entries.iter().filter(|e| e.conflicted()).map(|e| (e.path.clone(), conflict::kind(e.x, e.y))).collect();
        let counts: Vec<usize> = files.iter().map(|(p, _)| self.conflict_blocks(&repo, p).len()).collect();
        if self.conflicts.selected.as_ref().is_none_or(|s| !files.iter().any(|(p, _)| p == s)) {
            self.conflicts.selected = files.iter().zip(&counts).find(|(_, n)| **n > 0).or(files.iter().zip(&counts).next()).map(|((p, _), _)| p.clone());
        }
        let selected = self.conflicts.selected.clone();
        let blocks = selected.as_deref().map(|p| self.conflict_blocks(&repo, p)).unwrap_or_default();
        let ready: Vec<String> = files.iter().zip(&counts).filter(|((_, k), n)| **n == 0 && *k == Kind::Both).map(|((p, _), _)| p.clone()).collect();
        let with_blocks: Vec<String> = files.iter().zip(&counts).filter(|(_, n)| **n > 0).map(|((p, _), _)| p.clone()).collect();
        let claude_on = crate::features::on(crate::features::Feature::Claude);
        let mut act: Option<ConfAct> = None;

        // ------------------------------------------------ what is going on, and the way out
        egui::TopBottomPanel::top("conflicts_head").frame(egui::Frame::none().inner_margin(egui::Margin { left: 12.0, right: 12.0, top: 10.0, bottom: 8.0 })).show_inside(ui, |ui| {
            let what = match op {
                Operation::Rebase => tf!("Rebase de {} sur {}", "Rebase of {} onto {}", info.branch, info.onto),
                Operation::Merge => tf!("Fusion de {} dans {}", "Merge of {} into {}", info.onto, info.branch),
                Operation::CherryPick => tf!("Cherry-pick sur {}", "Cherry-pick onto {}", info.branch),
                Operation::Revert => tf!("Revert sur {}", "Revert on {}", info.branch),
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("⇄ {what}")).strong().size(16.0));
                if let Some((k, n)) = info.step {
                    ui.label(RichText::new(tf!("— commit {k} sur {n}", "— commit {k} of {n}")).weak().size(15.0));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t!("Abandonner", "Abort")).on_hover_text(t!("Remet la branche comme avant l'opération", "Puts the branch back as before the operation")).clicked() {
                        act = Some(ConfAct::Abort(op));
                    }
                    if matches!(op, Operation::Rebase | Operation::CherryPick | Operation::Revert)
                        && ui.button(t!("Passer ce commit", "Skip this commit")).on_hover_text(t!("Laisse tomber ce commit et passe au suivant", "Leaves this commit out and goes to the next one")).clicked()
                    {
                        act = Some(ConfAct::Skip(op));
                    }
                    let done = files.is_empty();
                    let label = if op == Operation::Rebase { t!("Continuer le rebase ▶", "Continue the rebase ▶") } else { t!("Continuer ▶", "Continue ▶") };
                    if ui
                        .add_enabled(done, filled_button(label, accent))
                        .on_hover_text(t!("Tous les fichiers sont résolus : git continue (commit suivant, ou fin)", "Every file is resolved: git goes on (next commit, or the end)"))
                        .on_disabled_hover_text(t!("Résous et marque d'abord chaque fichier", "First resolve and mark every file"))
                        .clicked()
                    {
                        act = Some(ConfAct::Continue(op));
                    }
                    if claude_on && !with_blocks.is_empty() && ui.button(tf!("✳ Résoudre avec Claude ({})", "✳ Resolve with Claude ({})", with_blocks.len())).on_hover_text(t!("Une conversation Claude reçoit les fichiers en conflit, ce que fait le commit et les deux côtés, et retire les marqueurs ; tu relis dans sa revue puis tu marques résolu", "A Claude conversation gets the conflicting files, what the commit does and both sides, and removes the markers; you check in its review then mark resolved")).clicked() {
                        act = Some(ConfAct::Claude(with_blocks.clone()));
                    }
                    if ui.small_button("⟳").on_hover_text(t!("Relire", "Read again")).clicked() {
                        act = Some(ConfAct::Refresh);
                    }
                });
            });
            if let Some((h, s)) = &info.commit {
                let doing = if op == Operation::Merge { t!("Fusionne", "Merging") } else { t!("Applique", "Applying") };
                ui.label(RichText::new(format!("{doing} {h} « {s} »")).monospace().size(12.5));
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("■").color(first_color()));
                ui.label(RichText::new(tf!("Côté 1 : {first_name}", "Side 1: {first_name}")).small());
                ui.add_space(14.0);
                ui.label(RichText::new("■").color(second_color()));
                ui.label(RichText::new(tf!("Côté 2 : {second_name}", "Side 2: {second_name}")).small());
            });
            if op == Operation::Rebase {
                ui.label(RichText::new(t!("Dans un rebase, le côté 1 est la base sur laquelle tes commits sont rejoués (ce que git appelle « ours »), le côté 2 ton commit.", "In a rebase, side 1 is the base your commits are replayed onto (what git calls “ours”), side 2 your commit.")).small().weak());
            }
        });

        // ------------------------------------------------ the files
        egui::SidePanel::left("conflicts_files").resizable(true).default_width(280.0).width_range(180.0..=520.0).show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(tf!("FICHIERS EN CONFLIT ({})", "CONFLICTING FILES ({})", files.len())).small().strong().weak());
            ui.add_space(4.0);
            if files.is_empty() {
                ui.label(RichText::new(t!("✔ Tout est résolu : « Continuer ».", "✔ Everything is resolved: “Continue”.")).color(ok()));
            }
            egui::ScrollArea::vertical().id_salt("conflicts_files_list").show(ui, |ui| {
                for ((path, kind), n) in files.iter().zip(&counts) {
                    let state = match kind {
                        Kind::Both if *n > 0 => RichText::new(tf!("⚠ {n} bloc(s)", "⚠ {n} block(s)")).color(warn()),
                        Kind::Both => RichText::new(t!("✔ prêt à marquer", "✔ ready to mark")).color(ok()),
                        Kind::DeletedFirst => RichText::new(t!("supprimé côté 1", "removed on side 1")).color(warn()),
                        Kind::DeletedSecond => RichText::new(t!("supprimé côté 2", "removed on side 2")).color(warn()),
                        Kind::Other => RichText::new(t!("à choisir", "to choose")).color(warn()),
                    };
                    let shown = selected.as_deref() == Some(path.as_str());
                    let r = ui.add(egui::SelectableLabel::new(shown, RichText::new(path.as_str()).monospace().size(12.5)));
                    if r.clicked() {
                        act = Some(ConfAct::Select(path.clone()));
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(state.small());
                    });
                    ui.add_space(2.0);
                }
            });
            if ready.len() > 1 {
                ui.add_space(6.0);
                if ui.button(tf!("✔ Marquer les {} prêts comme résolus", "✔ Mark the {} ready ones as resolved", ready.len())).clicked() {
                    act = Some(ConfAct::MarkReady(ready.clone()));
                }
            }
        });

        // ------------------------------------------------ the blocks of the file chosen
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let Some(path) = selected.clone() else { return };
            let kind = files.iter().find(|(p, _)| *p == path).map_or(Kind::Both, |(_, k)| *k);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&path).monospace().strong().color(accent));
                if ui.small_button(t!("Ouvrir dans l'éditeur", "Open in the editor")).clicked() {
                    act = Some(ConfAct::Open(path.clone(), blocks.first().map_or(0, |b| b.at.start)));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match kind {
                    Kind::Both => {
                        let r = ui.add_enabled(blocks.is_empty(), filled_button(t!("✔ Marquer résolu", "✔ Mark resolved"), accent));
                        if r.on_hover_text(t!("git add : le fichier est résolu", "git add: the file is resolved")).on_disabled_hover_text(t!("Il reste des blocs à résoudre", "There are blocks left to resolve")).clicked() {
                            act = Some(ConfAct::Mark(path.clone()));
                        }
                        if ui.button(t!("Tout du côté 2", "All from side 2")).on_hover_text(second_name.as_str()).clicked() {
                            act = Some(ConfAct::Side(path.clone(), false));
                        }
                        if ui.button(t!("Tout du côté 1", "All from side 1")).on_hover_text(first_name.as_str()).clicked() {
                            act = Some(ConfAct::Side(path.clone(), true));
                        }
                    }
                    _ => {
                        if ui.button(t!("Supprimer le fichier", "Remove the file")).on_hover_text("git rm").clicked() {
                            act = Some(ConfAct::Remove(path.clone()));
                        }
                        if ui.add(filled_button(t!("Garder le fichier", "Keep the file"), accent)).on_hover_text(t!("git add : le fichier reste, tel qu'il est sur le disque", "git add: the file stays, as it is on the disk")).clicked() {
                            act = Some(ConfAct::Mark(path.clone()));
                        }
                    }
                });
            });
            ui.separator();
            match kind {
                Kind::DeletedFirst | Kind::DeletedSecond => {
                    let (gone, kept) = if kind == Kind::DeletedFirst { (&first_name, &second_name) } else { (&second_name, &first_name) };
                    ui.label(tf!("Ce fichier a été supprimé par {gone}, mais changé par {kept}. Garde-le (avec ces changements) ou supprime-le.", "This file was removed by {gone}, but changed by {kept}. Keep it (with those changes) or remove it."));
                    return;
                }
                Kind::Other => {
                    ui.label(t!("Ce fichier a été ajouté ou supprimé d'un seul côté : garde-le ou supprime-le.", "This file was added or removed on one side only: keep it or remove it."));
                    return;
                }
                Kind::Both => {}
            }
            if blocks.is_empty() {
                ui.add_space(10.0);
                ui.label(RichText::new(t!("✔ Plus aucun marqueur de conflit dans ce fichier : relis-le si besoin, puis « Marquer résolu ».", "✔ No conflict marker left in this file: check it if needed, then “Mark resolved”.")).color(ok()));
                return;
            }
            egui::ScrollArea::vertical().id_salt(("conflict_blocks", &path)).auto_shrink([false, false]).show(ui, |ui| {
                for (k, b) in blocks.iter().enumerate() {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tf!("Bloc {} — ligne {}", "Block {} — line {}", k + 1, b.at.start + 1)).strong());
                        if ui.small_button(t!("Éditer à la main…", "Edit by hand…")).on_hover_text(t!("Ouvre le fichier sur ce bloc", "Opens the file on this block")).clicked() {
                            act = Some(ConfAct::Open(path.clone(), b.at.start));
                        }
                    });
                    let w = (ui.available_width() - 12.0) / 2.0;
                    // the indentation the sides share is left out of the display (not of the file)
                    let cut = common_indent([b.first.as_str(), b.second.as_str(), b.base.as_deref().unwrap_or("")]);
                    let (first_text, second_text) = (dedent(&b.first, cut), dedent(&b.second, cut));
                    ui.horizontal_top(|ui| {
                        for (first, text, color, name) in [(true, &first_text, first_color(), &first_name), (false, &second_text, second_color(), &second_name)] {
                            ui.vertical(|ui| {
                                ui.set_width(w);
                                // the button by the name of the side: the two stay level whatever their length
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(if first { t!("Côté 1", "Side 1") } else { t!("Côté 2", "Side 2") }).small().strong().color(color)).on_hover_text(name.as_str());
                                    let label = if first { t!("Garder le côté 1", "Keep side 1") } else { t!("Garder le côté 2", "Keep side 2") };
                                    if ui.small_button(label).clicked() {
                                        act = Some(ConfAct::Resolve(path.clone(), b.clone(), if first { Resolution::Ours } else { Resolution::Theirs }));
                                    }
                                });
                                side_box(ui, text, color, w);
                            });
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button(t!("Garder les deux (1 puis 2)", "Keep both (1 then 2)")).clicked() {
                            act = Some(ConfAct::Resolve(path.clone(), b.clone(), Resolution::Both));
                        }
                        if let Some(base) = &b.base {
                            egui::CollapsingHeader::new(RichText::new(t!("Version d'origine (avant les deux)", "Original version (before both)")).small().weak()).id_salt(("conflict_base", &path, k)).show(ui, |ui| {
                                side_box(ui, &dedent(base, cut), ui.visuals().weak_text_color(), ui.available_width());
                            });
                        }
                    });
                    ui.separator();
                }
            });
        });

        if let Some(a) = act {
            self.conflicts_act(repo, op, &info, a);
        }
    }

    fn conflicts_act(&mut self, repo: PathBuf, op: Operation, info: &OpInfo, act: ConfAct) {
        match act {
            ConfAct::Select(p) => self.conflicts.selected = Some(p),
            ConfAct::Refresh => {
                self.conflicts.stale = true;
                self.conflicts.blocks.clear();
                self.git_refresh();
            }
            ConfAct::Resolve(rel, b, how) => self.conflict_resolve(&repo, &rel, &b, how),
            ConfAct::Side(rel, first) => {
                let side = if first { "--ours" } else { "--theirs" };
                match git::run(&repo, &["checkout", side, "--", &rel]) {
                    Ok(_) => {
                        self.reload_open(&repo.join(&rel));
                        self.git_job_at(repo, "add", &["add", "--", &rel]);
                    }
                    Err(e) => self.set_error(tf!("git checkout {side} : {e}", "git checkout {side}: {e}")),
                }
            }
            ConfAct::Mark(rel) => self.git_job_at(repo, "add", &["add", "--", &rel]),
            ConfAct::MarkReady(list) => {
                let mut a = vec!["add", "--"];
                a.extend(list.iter().map(String::as_str));
                self.git_job_at(repo, "add", &a);
            }
            ConfAct::Remove(rel) => self.git_job_at(repo, "rm", &["rm", "--", &rel]),
            ConfAct::Open(rel, line) => {
                self.open_at(repo.join(&rel), line);
            }
            ConfAct::Claude(list) => self.conflicts_to_claude(&repo, op, info, &list),
            ConfAct::Continue(op) => {
                let mut a = vec!["-c", "core.editor=true"];
                a.extend(op.continue_args());
                self.git_job_at(repo, &op.continue_args().join(" "), &a);
            }
            ConfAct::Skip(op) => {
                let verb = if op == Operation::Rebase { "rebase" } else if op == Operation::Revert { "revert" } else { "cherry-pick" };
                self.git_job_at(repo, &format!("{verb} --skip"), &[verb, "--skip"]);
            }
            ConfAct::Abort(op) => self.git_job_at(repo, &op.abort_args().join(" "), &op.abort_args()),
        }
    }

    /// Block `b` of `rel` replaced by the side(s) chosen: in the editor when the file is open
    /// there with changes not saved (undoable), else on the disk.
    pub(super) fn conflict_resolve(&mut self, repo: &Path, rel: &str, b: &Block, how: Resolution) {
        let path = repo.join(rel);
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref() == Some(path.as_path()) && d.is_dirty()) {
            let c = self.docs[i].conflicts().into_iter().find(|c| c.start == b.at.start);
            if let Some(c) = c {
                self.docs[i].resolve_conflict(&c, how);
                self.save(i);
            }
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                // the block is still the one shown
                if !conflict::blocks(&text).iter().any(|x| x.at == b.at) {
                    self.set_error(t!("Le fichier a changé : relis-le", "The file changed: read it again"));
                    return;
                }
                if let Err(e) = std::fs::write(&path, conflict::resolve(&text, &b.at, how)) {
                    self.set_error(e.to_string());
                    return;
                }
                self.reload_open(&path);
            }
            Err(e) => self.set_error(e.to_string()),
        }
    }

    /// An open tab of `path` (without changes) shows the file as it is now on the disk.
    fn reload_open(&mut self, path: &Path) {
        for d in self.docs.iter_mut().filter(|d| d.path.as_deref() == Some(path) && !d.is_dirty()) {
            let _ = d.reload_from_disk();
        }
    }

    /// « Résoudre avec Claude »: a new conversation in the repository gets the files with
    /// blocks, what the operation does and both sides.
    fn conflicts_to_claude(&mut self, repo: &Path, op: Operation, info: &OpInfo, files: &[String]) {
        let (first, second) = conflict::sides(op, info);
        let mut text = tf!(
            "Résous les conflits de git dans ces fichiers ({} en cours). Côté 1 (entre <<<<<<< et |||||||/=======) : {first}. Côté 2 (entre ======= et >>>>>>>) : {second}. Le bloc après ||||||| est la version d'origine, d'avant les deux.\n\nPour chaque bloc, garde l'intention des deux côtés (souvent il faut les combiner), retire tous les marqueurs, et vérifie que le fichier reste correct (syntaxe, imports, noms). Ne lance ni git add, ni git rebase --continue, ni aucune commande git qui change l'état : je relis puis je marque résolu. À la fin, dis en une ligne par bloc ce que tu as choisi.",
            "Resolve the git conflicts in these files ({} in progress). Side 1 (between <<<<<<< and |||||||/=======): {first}. Side 2 (between ======= and >>>>>>>): {second}. The block after ||||||| is the original version, from before both.\n\nFor each block, keep the intent of both sides (they often have to be combined), remove every marker, and check the file stays correct (syntax, imports, names). Do not run git add, nor git rebase --continue, nor any git command that changes the state: I check then mark resolved. At the end, say in one line per block what you chose.",
            op.label()
        );
        if let Some((h, s)) = &info.commit {
            text += &tf!("\n\nLe commit appliqué : {h} « {s} ».", "\n\nThe commit being applied: {h} “{s}”.");
        }
        let rev = match op {
            Operation::Rebase => Some("REBASE_HEAD"),
            Operation::CherryPick => Some("CHERRY_PICK_HEAD"),
            _ => None,
        };
        for f in files {
            text += &format!("\n\n- `{f}`");
            // what the commit itself changes in that file: the change to keep
            if let Some(rev) = rev {
                let diff = git::stdout(repo, &["show", "--format=", rev, "--", f]).unwrap_or_default();
                if !diff.trim().is_empty() {
                    let lines: Vec<&str> = diff.lines().collect();
                    let cut = if lines.len() > CLAUDE_DIFF_LINES { t!("\n… (coupé)", "\n… (cut)") } else { "" };
                    text += &format!("\n  {}\n```diff\n{}{cut}\n```", t!("Ce que le commit y change :", "What the commit changes in it:"), lines[..lines.len().min(CLAUDE_DIFF_LINES)].join("\n"));
                }
            }
        }
        let empty = self.claude.session.is_none() && self.claude.items.iter().all(|i| matches!(i, claude_ui::Item::Info(_)));
        if !(empty && self.claude.dir.as_deref() == Some(repo)) {
            self.claude_new_in(repo.to_path_buf());
        }
        self.open_claude();
        if self.claude_send_text(text, false) {
            self.set_status(tf!("{} fichier(s) donné(s) à Claude : relis puis « Marquer résolu »", "{} file(s) given to Claude: check then “Mark resolved”", files.len()));
        }
    }
}

/// The spaces (or tabs) all the lines of these texts start with, the empty lines aside.
fn common_indent<'a>(texts: impl IntoIterator<Item = &'a str>) -> usize {
    texts.into_iter().flat_map(str::lines).filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start_matches([' ', '\t']).len()).min().unwrap_or(0)
}

/// `text` without the first `n` characters of indentation of each line.
fn dedent(text: &str, n: usize) -> String {
    text.split_inclusive('\n').map(|l| if l.trim().is_empty() { l.trim_start_matches([' ', '\t']) } else { &l[n.min(l.len() - l.trim_start_matches([' ', '\t']).len())..] }).collect()
}

/// A side of a block: its lines in a frame of its color.
fn side_box(ui: &mut egui::Ui, text: &str, color: Color32, width: f32) {
    egui::Frame::none().fill(color.gamma_multiply(0.10)).stroke(egui::Stroke::new(1.0_f32, color.gamma_multiply(0.6))).rounding(4.0).inner_margin(egui::Margin::symmetric(8.0, 6.0)).show(ui, |ui| {
        ui.set_width(width - 18.0);
        let shown = if text.is_empty() { t!("(rien : ce côté retire ces lignes)", "(nothing: this side removes these lines)").to_string() } else { text.trim_end_matches('\n').to_string() };
        // code as it is: no wrapping (a line cut in two misleads), long lines scroll sideways
        egui::ScrollArea::both().id_salt(ui.next_auto_id()).max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(shown).monospace().size(12.0)).extend());
        });
    });
}
