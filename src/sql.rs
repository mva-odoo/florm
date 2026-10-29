//! PostgreSQL through `psql`: queries, their results as a table (CSV output), the databases of
//! the server, and EXPLAIN ANALYZE run in a transaction rolled back (the data is not changed),
//! its plan read as a tree of steps.

use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Rows kept from a result (the rest is counted, not shown).
pub const MAX_ROWS: usize = 5000;

/// A result: column names and rows (`None`: NULL).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    /// Rows returned in all (more than `rows` when cut).
    pub total: usize,
}

/// What a query gave.
#[derive(Clone, Debug)]
pub enum Outcome {
    Table(Table, Duration),
    /// The plan of EXPLAIN ANALYZE (its times are in it).
    Plan(Plan),
    /// A statement without rows (UPDATE, CREATE…): psql's message, often empty.
    Done(String, Duration),
}

/// The query running, to stop it.
pub type Running = Arc<Mutex<Option<Child>>>;

fn psql(conn: &str) -> std::process::Command {
    let mut c = crate::os::command("psql");
    c.args(["-X", "-q", "--csv", "-v", "ON_ERROR_STOP=1"]);
    if !conn.trim().is_empty() {
        c.args(["-d", conn.trim()]);
    }
    c.env("PGCONNECT_TIMEOUT", "8").stdin(Stdio::null());
    c
}

fn not_installed() -> String {
    t!("La commande psql est introuvable : installe le client PostgreSQL (paquet « postgresql-client » ou « postgresql »).", "The psql command was not found: install the PostgreSQL client (package “postgresql-client” or “postgresql”).").into()
}

/// Runs `cmds` (each a `-c`) in one psql, which `running` lets stop; its standard output.
fn run(conn: &str, cmds: &[String], running: &Running) -> Result<String, String> {
    let mut c = psql(conn);
    for q in cmds {
        c.args(["-c", q]);
    }
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|_| not_installed())?;
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    *running.lock().unwrap() = Some(child);
    let errs = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let mut text = String::new();
    let _ = out.read_to_string(&mut text);
    let errs = errs.join().unwrap_or_default();
    let status = running.lock().unwrap().take().map(|mut c| c.wait());
    match status {
        Some(Ok(s)) if s.success() => Ok(text),
        Some(Ok(_)) => Err(clean_error(&errs)),
        Some(Err(e)) => Err(e.to_string()),
        // taken by `stop`
        None => Err(t!("Requête arrêtée", "Query stopped").into()),
    }
}

/// Stops the query running, if any.
pub fn stop(running: &Running) {
    if let Some(mut c) = running.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

fn clean_error(stderr: &str) -> String {
    let e = stderr.trim();
    let e = e.strip_prefix("psql:").map_or(e, |r| r.split_once(": ").map_or(r, |x| x.1));
    if e.is_empty() { t!("psql a échoué sans message", "psql failed without a message").into() } else { e.to_string() }
}

/// Runs `query`; with `explain`, its plan by EXPLAIN (ANALYZE, BUFFERS) inside BEGIN … ROLLBACK.
pub fn query(conn: &str, query: &str, explain: bool, running: &Running) -> Result<Outcome, String> {
    let q = query.trim().trim_end_matches(';').trim().to_string();
    let t0 = Instant::now();
    if explain {
        let cmds = ["BEGIN".to_string(), format!("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {q}"), "ROLLBACK".to_string()];
        let out = run(conn, &cmds, running)?;
        let json = parse_csv(&out, usize::MAX).rows.into_iter().next().and_then(|r| r.into_iter().next().flatten()).unwrap_or_default();
        return Ok(Outcome::Plan(parse_plan(&json)?));
    }
    let out = run(conn, &[q], running)?;
    let took = t0.elapsed();
    if out.trim().is_empty() {
        return Ok(Outcome::Done(String::new(), took));
    }
    Ok(Outcome::Table(parse_csv(&out, MAX_ROWS), took))
}

/// The databases of the server `server` (a connection without its database; empty: the local one).
pub fn databases(server: &str) -> Result<Vec<String>, String> {
    let running = Running::default();
    // through the « postgres » database, always there: psql's default (the user's name) may not be
    let out = run(&with_database(server, "postgres"), &["SELECT datname FROM pg_database WHERE NOT datistemplate ORDER BY 1".into()], &running)?;
    Ok(parse_csv(&out, usize::MAX).rows.into_iter().filter_map(|r| r.into_iter().next().flatten()).collect())
}

/// `conn` pointing at database `db`: a name alone or empty → the name; "key=value …" → its
/// dbname; an URI → its path.
pub fn with_database(conn: &str, db: &str) -> String {
    let c = conn.trim();
    if c.starts_with("postgres://") || c.starts_with("postgresql://") {
        let (base, query) = c.split_once('?').map_or((c, None), |(b, q)| (b, Some(q)));
        let scheme_end = base.find("://").map_or(0, |i| i + 3);
        let host_part = base[scheme_end..].split('/').next().unwrap_or("");
        let mut out = format!("{}{host_part}/{db}", &base[..scheme_end]);
        if let Some(q) = query {
            out.push('?');
            out.push_str(q);
        }
        return out;
    }
    if c.contains('=') {
        let mut parts: Vec<String> = c.split_whitespace().filter(|p| !p.starts_with("dbname=")).map(String::from).collect();
        parts.push(format!("dbname={db}"));
        return parts.join(" ");
    }
    db.to_string()
}

/// A connection split in its server and its database: a name alone → ("", name); "key=value …"
/// → the rest and its dbname; an URI → the URI without its path, and its path.
pub fn split_conn(conn: &str) -> (String, String) {
    let c = conn.trim();
    if c.starts_with("postgres://") || c.starts_with("postgresql://") {
        let (base, query) = c.split_once('?').map_or((c, None), |(b, q)| (b, Some(q)));
        let scheme_end = base.find("://").map_or(0, |i| i + 3);
        let (host, db) = base[scheme_end..].split_once('/').unwrap_or((&base[scheme_end..], ""));
        let mut server = format!("{}{host}", &base[..scheme_end]);
        if let Some(q) = query {
            server.push('?');
            server.push_str(q);
        }
        return (server, db.to_string());
    }
    if c.contains('=') {
        let db = c.split_whitespace().find_map(|p| p.strip_prefix("dbname=")).unwrap_or("").to_string();
        let rest: Vec<&str> = c.split_whitespace().filter(|p| !p.starts_with("dbname=")).collect();
        return (rest.join(" "), db);
    }
    (String::new(), c.to_string())
}

/// psql's CSV: the first line names the columns; an empty field not quoted is NULL.
pub fn parse_csv(text: &str, max_rows: usize) -> Table {
    let mut records: Vec<Vec<Option<String>>> = Vec::new();
    let mut row: Vec<Option<String>> = Vec::new();
    let (mut field, mut quoted, mut in_quotes) = (String::new(), false, false);
    let mut chars = text.chars().peekable();
    let mut total = 0usize;
    let mut push_row = |row: &mut Vec<Option<String>>, records: &mut Vec<Vec<Option<String>>>| {
        total += 1;
        // the header, then at most `max_rows` rows
        if records.len() <= max_rows {
            records.push(std::mem::take(row));
        } else {
            row.clear();
        }
    };
    while let Some(ch) = chars.next() {
        match (in_quotes, ch) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => in_quotes = false,
            (true, c) => field.push(c),
            (false, '"') => (in_quotes, quoted) = (true, true),
            (false, ',') => {
                row.push(if field.is_empty() && !quoted { None } else { Some(std::mem::take(&mut field)) });
                quoted = false;
            }
            (false, '\r') => {}
            (false, '\n') => {
                row.push(if field.is_empty() && !quoted { None } else { Some(std::mem::take(&mut field)) });
                quoted = false;
                push_row(&mut row, &mut records);
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || quoted || !row.is_empty() {
        row.push(if field.is_empty() && !quoted { None } else { Some(field) });
        push_row(&mut row, &mut records);
    }
    let mut it = records.into_iter();
    let columns = it.next().map(|h| h.into_iter().map(Option::unwrap_or_default).collect()).unwrap_or_default();
    Table { columns, rows: it.collect(), total: total.saturating_sub(1) }
}

/// A plan of EXPLAIN (ANALYZE, FORMAT JSON).
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub root: PlanNode,
    pub planning_ms: f64,
    pub execution_ms: f64,
    /// The plan as PostgreSQL gave it (to paste in explain.dalibo.com…).
    pub json: String,
}

/// A step of a plan.
#[derive(Clone, Debug, Default)]
pub struct PlanNode {
    /// "Seq Scan", "Hash Join (Left)"…
    pub kind: String,
    /// What it reads: "on res_partner p using res_partner_pkey".
    pub on: String,
    /// Its conditions, filters, sort keys, buffers…
    pub details: Vec<String>,
    /// Time of the step with its children, and of the step alone (ms, all loops).
    pub total_ms: f64,
    pub self_ms: f64,
    /// Rows given by each loop, and those the planner expected.
    pub rows: f64,
    pub plan_rows: f64,
    pub loops: f64,
    /// "never executed": the step was not needed.
    pub never: bool,
    pub children: Vec<PlanNode>,
}

impl PlanNode {
    /// How far the planner's estimate was from the rows given: (factor ≥ 1, too few expected).
    pub fn misestimate(&self) -> Option<(f64, bool)> {
        if self.never {
            return None;
        }
        let (r, p) = (self.rows.max(1.0), self.plan_rows.max(1.0));
        Some(if r >= p { (r / p, true) } else { (p / r, false) })
    }

    /// The steps of the plan, depth first, with their depth.
    pub fn walk(&self) -> Vec<(usize, &PlanNode)> {
        fn go<'a>(n: &'a PlanNode, depth: usize, out: &mut Vec<(usize, &'a PlanNode)>) {
            out.push((depth, n));
            for c in &n.children {
                go(c, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        go(self, 0, &mut out);
        out
    }
}

/// The plan of `EXPLAIN (ANALYZE, FORMAT JSON)`.
pub fn parse_plan(json: &str) -> Result<Plan, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| tf!("Plan illisible : {e}", "Unreadable plan: {e}"))?;
    let top = v.get(0).ok_or_else(|| t!("Plan vide", "Empty plan").to_string())?;
    let root = top.get("Plan").map(plan_node).ok_or_else(|| t!("Plan vide", "Empty plan").to_string())?;
    let f = |k: &str| top.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    Ok(Plan { root, planning_ms: f("Planning Time"), execution_ms: f("Execution Time"), json: serde_json::to_string_pretty(&v).unwrap_or_else(|_| json.into()) })
}

fn plan_node(v: &serde_json::Value) -> PlanNode {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let f = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let mut kind = s("Node Type");
    let join = s("Join Type");
    if !join.is_empty() && join != "Inner" {
        kind = format!("{kind} ({join})");
    }
    if let Some(strategy) = v.get("Strategy").and_then(|x| x.as_str()).filter(|_| kind == "Aggregate") {
        kind = format!("{kind} ({strategy})");
    }
    let mut on = Vec::new();
    let rel = s("Relation Name");
    if !rel.is_empty() {
        let alias = s("Alias");
        on.push(if alias.is_empty() || alias == rel { tf!("sur {rel}", "on {rel}") } else { tf!("sur {rel} {alias}", "on {rel} {alias}") });
    }
    let cte = s("CTE Name");
    if !cte.is_empty() {
        on.push(format!("CTE {cte}"));
    }
    let index = s("Index Name");
    if !index.is_empty() {
        on.push(tf!("avec {index}", "using {index}"));
    }
    let mut details = Vec::new();
    for (key, label) in [("Index Cond", "Index Cond"), ("Recheck Cond", "Recheck Cond"), ("Hash Cond", "Hash Cond"), ("Merge Cond", "Merge Cond"), ("Join Filter", "Join Filter"), ("Filter", "Filter")] {
        let c = s(key);
        if !c.is_empty() {
            details.push(format!("{label}: {c}"));
        }
    }
    for (key, label) in [("Rows Removed by Filter", t!("lignes écartées par le filtre", "rows removed by the filter")), ("Rows Removed by Join Filter", t!("lignes écartées par la jointure", "rows removed by the join")), ("Rows Removed by Index Recheck", t!("lignes écartées à la revérification", "rows removed by the recheck"))] {
        let n = f(key);
        if n > 0.0 {
            details.push(format!("{} {label}", n as u64));
        }
    }
    if let Some(keys) = v.get("Sort Key").and_then(|x| x.as_array()) {
        let keys: Vec<&str> = keys.iter().filter_map(|k| k.as_str()).collect();
        let method = s("Sort Method");
        let space = v.get("Sort Space Used").and_then(|x| x.as_f64()).map(|kb| format!(", {} kB {}", kb as u64, s("Sort Space Type"))).unwrap_or_default();
        details.push(format!("Sort: {}{}", keys.join(", "), if method.is_empty() { String::new() } else { format!(" ({method}{space})") }));
    }
    if let Some(keys) = v.get("Group Key").and_then(|x| x.as_array()) {
        details.push(format!("Group: {}", keys.iter().filter_map(|k| k.as_str()).collect::<Vec<_>>().join(", ")));
    }
    let (hit, read) = (f("Shared Hit Blocks"), f("Shared Read Blocks"));
    if hit + read > 0.0 {
        details.push(tf!("Buffers : {} en mémoire, {} lus sur le disque", "Buffers: {} in memory, {} read from disk", hit as u64, read as u64));
    }
    let workers = f("Workers Launched");
    if workers > 0.0 {
        details.push(tf!("{} processus parallèles", "{} parallel workers", workers as u64));
    }
    let loops = f("Actual Loops");
    let total_ms = f("Actual Total Time") * loops;
    let children: Vec<PlanNode> = v.get("Plans").and_then(|x| x.as_array()).map(|a| a.iter().map(plan_node).collect()).unwrap_or_default();
    // its own time: without that of the steps under it (an InitPlan can be counted twice: never below 0)
    let below: f64 = children.iter().map(|c| c.total_ms).sum();
    PlanNode {
        kind,
        on: on.join(" "),
        details,
        total_ms,
        self_ms: (total_ms - below).max(0.0),
        rows: f("Actual Rows"),
        plan_rows: f("Plan Rows"),
        loops,
        never: loops == 0.0,
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_of_psql() {
        let t = parse_csv("id,name,n\n1,\"x,\"\"y\"\"\",\n2,\"\",3.5\n3,\"a\nb\",\n", 10);
        assert_eq!(t.columns, ["id", "name", "n"]);
        assert_eq!(t.total, 3);
        assert_eq!(t.rows[0], vec![Some("1".into()), Some("x,\"y\"".into()), None]);
        // an empty string is not NULL
        assert_eq!(t.rows[1][1], Some(String::new()));
        assert_eq!(t.rows[2][1].as_deref(), Some("a\nb"));
        let cut = parse_csv("a\n1\n2\n3\n", 2);
        assert_eq!((cut.rows.len(), cut.total), (2, 3));
        assert_eq!(parse_csv("", 5), Table::default());
    }

    #[test]
    fn databases_in_connections() {
        assert_eq!(with_database("", "odoo17"), "odoo17");
        assert_eq!(with_database("odoo16", "odoo17"), "odoo17");
        assert_eq!(with_database("host=localhost user=odoo dbname=a", "b"), "host=localhost user=odoo dbname=b");
        assert_eq!(with_database("postgresql://odoo:pw@db:5432/a?sslmode=disable", "b"), "postgresql://odoo:pw@db:5432/b?sslmode=disable");
        assert_eq!(with_database("postgres://localhost", "b"), "postgres://localhost/b");
    }

    #[test]
    fn connections_split() {
        assert_eq!(split_conn(""), (String::new(), String::new()));
        assert_eq!(split_conn("odoo17"), (String::new(), "odoo17".into()));
        assert_eq!(split_conn("host=localhost user=odoo dbname=a"), ("host=localhost user=odoo".into(), "a".into()));
        assert_eq!(split_conn("postgresql://odoo:pw@db:5432/a?sslmode=disable"), ("postgresql://odoo:pw@db:5432?sslmode=disable".into(), "a".into()));
        assert_eq!(split_conn("postgres://localhost"), ("postgres://localhost".into(), String::new()));
        // split then joined again: the same connection
        let (server, db) = split_conn("host=localhost dbname=a");
        assert_eq!(with_database(&server, &db), "host=localhost dbname=a");
    }

    #[test]
    fn json_plan() {
        let json = r#"[{"Plan": {"Node Type": "Hash Join", "Join Type": "Left", "Actual Total Time": 12.5, "Actual Loops": 1, "Actual Rows": 10, "Plan Rows": 1000,
            "Hash Cond": "(a.id = b.a_id)",
            "Plans": [
                {"Node Type": "Seq Scan", "Relation Name": "res_partner", "Alias": "a", "Actual Total Time": 2.0, "Actual Loops": 1, "Actual Rows": 50, "Plan Rows": 50, "Filter": "active", "Rows Removed by Filter": 7, "Shared Hit Blocks": 3, "Shared Read Blocks": 1},
                {"Node Type": "Index Scan", "Relation Name": "b", "Alias": "b", "Index Name": "b_pkey", "Actual Total Time": 0.2, "Actual Loops": 40, "Actual Rows": 1, "Plan Rows": 1},
                {"Node Type": "Seq Scan", "Relation Name": "c", "Alias": "c", "Actual Total Time": 0.0, "Actual Loops": 0, "Actual Rows": 0, "Plan Rows": 5}
            ]},
            "Planning Time": 0.3, "Execution Time": 12.9}]"#;
        let p = parse_plan(json).unwrap();
        assert_eq!((p.planning_ms, p.execution_ms), (0.3, 12.9));
        assert_eq!(p.root.kind, "Hash Join (Left)");
        // 12.5 - 2.0 - 0.2 × 40
        assert!((p.root.self_ms - 2.5).abs() < 1e-9);
        assert_eq!(p.root.misestimate(), Some((100.0, false)));
        let scan = &p.root.children[0];
        assert!(scan.on.contains("res_partner") && scan.details.iter().any(|d| d == "Filter: active"));
        assert!((p.root.children[1].total_ms - 8.0).abs() < 1e-9);
        assert!(p.root.children[1].on.contains("b_pkey"));
        assert!(p.root.children[2].never && p.root.children[2].misestimate().is_none());
        assert_eq!(p.root.walk().iter().map(|(d, _)| *d).collect::<Vec<_>>(), [0, 1, 1, 1]);
        assert!(parse_plan("not json").is_err());
    }
}
