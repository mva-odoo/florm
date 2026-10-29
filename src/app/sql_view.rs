//! The SQL view: a database of a PostgreSQL server (through `psql`), a query, its result as a
//! table, and EXPLAIN ANALYZE as a tree of steps with their time, the slowest marked.

use super::*;
use crate::sql::{self, Outcome};

#[derive(Default)]
pub(super) struct SqlUi {
    /// The server: "host=… user=…" or "postgresql://…" without database; empty: the local one.
    pub(super) server: String,
    pub(super) db: String,
    pub(super) query: String,
    /// The databases of the server (None: not read yet), and whether they are being read.
    dbs: Option<Result<Vec<String>, String>>,
    dbs_loading: bool,
    result: Option<Result<Outcome, String>>,
    running: sql::Running,
    busy: bool,
    /// Number of the last query sent (an older answer is dropped).
    gen: u64,
}

impl SqlUi {
    /// The connection given to psql: the server and the database.
    pub(super) fn conn(&self) -> String {
        if self.db.is_empty() { self.server.clone() } else { sql::with_database(&self.server, &self.db) }
    }

    /// Back to the connection `conn` (kept with the project).
    pub(super) fn set_conn(&mut self, conn: &str) {
        (self.server, self.db) = sql::split_conn(conn);
        self.dbs = None;
    }
}

impl App {
    pub(super) fn open_sql(&mut self) {
        self.open_view_tab(document::View::Sql, "SQL".into());
    }

    fn sql_run(&mut self, explain: bool) {
        let s = &mut self.sql;
        if s.busy || s.query.trim().is_empty() {
            return;
        }
        if s.db.is_empty() {
            s.result = Some(Err(t!("Choisis d'abord une base, en haut.", "Choose a database first, at the top.").into()));
            return;
        }
        s.busy = true;
        s.gen += 1;
        let (gen, conn, query, running) = (s.gen, s.conn(), s.query.clone(), s.running.clone());
        self.jobs.spawn(move || JobResult::SqlDone(gen, sql::query(&conn, &query, explain, &running)));
    }

    pub(super) fn sql_done(&mut self, gen: u64, r: Result<Outcome, String>) {
        if gen == self.sql.gen {
            self.sql.busy = false;
            self.sql.result = Some(r);
        }
    }

    pub(super) fn sql_dbs_loaded(&mut self, r: Result<Vec<String>, String>) {
        self.sql.dbs_loading = false;
        self.sql.dbs = Some(r);
    }

    pub(super) fn sql_ui(&mut self, ui: &mut egui::Ui) {
        if self.install_guide(ui, crate::tools::Tool::Psql, true) {
            return;
        }
        let accent = self.hl.colors.accent;
        let fill = ui.visuals().panel_fill;
        let (mut run, mut stop) = (None, false);
        let hl = &self.hl;
        let s = &mut self.sql;
        // the databases of the server, read when the view first shows
        let mut load_dbs = s.dbs.is_none() && !s.dbs_loading;

        // ------------------------------------------------ the database and the server
        egui::TopBottomPanel::top("sql_bar")
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 8.0, bottom: 6.0 }))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    crate::view_bar::title(ui, "SQL");
                    ui.label(RichText::new(t!("Base", "Database")).weak());
                    let list: &[String] = match &s.dbs {
                        Some(Ok(l)) => l,
                        _ => &[],
                    };
                    let current = list.iter().position(|d| *d == s.db);
                    let shown = if s.db.is_empty() { RichText::new(t!("choisir une base…", "choose a database…")).weak() } else { RichText::new(&s.db).strong() };
                    if let Some(k) = crate::pick::combo(ui, "sql_db", shown, list, current) {
                        s.db = list[k].clone();
                    }
                    if s.dbs_loading {
                        spinner(ui);
                    } else if ui.small_button("⟳").on_hover_text(t!("Relire la liste des bases", "Read the list of databases again")).clicked() {
                        load_dbs = true;
                    }
                    if let Some(Err(e)) = &s.dbs {
                        ui.label(RichText::new(format!("⚠ {}", e.lines().next().unwrap_or(""))).color(err())).on_hover_text(e.as_str());
                    }
                    ui.add_space(12.0);
                    ui.label(RichText::new(t!("Serveur", "Server")).weak());
                    let r = crate::input::add(ui, egui::TextEdit::singleline(&mut s.server).hint_text(t!("local (par défaut)", "local (default)")).desired_width(240.0)).on_hover_text(t!(
                        "Vide : le PostgreSQL de cette machine. Sinon « host=… port=… user=… » ou « postgresql://user@host:port ». Le mot de passe : ~/.pgpass ou PGPASSWORD (évite de l'écrire ici).",
                        "Empty: the PostgreSQL of this machine. Else “host=… port=… user=…” or “postgresql://user@host:port”. The password: ~/.pgpass or PGPASSWORD (better than writing it here)."
                    ));
                    // another server: its databases
                    let changed_id = r.id.with("changed");
                    if r.changed() {
                        ui.data_mut(|d| d.insert_temp(changed_id, true));
                    }
                    if r.lost_focus() && ui.data_mut(|d| d.remove_temp::<bool>(changed_id)).unwrap_or(false) {
                        load_dbs = true;
                    }
                });
            });

        // ------------------------------------------------ the query
        // a few lines by default; its lower edge is dragged to make it taller
        let avail = ui.available_height();
        egui::TopBottomPanel::top("sql_query")
            .resizable(true)
            .default_height(150.0_f32.min(avail * 0.5))
            .height_range(70.0..=(avail - 100.0).max(100.0))
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin::symmetric(10.0, 4.0)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    let r = ui.add_enabled(!s.busy, filled_button(t!("▶ Exécuter", "▶ Run"), accent)).on_hover_text(t!("Ctrl+Entrée", "Ctrl+Enter"));
                    if r.clicked() {
                        run = Some(false);
                    }
                    if ui
                        .add_enabled(!s.busy, egui::Button::new("⏱ EXPLAIN ANALYZE"))
                        .on_hover_text(t!(
                            "Le plan de la requête avec ses vrais temps, étape par étape : elle est exécutée dans une transaction annulée ensuite (ROLLBACK), les données ne changent pas. Les étapes les plus lentes sont en couleur.",
                            "The plan of the query with its real times, step by step: it runs in a transaction rolled back afterwards (ROLLBACK), the data does not change. The slowest steps are colored."
                        ))
                        .clicked()
                    {
                        run = Some(true);
                    }
                    if s.busy {
                        spinner(ui);
                        if ui.button(t!("■ Arrêter", "■ Stop")).clicked() {
                            stop = true;
                        }
                    }
                });
                egui::ScrollArea::vertical().id_salt("sql_query_scroll").auto_shrink([false, false]).show(ui, |ui| {
                    let mut layouter = |ui: &egui::Ui, text: &str, wrap: f32| {
                        let font = egui::FontId::monospace(13.0);
                        let mut job = egui::text::LayoutJob::default();
                        for line in text.split_inclusive('\n') {
                            let body = line.trim_end_matches('\n');
                            let spans = hl.line_spans("SQL", body);
                            let mut at = 0;
                            for (c, r) in &spans {
                                if r.start < at || r.end > body.len() {
                                    continue;
                                }
                                if r.start > at {
                                    job.append(&body[at..r.start], 0.0, egui::TextFormat::simple(font.clone(), ui.visuals().text_color()));
                                }
                                job.append(&body[r.clone()], 0.0, egui::TextFormat::simple(font.clone(), *c));
                                at = r.end;
                            }
                            job.append(&line[at..], 0.0, egui::TextFormat::simple(font.clone(), ui.visuals().text_color()));
                        }
                        job.wrap.max_width = wrap;
                        ui.fonts(|f| f.layout_job(job))
                    };
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut s.query)
                            .hint_text("SELECT id, name FROM res_partner WHERE active ORDER BY id DESC LIMIT 50")
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(4)
                            .layouter(&mut layouter),
                    );
                    if r.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter)) {
                        run = Some(false);
                    }
                });
            });

        // ------------------------------------------------ the result
        egui::CentralPanel::default().frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin::symmetric(10.0, 6.0))).show_inside(ui, |ui| match &s.result {
            None => {
                ui.label(RichText::new(t!("Écris une requête puis ▶ Exécuter (Ctrl+Entrée). Les résultats s'affichent ici.", "Write a query then ▶ Run (Ctrl+Enter). The results show here.")).weak());
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(err()).monospace());
            }
            Some(Ok(Outcome::Done(msg, took))) => {
                ui.label(RichText::new(tf!("✔ Fait en {}", "✔ Done in {}", ms(*took))).color(ok()));
                if !msg.is_empty() {
                    ui.label(msg);
                }
            }
            Some(Ok(Outcome::Table(t, took))) => {
                ui.horizontal(|ui| {
                    let shown = if t.total > t.rows.len() { tf!(" ({} affichées)", " ({} shown)", t.rows.len()) } else { String::new() };
                    ui.label(RichText::new(tf!("{} ligne(s){shown} · {}", "{} row(s){shown} · {}", t.total, ms(*took))).strong());
                    if ui.small_button(t!("Copier (tableur)", "Copy (spreadsheet)")).on_hover_text(t!("Les lignes affichées, séparées par des tabulations : à coller dans un tableur", "The rows shown, separated by tabs: to paste in a spreadsheet")).clicked() {
                        ui.ctx().copy_text(to_tsv(t));
                    }
                });
                ui.add_space(4.0);
                result_table(ui, t);
            }
            Some(Ok(Outcome::Plan(plan, _))) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tf!("Exécution {}", "Execution {}", plan_ms(plan.execution_ms))).strong());
                    ui.label(RichText::new(tf!("· planification {} · annulée ensuite (ROLLBACK)", "· planning {} · rolled back (ROLLBACK)", plan_ms(plan.planning_ms))).weak());
                    let tip = t!("Le plan en JSON, tel que PostgreSQL le donne : à coller dans explain.dalibo.com par exemple", "The plan as JSON, as PostgreSQL gives it: to paste in explain.dalibo.com for instance");
                    if ui.small_button(t!("Copier le plan (JSON)", "Copy the plan (JSON)")).on_hover_text(tip).clicked() {
                        ui.ctx().copy_text(plan.json.clone());
                    }
                });
                ui.add_space(4.0);
                plan_view(ui, plan, accent);
            }
        });

        if load_dbs {
            self.sql.dbs_loading = true;
            let server = self.sql.server.clone();
            self.jobs.spawn(move || JobResult::SqlDbs(sql::databases(&server)));
        }
        if stop {
            sql::stop(&self.sql.running);
        }
        if let Some(explain) = run {
            self.sql_run(explain);
        }
    }
}

fn ms(d: std::time::Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 1000.0 { format!("{ms:.0} ms") } else { format!("{:.2} s", ms / 1000.0) }
}

fn to_tsv(t: &sql::Table) -> String {
    let clean = |s: &str| s.replace(['\t', '\n'], " ");
    let mut out = t.columns.iter().map(|c| clean(c)).collect::<Vec<_>>().join("\t");
    for r in &t.rows {
        out.push('\n');
        out.push_str(&r.iter().map(|v| v.as_deref().map(clean).unwrap_or_default()).collect::<Vec<_>>().join("\t"));
    }
    out
}

/// The rows, columns resizable; NULL in grey; a click on a cell copies it.
fn result_table(ui: &mut egui::Ui, t: &sql::Table) {
    if t.columns.is_empty() {
        return;
    }
    let mono = egui::FontId::monospace(12.5);
    egui::ScrollArea::horizontal().id_salt("sql_result_h").show(ui, |ui| {
        let mut b = egui_extras::TableBuilder::new(ui).striped(true).resizable(true).cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for _ in &t.columns {
            b = b.column(egui_extras::Column::initial(140.0).at_least(40.0).clip(true));
        }
        b.header(22.0, |mut h| {
            for c in &t.columns {
                h.col(|ui| {
                    ui.label(RichText::new(c).strong());
                });
            }
        })
        .body(|body| {
            body.rows(20.0, t.rows.len(), |mut row| {
                let r = &t.rows[row.index()];
                for v in r {
                    row.col(|ui| {
                        let resp = match v {
                            Some(v) => ui.add(egui::Label::new(RichText::new(v.lines().next().unwrap_or("")).font(mono.clone())).sense(Sense::click()).truncate()),
                            None => ui.add(egui::Label::new(RichText::new("NULL").font(mono.clone()).weak().italics()).sense(Sense::click())),
                        };
                        if resp.on_hover_text(t!("Clic : copier la valeur", "Click: copy the value")).clicked() {
                            ui.ctx().copy_text(v.clone().unwrap_or_default());
                        }
                    });
                }
            });
        });
    });
}

fn plan_ms(ms: f64) -> String {
    if ms < 1.0 { format!("{ms:.3} ms") } else if ms < 1000.0 { format!("{ms:.1} ms") } else { format!("{:.2} s", ms / 1000.0) }
}

/// Widths of the columns of the plan: share of the time, own time, rows.
const PLAN_COLS: [f32; 3] = [110.0, 84.0, 150.0];

/// The steps of the plan as a tree, in the manner of explain.dalibo.com: for each, its own time
/// and its share of the execution (red: half of it or more, orange: a fifth), the rows given
/// against those expected (⚠ when ten times off), what it reads, and its conditions.
fn plan_view(ui: &mut egui::Ui, plan: &sql::Plan, accent: egui::Color32) {
    let total = plan.execution_ms.max(plan.root.total_ms).max(1e-9);
    let mono = egui::FontId::monospace(12.0);
    let cell = |ui: &mut egui::Ui, w: f32, add: &mut dyn FnMut(&mut egui::Ui)| {
        ui.allocate_ui_with_layout(egui::vec2(w, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(w);
            add(ui);
        });
    };
    egui::ScrollArea::both().id_salt("sql_plan").auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal(|ui| {
            for (w, h) in PLAN_COLS.iter().zip([t!("Part du temps", "Share of time"), t!("Temps propre", "Own time"), t!("Lignes (estimées)", "Rows (estimated)")]) {
                cell(ui, *w, &mut |ui| {
                    ui.label(RichText::new(h).small().weak());
                });
            }
            ui.label(RichText::new(t!("Étape", "Step")).small().weak());
        });
        ui.separator();
        for (depth, n) in plan.root.walk() {
            let share = (n.self_ms / total) as f32;
            let color = if share >= 0.5 { err() } else if share >= 0.2 { warn() } else { accent };
            let indent = depth as f32 * 18.0;
            ui.horizontal(|ui| {
                // the share of the execution: a bar and its percentage
                cell(ui, PLAN_COLS[0], &mut |ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(60.0, 10.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, ui.visuals().faint_bg_color);
                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(60.0 * share.clamp(0.0, 1.0), 10.0)), 2.0, color);
                    ui.label(RichText::new(format!("{:.0} %", share * 100.0)).small());
                });
                cell(ui, PLAN_COLS[1], &mut |ui| {
                    let t = RichText::new(if n.never { t!("jamais", "never").to_string() } else { plan_ms(n.self_ms) }).font(mono.clone());
                    ui.label(if share >= 0.2 { t.color(color).strong() } else { t }).on_hover_text(tf!("Avec les étapes en dessous : {}", "With the steps below: {}", plan_ms(n.total_ms)));
                });
                cell(ui, PLAN_COLS[2], &mut |ui| {
                    let loops = if n.loops > 1.0 { format!(" ×{}", n.loops as u64) } else { String::new() };
                    let text = format!("{} ({}){loops}", n.rows as u64, n.plan_rows as u64);
                    match n.misestimate() {
                        Some((f, under)) if f >= 10.0 => {
                            let tip = if under {
                                tf!("{f:.0} fois plus de lignes que prévu : les statistiques sont-elles à jour ? (ANALYZE la table)", "{f:.0} times more rows than expected: are the statistics up to date? (ANALYZE the table)")
                            } else {
                                tf!("{f:.0} fois moins de lignes que prévu", "{f:.0} times fewer rows than expected")
                            };
                            ui.label(RichText::new(format!("{text} ⚠")).font(mono.clone()).color(warn())).on_hover_text(tip);
                        }
                        _ => {
                            ui.label(RichText::new(text).font(mono.clone())).on_hover_text(t!("Lignes données par boucle (estimées par le planificateur) × boucles", "Rows given per loop (estimated by the planner) × loops"));
                        }
                    }
                });
                ui.add_space(indent);
                if depth > 0 {
                    ui.label(RichText::new("↳").weak());
                }
                let kind = RichText::new(&n.kind).strong();
                ui.label(if n.never { kind.weak() } else if share >= 0.2 { kind.color(color) } else { kind });
                if !n.on.is_empty() {
                    ui.label(RichText::new(&n.on).weak());
                }
            });
            // its conditions, under its name
            let left = PLAN_COLS.iter().sum::<f32>() + 3.0 * ui.spacing().item_spacing.x + indent + if depth > 0 { 18.0 } else { 0.0 };
            for d in &n.details {
                ui.horizontal(|ui| {
                    ui.add_space(left);
                    ui.label(RichText::new(d).font(egui::FontId::monospace(11.5)).weak());
                });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(shapes: &[egui::epaint::ClippedShape]) -> String {
        fn walk(s: &egui::Shape, out: &mut String) {
            match s {
                egui::Shape::Text(t) => {
                    out.push_str(t.galley.text());
                    out.push('|');
                }
                egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        let mut out = String::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    #[test]
    fn table_and_plan_are_drawn() {
        let t = sql::parse_csv("id,name\n1,Azure\n2,\n", 10);
        let plan = sql::parse_plan(r#"[{"Plan": {"Node Type": "Seq Scan", "Relation Name": "res_partner", "Actual Total Time": 9.0, "Actual Loops": 1, "Actual Rows": 2, "Plan Rows": 900, "Filter": "active"}, "Planning Time": 0.1, "Execution Time": 9.1}]"#).unwrap();
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let out = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    result_table(ui, &t);
                    plan_view(ui, &plan, egui::Color32::BLUE);
                });
            });
            shapes = out.shapes;
        }
        let s = texts(&shapes);
        for want in ["id", "name", "Azure", "NULL", "Seq Scan", "res_partner", "Filter: active", "2 (900) ⚠"] {
            assert!(s.contains(want), "{want}: {s}");
        }
        assert_eq!(to_tsv(&t), "id\tname\n1\tAzure\n2\t");
    }
}
