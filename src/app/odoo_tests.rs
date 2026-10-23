//! Odoo tests run in a Docker container: from the Docker view (🧪 Tests, tags filled from the
//! code under the cursor, the last ones kept), again from anywhere (Ctrl+F6), or from the ▶ of
//! the gutter of a test file. A run goes to its terminal tab (an `ipdb` in a test works there);
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
    /// The tests row of the Docker view is open.
    pub(super) open: bool,
    pub(super) draft: String,
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

    /// Runs the tests `tags` in the container of the tests (asked in the Docker view when there
    /// is none yet).
    pub(super) fn run_tests(&mut self, tags: String) {
        let tags = tags.trim().to_string();
        let container = self.settings.odoo_test_container.clone();
        if container.is_empty() || tags.is_empty() {
            self.tests.draft = if tags.is_empty() { self.test_tags_here().unwrap_or_default() } else { tags };
            self.tests.open = true;
            self.open_docker();
            self.set_status(t!("Choisissez le conteneur des tests, puis ▶ Lancer", "Choose the tests container, then ▶ Run"));
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

    /// In the Docker view: the chosen container becomes the one of the tests.
    pub(super) fn run_tests_in(&mut self, container: String, tags: String) {
        if self.settings.odoo_test_container != container {
            self.settings.odoo_test_container = container.clone();
            self.persist("odoo_test_container", config::toml_str(&container));
        }
        self.tests.open = false;
        self.run_tests(tags);
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
        match self.term.open_command_tab(&ctx, &exec.argv, exec.cwd) {
            Ok(id) => {
                let title = format!("Tests {tags}");
                self.term.set_name(id, title.clone());
                self.term.set_mounts(id, exec.mounts);
                self.open_view_tab(document::View::Terminal(id), title);
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

    /// The tests row of the Docker view, under the bar of the container `name`.
    pub(super) fn tests_row(&mut self, ui: &mut egui::Ui, name: &str) {
        let mut run = None;
        ui.horizontal(|ui| {
            ui.label("Tags");
            let r = ui.add(egui::TextEdit::singleline(&mut self.tests.draft).desired_width(280.0).hint_text(t!(":MaClasse, /mon_module, :MaClasse.test_x", ":MyClass, /my_module, :MyClass.test_x")).font(egui::TextStyle::Monospace));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                run = Some(self.tests.draft.clone());
            }
            let history = &self.settings.odoo_test_history;
            ui.add_enabled_ui(!history.is_empty(), |ui| {
                ui.menu_button("▾", |ui| {
                    for t in history {
                        if ui.button(RichText::new(t).monospace()).clicked() {
                            self.tests.draft = t.clone();
                            ui.close_menu();
                        }
                    }
                })
                .response
                .on_hover_text(t!("Tags lancés récemment", "Recently run tags"));
            });
            if let Some(here) = self.test_tags_here() {
                if ui.small_button(format!("↳ {here}")).on_hover_text(t!("D'après le curseur dans le dernier fichier", "From the cursor in the last file")).clicked() {
                    self.tests.draft = here;
                }
            }
            if ui.add_enabled(!self.tests.draft.trim().is_empty(), egui::Button::new(t!("▶ Lancer", "▶ Run"))).on_hover_text(t!("Ctrl+F6 relance ensuite le dernier, de n'importe où", "Ctrl+F6 then runs the last one again, from anywhere")).clicked() {
                run = Some(self.tests.draft.clone());
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("Commande", "Command")).small().weak());
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.settings.odoo_test_command)
                    .desired_width(f32::INFINITY)
                    .hint_text(odoo_test::DEFAULT_COMMAND)
                    .font(egui::TextStyle::Small),
            );
            if r.lost_focus() {
                let c = config::toml_str(&self.settings.odoo_test_command);
                self.persist("odoo_test_command", c);
            }
        });
        if let Some(tags) = run {
            self.run_tests_in(name.to_string(), tags);
        }
    }
}
