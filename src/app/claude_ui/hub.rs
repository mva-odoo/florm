//! Every conversation in one Claude tab: the list on its left (what each does, its folder, how
//! long it has been working, what it cost), a click shows one, the others go on behind. A
//! conversation can still have its own tab (to put it beside a file), or the bottom panel.

use super::*;

/// `tab` of a conversation in the list of the Claude tab, not the one shown there.
pub(in crate::app) const LISTED: u64 = u64::MAX;

/// What a conversation is doing, for the list and the status bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::app) enum ConvState {
    Working,
    /// Its last answer asks questions: it waits for you.
    Waiting,
    /// It finished while you were elsewhere.
    Done,
    Idle,
}

/// What was done in the list.
pub(in crate::app) enum ListAct {
    Show(u64),
    New,
    Close(u64),
    Fork(u64),
    OwnTab(u64),
}

/// A conversation as the list shows it.
struct Row {
    id: u64,
    tab: Option<u64>,
    title: String,
    /// The project (repository or folder) it works in, the branch checked out there, its account
    /// (only with several accounts).
    project: String,
    branch: String,
    account: String,
    state: ConvState,
    /// What it waits for, as Claude Code says it (empty: not said).
    waiting: String,
    since: Option<Duration>,
    cost: f64,
    can_fork: bool,
}

/// A color of its own for each project, the same everywhere (list, header of the conversation):
/// two conversations of two projects are not mistaken for each other.
pub(in crate::app) fn project_color(name: &str, dark: bool) -> Color32 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    // hues far enough apart to be told apart at a glance
    const HUES: [f32; 8] = [0.58, 0.08, 0.36, 0.83, 0.0, 0.48, 0.70, 0.14];
    let hue = HUES[(h.finish() % HUES.len() as u64) as usize];
    let (sat, val) = if dark { (0.55, 0.92) } else { (0.75, 0.62) };
    egui::ecolor::Hsva::new(hue, sat, val, 1.0).into()
}

pub(in crate::app) fn conv_state(c: &ClaudeUi) -> ConvState {
    // Claude asks something: a permission, a question
    if c.term_waiting {
        ConvState::Waiting
    } else if c.busy {
        ConvState::Working
    } else if c.unseen {
        ConvState::Done
    } else {
        ConvState::Idle
    }
}

/// "12 s", "4 min", "1 h 05".
fn short_duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        _ => format!("{} h {:02}", s / 3600, s % 3600 / 60),
    }
}

impl App {
    /// All the conversations: the current one, then the others.
    fn all_convs(&self) -> impl Iterator<Item = &ClaudeUi> {
        std::iter::once(&self.claude).chain(self.claude_others.iter())
    }

    fn conv_mut(&mut self, id: u64) -> Option<&mut ClaudeUi> {
        std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()).find(|c| c.id == id)
    }

    /// `tab` is an open editor tab (not the bottom panel, nor only the list).
    pub(in crate::app) fn claude_in_open_tab(&self, tab: Option<u64>) -> bool {
        tab.is_some_and(|u| u != LISTED && self.docs.iter().any(|d| d.uid == u))
    }

    /// Puts `conv` in the tab of the current conversation, which goes on in the list.
    pub(in crate::app) fn claude_in_place(&mut self, mut conv: ClaudeUi) {
        conv.tab = self.claude.tab;
        let mut old = std::mem::replace(&mut self.claude, conv);
        old.tab = Some(LISTED);
        // an empty conversation is not kept in the list
        if !old.is_empty() {
            self.claude_others.push(old);
        }
        self.claude.unseen = false;
        self.claude.focus = true;
    }

    /// Keeps the time each conversation started working (for the list).
    pub(in crate::app) fn claude_track_busy(&mut self) {
        for c in std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()) {
            match (c.busy, c.busy_since) {
                (true, None) => c.busy_since = Some(Instant::now()),
                (false, Some(_)) => c.busy_since = None,
                _ => {}
            }
        }
    }

    /// The conversations working, waiting for you, and the first one finished and not seen yet
    /// (its id, and whether it is lit while it blinks), for the status bar.
    /// What the first conversation waiting for you waits for, as Claude Code says it.
    pub(in crate::app) fn claude_waiting_for(&self) -> Option<String> {
        self.all_convs().find(|c| conv_state(c) == ConvState::Waiting).map(|c| c.term_state.message.clone()).filter(|m| !m.is_empty())
    }

    pub(in crate::app) fn claude_states(&self) -> (usize, Vec<u64>, Option<(u64, bool)>) {
        let busy = self.all_convs().filter(|c| c.busy).count();
        let waiting = self.all_convs().filter(|c| conv_state(c) == ConvState::Waiting).map(|c| c.id).collect();
        let done = self.all_convs().find(|c| conv_state(c) == ConvState::Done).map(|c| (c.id, self.claude_lit(c)));
        (busy, waiting, done)
    }

    /// Shows the conversation `id` where it is: its tab, else in the Claude tab.
    pub(in crate::app) fn claude_go_to(&mut self, id: u64) {
        let Some(tab) = self.all_convs().find(|c| c.id == id).map(|c| c.tab) else { return };
        match tab.filter(|u| *u != LISTED && self.docs.iter().any(|d| d.uid == *u)) {
            Some(uid) => {
                if let Some(i) = self.docs.iter().position(|d| d.uid == uid) {
                    self.activate_tab(i);
                }
            }
            None => {
                // the Claude tab: the one of the current conversation, else any, else a new one
                let hub = self.claude.tab.filter(|u| self.docs.iter().any(|d| d.uid == *u)).or_else(|| self.docs.iter().find(|d| d.view == document::View::Claude).map(|d| d.uid));
                let hub = hub.unwrap_or_else(|| self.new_claude_tab("Claude".into()));
                // the one shown there goes back to the list, this one takes its place
                for c in std::iter::once(&mut self.claude).chain(self.claude_others.iter_mut()) {
                    if c.tab == Some(hub) {
                        c.tab = Some(LISTED);
                    }
                }
                if let Some(c) = self.conv_mut(id) {
                    c.tab = Some(hub);
                    c.focus = true;
                }
                if let Some(i) = self.docs.iter().position(|d| d.uid == hub) {
                    self.activate_tab(i);
                }
            }
        }
    }

    /// « Claude : conversation suivante qui attend » : the next one waiting for you (or finished).
    pub(in crate::app) fn claude_next_waiting(&mut self) {
        let mut ids: Vec<(u64, ConvState)> = self.all_convs().map(|c| (c.id, conv_state(c))).collect();
        ids.sort_by_key(|(id, _)| *id);
        let after = |want: ConvState| {
            let cands: Vec<u64> = ids.iter().filter(|(_, s)| *s == want).map(|(id, _)| *id).collect();
            cands.iter().find(|&&id| id > self.claude.id).or(cands.first()).copied()
        };
        match after(ConvState::Waiting).or_else(|| after(ConvState::Done)) {
            Some(id) => self.claude_go_to(id),
            None => self.set_status(t!("Aucune conversation n'attend ta réponse", "No conversation is waiting for your answer")),
        }
    }

    /// The list of the conversations, on the left of the Claude tab `hub`.
    pub(in crate::app) fn claude_list_ui(&self, ui: &mut egui::Ui, hub: u64) -> Option<ListAct> {
        let accent = self.hl.colors.accent;
        let weak = ui.visuals().weak_text_color();
        // with several accounts, each conversation says its own
        let accounts = self.claude_accounts();
        let mut rows: Vec<Row> = self
            .all_convs()
            .map(|c| {
                let project = c.dir.as_deref().map(|d| self.repo_label(d)).unwrap_or_default();
                // the branch checked out in its folder
                let branch = c.dir.as_deref().and_then(crate::stat_cache::head_branch).unwrap_or_default();
                let account = if accounts.len() > 1 { crate::claude_account::find(&accounts, &c.account).name.clone() } else { String::new() };
                let title = conv_title(c);
                Row {
                    id: c.id,
                    tab: c.tab,
                    title,
                    project,
                    branch,
                    account,
                    state: conv_state(c),
                    waiting: c.term_state.message.clone(),
                    since: c.busy_since.map(|t| t.elapsed()),
                    cost: c.cost,
                    can_fork: c.session_id.is_some(),
                }
            })
            .collect();
        // the newest at the top: its project is on its second line (headers per project took a
        // line each)
        rows.sort_by_key(|r| std::cmp::Reverse(r.id));
        let dark = ui.visuals().dark_mode;
        let mut act = None;
        // its header as high as the bar of the conversation, a line under both
        let size = egui::vec2(ui.available_width(), crate::app::TAB_STRIP_H);
        ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_size(size);
            ui.label(RichText::new(tf!("CONVERSATIONS ({})", "CONVERSATIONS ({})", rows.len())).small().strong().color(weak));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+").on_hover_text(t!("Nouvelle conversation : celle-ci continue dans la liste", "New conversation: this one goes on in the list")).clicked() {
                    act = Some(ListAct::New);
                }
            });
        });
        let line = ui.visuals().widgets.noninteractive.bg_stroke;
        ui.painter().hline(ui.clip_rect().x_range(), ui.cursor().top() + 0.5, line);
        ui.add_space(6.0);
        egui::ScrollArea::vertical().id_salt(("claude_list", hub)).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for r in &rows {
                let shown = r.tab == Some(hub);
                // in the row: its title, its state, its project and branch; the rest (account,
                // cost, where it is shown) in its tooltip
                let mut more: Vec<String> = Vec::new();
                if !r.account.is_empty() {
                    more.push(format!("👤 {}", r.account));
                }
                if r.cost > 0.0 {
                    more.push(format!("{:.2} $", r.cost));
                }
                match r.tab {
                    Some(LISTED) => {}
                    Some(u) if u == hub => {}
                    Some(_) => more.push(t!("dans son onglet", "in its tab").into()),
                    None => more.push(t!("dans le panneau du bas", "in the bottom panel").into()),
                }
                let project_name = if r.project.is_empty() { t!("(sans dossier)", "(no folder)").to_string() } else { r.project.clone() };
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 48.0), Sense::click());
                let v = ui.visuals();
                if shown {
                    ui.painter().rect_filled(rect, 6.0, crate::icons::mix(v.extreme_bg_color, accent, 0.12));
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 6.0, v.widgets.hovered.weak_bg_fill);
                }
                let p = ui.painter().with_clip_rect(rect.shrink2(egui::vec2(4.0, 2.0)));
                let close_zone = resp.hovered();
                // its state on the right of the title, where × shows on hover: working (and since
                // when), waiting for you, finished and not seen yet; nothing at rest
                let state = match r.state {
                    ConvState::Working => Some((r.since.map(|d| format!("⟳ {}", short_duration(d))).unwrap_or_else(|| "⟳".into()), accent)),
                    ConvState::Waiting => Some(("❓".to_string(), warn())),
                    ConvState::Done => Some(("●".to_string(), ok())),
                    ConvState::Idle => None,
                };
                let title_y = rect.top() + 7.0;
                let state_w = match state.as_ref().filter(|_| !close_zone) {
                    Some((s, c)) => p.text(egui::pos2(rect.right() - 10.0, title_y + 1.0), Align2::RIGHT_TOP, s, egui::FontId::proportional(12.0), *c).width() + 8.0,
                    None => 0.0,
                };
                let strong = r.state != ConvState::Idle || shown;
                let title_color = if strong { v.strong_text_color() } else { v.text_color() };
                let font = egui::FontId::proportional(13.5);
                let room = rect.width() - 12.0 - 10.0 - if close_zone { 24.0 } else { state_w };
                let mut title = r.title.clone();
                if ui.fonts_mut(|f| f.layout_no_wrap(title.clone(), font.clone(), title_color).size().x) > room {
                    while !title.is_empty() && ui.fonts_mut(|f| f.layout_no_wrap(format!("{title}…"), font.clone(), title_color).size().x) > room {
                        title.pop();
                    }
                    title = format!("{}…", title.trim_end());
                }
                p.text(egui::pos2(rect.left() + 12.0, title_y), Align2::LEFT_TOP, title, font, title_color);
                // its project: a dot in its color, the name and the branch faint
                let below = egui::pos2(rect.left() + 12.0, rect.top() + 27.0);
                p.circle_filled(below + egui::vec2(3.5, 7.5), 3.5, project_color(&r.project, dark));
                let line = if r.branch.is_empty() { project_name } else { format!("{project_name}  ·  {}", r.branch) };
                p.text(below + egui::vec2(12.0, 0.0), Align2::LEFT_TOP, line, egui::FontId::proportional(12.0), weak);
                // × on the hovered row: close it
                let mut on_close = false;
                if close_zone {
                    let x = egui::Rect::from_center_size(egui::pos2(rect.right() - 16.0, title_y + 8.0), egui::vec2(18.0, 18.0));
                    let xr = ui.interact(x, ui.scope_id().with(("conv_close", r.id)), Sense::click());
                    on_close = xr.hovered();
                    p.text(x.center(), Align2::CENTER_CENTER, "×", egui::FontId::proportional(14.0), if xr.hovered() { err() } else { weak });
                    if xr.on_hover_text(t!("Fermer cette conversation (son processus claude s'arrête ; elle reste dans l'historique)", "Close this conversation (its claude process stops; it stays in the history)")).clicked() {
                        act = Some(ListAct::Close(r.id));
                    }
                }
                let tip = match r.state {
                    ConvState::Working => t!("Claude travaille", "Claude is working"),
                    ConvState::Waiting if !r.waiting.is_empty() => r.waiting.as_str(),
                    ConvState::Waiting => t!("Claude attend ta réponse à ses questions", "Claude waits for your answer to its questions"),
                    ConvState::Done => t!("Claude a fini, pas encore vu", "Claude has finished, not seen yet"),
                    ConvState::Idle => t!("Au repos", "Idle"),
                };
                let tip = if more.is_empty() { tip.to_string() } else { format!("{tip}\n{}", more.join(" · ")) };
                let resp = resp.on_hover_text(tip);
                if resp.clicked() && !on_close {
                    act = Some(ListAct::Show(r.id));
                }
                resp.context_menu(|ui| {
                    if ui.add_enabled(r.can_fork, egui::Button::new(t!("⇉ Dupliquer", "⇉ Duplicate"))).on_hover_text(t!("Une copie qui repart de la dernière réponse : pour essayer autre chose sans perdre celle-ci", "A copy that starts again from the last answer: to try something else without losing this one")).on_disabled_hover_text(t!("Après une première réponse de Claude", "After a first answer of Claude")).clicked() {
                        act = Some(ListAct::Fork(r.id));
                        ui.close();
                    }
                    if r.tab == Some(LISTED) && ui.button(t!("⇱ Ouvrir dans son propre onglet", "⇱ Open in its own tab")).on_hover_text(t!("Pour la mettre à côté d'un fichier", "To put it beside a file")).clicked() {
                        act = Some(ListAct::OwnTab(r.id));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("× Fermer", "× Close")).clicked() {
                        act = Some(ListAct::Close(r.id));
                        ui.close();
                    }
                });
            }
        });
        act
    }

    /// What was chosen in the list of the Claude tab `hub` (its conversation is the current one,
    /// shown just before).
    pub(in crate::app) fn claude_list_act(&mut self, act: ListAct, hub: u64) {
        match act {
            ListAct::Show(id) if id == self.claude.id => self.claude.focus = true,
            ListAct::Show(id) => self.claude_go_to(id),
            ListAct::New => {
                let fresh = self.claude_fresh(Some(hub));
                if self.claude.tab == Some(hub) {
                    self.claude_in_place(fresh);
                    self.claude_prepare();
                } else {
                    self.claude_others.push(fresh);
                }
            }
            ListAct::Close(id) if id == self.claude.id => {
                // the next one of the list takes its place (else an empty one)
                let next = self.claude_others.iter().filter(|o| o.tab == Some(LISTED)).max_by_key(|o| o.id).map(|o| o.id);
                let mut repl = match next.and_then(|n| self.claude_others.iter().position(|o| o.id == n)) {
                    Some(k) => self.claude_others.remove(k),
                    None => self.claude_fresh(None),
                };
                repl.tab = self.claude.tab;
                repl.unseen = false;
                // dropped: its claude process stops (it was saved in the history as it went)
                drop(std::mem::replace(&mut self.claude, repl));
                self.claude_prepare();
            }
            ListAct::Close(id) => {
                let Some(k) = self.claude_others.iter().position(|o| o.id == id) else { return };
                match self.claude_others[k].tab.filter(|u| *u != LISTED).and_then(|u| self.docs.iter().position(|d| d.uid == u)) {
                    // its own tab: closed with it
                    Some(i) => self.close_tab(i, true),
                    None => drop(self.claude_others.remove(k)),
                }
            }
            ListAct::Fork(id) => {
                let Some(src) = self.all_convs().find(|c| c.id == id) else { return };
                let Some(session) = src.session_id.clone() else { return };
                let mut copy = self.claude_fresh(Some(LISTED));
                copy.dir = src.dir.clone();
                copy.wish = src.wish.clone();
                copy.given = src.given.clone();
                copy.context = src.context;
                (copy.account, copy.session_account) = (src.account.clone(), src.session_account.clone());
                // its `claude` starts with a copy of the session (`--fork-session`)
                copy.session_id = Some(session);
                copy.forked = true;
                copy.focus = true;
                let new_id = copy.id;
                self.claude_others.push(copy);
                self.claude_go_to(new_id);
            }
            ListAct::OwnTab(id) => {
                let Some(title) = self.all_convs().find(|c| c.id == id && c.tab == Some(LISTED)).map(conv_title) else { return };
                let uid = self.new_claude_tab(title);
                if let Some(c) = self.conv_mut(id) {
                    c.tab = Some(uid);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_of_a_conversation() {
        let mut c = ClaudeUi::default();
        assert_eq!(conv_state(&c), ConvState::Idle);
        c.unseen = true;
        assert_eq!(conv_state(&c), ConvState::Done);
        c.unseen = false;
        c.busy = true;
        assert_eq!(conv_state(&c), ConvState::Working);
        // a permission or a question: it waits for you
        c.term_waiting = true;
        assert_eq!(conv_state(&c), ConvState::Waiting);
        assert_eq!(short_duration(Duration::from_secs(3725)), "1 h 02");
        assert_eq!(short_duration(Duration::from_secs(42)), "42 s");
    }

    #[test]
    fn each_project_its_color() {
        // the same name, the same color; light and dark themes differ
        assert_eq!(project_color("shop", true), project_color("shop", true));
        assert_ne!(project_color("shop", true), project_color("shop", false));
        let names = ["shop", "billing", "florm", "themes", "api", "internal"];
        let colors: HashSet<[u8; 4]> = names.iter().map(|n| project_color(n, false).to_array()).collect();
        assert!(colors.len() >= 3, "{}", colors.len());
    }
}

#[cfg(test)]
mod glyph_tests {
    use eframe::egui;

    /// The symbols of the list are in the fonts built into Florm (a missing one shows as a box).
    #[test]
    fn symbols_of_the_list_exist() {
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::app::font_definitions());
        let _ = crate::run_ui(&ctx, Default::default(), |_| {});
        let id = egui::FontId::proportional(13.0);
        let missing: Vec<char> = "⟳❓✔·⇉+⇱×📄👤✏⇄■▶⚠".chars().filter(|c| !ctx.fonts_mut(|f| f.has_glyphs(&id, &c.to_string()))).collect();
        assert!(missing.is_empty(), "{missing:?}");
    }
}

