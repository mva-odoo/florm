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
        self.jobs.spawn(|| JobResult::DockerList(docker::list()));
    }

    pub(super) fn docker_listed(&mut self, list: Result<Vec<Container>, String>) {
        let d = &mut self.docker;
        d.loading = false;
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
                    Err(e) => self.set_status(format!("Logs de {name} : {e}")),
                }
            }
            Err(e) => self.set_status(e),
        }
    }

    pub(super) fn docker_done(&mut self, label: String, result: Result<(), String>) {
        self.docker.busy = None;
        match result {
            Ok(()) => self.set_status(format!("{label} : fait")),
            Err(e) => {
                self.set_status(format!("{label} : échec (voir Sortie)"));
                self.log(e);
            }
        }
        self.docker.loaded_at = None;
        self.docker_load();
    }

    pub(super) fn docker_ui(&mut self, ui: &mut egui::Ui) {
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

        // ------------------------------------------------ containers
        egui::SidePanel::left("docker_list")
            .resizable(true)
            .default_width(260.0)
            .width_range(160.0..=600.0)
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 8.0, right: 8.0, top: 6.0, bottom: 6.0 }))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Docker").strong().size(15.0));
                    // only the first reading: the list is read again every few seconds
                    if d.loading && d.list.is_none() {
                        ui.spinner();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if big {
                            if ui.small_button("⇲").on_hover_text("Afficher Docker dans le panneau du bas").clicked() {
                                act = Some(DockAct::Move(false));
                            }
                        } else if ui.small_button("⇱").on_hover_text("Afficher Docker en grand dans l'éditeur").clicked() {
                            act = Some(DockAct::Move(true));
                        }
                        if ui.small_button("⟳").on_hover_text("Relire la liste des conteneurs").clicked() {
                            act = Some(DockAct::Reload);
                        }
                    });
                });
                ui.separator();
                if let Some(Err(e)) = &d.list {
                    ui.label(RichText::new(e).color(crate::highlight::red()));
                    return;
                }
                if d.list.is_some() && list.is_empty() {
                    ui.label(RichText::new("Aucun conteneur.").weak());
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let mut k = 0;
                    while k < list.len() {
                        let project = list[k].project.clone();
                        let n = list[k..].iter().take_while(|c| c.project == project).count();
                        let group = &list[k..k + n];
                        k += n;
                        let names = |f: fn(&Container) -> bool| group.iter().filter(|c| f(c)).map(|c| c.name.clone()).collect::<Vec<_>>();
                        let up = group.iter().filter(|c| c.running()).count();
                        ui.horizontal(|ui| {
                            let title = project.as_deref().unwrap_or("Autres conteneurs");
                            ui.label(RichText::new(title).strong());
                            ui.label(RichText::new(format!("{up}/{n}")).small().color(weak));
                            if project.is_some() {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let stopped = names(|c| !c.running());
                                    let running = names(Container::running);
                                    if ui
                                        .add_enabled(!busy && !running.is_empty(), egui::Button::new("■ tout").small())
                                        .on_hover_text("Arrêter tous les conteneurs du projet, du bas de la liste vers le haut")
                                        .clicked()
                                    {
                                        act = Some(DockAct::Run("stop", running));
                                    }
                                    if ui
                                        .add_enabled(!busy && !stopped.is_empty(), egui::Button::new("▶ tout").small())
                                        .on_hover_text("Démarrer les conteneurs arrêtés dans l'ordre de la liste (poignée à points à gauche : glisser pour le changer), chacun une fois le précédent prêt (healthy)")
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
                                        let h = h.on_hover_text("Glisser pour changer l'ordre (▶ tout démarre dans cet ordre)");
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
                                        let (label, verb, tip) = if c.running() { ("■", "stop", "Arrêter ce conteneur") } else { ("▶", "start", "Démarrer ce conteneur") };
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
                                for (label, verb) in [("Démarrer", "start"), ("Arrêter", "stop"), ("Redémarrer", "restart")] {
                                    if ui.button(label).clicked() {
                                        act = Some(DockAct::Run(verb, vec![c.name.clone()]));
                                        ui.close_menu();
                                    }
                                }
                                if ui.add_enabled(c.running(), egui::Button::new("Terminal dans le conteneur")).clicked() {
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
            });

        // ------------------------------------------------ logs of the chosen one
        let (current, empty) = (current.cloned(), list.is_empty());
        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| {
            let Some(c) = current else {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(if empty { "" } else { "Choisissez un conteneur à gauche." }).weak()));
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
                            if ui.add_enabled(!busy, egui::Button::new("⟳ Redémarrer")).clicked() {
                                act = Some(DockAct::Run("restart", one()));
                            }
                            if ui.add_enabled(!busy, egui::Button::new("■ Arrêter")).clicked() {
                                act = Some(DockAct::Run("stop", one()));
                            }
                            if ui.button(">_ Terminal").on_hover_text("Un shell dans le conteneur (docker exec), dans un onglet").clicked() {
                                act = Some(DockAct::Shell(c.name.clone()));
                            }
                            if ui.selectable_label(self.tests.open, "🧪 Tests").on_hover_text("Lancer des tests Odoo dans ce conteneur").clicked() {
                                act = Some(DockAct::Tests);
                            }
                        } else if ui.add_enabled(!busy, egui::Button::new("▶ Démarrer")).clicked() {
                            act = Some(DockAct::Run("start", one()));
                        }
                        if let Some(b) = &self.docker.busy {
                            ui.spinner();
                            ui.label(RichText::new(b).weak());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let hint = if self.docker.interactive {
                                "Attaché : ce que vous tapez va au conteneur (ipdb…) · Ctrl+C l'interrompt"
                            } else {
                                "Logs en lecture seule"
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
                        ui.spinner();
                        ui.label(RichText::new("Lecture des logs…").weak());
                    });
                }
            });
        });

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
                    "start" => "Démarrage",
                    "stop" => "Arrêt",
                    _ => "Redémarrage",
                };
                let label = format!("{what} de {}", names.iter().map(|n| self.docker_label(n)).collect::<Vec<_>>().join(", "));
                self.docker.busy = Some(format!("{label}…"));
                self.jobs.spawn(move || JobResult::DockerDone { result: docker::act(verb, &names), label });
            }
            Some(DockAct::Shell(name)) => {
                let ctx = self.egui_ctx.clone();
                let title = format!("{} (conteneur)", self.docker_label(&name));
                match self.term.open_tab(&ctx, Some(&format!("docker exec -it {name} bash")), None) {
                    Ok((id, _)) => self.open_view_tab(document::View::Terminal(id), title),
                    Err(e) => self.set_status(e),
                }
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
            format!("{t} · malade")
        } else if status.contains("(health: starting)") {
            format!("{t} · démarre")
        } else {
            t.to_string()
        }
    } else if status.starts_with("Exited") {
        "arrêté".into()
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
