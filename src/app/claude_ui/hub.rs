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
    place: String,
    state: ConvState,
    since: Option<Duration>,
    cost: f64,
    can_fork: bool,
}

pub(in crate::app) fn conv_state(c: &ClaudeUi) -> ConvState {
    if c.busy {
        ConvState::Working
    } else if waits(c) {
        ConvState::Waiting
    } else if c.unseen {
        ConvState::Done
    } else {
        ConvState::Idle
    }
}

/// Its last answer, since your last message, asks questions (with options to tick).
fn waits(c: &ClaudeUi) -> bool {
    for it in c.items.iter().rev() {
        match it {
            Item::User(..) => return false,
            Item::Text(t) => return !claude_ask::parse(t).1.is_empty(),
            _ => {}
        }
    }
    false
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

    /// One Claude tab for every conversation (the preference), and `tab` is a real tab.
    pub(in crate::app) fn claude_one_tab_at(&self, tab: Option<u64>) -> bool {
        self.settings.claude_one_tab && tab.is_some_and(|u| u != LISTED && self.docs.iter().any(|d| d.uid == u))
    }

    /// Puts `conv` in the tab of the current conversation, which goes on in the list.
    pub(in crate::app) fn claude_in_place(&mut self, mut conv: ClaudeUi) {
        conv.tab = self.claude.tab;
        let mut old = std::mem::replace(&mut self.claude, conv);
        old.tab = Some(LISTED);
        // an empty conversation is not kept in the list
        let empty = old.session.is_none() && old.items.iter().all(|i| matches!(i, Item::Info(_)));
        if !empty {
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
    pub(in crate::app) fn claude_states(&self) -> (usize, Vec<u64>, Option<(u64, bool)>) {
        let busy = self.all_convs().filter(|c| c.busy).count();
        let waiting = self.all_convs().filter(|c| conv_state(c) == ConvState::Waiting).map(|c| c.id).collect();
        let done = self.all_convs().find(|c| conv_state(c) == ConvState::Done).map(|c| (c.id, self.claude_lit(c)));
        (busy, waiting, done)
    }

    /// Shows the conversation `id` where it is: its tab, the bottom panel, or the Claude tab when
    /// it is in its list.
    pub(in crate::app) fn claude_go_to(&mut self, id: u64) {
        let Some(tab) = self.all_convs().find(|c| c.id == id).map(|c| c.tab) else { return };
        match tab {
            Some(LISTED) => {
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
            Some(uid) => match self.docs.iter().position(|d| d.uid == uid) {
                Some(i) => self.activate_tab(i),
                None => self.open_claude(),
            },
            None => self.show_bottom(BottomTab::Claude, false),
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
                let base = c.dir.as_deref().map(|d| self.repo_label(d)).unwrap_or_default();
                let mut place = match &c.worktree {
                    Some(w) => format!("🌿 {}", w.branch),
                    None => base,
                };
                if accounts.len() > 1 {
                    place = format!("👤 {} · {place}", crate::claude_account::find(&accounts, &c.account).name);
                }
                let title = if c.forked { format!("⇉ {}", conv_title(c)) } else { conv_title(c) };
                Row {
                    id: c.id,
                    tab: c.tab,
                    title,
                    place,
                    state: conv_state(c),
                    since: c.busy_since.map(|t| t.elapsed()),
                    cost: c.cost,
                    can_fork: c.session_id.is_some() && c.last_uuid.is_some(),
                }
            })
            .collect();
        // the newest at the top
        rows.sort_by_key(|r| std::cmp::Reverse(r.id));
        let mut act = None;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tf!("CONVERSATIONS ({})", "CONVERSATIONS ({})", rows.len())).small().strong().color(weak));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+").on_hover_text(t!("Nouvelle conversation : celle-ci continue dans la liste", "New conversation: this one goes on in the list")).clicked() {
                    act = Some(ListAct::New);
                }
            });
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt(("claude_list", hub)).auto_shrink([false, false]).show(ui, |ui| {
            for r in &rows {
                let shown = r.tab == Some(hub);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), Sense::click());
                let v = ui.visuals();
                if shown {
                    // a light tint of the accent: the folder and the cost stay readable on it
                    ui.painter().rect_filled(rect, 6.0, crate::icons::mix(v.panel_fill, accent, 0.14));
                    ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 8.0), egui::vec2(3.0, rect.height() - 16.0)), 1.5, accent);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 6.0, v.widgets.hovered.weak_bg_fill);
                }
                let (icon, color) = match r.state {
                    ConvState::Working => ("⟳", accent),
                    ConvState::Waiting => ("❓", warn()),
                    ConvState::Done => ("✔", ok()),
                    ConvState::Idle => ("·", weak),
                };
                let p = ui.painter().with_clip_rect(rect.shrink2(egui::vec2(4.0, 2.0)));
                p.text(rect.left_top() + egui::vec2(10.0, 8.0), Align2::LEFT_TOP, icon, egui::FontId::proportional(13.0), color);
                let strong = r.state != ConvState::Idle || shown;
                let title_color = if strong { v.strong_text_color() } else { v.text_color() };
                p.text(rect.left_top() + egui::vec2(28.0, 7.0), Align2::LEFT_TOP, &r.title, egui::FontId::proportional(13.5), title_color);
                // where it is and what it costs; how long it has been working
                let mut info = r.place.clone();
                if let Some(d) = r.since {
                    info = format!("{info} · {}", short_duration(d));
                }
                if r.cost > 0.0 {
                    info = format!("{info} · {:.2} $", r.cost);
                }
                match r.tab {
                    Some(LISTED) => {}
                    Some(u) if u == hub => {}
                    Some(_) => info = format!("{info} · {}", t!("son onglet", "its tab")),
                    None => info = format!("{info} · {}", t!("panneau du bas", "bottom panel")),
                }
                p.text(rect.left_top() + egui::vec2(28.0, 26.0), Align2::LEFT_TOP, info, egui::FontId::proportional(11.5), weak);
                // × on the hovered row: close it
                let mut on_close = false;
                if resp.hovered() || shown {
                    let x = egui::Rect::from_center_size(egui::pos2(rect.right() - 14.0, rect.top() + 15.0), egui::vec2(18.0, 18.0));
                    let xr = ui.interact(x, ui.id().with(("conv_close", r.id)), Sense::click());
                    on_close = xr.hovered();
                    p.text(x.center(), Align2::CENTER_CENTER, "×", egui::FontId::proportional(14.0), if xr.hovered() { err() } else { weak });
                    if xr.on_hover_text(t!("Fermer cette conversation (son processus claude s'arrête ; elle reste dans l'historique)", "Close this conversation (its claude process stops; it stays in the history)")).clicked() {
                        act = Some(ListAct::Close(r.id));
                    }
                }
                let tip = match r.state {
                    ConvState::Working => t!("Claude travaille", "Claude is working"),
                    ConvState::Waiting => t!("Claude attend ta réponse à ses questions", "Claude waits for your answer to its questions"),
                    ConvState::Done => t!("Claude a fini, pas encore vu", "Claude has finished, not seen yet"),
                    ConvState::Idle => t!("Au repos", "Idle"),
                };
                let resp = resp.on_hover_text(tip);
                if resp.clicked() && !on_close {
                    act = Some(ListAct::Show(r.id));
                }
                resp.context_menu(|ui| {
                    if ui.add_enabled(r.can_fork, egui::Button::new(t!("⇉ Dupliquer", "⇉ Duplicate"))).on_hover_text(t!("Une copie qui repart de la dernière réponse : pour essayer autre chose sans perdre celle-ci", "A copy that starts again from the last answer: to try something else without losing this one")).on_disabled_hover_text(t!("Après une première réponse de Claude", "After a first answer of Claude")).clicked() {
                        act = Some(ListAct::Fork(r.id));
                        ui.close_menu();
                    }
                    if r.tab == Some(LISTED) && ui.button(t!("⇱ Ouvrir dans son propre onglet", "⇱ Open in its own tab")).on_hover_text(t!("Pour la mettre à côté d'un fichier", "To put it beside a file")).clicked() {
                        act = Some(ListAct::OwnTab(r.id));
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(t!("× Fermer", "× Close")).clicked() {
                        act = Some(ListAct::Close(r.id));
                        ui.close_menu();
                    }
                });
                ui.add_space(2.0);
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
                let (Some(session), Some(at)) = (src.session_id.clone(), src.last_uuid.clone()) else { return };
                let mut copy = self.claude_fresh(Some(LISTED));
                copy.dir = src.dir.clone();
                copy.model = src.model;
                copy.mode = src.mode;
                copy.add_dirs = src.add_dirs.clone();
                copy.items = src.items.clone();
                copy.wish = src.wish.clone();
                copy.context = src.context;
                // the next message starts a copy of its session after its last answer
                copy.session_id = Some(session);
                copy.resume_at = Some(at);
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
        c.items.push(Item::User("x".into(), Vec::new(), Vec::new(), Vec::new()));
        c.items.push(Item::Text("Fini.".into()));
        c.unseen = true;
        assert_eq!(conv_state(&c), ConvState::Done);
        // its answer asks a question: it waits for you, until your next message
        c.items.push(Item::Text("[[QUESTION]] Où ?\n- ici\n- là\n[[/QUESTION]]".into()));
        assert_eq!(conv_state(&c), ConvState::Waiting);
        c.items.push(Item::User("ici".into(), Vec::new(), Vec::new(), Vec::new()));
        c.unseen = false;
        assert_eq!(conv_state(&c), ConvState::Idle);
        c.busy = true;
        assert_eq!(conv_state(&c), ConvState::Working);
        assert_eq!(short_duration(Duration::from_secs(3725)), "1 h 02");
        assert_eq!(short_duration(Duration::from_secs(42)), "42 s");
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
        let _ = ctx.run(Default::default(), |_| {});
        let id = egui::FontId::proportional(13.0);
        let missing: Vec<char> = "⟳❓✔·⇉+⇱×📄👤✏⇄■▶⚠".chars().filter(|c| !ctx.fonts(|f| f.has_glyphs(&id, &c.to_string()))).collect();
        assert!(missing.is_empty(), "{missing:?}");
    }
}
