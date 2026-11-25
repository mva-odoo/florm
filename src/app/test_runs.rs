//! Tests (pytest, jest, `cargo test`, `go test`), run where the project is. A command is
//! asked with the one of the code under the cursor already filled, the last ones run again from
//! anywhere (Ctrl+F6), or the ▶ of the gutter of a test file runs one. A run goes to the bottom
//! panel's terminal (an `ipdb` in a test works there); its summary goes
//! to the status bar, its tracebacks open their file with Ctrl+clic.

use super::*;

/// Most test commands kept in the history.
const HISTORY: usize = 12;

/// The run being shown.
struct Run {
    session: u64,
    tags: String,
    /// Its summary was given (again when it is started anew with Enter).
    summarized: bool,
}

#[derive(Default)]
pub(super) struct Tests {
    run: Option<Run>,
    /// Tests of the open test files, for the gutter: uid → (version, (line, tags)).
    marks: HashMap<u64, (u64, Vec<(usize, String)>)>,
}

impl App {
    /// The test command of the code under the cursor of the last file worked on.
    pub(super) fn test_tags_here(&self) -> Option<String> {
        let text_doc = |d: &&Document| d.view == document::View::Text && d.path.is_some();
        let d = self.docs.get(self.active).filter(text_doc).or_else(|| {
            let recent = self.index.recent.first()?;
            self.docs.iter().find(|d| d.path.as_ref() == Some(recent))
        })?;
        let line = d.rope.char_to_line(d.primary_sel().head.min(d.rope.len_chars()));
        let path = d.path.as_deref()?;
        let text = d.rope.to_string();
        crate::test_cmd::command_at(path, &text, &self.test_root(path), line)
    }

    /// The folder of the project of `path` (for the commands of its tests).
    fn test_root(&self, path: &Path) -> PathBuf {
        match &self.root {
            Some(r) if path.starts_with(r) => r.clone(),
            _ => path.parent().map(Path::to_path_buf).unwrap_or_default(),
        }
    }

    /// Runs the test command `tags`: without one, it is asked (the one of the code under the
    /// cursor filled).
    pub(super) fn run_tests(&mut self, tags: String) {
        let tags = tags.trim().to_string();
        let local = crate::test_cmd::parse(&tags);
        if local.is_none() && !tags.is_empty() {
            self.set_error(tf!("Pas une commande de test : {tags} (pytest, jest, cargo test, go test…)", "Not a test command: {tags} (pytest, jest, cargo test, go test…)"));
            return;
        }
        if tags.is_empty() {
            let draft = self.test_tags_here().or_else(|| self.settings.test_history.first().cloned()).unwrap_or_default();
            self.open_prompt(Prompt::TestTags(draft));
            return;
        }
        let h = &mut self.settings.test_history;
        h.retain(|t| *t != tags);
        h.insert(0, tags.clone());
        h.truncate(HISTORY);
        let history = config::toml_str_array(h);
        self.persist("test_history", history);
        let root = self.root.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        if let Some((dir, argv)) = local {
            let cwd = dir.map_or(root.clone(), |d| root.join(d));
            self.show_run(tags, &argv, Some(cwd));
        }
    }

    /// A run in the bottom panel's terminal, in place of the previous one.
    fn show_run(&mut self, tags: String, argv: &[String], cwd: Option<PathBuf>) {
        // one tab for the runs: the previous one goes
        if let Some(old) = self.tests.run.take() {
            match self.docs.iter().position(|d| d.view == document::View::Terminal(old.session)) {
                Some(i) => self.close_tab(i, true),
                None => self.term.close(old.session),
            }
        }
        let ctx = self.egui_ctx.clone();
        match self.term.open_command_panel(&ctx, argv, cwd) {
            Ok(id) => {
                self.term.set_name(id, format!("Tests {tags}"));
                self.bottom = Some(BottomTab::Terminal);
                self.tests.run = Some(Run { session: id, tags, summarized: false });
            }
            Err(e) => self.set_error(tf!("Tests impossibles à lancer : {e}", "Cannot run the tests: {e}")),
        }
    }

    /// Once a run ended: its summary in the status bar (read from what the tests wrote at the end).
    pub(super) fn tests_check(&mut self) {
        let Some(run) = self.tests.run.as_mut() else { return };
        if !self.term.exited(run.session) {
            run.summarized = false;
            return;
        }
        if run.summarized {
            return;
        }
        run.summarized = true;
        let Some(out) = self.term.output(run.session) else {
            self.tests.run = None;
            return;
        };
        let tags = run.tags.clone();
        let msg = match crate::test_cmd::summary(&out) {
            Some((ok, s)) => format!("{} {tags} : {s}", if ok { "✔" } else { "⚠" }),
            None => tf!("{tags} terminé (pas de résumé : voir l'onglet)", "{tags} finished (no summary: see the tab)"),
        };
        self.log(msg.clone());
        self.set_status(msg);
    }

    /// The tests of the document `i` (a test file), for the ▶ of its gutter.
    pub(super) fn test_marks(&mut self, i: usize) -> Vec<(usize, String)> {
        let d = &self.docs[i];
        let Some(path) = d.path.as_deref() else { return Vec::new() };
        // the ones of this version; while typing, the ones before (read again after a pause): the
        // disk is only asked when the text changed
        if let Some((v, m)) = self.tests.marks.get(&d.uid) {
            if *v == d.version || d.last_change.elapsed() < Duration::from_millis(300) {
                return m.clone();
            }
        }
        if crate::test_cmd::runner_of(path).is_none() {
            return Vec::new();
        }
        let m = crate::test_cmd::tests_in(path, &d.rope.to_string(), &self.test_root(path));
        self.tests.marks.insert(d.uid, (d.version, m.clone()));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real `go test` from a ▶ of the gutter: run in the project, its summary in the status bar.
    /// Left aside on Windows: it fails on the CI there (no summary in 60 s), cause not found yet.
    #[test]
    #[ignore = "runs the go installed (unsteady while the whole suite runs in parallel): cargo test -- --ignored"]
    fn a_go_test_runs_here_and_is_summed_up() {
        if crate::os::command("go").arg("version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("florm_generic_tests_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (conf, proj) = (dir.join("conf"), dir.join("proj"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(proj.join("calc")).unwrap();
        std::fs::write(conf.join("settings.toml"), "check_updates = false\nlsp = false\n").unwrap();
        std::fs::write(proj.join("go.mod"), "module calc\n\ngo 1.21\n").unwrap();
        let go = "package calc\n\nimport \"testing\"\n\nfunc TestAdd(t *testing.T) {}\n";
        std::fs::write(proj.join("calc/calc_test.go"), go).unwrap();
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FLORM_CONFIG", &conf);
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Some(proj.clone()));
        let marks = crate::test_cmd::tests_in(&proj.join("calc/calc_test.go"), go, &proj);
        assert_eq!(marks.len(), 1);
        app.run_tests(marks[0].1.clone());
        let t = std::time::Instant::now();
        while app.status.as_ref().is_none_or(|s| !s.text.contains("go test")) && t.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            app.tests_check();
        }
        let status = app.status.as_ref().map(|s| s.text.clone()).unwrap_or_default();
        let out = app.tests.run.as_ref().and_then(|r| app.term.output(r.session)).unwrap_or_default();
        assert!(status.starts_with("✔ go test ./calc"), "{status}\n---\n{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
