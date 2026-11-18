//! Odoo tests run in a Docker container (set in Preferences → Tests): tags asked with those of
//! the code under the cursor already filled, the last ones again from anywhere (Ctrl+F6), or the
//! ▶ of the gutter of a test file. A run goes to the bottom panel's terminal (an `ipdb` in a test works there);
//! its summary goes to the status bar, its tracebacks open their file with Ctrl+clic.

use super::*;
use crate::odoo_test;

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
    /// Tags for the code under the cursor of the last file worked on.
    pub(super) fn test_tags_here(&self) -> Option<String> {
        let text_doc = |d: &&Document| d.view == document::View::Text && d.path.is_some();
        let d = self.docs.get(self.active).filter(text_doc).or_else(|| {
            let recent = self.index.recent.first()?;
            self.docs.iter().find(|d| d.path.as_ref() == Some(recent))
        })?;
        let line = d.rope.char_to_line(d.primary_sel().head.min(d.rope.len_chars()));
        odoo_test::tags_at(d.path.as_deref()?, &d.rope.to_string(), line)
    }

    /// Runs the tests `tags` in the container of the tests: without tags, they are asked (those of
    /// the code under the cursor filled); without a container, the preferences show where to set it.
    pub(super) fn run_tests(&mut self, tags: String) {
        let tags = tags.trim().to_string();
        let container = self.settings.odoo_test_container.clone();
        if container.is_empty() {
            self.prefs.section = PrefSection::Tests;
            self.open_settings();
            self.set_status(t!("Indique le conteneur Docker des tests Odoo, puis relance (Ctrl+F6)", "Set the Docker container of the Odoo tests, then run again (Ctrl+F6)"));
            return;
        }
        if tags.is_empty() {
            let draft = self.test_tags_here().or_else(|| self.settings.odoo_test_history.first().cloned()).unwrap_or_default();
            self.open_prompt(Prompt::TestTags(draft));
            return;
        }
        let h = &mut self.settings.odoo_test_history;
        h.retain(|t| *t != tags);
        h.insert(0, tags.clone());
        h.truncate(odoo_test::HISTORY);
        let history = config::toml_str_array(h);
        self.persist("odoo_test_history", history);
        let cmd = odoo_test::command(&self.settings.odoo_test_command, &tags);
        self.set_status(tf!("Tests {tags} dans {container}…", "Tests {tags} in {container}…"));
        self.jobs.spawn(move || JobResult::TestsReady { result: crate::docker::exec_in(&container, &cmd), tags });
    }

    pub(super) fn tests_ready(&mut self, tags: String, result: Result<crate::docker::Exec, String>) {
        let exec = match result {
            Ok(e) => e,
            Err(e) => {
                self.set_error(format!("Tests {tags} : {e}"));
                self.log(e);
                return;
            }
        };
        // one tab for the runs: the previous one goes
        if let Some(old) = self.tests.run.take() {
            match self.docs.iter().position(|d| d.view == document::View::Terminal(old.session)) {
                Some(i) => self.close_tab(i, true),
                None => self.term.close(old.session),
            }
        }
        let ctx = self.egui_ctx.clone();
        match self.term.open_command_panel(&ctx, &exec.argv, exec.cwd) {
            Ok(id) => {
                self.term.set_name(id, format!("Tests {tags}"));
                self.term.set_mounts(id, exec.mounts);
                self.bottom = Some(BottomTab::Terminal);
                self.tests.run = Some(Run { session: id, tags, summarized: false });
            }
            Err(e) => self.set_error(tf!("Tests impossibles à lancer : {e}", "Cannot run the tests: {e}")),
        }
    }

    /// Once a run ended: its summary in the status bar (read from what Odoo wrote at the end).
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
        let msg = match odoo_test::summary(&out) {
            Some(s) => format!("Tests {tags} : {}", s.text()),
            None => tf!("Tests {tags} terminés (pas de résumé d'Odoo : voir l'onglet)", "Tests {tags} finished (no summary from Odoo: see the tab)"),
        };
        self.log(msg.clone());
        self.set_status(msg);
    }

    /// The tests of the document `i` (a test file), for the ▶ of its gutter.
    pub(super) fn test_marks(&mut self, i: usize) -> Vec<(usize, String)> {
        let d = &self.docs[i];
        if !crate::docker::available() || !d.path.as_deref().is_some_and(odoo_test::is_test_file) {
            return Vec::new();
        }
        match self.tests.marks.get(&d.uid) {
            Some((v, m)) if *v == d.version => m.clone(),
            _ => {
                let m = odoo_test::tests_in(&d.rope.to_string());
                self.tests.marks.insert(d.uid, (d.version, m.clone()));
                m
            }
        }
    }

}
