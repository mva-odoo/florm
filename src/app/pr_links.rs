//! Pull requests in the Git view: those of the current branch and of the selected commit, and
//! the window to open one (branch pushed, base branch chosen).

use super::*;
use crate::github;

#[derive(Default)]
pub(super) struct GhUi {
    /// Per repository: the repository pull requests go to ("odoo/odoo") and its default branch
    /// (None while asked, or when `gh` has no answer).
    base: HashMap<PathBuf, Option<(String, String)>>,
    /// Pull requests of a branch (repository, branch).
    branch_prs: HashMap<(PathBuf, String), Vec<github::PrSummary>>,
    /// Pull requests containing a commit (hash).
    commit_prs: HashMap<String, Vec<github::LinkedPr>>,
    /// What was asked and not answered yet.
    pending: HashSet<String>,
    pub(super) create: Option<PrCreate>,
}

pub(super) struct PrCreate {
    repo: PathBuf,
    branch: String,
    title: String,
    body: String,
    /// Branches of the repository pull requests go to, and the one chosen.
    bases: Vec<String>,
    base: String,
    /// Remotes the branch can be pushed to: (name, GitHub owner).
    remotes: Vec<(String, String)>,
    remote: usize,
    draft: bool,
    busy: bool,
    error: Option<String>,
    /// Remote holding the base branch ("" if none): the description is written from its changes.
    base_remote: String,
    /// Claude is writing the description.
    writing: bool,
}

impl GhUi {
    /// Default branch of the repository pull requests of `repo` go to, when already known.
    pub(super) fn base_default(&self, repo: &Path) -> Option<String> {
        self.base.get(repo).cloned().flatten().map(|(_, d)| d)
    }
}

impl App {
    /// The repository pull requests of `repo` go to, asked once to `gh`.
    fn gh_base(&mut self, repo: &Path) -> Option<(String, String)> {
        if let Some(b) = self.gh.base.get(repo) {
            return b.clone();
        }
        self.gh.base.insert(repo.to_path_buf(), None);
        let r = repo.to_path_buf();
        self.jobs.spawn(move || JobResult::GhBase(r.clone(), github::base_repo(&r)));
        None
    }

    pub(super) fn gh_base_loaded(&mut self, repo: PathBuf, r: Result<(String, String), String>) {
        let ok = r.ok().filter(|(n, _)| !n.is_empty());
        if ok.is_none() {
            // asked again at the next opening of the window, not at every frame
            self.gh.base.remove(&repo);
            if let Some(c) = self.gh.create.as_mut().filter(|c| c.repo == repo) {
                c.error = Some(t!("GitHub ne répond pas pour ce dépôt (gh installé et connecté ?)", "GitHub does not answer for this repository (gh installed and logged in?)").into());
            }
            return;
        }
        self.gh.base.insert(repo, ok);
    }

    /// Pull requests of the current branch of the selected repository (asked when needed).
    pub(super) fn current_branch_prs(&mut self) -> Vec<github::PrSummary> {
        let (Some(repo), Some(branch)) = (self.repo.clone(), self.git.status.as_ref().map(|s| s.branch.clone())) else { return Vec::new() };
        if branch.is_empty() || branch.starts_with("HEAD") {
            return Vec::new();
        }
        let key = (repo.clone(), branch.clone());
        if let Some(v) = self.gh.branch_prs.get(&key) {
            return v.clone();
        }
        let pending = format!("branch:{}:{branch}", repo.display());
        if self.gh.pending.insert(pending) {
            self.jobs.spawn(move || JobResult::GhBranchPrs(repo.clone(), branch.clone(), github::for_branch(&repo, &branch)));
        }
        Vec::new()
    }

    pub(super) fn gh_branch_prs_loaded(&mut self, repo: PathBuf, branch: String, r: Result<Vec<github::PrSummary>, String>) {
        self.gh.pending.remove(&format!("branch:{}:{branch}", repo.display()));
        // no answer (no gh, no GitHub remote): nothing shown, not asked again
        self.gh.branch_prs.insert((repo, branch), r.unwrap_or_default());
    }

    /// Pull requests of commit `sha` of `repo`: from GitHub, and those named by the merge bot in
    /// its message ("closes odoo/odoo#123").
    pub(super) fn commit_prs(&mut self, repo: &Path, sha: &str, message: &str) -> Vec<github::LinkedPr> {
        let Some((base, _)) = self.gh_base(repo) else { return Vec::new() };
        let mut v = match self.gh.commit_prs.get(sha) {
            Some(v) => v.clone(),
            None => {
                if self.gh.pending.insert(format!("commit:{sha}")) {
                    let (r, s, b) = (repo.to_path_buf(), sha.to_string(), base.clone());
                    self.jobs.spawn(move || JobResult::GhCommitPrs(s.clone(), github::for_commit(&r, &b, &s)));
                }
                Vec::new()
            }
        };
        for n in github::closed_by_message(message, &base) {
            if !v.iter().any(|p| p.number == n) {
                v.push(github::LinkedPr { number: n, title: String::new(), state: "MERGED".into() });
            }
        }
        v
    }

    pub(super) fn gh_commit_prs_loaded(&mut self, sha: String, r: Result<Vec<github::LinkedPr>, String>) {
        self.gh.pending.remove(&format!("commit:{sha}"));
        self.gh.commit_prs.insert(sha, r.unwrap_or_default());
    }

    /// Forgets the pull requests of the branches (after creating one, or a refresh).
    pub(super) fn gh_forget_branches(&mut self) {
        self.gh.branch_prs.clear();
    }

    /// The pull request tab on pull request `n` of `repo`.
    pub(super) fn open_pr(&mut self, repo: PathBuf, n: u64) {
        self.pr.repo = Some(repo);
        self.open_pulls();
        self.load_pr(n);
    }

    /// The window to open a pull request of the current branch.
    pub(super) fn start_pr_create(&mut self) {
        let (Some(repo), Some(st)) = (self.repo.clone(), self.git.status.clone()) else {
            self.set_status(t!("Pas de dépôt git", "No git repository"));
            return;
        };
        let branch = st.branch.clone();
        if branch.is_empty() || branch.starts_with("HEAD") {
            self.set_status(t!("Pas de branche (HEAD détachée) : crée d'abord une branche", "No branch (detached HEAD): create a branch first"));
            return;
        }
        let remotes: Vec<(String, String)> = git::stdout(&repo, &["remote"])
            .unwrap_or_default()
            .lines()
            .filter(|r| !r.is_empty())
            .map(|r| {
                let url = git::stdout(&repo, &["remote", "get-url", r]).unwrap_or_default();
                (r.to_string(), git::parse_github(url.trim()).map(|(o, _)| o).unwrap_or_default())
            })
            .collect();
        // the remote it already follows, else "dev" (the fork of the odoo developers), else origin
        let upstream = st.upstream().and_then(|(u, _, _)| u.split_once('/').map(|(r, _)| r.to_string()));
        let remote = upstream
            .and_then(|u| remotes.iter().position(|(n, _)| *n == u))
            .or_else(|| remotes.iter().position(|(n, _)| n == "dev"))
            .or_else(|| remotes.iter().position(|(n, _)| n == "origin"))
            .unwrap_or(0);
        let title = git::stdout(&repo, &["log", "-1", "--format=%s"]).unwrap_or_default().trim().to_string();
        let body = git::stdout(&repo, &["log", "-1", "--format=%b"]).unwrap_or_default().trim().to_string();
        self.gh_base(&repo);
        self.gh.create = Some(PrCreate {
            repo,
            branch,
            title,
            body,
            bases: Vec::new(),
            base: String::new(),
            remotes,
            remote,
            draft: false,
            busy: false,
            error: None,
            base_remote: String::new(),
            writing: false,
        });
    }

    /// The description written by Claude goes in the box, if it is empty (else a message says so).
    pub(super) fn pr_body_ready(&mut self, r: Result<String, String>) {
        let Some(c) = self.gh.create.as_mut() else { return };
        c.writing = false;
        match r {
            Ok(text) if c.body.trim().is_empty() => c.body = text,
            Ok(_) => c.error = Some(t!("La description n'est pas vide : efface-la pour la faire rédiger par Claude", "The description is not empty: clear it to have Claude write it").into()),
            Err(e) => c.error = Some(e),
        }
    }

    pub(super) fn pr_created(&mut self, repo: PathBuf, r: Result<String, String>) {
        let Some(c) = self.gh.create.as_mut() else { return };
        c.busy = false;
        match r {
            Ok(url) => {
                self.gh.create = None;
                self.gh_forget_branches();
                self.git_refresh();
                self.set_status(tf!("Pull request créée : {url}", "Pull request created: {url}"));
                if let Some(n) = github::number_of(&url) {
                    self.open_pr(repo, n);
                }
            }
            Err(e) => c.error = Some(e),
        }
    }

    pub(super) fn pr_create_ui(&mut self, ctx: &egui::Context) {
        let Some(repo) = self.gh.create.as_ref().map(|c| c.repo.clone()) else { return };
        let base = self.gh.base.get(&repo).cloned().flatten();
        let accent = self.hl.colors.accent;
        let Some(c) = self.gh.create.as_mut() else { return };
        // the branches of the repository pull requests go to, once it is known
        if let (Some((full, default)), true) = (&base, c.bases.is_empty()) {
            let owner = full.split('/').next().unwrap_or("");
            let base_remote = c.remotes.iter().find(|(_, o)| o.eq_ignore_ascii_case(owner)).map(|(n, _)| n.clone());
            c.base_remote = base_remote.clone().unwrap_or_default();
            if let Some(r) = base_remote {
                let refs = git::stdout(&repo, &["for-each-ref", "--format=%(refname:short)", &format!("refs/remotes/{r}")]).unwrap_or_default();
                c.bases = refs.lines().filter_map(|l| l.strip_prefix(&format!("{r}/"))).filter(|b| *b != "HEAD").map(String::from).collect();
            }
            if c.bases.is_empty() {
                c.bases.push(default.clone());
            }
            c.bases.sort();
            c.base = github::guess_base(&c.branch, &c.bases, default);
        }
        let mut open = true;
        let (mut create, mut cancel, mut write) = (false, false, false);
        egui::Window::new(t!("Créer une pull request", "Create a pull request"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("pr_create_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label(RichText::new(t!("De", "From")).weak());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&c.branch).monospace().strong().color(accent));
                        ui.label(RichText::new(t!("poussée vers", "pushed to")).weak());
                        let label = c.remotes.get(c.remote).map(|(n, o)| format!("{n} ({o})")).unwrap_or_default();
                        egui::ComboBox::from_id_salt("pr_create_remote").selected_text(label).show_ui(ui, |ui| {
                            for (k, (n, o)) in c.remotes.iter().enumerate() {
                                ui.selectable_value(&mut c.remote, k, format!("{n} ({o})"));
                            }
                        });
                    });
                    ui.end_row();
                    ui.label(RichText::new(t!("Vers", "To")).weak());
                    ui.horizontal(|ui| match &base {
                        None => {
                            spinner(ui);
                            ui.label(RichText::new(t!("dépôt GitHub…", "GitHub repository…")).weak());
                        }
                        Some((full, _)) => {
                            ui.label(RichText::new(full).strong());
                            ui.label(RichText::new(t!("branche", "branch")).weak());
                            let current = c.bases.iter().position(|b| *b == c.base);
                            if let Some(k) = crate::pick::combo(ui, "pr_create_base", RichText::new(&c.base).monospace().color(accent), &c.bases, current) {
                                c.base = c.bases[k].clone();
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new(t!("Titre", "Title")).weak());
                    ui.add(egui::TextEdit::singleline(&mut c.title).desired_width(f32::INFINITY));
                    ui.end_row();
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Description").weak());
                        let can = base.is_some() && !c.base_remote.is_empty() && !c.writing && !c.busy;
                        if ui.add_enabled(can, egui::Button::new(t!("✳ Rédiger", "✳ Write")).small()).on_hover_text(t!("Claude rédige la description à partir des commits et du diff de la branche (si elle est vide)", "Claude writes the description from the commits and the diff of the branch (if it is empty)")).clicked() {
                            write = true;
                        }
                        if c.writing {
                            spinner(ui);
                        }
                    });
                    ui.add(egui::TextEdit::multiline(&mut c.body).desired_rows(8).desired_width(f32::INFINITY).hint_text("Markdown"));
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut c.draft, t!("Brouillon (draft)", "Draft"));
                    ui.end_row();
                });
                if let Some(e) = &c.error {
                    ui.add_space(4.0);
                    ui.label(RichText::new(e).color(err()));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let can = base.is_some() && !c.base.is_empty() && !c.title.trim().is_empty() && !c.busy;
                    if ui.add_enabled(can, filled_button(t!("Pousser et créer la PR", "Push and create the PR"), accent)).on_hover_text(t!("git push -u, puis gh pr create", "git push -u, then gh pr create")).clicked() {
                        create = true;
                    }
                    if ui.button(t!("Annuler", "Cancel")).clicked() {
                        cancel = true;
                    }
                    if c.busy {
                        spinner(ui);
                        ui.label(RichText::new(t!("push et création…", "pushing and creating…")).weak());
                    }
                });
            });
        if !open || cancel {
            self.gh.create = None;
            return;
        }
        if write {
            let Some(c) = self.gh.create.as_mut() else { return };
            c.writing = true;
            c.error = None;
            let (repo, base_ref) = (c.repo.clone(), format!("{}/{}", c.base_remote, c.base));
            self.jobs.spawn(move || {
                let r = git::run(&repo, &["log", "--format=- %s", &format!("{base_ref}..HEAD")]).and_then(|log| {
                    let diff = git::run(&repo, &["diff", &format!("{base_ref}...HEAD")])?;
                    crate::claude::pr_description(&repo, &log, &diff)
                });
                JobResult::PrBody(r)
            });
        }
        if create {
            let (Some(c), Some((full, _))) = (self.gh.create.as_mut(), base) else { return };
            let Some((remote, owner)) = c.remotes.get(c.remote).cloned() else { return };
            c.busy = true;
            c.error = None;
            // from a fork, GitHub wants "owner:branch"
            let head = if owner.eq_ignore_ascii_case(full.split('/').next().unwrap_or("")) || owner.is_empty() { c.branch.clone() } else { format!("{owner}:{}", c.branch) };
            let (repo, branch, b, title, body, draft) = (c.repo.clone(), c.branch.clone(), c.base.clone(), c.title.trim().to_string(), c.body.clone(), c.draft);
            self.set_status(tf!("git push {remote} {branch}, puis création de la pull request…", "git push {remote} {branch}, then creating the pull request…"));
            self.jobs.spawn(move || {
                let r = git::run(&repo, &["push", "-u", &remote, &branch]).and_then(|_| github::create(&repo, &full, &b, &head, &title, &body, draft));
                JobResult::PrCreated(repo.clone(), r)
            });
        }
    }
}
