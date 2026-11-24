//! Conflicts, as in VS Code: a merge, rebase or cherry-pick stopped on conflicts opens the first
//! file in conflict. In the editor, links after the `<<<<<<<` of each block keep a side, both,
//! or compare them; a bar above the file says what is going on (step, the real name of the green
//! and the blue side), goes from block to block, marks the file resolved and goes on to the next
//! one or continues the operation. The files where a side removed the file are kept or removed
//! from the Git view.

use super::*;
use crate::conflict::{self, Kind, OpInfo};
use crate::document::Resolution;
use crate::git::Operation;

/// Most lines of the commit's own changes given to Claude for a file.
const CLAUDE_DIFF_LINES: usize = 200;

#[derive(Default)]
pub(super) struct ConflictsUi {
    /// The operation and what it is about (repository, operation, info), read again when `stale`.
    pub(super) info: Option<(PathBuf, Operation, OpInfo)>,
    pub(super) stale: bool,
    /// The conflicting files already shown by themselves (repository, commit being applied,
    /// files): a new stop (the next commit of a rebase) opens its first one, marking one
    /// resolved does not.
    pub(super) seen: Option<(PathBuf, String, Vec<String>)>,
    /// A new stop found: its first file opens once the status is in place.
    pub(super) open_pending: Option<PathBuf>,
}

impl App {
    /// The operation in progress in `repo` and what it is about.
    pub(super) fn conflict_op(&mut self, repo: &Path) -> Option<(Operation, OpInfo)> {
        let st = self.git.status.as_ref().filter(|_| self.repo.as_deref() == Some(repo))?;
        let op = st.operation?;
        let fresh = self.conflicts.info.as_ref().is_some_and(|(r, o, _)| r == repo && *o == op);
        if self.conflicts.stale || !fresh {
            self.conflicts.stale = false;
            self.conflicts.info = Some((repo.to_path_buf(), op, conflict::info(repo, op)));
        }
        self.conflicts.info.as_ref().map(|(_, o, i)| (*o, i.clone()))
    }

    /// The files in conflict of the repository shown (relative paths), with what they are about.
    pub(super) fn conflict_files(&self) -> Vec<(String, Kind)> {
        self.git.status.as_ref().map(|st| st.entries.iter().filter(|e| e.conflicted()).map(|e| (e.path.clone(), conflict::kind(e.x, e.y))).collect()).unwrap_or_default()
    }

    /// Opens the first file in conflict with blocks (but `skip`) on its first block; none (only
    /// files removed on a side): the Git view, where they are kept or removed. False when
    /// there is nothing left.
    pub(super) fn open_next_conflict(&mut self, repo: &Path, skip: Option<&str>) -> bool {
        let files = self.conflict_files();
        let mut rest = files.iter().filter(|(p, _)| Some(p.as_str()) != skip);
        let Some((first, _)) = rest.clone().find(|(_, k)| *k == Kind::Both).or_else(|| rest.next()) else { return false };
        let path = repo.join(first);
        let line = std::fs::read_to_string(&path).ok().and_then(|t| conflict::blocks(&t).first().map(|b| b.at.start));
        match line {
            Some(l) => self.open_at(path, l),
            None => self.show_tab(document::View::Git),
        }
        true
    }

    /// git found stopped on conflicts (from Florm or a terminal): the first file opens, once
    /// for each stop.
    pub(super) fn conflicts_found(&mut self, repo: &Path, st: &git::Status) {
        let files: Vec<String> = st.entries.iter().filter(|e| e.conflicted()).map(|e| e.path.clone()).collect();
        if st.operation.is_none() || files.is_empty() {
            self.conflicts.seen = None;
            return;
        }
        let head = applied_head(repo);
        let new = self.conflicts.seen.as_ref().is_none_or(|(r, h, seen)| r != repo || *h != head || files.iter().any(|f| !seen.contains(f)));
        self.conflicts.seen = Some((repo.to_path_buf(), head, files));
        if new {
            self.conflicts.stale = true;
            // after the status is in place: the files are read from it
            self.conflicts.open_pending = Some(repo.to_path_buf());
        }
    }

    /// Bar above a file with conflict markers: what is going on and the names of the sides, the
    /// block under the cursor and its choices, then « mark resolved » and the way on.
    pub(super) fn conflict_bar(&mut self, ui: &mut egui::Ui, i: usize, pane: u8) {
        let Some(d) = self.docs.get_mut(i) else { return };
        let conflicts = d.conflicts();
        if conflicts.is_empty() && !d.had_conflicts {
            return;
        }
        let line = d.rope.char_to_line(d.primary_sel().head);
        let k = conflicts.iter().position(|c| line <= c.end).unwrap_or(0);
        let path = d.path.clone();
        let repo = path.as_deref().and_then(|p| self.repo_for(p));
        let rel = path.as_deref().zip(repo.as_deref()).map(|(p, r)| p.strip_prefix(r).unwrap_or(p).to_string_lossy().replace('\\', "/"));
        let op = repo.as_deref().and_then(|r| self.conflict_op(r));
        let others = rel.as_deref().map_or(0, |rel| self.conflict_files().iter().filter(|(p, _)| p != rel).count());
        let claude_on = crate::features::on(crate::features::Feature::Claude);
        let (green, blue) = match &op {
            Some((o, info)) => conflict::sides(*o, info),
            None => conflicts.get(k).map(|c| (c.ours.clone(), c.theirs.clone())).unwrap_or_default(),
        };
        enum Act {
            Resolve(document::Conflict, Resolution),
            Goto(usize),
            /// Save, git add, then: open the next file (true) or continue the operation.
            Mark(Option<bool>),
            Claude,
            Abort,
        }
        let mut act = None;
        let bg = icons::mix(ui.visuals().panel_fill, if conflicts.is_empty() { ok() } else { warn() }, 0.15);
        egui::TopBottomPanel::top(egui::Id::new("conflicts").with(pane))
            .frame(egui::Frame::none().fill(bg).inner_margin(egui::Margin::symmetric(10.0, 4.0)))
            .show_inside(ui, |ui| {
                // what is going on: « Rebase of feature onto master — commit 3 of 7 »
                if let Some((o, info)) = &op {
                    ui.horizontal_wrapped(|ui| {
                        let what = match o {
                            Operation::Rebase => tf!("Rebase de {} sur {}", "Rebase of {} onto {}", info.branch, info.onto),
                            Operation::Merge => tf!("Fusion de {} dans {}", "Merge of {} into {}", info.onto, info.branch),
                            Operation::CherryPick => tf!("Cherry-pick sur {}", "Cherry-pick onto {}", info.branch),
                            Operation::Revert => tf!("Revert sur {}", "Revert on {}", info.branch),
                        };
                        ui.label(RichText::new(format!("⇄ {what}")).strong());
                        if let Some((k, n)) = info.step {
                            ui.label(RichText::new(tf!("— commit {k} sur {n}", "— commit {k} of {n}")).weak());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(t!("Abandonner", "Abort")).on_hover_text(t!("Remet la branche comme avant l'opération", "Puts the branch back as before the operation")).clicked() {
                                act = Some(Act::Abort);
                            }
                        });
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    if conflicts.is_empty() {
                        ui.label(RichText::new(t!("✔ Plus de conflit dans ce fichier", "✔ No conflict left in this file")).color(ok()).strong());
                        if rel.is_none() {
                            return;
                        }
                        let accent = self.hl.colors.accent;
                        match (others, &op) {
                            (0, Some((o, _))) => {
                                let label = if *o == Operation::Rebase { t!("✔ Marquer résolu et continuer le rebase ▶", "✔ Mark resolved and continue the rebase ▶") } else { t!("✔ Marquer résolu et continuer ▶", "✔ Mark resolved and continue ▶") };
                                if ui.add(filled_button(label, accent)).on_hover_text(t!("Enregistre, git add, puis git continue (commit suivant, ou fin)", "Saves, git add, then git goes on (next commit, or the end)")).clicked() {
                                    act = Some(Act::Mark(Some(false)));
                                }
                                if ui.button(t!("✔ Marquer résolu", "✔ Mark resolved")).on_hover_text(t!("Enregistre et git add", "Saves and git add")).clicked() {
                                    act = Some(Act::Mark(None));
                                }
                            }
                            (0, None) => {
                                if ui.add(filled_button(t!("✔ Marquer résolu", "✔ Mark resolved"), accent)).on_hover_text(t!("Enregistre et git add", "Saves and git add")).clicked() {
                                    act = Some(Act::Mark(None));
                                }
                            }
                            (n, _) => {
                                if ui.add(filled_button(tf!("✔ Marquer résolu, fichier suivant › ({n} restant(s))", "✔ Mark resolved, next file › ({n} left)"), accent)).on_hover_text(t!("Enregistre, git add, et ouvre le fichier en conflit suivant", "Saves, git add, and opens the next conflicting file")).clicked() {
                                    act = Some(Act::Mark(Some(true)));
                                }
                            }
                        }
                        return;
                    }
                    let c = &conflicts[k];
                    ui.label(RichText::new(tf!("⚠ Conflit {}/{}", "⚠ Conflict {}/{}", k + 1, conflicts.len())).color(warn()).strong());
                    if ui.small_button("◀").on_hover_text(t!("Conflit précédent", "Previous conflict")).clicked() {
                        act = Some(Act::Goto(conflicts[(k + conflicts.len() - 1) % conflicts.len()].start));
                    }
                    if ui.small_button("▶").on_hover_text(t!("Conflit suivant", "Next conflict")).clicked() {
                        act = Some(Act::Goto(conflicts[(k + 1) % conflicts.len()].start));
                    }
                    ui.add_space(8.0);
                    let green_l = ui.button(RichText::new(t!("Garder le vert", "Keep the green")).color(highlight::green())).on_hover_text(green.as_str());
                    if green_l.clicked() {
                        act = Some(Act::Resolve(c.clone(), Resolution::Ours));
                    }
                    if ui.button(RichText::new(t!("Garder le bleu", "Keep the blue")).color(highlight::blue())).on_hover_text(blue.as_str()).clicked() {
                        act = Some(Act::Resolve(c.clone(), Resolution::Theirs));
                    }
                    if ui.button(t!("Garder les deux", "Keep both")).on_hover_text(t!("Le vert puis le bleu", "The green then the blue")).clicked() {
                        act = Some(Act::Resolve(c.clone(), Resolution::Both));
                    }
                    if claude_on && rel.is_some() && ui.button(t!("✳ Résoudre avec Claude", "✳ Resolve with Claude")).on_hover_text(t!("Claude reçoit le fichier, les deux côtés et ce que fait le commit, et retire les marqueurs ; tu relis puis tu marques résolu", "Claude gets the file, both sides and what the commit does, and removes the markers; you check then mark resolved")).clicked() {
                        act = Some(Act::Claude);
                    }
                    ui.label(RichText::new(t!("(Ctrl+Z pour revenir)", "(Ctrl+Z to go back)")).weak().small());
                });
                // the real names of the sides: in a rebase the green one is the base, not your branch
                if !conflicts.is_empty() && !(green.is_empty() && blue.is_empty()) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("■").color(highlight::green()));
                        ui.label(RichText::new(tf!("vert : {green}", "green: {green}")).small());
                        ui.add_space(12.0);
                        ui.label(RichText::new("■").color(highlight::blue()));
                        ui.label(RichText::new(tf!("bleu : {blue}", "blue: {blue}")).small());
                    });
                }
            });
        let Some(a) = act else { return };
        match a {
            Act::Resolve(c, how) => {
                self.docs[i].resolve_conflict(&c, how);
                self.active = i;
                self.pending_focus = true;
            }
            Act::Goto(l) => {
                self.docs[i].goto_line(l, 0);
                self.active = i;
            }
            Act::Mark(then) => {
                let (Some(repo), Some(rel)) = (repo, rel) else { return };
                self.docs[i].had_conflicts = false;
                if !self.save(i) {
                    return;
                }
                let mut cmds = vec![vec!["add".to_string(), "--".to_string(), rel.clone()]];
                if let (Some(false), Some((o, _))) = (then, &op) {
                    let mut c = vec!["-c".to_string(), "core.editor=true".to_string()];
                    c.extend(o.continue_args().iter().map(|s| s.to_string()));
                    cmds.push(c);
                }
                if then == Some(true) {
                    self.open_next_conflict(&repo, Some(&rel));
                }
                self.git_seq_at(repo, "add", cmds);
            }
            Act::Claude => {
                let (Some(repo), Some(rel), Some((o, info))) = (repo, rel, op) else {
                    return;
                };
                self.conflicts_to_claude(&repo, o, &info, &[rel]);
            }
            Act::Abort => {
                if let (Some(repo), Some((o, _))) = (repo, op) {
                    self.git_job_at(repo, &o.abort_args().join(" "), &o.abort_args());
                }
            }
        }
    }

    /// A link after a `<<<<<<<` was clicked in the editor (`doc.conflict_pick`): the side kept,
    /// or the two sides compared in a tab.
    pub(super) fn conflict_picked(&mut self, i: usize) {
        let Some((line, how)) = self.docs.get_mut(i).and_then(|d| d.conflict_pick.take()) else { return };
        let Some(c) = self.docs[i].conflicts().into_iter().find(|c| c.start == line) else { return };
        match how {
            Some(how) => self.docs[i].resolve_conflict(&c, how),
            None => {
                let d = &self.docs[i];
                let text = |a: usize, b: usize| -> String { (a..b).map(|l| d.rope.line(l).to_string()).collect() };
                let (green, blue) = (text(c.start + 1, c.base.unwrap_or(c.mid)), text(c.mid + 1, c.end));
                if let (Ok(a), Ok(b)) = (crate::local_history::temp_file("vert", &green), crate::local_history::temp_file("bleu", &blue)) {
                    let label = |side: &str, name: &str| if name.is_empty() { side.to_string() } else { format!("{side} ({name})") };
                    let (la, lb) = (label(t!("vert", "green"), &c.ours), label(t!("bleu", "blue"), &c.theirs));
                    self.compare_labeled(a, b, la, lb);
                }
            }
        }
    }

    /// « Résoudre avec Claude »: a new conversation in the repository gets the files with
    /// blocks, what the operation does and both sides.
    pub(super) fn conflicts_to_claude(&mut self, repo: &Path, op: Operation, info: &OpInfo, files: &[String]) {
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
        // what each commit changes read on a thread
        let (repo, files) = (repo.to_path_buf(), files.to_vec());
        self.jobs.spawn(move || {
            for f in &files {
                text += &format!("\n\n- `{f}`");
                // what the commit itself changes in that file: the change to keep
                if let Some(rev) = rev {
                    let diff = git::stdout(&repo, &["show", "--format=", rev, "--", f]).unwrap_or_default();
                    if !diff.trim().is_empty() {
                        let lines: Vec<&str> = diff.lines().collect();
                        let cut = if lines.len() > CLAUDE_DIFF_LINES { t!("\n… (coupé)", "\n… (cut)") } else { "" };
                        text += &format!("\n  {}\n```diff\n{}{cut}\n```", t!("Ce que le commit y change :", "What the commit changes in it:"), lines[..lines.len().min(CLAUDE_DIFF_LINES)].join("\n"));
                    }
                }
            }
            JobResult::ConflictsForClaude(repo, files.len(), text)
        });
    }

    /// The request « Résoudre avec Claude » written: sent to a conversation in the repository.
    pub(super) fn conflicts_sent(&mut self, repo: PathBuf, n: usize, text: String) {
        if !(self.claude.is_empty() && self.claude.dir.as_deref() == Some(repo.as_path())) {
            self.claude_new_in(repo);
        }
        self.open_claude();
        if self.claude_send_text(text) {
            self.set_status(tf!("{n} fichier(s) donné(s) à Claude : relis puis « Marquer résolu »", "{n} file(s) given to Claude: check then “Mark resolved”"));
        }
    }
}

/// The commit git is applying or merging (REBASE_HEAD, CHERRY_PICK_HEAD…), read from the files
/// of the repository without starting git: it tells one stop of a rebase from the next.
fn applied_head(repo: &Path) -> String {
    let dir = repo.join(".git");
    ["REBASE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "MERGE_HEAD"].iter().find_map(|f| std::fs::read_to_string(dir.join(f)).ok()).unwrap_or_default().trim().to_string()
}
