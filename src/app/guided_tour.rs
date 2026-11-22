//! 🗺 The guided tour of changes, in a tab: the problem, the idea, then one step per idea (a short
//! explanation, what could break, then each of its blocks of the diff under a note saying what
//! it does), and what is left to decide. On the changes not committed, an idea is kept or taken out of the files as a whole
//! (↶ puts it back); on a pull request or a comment, the tour only explains.

use super::*;
use crate::{github, split};
use crate::split::Block;
use crate::tour::{self, Tour, Undo};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum TourSource {
    /// The changes not committed of the repository.
    Worktree,
    /// A pull request (its number).
    Pr(u64),
    /// A comment of a pull request (its number).
    Comment(u64),
}

#[derive(Clone, Copy, Default, PartialEq)]
enum StepState {
    #[default]
    Todo,
    Kept,
    /// Taken out of the files (changes not committed), or not understood (elsewhere).
    Dropped,
}

#[derive(Default)]
struct StepUi {
    state: StepState,
    /// How to put back its blocks taken out of the files.
    undo: Vec<Undo>,
    /// Its blocks are being taken out or put back.
    busy: bool,
    /// The talk with Claude about it: (written by the developer, text).
    thread: Vec<(bool, String)>,
    draft: String,
    asking: bool,
}

pub(super) struct TourUi {
    repo: PathBuf,
    source: TourSource,
    /// What is explained ("Changements non commités", "PR #12"…).
    label: String,
    about: String,
    /// The comment explained (TourSource::Comment).
    comment: String,
    loading: bool,
    error: Option<String>,
    tour: Option<Tour>,
    blocks: Vec<Block>,
    steps: Vec<StepUi>,
    /// The page shown: 0 the whole, 1..=n a step, n + 1 what is left to decide.
    page: usize,
    /// Number of this tour: answers for an older one are dropped.
    gen: u64,
}

impl TourUi {
    fn editable(&self) -> bool {
        self.source == TourSource::Worktree
    }
}

enum TourAct {
    Page(usize),
    Keep(usize),
    Drop(usize),
    Restore(usize),
    Ask(usize),
    Open(String),
    Again,
}

/// Lines added and removed by a block.
fn block_stats(b: &Block) -> (usize, usize) {
    b.lines.iter().filter(|l| !l.starts_with("@@")).fold((0, 0), |(a, d), l| match l.chars().next() {
        Some('+') => (a + 1, d),
        Some('-') => (a, d + 1),
        _ => (a, d),
    })
}

/// A titled part of a step, its text in Markdown (nothing when it is empty).
fn section(ui: &mut egui::Ui, cache: &mut egui_commonmark::CommonMarkCache, title: &str, text: &str, color: Color32) {
    if text.trim().is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong().color(color));
    ui.push_id(title, |ui| super::md::markdown(ui, cache, None, text));
}

/// A block of the diff, unfolded (folded when long).
fn block_ui(ui: &mut egui::Ui, b: &Block, id: (u64, usize)) {
    let (a, d) = block_stats(b);
    let what = match b.kind {
        'A' => t!(" (nouveau)", " (new)"),
        'D' => t!(" (supprimé)", " (deleted)"),
        _ => "",
    };
    let open = b.lines.len() <= 60;
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id(("tour_block", id)), open)
        .show_header(ui, |ui| {
            ui.label(RichText::new(format!("{}{what}", b.path)).monospace());
            ui.label(RichText::new(format!("+{a}")).monospace().color(ok()));
            ui.label(RichText::new(format!("−{d}")).monospace().color(err()));
        })
        .body(|ui| {
            egui::Frame::none().fill(ui.visuals().extreme_bg_color).rounding(4.0).inner_margin(6.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for l in b.lines.iter().take(400) {
                    let color = match l.chars().next() {
                        _ if l.starts_with("@@") => ui.visuals().weak_text_color(),
                        Some('+') => ok(),
                        Some('-') => err(),
                        _ => ui.visuals().text_color(),
                    };
                    ui.label(RichText::new(l).monospace().color(color));
                }
                if b.lines.len() > 400 {
                    ui.label(RichText::new(tf!("… {} lignes de plus", "… {} more lines", b.lines.len() - 400)).weak());
                }
            });
        });
}

impl App {
    /// A new tour, shown loading in its tab; returns its number.
    fn tour_begin(&mut self, repo: PathBuf, source: TourSource, label: String, about: String, comment: String) -> u64 {
        self.tour_gen += 1;
        let gen = self.tour_gen;
        self.guided = Some(TourUi { repo, source, label, about, comment, loading: true, error: None, tour: None, blocks: Vec::new(), steps: Vec::new(), page: 0, gen });
        self.show_tab(document::View::Tour);
        self.set_status(t!("Claude range les changements par idée…", "Claude sorts the changes by idea…"));
        gen
    }

    /// 🗺 The tour of the changes not committed of `repo`.
    pub(super) fn explain_worktree(&mut self, repo: PathBuf) {
        let Some(root) = git::repo_root(&repo) else {
            self.set_status(t!("Pas un dépôt git", "Not a git repository"));
            return;
        };
        let wish = self.claude.wish.clone().filter(|_| self.claude.dir.as_ref() == Some(&repo) || self.claude.dir.as_ref() == Some(&root)).unwrap_or_default();
        let about = if wish.trim().is_empty() { String::new() } else { tf!("Ce que le développeur avait demandé : {wish}", "What the developer had asked: {wish}") };
        let gen = self.tour_begin(root.clone(), TourSource::Worktree, t!("Changements non commités", "Uncommitted changes").into(), about.clone(), String::new());
        self.jobs.spawn(move || {
            let r = split::worktree_changes(&root).and_then(|files| {
                let blocks = split::blocks(&files);
                if blocks.is_empty() {
                    return Err(t!("Aucun changement à expliquer.", "No change to explain.").to_string());
                }
                tour::explain(&root, &blocks, &about).map(|t| (t, blocks))
            });
            JobResult::Tour(gen, r)
        });
    }

    /// 🗺 The tour of pull request `d`.
    pub(super) fn explain_pr(&mut self, repo: PathBuf, d: &github::PrDetail) {
        let about = tf!("Pull request « {} »\n{}", "Pull request “{}”\n{}", d.title, d.body);
        let gen = self.tour_begin(repo.clone(), TourSource::Pr(d.number), format!("PR #{}", d.number), about.clone(), String::new());
        let diff = d.diff.clone();
        self.jobs.spawn(move || {
            let blocks = tour::blocks_of_diff(&diff);
            let r = if blocks.is_empty() { Err(t!("La pull request ne change rien.", "The pull request changes nothing.").to_string()) } else { tour::explain(&repo, &blocks, &about).map(|t| (t, blocks)) };
            JobResult::Tour(gen, r)
        });
    }

    /// 🗺 A conversation (`id`) of pull request `d` explained.
    pub(super) fn explain_thread(&mut self, repo: PathBuf, d: &github::PrDetail, id: &str) {
        let Some(t) = d.threads.iter().find(|t| t.id == id) else { return };
        let comment = super::pr_claude::thread_text(t);
        let about = tf!("Pull request « {} »\n{}", "Pull request “{}”\n{}", d.title, d.body);
        let gen = self.tour_begin(repo.clone(), TourSource::Comment(d.number), tf!("Commentaire sur {} (PR #{})", "Comment on {} (PR #{})", t.path.rsplit('/').next().unwrap_or(&t.path), d.number), about.clone(), comment.clone());
        self.jobs.spawn(move || JobResult::Tour(gen, tour::explain_comment(&repo, &comment, &about).map(|t| (t, Vec::new()))));
    }

    /// The tour explained again from the start (the ideas taken out stay out).
    fn tour_again(&mut self) {
        let Some(g) = self.guided.as_ref() else { return };
        let (repo, source, about, comment) = (g.repo.clone(), g.source, g.about.clone(), g.comment.clone());
        match source {
            TourSource::Worktree => self.explain_worktree(repo),
            TourSource::Pr(n) | TourSource::Comment(n) => match self.pr.detail.clone() {
                Some(Ok(d)) if d.number == n && source == TourSource::Pr(n) => self.explain_pr(repo, &d),
                _ if !comment.is_empty() => {
                    let label = self.guided.as_ref().map(|g| g.label.clone()).unwrap_or_default();
                    let gen = self.tour_begin(repo.clone(), source, label, about.clone(), comment.clone());
                    self.jobs.spawn(move || JobResult::Tour(gen, tour::explain_comment(&repo, &comment, &about).map(|t| (t, Vec::new()))));
                }
                _ => self.set_status(t!("Rouvre la pull request pour la réexpliquer", "Open the pull request again to explain it again")),
            },
        }
    }

    pub(super) fn tour_loaded(&mut self, gen: u64, r: Result<(Tour, Vec<Block>), String>) {
        let Some(g) = self.guided.as_mut().filter(|g| g.gen == gen) else { return };
        g.loading = false;
        match r {
            Ok((tour, blocks)) => {
                let n = tour.steps.len();
                g.steps = (0..n).map(|_| StepUi::default()).collect();
                g.tour = Some(tour);
                g.blocks = blocks;
                g.page = 0;
                let title = self.tour_title();
                if let Some(d) = self.docs.iter_mut().find(|d| d.view == document::View::Tour) {
                    d.title_override = Some(title);
                }
                self.set_status(tf!("🗺 {n} idée(s) à parcourir", "🗺 {n} idea(s) to go through"));
            }
            Err(e) => {
                let first = e.lines().next().unwrap_or("").to_string();
                g.error = Some(e);
                self.set_error(tf!("Visite : échec — {first}", "Tour: failed — {first}"));
            }
        }
    }

    /// The blocks of step `k` taken out of the files, or put back.
    fn tour_toggle(&mut self, k: usize, out: bool) {
        let Some(g) = self.guided.as_mut() else { return };
        let Some(s) = g.steps.get_mut(k) else { return };
        let Some(tour) = g.tour.as_ref() else { return };
        s.busy = true;
        let (repo, gen) = (g.repo.clone(), g.gen);
        if out {
            let blocks: Vec<Block> = tour.steps[k].blocks.iter().filter_map(|&b| g.blocks.get(b).cloned()).collect();
            let title = format!("{} — {}", tour.title, tour.steps[k].title);
            self.jobs.spawn(move || {
                let r = tour::discard(&repo, &blocks);
                // a copy on the disk: closing the tour or a crash does not lose the code
                if let Ok(undo) = &r {
                    let _ = tour::save_copy(&repo, &title, undo);
                }
                JobResult::TourDrop(gen, k, r)
            });
        } else {
            let undo = s.undo.clone();
            self.jobs.spawn(move || JobResult::TourRestore(gen, k, tour::restore(&repo, &undo)));
        }
    }

    pub(super) fn tour_dropped(&mut self, gen: u64, k: usize, r: Result<Vec<Undo>, String>) {
        let Some(g) = self.guided.as_mut().filter(|g| g.gen == gen) else { return };
        let Some(s) = g.steps.get_mut(k) else { return };
        s.busy = false;
        let paths: Vec<PathBuf> = g.tour.as_ref().map(|t| t.steps[k].blocks.iter().filter_map(|&b| g.blocks.get(b)).map(|b| g.repo.join(&b.path)).collect()).unwrap_or_default();
        match r {
            Ok(undo) => {
                s.undo = undo;
                s.state = StepState::Dropped;
                self.set_status(t!("Idée enlevée des fichiers (↶ pour la remettre)", "Idea taken out of the files (↶ to put it back)"));
            }
            Err(e) => self.set_error(tf!("Idée pas enlevée : {e}", "Idea not taken out: {e}")),
        }
        self.tour_files_changed(paths);
    }

    pub(super) fn tour_restored(&mut self, gen: u64, k: usize, r: Result<(), String>) {
        let Some(g) = self.guided.as_mut().filter(|g| g.gen == gen) else { return };
        let Some(s) = g.steps.get_mut(k) else { return };
        s.busy = false;
        let paths: Vec<PathBuf> = g.tour.as_ref().map(|t| t.steps[k].blocks.iter().filter_map(|&b| g.blocks.get(b)).map(|b| g.repo.join(&b.path)).collect()).unwrap_or_default();
        match r {
            Ok(()) => {
                s.undo.clear();
                s.state = StepState::Todo;
                self.set_status(t!("Idée remise dans les fichiers", "Idea put back in the files"));
            }
            Err(e) => self.set_error(tf!("Idée pas remise : {e}", "Idea not put back: {e}")),
        }
        self.tour_files_changed(paths);
    }

    /// The open tabs of `paths` and the Git view follow the files.
    fn tour_files_changed(&mut self, paths: Vec<PathBuf>) {
        for p in paths {
            self.reload_open(&p);
        }
        self.git_refresh();
    }

    /// The question written on step `k` sent to Claude.
    fn tour_ask(&mut self, k: usize) {
        let Some(g) = self.guided.as_mut() else { return };
        let (Some(tour), Some(s)) = (g.tour.clone(), g.steps.get_mut(k)) else { return };
        let q = s.draft.trim().to_string();
        if q.is_empty() || s.asking {
            return;
        }
        s.draft.clear();
        s.thread.push((true, q));
        s.asking = true;
        let (repo, gen, blocks, thread) = (g.repo.clone(), g.gen, g.blocks.clone(), s.thread.clone());
        self.jobs.spawn(move || JobResult::TourAnswer(gen, k, tour::ask(&repo, &tour, k, &blocks, &thread)));
    }

    pub(super) fn tour_answered(&mut self, gen: u64, k: usize, r: Result<String, String>) {
        let Some(s) = self.guided.as_mut().filter(|g| g.gen == gen).and_then(|g| g.steps.get_mut(k)) else { return };
        s.asking = false;
        match r {
            Ok(a) => s.thread.push((false, a)),
            Err(e) => s.thread.push((false, format!("⚠ {}", e.lines().next().unwrap_or("")))),
        }
    }

    /// The tab closed: the tour is forgotten (the ideas taken out stay out).
    pub(super) fn tour_closed(&mut self) {
        self.guided = None;
    }

    pub(super) fn tour_title(&self) -> String {
        match self.guided.as_ref().and_then(|g| g.tour.as_ref()) {
            Some(t) if !t.title.is_empty() => format!("🗺 {}", t.title),
            _ => t!("🗺 Visite", "🗺 Tour").into(),
        }
    }

    pub(super) fn tour_ui(&mut self, ui: &mut egui::Ui) {
        let accent = self.hl.colors.accent;
        let mut act = None;
        let (Some(g), cache) = (self.guided.as_mut(), &mut self.md_cache) else {
            ui.centered_and_justified(|ui| ui.label(RichText::new(t!("Aucune visite : 🗺 Expliquer, dans Git ou une pull request", "No tour: 🗺 Explain, in Git or a pull request")).weak()));
            return;
        };
        let fill = ui.visuals().panel_fill;
        egui::TopBottomPanel::top(egui::Id::new("tour_top"))
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 12.0, right: 12.0, top: 10.0, bottom: 6.0 }))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    // 🗺 is in the title of the tab, not here again
                    let title = g.tour.as_ref().map(|t| t.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| t!("Visite guidée", "Guided tour").into());
                    ui.label(RichText::new(title).size(17.0).strong());
                    ui.label(RichText::new(&g.label).weak());
                    if g.loading {
                        spinner(ui);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(!g.loading, egui::Button::new("⟳")).on_hover_text(t!("Réexpliquer depuis le début (les idées enlevées restent enlevées)", "Explain again from the start (the ideas taken out stay out)")).clicked() {
                            act = Some(TourAct::Again);
                        }
                    });
                });
            });
        if g.loading {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    spinner(ui);
                    ui.label(t!("Claude lit les changements et les range par idée… (une à quelques minutes)", "Claude reads the changes and sorts them by idea… (one to a few minutes)"));
                });
            });
            return;
        }
        let Some(tour) = g.tour.as_ref() else {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                if let Some(e) = &g.error {
                    ui.label(RichText::new(format!("⚠ {e}")).color(err()));
                }
            });
            if matches!(act, Some(TourAct::Again)) {
                self.tour_again();
            }
            return;
        };
        let n = tour.steps.len();
        let editable = g.editable();
        // the steps on the left
        // the titles go to the line: a long one must not widen the panel past its room
        egui::SidePanel::left(egui::Id::new("tour_steps"))
            .resizable(true)
            .default_width(260.0)
            .width_range(180.0..=420.0)
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 8.0, top: 4.0, bottom: 4.0 }))
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("tour_steps_list").auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_space(6.0);
                    let mut row = |ui: &mut egui::Ui, page: usize, mark: RichText, text: &str| {
                        let (sel, w) = (g.page == page, ui.available_width());
                        ui.horizontal_top(|ui| {
                            ui.set_max_width(w);
                            ui.label(mark);
                            let text = if sel { RichText::new(text).strong().color(accent) } else { RichText::new(text) };
                            if ui.add(egui::Label::new(text).wrap().sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                act = Some(TourAct::Page(page));
                            }
                        });
                        ui.add_space(2.0);
                    };
                    row(ui, 0, RichText::new("🎯"), t!("Le problème et l'idée", "The problem and the idea"));
                    ui.separator();
                    for (k, st) in tour.steps.iter().enumerate() {
                        let mark = match g.steps[k].state {
                            StepState::Todo => RichText::new("○").color(ui.visuals().weak_text_color()),
                            StepState::Kept => RichText::new("✔").color(ok()),
                            StepState::Dropped => RichText::new("✖").color(err()),
                        };
                        row(ui, k + 1, mark, &format!("{}. {}", k + 1, st.title));
                    }
                    ui.separator();
                    row(ui, n + 1, RichText::new("⚖"), t!("À décider", "To decide"));
                    ui.add_space(10.0);
                    let kept = g.steps.iter().filter(|s| s.state == StepState::Kept).count();
                    let dropped = g.steps.iter().filter(|s| s.state == StepState::Dropped).count();
                    let text = if editable {
                        tf!("{kept} gardée(s), {dropped} enlevée(s) sur {n}", "{kept} kept, {dropped} taken out of {n}")
                    } else {
                        tf!("{kept} comprise(s) sur {n}", "{kept} understood of {n}")
                    };
                    ui.label(RichText::new(text).small().weak());
                });
            });
        // the page reads like a document: the color of the editor, room around the text
        let page_frame = egui::Frame::none().fill(self.hl.colors.bg).inner_margin(egui::Margin { left: 20.0, right: 20.0, top: 8.0, bottom: 0.0 });
        egui::CentralPanel::default().frame(page_frame).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt(("tour_page", g.page)).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(900.0));
                ui.add_space(6.0);
                if g.page == 0 {
                    section(ui, cache, t!("🎯 Le problème", "🎯 The problem"), &tour.problem, accent);
                    section(ui, cache, t!("💡 L'idée", "💡 The idea"), &tour.idea, accent);
                    ui.add_space(8.0);
                    ui.label(RichText::new(t!("👣 Les étapes", "👣 The steps")).strong().color(accent));
                    for (k, st) in tour.steps.iter().enumerate() {
                        if ui.link(format!("{}. {}", k + 1, st.title)).clicked() {
                            act = Some(TourAct::Page(k + 1));
                        }
                    }
                    ui.add_space(10.0);
                    if editable {
                        ui.label(RichText::new(t!("Pour chaque idée : ✔ la garder, ou ✖ l'enlever des fichiers (tous ses blocs d'un coup, ↶ pour la remettre).", "For each idea: ✔ keep it, or ✖ take it out of the files (all its blocks at once, ↶ to put it back).")).weak());
                    }
                    if ui.add(filled_button(t!("Commencer ▸", "Start ▸"), accent)).clicked() {
                        act = Some(TourAct::Page(1));
                    }
                } else if g.page > n {
                    section(ui, cache, t!("⚖ À décider", "⚖ To decide"), &tour.decide, accent);
                    ui.add_space(10.0);
                    for (k, st) in tour.steps.iter().enumerate() {
                        let (mark, color) = match g.steps[k].state {
                            StepState::Todo => ("○", ui.visuals().weak_text_color()),
                            StepState::Kept => ("✔", ok()),
                            StepState::Dropped => ("✖", err()),
                        };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(mark).color(color));
                            if ui.link(format!("{}. {}", k + 1, st.title)).clicked() {
                                act = Some(TourAct::Page(k + 1));
                            }
                        });
                    }
                    if editable && g.steps.iter().any(|s| s.state == StepState::Dropped) {
                        ui.add_space(6.0);
                        ui.label(RichText::new(t!("Les idées enlevées ne sont plus dans les fichiers ; fermer la visite les laisse enlevées. Une copie est gardée 30 jours :", "The ideas taken out are no longer in the files; closing the tour leaves them out. A copy is kept 30 days:")).weak());
                        if ui.link(RichText::new(tour::copies_dir().display().to_string()).monospace()).clicked() {
                            let _ = crate::os::reveal(&tour::copies_dir());
                        }
                    }
                } else {
                    let k = g.page - 1;
                    let st = &tour.steps[k];
                    let s = &mut g.steps[k];
                    ui.label(RichText::new(tf!("Étape {} sur {n}", "Step {} of {n}", k + 1)).small().weak());
                    ui.label(RichText::new(&st.title).size(18.0).strong());
                    if !st.text.is_empty() {
                        ui.add_space(6.0);
                        ui.push_id("tour_text", |ui| super::md::markdown(ui, cache, None, &st.text));
                    }
                    section(ui, cache, t!("⚠ Ce qui pourrait casser", "⚠ What could break"), &st.risks, warn());
                    // the places to read, for a comment (a step with blocks shows them instead)
                    if st.blocks.is_empty() && !st.places.is_empty() {
                        ui.add_space(6.0);
                        ui.label(RichText::new(t!("Où", "Where")).strong().color(accent));
                        ui.horizontal_wrapped(|ui| {
                            for p in &st.places {
                                if ui.link(RichText::new(p).monospace()).on_hover_text(t!("Ouvrir à cette ligne", "Open at this line")).clicked() {
                                    act = Some(TourAct::Open(p.clone()));
                                }
                            }
                        });
                    }
                    // the code block by block, each under what it does
                    for (i, &b) in st.blocks.iter().enumerate() {
                        let Some(block) = g.blocks.get(b) else { continue };
                        ui.add_space(12.0);
                        if let Some(note) = st.notes.get(i).filter(|n| !n.is_empty()) {
                            ui.push_id(("tour_note", b), |ui| super::md::markdown(ui, cache, None, note));
                            ui.add_space(2.0);
                        }
                        block_ui(ui, block, (g.gen, b));
                    }
                    // the decision
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if s.busy {
                            spinner(ui);
                        }
                        match (editable, s.state) {
                            (true, StepState::Dropped) => {
                                ui.label(RichText::new(t!("✖ Enlevée des fichiers", "✖ Taken out of the files")).color(err()));
                                if ui.add_enabled(!s.busy, egui::Button::new(t!("↶ Remettre", "↶ Put back"))).on_hover_text(t!("Remettre ses blocs dans les fichiers", "Put its blocks back in the files")).clicked() {
                                    act = Some(TourAct::Restore(k));
                                }
                            }
                            (true, state) => {
                                let keep = if state == StepState::Kept { t!("✔ Gardée", "✔ Kept") } else { t!("✔ Je garde cette idée", "✔ I keep this idea") };
                                if ui.add_enabled(!s.busy, filled_button(keep, if state == StepState::Kept { ok() } else { accent })).clicked() {
                                    act = Some(TourAct::Keep(k));
                                }
                                let tip = t!("Tous ses blocs sont retirés des fichiers (↶ pour les remettre)", "All its blocks are taken out of the files (↶ to put them back)");
                                if ui.add_enabled(!s.busy && !st.blocks.is_empty(), egui::Button::new(t!("✖ J'enlève cette idée", "✖ I take this idea out"))).on_hover_text(tip).clicked() {
                                    act = Some(TourAct::Drop(k));
                                }
                            }
                            (false, state) => {
                                let label = if state == StepState::Kept { t!("✔ Compris", "✔ Understood") } else { t!("✔ J'ai compris", "✔ I understand") };
                                if ui.add(filled_button(label, if state == StepState::Kept { ok() } else { accent })).clicked() {
                                    act = Some(TourAct::Keep(k));
                                }
                            }
                        }
                    });
                    // the talk about it
                    ui.add_space(10.0);
                    ui.label(RichText::new(t!("💬 Une question sur cette étape ?", "💬 A question about this step?")).strong().color(accent));
                    for (dev, text) in &s.thread {
                        let who = if *dev { t!("Toi", "You") } else { "✳ Claude" };
                        egui::Frame::none().fill(if *dev { ui.visuals().faint_bg_color } else { icons::mix(fill, accent, 0.06) }).rounding(6.0).inner_margin(8.0).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(who).small().strong());
                            ui.push_id(("tour_talk", k, text.len()), |ui| super::md::markdown(ui, cache, None, text));
                        });
                        ui.add_space(4.0);
                    }
                    if s.asking {
                        ui.horizontal(|ui| {
                            spinner(ui);
                            ui.label(RichText::new(t!("Claude répond…", "Claude is answering…")).weak());
                        });
                    }
                    let r = ui.add(egui::TextEdit::multiline(&mut s.draft).id_salt(("tour_draft", k)).hint_text(t!("« Pourquoi pas un simple dictionnaire ? », « Donne un exemple »… (Ctrl+Entrée)", "“Why not a plain dict?”, “Give an example”… (Ctrl+Enter)")).desired_rows(2).desired_width(f32::INFINITY));
                    let send = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
                    if (ui.add_enabled(!s.asking && !s.draft.trim().is_empty(), egui::Button::new(t!("Demander à Claude", "Ask Claude"))).clicked() || send) && !s.asking {
                        act = Some(TourAct::Ask(k));
                    }
                    // previous and next
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button(t!("◂ Précédente", "◂ Previous")).clicked() {
                            act = Some(TourAct::Page(g.page - 1));
                        }
                        let next = if k + 1 < n { t!("Suivante ▸", "Next ▸") } else { t!("À décider ▸", "To decide ▸") };
                        if ui.button(next).clicked() {
                            act = Some(TourAct::Page(g.page + 1));
                        }
                    });
                }
                ui.add_space(20.0);
            });
        });
        match act {
            Some(TourAct::Page(p)) => g.page = p.min(n + 1),
            Some(TourAct::Keep(k)) => {
                g.steps[k].state = StepState::Kept;
                // on to the next one
                g.page = (k + 2).min(n + 1);
            }
            Some(TourAct::Drop(k)) => self.tour_toggle(k, true),
            Some(TourAct::Restore(k)) => self.tour_toggle(k, false),
            Some(TourAct::Ask(k)) => self.tour_ask(k),
            Some(TourAct::Open(p)) => {
                let (rel, line) = tour::place(&p);
                match crate::os::inside_repo(&g.repo, &rel).filter(|p| p.is_file()) {
                    Some(path) => self.open_at(path, line.saturating_sub(1)),
                    None => self.set_status(tf!("« {rel} » n'est pas dans tes fichiers", "“{rel}” is not in your files")),
                }
            }
            Some(TourAct::Again) => self.tour_again(),
            None => {}
        }
    }
}
