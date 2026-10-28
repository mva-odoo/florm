//! PostgreSQL through `psql`: queries, their results as a table (CSV output), the databases of
//! the server, and EXPLAIN ANALYZE run in a transaction rolled back (the data is not changed).

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
    /// The plan of EXPLAIN ANALYZE, a line per node.
    Plan(Vec<String>, Duration),
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
        let cmds = ["BEGIN".to_string(), format!("EXPLAIN (ANALYZE, BUFFERS) {q}"), "ROLLBACK".to_string()];
        let out = run(conn, &cmds, running)?;
        let t = parse_csv(&out, usize::MAX);
        return Ok(Outcome::Plan(t.rows.into_iter().filter_map(|r| r.into_iter().next().flatten()).collect(), t0.elapsed()));
    }
    let out = run(conn, &[q], running)?;
    let took = t0.elapsed();
    if out.trim().is_empty() {
        return Ok(Outcome::Done(String::new(), took));
    }
    Ok(Outcome::Table(parse_csv(&out, MAX_ROWS), took))
}

/// The databases of the server (connected through `conn`, else the default one).
pub fn databases(conn: &str) -> Result<Vec<String>, String> {
    let running = Running::default();
    let out = run(conn, &["SELECT datname FROM pg_database WHERE NOT datistemplate ORDER BY 1".into()], &running)?;
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

/// For each line of a plan, its time (ms, all loops) when it says "actual time=a..b … loops=n".
pub fn node_times(plan: &[String]) -> Vec<Option<f64>> {
    plan.iter()
        .map(|l| {
            let (_, rest) = l.split_once("actual time=")?;
            let end: f64 = rest.split_once("..")?.1.split_whitespace().next()?.parse().ok()?;
            let loops: f64 = rest.split_once("loops=").and_then(|(_, x)| x.trim_end_matches(')').split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()).unwrap_or(1.0);
            Some(end * loops)
        })
        .collect()
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
    fn plan_times() {
        let plan: Vec<String> = [
            "Hash Join  (cost=1.0..2.0 rows=10 width=4) (actual time=0.050..12.500 rows=10 loops=1)",
            "  ->  Index Scan on a  (cost=0.1..1.0 rows=1 width=4) (actual time=0.010..0.200 rows=1 loops=40)",
            "Planning Time: 0.1 ms",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(node_times(&plan), [Some(12.5), Some(8.0), None]);
    }
}
