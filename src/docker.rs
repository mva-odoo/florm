//! Docker containers through the `docker` command: their list and state, start / stop / restart,
//! and their logs followed in a terminal, attached to the container when it reads its input
//! (`stdin_open` and `tty` in docker-compose.yml) so that an `ipdb` in it can be answered there.

use serde::Deserialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::OnceLock;

/// Lines of logs shown when a container is chosen.
const TAIL: &str = "500";

/// Keys that detach `docker attach` (the default Ctrl+P Ctrl+Q would eat Ctrl+P, the previous
/// command of ipdb); never needed: closing the view ends `docker attach`, not the container.
const DETACH_KEYS: &str = "--detach-keys=ctrl-^,ctrl-^";

#[derive(Clone, Debug, PartialEq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    /// created, running, paused, restarting, exited, dead.
    pub state: String,
    /// "Up 2 hours (healthy)", "Exited (0) 3 minutes ago".
    pub status: String,
    pub ports: String,
    /// docker compose project and service, when it is one of theirs.
    pub project: Option<String>,
    pub service: Option<String>,
}

impl Container {
    pub fn running(&self) -> bool {
        self.state == "running"
    }

    /// The service name inside its compose project, else the container name.
    pub fn label(&self) -> &str {
        self.service.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PsLine {
    #[serde(rename = "ID")]
    id: String,
    names: String,
    image: String,
    state: String,
    status: String,
    #[serde(default)]
    ports: String,
    #[serde(default)]
    labels: String,
}

/// Whether `docker` is installed (the Docker view is offered only then).
pub fn available() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| {
        let names: &[&str] = if cfg!(windows) { &["docker.exe"] } else { &["docker"] };
        std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| names.iter().any(|n| d.join(n).is_file())))
    })
}

/// Fields read by `docker ps` (`{{json .}}` would also compute the size of every container:
/// seconds, even tens of seconds).
const PS_FORMAT: &str = r#"{"ID":{{json .ID}},"Names":{{json .Names}},"Image":{{json .Image}},"State":{{json .State}},"Status":{{json .Status}},"Ports":{{json .Ports}},"Labels":{{json .Labels}}}"#;

/// Every container, running or not, by compose project then name.
pub fn list() -> Result<Vec<Container>, String> {
    let out = run(&["ps", "-a", "--format", PS_FORMAT])?;
    let mut list: Vec<Container> = out.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| parse_ps(l).ok()).collect();
    list.sort_by(|a, b| (a.project.is_none(), &a.project, &a.name).cmp(&(b.project.is_none(), &b.project, &b.name)));
    Ok(list)
}

fn parse_ps(line: &str) -> Result<Container, serde_json::Error> {
    let p: PsLine = serde_json::from_str(line)?;
    let label = |key: &str| p.labels.split(',').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('=')).map(str::to_string);
    Ok(Container {
        project: label("com.docker.compose.project"),
        service: label("com.docker.compose.service"),
        id: p.id,
        name: p.names,
        image: p.image,
        state: p.state,
        status: p.status,
        ports: p.ports,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Mount {
    #[serde(default)]
    source: String,
    #[serde(default)]
    destination: String,
}

fn mounts(name: &str) -> Vec<(String, PathBuf)> {
    let out = run(&["inspect", "-f", "{{json .Mounts}}", name]).unwrap_or_default();
    let m: Vec<Mount> = serde_json::from_str(out.trim()).unwrap_or_default();
    m.into_iter().filter(|m| !m.source.is_empty() && !m.destination.is_empty()).map(|m| (m.destination, PathBuf::from(m.source))).collect()
}

/// `docker start|stop|restart <names…>`.
/// Several are started one by one in their order, each once the previous one is ready (healthy,
/// when it has a healthcheck), and stopped the other way.
pub fn act(verb: &str, names: &[String]) -> Result<(), String> {
    match verb {
        "start" if names.len() > 1 => {
            for n in names {
                run(&["start", n])?;
                wait_ready(n);
            }
            Ok(())
        }
        "stop" if names.len() > 1 => names.iter().rev().try_for_each(|n| run(&["stop", n]).map(|_| ())),
        _ => {
            let mut args = vec![verb];
            args.extend(names.iter().map(String::as_str));
            run(&args).map(|_| ())
        }
    }
}

/// Waits (3 minutes at most) for a container started with a healthcheck to be healthy; one
/// without healthcheck, or that stopped, is not waited for.
fn wait_ready(name: &str) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while std::time::Instant::now() < until {
        let s = run(&["inspect", "-f", "{{.State.Running}} {{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}", name]).unwrap_or_default();
        let mut it = s.split_whitespace();
        let (running, health) = (it.next() == Some("true"), it.next().unwrap_or("none"));
        if !running || health != "starting" {
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// `names` (service → position) in the order chosen: those of `order` first, in it, then the
/// others by name.
pub fn order_key(order: &[String], service: &str) -> (usize, String) {
    (order.iter().position(|s| s == service).unwrap_or(usize::MAX), service.to_string())
}

/// How to show a container in the terminal of the view.
pub struct Follow {
    /// Command run in the terminal.
    pub argv: Vec<String>,
    /// Shown before its output (the last logs, `docker attach` only shows what comes next).
    pub prefill: Vec<u8>,
    /// The terminal sends what is typed to the container.
    pub interactive: bool,
    /// State and start time of the container when it was read: the same key, the same logs.
    pub key: String,
    /// (folder in the container, folder here), for the files of its tracebacks.
    pub mounts: Vec<(String, PathBuf)>,
}

/// The command following the logs of `name`: `docker attach` after the last logs when the
/// container reads its input, else `docker logs -f` (or just the logs when it is stopped).
/// `None` when the container is still in the state `unless` (not restarted since).
pub fn follow(name: &str, unless: Option<&str>) -> Result<Option<Follow>, String> {
    let info = run(&["inspect", "-f", "{{.State.Running}} {{.Config.OpenStdin}} {{.State.StartedAt}}", name])?;
    let key = info.trim().to_string();
    if unless == Some(key.as_str()) {
        return Ok(None);
    }
    let mut it = key.split_whitespace();
    let (running, stdin) = (it.next() == Some("true"), it.next() == Some("true"));
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if running && stdin {
        let out = crate::os::command("docker")
            .args(["logs", "--tail", TAIL, name])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("docker : {e}"))?;
        let mut prefill = out.stdout;
        prefill.extend(out.stderr);
        return Ok(Some(Follow { argv: argv(&["docker", "attach", "--sig-proxy=false", DETACH_KEYS, name]), prefill: crlf(&prefill), interactive: true, key, mounts: mounts(name) }));
    }
    let mut prefill = Vec::new();
    if running {
        prefill.extend_from_slice(
            "\x1b[2m(lecture seule : pour répondre à un ipdb, ajoutez stdin_open: true et tty: true à ce service dans docker-compose.yml)\x1b[0m\r\n".as_bytes(),
        );
        return Ok(Some(Follow { argv: argv(&["docker", "logs", "-f", "--tail", TAIL, name]), prefill, interactive: false, key, mounts: mounts(name) }));
    }
    Ok(Some(Follow { argv: argv(&["docker", "logs", "--tail", TAIL, name]), prefill, interactive: false, key, mounts: mounts(name) }))
}

/// Line feeds as the terminal needs them (a container without tty writes bare `\n`).
fn crlf(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len() + b.len() / 40);
    for (i, &c) in b.iter().enumerate() {
        if c == b'\n' && (i == 0 || b[i - 1] != b'\r') {
            out.push(b'\r');
        }
        out.push(c);
    }
    out
}

fn run(args: &[&str]) -> Result<String, String> {
    let out = crate::os::command("docker")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "La commande docker est introuvable".to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("docker {} : {}", args[0], String::from_utf8_lossy(&out.stderr).trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ps_line() {
        let l = r#"{"ID":"18fe7fecb18f","Image":"odoo.com/odoo:saas-19.3","Labels":"com.docker.compose.container-number=1,com.docker.compose.project=dockerized-saas,com.docker.compose.service=accounts,com.docker.compose.project.working_dir=/x","Names":"dockerized-saas-accounts-1","Ports":"","State":"exited","Status":"Exited (255) 3 seconds ago"}"#;
        let c = parse_ps(l).unwrap();
        assert_eq!(c.project.as_deref(), Some("dockerized-saas"));
        assert_eq!(c.service.as_deref(), Some("accounts"));
        assert_eq!(c.label(), "accounts");
        assert!(!c.running());
        let l = r#"{"ID":"1","Image":"nginx","Labels":"","Names":"web","State":"running","Status":"Up 2 hours"}"#;
        let c = parse_ps(l).unwrap();
        assert_eq!(c.label(), "web");
        assert_eq!(c.project, None);
    }

    #[test]
    fn line_feeds() {
        assert_eq!(crlf(b"a\nb\r\nc\n"), b"a\r\nb\r\nc\r\n");
    }
}
