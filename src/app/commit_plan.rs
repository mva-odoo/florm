//! Several commits instead of one: Claude splits the changes not committed by theme (hunks or
//! whole files), the plan is read and changed in the Git view, nothing is committed before
//! « Commiter les N commits ».

use super::*;
use crate::claude;
use crate::split::{self, Block};

pub(super) struct CommitPlan {
    repo: PathBuf,
    blocks: Vec<Block>,
    /// The commits proposed, in order: message and blocks (indices in `blocks`).
    commits: Vec<(String, Vec<usize>)>,
    /// The commits are being made.
    busy: bool,
    error: Option<String>,
}

impl CommitPlan {
    /// Blocks in no commit: they stay in the files.
    fn left_out(&self) -> Vec<usize> {
        (0..self.blocks.len()).filter(|b| !self.commits.iter().any(|(_, bs)| bs.contains(b))).collect()
    }
}

/// What to do with a block of the plan: move it to a commit (None: commit it not).
enum PlanAct {
    Move(usize, Option<usize>),
    Up(usize),
    Remove(usize),
    Add,
    Commit,
    Cancel,
}

/// Lines added and removed by a block.
fn block_stats(b: &Block) -> (usize, usize) {
    let body = b.lines.iter().filter(|l| !l.starts_with("@@"));
    body.fold((0, 0), |(a, d), l| match l.chars().next() {
        Some('+') => (a + 1, d),
        Some('-') => (a, d + 1),
        _ => (a, d),
    })
}

impl App {
    /// Asks Claude to split the changes not committed into commits by theme.
    pub(super) fn start_commit_plan(&mut self) {
        let Some(repo) = self.repo.clone() else { return };
        let wish = self.claude.wish.clone().filter(|_| self.claude.dir.as_ref() == Some(&repo)).unwrap_or_default();
        self.git.planning = true;
        self.set_status(t!("Claude découpe les changements par thème…", "Claude splits the changes by theme…"));
        self.jobs.spawn(move || {
            let r = split::worktree_changes(&repo).and_then(|files| {
                let blocks = split::blocks(&files);
                if blocks.is_empty() {
                    return Err(t!("Rien à découper : aucun changement", "Nothing to split: no change").to_string());
                }
                let recent = git::stdout(&repo, &["log", "-12", "--format=%s"]).unwrap_or_default();
                let plan = claude::commit_plan(&repo, &split::blocks_text(&blocks), &recent, &wish)?;
                // numbers from 1, each block once (the first commit that names it)
                let mut seen = HashSet::new();
                let commits: Vec<(String, Vec<usize>)> = plan
                    .into_iter()
                    .map(|(m, bs)| (m, bs.into_iter().filter(|n| (1..=blocks.len()).contains(n) && seen.insert(*n)).map(|n| n - 1).collect::<Vec<_>>()))
                    .filter(|(_, bs)| !bs.is_empty())
                    .collect();
                if commits.is_empty() {
                    return Err(t!("Claude n'a proposé aucun commit", "Claude suggested no commit").to_string());
                }
                Ok((blocks, commits))
            });
            JobResult::CommitPlan(repo, r)
        });
    }

    pub(super) fn commit_plan_loaded(&mut self, repo: PathBuf, r: Result<(Vec<Block>, Vec<(String, Vec<usize>)>), String>) {
        self.git.planning = false;
        match r {
            Ok((blocks, commits)) => {
                let n = commits.len();
                self.git.plan = Some(CommitPlan { repo, blocks, commits, busy: false, error: None });
                self.set_status(tf!("{n} commit(s) proposé(s) — à relire avant de commiter", "{n} commit(s) suggested — to read over before committing"));
            }
            Err(e) => self.set_error(tf!("Pas de découpage proposé : {}", "No splitting suggested: {}", e.lines().next().unwrap_or(""))),
        }
    }

    /// The commits of the plan made one after the other; stops at the first that fails.
    fn commit_plan_run(&mut self) {
        let Some(p) = self.git.plan.as_mut() else { return };
        let commits: Vec<(String, Vec<Block>)> = p
            .commits
            .iter()
            .filter(|(_, bs)| !bs.is_empty())
            .map(|(m, bs)| (m.trim().to_string(), bs.iter().map(|&b| p.blocks[b].clone()).collect()))
            .collect();
        p.busy = true;
        p.error = None;
        let repo = p.repo.clone();
        self.jobs.spawn(move || {
            let mut done = Vec::new();
            for (message, blocks) in &commits {
                match split::commit_blocks(&repo, blocks, message) {
                    Ok(c) => done.push(c),
                    Err(e) => return JobResult::CommitPlanDone(repo.clone(), done, Some(e)),
                }
            }
            JobResult::CommitPlanDone(repo, done, None)
        });
    }

    pub(super) fn commit_plan_done(&mut self, repo: PathBuf, done: Vec<(String, String)>, error: Option<String>) {
        for (h, s) in &done {
            self.log(format!("commit {h} {s}"));
        }
        match error {
            None => {
                self.git.plan = None;
                self.git.commit_msg.clear();
                self.set_status(tf!("{} commit(s) créé(s)", "{} commit(s) made", done.len()));
            }
            Some(e) => {
                // the commits made stay out of the plan; the rest can be tried again
                if let Some(p) = self.git.plan.as_mut().filter(|p| p.repo == repo) {
                    p.busy = false;
                    let made = done.len();
                    let mut k = 0;
                    p.commits.retain(|(_, bs)| {
                        let keep = bs.is_empty() || k >= made;
                        if !bs.is_empty() {
                            k += 1;
                        }
                        keep
                    });
                    p.error = Some(e.clone());
                }
                self.set_error(tf!("Commit {} du plan : {e}", "Commit {} of the plan: {e}", done.len() + 1));
            }
        }
        self.git_refresh();
        self.load_history();
    }

    /// The plan, in place of the Git view's list and diff: the commits with their message and
    /// blocks, to change before committing.
    pub(super) fn commit_plan_ui(&mut self, ui: &mut egui::Ui) {
        let accent = self.hl.colors.accent;
        let Some(p) = self.git.plan.as_mut() else { return };
        let mut act = None;
        let n = p.commits.iter().filter(|(_, bs)| !bs.is_empty()).count();
        let ready = n > 0 && p.commits.iter().all(|(m, bs)| bs.is_empty() || !m.trim().is_empty());
        egui::TopBottomPanel::top("plan_top").show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("✂ Commits proposés par Claude", "✂ Commits suggested by Claude")).size(17.0).strong());
                if p.busy {
                    spinner(ui);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = tf!("✔ Commiter les {n} commits", "✔ Commit the {n} commits");
                    let tip = if ready { t!("Dans cet ordre ; ce qui est indexé est remis à zéro d'abord", "In this order; what is staged is reset first") } else { t!("Chaque commit doit avoir un message", "Each commit needs a message") };
                    if ui.add_enabled(ready && !p.busy, filled_button(&label, accent)).on_hover_text(tip).on_disabled_hover_text(tip).clicked() {
                        act = Some(PlanAct::Commit);
                    }
                    if ui.add_enabled(!p.busy, egui::Button::new(t!("Annuler", "Cancel"))).on_hover_text(t!("Rien n'est commité, les changements restent comme ils sont", "Nothing is committed, the changes stay as they are")).clicked() {
                        act = Some(PlanAct::Cancel);
                    }
                });
            });
            ui.label(RichText::new(t!(
                "Rien n'est commité avant « Commiter ». Relis les messages, déplace un bloc d'un commit à l'autre (→), ou laisse-le hors des commits.",
                "Nothing is committed before “Commit”. Read the messages over, move a block from one commit to another (→), or leave it out of the commits."
            )).weak());
            if let Some(e) = &p.error {
                ui.label(RichText::new(format!("⚠ {e}")).color(err()));
            }
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("plan_list").auto_shrink([false, false]).show(ui, |ui| {
                let targets: Vec<String> = (1..=p.commits.len()).map(|k| tf!("Commit {k}", "Commit {k}")).collect();
                let left = p.left_out();
                let (blocks, commits) = (&p.blocks, &mut p.commits);
                for (k, (message, bs)) in commits.iter_mut().enumerate() {
                    egui::Frame::group(ui.style()).inner_margin(8.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tf!("Commit {}", "Commit {}", k + 1)).strong().color(accent));
                            ui.label(RichText::new(tf!("{} bloc(s)", "{} block(s)", bs.len())).weak());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("×").on_hover_text(t!("Retirer ce commit (ses blocs restent dans les fichiers)", "Remove this commit (its blocks stay in the files)")).clicked() {
                                    act = Some(PlanAct::Remove(k));
                                }
                                if k > 0 && ui.small_button("↑").on_hover_text(t!("Le faire avant le précédent", "Make it before the previous one")).clicked() {
                                    act = Some(PlanAct::Up(k));
                                }
                            });
                        });
                        ui.add(egui::TextEdit::multiline(message).id_salt(("plan_msg", k)).hint_text(t!("Message du commit", "Commit message")).desired_rows(2).desired_width(f32::INFINITY));
                        if bs.is_empty() {
                            ui.label(RichText::new(t!("Vide : déplace des blocs ici avec →", "Empty: move blocks here with →")).weak().italics());
                        }
                        for &b in bs.iter() {
                            if let Some(to) = block_row(ui, &blocks[b], b, Some(k), &targets) {
                                act = Some(PlanAct::Move(b, to));
                            }
                        }
                    });
                    ui.add_space(6.0);
                }
                if ui.button(t!("+ Nouveau commit", "+ New commit")).on_hover_text(t!("Un commit vide où déplacer des blocs", "An empty commit to move blocks into")).clicked() {
                    act = Some(PlanAct::Add);
                }
                if !left.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new(tf!("Hors des commits ({}) : restent dans les fichiers", "Out of the commits ({}): stay in the files", left.len())).strong().color(warn()));
                    for &b in &left {
                        if let Some(to) = block_row(ui, &blocks[b], b, None, &targets) {
                            act = Some(PlanAct::Move(b, to));
                        }
                    }
                }
            });
        });
        match act {
            Some(PlanAct::Move(b, to)) => {
                for (_, bs) in p.commits.iter_mut() {
                    bs.retain(|x| *x != b);
                }
                if let Some(k) = to {
                    p.commits[k].1.push(b);
                    p.commits[k].1.sort_unstable();
                }
            }
            Some(PlanAct::Up(k)) => p.commits.swap(k, k - 1),
            Some(PlanAct::Remove(k)) => {
                p.commits.remove(k);
            }
            Some(PlanAct::Add) => p.commits.push((String::new(), Vec::new())),
            Some(PlanAct::Commit) => self.commit_plan_run(),
            Some(PlanAct::Cancel) => {
                self.git.plan = None;
                self.set_status(t!("Découpage abandonné : rien n'a été commité", "Splitting dropped: nothing was committed"));
            }
            None => {}
        }
    }
}

/// A block of the plan: its file and lines (unfolded on click), and → to move it; returns where
/// it goes when moved (None: out of the commits).
fn block_row(ui: &mut egui::Ui, b: &Block, id: usize, current: Option<usize>, targets: &[String]) -> Option<Option<usize>> {
    let mut to = None;
    let (a, d) = block_stats(b);
    let what = match b.kind {
        'A' => t!(" (nouveau)", " (new)"),
        'D' => t!(" (supprimé)", " (deleted)"),
        _ => "",
    };
    let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id(("plan_block", id)), false);
    state
        .show_header(ui, |ui| {
        ui.menu_button("→", |ui| {
            for (k, label) in targets.iter().enumerate() {
                if current != Some(k) && ui.button(label).clicked() {
                    to = Some(Some(k));
                    ui.close_menu();
                }
            }
            if current.is_some() && ui.button(t!("Hors des commits", "Out of the commits")).clicked() {
                to = Some(None);
                ui.close_menu();
            }
        })
        .response
        .on_hover_text(t!("Déplacer ce bloc", "Move this block"));
        ui.label(RichText::new(format!("{}{what}", b.path)).monospace());
        ui.label(RichText::new(format!("+{a}")).monospace().color(ok()));
        ui.label(RichText::new(format!("−{d}")).monospace().color(err()));
        })
        .body(|ui| {
            for l in b.lines.iter().take(300) {
                let color = match l.chars().next() {
                    Some('+') => ok(),
                    Some('-') => err(),
                    _ => ui.visuals().weak_text_color(),
                };
                ui.label(RichText::new(l).monospace().color(color));
            }
            if b.lines.len() > 300 {
                ui.label(RichText::new(tf!("… {} lignes de plus", "… {} more lines", b.lines.len() - 300)).weak());
            }
        });
    to
}
