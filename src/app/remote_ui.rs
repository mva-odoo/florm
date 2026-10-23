//! Connecting to another machine (SSH, WSL): the list of machines, the connection in the
//! background, then the folder to open there (typed with Tab completion, like any path).

use super::*;
use crate::remote::{self, Target};

impl App {
    /// The hosts of ~/.ssh/config and the WSL distributions.
    pub(super) fn remote_items(&self) -> Vec<Item> {
        let mut v: Vec<Item> = remote::wsl_distros().into_iter().map(|d| Item { label: d.clone(), detail: "WSL".into(), action: Action::Remote(Target::Wsl(d)) }).collect();
        v.extend(remote::ssh_hosts().into_iter().map(|h| Item { label: h.clone(), detail: "SSH".into(), action: Action::Remote(Target::Ssh(h)) }));
        v
    }

    pub(super) fn remote_connect(&mut self, target: Target) {
        self.set_status(tf!("Connexion à {}…", "Connecting to {}…", target.label()));
        self.jobs.spawn(move || {
            let r = remote::connect(&target);
            JobResult::RemoteConnected(target, r)
        });
    }

    /// Connected: the folder to open is asked, from the home folder over there.
    pub(super) fn remote_connected(&mut self, target: Target, result: Result<PathBuf, String>) {
        match result {
            Ok(home) => {
                self.set_status(tf!("Connecté à {} : choisis le dossier à ouvrir", "Connected to {}: choose the folder to open", target.label()));
                self.open_overlay_with(Mode::Prompt(Prompt::OpenFolder), Vec::new(), format!("{}{}", home.display(), std::path::MAIN_SEPARATOR));
            }
            Err(e) => self.set_error(e),
        }
    }

    /// Mounts the host of an SSH project, then opens it.
    pub(super) fn remote_mount(&mut self, path: PathBuf, switch: bool) {
        let host = remote::describe(&path).unwrap_or_default();
        self.set_status(tf!("Connexion ({host})…", "Connecting ({host})…"));
        self.jobs.spawn(move || {
            let result = remote::mount_for(&path);
            JobResult::RemoteMounted { path, switch, result }
        });
    }

    pub(super) fn remote_mounted(&mut self, path: PathBuf, switch: bool, result: Result<(), String>) {
        match result {
            // still not mounted (it failed without saying so): stop there, not in a loop
            Ok(()) if remote::needs_mount(&path) => self.set_error(tf!("Le dossier {} n'est pas monté", "The folder {} is not mounted", path.display())),
            Ok(()) if switch => self.switch_project(path),
            Ok(()) => self.open_project(path),
            Err(e) => self.set_error(e),
        }
    }

    /// Closes the SSH project and unmounts its host.
    pub(super) fn remote_disconnect(&mut self) {
        let Some(root) = self.root.clone() else {
            self.set_status(t!("Aucun dossier ouvert", "No folder open"));
            return;
        };
        let Some(label) = remote::describe(&root).filter(|_| remote::ssh_location(&root).is_some()) else {
            self.set_status(t!("Le dossier ouvert n'est pas sur une machine SSH", "The open folder is not on an SSH machine"));
            return;
        };
        self.close_folder();
        if self.root.is_some() {
            // close_folder refused (unsaved changes): it said why
            return;
        }
        match remote::disconnect(&root) {
            Ok(true) => self.set_status(tf!("Déconnecté ({label})", "Disconnected ({label})")),
            Ok(false) => self.set_status(tf!("Dossier fermé ({label})", "Folder closed ({label})")),
            Err(e) => self.set_error(tf!("Démontage impossible : {e} (un terminal ou un programme l'utilise peut-être encore)", "Cannot unmount: {e} (a terminal or a program may still use it)")),
        }
    }
}
