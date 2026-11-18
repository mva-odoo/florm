//! Opening a folder of a WSL distribution (Windows): the list of distributions, the connection in
//! the background, then the folder to open there (typed with Tab completion, like any path).

use super::*;
use crate::remote::{self, Target};

impl App {
    /// The WSL distributions.
    pub(super) fn remote_items(&self) -> Vec<Item> {
        remote::wsl_distros().into_iter().map(|d| Item { label: d.clone(), detail: "WSL".into(), action: Action::Remote(Target::Wsl(d)) }).collect()
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

}
