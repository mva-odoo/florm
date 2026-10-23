//! Docker containers through the `docker` command: their list and state, start / stop / restart,
//! and their logs followed in a terminal, attached to the container when it reads its input
//! (`stdin_open` and `tty` in docker-compose.yml) so that an `ipdb` in it can be answered there.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    /// Folder and files of its compose project (`docker compose` is run there with them).
    pub compose_dir: Option<PathBuf>,
    pub compose_files: Vec<PathBuf>,
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
    let labels = parse_labels(&p.labels);
    let label = |key: &str| labels.iter().find(|(k, v)| k == key && !v.is_empty()).map(|(_, v)| v.clone());
    Ok(Container {
        project: label("com.docker.compose.project"),
        service: label("com.docker.compose.service"),
        compose_dir: label("com.docker.compose.project.working_dir").map(PathBuf::from),
        compose_files: label("com.docker.compose.project.config_files").map_or(Vec::new(), |v| v.split(',').map(PathBuf::from).collect()),
        id: p.id,
        name: p.names,
        image: p.image,
        state: p.state,
        status: p.status,
        ports: p.ports,
    })
}

/// `docker ps` labels, `k=v,k=v`: a value may itself hold commas (the compose files), a piece
/// without `=` belongs to the value before it.
fn parse_labels(s: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for piece in s.split(',') {
        match (piece.split_once('='), out.last_mut()) {
            (Some((k, v)), _) => out.push((k.to_string(), v.to_string())),
            (None, Some((_, v))) => {
                v.push(',');
                v.push_str(piece);
            }
            (None, None) => {}
        }
    }
    out
}

/// A compose project that `docker compose up` can bring up.
#[derive(Clone, Debug, PartialEq)]
pub struct Compose {
    /// Its project name (that of its containers).
    pub name: String,
    pub dir: PathBuf,
    /// Its files, none when found on disk (compose then reads those of `dir`, override included).
    pub files: Vec<PathBuf>,
    /// The file found, shown to say where it comes from.
    pub file: Option<PathBuf>,
}

/// Names compose looks for in a folder, in its order.
const COMPOSE_FILES: [&str; 4] = ["compose.yaml", "compose.yml", "docker-compose.yaml", "docker-compose.yml"];

/// The compose projects of the folder `root` and of its direct subfolders.
pub fn composes_in(root: &Path) -> Vec<Compose> {
    let mut dirs = vec![root.to_path_buf()];
    if let Ok(rd) = std::fs::read_dir(root) {
        let mut subs: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| e.path())
            .collect();
        subs.sort();
        dirs.extend(subs);
    }
    dirs.into_iter()
        .filter_map(|dir| {
            let file = COMPOSE_FILES.iter().map(|f| dir.join(f)).find(|f| f.is_file())?;
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let name = top_level_name(&text).unwrap_or_else(|| dir.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned()));
            Some(Compose { name: normalize_project(&name), dir, files: Vec::new(), file: Some(file) })
        })
        .collect()
}

/// `name:` at the top level of a compose file.
fn top_level_name(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.strip_prefix("name:")).map(|v| v.trim().trim_matches(['"', '\'']).to_string()).filter(|v| !v.is_empty() && !v.contains('$'))
}

/// The project name as compose makes it: lowercase, only letters, digits, `-` and `_`.
fn normalize_project(name: &str) -> String {
    name.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect()
}

/// `docker compose up -d` for the project `c`, and the folder to run it in.
pub fn compose_up(c: &Compose) -> (Vec<String>, PathBuf) {
    let mut a = vec!["docker".to_string(), "compose".into()];
    if !c.files.is_empty() {
        a.extend(["-p".to_string(), c.name.clone()]);
        for f in &c.files {
            a.extend(["-f".to_string(), f.to_string_lossy().into_owned()]);
        }
    }
    a.extend(["up".to_string(), "-d".into()]);
    (a, c.dir.clone())
}

/// A command to run in a container.
pub struct Exec {
    pub argv: Vec<String>,
    /// Folder of its compose project (`docker compose exec` is run there).
    pub cwd: Option<PathBuf>,
    /// (folder in the container, folder here), for the files of its tracebacks.
    pub mounts: Vec<(String, PathBuf)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Mount {
    #[serde(default)]
    source: String,
    #[serde(default)]
    destination: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Inspect {
    state: InspectState,
    config: InspectConfig,
    #[serde(default)]
    mounts: Vec<Mount>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectState {
    running: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectConfig {
    #[serde(default)]
    labels: Option<HashMap<String, String>>,
}

/// `cmd` run in the container `name`, interactive: `docker compose exec <service>` in the folder
/// of its project when it is a compose one (the service, even after it was recreated), else
/// `docker exec -it`.
pub fn exec_in(name: &str, cmd: &[String]) -> Result<Exec, String> {
    let out = run(&["inspect", name])?;
    let mut info: Vec<Inspect> = serde_json::from_str(&out).map_err(|e| format!("docker inspect : {e}"))?;
    let info = info.pop().ok_or_else(|| format!("Conteneur introuvable : {name}"))?;
    if !info.state.running {
        return Err(format!("{name} est arrêté : démarrez-le d'abord"));
    }
    let labels = info.config.labels.unwrap_or_default();
    let mounts = info.mounts.into_iter().filter(|m| !m.source.is_empty() && !m.destination.is_empty()).map(|m| (m.destination, PathBuf::from(m.source))).collect();
    let label = |k: &str| labels.get(k).filter(|v| !v.is_empty());
    let (mut argv, cwd) = match (label("com.docker.compose.service"), label("com.docker.compose.project")) {
        (Some(service), Some(project)) => {
            // the project by its name and files: it may not be named after its folder (`-p`)
            let mut a = vec!["docker".to_string(), "compose".into(), "-p".into(), project.clone()];
            for f in label("com.docker.compose.project.config_files").map_or("", |v| v).split(',').filter(|f| Path::new(f).is_file()) {
                a.extend(["-f".to_string(), f.to_string()]);
            }
            a.extend(["exec".to_string(), service.clone()]);
            (a, label("com.docker.compose.project.working_dir").map(PathBuf::from).filter(|d| d.is_dir()))
        }
        _ => (vec!["docker".to_string(), "exec".into(), "-it".into(), name.to_string()], None),
    };
    argv.extend(cmd.iter().cloned());
    Ok(Exec { argv, cwd, mounts })
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
        assert_eq!(c.compose_dir, None);
    }

    #[test]
    fn labels_with_commas() {
        let l = parse_labels("com.docker.compose.project=p,com.docker.compose.project.config_files=/x/a.yml,/x/b.yml,com.docker.compose.service=web");
        assert_eq!(l[1], ("com.docker.compose.project.config_files".to_string(), "/x/a.yml,/x/b.yml".to_string()));
        assert_eq!(l[2].1, "web");
    }

    #[test]
    fn compose_names() {
        assert_eq!(top_level_name("services:\n  name: x\nname: \"Mon.Projet\"\n").as_deref(), Some("Mon.Projet"));
        assert_eq!(top_level_name("services:\n  web:\n    name: x\n"), None);
        assert_eq!(normalize_project("Mon.Projet"), "monprojet");
        let c = Compose { name: "p".into(), dir: "/x".into(), files: vec!["/x/a.yml".into()], file: None };
        assert_eq!(compose_up(&c).0, ["docker", "compose", "-p", "p", "-f", "/x/a.yml", "up", "-d"]);
    }

    #[test]
    fn line_feeds() {
        assert_eq!(crlf(b"a\nb\r\nc\n"), b"a\r\nb\r\nc\r\n");
    }
}
