//! The commit of a line of the blame of the gutter, on hover: its whole message, read with
//! `git show` on a thread, and links to it in Florm's Git view and on GitHub. A small window that
//! stays while the pointer goes to it (unlike a tooltip, its buttons can be clicked).

use super::*;

/// How long the window stays once the pointer left it and its line (seconds).
const GRACE: f64 = 0.4;

pub(super) struct BlamePop {
    pub uid: u64,
    pub blame: git::Blame,
    pub repo: PathBuf,
    /// The blame it was opened from: a cell of the gutter (it opens at its right) or the line
    /// blame at the end of a line (under it).
    pub cell: egui::Rect,
    pub below: bool,
    /// When the pointer left it and its cell (None: on one of them).
    pub left: Option<f64>,
    pub rect: Option<egui::Rect>,
}

/// A commit's details for the window: its whole message, and its address on GitHub (if the
/// repository's remote is there).
#[derive(Clone)]
pub(crate) struct CommitInfo {
    pub message: String,
    pub github: Option<String>,
}

impl App {
    /// After the editor of document `i`: the window of the commit hovered in its blame column.
    pub(super) fn blame_popup(&mut self, ctx: &egui::Context, i: usize) {
        let Some(d) = self.docs.get_mut(i) else { return };
        let uid = d.uid;
        let hovered = d.blame_hover.take();
        let path = d.path.clone();
        let now = ctx.input(|inp| inp.time);
        // a cell hovered: its commit (a new one replaces the window)
        let mut on_cell = false;
        if let Some((b, cell, below)) = hovered {
            let same = self.blame_pop.as_ref().is_some_and(|p| p.uid == uid && p.blame.hash == b.hash);
            on_cell = true;
            if !same {
                let Some(repo) = path.as_deref().and_then(|p| self.repo_for(p)) else { return };
                if !self.commit_info.contains_key(&b.hash) {
                    self.commit_info.insert(b.hash.clone(), None);
                    let (r, h) = (repo.clone(), b.hash.clone());
                    self.jobs.spawn(move || {
                        let message = git::stdout(&r, &["show", "-s", "--format=%B", &h]).map(|m| m.trim_end().to_string());
                        let github = git::stdout(&r, &["remote", "get-url", "origin"]).ok().and_then(|u| git::parse_github(u.trim())).map(|(o, n)| format!("https://github.com/{o}/{n}/commit/{h}"));
                        JobResult::CommitInfo(h, message.map(|message| CommitInfo { message, github }))
                    });
                }
                self.blame_pop = Some(BlamePop { uid, blame: b, repo, cell, below, left: None, rect: None });
            } else if let Some(p) = self.blame_pop.as_mut() {
                (p.cell, p.below) = (cell, below);
            }
        }
        let Some(pop) = self.blame_pop.as_mut().filter(|p| p.uid == uid) else { return };
        let on_window = pop.rect.is_some_and(|r| ctx.pointer_hover_pos().is_some_and(|p| r.expand(4.0).contains(p)));
        if on_window || on_cell {
            pop.left = None;
        } else if pop.left.is_none() {
            pop.left = Some(now);
        }
        if pop.left.is_some_and(|t| now - t > GRACE) {
            self.blame_pop = None;
            return;
        }
        if pop.left.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let b = pop.blame.clone();
        let repo = pop.repo.clone();
        let info = self.commit_info.get(&b.hash).cloned().flatten();
        let mut act: Option<u8> = None;
        let area = egui::Area::new(egui::Id::new(("blame_pop", uid))).fixed_pos(if pop.below { pop.cell.left_bottom() + egui::vec2(0.0, 2.0) } else { pop.cell.right_top() + egui::vec2(4.0, 0.0) }).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(520.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&b.hash[..b.hash.len().min(10)]).monospace().color(self.hl.colors.accent));
                    ui.label(RichText::new(&b.author).strong());
                    ui.label(RichText::new(git::ago(b.time)).weak());
                });
                ui.separator();
                match &info {
                    Some(Ok(ci)) => {
                        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                            ui.add(egui::Label::new(&ci.message).selectable(true).wrap());
                        });
                    }
                    Some(Err(e)) => {
                        ui.label(&b.summary);
                        ui.label(RichText::new(e.lines().next().unwrap_or("")).small().weak());
                    }
                    None => {
                        ui.label(&b.summary);
                        ui.horizontal(|ui| {
                            crate::app::spinner_sized(ui, 12.0);
                            ui.label(RichText::new(t!("le message entier…", "the whole message…")).small().weak());
                        });
                    }
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("Voir dans Florm (Git)", "See in Florm (Git)")).on_hover_text(t!("Le commit dans le graphe de la vue Git, avec ses fichiers", "The commit in the graph of the Git view, with its files")).clicked() {
                        act = Some(0);
                    }
                    if let Some(Ok(CommitInfo { github: Some(url), .. })) = &info {
                        if ui.button(t!("Ouvrir sur GitHub ↗", "Open on GitHub ↗")).on_hover_text(url.as_str()).clicked() {
                            act = Some(1);
                        }
                    }
                    if ui.small_button(t!("Copier le hash", "Copy the hash")).clicked() {
                        act = Some(2);
                    }
                });
            });
        });
        if let Some(p) = self.blame_pop.as_mut() {
            p.rect = Some(area.response.rect);
        }
        match act {
            Some(0) => {
                self.blame_pop = None;
                self.git_show_commit(repo, b.hash);
            }
            Some(1) => {
                if let Some(Ok(CommitInfo { github: Some(url), .. })) = &info {
                    if let Err(e) = crate::os::open_url(url) {
                        self.set_error(e);
                    }
                }
            }
            Some(2) => {
                ctx.copy_text(b.hash.clone());
                self.set_status(t!("Hash copié", "Hash copied"));
            }
            _ => {}
        }
    }

    pub(super) fn on_commit_info(&mut self, hash: String, r: Result<CommitInfo, String>) {
        self.commit_info.insert(hash, Some(r));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blame cell hovered: the window of its commit, with the whole message once read and the
    /// links (Florm, GitHub); it stays while the pointer is on it, goes away after it left.
    #[test]
    fn the_commit_window_of_the_blame() {
        let dir = std::env::temp_dir().join(format!("florm_blame_pop_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let g = |args: &[&str]| assert!(std::process::Command::new("git").args(args).current_dir(&repo).output().unwrap().status.success(), "{args:?}");
        g(&["init", "-q"]);
        g(&["config", "user.email", "a@b.c"]);
        g(&["config", "user.name", "Alice"]);
        g(&["remote", "add", "origin", "git@github.com:me/proj.git"]);
        std::fs::write(repo.join("f.txt"), "one\n").unwrap();
        g(&["add", "f.txt"]);
        g(&["commit", "-q", "-m", "First line\n\nThe body of the message,\non two lines."]);
        let hash = String::from_utf8(std::process::Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&repo).output().unwrap().stdout).unwrap().trim().to_string();
        std::fs::write(dir.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &dir);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(repo.clone()));
        assert!(app.open_path(repo.join("f.txt")));
        let i = app.active;
        // the repositories of the project are looked for on a thread
        let t = Instant::now();
        while app.repo_for(&repo.join("f.txt")).is_none() {
            assert!(t.elapsed() < Duration::from_secs(10));
            app.process_jobs();
            std::thread::sleep(Duration::from_millis(10));
        }
        let blame = git::Blame { hash: hash.clone(), author: "Alice".into(), time: 0, summary: "First line".into() };
        let cell = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(150.0, 18.0));
        let frame = |app: &mut App, time: f64, pointer: egui::Pos2, hover: bool| {
            let input = egui::RawInput { time: Some(time), events: vec![egui::Event::PointerMoved(pointer)], ..Default::default() };
            let out = ctx.run(input, |ctx| {
                if hover {
                    app.docs[i].blame_hover = Some((blame.clone(), cell, false));
                }
                app.blame_popup(ctx, i);
            });
            let mut texts = Vec::new();
            fn walk(s: &egui::Shape, out: &mut Vec<String>) {
                match s {
                    egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                    egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                    _ => {}
                }
            }
            out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
            texts
        };
        frame(&mut app, 0.0, cell.center(), true);
        // the message read on a thread
        let t = Instant::now();
        while !matches!(app.commit_info.get(&hash), Some(Some(_))) {
            assert!(t.elapsed() < Duration::from_secs(10));
            app.process_jobs();
            std::thread::sleep(Duration::from_millis(10));
        }
        let texts = frame(&mut app, 0.1, cell.center(), true);
        assert!(texts.iter().any(|x| x.contains("The body of the message,\non two lines.")), "{texts:?}");
        assert!(texts.iter().any(|x| x == "Voir dans Florm (Git)") && texts.iter().any(|x| x == "Ouvrir sur GitHub ↗"), "{texts:?}");
        let info = app.commit_info[&hash].clone().unwrap().unwrap();
        assert_eq!(info.github.as_deref(), Some(format!("https://github.com/me/proj/commit/{hash}").as_str()));
        // the pointer goes into the window: it stays
        let win = app.blame_pop.as_ref().unwrap().rect.unwrap();
        frame(&mut app, 0.2, win.center(), false);
        frame(&mut app, 1.5, win.center(), false);
        assert!(app.blame_pop.is_some());
        // away from both: gone after a moment
        frame(&mut app, 1.6, egui::pos2(900.0, 900.0), false);
        assert!(app.blame_pop.is_some());
        frame(&mut app, 1.6 + GRACE + 0.1, egui::pos2(900.0, 900.0), false);
        assert!(app.blame_pop.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
