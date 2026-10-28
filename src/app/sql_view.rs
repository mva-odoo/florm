//! The SQL view: a PostgreSQL connection (through `psql`), a query, its result as a table, and
//! EXPLAIN ANALYZE with its slowest steps marked.

use super::*;
use crate::sql::{self, Outcome};

#[derive(Default)]
pub(super) struct SqlUi {
    /// A database name, "host=… user=… dbname=…" or "postgresql://…"; empty: psql's default.
    pub(super) conn: String,
    pub(super) query: String,
    dbs: Option<Result<Vec<String>, String>>,
    result: Option<Result<Outcome, String>>,
    running: sql::Running,
    busy: bool,
    /// Number of the last query sent (an older answer is dropped).
    gen: u64,
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
        s.busy = true;
        s.gen += 1;
        let (gen, conn, query, running) = (s.gen, s.conn.clone(), s.query.clone(), s.running.clone());
        self.jobs.spawn(move || JobResult::SqlDone(gen, sql::query(&conn, &query, explain, &running)));
    }

    pub(super) fn sql_done(&mut self, gen: u64, r: Result<Outcome, String>) {
        if gen == self.sql.gen {
            self.sql.busy = false;
            self.sql.result = Some(r);
        }
    }

    pub(super) fn sql_dbs_loaded(&mut self, r: Result<Vec<String>, String>) {
        self.sql.dbs = Some(r);
    }

    pub(super) fn sql_ui(&mut self, ui: &mut egui::Ui) {
        if self.install_guide(ui, crate::tools::Tool::Psql, true) {
            return;
        }
        let accent = self.hl.colors.accent;
        let fill = ui.visuals().panel_fill;
        let (mut run, mut stop, mut load_dbs) = (None, false, false);
        let hl = &self.hl;
        let s = &mut self.sql;

        // ------------------------------------------------ connection
        egui::TopBottomPanel::top("sql_bar")
            .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 8.0, bottom: 6.0 }))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    crate::view_bar::title(ui, "SQL");
                    crate::input::add(ui,
                        egui::TextEdit::singleline(&mut s.conn)
                            .hint_text(t!("base (odoo17), host=… user=… dbname=…, ou postgresql://…", "database (odoo17), host=… user=… dbname=…, or postgresql://…"))
                            .desired_width(340.0),
                    )
                    .on_hover_text(t!("Passé à psql -d. Le mot de passe : ~/.pgpass ou PGPASSWORD (évite de l'écrire ici).", "Given to psql -d. The password: ~/.pgpass or PGPASSWORD (better than writing it here)."));
                    let menu = ui.menu_button(t!("Bases ▾", "Databases ▾"), |ui| {
                        ui.set_min_width(200.0);
                        match &s.dbs {
                            None => {
                                spinner(ui);
                            }
                            Some(Err(e)) => {
                                ui.label(RichText::new(e).color(err()));
                            }
                            Some(Ok(list)) => {
                                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                                    for db in list {
                                        if ui.button(db).clicked() {
                                            s.conn = sql::with_database(&s.conn, db);
                                            ui.close_menu();
                                        }
                                    }
                                });
                            }
                        }
                    });
                    if menu.response.clicked() {
                        load_dbs = true;
                    }
                });
            });

        // ------------------------------------------------ the query
        let avail = ui.available_height();
        egui::TopBottomPanel::top("sql_query")
            .resizable(true)
            .default_height((avail * 0.35).max(120.0))
            .height_range(80.0..=(avail - 100.0).max(100.0))
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
                            "Le plan de la requête avec ses vrais temps : elle est exécutée dans une transaction annulée ensuite (ROLLBACK), les données ne changent pas. Les étapes les plus lentes sont en couleur.",
                            "The plan of the query with its real times: it runs in a transaction rolled back afterwards (ROLLBACK), the data does not change. The slowest steps are colored."
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
                            .desired_rows(6)
                            .layouter(&mut layouter),
                    );
                    if r.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter)) {
                        run = Some(false);
                    }
                });
            });

        // ------------------------------------------------ the result
        egui::CentralPanel::default().frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 6.0))).show_inside(ui, |ui| match &s.result {
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
            Some(Ok(Outcome::Plan(lines, took))) => {
                ui.label(RichText::new(tf!("Plan (EXPLAIN ANALYZE, annulé ensuite) · {}", "Plan (EXPLAIN ANALYZE, rolled back) · {}", ms(*took))).strong());
                ui.add_space(4.0);
                plan_view(ui, lines);
            }
        });

        if load_dbs {
            self.sql.dbs = None;
            let conn = self.sql.conn.clone();
            self.jobs.spawn(move || JobResult::SqlDbs(sql::databases(&conn)));
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

/// The plan, its slowest steps colored (red: half of the slowest or more, orange: a fifth).
fn plan_view(ui: &mut egui::Ui, lines: &[String]) {
    let times = sql::node_times(lines);
    let max = times.iter().flatten().cloned().fold(0.0, f64::max);
    egui::ScrollArea::both().id_salt("sql_plan").auto_shrink([false, false]).show(ui, |ui| {
        for (l, time) in lines.iter().zip(&times) {
            let mut text = RichText::new(l).monospace();
            match time {
                Some(t) if max > 0.0 && *t >= max * 0.5 => text = text.color(err()).strong(),
                Some(t) if max > 0.0 && *t >= max * 0.2 => text = text.color(warn()),
                _ if l.starts_with("Planning Time") || l.starts_with("Execution Time") => text = text.strong(),
                _ => {}
            }
            ui.label(text);
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
        let plan: Vec<String> = ["Seq Scan on res_partner  (actual time=0.010..9.000 rows=2 loops=1)", "Execution Time: 9.1 ms"].map(String::from).to_vec();
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let out = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    result_table(ui, &t);
                    plan_view(ui, &plan);
                });
            });
            shapes = out.shapes;
        }
        let s = texts(&shapes);
        for want in ["id", "name", "Azure", "NULL", "Seq Scan", "Execution Time"] {
            assert!(s.contains(want), "{want}: {s}");
        }
        assert_eq!(to_tsv(&t), "id\tname\n1\tAzure\n2\t");
    }
}
