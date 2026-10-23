//! The Docker view, like lazydocker: the containers and their state on the left (by compose
//! project), the logs of the chosen one on the right in a terminal, attached to the container
//! when it reads its input, so that an `ipdb` stopped in it is answered right there.

use super::*;
use crate::docker::{self, Container};

/// The list is read again this often while the view is shown.
const REFRESH: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct DockerUi {
    list: Option<Result<Vec<Container>, String>>,
    /// Compose projects of the open folder (and its subfolders).
    composes: Vec<docker::Compose>,
    loading: bool,
    loaded_at: Option<Instant>,
    /// Name of the chosen container.
    selected: Option<String>,
    /// Terminal following it (session of `term`), and whether it is attached to its input.
    session: Option<u64>,
    interactive: bool,
    /// State and start time of the container shown (`docker::Follow::key`).
    follow_key: Option<String>,
    /// The command of the terminal is being prepared (inspect, last logs).
    preparing: bool,
    started_at: Option<Instant>,
    /// Action running ("Redémarrage de web…").
    busy: Option<String>,
}

/// A row of the list being dragged: (project, service).
#[derive(Clone)]
struct DragRow(String, String);

enum DockAct {
    /// `service` of `project` moved before (or after) `target`.
    Reorder { project: String, service: String, target: String, after: bool },
    Reload,
    Select(String),
    Run(&'static str, Vec<String>),
    Shell(String),
    /// `docker compose up -d` of a project, in a tab.
    Up(docker::Compose),
    Move(bool),
    /// The container may have restarted: its logs again if so.
    Refollow,
    /// Enter after the logs ended: again, whatever the state.
    Restart,
    /// Opens / closes the tests row.
    Tests,
}

impl App {
    pub(super) fn open_docker(&mut self) {
        if self.bottom == Some(BottomTab::Docker) {
            self.bottom = None;
        }
        self.open_view_tab(document::View::Docker, "Docker".into());
        self.docker_load();
    }

    pub(super) fn docker_load(&mut self) {
        if self.docker.loading {
            return;
        }
        self.docker.loading = true;
        let root = self.root.clone();
        self.jobs.spawn(move || JobResult::DockerList(docker::list(), root.as_deref().map(docker::composes_in).unwrap_or_default()));
    }

    pub(super) fn docker_listed(&mut self, list: Result<Vec<Container>, String>, composes: Vec<docker::Compose>) {
        let d = &mut self.docker;
        d.loading = false;
        d.composes = composes;
        d.loaded_at = Some(Instant::now());
        if d.selected.is_none() {
            // the first running one, from the folder's compose project if there is one
            let root = self.root.as_deref().and_then(|r| r.file_name()).map(|n| n.to_string_lossy().to_lowercase());
            d.selected = list.as_ref().ok().and_then(|l| {
                let running = || l.iter().filter(|c| c.running());
                running().find(|c| c.project.is_some() && c.project.as_deref().map(str::to_lowercase) == root).or_else(|| running().next()).map(|c| c.name.clone())
            });
        }
        d.list = Some(list);
        self.docker_sort();
        if self.docker.selected.is_some() && self.docker.session.is_none() && !self.docker.preparing {
            self.docker_follow(true);
        }
    }

    /// Each compose project in the order of its services chosen (`docker_order`).
    fn docker_sort(&mut self) {
        let order = &self.settings.docker_order;
        if let Some(Ok(l)) = &mut self.docker.list {
            let none = Vec::new();
            l.sort_by_cached_key(|c| {
                let o = c.project.as_ref().and_then(|p| order.get(p)).unwrap_or(&none);
                (c.project.is_none(), c.project.clone(), docker::order_key(o, c.label()), c.name.clone())
            });
        }
    }

    fn docker_reorder(&mut self, project: String, service: String, target: String, after: bool) {
        let Some(Ok(l)) = &self.docker.list else { return };
        let mut order: Vec<String> = l.iter().filter(|c| c.project.as_ref() == Some(&project)).map(|c| c.label().to_string()).collect();
        order.retain(|s| *s != service);
        let at = order.iter().position(|s| *s == target).map_or(order.len(), |i| i + usize::from(after));
        order.insert(at, service);
        self.settings.docker_order.insert(project, order);
        let table = self
            .settings
            .docker_order
            .iter()
            .map(|(p, o)| format!("{} = {}", config::toml_str(p), config::toml_str_array(o)))
            .collect::<Vec<_>>()
            .join(", ");
        self.persist("docker_order", format!("{{ {table} }}"));
        self.docker_sort();
    }

    /// Prepares the terminal of the chosen container (its last logs, attached or not). Not
    /// `fresh`: only if the container was (re)started since its logs were shown, else they stay
    /// as they are (no blinking while the list still says "running" for a stopping container).
    fn docker_follow(&mut self, fresh: bool) {
        let Some(name) = self.docker.selected.clone() else { return };
        let unless = if fresh { None } else { self.docker.follow_key.clone() };
        self.docker.preparing = true;
        self.jobs.spawn(move || JobResult::DockerFollow { result: docker::follow(&name, unless.as_deref()), name });
    }

    /// Another container chosen: its logs, not the ones of the previous one meanwhile.
    fn docker_select(&mut self, name: String) {
        if let Some(id) = self.docker.session.take() {
            self.term.close(id);
        }
        self.docker.follow_key = None;
        self.docker.selected = Some(name);
        self.docker_follow(true);
    }

    pub(super) fn docker_followed(&mut self, name: String, result: Result<Option<docker::Follow>, String>) {
        if self.docker.selected.as_deref() != Some(name.as_str()) {
            return;
        }
        self.docker.preparing = false;
        self.docker.started_at = Some(Instant::now());
        match result {
            Ok(None) => {}
            Ok(Some(f)) => {
                if let Some(id) = self.docker.session.take() {
                    self.term.close(id);
                }
                self.docker.follow_key = Some(f.key.clone());
                let ctx = self.egui_ctx.clone();
                match self.term.open_embedded(&ctx, &f.argv, &f.prefill, None) {
                    Ok(id) => {
                        if f.interactive {
                            self.term.focus(id);
                        }
                        self.term.set_mounts(id, f.mounts);
                        self.docker.session = Some(id);
                        self.docker.interactive = f.interactive;
                    }
                    Err(e) => self.set_error(tf!("Logs de {name} : {e}", "Logs of {name}: {e}")),
                }
            }
            Err(e) => self.set_error(tf!("Docker : {e}", "Docker: {e}")),
        }
    }

    pub(super) fn docker_done(&mut self, label: String, result: Result<(), String>) {
        self.docker.busy = None;
        match result {
            Ok(()) => self.set_status(tf!("{label} : fait", "{label}: done")),
            Err(e) => {
                self.set_error(tf!("{label} : échec (voir Sortie)", "{label}: failed (see Output)"));
                self.log(e);
            }
        }
        self.docker.loaded_at = None;
        self.docker_load();
    }

    pub(super) fn docker_ui(&mut self, ui: &mut egui::Ui) {
        let in_tab = self.docs.iter().any(|d| d.view == document::View::Docker);
        if self.install_guide(ui, crate::tools::Tool::Docker, in_tab) {
            return;
        }
        let mut act: Option<DockAct> = None;
        if self.docker.loaded_at.is_none_or(|t| t.elapsed() >= REFRESH) {
            act = Some(DockAct::Reload);
        }
        ui.ctx().request_repaint_after(REFRESH);
        let big = self.docs.iter().any(|d| d.view == document::View::Docker);
        let fill = ui.visuals().panel_fill;
        let weak = ui.visuals().weak_text_color();
        let d = &self.docker;
        let list: &[Container] = match &d.list {
            Some(Ok(l)) => l,
            _ => &[],
        };
        let busy = d.busy.is_some();
        let current = d.selected.as_deref().and_then(|n| list.iter().find(|c| c.name == n));
        // the container (re)started since its logs were shown: follow it again
        if let (Some(c), Some(id)) = (current, d.session) {
            if c.running() && self.term.exited(id) && !d.preparing && d.started_at.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
                act = Some(DockAct::Refollow);
            }
        }

        // one of the two can take all the room (the logs only when a container is chosen)
        let view = egui::Id::new("docker_view");
        let mut maxed = maximize::maxed(ui.ctx(), view);
        if maxed == Some("logs") && current.is_none() {
            maximize::restore(ui.ctx(), view);
            maxed = None;
        }

        // ------------------------------------------------ containers
        let list_frame = egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 8.0, right: 8.0, top: 6.0, bottom: 6.0 });
        let list_ui = |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Docker").strong().size(15.0));
                // only the first reading: the list is read again every few seconds
                if d.loading && d.list.is_none() {
                    spinner(ui);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if current.is_some() {
                        maximize::button(ui, view, "list");
                    }
                    if big {
                        if ui.small_button("⇲").on_hover_text(t!("Afficher Docker dans le panneau du bas", "Show Docker in the bottom panel")).clicked() {
                            act = Some(DockAct::Move(false));
                        }
                    } else if ui.small_button("⇱").on_hover_text(t!("Afficher Docker en grand dans l'éditeur", "Show Docker large in the editor")).clicked() {
                        act = Some(DockAct::Move(true));
                    }
                    if ui.small_button("⟳").on_hover_text(t!("Relire la liste des conteneurs", "Read the list of containers again")).clicked() {
                        act = Some(DockAct::Reload);
                    }
                });
            });
            ui.separator();
            if let Some(Err(e)) = &d.list {
                ui.label(RichText::new(e).color(crate::highlight::red()));
                return;
            }
            // compose projects of the folder that have no container yet
            let idle: Vec<&docker::Compose> = d.composes.iter().filter(|c| !list.iter().any(|k| k.project.as_ref() == Some(&c.name))).collect();
            if d.list.is_some() && list.is_empty() && idle.is_empty() {
                ui.label(RichText::new(t!("Aucun conteneur.", "No container.")).weak());
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for c in idle {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&c.name).strong());
                        ui.label(RichText::new(t!("aucun conteneur", "no container")).small().color(weak));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let file = c.file.as_deref().map_or(String::new(), |f| f.display().to_string());
                            if ui
                                .add_enabled(!busy, egui::Button::new("⬆ compose up").small())
                                .on_hover_text(tf!("docker compose up -d : crée et démarre les conteneurs de {file}", "docker compose up -d: creates and starts the containers of {file}"))
                                .clicked()
                            {
                                act = Some(DockAct::Up(c.clone()));
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
                let mut k = 0;
                while k < list.len() {
                    let project = list[k].project.clone();
                    let n = list[k..].iter().take_while(|c| c.project == project).count();
                    let group = &list[k..k + n];
                    k += n;
                    let names = |f: fn(&Container) -> bool| group.iter().filter(|c| f(c)).map(|c| c.name.clone()).collect::<Vec<_>>();
                    let up = group.iter().filter(|c| c.running()).count();
                    ui.horizontal(|ui| {
                        let title = project.as_deref().unwrap_or(t!("Autres conteneurs", "Other containers"));
                        ui.label(RichText::new(title).strong());
                        ui.label(RichText::new(format!("{up}/{n}")).small().color(weak));
                        if project.is_some() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let stopped = names(|c| !c.running());
                                let running = names(Container::running);
                                // the files its containers were made from, else those found in the folder
                                let compose = group
                                    .iter()
                                    .find_map(|c| Some(docker::Compose { name: project.clone()?, dir: c.compose_dir.clone().filter(|d| d.is_dir())?, files: c.compose_files.iter().filter(|f| f.is_file()).cloned().collect(), file: None }))
                                    .or_else(|| d.composes.iter().find(|c| project.as_ref() == Some(&c.name)).cloned());
                                if let Some(compose) = compose {
                                    if ui
                                        .add_enabled(!busy, egui::Button::new("⬆").small())
                                        .on_hover_text(t!("docker compose up -d : crée et démarre tous les services du projet, même ceux sans conteneur, et recrée ceux dont la configuration a changé", "docker compose up -d: creates and starts all the services of the project, even those without a container, and recreates those whose configuration changed"))
                                        .clicked()
                                    {
                                        act = Some(DockAct::Up(compose));
                                    }
                                }
                                if ui
                                    .add_enabled(!busy && !running.is_empty(), egui::Button::new(t!("■ tout", "■ all")).small())
                                    .on_hover_text(t!("Arrêter tous les conteneurs du projet, du bas de la liste vers le haut", "Stop all the containers of the project, from the bottom of the list up"))
                                    .clicked()
                                {
                                    act = Some(DockAct::Run("stop", running));
                                }
                                if ui
                                    .add_enabled(!busy && !stopped.is_empty(), egui::Button::new(t!("▶ tout", "▶ all")).small())
                                    .on_hover_text(t!("Démarrer les conteneurs arrêtés dans l'ordre de la liste (poignée à points à gauche : glisser pour le changer), chacun une fois le précédent prêt (healthy)", "Start the stopped containers in the order of the list (dotted handle on the left: drag to change it), each once the previous one is ready (healthy)"))
                                    .clicked()
                                {
                                    act = Some(DockAct::Run("start", stopped));
                                }
                            });
                        }
                    });
                    for c in group {
                        let on = d.selected.as_deref() == Some(c.name.as_str());
                        let row = ui.horizontal(|ui| {
                                if let Some(p) = &project {
                                    // handle to drag the row elsewhere in its project
                                    let (rect, h) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), Sense::drag());
                                    let dots = if h.hovered() || h.dragged() { ui.visuals().strong_text_color() } else { weak };
                                    for (dx, dy) in [(-2.0, -4.0), (2.0, -4.0), (-2.0, 0.0), (2.0, 0.0), (-2.0, 4.0), (2.0, 4.0)] {
                                        ui.painter().circle_filled(rect.center() + egui::vec2(dx, dy), 1.1, dots);
                                    }
                                    let h = h.on_hover_text(t!("Glisser pour changer l'ordre (▶ tout démarre dans cet ordre)", "Drag to change the order (▶ all starts in this order)"));
                                    if h.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                    }
                                    if h.drag_started() {
                                        egui::DragAndDrop::set_payload(ui.ctx(), DragRow(p.clone(), c.label().to_string()));
                                    }
                                }
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), Sense::hover());
                                ui.painter().circle_filled(rect.center(), 4.0, state_color(c, weak));
                                let r = ui.selectable_label(on, c.label());
                                ui.label(RichText::new(short_status(&c.status)).small().color(weak));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let (label, verb, tip) = if c.running() { ("■", "stop", t!("Arrêter ce conteneur", "Stop this container")) } else { ("▶", "start", t!("Démarrer ce conteneur", "Start this container")) };
                                    if ui.add_enabled(!busy, egui::Button::new(label).small()).on_hover_text(tip).clicked() {
                                        act = Some(DockAct::Run(verb, vec![c.name.clone()]));
                                    }
                                });
                                r
                            });
                        let (r, row) = (row.inner, row.response);
                        // a dragged row dropped on this one: before it, or after it in its lower half
                        if let Some(p) = &project {
                            let after = ui.input(|i| i.pointer.hover_pos()).is_some_and(|m| m.y > row.rect.center().y);
                            if row.dnd_hover_payload::<DragRow>().is_some_and(|dr| dr.0 == *p && dr.1 != c.label()) {
                                let y = if after { row.rect.bottom() + 1.0 } else { row.rect.top() - 1.0 };
                                ui.painter().hline(row.rect.x_range(), y, egui::Stroke::new(2.0_f32, ui.visuals().selection.stroke.color));
                            }
                            if let Some(dr) = row.dnd_release_payload::<DragRow>().filter(|dr| dr.0 == *p && dr.1 != c.label()) {
                                act = Some(DockAct::Reorder { project: p.clone(), service: dr.1.clone(), target: c.label().to_string(), after });
                            }
                        }
                        let r = r.on_hover_text(format!("{}\n{}\n{}{}", c.name, c.image, c.status, if c.ports.is_empty() { String::new() } else { format!("\n{}", c.ports) }));
                        if r.clicked() {
                            act = Some(DockAct::Select(c.name.clone()));
                        }
                        r.context_menu(|ui| {
                            for (label, verb) in [(t!("Démarrer", "Start"), "start"), (t!("Arrêter", "Stop"), "stop"), (t!("Redémarrer", "Restart"), "restart")] {
                                if ui.button(label).clicked() {
                                    act = Some(DockAct::Run(verb, vec![c.name.clone()]));
                                    ui.close_menu();
                                }
                            }
                            if ui.add_enabled(c.running(), egui::Button::new(t!("Terminal dans le conteneur", "Terminal in the container"))).clicked() {
                                act = Some(DockAct::Shell(c.name.clone()));
                                ui.close_menu();
                            }
                        });
                    }
                    ui.add_space(6.0);
                }
                if egui::DragAndDrop::has_payload_of_type::<DragRow>(ui.ctx()) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                }
            });
        };
        match maxed {
            Some("list") => {
                egui::CentralPanel::default().frame(list_frame).show_inside(ui, list_ui);
            }
            Some("logs") => {}
            _ => {
                egui::SidePanel::left("docker_list").resizable(true).default_width(260.0).width_range(160.0..=600.0).frame(list_frame).show_inside(ui, list_ui);
            }
        }

        // ------------------------------------------------ logs of the chosen one
        let (current, empty) = (current.cloned(), list.is_empty());
        if maxed != Some("list") {
            egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| {
                let Some(c) = current else {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| ui.label(RichText::new(if empty { "" } else { t!("Choisissez un conteneur à gauche.", "Choose a container on the left.") }).weak()));
                    return;
                };
                egui::TopBottomPanel::top("docker_bar")
                    .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 6.0, bottom: 6.0 }))
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), Sense::hover());
                            ui.painter().circle_filled(rect.center(), 5.0, state_color(&c, weak));
                            ui.label(RichText::new(c.label()).strong());
                            ui.label(RichText::new(&c.status).small().color(weak));
                            ui.add_space(8.0);
                            let busy = self.docker.busy.is_some();
                            let one = || vec![c.name.clone()];
                            if c.running() {
                                if ui.add_enabled(!busy, egui::Button::new(t!("⟳ Redémarrer", "⟳ Restart"))).clicked() {
                                    act = Some(DockAct::Run("restart", one()));
                                }
                                if ui.add_enabled(!busy, egui::Button::new(t!("■ Arrêter", "■ Stop"))).clicked() {
                                    act = Some(DockAct::Run("stop", one()));
                                }
                                if ui.button(">_ Terminal").on_hover_text(t!("Un shell dans le conteneur (docker exec), dans un onglet", "A shell in the container (docker exec), in a tab")).clicked() {
                                    act = Some(DockAct::Shell(c.name.clone()));
                                }
                                if ui.selectable_label(self.tests.open, "🧪 Tests").on_hover_text(t!("Lancer des tests Odoo dans ce conteneur", "Run Odoo tests in this container")).clicked() {
                                    act = Some(DockAct::Tests);
                                }
                            } else if ui.add_enabled(!busy, egui::Button::new(t!("▶ Démarrer", "▶ Start"))).clicked() {
                                act = Some(DockAct::Run("start", one()));
                            }
                            if let Some(b) = &self.docker.busy {
                                spinner(ui);
                                ui.label(RichText::new(b).weak());
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                maximize::button(ui, view, "logs");
                                let hint = if self.docker.interactive {
                                    t!("Attaché : ce que vous tapez va au conteneur (ipdb…) · Ctrl+C l'interrompt", "Attached: what you type goes to the container (ipdb…) · Ctrl+C interrupts it")
                                } else {
                                    t!("Logs en lecture seule", "Read-only logs")
                                };
                                ui.label(RichText::new(hint).small().color(weak));
                            });
                        });
                        if self.tests.open && c.running() {
                            ui.separator();
                            self.tests_row(ui, &c.name);
                        }
                    });
                let bg = self.hl.colors.bg;
                egui::CentralPanel::default().frame(egui::Frame::none().fill(bg)).show_inside(ui, |ui| match self.docker.session {
                    Some(id) => {
                        if self.term.embedded_ui(ui, id, self.hl.colors, self.hl.is_light()) {
                            act = Some(DockAct::Restart);
                        }
                    }
                    None => {
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            spinner(ui);
                            ui.label(RichText::new(t!("Lecture des logs…", "Reading the logs…")).weak());
                        });
                    }
                });
            });
        }

        match act {
            Some(DockAct::Reload) => self.docker_load(),
            Some(DockAct::Reorder { project, service, target, after }) => self.docker_reorder(project, service, target, after),
            Some(DockAct::Select(name)) => {
                if self.docker.selected.as_deref() != Some(name.as_str()) {
                    self.docker_select(name);
                } else if let Some(id) = self.docker.session {
                    self.term.focus(id);
                }
            }
            Some(DockAct::Refollow) => self.docker_follow(false),
            Some(DockAct::Restart) => self.docker_follow(true),
            Some(DockAct::Tests) => {
                self.tests.open = !self.tests.open;
                if self.tests.open && self.tests.draft.is_empty() {
                    self.tests.draft = self.test_tags_here().or_else(|| self.settings.odoo_test_history.first().cloned()).unwrap_or_default();
                }
            }
            Some(DockAct::Run(verb, names)) => {
                let what = match verb {
                    "start" => t!("Démarrage", "Start"),
                    "stop" => t!("Arrêt", "Stop"),
                    _ => t!("Redémarrage", "Restart"),
                };
                let label = tf!("{what} de {}", "{what} of {}", names.iter().map(|n| self.docker_label(n)).collect::<Vec<_>>().join(", "));
                self.docker.busy = Some(format!("{label}…"));
                self.jobs.spawn(move || JobResult::DockerDone { result: docker::act(verb, &names), label });
            }
            Some(DockAct::Shell(name)) => {
                let ctx = self.egui_ctx.clone();
                let title = tf!("{} (conteneur)", "{} (container)", self.docker_label(&name));
                match self.term.open_tab(&ctx, Some(&format!("docker exec -it {name} bash")), None) {
                    Ok((id, _)) => self.open_view_tab(document::View::Terminal(id), title),
                    Err(e) => self.set_error(tf!("Terminal impossible à ouvrir : {e}", "Cannot open a terminal: {e}")),
                }
            }
            Some(DockAct::Up(c)) => {
                let (argv, cwd) = docker::compose_up(&c);
                let ctx = self.egui_ctx.clone();
                match self.term.open_command_tab(&ctx, &argv, Some(cwd)) {
                    Ok(id) => {
                        let title = format!("compose up {}", c.name);
                        self.term.set_name(id, title.clone());
                        self.open_view_tab(document::View::Terminal(id), title);
                    }
                    Err(e) => self.set_error(tf!("Terminal impossible à ouvrir : {e}", "Cannot open a terminal: {e}")),
                }
                // its containers appear in the list as they are created
                self.docker.loaded_at = None;
            }
            Some(DockAct::Move(true)) => self.show_big(BottomTab::Docker),
            Some(DockAct::Move(false)) => self.show_bottom(BottomTab::Docker, false),
            None => {}
        }
    }

    fn docker_label(&self, name: &str) -> String {
        match &self.docker.list {
            Some(Ok(l)) => l.iter().find(|c| c.name == name).map_or(name, |c| c.label()).to_string(),
            _ => name.to_string(),
        }
    }
}

fn state_color(c: &Container, weak: Color32) -> Color32 {
    match c.state.as_str() {
        "running" if c.status.contains("unhealthy") => crate::highlight::yellow(),
        "running" => crate::highlight::green(),
        "restarting" | "paused" | "created" => crate::highlight::yellow(),
        "dead" => crate::highlight::red(),
        _ => weak,
    }
}

/// "Up 2 hours (healthy)" → "2 hours", "Exited (0) 3 minutes ago" → "arrêté".
fn short_status(status: &str) -> String {
    if let Some(rest) = status.strip_prefix("Up ") {
        let t = rest.split(" (").next().unwrap_or(rest);
        if status.contains("(unhealthy)") {
            tf!("{t} · malade", "{t} · unhealthy")
        } else if status.contains("(health: starting)") {
            tf!("{t} · démarre", "{t} · starting")
        } else {
            t.to_string()
        }
    } else if status.starts_with("Exited") {
        t!("arrêté", "stopped").into()
    } else {
        status.split_whitespace().next().unwrap_or("").to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::short_status;

    #[test]
    fn statuses() {
        assert_eq!(short_status("Up 2 hours (healthy)"), "2 hours");
        assert_eq!(short_status("Up 5 seconds (health: starting)"), "5 seconds · démarre");
        assert_eq!(short_status("Exited (255) 3 seconds ago"), "arrêté");
        assert_eq!(short_status("Created"), "created");
    }
}
