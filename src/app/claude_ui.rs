//! The Claude tab: Claude Code itself in a terminal (`crate::claude_term`), the list of the
//! conversations on its left (`hub`), its session on its right; what Florm gives Claude (lines,
//! files, pictures) is written into its field (`context`).

use super::*;
use crate::claude;
use crate::claude_account;

mod bar;
mod context;
mod hub;
mod session;
mod store;
mod term;
pub(in crate::app) use context::*;
pub(in crate::app) use hub::*;
pub(in crate::app) use store::*;

#[derive(Default)]
pub(super) struct ClaudeUi {
    /// Number of the conversation (0: the first one), and the editor tab showing it (document
    /// uid; None: the bottom panel).
    pub(super) id: u64,
    tab: Option<u64>,
    /// It finished while it was not shown: its tab is colored until seen (and blinks a few
    /// seconds after `done_at`).
    unseen: bool,
    done_at: Option<Instant>,
    /// Key of the conversation in the saved ones (0: not saved yet).
    saved_key: u64,
    /// Session of Claude Code, continued (`--resume`) when its terminal starts again (after a
    /// restart of Florm); `forked`: a copy of another conversation (« ⇉ Dupliquer »).
    pub(super) session_id: Option<String>,
    pub(super) forked: bool,
    /// Folder of the conversation (the selected repository, else the project).
    pub(super) dir: Option<PathBuf>,
    /// First message of the conversation: its title, and the wish behind the changes (for the
    /// commit message).
    pub(super) wish: Option<String>,
    /// What was given to it (files, lines…): « Reprendre la conversation de ce fichier ».
    pub(super) given: Vec<String>,
    pub(super) busy: bool,
    /// Since when it has been working (shown in the list of the conversations).
    pub(super) busy_since: Option<Instant>,
    cost: f64,
    /// Tokens of the conversation read by the model for its last answer, and its window.
    pub(super) context: (u64, u64),
    pub(super) focus: bool,
    /// Last commits of its folder for « + Contexte → Un commit »: (hash, short hash, subject, when).
    recent_commits: Vec<(String, String, String, String)>,
    /// The clipboard held an image when « + Contexte » was opened (« Image du presse-papiers »).
    clip_image: bool,
    /// Where it was drawn, and at which frame: an image dropped there is given to it.
    pub(super) drop_area: Option<(u64, egui::Rect)>,
    /// The Claude account it runs with (its name; empty: the first one), and the one its session
    /// (`session_id`) was made with: another one, it is copied there before going on.
    pub(super) account: String,
    pub(super) session_account: String,
    /// The account was chosen by hand (👤): the folders of the accounts no longer choose it.
    pub(super) account_chosen: bool,
    /// Its session in the terminals, the file where its hooks write (see `crate::claude_term`),
    /// what to start it with, what waits to be pasted once it is ready (and then Enter), whether
    /// it waits for you, and what its status line said last.
    pub(super) term: Option<u64>,
    pub(super) term_file: Option<PathBuf>,
    pub(super) term_prompt: Option<String>,
    pub(super) term_pending: String,
    pub(super) term_submit: bool,
    pub(super) term_waiting: bool,
    pub(super) term_state: crate::claude_term::State,
    /// Its `claude` is started again once it is done (a setting it starts with changed).
    pub(super) term_restart: bool,
    /// The files Claude changed in it (each once), and how much of the file listing them was read.
    pub(super) changed: Vec<PathBuf>,
    pub(super) changed_len: u64,
}

impl ClaudeUi {
    /// Its `claude` dropped (its terminal is closed with it): started again, going on with its
    /// session, the next time it is shown.
    pub(super) fn drop_term(&mut self) {
        self.term = None;
        self.term_file = None;
        self.term_state = Default::default();
        self.term_waiting = false;
        self.term_restart = false;
        // its next `claude` lists them in a file of its own (those of before stay in `changed`)
        self.changed_len = 0;
    }
}

impl ClaudeUi {
    /// Nothing asked yet. Its `claude` gives a session as soon as it starts (its status line),
    /// before any message: that one does not count, the folder can still be changed.
    pub(super) fn is_empty(&self) -> bool {
        self.wish.is_none()
    }
}

/// The configuration folder of the account chosen by default in `s`.
pub(in crate::app) fn default_account_dir(s: &config::Settings) -> Option<PathBuf> {
    let accounts = claude_account::accounts(&s.claude_accounts);
    claude_account::find(&accounts, &s.claude_account).dir.clone()
}

/// The image in the clipboard as PNG, if there is one.
fn clipboard_image() -> Option<Vec<u8>> {
    if let Some(img) = arboard::Clipboard::new().ok().and_then(|mut c| c.get_image().ok()) {
        return claude::png_of(img.width, img.height, img.bytes.into_owned()).ok();
    }
    // Wayland: the clipboard of the X11 side may not have it, wl-paste reads the Wayland one
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let types = crate::os::command("wl-paste").arg("--list-types").stderr(std::process::Stdio::null()).output().ok()?;
        let types = String::from_utf8_lossy(&types.stdout).into_owned();
        let kind = types.lines().find(|t| t.starts_with("image/"))?;
        let o = crate::os::command("wl-paste").args(["--no-newline", "--type", kind]).stderr(std::process::Stdio::null()).output().ok()?;
        return o.status.success().then(|| claude::png_of_file_bytes(&o.stdout).ok()).flatten();
    }
    None
}

/// The clipboard holds an image (its types only are read: nothing is decoded).
fn clipboard_has_image() -> bool {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let types = crate::os::command("wl-paste").arg("--list-types").stderr(std::process::Stdio::null()).output();
        if types.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).lines().any(|t| t.starts_with("image/"))) {
            return true;
        }
    }
    arboard::Clipboard::new().ok().is_some_and(|mut c| c.get_image().is_ok())
}

/// Whether `path` is a file Claude can be given: an image or a PDF.
pub(super) fn is_image_file(path: &Path) -> bool {
    claude::is_pdf(path) || path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&e.to_lowercase().as_str()))
}

/// Title of a conversation: its first message, short ("Claude" before it).
fn conv_title(c: &ClaudeUi) -> String {
    match c.wish.as_deref().and_then(|w| w.lines().find(|l| !l.trim().is_empty())) {
        Some(l) if l.chars().count() > 28 => format!("{}…", l.trim().chars().take(27).collect::<String>()),
        Some(l) => l.trim().to_string(),
        None => "Claude".into(),
    }
}

impl App {
    /// Shows Claude in the bottom panel, or in an editor tab ("claude" in `views_in_tab`).
    pub(super) fn open_claude(&mut self) {
        if self.settings.views_in_tab.iter().any(|k| k == "claude") {
            if self.bottom == Some(BottomTab::Claude) {
                self.bottom = None;
            }
            // the tab of the current conversation, else the first one (or a new one)
            match self.claude.tab.and_then(|u| self.docs.iter().position(|d| d.uid == u)) {
                Some(i) => self.activate_tab(i),
                None => self.open_view_tab(document::View::Claude, "Claude".into()),
            }
        } else {
            self.bottom = Some(BottomTab::Claude);
        }
        self.claude_prepare();
    }

    /// Shows Claude with `text` written in its field (not sent: to read and send), in `dir`
    /// when its conversation has not started yet.
    pub(super) fn claude_prefill(&mut self, dir: PathBuf, text: String) {
        if self.claude.term.is_none() && self.claude.is_empty() {
            self.claude.dir = Some(dir);
        }
        self.open_claude();
        self.claude_term_give(text, t!("La demande", "The request"));
    }

    /// Its folder and the keyboard, wherever Claude is shown.
    pub(super) fn claude_prepare(&mut self) {
        self.claude_restore_last();
        if self.claude.dir.is_none() {
            self.claude.dir = self.repo.clone().or_else(|| self.root.clone());
        }
        self.claude.focus = true;
    }

    /// Claude into an editor tab (⇱, more room) or back into the bottom panel (⇲), remembered.
    pub(super) fn move_claude(&mut self, to_tab: bool) {
        if to_tab {
            if self.claude.tab.is_none() {
                let uid = self.new_claude_tab(conv_title(&self.claude));
                self.claude.tab = Some(uid);
            }
        } else if let Some(uid) = self.claude.tab.take() {
            // the conversation of the bottom panel until now takes its tab
            match self.claude_others.iter_mut().find(|o| o.tab.is_none()) {
                Some(o) => o.tab = Some(uid),
                None => {
                    if let Some(i) = self.docs.iter().position(|d| d.uid == uid) {
                        self.close_tab(i, true);
                    }
                }
            }
        }
        let list = &mut self.settings.views_in_tab;
        list.retain(|k| k != "claude");
        if to_tab {
            list.push("claude".into());
        }
        let v = config::toml_str_array(list);
        self.persist("views_in_tab", v);
        self.open_claude();
    }

    /// A new editor tab for a conversation; returns its uid.
    fn new_claude_tab(&mut self, title: String) -> u64 {
        let uid = self.next_uid();
        let mut d = Document::new(uid, self.settings.tab_size, self.settings.translate_tabs_to_spaces);
        d.view = document::View::Claude;
        d.read_only = true;
        d.title_override = Some(title);
        self.docs.push(d);
        self.activate_tab(self.docs.len() - 1);
        uid
    }

    /// Every conversation, for the ▾ menu of the bottom panel and the palette: its id, its tab
    /// (None: the panel's), its title, working, finished and not seen yet. The panel's first,
    /// then the tabs in their order, then the list of the Claude tab.
    pub(super) fn claude_conversations(&self) -> Vec<(u64, Option<u64>, String, bool, bool)> {
        let mut all: Vec<&ClaudeUi> = std::iter::once(&self.claude).chain(self.claude_others.iter()).collect();
        let order = |c: &&ClaudeUi| c.tab.map_or(0, |u| 1 + self.docs.iter().position(|d| d.uid == u).unwrap_or(usize::MAX - 1));
        all.sort_by_key(order);
        all.into_iter()
            .map(|c| {
                let title = c.tab.and_then(|u| self.docs.iter().find(|d| d.uid == u)).and_then(|d| d.title_override.clone()).unwrap_or_else(|| conv_title(c));
                (c.id, c.tab, title, c.busy, c.unseen && !c.busy)
            })
            .collect()
    }

    /// The folder Claude works in, for the conversation not started yet.
    pub(super) fn claude_set_dir(&mut self, dir: PathBuf) {
        if !self.claude.is_empty() {
            self.set_error(t!("Pour changer de dossier : « Nouvelle conversation »", "To change the folder: “New conversation”"));
            return;
        }
        if !dir.is_dir() {
            self.set_error(tf!("Dossier introuvable : {}", "Folder not found: {}", dir.display()));
            return;
        }
        self.claude.dir = Some(crate::os::canon(&dir));
        // its `claude` started in the other folder: started again in this one, with a session of
        // its own (Claude Code keeps them by folder)
        self.claude_term_stop();
        self.claude.session_id = None;
    }

    /// An empty conversation with the folder of the current one, shown in `tab`.
    fn claude_fresh(&self, tab: Option<u64>) -> ClaudeUi {
        static N: AtomicU64 = AtomicU64::new(1);
        ClaudeUi {
            id: N.fetch_add(1, Ordering::Relaxed),
            tab,
            dir: self.claude.dir.clone(),
            // the default one; its folder may choose another (`claude_sync_account`)
            account: self.settings.claude_account.clone(),
            focus: true,
            ..Default::default()
        }
    }

    /// The Claude accounts of the settings.
    pub(in crate::app) fn claude_accounts(&self) -> Vec<claude_account::Account> {
        claude_account::accounts(&self.settings.claude_accounts)
    }

    /// A conversation not started yet takes the account of its folder (Préférences → Comptes
    /// Claude → its folders), unless one was chosen by hand.
    pub(in crate::app) fn claude_sync_account(&mut self) {
        let c = &self.claude;
        if c.account_chosen || c.session_id.is_some() || c.term.is_some() {
            return;
        }
        let Some(dir) = c.dir.as_deref() else { return };
        if let Some(a) = claude_account::for_folder(&self.settings.claude_account_folders, dir) {
            if self.claude_accounts().iter().any(|x| x.name == a) {
                self.claude.account = a;
            }
        }
    }

    /// The key of the quotas of account `name`: empty for the first one.
    pub(in crate::app) fn claude_limits_key(&self, name: &str) -> String {
        let accounts = self.claude_accounts();
        let a = claude_account::find(&accounts, name);
        if a.name == accounts[0].name { String::new() } else { a.name.clone() }
    }

    /// The configuration folder of the account named `name` (None: the usual one).
    pub(super) fn claude_account_dir(&self, name: &str) -> Option<PathBuf> {
        claude_account::find(&self.claude_accounts(), name).dir.clone()
    }

    /// The current conversation goes on with the account `name`: its session copied there, its
    /// `claude` started again with it.
    pub(in crate::app) fn claude_set_account(&mut self, name: String) {
        if self.claude.account == name {
            return;
        }
        self.claude.account_chosen = true;
        self.claude.account = name;
        self.claude_session_to_account();
        self.claude_term_stop();
        self.claude_save();
    }

    /// The session of the current conversation copied to its account, when it was made with
    /// another one (else `claude --resume` would not find it there).
    pub(in crate::app) fn claude_session_to_account(&mut self) {
        let c = &self.claude;
        let Some(session) = c.session_id.clone().filter(|_| c.session_account != c.account) else { return };
        let (from, to) = (self.claude_account_dir(&c.session_account), self.claude_account_dir(&c.account));
        let name = c.account.clone();
        if let Err(e) = claude_account::copy_session(from.as_deref(), to.as_deref(), &session) {
            self.log(tf!("Claude : session non copiée vers le compte {name} : {e}", "Claude: session not copied to the account {name}: {e}"));
            self.claude.session_id = None;
        }
        self.claude.session_account = self.claude.account.clone();
    }

    /// A new conversation in `dir`, in a new tab; the current one goes on in its place.
    pub(super) fn claude_new_in(&mut self, dir: PathBuf) {
        let mut fresh = self.claude_fresh(None);
        fresh.dir = Some(dir);
        self.claude_add(fresh);
    }

    /// « Nouvelle conversation »: in a new tab, the current one goes on in its place.
    fn claude_new_conversation(&mut self) {
        if self.claude.is_empty() {
            self.claude.focus = true;
            return;
        }
        let fresh = self.claude_fresh(None);
        self.claude_add(fresh);
    }

    /// `fresh` made the current conversation: in the Claude tab (the current one goes on in its
    /// list), else in a new tab.
    fn claude_add(&mut self, mut fresh: ClaudeUi) {
        if self.claude_in_open_tab(self.claude.tab) {
            self.claude_in_place(fresh);
            self.claude_prepare();
            return;
        }
        fresh.tab = Some(self.new_claude_tab("Claude".into()));
        let old = std::mem::replace(&mut self.claude, fresh);
        self.claude_others.push(old);
        if self.bottom == Some(BottomTab::Claude) && !self.claude_has_bottom() {
            self.bottom = None;
        }
        self.claude_prepare();
    }

    /// A conversation is meant for the bottom panel.
    pub(super) fn claude_has_bottom(&self) -> bool {
        self.claude.tab.is_none() || self.claude_others.iter().any(|o| o.tab.is_none())
    }

    /// The conversation of `tab` (None: the bottom panel), made the current one, and shown.
    pub(super) fn claude_show(&mut self, ui: &mut egui::Ui, tab: Option<u64>) {
        self.claude_restore_last();
        self.claude_sync_account();
        if self.claude.tab != tab {
            if let Some(k) = self.claude_others.iter().position(|o| o.tab == tab) {
                std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
            } else if tab.is_some() && self.claude.tab == Some(LISTED) {
                // one Claude tab (opened again): the current one of the list comes back in it
                self.claude.tab = tab;
            } else if let Some(k) = self.claude_others.iter().enumerate().filter(|(_, o)| tab.is_some() && o.tab == Some(LISTED)).max_by_key(|(_, o)| o.id).map(|(k, _)| k) {
                self.claude_others[k].tab = tab;
                std::mem::swap(&mut self.claude, &mut self.claude_others[k]);
            } else if tab.is_some() && self.claude.tab.is_none() && self.bottom != Some(BottomTab::Claude) {
                // a Claude tab without a conversation (from the menu, a restored session): the
                // current one, shown nowhere, goes there
                self.claude.tab = tab;
            } else {
                let fresh = self.claude_fresh(tab);
                let old = std::mem::replace(&mut self.claude, fresh);
                self.claude_others.push(old);
                self.claude_prepare();
            }
        }
        self.claude.unseen = false;
        // one tab for every conversation: it is named after the one shown
        if let Some(uid) = tab {
            let title = conv_title(&self.claude);
            if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid && d.title_override.as_deref() != Some(title.as_str())) {
                d.title_override = Some(title);
            }
        }
        self.claude_ui(ui, tab.is_some());
    }

    /// What every conversation's hooks said; those whose tab was closed are over.
    pub(super) fn claude_poll_all(&mut self) {
        self.claude_term_tick();
        self.claude_track_busy();
        let docs = &self.docs;
        let alive = |c: &ClaudeUi| c.tab.map_or(true, |u| u == LISTED || docs.iter().any(|d| d.uid == u));
        // one Claude tab: a closed tab leaves its conversation in the list (× in the list stops it),
        // unless it is empty
        for c in std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()) {
            if !alive(c) && !c.is_empty() {
                c.tab = Some(LISTED);
            }
        }
        self.claude_others.retain(alive);
        if !alive(&self.claude) {
            self.claude = match self.claude_others.iter().position(|o| o.tab.is_none()) {
                Some(k) => self.claude_others.remove(k),
                None => self.claude_fresh(None),
            };
        }
    }

    /// Color of the title of a Claude tab: working (accent), finished and not seen yet (green,
    /// blinking a few seconds first).
    pub(super) fn claude_tab_tint(&self, uid: u64) -> Option<Color32> {
        let c = std::iter::once(&self.claude).chain(self.claude_others.iter()).find(|c| c.tab == Some(uid))?;
        if c.busy {
            Some(self.hl.colors.accent)
        } else if c.unseen {
            self.claude_lit(c).then(ok)
        } else {
            None
        }
    }

    /// The whole tab of a conversation lit while it blinks, just after it finished.
    pub(super) fn claude_tab_flash(&self, uid: u64) -> Option<Color32> {
        let c = std::iter::once(&self.claude).chain(self.claude_others.iter()).find(|c| c.tab == Some(uid))?;
        let blinking = c.unseen && !c.busy && self.settings.claude_done_blink && c.done_at.is_some_and(|t| t.elapsed() < Duration::from_secs(8));
        (blinking && self.claude_lit(c)).then(ok)
    }

    /// The conversation of the bottom panel finished and is not seen yet: its tab and its icon
    /// light up (blinking a few seconds first).
    pub(super) fn claude_bottom_done(&self) -> bool {
        std::iter::once(&self.claude).chain(self.claude_others.iter()).any(|c| c.tab.is_none() && c.unseen && !c.busy && self.claude_lit(c))
    }

    /// On or off: blinking for a few seconds after the end of the answer, then on.
    fn claude_lit(&self, c: &ClaudeUi) -> bool {
        match c.done_at.map(|t| t.elapsed()) {
            Some(e) if self.settings.claude_done_blink && e < Duration::from_secs(8) => {
                self.egui_ctx.request_repaint_after(Duration::from_millis(100));
                e.as_millis() / 400 % 2 == 0
            }
            _ => true,
        }
    }

    /// The last commits of the folder of conversation `id`, for « + Contexte → Un commit ».
    pub(super) fn claude_commits_loaded(&mut self, id: u64, out: String) {
        let Some(c) = std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).find(|c| c.id == id) else { return };
        c.recent_commits = out
            .lines()
            .filter_map(|l| {
                let mut f = l.split('\x1f');
                Some((f.next()?.to_string(), f.next()?.to_string(), f.next()?.to_string(), f.next()?.to_string()))
            })
            .collect();
    }

    /// The Claude tab: the bar at the top, the session on the right, the terminal of `claude`.
    pub(super) fn claude_ui(&mut self, ui: &mut egui::Ui, in_tab: bool) {
        if self.install_guide(ui, crate::tools::Tool::Claude, in_tab) {
            return;
        }
        // Claude Code itself in a terminal, started the first time its conversation is shown
        self.claude_term_start();
        let dir_name = self.claude.dir.as_deref().map(|d| self.repo_label(d)).unwrap_or_default();
        // the repositories of the opened folder, then the recent projects: any of them, not only a repository
        let mut dirs: Vec<(PathBuf, String)> = self.repos.iter().map(|r| (r.clone(), self.repo_label(r))).collect();
        for p in self.projects.iter().filter(|p| p.path.is_dir()) {
            if !dirs.iter().any(|(d, _)| *d == p.path) {
                dirs.push((p.path.clone(), format!("{}  ·  {}", p.name, p.path.display())));
            }
        }
        if let Some(d) = self.claude.dir.as_ref().filter(|d| !dirs.iter().any(|(x, _)| x == *d)) {
            dirs.insert(0, (d.clone(), d.display().to_string()));
        }
        // an image file dropped on it is given to it (seen at the next frame)
        self.claude.drop_area = Some((ui.ctx().cumulative_pass_nr(), ui.max_rect()));
        // the kept conversations: (key, title, folder and date)
        let history: Vec<(u64, String, String)> = self
            .claude_saved
            .iter()
            .map(|s| {
                let place = s.dir.as_deref().map(|d| self.repo_label(d)).unwrap_or_default();
                (s.key, s.title.clone(), format!("{place} · {}", git::ago(s.updated)))
            })
            .collect();
        let accounts = self.claude_accounts();
        let account_name = claude_account::find(&accounts, &self.claude.account).name.clone();
        let no_limits = Vec::new();
        let limits = self.claude_limits.get(&self.claude_limits_key(&self.claude.account)).unwrap_or(&no_limits);
        let branch = self.claude.dir.as_deref().and_then(git::head_branch);
        let accent = self.hl.colors.accent;
        let (term, colors, light) = (&mut self.term, self.hl.colors, self.hl.is_light());
        let c = &mut self.claude;
        let d = bar::BarData { dirs: &dirs, dir_name: &dir_name, history: &history, accounts: &accounts, account_name: &account_name, limits, in_tab, side: self.settings.claude_review, listed: self.claude_list_shown };
        let a = bar::bar(ui, c, accent, d);
        let side = a.side.unwrap_or(self.settings.claude_review);
        let mut open_file = None;
        if side {
            egui::SidePanel::right(egui::Id::new("claude_session").with(c.id)).resizable(true).default_width(230.0).width_range(200.0..=420.0).show_inside(ui, |ui| {
                open_file = session::session_ui(ui, c, &dir_name, branch.as_deref(), limits, accent);
            });
        }
        egui::CentralPanel::default().frame(egui::Frame::none().fill(colors.bg)).show_inside(ui, |ui| match c.term {
            Some(id) => term.embedded_ui(ui, id, std::mem::take(&mut c.focus), colors, light),
            None => {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(t!("Choisis un dossier en haut pour lancer Claude.", "Choose a folder at the top to start Claude.")).weak()));
            }
        });
        self.claude_apply(a);
        if let Some(p) = open_file {
            self.open_path(p);
        }
    }

    /// What the bar asked for, done now that `self` is free.
    fn claude_apply(&mut self, a: bar::Acts) {
        if let Some(t) = a.move_to {
            self.move_claude(t);
        }
        if let Some(side) = a.side {
            self.settings.claude_review = side;
            self.persist("claude_review", side.to_string());
        }
        if let Some(name) = a.account {
            self.set_status(tf!("Compte Claude : {name}", "Claude account: {name}"));
            self.claude_set_account(name);
        }
        if a.manage_accounts {
            self.prefs.section = PrefSection::Claude;
            self.open_settings();
        }
        if a.read_commits {
            if let Some(dir) = self.claude.dir.clone() {
                let id = self.claude.id;
                self.jobs.spawn(move || JobResult::ClaudeCommits(id, git::stdout(&dir, &["log", "-40", "--format=%H%x1f%h%x1f%s%x1f%cr"]).unwrap_or_default()));
            }
        }
        if a.shot {
            self.screenshot_for_claude(Duration::ZERO);
        }
        if let Some(w) = a.ctx_add {
            self.claude_add_context(w);
        }
        if a.clipboard {
            match clipboard_image() {
                Some(png) => self.claude_attach_image(png, t!("Image collée", "Pasted image").into()),
                None => self.set_status(t!("Pas d'image dans le presse-papiers", "No image in the clipboard")),
            }
        }
        if let Some(k) = a.open_saved {
            self.claude_open_saved(k);
        }
        if let Some(k) = a.forget_saved {
            self.claude_forget_saved(k);
        }
        if let Some(d) = a.dir {
            self.claude_set_dir(d);
        }
        if a.pick_dir {
            self.file_dialog(crate::os::Pick::ClaudeDir);
        }
        if a.new_conv {
            self.claude_new_conversation();
        }
        if a.restart {
            self.claude_term_stop();
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
            let r = if diff.trim().is_empty() { Err(t!("Rien à décrire : aucun changement", "Nothing to describe: no change").to_string()) } else { claude::commit_message(&repo, &diff, &recent, &wish) };
            JobResult::CommitMessage(r)
        });
    }

    pub(super) fn commit_message_ready(&mut self, r: Result<String, String>) {
        self.git.suggesting = false;
        match r {
            Ok(m) => {
                self.git.commit_msg = m;
                self.set_status(t!("Message de commit proposé par Claude — à relire", "Commit message suggested by Claude — to read over"));
            }
            Err(e) => self.set_error(tf!("Pas de message proposé : {}", "No message suggested: {}", e.lines().next().unwrap_or(""))),
        }
    }
}

/// A small gauge (right to left: the bar, then its label), red when nearly full.
fn gauge(ui: &mut egui::Ui, label: &str, used: f32, accent: Color32, tip: impl FnOnce(&mut egui::Ui)) {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(46.0, 14.0), Sense::hover());
    gauge_bar(ui, rect, used, accent);
    let text = ui.label(RichText::new(format!("{label} {:.0} %", used * 100.0)).small().weak());
    r.union(text).on_hover_ui(tip);
    ui.add_space(6.0);
}

/// The bar of a gauge in `rect`: `used` (0 to 1) filled, orange then red when nearly full.
fn gauge_bar(ui: &egui::Ui, rect: egui::Rect, used: f32, accent: Color32) {
    let color = if used >= 0.9 { err() } else if used >= 0.7 { warn() } else { accent };
    let bar = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 6.0));
    ui.painter().rect_filled(bar, 3.0, ui.visuals().widgets.inactive.bg_fill);
    ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * used.max(0.0), bar.height())), 3.0, color);
}

/// Every gauge (label, used or -1 when unknown, details) one under the other, for a tooltip.
fn gauges_tip(ui: &mut egui::Ui, gauges: &[(String, f32, String)], accent: Color32) {
    ui.set_max_width(420.0);
    for (k, (label, used, tip)) in gauges.iter().enumerate() {
        if k > 0 {
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            ui.add_sized([64.0, 14.0], egui::Label::new(RichText::new(label).strong()));
            let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 14.0), Sense::hover());
            gauge_bar(ui, rect, *used, accent);
            let pct = if *used < 0.0 { "– %".to_string() } else { format!("{:.0} %", used * 100.0) };
            ui.label(RichText::new(pct).small());
        });
        ui.label(RichText::new(tip).small().weak());
    }
}

/// "dans 2 h 05", "dans 3 j" until a unix time.
fn until(ts: i64) -> String {
    let s = (ts - now_secs()).max(0);
    match s {
        0..=59 => t!("maintenant", "now").into(),
        60..=3599 => tf!("dans {} min", "in {} min", s / 60),
        3600..=86_399 => tf!("dans {} h {:02}", "in {} h {:02}", s / 3600, s % 3600 / 60),
        _ => tf!("dans {} j {} h", "in {} d {} h", s / 86_400, s % 86_400 / 3600),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_titles() {
        assert_eq!(thousands(1234567), "1 234 567");
        assert_eq!(thousands(12), "12");
        let mut c = ClaudeUi::default();
        assert_eq!(conv_title(&c), "Claude");
        assert!(c.is_empty());
        // a session given by `claude` at its start: nothing asked yet
        c.session_id = Some("s1".into());
        assert!(c.is_empty());
        c.wish = Some("\nCorrige le bug de la vue Git qui ne montre rien".into());
        assert_eq!(conv_title(&c), "Corrige le bug de la vue Gi…");
        assert!(!c.is_empty());
    }

    /// On a small screen the Claude tab stays in the window: the bar wraps instead of pushing
    /// the rest past its right edge.
    #[test]
    fn claude_fits_a_small_window() {
        let dir = std::env::temp_dir().join(format!("florm_claude_narrow_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        crate::tools::Tool::Claude.pretend_installed();
        let width = 560.0;
        let mut right = 0.0_f32;
        for frame in 0..3 {
            if frame == 1 {
                app.claude.wish = Some("Corrige le bug".into());
                app.claude.busy = true;
                app.claude.cost = 1.25;
                app.claude.context = (150_000, 200_000);
            }
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 700.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.claude_show(ui, None);
                    right = ui.min_rect().right();
                });
            });
        }
        assert!(right <= width + 0.5, "the Claude tab goes to x = {right}, past the window ({width})");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
