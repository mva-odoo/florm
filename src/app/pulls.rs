//! Pull requests of GitHub (through `gh`): list, description and conversation, commits, diff,
//! comments on lines (read, answered, resolved, written from the diff) with their suggestions
//! applied in the files, and shown in the editor; checks, reviews, merge, reviewers and labels,
//! reactions, files viewed.

use super::*;
use crate::github::{self, Part};
use history::{DiffAct, DiffClick, DiffKind};

/// `gh pr list --state` values and their labels (French, English).
pub(super) const STATES: [(&str, &str, &str); 4] = [("open", "Ouvertes", "Open"), ("merged", "Fusionnées", "Merged"), ("closed", "Fermées", "Closed"), ("all", "Toutes", "All")];

/// Key of the draft of the comment on the whole pull request, and of the one on a line.
const DRAFT_PR: &str = "pr";
const DRAFT_LINE: &str = "line";
/// Key of the kept remarks of the AI review, published together.
const DRAFT_AI: &str = "ai";
/// Keys of the title / description being changed, and of the comment being changed.
const DRAFT_HEAD: &str = "head";
const DRAFT_EDIT: &str = "edit";

/// `gh pr merge` methods and their labels.
const MERGE_METHODS: [(&str, &str, &str); 3] = [("merge", "Commit de fusion", "Merge commit"), ("squash", "Squash", "Squash"), ("rebase", "Rebase", "Rebase")];

#[derive(Default, Clone, Copy, PartialEq)]
pub(super) enum PrSection {
    #[default]
    Conversation,
    Commits,
    Files,
}

/// A diff shown in the view, with its folded files and where to scroll.
#[derive(Default)]
struct PrDiff {
    diff: Option<HistoryDiff>,
    loading: bool,
    collapsed: HashSet<usize>,
    jump: Option<history::FileJump>,
}

impl PrDiff {
    fn set(&mut self, text: String) {
        let text = if text.trim().is_empty() { t!("(aucune différence)", "(no difference)").to_string() } else { text };
        let lines = history::parse_diff(&text);
        let files = history::diff_files(&lines);
        self.loading = false;
        self.diff = Some(HistoryDiff { hash: String::new(), path: None, text, lines, files });
    }
}

#[derive(Default)]
pub(super) struct PrUi {
    /// Repository of the list: the current one at first, another one chosen in the list.
    pub(super) repo: Option<PathBuf>,
    /// Branch checked out in `repo` (from the git status of all the repositories).
    pub(super) branch: Option<String>,
    list: Option<Result<Vec<github::PrSummary>, String>>,
    loading: bool,
    state: usize,
    /// Index in `github::WHO`: all, mine, to review…
    who: usize,
    filter: String,
    selected: Option<u64>,
    detail: Option<Result<github::PrDetail, String>>,
    detail_loading: bool,
    section: PrSection,
    show_resolved: bool,
    /// The whole pull request ("Fichiers").
    files: PrDiff,
    /// The commit chosen in "Commits" and its own changes.
    commit: Option<String>,
    commit_diff: PrDiff,
    /// Comments being written: on the pull request, on a line, or replies (key: thread id).
    drafts: HashMap<String, String>,
    /// Line of the diff being commented: (file, line, "RIGHT" new / "LEFT" old, index in the diff).
    new_line: Option<(String, usize, &'static str, usize)>,
    /// Comments whose suggestion was applied.
    applied: HashSet<String>,
    /// Lines added (or removed) by the suggestions applied, per file: (last line replaced, delta),
    /// to find the lines of the next ones.
    shifts: HashMap<String, Vec<(usize, isize)>>,
    /// Comments on lines shown in the editor (when the branch of the pull request is checked out):
    /// file → (line, text).
    pub(super) notes: HashMap<PathBuf, Vec<(usize, String)>>,
    busy: bool,
    /// AI review of the selected pull request: remarks proposed, published only when asked.
    pub(super) ai: ai_review::AiReview,
    /// Title, description and base being changed (✏ of the header).
    head_edit: Option<(String, String, String)>,
    /// Comment being changed: its id and its new text.
    editing: Option<(String, String)>,
    /// Labels and milestones of the repository, read once.
    choices: Option<(Vec<github::Label>, Vec<String>)>,
    choices_asked: bool,
    /// Logins being typed to add a reviewer, an assignee.
    new_reviewer: String,
    new_assignee: String,
    /// Index in `MERGE_METHODS`, and the branch deleted once merged.
    merge_method: usize,
    delete_branch: bool,
    /// All the checks shown (else only those that fail).
    all_checks: bool,
}

impl PrUi {
    /// The state and the resolved conversations of the settings: at the start, and when they change.
    pub(super) fn apply_settings(&mut self, state: &str, show_resolved: bool) {
        self.state = STATES.iter().position(|(k, _, _)| *k == state).unwrap_or(0);
        self.show_resolved = show_resolved;
    }
}

/// What the pull request view asked for during the frame.
enum PrAct {
    Reload,
    /// `gh auth login` typed in a terminal.
    Login,
    Repo(PathBuf),
    Select(u64),
    Checkout(u64),
    Web(u64),
    Commit(String),
    Open(String, usize),
    Apply { id: String, path: String, start: usize, end: usize, text: String },
    /// Send the draft of this key.
    Send(String),
    Resolve(String, bool),
    /// Ask Claude for a review of the pull request.
    AiReview,
    /// Publish the kept remarks of the AI review.
    AiPublish,
    Ai(ai_review::NoteAct),
    /// `gh` commands on the pull request, then read again; `draft`: the draft they send.
    Gh { label: String, cmds: Vec<Vec<String>>, draft: Option<String> },
    /// Same, asked first: it cannot be undone.
    Ask { title: String, msg: String, yes: String, label: String, cmds: Vec<Vec<String>> },
    React { id: String, content: String, add: bool },
    /// Save the text of the comment being changed.
    SaveComment,
    DeleteComment(String),
    /// Save the title, description and base being changed.
    SaveHead,
    Viewed(String, bool),
    /// The log of a GitHub Actions job (name of the check, job).
    Log(String, u64),
    /// Something copied, said in the status bar.
    Copied(String),
    /// The files of the pull request opened together, in a group of tabs.
    OpenGroup,
}

impl App {
    /// The "Pull requests" tab, loaded for the selected repository.
    pub(super) fn open_pulls(&mut self) {
        if self.bottom == Some(BottomTab::Pulls) {
            self.bottom = None;
        }
        self.open_view_tab(document::View::PullRequests, "Pull requests".into());
        self.pulls_load();
    }

    /// The pull requests read, wherever the view is shown.
    pub(super) fn pulls_load(&mut self) {
        // the current repository at first (else the first one of the folder); the one chosen in the view stays
        let repo = self.pr.repo.clone().filter(|r| self.repos.contains(r)).or_else(|| self.repo.clone()).or_else(|| self.repos.first().cloned());
        if self.pr.list.is_none() || self.pr.repo != repo {
            self.load_prs(repo);
        }
    }

    /// The list read again with the settings just changed (only if it was read before).
    pub(super) fn reload_prs(&mut self) {
        if self.pr.list.is_some() {
            if let Some(repo) = self.pr.repo.clone() {
                self.load_prs(Some(repo));
            }
        }
    }

    fn load_prs(&mut self, repo: Option<PathBuf>) {
        let Some(repo) = repo else {
            self.pr.list = Some(Err(t!("Aucun dépôt git dans ce dossier.", "No git repository in this folder.").into()));
            return;
        };
        if self.pr.repo.as_ref() != Some(&repo) {
            self.pr.selected = None;
            self.pr.detail = None;
            self.pr.files = PrDiff::default();
            self.pr.list = None;
            self.pr.notes.clear();
            self.pr.branch = None;
            self.pr.ai = Default::default();
            self.pr.choices = None;
            self.pr.choices_asked = false;
            // its branch comes with the status of the repositories
            self.git_refresh();
        }
        self.pr.repo = Some(repo.clone());
        self.pr.loading = true;
        let (state, who) = (STATES[self.pr.state].0, github::WHO[self.pr.who].0);
        self.jobs.spawn(move || JobResult::PrList(repo.clone(), github::list(&repo, state, who)));
    }

    pub(super) fn load_pr(&mut self, n: u64) {
        let Some(repo) = self.pr.repo.clone() else { return };
        if self.pr.selected != Some(n) {
            self.pr.files = PrDiff::default();
            self.pr.commit = None;
            self.pr.commit_diff = PrDiff::default();
            self.pr.new_line = None;
            self.pr.applied.clear();
            self.pr.shifts.clear();
            self.pr.ai = Default::default();
            self.pr.head_edit = None;
            self.pr.editing = None;
        }
        self.pr.selected = Some(n);
        self.pr.detail_loading = true;
        if !self.pr.choices_asked {
            self.pr.choices_asked = true;
            let repo = repo.clone();
            self.jobs.spawn(move || JobResult::PrChoices(repo.clone(), github::choices(&repo)));
        }
        self.jobs.spawn(move || JobResult::PrDetail(repo.clone(), n, github::detail(&repo, n)));
    }

    pub(super) fn pr_choices_loaded(&mut self, repo: PathBuf, r: Result<(Vec<github::Label>, Vec<String>), String>) {
        if self.pr.repo.as_ref() == Some(&repo) {
            self.pr.choices = Some(r.unwrap_or_else(|e| {
                self.log(e);
                Default::default()
            }));
        }
    }

    pub(super) fn pr_log_loaded(&mut self, name: String, r: Result<String, String>) {
        match r {
            Ok(text) => self.new_scratch(tf!("Log : {name}", "Log: {name}"), text, "Plain Text", true),
            Err(e) => {
                self.log(e.clone());
                self.set_error(tf!("Log de {name} : échec — {}", "Log of {name}: failed — {}", e.lines().next().unwrap_or("")));
            }
        }
    }

    pub(super) fn pr_list_loaded(&mut self, repo: PathBuf, r: Result<Vec<github::PrSummary>, String>) {
        if self.pr.repo.as_ref() == Some(&repo) {
            self.pr.loading = false;
            self.pr.list = Some(r);
        }
    }

    pub(super) fn pr_detail_loaded(&mut self, repo: PathBuf, n: u64, r: Result<github::PrDetail, String>) {
        if self.pr.repo.as_ref() != Some(&repo) || self.pr.selected != Some(n) {
            return;
        }
        self.pr.detail_loading = false;
        if let Ok(d) = &r {
            let fresh = self.pr.files.diff.is_none();
            self.pr.files.set(d.diff.clone());
            // the files viewed folded, like on GitHub
            if let (true, Some(diff)) = (fresh, &self.pr.files.diff) {
                self.pr.files.collapsed.extend(diff.files.iter().filter(|f| d.viewed.contains(&f.path)).map(|f| f.start));
            }
            // the list follows what changed (merged, closed, ready, approved…)
            if let Some(Ok(list)) = &mut self.pr.list {
                if let Some(s) = list.iter_mut().find(|s| s.number == d.number) {
                    (s.state, s.is_draft, s.title) = (d.state.clone(), d.is_draft, d.title.clone());
                    s.review_decision = Some(d.review_decision.clone()).filter(|x| !x.is_empty());
                }
            }
        }
        self.pr.detail = Some(r);
        self.pr_update_notes();
    }

    pub(super) fn pr_commit_diff_loaded(&mut self, sha: String, r: Result<String, String>) {
        if self.pr.commit.as_deref() == Some(sha.as_str()) {
            self.pr.commit_diff.set(r.unwrap_or_else(|e| e));
        }
    }

    /// End of a `gh` command; `reload`: the pull request to read again (a comment was sent),
    /// `draft`: the comment sent, cleared when it worked.
    pub(super) fn pr_op_done(&mut self, label: String, r: Result<String, String>, reload: Option<u64>, draft: Option<String>) {
        self.pr.busy = false;
        match r {
            Ok(out) => {
                if !out.is_empty() && reload.is_none() {
                    self.log(out);
                }
                self.set_status(tf!("{label} : OK", "{label}: OK"));
                if let Some(k) = draft {
                    self.pr.drafts.remove(&k);
                    if k == DRAFT_LINE {
                        self.pr.new_line = None;
                    }
                    if k == DRAFT_AI {
                        self.pr.ai.notes.retain(|n| !n.keep);
                    }
                    if k == DRAFT_HEAD {
                        self.pr.head_edit = None;
                    }
                    if k == DRAFT_EDIT {
                        self.pr.editing = None;
                    }
                }
                if let Some(n) = reload.filter(|n| self.pr.selected == Some(*n)) {
                    self.load_pr(n);
                }
            }
            Err(e) => {
                self.log(e.clone());
                self.set_error(tf!("{label} : échec — {}", "{label}: failed — {}", e.lines().next().unwrap_or("")));
            }
        }
        self.git_refresh();
    }

    /// The branch of the pull request is the one checked out in its repository.
    fn pr_checked_out(&self) -> bool {
        let Some(Ok(d)) = &self.pr.detail else { return false };
        self.pr.branch.as_deref() == Some(d.head.as_str())
    }

    /// Comments on lines shown in the editor, when the branch of the pull request is checked out.
    pub(super) fn pr_update_notes(&mut self) {
        let mut notes: HashMap<PathBuf, Vec<(usize, String)>> = HashMap::new();
        if let (true, Some(Ok(d)), Some(repo)) = (self.pr_checked_out(), &self.pr.detail, &self.pr.repo) {
            for t in d.threads.iter().filter(|t| !t.resolved) {
                let (Some(line), Some(first)) = (t.line, t.comments.first()) else { continue };
                let body: String = first.body.lines().take(6).collect::<Vec<_>>().join("\n");
                let more = match t.comments.len() {
                    1 => String::new(),
                    n => tf!("\n(+{} réponse(s))", "\n(+{} reply(ies))", n - 1),
                };
                notes.entry(repo.join(&t.path)).or_default().push((line - 1, tf!("💬 PR #{} · {} :\n{body}{more}", "💬 PR #{} · {}:\n{body}{more}", d.number, first.author)));
            }
        }
        self.ai_editor_notes(&mut notes);
        self.pr.notes = notes;
    }

    pub(super) fn pulls_ui(&mut self, ui: &mut egui::Ui) {
        let in_tab = self.docs.iter().any(|d| d.view == document::View::PullRequests);
        if self.install_guide(ui, crate::tools::Tool::Gh, in_tab) {
            return;
        }
        // gh just installed: the list again
        if matches!(&self.pr.list, Some(Err(e)) if *e == github::not_installed()) {
            self.pr.list = None;
            self.pulls_load();
        }
        let mut act: Option<PrAct> = None;
        let accent = self.hl.colors.accent;
        let checked_out = self.pr_checked_out();
        let repo_name = self.pr.repo.as_deref().map(|r| self.repo_label(r)).unwrap_or_default();
        let repo_choices: Vec<(PathBuf, String)> = self.repos.iter().map(|r| (r.clone(), self.repo_label(r))).collect();
        let fill = ui.visuals().panel_fill;
        // in the editor area when its tab exists (else in the bottom panel): ⇲ / ⇱ to move it
        let (big, mut move_to) = (self.docs.iter().any(|d| d.view == document::View::PullRequests), None);
        let p = &mut self.pr;
        let cache = &mut self.md_cache;

        // ------------------------------------------------ filters
        egui::TopBottomPanel::top("pr_bar")
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 8.0, bottom: 6.0 }))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    crate::view_bar::title(ui, "Pull requests");
                    let labels: Vec<String> = repo_choices.iter().map(|(_, l)| l.clone()).collect();
                    let current = repo_choices.iter().position(|(r, _)| p.repo.as_ref() == Some(r));
                    if let Some(k) = crate::view_bar::repo_chip(ui, "pr_repo", &repo_name, &labels, current).filter(|k| Some(*k) != current) {
                        act = Some(PrAct::Repo(repo_choices[k].0.clone()));
                    }
                    ui.add_space(8.0);
                    let before = (p.state, p.who);
                    egui::ComboBox::from_id_salt("pr_who").selected_text(t!(github::WHO[p.who].1, github::WHO[p.who].2)).show_ui(ui, |ui| {
                        for (k, (_, fr, en)) in github::WHO.iter().enumerate() {
                            ui.selectable_value(&mut p.who, k, t!(*fr, *en));
                        }
                    });
                    egui::ComboBox::from_id_salt("pr_state").selected_text(t!(STATES[p.state].1, STATES[p.state].2)).show_ui(ui, |ui| {
                        for (k, (_, fr, en)) in STATES.iter().enumerate() {
                            ui.selectable_value(&mut p.state, k, t!(*fr, *en));
                        }
                    });
                    if before != (p.state, p.who) {
                        act = Some(PrAct::Reload);
                    }
                    ui.add(egui::TextEdit::singleline(&mut p.filter).hint_text(t!("🔍 Filtrer : titre, auteur, branche, numéro…", "🔍 Filter: title, author, branch, number…")).desired_width(240.0));
                    if p.loading || p.busy {
                        spinner(ui);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match crate::view_bar::controls(ui, big, Some(t!("Recharger la liste", "Reload the list")), None) {
                            Some(crate::view_bar::BarAct::Move(b)) => move_to = Some(b),
                            Some(crate::view_bar::BarAct::Refresh) => act = Some(PrAct::Reload),
                            None => {}
                        }
                    });
                });
            });

        // one of the two can take all the room (the pull request only when one is shown)
        let view = egui::Id::new("pr_view");
        let mut maxed = maximize::maxed(ui.ctx(), view);
        let shown = matches!(p.detail, Some(Ok(_)));
        if maxed == Some("detail") && !shown && !p.detail_loading {
            maximize::restore(ui.ctx(), view);
            maxed = None;
        }

        // ------------------------------------------------ list
        let mut list_ui = |ui: &mut egui::Ui| {
            let list = match &p.list {
                None => {
                    spinner(ui);
                    return;
                }
                Some(Err(e)) => {
                    ui.add_space(8.0);
                    ui.label(RichText::new(e).color(err()));
                    if e == github::not_logged() && ui.button(t!("▶ gh auth login dans un terminal", "▶ gh auth login in a terminal")).clicked() {
                        act = Some(PrAct::Login);
                    }
                    return;
                }
                Some(Ok(l)) => l,
            };
            let f = p.filter.to_lowercase();
            let shown: Vec<&github::PrSummary> = list
                .iter()
                .filter(|pr| {
                    f.is_empty()
                        || pr.title.to_lowercase().contains(&f)
                        || pr.author.login.to_lowercase().contains(&f)
                        || pr.head_ref_name.to_lowercase().contains(&f)
                        || pr.number.to_string() == f.trim_start_matches('#')
                })
                .collect();
            if shown.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(t!("Aucune pull request", "No pull request")).weak());
                return;
            }
            egui::ScrollArea::vertical().id_salt("pr_list_scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for pr in shown {
                    let r = pr_row(ui, pr, p.selected == Some(pr.number), accent);
                    if r.clicked() {
                        act = Some(PrAct::Select(pr.number));
                    }
                    r.context_menu(|ui| quick_menu(ui, pr.number, &pr.url, true, &mut act));
                }
            });
        };
        match maxed {
            Some("list") => {
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    list_ui(ui);
                    maximize::corner(ui, view, "list");
                });
            }
            Some("detail") => {}
            _ => {
                egui::SidePanel::left("pr_list").resizable(true).default_width(380.0).width_range(240.0..=700.0).show_inside(ui, |ui| {
                    list_ui(ui);
                    if shown {
                        maximize::corner(ui, view, "list");
                    }
                });
            }
        }

        // ------------------------------------------------ the selected pull request
        if maxed != Some("list") {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                let d = match &p.detail {
                    None if p.detail_loading => {
                        spinner(ui);
                        return;
                    }
                    None => {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| ui.label(RichText::new(t!("Choisis une pull request à gauche", "Choose a pull request on the left")).weak()));
                        return;
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).color(err()));
                        return;
                    }
                    Some(Ok(d)) => d,
                };
                // header
                let mut cancel_edit = false;
                if let Some((title, body, base)) = &mut p.head_edit {
                    ui.add(egui::TextEdit::singleline(title).font(egui::FontId::proportional(16.0)).desired_width(f32::INFINITY));
                    ui.horizontal(|ui| {
                        ui.label(t!("Branche de base :", "Base branch:"));
                        ui.add(egui::TextEdit::singleline(base).desired_width(180.0));
                    });
                    ui.add(egui::TextEdit::multiline(body).hint_text(t!("Description (Markdown)", "Description (Markdown)")).desired_rows(6).desired_width(f32::INFINITY));
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!p.busy && !title.trim().is_empty(), filled_button(t!("Enregistrer", "Save"), accent)).clicked() {
                            act = Some(PrAct::SaveHead);
                        }
                        cancel_edit = ui.button(t!("Annuler", "Cancel")).clicked();
                    });
                    ui.add_space(4.0);
                } else {
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(RichText::new(&d.title).size(17.0).strong()).truncate());
                        ui.label(RichText::new(format!("#{}", d.number)).size(17.0).weak());
                        if ui.small_button("✏").on_hover_text(t!("Modifier le titre, la description ou la branche de base", "Edit the title, the description or the base branch")).clicked() {
                            p.head_edit = Some((d.title.clone(), d.body.clone(), d.base.clone()));
                        }
                        if p.detail_loading {
                            spinner(ui);
                        }
                    });
                }
                if cancel_edit {
                    p.head_edit = None;
                }
                ui.horizontal(|ui| {
                    let (label, color) = state_badge(&d.state, d.is_draft);
                    badge(ui, label, color);
                    ui.label(RichText::new(&d.author).strong());
                    ui.label(RichText::new(t!("veut fusionner", "wants to merge")).weak());
                    ui.label(RichText::new(&d.head).monospace().color(accent));
                    ui.label(RichText::new(t!("dans", "into")).weak());
                    ui.label(RichText::new(&d.base).monospace().color(accent));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        maximize::button(ui, view, "detail");
                        ui.menu_button("⋯", |ui| quick_menu(ui, d.number, &d.url, false, &mut act));
                        if ui
                            .button(t!("📑 Ouvrir les fichiers", "📑 Open the files"))
                            .on_hover_text(t!("Ouvre les fichiers de la PR dans un groupe d'onglets « PR #… » (clic sur son nom pour le replier). Fais le Checkout d'abord pour avoir leur version de la PR.", "Opens the files of the PR in a group of tabs “PR #…” (click its name to fold it). Checkout first to get their version of the PR."))
                            .clicked()
                        {
                            act = Some(PrAct::OpenGroup);
                        }
                        if ui.button("⟳").on_hover_text(t!("Recharger la pull request", "Reload the pull request")).clicked() {
                            act = Some(PrAct::Select(d.number));
                        }
                        if ui.button(t!("Ouvrir sur GitHub", "Open on GitHub")).clicked() {
                            act = Some(PrAct::Web(d.number));
                        }
                        if crate::features::on(crate::features::Feature::Claude)
                            && ui
                            .add_enabled(!p.ai.loading, egui::Button::new(t!("✳ Revue IA", "✳ AI review")))
                            .on_hover_text(t!("Claude relit la PR et propose des remarques sous les lignes : tu gardes, modifies ou écartes chacune, rien n'est publié sans toi", "Claude reads over the PR and suggests remarks under the lines: you keep, edit or drop each one, nothing is published without you"))
                            .clicked()
                        {
                            act = Some(PrAct::AiReview);
                            p.section = PrSection::Files;
                        }
                        if checked_out {
                            ui.label(RichText::new(t!("✔ branche active", "✔ current branch")).color(ok())).on_hover_text(t!("Les commentaires sur les lignes sont aussi dans l'éditeur", "The comments on the lines are in the editor too"));
                        } else if ui
                            .add_enabled(!p.busy, filled_button("⤵ Checkout", accent))
                            .on_hover_text(t!("gh pr checkout : prendre la branche de la PR pour la modifier, appliquer les suggestions et voir les commentaires dans l'éditeur", "gh pr checkout: take the branch of the PR to change it, apply the suggestions and see the comments in the editor"))
                            .clicked()
                        {
                            act = Some(PrAct::Checkout(d.number));
                        }
                    });
                });
                ui.add_space(4.0);
                let open_threads = d.threads.iter().filter(|t| !t.resolved).count();
                ui.horizontal(|ui| {
                    let conv = if open_threads > 0 { tf!("Conversation · {open_threads} à traiter", "Conversation · {open_threads} to handle") } else { "Conversation".into() };
                    ui.selectable_value(&mut p.section, PrSection::Conversation, conv);
                    ui.selectable_value(&mut p.section, PrSection::Commits, format!("Commits · {}", d.commits.len()));
                    let n = p.files.diff.as_ref().map_or(0, |x| x.files.len());
                    ui.selectable_value(&mut p.section, PrSection::Files, tf!("Fichiers · {n}", "Files · {n}"));
                });
                ui.separator();
                match p.section {
                    PrSection::Conversation => {
                        // reviewers, assignees, labels, milestone on the right, like on GitHub
                        egui::SidePanel::right("pr_side").resizable(true).default_width(230.0).width_range(170.0..=420.0).show_inside(ui, |ui| {
                            egui::ScrollArea::vertical().id_salt("pr_side_scroll").auto_shrink([false, false]).show(ui, |ui| {
                                side_info(ui, d, &mut p.new_reviewer, &mut p.new_assignee, p.choices.as_ref(), p.busy, &mut act);
                            });
                        });
                        // like the chatter of Odoo: write on top, then everything that happened, newest first
                        egui::ScrollArea::vertical().id_salt("pr_conv").auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_max_width(ui.available_width().min(1000.0));
                            ui.add_space(4.0);
                            merge_box(ui, d, &mut p.merge_method, &mut p.delete_branch, &mut p.all_checks, p.busy, accent, &mut act);
                            let draft = p.drafts.entry(DRAFT_PR.into()).or_default();
                            if composer(ui, draft, t!("Envoyer un message sur la pull request…", "Send a message on the pull request…"), t!("Envoyer", "Send"), p.busy, accent) {
                                act = Some(PrAct::Send(DRAFT_PR.into()));
                            }
                            // a review: GitHub does not let one approve their own pull request
                            if d.state == "OPEN" && !d.viewer.is_empty() && d.viewer != d.author {
                                ui.horizontal(|ui| {
                                    let text = draft.trim().to_string();
                                    let review = |event: &str, label: &str| PrAct::Gh { label: label.into(), cmds: vec![github::review_args(d.number, event, &text)], draft: Some(DRAFT_PR.into()) };
                                    if ui
                                        .add_enabled(!p.busy, egui::Button::new(RichText::new(t!("✔ Approuver", "✔ Approve")).color(ok())))
                                        .on_hover_text(t!("Approuver la PR, avec le message écrit au-dessus s'il y en a un", "Approve the PR, with the message written above if there is one"))
                                        .clicked()
                                    {
                                        act = Some(review("approve", t!("Approbation", "Approval")));
                                    }
                                    if ui
                                        .add_enabled(!p.busy && !text.is_empty(), egui::Button::new(RichText::new(t!("✖ Demander des changements", "✖ Request changes")).color(err())))
                                        .on_hover_text(t!("Le message écrit au-dessus dit ce qu'il faut changer", "The message written above says what to change"))
                                        .on_disabled_hover_text(t!("Écris d'abord au-dessus ce qu'il faut changer", "First write above what to change"))
                                        .clicked()
                                    {
                                        act = Some(review("request-changes", t!("Changements demandés", "Changes requested")));
                                    }
                                });
                            }
                            let resolved = d.threads.len() - open_threads;
                            if resolved > 0 {
                                ui.checkbox(&mut p.show_resolved, RichText::new(tf!("afficher les {resolved} conversation(s) résolue(s)", "show the {resolved} resolved conversation(s)")).small());
                            }
                            let mut day_shown = String::new();
                            for (date, ev) in timeline(d, p.show_resolved) {
                                if day(&date) != day_shown {
                                    day_shown = day(&date).to_string();
                                    day_separator(ui, &date);
                                }
                                match ev {
                                    Event::Opened => {
                                        comment_block(ui, &d.author, &date, Some((t!("a ouvert la pull request", "opened the pull request"), accent)), |ui| {
                                            if d.body.trim().is_empty() {
                                                ui.label(RichText::new(t!("Pas de description.", "No description.")).weak().italics());
                                            } else {
                                                egui_commonmark::CommonMarkViewer::new().show(ui, cache, &d.body);
                                            }
                                            ui.horizontal_wrapped(|ui| reactions_row(ui, &d.id, &d.reactions, p.busy, &mut act));
                                        });
                                    }
                                    Event::Comment(k) => {
                                        let c = &d.comments[k];
                                        let review = match c.state.as_str() {
                                            "APPROVED" => Some((t!("✔ a approuvé", "✔ approved"), ok())),
                                            "CHANGES_REQUESTED" => Some((t!("✖ demande des changements", "✖ requested changes"), err())),
                                            _ => None,
                                        };
                                        if c.body.trim().is_empty() {
                                            let (t, color) = review.unwrap_or((t!("a fait une revue", "reviewed"), accent));
                                            tracking(ui, &c.author.login, &date, t, color);
                                        } else {
                                            comment_block(ui, &c.author.login, &date, review, |ui| {
                                                let tools = CommentTools { id: &c.id, body: &c.body, mine: c.mine, deletable: !c.is_review(), reactions: &c.reactions, busy: p.busy, accent };
                                                comment_body(ui, cache, tools, &mut p.editing, &mut act, |ui, cache, _| {
                                                    egui_commonmark::CommonMarkViewer::new().show(ui, cache, &c.body);
                                                });
                                            });
                                        }
                                    }
                                    Event::Thread(k) => {
                                        let t = &d.threads[k];
                                        let who = t.comments.first().map_or("", |c| c.author.as_str());
                                        tracking(ui, who, &date, &tf!("a commenté {}", "commented {}", t.path.rsplit('/').next().unwrap_or(&t.path)), accent);
                                        ui.horizontal_top(|ui| {
                                            ui.add_space(34.0);
                                            ui.vertical(|ui| {
                                                let draft = p.drafts.entry(t.id.clone()).or_default();
                                                thread_ui(ui, t, cache, &p.applied, &p.shifts, checked_out, p.busy, draft, &mut p.editing, accent, false, &mut act);
                                            });
                                        });
                                    }
                                    Event::Push { author, commits } => {
                                        let what = if commits.len() == 1 { t!("a poussé un commit", "pushed a commit").to_string() } else { tf!("a poussé {} commits", "pushed {} commits", commits.len()) };
                                        tracking(ui, &author, &date, &what, accent);
                                        for &k in &commits {
                                            let c = &d.commits[k];
                                            ui.horizontal(|ui| {
                                                ui.add_space(34.0);
                                                if ui.link(RichText::new(&c.oid[..c.oid.len().min(8)]).monospace().small().color(accent)).on_hover_text(t!("Voir ce commit", "Show this commit")).clicked() {
                                                    act = Some(PrAct::Commit(c.oid.clone()));
                                                    p.section = PrSection::Commits;
                                                }
                                                ui.label(RichText::new(&c.message_headline).small());
                                            });
                                        }
                                    }
                                }
                            }
                            ui.add_space(20.0);
                        });
                    }
                    PrSection::Commits => {
                        // the commits on top, the changes of the one chosen below
                        let avail = ui.available_height();
                        egui::TopBottomPanel::top("pr_commit_list")
                            .resizable(true)
                            .default_height((avail * 0.3).max(90.0))
                            .height_range(60.0..=(avail - 120.0).max(80.0))
                            .show_inside(ui, |ui| {
                                egui::ScrollArea::vertical().id_salt("pr_commits").auto_shrink([false, false]).show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for c in d.commits.iter().rev() {
                                        if commit_row(ui, c, p.commit.as_deref() == Some(c.oid.as_str()), accent).clicked() {
                                            act = Some(PrAct::Commit(c.oid.clone()));
                                        }
                                    }
                                });
                            });
                        egui::CentralPanel::default().frame(egui::Frame::none().inner_margin(egui::Margin { top: 6.0, ..Default::default() })).show_inside(ui, |ui| {
                            let Some(sha) = &p.commit else {
                                ui.add_space(20.0);
                                ui.vertical_centered(|ui| ui.label(RichText::new(t!("Choisis un commit : ses changements à lui seul s'affichent ici", "Choose a commit: its changes alone show here")).weak()));
                                return;
                            };
                            let headline = d.commits.iter().find(|c| &c.oid == sha).map_or("", |c| c.message_headline.as_str());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&sha[..sha.len().min(8)]).monospace().color(accent));
                                ui.label(RichText::new(headline).strong());
                            });
                            if let Some(DiffClick::File(i, _) | DiffClick::Line(i) | DiffClick::Hunk(i, _)) = diff_area(ui, "pr_commit", &mut p.commit_diff, &d.threads, None, None, &mut act) {
                                if let Some(diff) = &p.commit_diff.diff {
                                    if let Some(path) = history::file_at(&diff.text, i) {
                                        act = Some(PrAct::Open(path, history::target_line(&diff.lines, i)));
                                    }
                                }
                            }
                        });
                    }
                    PrSection::Files => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(t!("Double-clic sur une ligne pour la commenter", "Double-click a line to comment on it")).small().weak());
                            if let Some(diff) = &p.files.diff {
                                let seen = diff.files.iter().filter(|f| d.viewed.contains(&f.path)).count();
                                ui.label(RichText::new(tf!("· {seen}/{} vu(s)", "· {seen}/{} viewed", diff.files.len())).small().weak())
                                    .on_hover_text(t!("Coche un fichier à gauche quand tu l'as relu : il se replie et GitHub s'en souvient", "Tick a file on the left once you read it: it folds and GitHub remembers it"));
                            }
                            let resolved = d.threads.iter().filter(|t| t.resolved).count();
                            if resolved > 0 {
                                ui.checkbox(&mut p.show_resolved, RichText::new(tf!("afficher les {resolved} conversation(s) résolue(s)", "show the {resolved} resolved conversation(s)")).small());
                            }
                        });
                        // the AI review: its state and buttons, the remarks off the diff
                        let ai_rows = p.ai.rows(p.files.diff.as_ref());
                        let mut ai_act = None;
                        let (busy, mut publish) = (p.busy, false);
                        ai_review::review_bar(ui, cache, &mut p.ai, &ai_rows, accent, &mut ai_act, |ui, kept| {
                            let label = tf!("Publier les {kept} gardée(s) sur GitHub", "Publish the {kept} kept on GitHub");
                            let tip = t!("Une seule revue GitHub (commentaire, sans approuver ni demander de changements) ; les remarques hors du diff vont dans son message", "A single GitHub review (comment, without approving or requesting changes); the remarks outside the diff go in its message");
                            publish = ui.add_enabled(kept > 0 && !busy, filled_button(label, accent)).on_hover_text(tip).clicked();
                        });
                        // the conversations under the lines they are about, like on GitHub
                        let mut at: HashMap<usize, Vec<usize>> = HashMap::new();
                        if let Some(diff) = &p.files.diff {
                            for (k, t) in d.threads.iter().enumerate().filter(|(_, t)| p.show_resolved || !t.resolved) {
                                if let Some(i) = thread_row(diff, t) {
                                    at.entry(i).or_default().push(k);
                                }
                            }
                        }
                        let mut ai_at: HashMap<usize, Vec<usize>> = HashMap::new();
                        for (k, row) in ai_rows.iter().enumerate() {
                            if let Some(i) = row {
                                ai_at.entry(*i).or_default().push(k);
                            }
                        }
                        let new_at = p.new_line.as_ref().map(|n| n.3);
                        let lines: HashSet<usize> = at.keys().chain(ai_at.keys()).copied().chain(new_at).collect();
                        let (drafts, applied, shifts, new_line, ai_notes, editing) = (&mut p.drafts, &p.applied, &p.shifts, &mut p.new_line, &mut p.ai.notes, &mut p.editing);
                        let mut line_act = None;
                        let mut show = |ui: &mut egui::Ui, i: usize| {
                            for &k in at.get(&i).map_or(&[][..], |v| &v[..]) {
                                let t = &d.threads[k];
                                let draft = drafts.entry(t.id.clone()).or_default();
                                thread_ui(ui, t, cache, applied, shifts, checked_out, busy, draft, editing, accent, true, &mut line_act);
                            }
                            for &k in ai_at.get(&i).map_or(&[][..], |v| &v[..]) {
                                ai_review::note_card(ui, cache, k, &mut ai_notes[k], true, accent, &mut ai_act);
                            }
                            if new_at == Some(i) {
                                line_composer(ui, drafts.entry(DRAFT_LINE.into()).or_default(), new_line, busy, accent, &mut line_act);
                            }
                        };
                        let notes = history::Notes { at: &lines, show: &mut show };
                        let clicked = diff_area(ui, "pr_files", &mut p.files, &d.threads, Some(notes), Some(&d.viewed), &mut act);
                        if line_act.is_some() {
                            act = line_act;
                        }
                        match clicked {
                            Some(DiffClick::Line(i)) => {
                                if let Some(diff) = &p.files.diff {
                                    if let (Some(path), Some(l)) = (history::file_at(&diff.text, i), diff.lines.get(i)) {
                                        let at = match l.kind {
                                            DiffKind::Removed => l.old.map(|n| (n, "LEFT")),
                                            DiffKind::Hunk => None,
                                            _ => l.new.map(|n| (n, "RIGHT")),
                                        };
                                        if let Some((n, side)) = at {
                                            p.new_line = Some((path, n, side, i));
                                        }
                                    }
                                }
                            }
                            Some(DiffClick::File(i, _) | DiffClick::Hunk(i, _)) => {
                                if let Some(diff) = &p.files.diff {
                                    if let Some(path) = history::file_at(&diff.text, i) {
                                        act = Some(PrAct::Open(path, history::target_line(&diff.lines, i)));
                                    }
                                }
                            }
                            None => {}
                        }
                        if publish {
                            act = Some(PrAct::AiPublish);
                        }
                        if let Some(a) = ai_act {
                            act = Some(PrAct::Ai(a));
                        }
                    }
                }
            });
        }

        match move_to {
            Some(true) => self.show_big(BottomTab::Pulls),
            Some(false) => self.show_bottom(BottomTab::Pulls, false),
            None => {}
        }
        // a repository chosen: also when none could be read yet
        if let Some(PrAct::Repo(r)) = act {
            self.load_prs(Some(r));
            return;
        }
        let Some(repo) = self.pr.repo.clone() else {
            if matches!(act, Some(PrAct::Reload)) {
                self.pulls_load();
            }
            return;
        };
        let n = self.pr.selected.unwrap_or(0);
        match act {
            Some(PrAct::Reload) => self.load_prs(Some(repo)),
            Some(PrAct::Login) => self.install_type("gh auth login", big),
            // done above
            Some(PrAct::Repo(_)) => {}
            Some(PrAct::Select(n)) => self.load_pr(n),
            Some(PrAct::Checkout(n)) => {
                self.set_status(format!("gh pr checkout {n}…"));
                self.pr_job(tf!("Checkout de la PR #{n}", "Checkout of PR #{n}"), None, None, move || github::checkout(&repo, n));
            }
            Some(PrAct::Web(n)) => self.pr_job(t!("Ouvrir sur GitHub", "Open on GitHub").into(), None, None, move || github::open_web(&repo, n)),
            Some(PrAct::Commit(sha)) => {
                self.pr.commit = Some(sha.clone());
                self.pr.commit_diff = PrDiff { loading: true, ..Default::default() };
                self.jobs.spawn(move || JobResult::PrCommitDiff(sha.clone(), github::commit_diff(&repo, &sha)));
            }
            Some(PrAct::Open(path, line)) => self.pr_open_file(&path, line),
            Some(PrAct::Apply { id, path, start, end, text }) => self.apply_suggestion(id, &path, start, end, &text),
            Some(PrAct::Send(key)) => {
                let body = self.pr.drafts.get(&key).map(|s| s.trim().to_string()).unwrap_or_default();
                let Some(Ok(d)) = &self.pr.detail else { return };
                if body.is_empty() {
                    return;
                }
                if key == DRAFT_PR {
                    self.pr_job(t!("Commentaire", "Comment").into(), Some(n), Some(key), move || github::comment(&repo, n, &body));
                } else if key == DRAFT_LINE {
                    let Some((path, line, side, _)) = self.pr.new_line.clone() else { return };
                    let commit = d.head_oid.clone();
                    self.pr_job(t!("Commentaire sur la ligne", "Comment on the line").into(), Some(n), Some(key), move || github::line_comment(&repo, n, &commit, &path, line, side, &body));
                } else if let Some(id) = d.threads.iter().find(|t| t.id == key).and_then(|t| t.comments.first()).map(|c| c.db_id) {
                    self.pr_job(t!("Réponse", "Reply").into(), Some(n), Some(key), move || github::reply(&repo, n, id, &body));
                }
            }
            Some(PrAct::Resolve(id, yes)) => {
                let label = if yes { t!("Conversation résolue", "Conversation resolved") } else { t!("Conversation rouverte", "Conversation reopened") };
                self.pr_job(label.into(), Some(n), None, move || github::resolve(&repo, &id, yes));
            }
            Some(PrAct::AiReview) => {
                if let Some(Ok(d)) = self.pr.detail.clone() {
                    self.ai_review_pr(repo, &d);
                }
            }
            Some(PrAct::AiPublish) => {
                let Some(Ok(d)) = &self.pr.detail else { return };
                let commit = d.head_oid.clone();
                // on a line of the diff: a comment there; else in the message of the review
                let rows = self.pr.ai.rows(self.pr.files.diff.as_ref());
                let (mut on_lines, mut general) = (Vec::new(), Vec::new());
                for (a, row) in self.pr.ai.notes.iter().zip(&rows).filter(|(a, _)| a.keep) {
                    let (note, body) = (&a.note, a.note.body.trim().to_string());
                    if row.is_some() {
                        on_lines.push((note.path.clone(), note.line, note.side.clone(), body));
                    } else if note.path.is_empty() {
                        general.push(body);
                    } else if note.line > 0 {
                        general.push(format!("**`{}:{}`** — {body}", note.path, note.line));
                    } else {
                        general.push(format!("**`{}`** — {body}", note.path));
                    }
                }
                let count = on_lines.len() + general.len();
                let body = general.join("\n\n");
                self.pr_job(tf!("Revue publiée ({count} remarque(s))", "Review published ({count} remark(s))"), Some(n), Some(DRAFT_AI.into()), move || github::review(&repo, n, &commit, &body, &on_lines));
            }
            Some(PrAct::Ai(a)) => {
                if let Some((path, line)) = self.pr.ai.apply(a) {
                    self.pr_open_file(&path, line);
                }
            }
            Some(PrAct::Gh { label, cmds, draft }) => self.pr_job(label, Some(n), draft, move || github::run_all(&repo, &cmds)),
            Some(PrAct::Ask { title, msg, yes, label, cmds }) => self.confirm = Some(Confirm::Gh { title, msg, yes, label, repo, cmds, n }),
            Some(PrAct::React { id, content, add }) => {
                // shown at once, GitHub follows
                if let Some(Ok(d)) = &mut self.pr.detail {
                    if let Some(r) = d.reactions_mut(&id) {
                        github::toggle_reaction(r, &content, add);
                    }
                }
                self.pr_job(t!("Réaction", "Reaction").into(), None, None, move || github::react(&repo, &id, &content, add));
            }
            Some(PrAct::SaveComment) => {
                let Some((id, body)) = self.pr.editing.clone() else { return };
                self.pr_job(t!("Commentaire modifié", "Comment edited").into(), Some(n), Some(DRAFT_EDIT.into()), move || github::edit_comment(&repo, &id, body.trim()));
            }
            Some(PrAct::DeleteComment(id)) => {
                self.confirm = Some(Confirm::Gh {
                    title: t!("Supprimer le commentaire", "Delete the comment").into(),
                    msg: t!("Supprimer ce commentaire sur GitHub ? Il ne pourra pas être récupéré.", "Delete this comment on GitHub? It cannot be brought back.").into(),
                    yes: t!("Supprimer", "Delete").into(),
                    label: t!("Commentaire supprimé", "Comment deleted").into(),
                    repo,
                    cmds: vec![github::delete_comment_args(&id)],
                    n,
                });
            }
            Some(PrAct::SaveHead) => {
                let (Some(Ok(d)), Some((title, body, base))) = (&self.pr.detail, &self.pr.head_edit) else { return };
                let mut more: Vec<&str> = Vec::new();
                let (title, base) = (title.trim(), base.trim());
                if title != d.title {
                    more.extend(["--title", title]);
                }
                if *body != d.body {
                    more.extend(["--body", body]);
                }
                if !base.is_empty() && base != d.base {
                    more.extend(["--base", base]);
                }
                if more.is_empty() {
                    self.pr.head_edit = None;
                    return;
                }
                let cmds = vec![github::pr_args("edit", n, &more)];
                self.pr_job(t!("Pull request modifiée", "Pull request edited").into(), Some(n), Some(DRAFT_HEAD.into()), move || github::run_all(&repo, &cmds));
            }
            Some(PrAct::Viewed(path, yes)) => {
                let Some(Ok(d)) = &mut self.pr.detail else { return };
                if yes {
                    d.viewed.insert(path.clone());
                } else {
                    d.viewed.remove(&path);
                }
                let id = d.id.clone();
                self.pr_job(t!("Fichier vu", "File viewed").into(), None, None, move || github::set_viewed(&repo, &id, &path, yes));
            }
            Some(PrAct::Log(name, job)) => {
                self.set_status(tf!("Log de {name}…", "Log of {name}…"));
                self.jobs.spawn(move || JobResult::PrLog(name, github::job_log(&repo, job)));
            }
            Some(PrAct::Copied(what)) => self.set_status(tf!("Copié : {what}", "Copied: {what}")),
            Some(PrAct::OpenGroup) => self.pr_open_group(repo, n),
            None => {}
        }
    }

    /// The files of the pull request (those still there) opened in the group of tabs "PR #n".
    fn pr_open_group(&mut self, repo: PathBuf, n: u64) {
        const MAX: usize = 40;
        let Some(diff) = &self.pr.files.diff else { return };
        let paths: Vec<PathBuf> = diff.files.iter().filter(|f| f.status != 'D').map(|f| repo.join(&f.path)).filter(|p| p.is_file()).collect();
        if paths.is_empty() {
            self.set_status(t!("Aucun fichier de la PR dans tes fichiers (Checkout de la PR ?)", "No file of the PR in your files (Checkout of the PR?)"));
            return;
        }
        let back = self.docs.get(self.active).map(|d| d.uid);
        let group = format!("PR #{n}");
        let mut opened = 0;
        for p in paths.iter().take(MAX) {
            if self.open_path(p.clone()) {
                self.set_tab_group(self.active, Some(group.clone()));
                opened += 1;
            }
        }
        if let Some(k) = back.and_then(|u| self.docs.iter().position(|d| d.uid == u)) {
            self.activate_tab(k);
        }
        self.set_status(if paths.len() > MAX {
            tf!("{opened} fichier(s) ouverts dans le groupe {group} (les {MAX} premiers sur {})", "{opened} file(s) opened in the group {group} (the first {MAX} of {})", paths.len())
        } else {
            tf!("{opened} fichier(s) ouverts dans le groupe {group}", "{opened} file(s) opened in the group {group}")
        });
    }

    pub(super) fn pr_job(&mut self, label: String, reload: Option<u64>, draft: Option<String>, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
        self.pr.busy = true;
        self.jobs.spawn(move || JobResult::PrOp { label, result: f(), reload, draft });
    }

    fn pr_open_file(&mut self, rel: &str, line: usize) {
        let Some(repo) = self.pr.repo.clone() else { return };
        let path = repo.join(rel);
        if !path.is_file() {
            self.set_status(tf!("« {rel} » n'est pas dans tes fichiers (Checkout de la PR ?)", "“{rel}” is not in your files (Checkout of the PR?)"));
            return;
        }
        if self.open_path(path) {
            if let Some(d) = self.docs.get_mut(self.active) {
                d.goto_line(line.saturating_sub(1), 0);
            }
            self.pending_focus = true;
        }
    }

    /// Replaces lines `start..=end` (1-based, as on GitHub) of `rel` with the suggestion, in the
    /// editor (Ctrl+Z undoes it), then saves the file.
    fn apply_suggestion(&mut self, id: String, rel: &str, start: usize, end: usize, text: &str) {
        let Some(repo) = self.pr.repo.clone() else { return };
        let path = repo.join(rel);
        let back = self.docs.get(self.active).map(|d| d.uid);
        let Some(i) = self.docs.iter().position(|d| d.path.as_ref() == Some(&path)).or_else(|| self.open_path(path.clone()).then_some(self.active)) else {
            self.set_error(tf!("Impossible d'ouvrir {rel}", "Cannot open {rel}"));
            return;
        };
        // the suggestions applied above in the file moved the lines
        let shift: isize = self.pr.shifts.get(rel).map_or(0, |v| v.iter().filter(|(l, _)| *l < start).map(|(_, d)| d).sum());
        let d = &mut self.docs[i];
        let was_dirty = d.is_dirty();
        let (s0, e0) = ((start as isize - 1 + shift).max(0) as usize, (end as isize - 1 + shift).max(0) as usize);
        let n = d.rope.len_lines();
        if e0 >= n {
            self.set_status(tf!("{rel} n'a plus la ligne {end} : fichier changé depuis le commentaire ?", "{rel} no longer has line {end}: file changed since the comment?"));
            return;
        }
        let a = d.rope.line_to_char(s0);
        let (b, newline) = if e0 + 1 < n { (d.rope.line_to_char(e0 + 1), true) } else { (d.rope.len_chars(), false) };
        let repl = if text.is_empty() || !newline { text.to_string() } else { format!("{text}\n") };
        d.replace_range(a, b, &repl);
        d.sels = vec![Sel::cursor(a)];
        d.scroll_to_cursor = true;
        let added = if text.is_empty() { 0 } else { text.split('\n').count() as isize };
        self.pr.shifts.entry(rel.to_string()).or_default().push((end, added - (end - start + 1) as isize));
        self.pr.applied.insert(id);
        let saved = !was_dirty && self.save(i);
        self.set_status(if saved {
            tf!("Suggestion appliquée et enregistrée : {rel}:{start} — à committer (Ctrl+Z dans le fichier pour l'annuler)", "Suggestion applied and saved: {rel}:{start} — to commit (Ctrl+Z in the file to undo it)")
        } else {
            tf!("Suggestion appliquée dans {rel}:{start} (non enregistré : le fichier avait d'autres modifications)", "Suggestion applied in {rel}:{start} (not saved: the file had other changes)")
        });
        if let Some(k) = back.and_then(|u| self.docs.iter().position(|d| d.uid == u)) {
            self.activate_tab(k);
        }
    }
}

/// Row of the diff a conversation is about: its last line, on its side of the file.
fn thread_row(diff: &HistoryDiff, t: &github::Thread) -> Option<usize> {
    ai_review::diff_row(diff, &t.path, t.line?, &t.side)
}

/// The comment being written on a line of the diff, under it.
fn line_composer(ui: &mut egui::Ui, draft: &mut String, new_line: &mut Option<(String, usize, &'static str, usize)>, busy: bool, accent: Color32, act: &mut Option<PrAct>) {
    let Some((path, line, side, _)) = new_line.clone() else { return };
    egui::Frame::none()
        .stroke(egui::Stroke::new(1.0_f32, icons::mix(ui.visuals().panel_fill, accent, 0.6)))
        .rounding(8.0)
        .inner_margin(egui::Margin::same(10.0))
        .outer_margin(egui::Margin::symmetric(0.0, 6.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let what = if side == "LEFT" { t!(" (ancienne version)", " (old version)") } else { "" };
                ui.label(RichText::new(tf!("💬 Nouveau commentaire sur {path}:{line}{what}", "💬 New comment on {path}:{line}{what}")).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(t!("Annuler", "Cancel")).clicked() {
                        *new_line = None;
                    }
                });
            });
            if composer(ui, draft, t!("Ton commentaire (Markdown ; ```suggestion pour proposer un remplacement)…", "Your comment (Markdown; ```suggestion to suggest a replacement)…"), t!("Commenter la ligne", "Comment on the line"), busy, accent) {
                *act = Some(PrAct::Send(DRAFT_LINE.into()));
            }
        });
}

/// Files on the left (with their comments to deal with), every diff one under the other on the right.
/// `viewed`: the files I marked as viewed, with a box to tick before each (whole pull request only).
fn diff_area(ui: &mut egui::Ui, id: &str, pd: &mut PrDiff, threads: &[github::Thread], notes: Option<history::Notes>, viewed: Option<&HashSet<String>>, act: &mut Option<PrAct>) -> Option<DiffClick> {
    let Some(diff) = &pd.diff else {
        if pd.loading {
            spinner(ui);
        }
        return None;
    };
    egui::SidePanel::left(egui::Id::new(id).with("files")).resizable(true).default_width(300.0).width_range(180.0..=600.0).show_inside(ui, |ui| {
        egui::ScrollArea::vertical().id_salt((id, "files_scroll")).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (k, f) in diff.files.iter().enumerate() {
                let n = threads.iter().filter(|t| !t.resolved && t.path == f.path).count();
                let label = if n > 0 { format!("{}  💬{n}", f.path) } else { f.path.clone() };
                let row = |ui: &mut egui::Ui| super::git_view::file_row(ui, f.status, &label, Some((Some(f.adds as u32), Some(f.dels as u32))));
                let clicked = match viewed {
                    Some(v) => {
                        ui.horizontal(|ui| {
                            let mut seen = v.contains(&f.path);
                            if ui.checkbox(&mut seen, "").on_hover_text(t!("Vu : le fichier se replie et GitHub s'en souvient pour ta revue", "Viewed: the file folds and GitHub remembers it for your review")).changed() {
                                *act = Some(PrAct::Viewed(f.path.clone(), seen));
                                if seen {
                                    pd.collapsed.insert(f.start);
                                } else {
                                    pd.collapsed.remove(&f.start);
                                }
                            }
                            row(ui).clicked()
                        })
                        .inner
                    }
                    None => row(ui).clicked(),
                };
                if clicked {
                    pd.collapsed.remove(&f.start);
                    pd.jump = Some(history::FileJump::To(k));
                }
            }
        });
    });
    egui::CentralPanel::default()
        .frame(egui::Frame::none().inner_margin(egui::Margin { left: 6.0, ..Default::default() }))
        .show_inside(ui, |ui| {
            let file_buttons: &[history::DiffButton] = &[(t!("Ouvrir", "Open"), t!("Ouvrir le fichier (tel qu'il est dans tes fichiers)", "Open the file (as it is in your files)"), DiffAct::Open)];
            let jump = pd.jump.take();
            history::stacked_diff_view(ui, &diff.lines, &diff.files, &mut pd.collapsed, id, &[], file_buttons, jump, notes)
        })
        .inner
}

/// Open, open on GitHub, checkout, copy the address or the number: menu of a pull request.
fn quick_menu(ui: &mut egui::Ui, n: u64, url: &str, in_list: bool, act: &mut Option<PrAct>) {
    let mut pick = |a: PrAct, ui: &mut egui::Ui| {
        *act = Some(a);
        ui.close_menu();
    };
    if in_list && ui.button(t!("Ouvrir", "Open")).clicked() {
        pick(PrAct::Select(n), ui);
    }
    if ui.button(t!("Ouvrir sur GitHub", "Open on GitHub")).clicked() {
        pick(PrAct::Web(n), ui);
    }
    if ui.button("⤵ Checkout").clicked() {
        pick(PrAct::Checkout(n), ui);
    }
    ui.separator();
    if !url.is_empty() && ui.button(t!("Copier le lien", "Copy the link")).clicked() {
        ui.ctx().copy_text(url.to_string());
        pick(PrAct::Copied(url.to_string()), ui);
    }
    if ui.button(tf!("Copier le numéro (#{n})", "Copy the number (#{n})")).clicked() {
        ui.ctx().copy_text(format!("#{n}"));
        pick(PrAct::Copied(format!("#{n}")), ui);
    }
}

/// On top of the conversation: the checks, whether it can be merged, and the buttons to merge,
/// update the branch, close, reopen, or move between draft and ready.
#[allow(clippy::too_many_arguments)]
fn merge_box(ui: &mut egui::Ui, d: &github::PrDetail, method: &mut usize, delete_branch: &mut bool, all_checks: &mut bool, busy: bool, accent: Color32, act: &mut Option<PrAct>) {
    use github::CheckState;
    let v = ui.visuals().clone();
    let n = d.number;
    let gh = |label: &str, more: &[&str], verb: &str| PrAct::Gh { label: label.into(), cmds: vec![github::pr_args(verb, n, more)], draft: None };
    egui::Frame::none()
        .stroke(egui::Stroke::new(1.0_f32, v.widgets.noninteractive.bg_stroke.color))
        .rounding(8.0)
        .inner_margin(egui::Margin::same(10.0))
        .outer_margin(egui::Margin { bottom: 8.0, ..Default::default() })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // the checks of the last commit
            if !d.checks.is_empty() {
                let count = |s: CheckState| d.checks.iter().filter(|c| c.state == s).count();
                let (fail, pending, pass) = (count(CheckState::Fail), count(CheckState::Pending), count(CheckState::Pass));
                let (icon, color) = if fail > 0 { ("✖", err()) } else if pending > 0 { ("◌", warn()) } else { ("✔", ok()) };
                let mut parts = Vec::new();
                if fail > 0 {
                    parts.push(tf!("{fail} en échec", "{fail} failing"));
                }
                if pending > 0 {
                    parts.push(tf!("{pending} en cours", "{pending} running"));
                }
                if pass > 0 {
                    parts.push(tf!("{pass} réussi(s)", "{pass} passed"));
                }
                ui.horizontal(|ui| {
                    let arrow = if *all_checks { "⏷" } else { "⏵" };
                    if ui.add(egui::Label::new(RichText::new(format!("{arrow} {icon} Checks · {}", parts.join(" · "))).strong().color(color)).sense(Sense::click())).on_hover_text(t!("Voir tous les checks", "Show all the checks")).clicked() {
                        *all_checks = !*all_checks;
                    }
                    // the GitHub Actions runs with a failed job
                    let mut runs: Vec<u64> = d.checks.iter().filter(|c| c.state == CheckState::Fail).filter_map(|c| c.job()).map(|(r, _)| r).collect();
                    runs.sort();
                    runs.dedup();
                    if !runs.is_empty()
                        && ui
                            .add_enabled(!busy, egui::Button::new(t!("↻ Relancer les échecs", "↻ Rerun the failed ones")).small())
                            .on_hover_text(t!("gh run rerun --failed : relance les jobs GitHub Actions en échec", "gh run rerun --failed: runs the failed GitHub Actions jobs again"))
                            .clicked()
                    {
                        let cmds = runs.iter().map(|r| ["run", "rerun", &r.to_string(), "--failed"].iter().map(|s| s.to_string()).collect()).collect();
                        *act = Some(PrAct::Gh { label: t!("Checks relancés", "Checks rerun").into(), cmds, draft: None });
                    }
                });
                for c in d.checks.iter().filter(|c| *all_checks || c.state == CheckState::Fail) {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        let (icon, color) = match c.state {
                            CheckState::Fail => ("✖", err()),
                            CheckState::Pending => ("◌", warn()),
                            CheckState::Pass => ("✔", ok()),
                            CheckState::Skip => ("–", Color32::GRAY),
                        };
                        ui.label(RichText::new(icon).color(color));
                        let name = if c.workflow.is_empty() { c.name.clone() } else { format!("{} / {}", c.workflow, c.name) };
                        ui.label(RichText::new(&name).small());
                        if !c.description.is_empty() {
                            ui.label(RichText::new(&c.description).small().weak());
                        }
                        if let Some((_, job)) = c.job() {
                            if ui.small_button("Logs").on_hover_text(t!("Ouvrir le log dans un onglet (les étapes en échec seulement s'il y en a)", "Open the log in a tab (only the failed steps if any)")).clicked() {
                                *act = Some(PrAct::Log(name.clone(), job));
                            }
                        }
                        if !c.url.is_empty() && ui.small_button("↗").on_hover_text(t!("Ouvrir les détails dans le navigateur", "Open the details in the browser")).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(&c.url));
                        }
                    });
                }
                ui.add_space(4.0);
            }
            // can it be merged
            let base = &d.base;
            let purple = Color32::from_rgb(0x8a, 0x5c, 0xd8);
            let (state, color) = match (d.state.as_str(), d.merge_state.as_str(), d.mergeable.as_str()) {
                ("MERGED", ..) => (t!("Fusionnée", "Merged").to_string(), purple),
                ("CLOSED", ..) => (t!("Fermée sans être fusionnée", "Closed without being merged").to_string(), err()),
                (_, _, "CONFLICTING") | (_, "DIRTY", _) => (tf!("✖ Conflits avec {base} : à résoudre avant de fusionner", "✖ Conflicts with {base}: to resolve before merging"), err()),
                (_, "DRAFT", _) => (t!("Brouillon : pas encore prête pour la revue", "Draft: not ready for review yet").to_string(), Color32::GRAY),
                (_, "BEHIND", _) => (tf!("En retard sur {base} : mets la branche à jour", "Behind {base}: update the branch"), warn()),
                (_, "BLOCKED", _) => (t!("Bloquée : il manque des approbations ou des checks requis", "Blocked: approvals or required checks are missing").to_string(), warn()),
                (_, "UNSTABLE", _) => (t!("Fusionnable, mais des checks ne passent pas", "Can be merged, but some checks fail").to_string(), warn()),
                (_, "CLEAN" | "HAS_HOOKS", _) => (t!("✔ Prête à fusionner", "✔ Ready to merge").to_string(), ok()),
                _ => (t!("GitHub vérifie encore si elle peut être fusionnée… (⟳ pour relire)", "GitHub is still checking whether it can be merged… (⟳ to read again)").to_string(), v.weak_text_color()),
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(state).strong().color(color));
                let review = match d.review_decision.as_str() {
                    "APPROVED" => Some((t!("· ✔ approuvée", "· ✔ approved"), ok())),
                    "CHANGES_REQUESTED" => Some((t!("· ✖ changements demandés", "· ✖ changes requested"), err())),
                    "REVIEW_REQUIRED" => Some((t!("· revue requise", "· review required"), warn())),
                    _ => None,
                };
                if let (Some((t, c)), "OPEN") = (review, d.state.as_str()) {
                    ui.label(RichText::new(t).color(c));
                }
            });
            match d.state.as_str() {
                "OPEN" => {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        if d.is_draft {
                            if ui.add_enabled(!busy, filled_button(t!("Prête pour la revue", "Ready for review"), accent)).on_hover_text("gh pr ready").clicked() {
                                *act = Some(gh(t!("Prête pour la revue", "Ready for review"), &[], "ready"));
                            }
                        } else {
                            let (m, fr, en) = MERGE_METHODS[*method];
                            let conflicts = d.mergeable == "CONFLICTING";
                            if ui.add_enabled(!busy && !conflicts, filled_button(t!("Fusionner", "Merge"), accent)).on_hover_text(format!("gh pr merge --{m}")).clicked() {
                                let gone = if *delete_branch { tf!("\nLa branche {} sera supprimée.", "\nThe branch {} will be deleted.", d.head) } else { String::new() };
                                *act = Some(PrAct::Ask {
                                    title: t!("Fusionner la pull request", "Merge the pull request").into(),
                                    msg: tf!("Fusionner #{n} « {} » dans {base} ({}) ?{gone}", "Merge #{n} “{}” into {base} ({})?{gone}", d.title, t!(fr, en)),
                                    yes: t!("Fusionner", "Merge").into(),
                                    label: tf!("Fusion de la PR #{n}", "Merge of PR #{n}"),
                                    cmds: vec![github::merge_args(n, m, *delete_branch)],
                                });
                            }
                            egui::ComboBox::from_id_salt("pr_merge_method").selected_text(t!(fr, en)).show_ui(ui, |ui| {
                                for (k, (_, fr, en)) in MERGE_METHODS.iter().enumerate() {
                                    ui.selectable_value(method, k, t!(*fr, *en));
                                }
                            });
                            ui.checkbox(delete_branch, t!("supprimer la branche", "delete the branch"));
                        }
                        ui.menu_button(t!("⟳ Mettre à jour la branche", "⟳ Update the branch"), |ui| {
                            if ui.button(tf!("Fusionner {base} dedans", "Merge {base} into it")).on_hover_text("gh pr update-branch").clicked() {
                                *act = Some(gh(t!("Branche mise à jour", "Branch updated"), &[], "update-branch"));
                                ui.close_menu();
                            }
                            if ui.button(tf!("Rebaser sur {base}", "Rebase onto {base}")).on_hover_text("gh pr update-branch --rebase").clicked() {
                                *act = Some(PrAct::Ask {
                                    title: t!("Rebaser la branche", "Rebase the branch").into(),
                                    msg: tf!("Rejouer les commits de {} par-dessus {base} sur GitHub ?\nLa branche est réécrite : fais un pull --rebase (ou un nouveau checkout) ensuite.", "Replay the commits of {} on top of {base} on GitHub?\nThe branch is rewritten: pull --rebase (or checkout again) afterwards.", d.head),
                                    yes: t!("Rebaser", "Rebase").into(),
                                    label: t!("Branche rebasée", "Branch rebased").into(),
                                    cmds: vec![github::pr_args("update-branch", n, &["--rebase"])],
                                });
                                ui.close_menu();
                            }
                        });
                        if !d.is_draft && ui.add_enabled(!busy, egui::Button::new(t!("Repasser en brouillon", "Back to draft"))).on_hover_text("gh pr ready --undo").clicked() {
                            *act = Some(gh(t!("Repassée en brouillon", "Back to draft"), &["--undo"], "ready"));
                        }
                        if ui.add_enabled(!busy, egui::Button::new(RichText::new(t!("Fermer", "Close")).color(err()))).clicked() {
                            *act = Some(PrAct::Ask {
                                title: t!("Fermer la pull request", "Close the pull request").into(),
                                msg: tf!("Fermer #{n} « {} » sans la fusionner ? Elle pourra être rouverte.", "Close #{n} “{}” without merging it? It can be reopened.", d.title),
                                yes: t!("Fermer", "Close").into(),
                                label: tf!("PR #{n} fermée", "PR #{n} closed"),
                                cmds: vec![github::pr_args("close", n, &[])],
                            });
                        }
                    });
                }
                "CLOSED" => {
                    if ui.add_enabled(!busy, egui::Button::new(t!("Rouvrir", "Reopen"))).clicked() {
                        *act = Some(gh(t!("PR rouverte", "PR reopened"), &[], "reopen"));
                    }
                }
                _ => {}
            }
        });
}

/// Reviewers, assignees, labels and milestone, each with what changes them.
#[allow(clippy::too_many_arguments)]
fn side_info(ui: &mut egui::Ui, d: &github::PrDetail, new_reviewer: &mut String, new_assignee: &mut String, choices: Option<&(Vec<github::Label>, Vec<String>)>, busy: bool, act: &mut Option<PrAct>) {
    let n = d.number;
    let edit = |label: String, more: &[&str]| PrAct::Gh { label, cmds: vec![github::pr_args("edit", n, more)], draft: None };
    let title = |ui: &mut egui::Ui, text: &str| {
        ui.add_space(8.0);
        ui.label(RichText::new(text).small().strong().weak());
    };
    // a list of logins, ✖ to take one off, a field to add one
    let mut people = |ui: &mut egui::Ui, list: &[String], new: &mut String, add: &str, remove: &str, hint: &str| {
        if list.is_empty() {
            ui.label(RichText::new(t!("Personne", "Nobody")).weak());
        }
        for who in list {
            ui.horizontal(|ui| {
                avatar(ui, who.rsplit('/').next().unwrap_or(who), 18.0);
                ui.label(who);
                if ui.add_enabled(!busy, egui::Button::new("✖").small().frame(false)).on_hover_text(t!("Retirer", "Remove")).clicked() {
                    *act = Some(edit(tf!("{who} retiré(e)", "{who} removed"), &[remove, who]));
                }
            });
        }
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(new).hint_text(hint).desired_width(ui.available_width() - 30.0));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (ui.add_enabled(!busy && !new.trim().is_empty(), egui::Button::new("+")).clicked() || enter) && !new.trim().is_empty() {
                let who = new.trim().trim_start_matches('@').to_string();
                let who = if who == "me" { "@me".to_string() } else { who };
                *act = Some(edit(tf!("{who} ajouté(e)", "{who} added"), &[add, &who]));
                new.clear();
            }
        });
    };
    title(ui, t!("RELECTEURS", "REVIEWERS"));
    people(ui, &d.reviewers, new_reviewer, "--add-reviewer", "--remove-reviewer", t!("login ou org/équipe", "login or org/team"));
    title(ui, t!("ASSIGNÉS", "ASSIGNEES"));
    people(ui, &d.assignees, new_assignee, "--add-assignee", "--remove-assignee", t!("login (me : toi)", "login (me: you)"));

    title(ui, t!("ÉTIQUETTES", "LABELS"));
    ui.horizontal_wrapped(|ui| {
        for l in &d.labels {
            let c = hex_color(&l.color);
            let bg = icons::mix(ui.visuals().panel_fill, c, 0.25);
            let r = ui.add_enabled(!busy, egui::Button::new(RichText::new(format!("{} ✖", l.name)).small().color(ui.visuals().strong_text_color())).fill(bg).rounding(10.0));
            if r.on_hover_text(t!("Retirer l'étiquette", "Remove the label")).clicked() {
                *act = Some(edit(tf!("Étiquette « {} » retirée", "Label “{}” removed", l.name), &["--remove-label", &l.name]));
            }
        }
        ui.menu_button("+", |ui| {
            let Some((labels, _)) = choices else {
                spinner(ui);
                return;
            };
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for l in labels.iter().filter(|l| !d.labels.iter().any(|x| x.name == l.name)) {
                    if ui.button(RichText::new(format!("● {}", l.name)).color(hex_color(&l.color))).clicked() {
                        *act = Some(edit(tf!("Étiquette « {} » ajoutée", "Label “{}” added", l.name), &["--add-label", &l.name]));
                        ui.close_menu();
                    }
                }
            });
        });
    });

    title(ui, t!("JALON", "MILESTONE"));
    ui.horizontal(|ui| {
        let now = if d.milestone.is_empty() { RichText::new(t!("Aucun", "None")).weak() } else { RichText::new(&d.milestone) };
        ui.menu_button(now, |ui| {
            let Some((_, milestones)) = choices else {
                spinner(ui);
                return;
            };
            if !d.milestone.is_empty() && ui.button(t!("Aucun", "None")).clicked() {
                *act = Some(edit(t!("Jalon retiré", "Milestone removed").into(), &["--remove-milestone"]));
                ui.close_menu();
            }
            if milestones.is_empty() {
                ui.label(RichText::new(t!("Pas de jalon ouvert dans ce dépôt", "No open milestone in this repository")).weak());
            }
            for m in milestones.iter().filter(|m| **m != d.milestone) {
                if ui.button(m).clicked() {
                    *act = Some(edit(tf!("Jalon : {m}", "Milestone: {m}"), &["--milestone", m]));
                    ui.close_menu();
                }
            }
        });
    });
}

/// "d73a4a" → its color.
fn hex_color(hex: &str) -> Color32 {
    let c = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).unwrap_or(128);
    Color32::from_rgb(c(0), c(2), c(4))
}

/// The reactions given (a click gives or takes back mine), then 🙂 to add one.
fn reactions_row(ui: &mut egui::Ui, id: &str, reactions: &[github::Reaction], busy: bool, act: &mut Option<PrAct>) {
    if id.is_empty() {
        return;
    }
    for r in reactions {
        let tip = if r.mine { t!("Retirer ta réaction", "Take back your reaction") } else { t!("Réagir pareil", "React the same") };
        if ui.add_enabled(!busy, egui::Button::new(RichText::new(format!("{} {}", github::emoji(&r.content), r.count)).small()).selected(r.mine).rounding(10.0)).on_hover_text(tip).clicked() {
            *act = Some(PrAct::React { id: id.into(), content: r.content.clone(), add: !r.mine });
        }
    }
    ui.add_enabled_ui(!busy, |ui| {
        ui.menu_button(RichText::new("🙂").small(), |ui| {
            ui.horizontal(|ui| {
                for (content, e) in github::REACTIONS {
                    let mine = reactions.iter().any(|r| r.content == content && r.mine);
                    if ui.add(egui::Button::new(e).selected(mine)).clicked() {
                        *act = Some(PrAct::React { id: id.into(), content: content.into(), add: !mine });
                        ui.close_menu();
                    }
                }
            });
        })
        .response
        .on_hover_text(t!("Ajouter une réaction", "Add a reaction"));
    });
}

/// What a comment needs for its reactions and, when mine, to be changed or deleted.
struct CommentTools<'a> {
    id: &'a str,
    body: &'a str,
    mine: bool,
    /// Reviews cannot be deleted, only changed.
    deletable: bool,
    reactions: &'a [github::Reaction],
    busy: bool,
    accent: Color32,
}

/// The body of a comment (`show`), or its editor while it is being changed; then its reactions
/// and, for mine, ✏ / 🗑.
fn comment_body(
    ui: &mut egui::Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    c: CommentTools,
    editing: &mut Option<(String, String)>,
    act: &mut Option<PrAct>,
    show: impl FnOnce(&mut egui::Ui, &mut egui_commonmark::CommonMarkCache, &mut Option<PrAct>),
) {
    if let Some((_, text)) = editing.as_mut().filter(|(id, _)| id == c.id) {
        ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY));
        let (save, cancel) = ui
            .horizontal(|ui| {
                let save = ui.add_enabled(!c.busy && !text.trim().is_empty(), filled_button(t!("Enregistrer", "Save"), c.accent)).clicked();
                (save, ui.button(t!("Annuler", "Cancel")).clicked())
            })
            .inner;
        if save {
            *act = Some(PrAct::SaveComment);
        }
        if cancel {
            *editing = None;
        }
        return;
    }
    show(ui, cache, act);
    ui.horizontal_wrapped(|ui| {
        reactions_row(ui, c.id, c.reactions, c.busy, act);
        if c.mine && !c.id.is_empty() {
            if ui.add_enabled(!c.busy, egui::Button::new("✏").small().frame(false)).on_hover_text(t!("Modifier ton commentaire", "Edit your comment")).clicked() {
                *editing = Some((c.id.to_string(), c.body.to_string()));
            }
            if c.deletable && ui.add_enabled(!c.busy, egui::Button::new("🗑").small().frame(false)).on_hover_text(t!("Supprimer ton commentaire", "Delete your comment")).clicked() {
                *act = Some(PrAct::DeleteComment(c.id.to_string()));
            }
        }
    });
}

/// A pull request in the list: number and title, then author, branch, date and review state.
fn pr_row(ui: &mut egui::Ui, pr: &github::PrSummary, selected: bool, accent: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), Sense::click());
    let v = ui.visuals();
    if selected {
        ui.painter().rect_filled(rect, 5.0, v.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, v.widgets.hovered.weak_bg_fill);
    }
    let p = ui.painter().with_clip_rect(rect.shrink(2.0));
    let (_, color) = state_badge(&pr.state, pr.is_draft);
    p.circle_filled(egui::pos2(rect.left() + 10.0, rect.top() + 14.0), 4.0, color);
    let r = p.text(egui::pos2(rect.left() + 20.0, rect.top() + 6.0), Align2::LEFT_TOP, format!("#{}", pr.number), egui::FontId::proportional(13.5), v.weak_text_color());
    p.text(egui::pos2(r.right() + 6.0, rect.top() + 6.0), Align2::LEFT_TOP, &pr.title, egui::FontId::proportional(13.5), v.strong_text_color());
    let mut info = format!("{} · {} → {} · {}", pr.author.login, pr.head_ref_name, pr.base_ref_name, ago(&pr.updated_at));
    if pr.is_draft {
        info.push_str(t!(" · brouillon", " · draft"));
    }
    let r = p.text(egui::pos2(rect.left() + 20.0, rect.top() + 26.0), Align2::LEFT_TOP, info, egui::FontId::proportional(11.5), v.weak_text_color());
    let review = match pr.review_decision.as_deref() {
        Some("APPROVED") => Some((t!("✔ approuvée", "✔ approved"), ok())),
        Some("CHANGES_REQUESTED") => Some((t!("✖ à corriger", "✖ to fix"), err())),
        Some("REVIEW_REQUIRED") => Some((t!("revue demandée", "review requested"), accent)),
        _ => None,
    };
    if let Some((t, c)) = review {
        p.text(egui::pos2(r.right() + 8.0, rect.top() + 26.0), Align2::LEFT_TOP, t, egui::FontId::proportional(11.5), c);
    }
    resp.on_hover_text(&pr.title)
}

/// A commit of the pull request: hash, message, author and date.
fn commit_row(ui: &mut egui::Ui, c: &github::Commit, selected: bool, accent: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::click());
    let v = ui.visuals();
    if selected {
        ui.painter().rect_filled(rect, 4.0, v.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let pt = ui.painter().with_clip_rect(rect);
    let cy = rect.center().y;
    let r = pt.text(egui::pos2(rect.left() + 8.0, cy), Align2::LEFT_CENTER, &c.oid[..c.oid.len().min(8)], egui::FontId::monospace(12.0), accent);
    let right = pt.text(
        egui::pos2(rect.right() - 8.0, cy),
        Align2::RIGHT_CENTER,
        format!("{} · {}", c.author(), ago(&c.committed_date)),
        egui::FontId::proportional(12.0),
        v.weak_text_color(),
    );
    pt.with_clip_rect(egui::Rect::from_min_max(rect.min, egui::pos2(right.left() - 10.0, rect.bottom()))).text(
        egui::pos2(r.right() + 12.0, cy),
        Align2::LEFT_CENTER,
        &c.message_headline,
        egui::FontId::proportional(13.5),
        v.strong_text_color(),
    );
    resp.on_hover_text(t!("Voir les changements de ce commit seul", "See the changes of this commit alone"))
}

pub(super) fn state_badge(state: &str, draft: bool) -> (&'static str, Color32) {
    match state {
        _ if draft && state == "OPEN" => (t!("Brouillon", "Draft"), Color32::GRAY),
        "OPEN" => (t!("Ouverte", "Open"), ok()),
        "MERGED" => (t!("Fusionnée", "Merged"), Color32::from_rgb(0x8a, 0x5c, 0xd8)),
        _ => (t!("Fermée", "Closed"), err()),
    }
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    let bg = icons::mix(ui.visuals().panel_fill, color, 0.18);
    ui.label(RichText::new(format!(" {text} ")).small().strong().color(color).background_color(bg));
}

/// A round avatar with the initial of `login`, in a color of its own.
fn avatar(ui: &mut egui::Ui, login: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let h = login.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let color: Color32 = egui::epaint::Hsva::new((h % 360) as f32 / 360.0, 0.45, 0.75, 1.0).into();
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
    let initial: String = login.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, initial, egui::FontId::proportional(size * 0.5), Color32::WHITE);
}

/// A comment like on GitHub: avatar, author and date on a line, then its body under the name.
fn comment_block(ui: &mut egui::Ui, author: &str, date: &str, tag: Option<(&str, Color32)>, body: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        avatar(ui, author, 26.0);
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(author).strong());
                if let Some((t, c)) = tag {
                    ui.label(RichText::new(t).color(c).strong());
                }
                if !date.is_empty() {
                    ui.label(RichText::new(ago(date)).weak().small()).on_hover_text(date);
                }
            });
            // the message in a bubble, like a chat
            let v = ui.visuals();
            egui::Frame::none()
                .fill(icons::mix(v.panel_fill, v.text_color(), 0.045))
                .rounding(egui::Rounding { nw: 2.0, ne: 10.0, sw: 10.0, se: 10.0 })
                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width());
                    body(ui)
                });
        });
    });
}

/// Something that happened, on one discreet line (like the tracking of the chatter of Odoo).
fn tracking(ui: &mut egui::Ui, author: &str, date: &str, what: &str, color: Color32) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        avatar(ui, author, 18.0);
        ui.add_space(6.0);
        ui.label(RichText::new(author).strong());
        ui.label(RichText::new(what).color(color));
        ui.label(RichText::new(ago(date)).weak().small()).on_hover_text(date);
    });
}

/// "─── 21 octobre 2026 ───" between the days of the conversation.
fn day_separator(ui: &mut egui::Ui, date: &str) {
    const MONTHS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
    const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let n = |a: usize, b: usize| date.get(a..b).and_then(|s| s.parse::<usize>().ok());
    let label = match (n(0, 4), n(5, 7), n(8, 10)) {
        (Some(y), Some(m), Some(d)) if (1..=12).contains(&m) => tf!("{d} {} {y}", "{} {d}, {y}", t!(MONTHS[m - 1], MONTHS_EN[m - 1])),
        _ => day(date).to_string(),
    };
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), Sense::hover());
    let v = ui.visuals();
    let p = ui.painter();
    let r = p.text(rect.center(), Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), v.weak_text_color());
    let stroke = egui::Stroke::new(1.0_f32, v.widgets.noninteractive.bg_stroke.color);
    p.line_segment([egui::pos2(rect.left(), rect.center().y), egui::pos2(r.left() - 10.0, rect.center().y)], stroke);
    p.line_segment([egui::pos2(r.right() + 10.0, rect.center().y), egui::pos2(rect.right(), rect.center().y)], stroke);
}

/// What happened in a pull request, for its conversation.
#[derive(Debug, PartialEq)]
enum Event {
    Opened,
    /// A general comment or a review (index in `comments`).
    Comment(usize),
    /// A conversation on lines (index in `threads`).
    Thread(usize),
    /// Commits pushed in a row by the same author the same day (indexes in `commits`).
    Push { author: String, commits: Vec<usize> },
}

/// Everything that happened, newest first, with its date.
fn timeline(d: &github::PrDetail, show_resolved: bool) -> Vec<(String, Event)> {
    let mut ev: Vec<(String, Event)> = vec![(d.created_at.clone(), Event::Opened)];
    ev.extend(d.comments.iter().enumerate().map(|(k, c)| (c.created_at.clone(), Event::Comment(k))));
    ev.extend(
        d.threads
            .iter()
            .enumerate()
            .filter(|(_, t)| show_resolved || !t.resolved)
            .map(|(k, t)| (t.comments.first().map_or(String::new(), |c| c.created_at.clone()), Event::Thread(k))),
    );
    for (k, c) in d.commits.iter().enumerate() {
        let author = c.author();
        match ev.last_mut() {
            Some((date, Event::Push { author: a, commits })) if *a == author && day(date) == day(&c.committed_date) => {
                commits.push(k);
                *date = c.committed_date.clone();
            }
            _ => ev.push((c.committed_date.clone(), Event::Push { author, commits: vec![k] })),
        }
    }
    // stable: things of the same second keep their order
    ev.sort_by(|a, b| b.0.cmp(&a.0));
    ev
}

/// A text field to write a comment, with its button; true: send it (button or Ctrl+Entrée).
fn composer(ui: &mut egui::Ui, text: &mut String, hint: &str, button: &str, busy: bool, accent: Color32) -> bool {
    let rows = if text.is_empty() { 1 } else { 3 };
    let resp = ui.add(egui::TextEdit::multiline(text).hint_text(hint).desired_rows(rows).desired_width(f32::INFINITY));
    let ctrl_enter = resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
    let mut send = false;
    if !text.trim().is_empty() {
        ui.horizontal(|ui| {
            send = ui.add_enabled(!busy, filled_button(button, accent)).on_hover_text(t!("Ctrl+Entrée", "Ctrl+Enter")).clicked();
            ui.label(RichText::new("Markdown").small().weak());
        });
    }
    (send || ctrl_enter) && !busy && !text.trim().is_empty()
}

/// The code commented, numbered like in a diff, the commented lines highlighted.
fn code_snippet(ui: &mut egui::Ui, hunk: &str, from: usize, to: usize) {
    let rows = github::hunk_tail(hunk, (to - from + 1).max(4));
    let font = egui::FontId::monospace(12.0);
    let row_h = 18.0;
    let v = ui.visuals().clone();
    let base = v.extreme_bg_color;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), rows.len() as f32 * row_h + 6.0), Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 4.0, base);
    for (k, (old, new, kind, text)) in rows.iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.top() + 3.0 + k as f32 * row_h), egui::vec2(rect.width(), row_h));
        let commented = new.is_some_and(|n| n >= from && n <= to);
        let (tint, color) = match kind {
            '+' => (Some(icons::mix(base, ok(), 0.16)), ok()),
            '-' => (Some(icons::mix(base, err(), 0.16)), err()),
            _ => (None, v.text_color()),
        };
        if let Some(t) = tint {
            p.rect_filled(r, 0.0, t);
        }
        if commented {
            p.rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(3.0, row_h)), 0.0, highlight::yellow());
        }
        let num = new.or(*old).map(|n| n.to_string()).unwrap_or_default();
        p.text(egui::pos2(r.left() + 40.0, r.center().y), Align2::RIGHT_CENTER, num, font.clone(), v.weak_text_color());
        p.text(egui::pos2(r.left() + 48.0, r.center().y), Align2::LEFT_CENTER, format!("{kind} {text}"), font.clone(), color);
    }
}

/// A conversation on lines: where, the code commented, each comment with its suggestions, then
/// the answer being written and the button to resolve it.
#[allow(clippy::too_many_arguments)]
fn thread_ui(
    ui: &mut egui::Ui,
    t: &github::Thread,
    cache: &mut egui_commonmark::CommonMarkCache,
    applied: &HashSet<String>,
    shifts: &HashMap<String, Vec<(usize, isize)>>,
    checked_out: bool,
    busy: bool,
    draft: &mut String,
    editing: &mut Option<(String, String)>,
    accent: Color32,
    in_diff: bool,
    act: &mut Option<PrAct>,
) {
    let line = t.line.or(t.original_line).unwrap_or(1);
    let start = t.start_line.unwrap_or(line);
    let v = ui.visuals().clone();
    egui::Frame::none()
        .stroke(egui::Stroke::new(1.0_f32, v.widgets.noninteractive.bg_stroke.color))
        .rounding(8.0)
        .outer_margin(egui::Margin { left: 0.0, right: 0.0, top: 6.0, bottom: 6.0 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // header: the file and lines, state, resolve
            egui::Frame::none()
                .fill(icons::mix(v.panel_fill, accent, 0.07))
                .rounding(egui::Rounding { nw: 8.0, ne: 8.0, sw: 0.0, se: 0.0 })
                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let lines = if start < line { tf!("lignes {start}-{line}", "lines {start}-{line}") } else { tf!("ligne {line}", "line {line}") };
                        let place = if in_diff { format!("💬 {lines}") } else { format!("{} · {lines}", t.path) };
                        if ui.link(RichText::new(place).monospace().strong()).on_hover_text(t!("Ouvrir le fichier à cette ligne", "Open the file at this line")).clicked() {
                            *act = Some(PrAct::Open(t.path.clone(), line));
                        }
                        if t.resolved {
                            badge(ui, t!("résolu", "resolved"), ok());
                        }
                        if t.outdated {
                            badge(ui, t!("code modifié depuis", "code changed since"), Color32::GRAY);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (label, tip) = if t.resolved { (t!("Rouvrir", "Reopen"), t!("Marquer la conversation comme non résolue", "Mark the conversation as unresolved")) } else { (t!("✔ Résoudre", "✔ Resolve"), t!("Marquer la conversation comme résolue", "Mark the conversation as resolved")) };
                            if ui.add_enabled(!busy, egui::Button::new(label).small()).on_hover_text(tip).clicked() {
                                *act = Some(PrAct::Resolve(t.id.clone(), !t.resolved));
                            }
                        });
                    });
                });
            egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 8.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                // in the diff, the code is just above
                if let Some(first) = t.comments.first().filter(|_| !in_diff) {
                    code_snippet(ui, &first.diff_hunk, start, line);
                }
                for c in &t.comments {
                    comment_block(ui, &c.author, &c.created_at, None, |ui| {
                        let tools = CommentTools { id: &c.id, body: &c.body, mine: c.mine, deletable: true, reactions: &c.reactions, busy, accent };
                        comment_body(ui, cache, tools, editing, act, |ui, cache, act| {
                            for part in github::parts(&c.body) {
                                match part {
                                    Part::Text(text) => {
                                        egui_commonmark::CommonMarkViewer::new().show(ui, cache, &text);
                                    }
                                    Part::Suggestion(text) => suggestion_ui(ui, t, c, &text, start, line, applied, shifts, checked_out, accent, act),
                                }
                            }
                        });
                    });
                }
                ui.add_space(6.0);
                if composer(ui, draft, t!("Répondre…", "Reply…"), t!("Répondre", "Reply"), busy, accent) {
                    *act = Some(PrAct::Send(t.id.clone()));
                }
            });
        });
}

/// A suggested replacement: the lines now (red) and proposed (green), and the button to apply it.
#[allow(clippy::too_many_arguments)]
fn suggestion_ui(
    ui: &mut egui::Ui,
    t: &github::Thread,
    c: &github::LineComment,
    text: &str,
    start: usize,
    line: usize,
    applied: &HashSet<String>,
    shifts: &HashMap<String, Vec<(usize, isize)>>,
    checked_out: bool,
    accent: Color32,
    act: &mut Option<PrAct>,
) {
    let old = github::commented_lines(&c.diff_hunk, line - start + 1);
    let v = ui.visuals().clone();
    let base = v.extreme_bg_color;
    egui::Frame::none()
        .stroke(egui::Stroke::new(1.0_f32, icons::mix(v.panel_fill, accent, 0.45)))
        .rounding(6.0)
        .outer_margin(egui::Margin::symmetric(0.0, 4.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Frame::none().fill(icons::mix(v.panel_fill, accent, 0.10)).inner_margin(egui::Margin::symmetric(8.0, 4.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(t!("Changement suggéré", "Suggested change")).small().strong().color(accent));
            });
            let font = egui::FontId::monospace(12.0);
            let new: Vec<&str> = if text.is_empty() { Vec::new() } else { text.split('\n').collect() };
            let rows: Vec<(char, &str)> = old.iter().map(|l| ('-', l.as_str())).chain(new.iter().map(|l| ('+', *l))).collect();
            let row_h = 18.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), rows.len() as f32 * row_h + 4.0), Sense::hover());
            let p = ui.painter().with_clip_rect(rect);
            for (k, (kind, l)) in rows.iter().enumerate() {
                let r = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.top() + 2.0 + k as f32 * row_h), egui::vec2(rect.width(), row_h));
                let color = if *kind == '+' { ok() } else { err() };
                p.rect_filled(r, 0.0, icons::mix(base, color, 0.16));
                p.text(egui::pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, format!("{kind} {l}"), font.clone(), color);
            }
            egui::Frame::none().inner_margin(egui::Margin::symmetric(8.0, 6.0)).show(ui, |ui| {
                if applied.contains(&c.id) {
                    ui.label(RichText::new(t!("✔ appliquée dans ton fichier", "✔ applied in your file")).color(ok()));
                    return;
                }
                let why = if !checked_out {
                    Some(t!("Fais d'abord « ⤵ Checkout » de la PR", "First do “⤵ Checkout” of the PR"))
                } else if t.line.is_none() || t.outdated {
                    Some(t!("Le code a changé depuis ce commentaire", "The code changed since this comment"))
                } else {
                    None
                };
                let moved = shifts.get(&t.path).is_some_and(|v| v.iter().any(|(l, _)| *l >= start && *l <= line));
                ui.horizontal(|ui| {
                    let resp = ui.add_enabled(why.is_none() && !moved, filled_button(t!("Appliquer la suggestion", "Apply the suggestion"), accent));
                    if resp.on_hover_text(t!("Remplace ces lignes dans ton fichier et l'enregistre (à committer ensuite)", "Replaces these lines in your file and saves it (to commit afterwards)")).clicked() {
                        *act = Some(PrAct::Apply { id: c.id.clone(), path: t.path.clone(), start, end: line, text: text.to_string() });
                    }
                    if let Some(w) = why {
                        ui.label(RichText::new(w).small().weak());
                    }
                });
            });
        });
}

/// "2026-10-21T19:53:49Z" → "2026-10-21".
fn day(iso: &str) -> &str {
    iso.get(..10).unwrap_or(iso)
}

/// "2026-10-21T19:53:49Z" → "il y a 3 jours".
fn ago(iso: &str) -> String {
    let n = |a: usize, b: usize| iso.get(a..b).and_then(|s| s.parse::<i64>().ok());
    let (Some(y), Some(mo), Some(d)) = (n(0, 4), n(5, 7), n(8, 10)) else { return iso.to_string() };
    let (h, mi, s) = (n(11, 13).unwrap_or(0), n(14, 16).unwrap_or(0), n(17, 19).unwrap_or(0));
    // days since 1970-01-01 (civil calendar)
    let (yy, m) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    git::ago(days * 86_400 + h * 3600 + mi * 60 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_newest_first() {
        let commit = |oid: &str, date: &str, who: &str| github::Commit {
            oid: oid.into(),
            message_headline: oid.into(),
            authors: vec![github::CommitAuthor { login: who.into(), name: String::new() }],
            committed_date: date.into(),
        };
        let d = github::PrDetail {
            number: 1,
            title: String::new(),
            body: String::new(),
            author: "me".into(),
            created_at: "2026-01-01T10:00:00Z".into(),
            state: "OPEN".into(),
            is_draft: false,
            head: String::new(),
            head_oid: String::new(),
            base: String::new(),
            commits: vec![commit("a", "2026-01-01T09:00:00Z", "me"), commit("b", "2026-01-01T09:30:00Z", "me"), commit("c", "2026-01-03T09:00:00Z", "me")],
            comments: vec![github::Comment { author: github::Author { login: "bob".into() }, body: "ok".into(), created_at: "2026-01-02T08:00:00Z".into(), ..Default::default() }],
            ..Default::default()
        };
        let t: Vec<Event> = timeline(&d, false).into_iter().map(|(_, e)| e).collect();
        assert_eq!(
            t,
            vec![
                Event::Push { author: "me".into(), commits: vec![2] },
                Event::Comment(0),
                Event::Opened,
                Event::Push { author: "me".into(), commits: vec![0, 1] },
            ]
        );
    }

    #[test]
    fn iso_dates() {
        // 2026-10-21T19:53:49Z is 1792612429 seconds after 1970
        assert_eq!(ago("1970-01-01T00:00:00Z"), crate::git::ago(0));
        assert_eq!(ago("2026-10-21T19:53:49Z"), crate::git::ago(1_792_612_429));
        assert_eq!(ago("bientôt"), "bientôt");
    }
}

#[cfg(test)]
mod render_check {
    use super::*;

    fn texts(shapes: &[egui::epaint::ClippedShape]) -> String {
        let mut out = String::new();
        fn walk(s: &egui::Shape, out: &mut String) {
            match s {
                egui::Shape::Text(t) => {
                    out.push_str(t.galley.text());
                    out.push('|');
                }
                egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    #[test]
    fn comment_text_is_drawn() {
        let ctx = egui::Context::default();
        let mut cache = egui_commonmark::CommonMarkCache::default();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let out = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        comment_block(ui, "bob", "2026-01-01T00:00:00Z", None, |ui| {
                            egui_commonmark::CommonMarkViewer::new().show(ui, &mut cache, "Bonjour `_get_valid_date()` monde");
                        });
                        comment_block(ui, "alice", "2026-01-01T00:00:00Z", None, |ui| {
                            egui_commonmark::CommonMarkViewer::new().show(ui, &mut cache, "Deuxième message");
                        });
                    });
                });
            });
            shapes = out.shapes;
        }
        let t = texts(&shapes);
        assert!(t.contains("Deuxième"), "{t}");
    }

    #[test]
    fn merge_box_and_side() {
        let d = github::PrDetail {
            id: "PR_1".into(),
            number: 7,
            title: "Fix".into(),
            author: "bob".into(),
            viewer: "me".into(),
            state: "OPEN".into(),
            base: "17.0".into(),
            head: "17.0-fix".into(),
            merge_state: "BEHIND".into(),
            review_decision: "APPROVED".into(),
            reviewers: vec!["alice".into()],
            labels: vec![github::Label { name: "bug".into(), color: "d73a4a".into() }],
            checks: vec![github::Check { name: "tests".into(), workflow: "CI".into(), state: github::CheckState::Fail, url: "https://github.com/o/r/actions/runs/1/job/2".into(), description: String::new() }],
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut cache = egui_commonmark::CommonMarkCache::default();
        let (mut method, mut delete, mut all, mut rev, mut asg, mut editing) = (0, false, false, String::new(), String::new(), None);
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let out = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut act = None;
                    merge_box(ui, &d, &mut method, &mut delete, &mut all, false, Color32::BLUE, &mut act);
                    side_info(ui, &d, &mut rev, &mut asg, None, false, &mut act);
                    let reactions = [github::Reaction { content: "HEART".into(), count: 3, mine: true }];
                    let tools = CommentTools { id: "IC_1", body: "salut", mine: true, deletable: true, reactions: &reactions, busy: false, accent: Color32::BLUE };
                    comment_body(ui, &mut cache, tools, &mut editing, &mut act, |ui, _, _| {
                        ui.label("salut");
                    });
                });
            });
            shapes = out.shapes;
        }
        let t = texts(&shapes);
        for want in ["Checks", "CI / tests", "Logs", "17.0", "alice", "bug ✖", "❤ 3", "🗑"] {
            assert!(t.contains(want), "{want}: {t}");
        }
    }

    #[test]
    fn comments_under_their_line() {
        let diff = "diff --git a/x.py b/x.py\n--- a/x.py\n+++ b/x.py\n@@ -1,3 +1,3 @@\n a = 1\n-b = 2\n+b = 3\n c = 4";
        let lines = history::parse_diff(diff);
        let files = history::diff_files(&lines);
        let ctx = egui::Context::default();
        let mut collapsed = HashSet::new();
        let at: HashSet<usize> = [6].into();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let out = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut show = |ui: &mut egui::Ui, i: usize| {
                        ui.label(format!("NOTE sous {i}"));
                        ui.label("deuxième ligne de la note");
                    };
                    history::stacked_diff_view(ui, &lines, &files, &mut collapsed, "t", &[], &[], None, Some(history::Notes { at: &at, show: &mut show }));
                });
            });
            shapes = out.shapes;
        }
        let t = texts(&shapes);
        // the note right after "+b = 3", then the next line of code
        let (note, after) = (t.find("NOTE sous 6").expect(&t), t.find("c = 4").expect(&t));
        assert!(t.find("b = 3").unwrap() < note && note < after, "{t}");
    }
}
