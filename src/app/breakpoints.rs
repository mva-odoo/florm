//! Breakpoints written in the code instead of a debugger: F9 (or a click in the gutter) puts an
//! `ipdb` line (`debugger;` in JavaScript) above the line or takes it away, the editor follows
//! where pdb / ipdb waits in a terminal, and a commit that would add one asks first.

use super::*;
use crate::trace;

/// Where pdb / ipdb waits, followed in the editor.
#[derive(Default)]
pub(super) struct Follow {
    /// Per terminal: its output count when last read.
    seen: HashMap<u64, u64>,
    /// The terminal where it waits, the file and the line (0-based).
    pub(super) stop: Option<(u64, PathBuf, usize)>,
}

impl App {
    /// F9: an `ipdb` above the line of the cursor, or the one on it taken away.
    pub(super) fn trace_toggle(&mut self) {
        let Some(d) = self.docs.get(self.active).filter(|d| d.view == document::View::Text) else { return };
        let line = d.rope.char_to_line(d.primary_sel().head.min(d.rope.len_chars()));
        self.trace_toggle_at(self.active, line);
    }

    /// The breakpoint of line `line` of document `i` added or taken away.
    pub(super) fn trace_toggle_at(&mut self, i: usize, line: usize) {
        let Some(d) = self.docs.get_mut(i) else { return };
        let Some(lang) = d.path.as_deref().and_then(trace::lang_of) else {
            self.set_status(t!("Point d'arrêt : en Python (ipdb) et en JavaScript (debugger;) seulement", "Breakpoint: in Python (ipdb) and JavaScript (debugger;) only"));
            return;
        };
        if d.read_only {
            return;
        }
        let statement = trace::statement(lang);
        if d.toggle_trace(lang, line, &statement) {
            self.set_status(tf!("{statement} ajouté (F9 sur la ligne pour le retirer)", "{statement} added (F9 on the line to remove it)"));
        }
    }

    /// The breakpoints of all the open files taken away.
    pub(super) fn trace_clear(&mut self) {
        let mut n = 0;
        for d in self.docs.iter_mut().filter(|d| d.view == document::View::Text && !d.read_only) {
            if let Some(lang) = d.path.as_deref().and_then(trace::lang_of) {
                n += d.remove_traces(lang);
            }
        }
        self.set_status(match n {
            0 => t!("Aucun point d'arrêt dans les fichiers ouverts", "No breakpoint in the open files").to_string(),
            n => tf!("{n} point(s) d'arrêt retiré(s) (à enregistrer)", "{n} breakpoint(s) removed (to save)"),
        });
    }

    /// At each frame: a terminal whose output changed is read again; when pdb / ipdb waits there
    /// at a new place, the file opens at that line (the keyboard stays in the terminal).
    pub(super) fn trace_follow(&mut self) {
        for id in self.term.plain_ids() {
            let Some(n) = self.term.changes(id) else { continue };
            if self.follow.seen.insert(id, n) == Some(n) {
                continue;
            }
            match self.term.debugger_stop(id) {
                Some((path, line, cwd)) => {
                    let dirs: Vec<&Path> = [Some(cwd.as_path()), self.root.as_deref()].into_iter().flatten().collect();
                    let Some(file) = trace::find_file(&path, &dirs) else { continue };
                    let line = line.saturating_sub(1);
                    if self.follow.stop.as_ref().is_some_and(|(s, f, l)| *s == id && *f == file && *l == line) {
                        continue;
                    }
                    self.follow.stop = Some((id, file.clone(), line));
                    // a terminal in a tab: the file would take its place
                    if !self.term.is_docked(id) {
                        let editor = self.active;
                        self.open_at(file, line);
                        self.pending_focus = false;
                        self.term.want_focus = true;
                        if self.active != editor {
                            self.egui_ctx.request_repaint();
                        }
                    }
                }
                None if self.follow.stop.as_ref().is_some_and(|(s, ..)| *s == id) => self.follow.stop = None,
                None => {}
            }
        }
        self.follow.seen.retain(|id, _| self.term.changes(*id).is_some());
        // its terminal closed: the line is no longer marked
        if self.follow.stop.as_ref().is_some_and(|(s, ..)| self.term.changes(*s).is_none()) {
            self.follow.stop = None;
        }
    }

    /// The line where ipdb waits in document `i`.
    pub(super) fn trace_exec_line(&self, i: usize) -> Option<usize> {
        let (_, file, line) = self.follow.stop.as_ref()?;
        (self.docs.get(i)?.path.as_ref() == Some(file)).then_some(*line)
    }

    /// Commits with these arguments, after a look at what it adds: a breakpoint (`ipdb`,
    /// `debugger;`) among them is asked about first.
    pub(super) fn commit_checked(&mut self, repo: PathBuf, args: Vec<String>, all: bool) {
        self.set_status(t!("git commit…", "git commit…"));
        self.jobs.spawn(move || {
            let diff = if all { git::stdout(&repo, &["diff", "HEAD", "-U0", "--no-color"]) } else { git::stdout(&repo, &["diff", "--cached", "-U0", "--no-color"]) };
            let traces = trace::added_in_diff(&diff.unwrap_or_default());
            JobResult::CommitChecked { repo, args, traces }
        });
    }

    /// What the commit adds was read: committed, or a question first when it adds breakpoints.
    pub(super) fn commit_ready(&mut self, repo: PathBuf, args: Vec<String>, traces: Vec<(String, usize)>) {
        if traces.is_empty() {
            self.git_seq_at(repo, "commit", vec![args]);
            return;
        }
        let list = traces.iter().take(8).map(|(f, l)| format!("  {f}:{l}")).collect::<Vec<_>>().join("\n");
        let more = if traces.len() > 8 { format!("\n  … (+{})", traces.len() - 8) } else { String::new() };
        let msg = tf!(
            "Le commit ajoute des points d'arrêt :\n{list}{more}\n\nF9 sur la ligne (ou « Point d'arrêt : retirer… » dans la palette) les retire.",
            "The commit adds breakpoints:\n{list}{more}\n\nF9 on the line (or “Breakpoint: remove…” in the palette) removes them."
        );
        self.confirm_git(t!("ipdb dans le commit", "ipdb in the commit"), msg, t!("Commiter quand même", "Commit anyway"), "commit", vec![args], None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real pdb in a terminal of the bottom panel: the file opens at the line where it waits, then
    /// the line is no longer marked once the program goes on.
    #[test]
    #[ignore = "runs the python3 installed: cargo test -- --ignored"]
    fn the_editor_follows_pdb_in_a_terminal() {
        if crate::os::command("python3").arg("--version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("florm_follow_pdb_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        std::fs::write(proj.join("a.py"), "x = 1\nbreakpoint()\ny = 2\nprint('done')\n").unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        let argv = ["python3".to_string(), "a.py".to_string()];
        let id = app.term.open_command_panel(&ctx, &argv, Some(proj.clone())).unwrap();
        let t = std::time::Instant::now();
        while app.follow.stop.is_none() && t.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            app.trace_follow();
        }
        let file = crate::os::canon(&proj.join("a.py"));
        let i = app.docs.iter().position(|d| d.path.as_deref().map(crate::os::canon) == Some(file.clone()));
        assert!(i.is_some(), "a.py not opened: {:?}", app.follow.stop);
        // on breakpoint() (Python 3.13 and later) or on the line after it
        assert!(matches!(app.trace_exec_line(i.unwrap()), Some(1 | 2)), "{:?}", app.follow.stop);
        app.term.type_text(id, "c\n");
        let t = std::time::Instant::now();
        while app.follow.stop.is_some() && t.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            app.trace_follow();
        }
        assert!(app.follow.stop.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
