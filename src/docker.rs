//! Docker containers through the `docker` command: their list and state, start / stop / restart,
//! and their logs followed in a terminal, attached to the container when it reads its input
//! (`stdin_open` and `tty` in docker-compose.yml) so that an `ipdb` in it can be answered there.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

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
    crate::tools::Tool::Docker.installed()
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
    let info = info.pop().ok_or_else(|| tf!("Conteneur introuvable : {name}", "Container not found: {name}"))?;
    if !info.state.running {
        return Err(tf!("{name} est arrêté : démarrez-le d'abord", "{name} is stopped: start it first"));
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

/// The folders of the container `name` (container folder, folder here), once it is checked to
/// be running.
pub fn running_mounts(name: &str) -> Result<Vec<(String, PathBuf)>, String> {
    let out = run(&["inspect", name])?;
    let mut info: Vec<Inspect> = serde_json::from_str(&out).map_err(|e| format!("docker inspect : {e}"))?;
    let info = info.pop().ok_or_else(|| tf!("Conteneur introuvable : {name}", "Container not found: {name}"))?;
    if !info.state.running {
        return Err(tf!("{name} est arrêté : démarrez-le d'abord", "{name} is stopped: start it first"));
    }
    Ok(info.mounts.into_iter().filter(|m| !m.source.is_empty() && !m.destination.is_empty()).map(|m| (m.destination, PathBuf::from(m.source))).collect())
}

/// What `cmd` writes when run in the container `name` (`docker exec`, no terminal).
pub fn exec_output(name: &str, cmd: &[&str]) -> Result<String, String> {
    let mut args = vec!["exec", name];
    args.extend_from_slice(cmd);
    run(&args)
}

fn run(args: &[&str]) -> Result<String, String> {
    let out = crate::os::command("docker")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| t!("La commande docker est introuvable", "The docker command was not found").to_string())?;
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
        assert!(!c.running());
        let l = r#"{"ID":"1","Image":"nginx","Labels":"","Names":"web","State":"running","Status":"Up 2 hours"}"#;
        let c = parse_ps(l).unwrap();
        assert_eq!(c.project, None);
        assert_eq!(c.compose_dir, None);
    }

    #[test]
    fn labels_with_commas() {
        let l = parse_labels("com.docker.compose.project=p,com.docker.compose.project.config_files=/x/a.yml,/x/b.yml,com.docker.compose.service=web");
        assert_eq!(l[1], ("com.docker.compose.project.config_files".to_string(), "/x/a.yml,/x/b.yml".to_string()));
        assert_eq!(l[2].1, "web");
    }

}
