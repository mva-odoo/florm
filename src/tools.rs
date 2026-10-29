//! Programs Florm drives but does not ship (gh, claude, docker, python): whether they are installed, and
//! how to install them on this system, shown by the view that needs them instead of an error.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Git,
    Gh,
    Claude,
    Docker,
    Psql,
    Python,
}

/// A step of an installation: what it does, and the command to type (None: only a note).
pub struct Step {
    pub label: String,
    pub command: Option<String>,
}

/// The way packages are installed here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    Arch,
    Debian,
    Fedora,
    Suse,
    OtherLinux,
    Mac,
    Windows,
}

impl System {
    pub fn name(self) -> &'static str {
        match self {
            System::Arch => "Arch Linux",
            System::Debian => "Debian / Ubuntu",
            System::Fedora => "Fedora / RHEL",
            System::Suse => "openSUSE",
            System::OtherLinux => "Linux",
            System::Mac => "macOS",
            System::Windows => "Windows",
        }
    }
}

/// A found answer is kept this long (the installation guide asks again after it).
const FRESH: Duration = Duration::from_secs(2);

static FOUND: Mutex<Option<HashMap<Tool, (bool, Instant)>>> = Mutex::new(None);

impl Tool {
    pub fn program(self) -> &'static str {
        match self {
            Tool::Git => "git",
            Tool::Gh => "gh",
            Tool::Claude => "claude",
            Tool::Docker => "docker",
            Tool::Psql => "psql",
            Tool::Python => if cfg!(windows) { "python" } else { "python3" },
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tool::Git => "Git",
            Tool::Gh => "GitHub CLI (gh)",
            Tool::Claude => "Claude Code (claude)",
            Tool::Docker => "Docker",
            Tool::Psql => "PostgreSQL (psql)",
            Tool::Python => "Python",
        }
    }

    /// What Florm does with it.
    pub fn purpose(self) -> &'static str {
        match self {
            Tool::Git => t!(
                "Florm affiche l'historique, les branches, les diffs et les commits avec la commande git, et prend ta configuration (nom, email, identifiants) telle quelle.",
                "Florm shows the history, branches, diffs and commits with the git command, and uses your own configuration (name, email, credentials) as it is."
            ),
            Tool::Gh => t!(
                "Florm lit et commente les pull requests avec la commande gh, connectée une fois à ton compte GitHub (pas de token à créer).",
                "Florm reads and comments on pull requests with the gh command, logged in once to your GitHub account (no token to create)."
            ),
            Tool::Claude => t!(
                "Florm parle à Claude par la commande claude (Claude Code), avec ta propre connexion (pas de clé d'API).",
                "Florm talks to Claude through the claude command (Claude Code), with your own login (no API key)."
            ),
            Tool::Docker => t!(
                "Florm liste les conteneurs, les démarre et suit leurs logs avec la commande docker.",
                "Florm lists the containers, starts them and follows their logs with the docker command."
            ),
            Tool::Psql => t!(
                "Florm envoie les requêtes SQL à PostgreSQL avec la commande psql (le client seul suffit, le serveur peut être ailleurs).",
                "Florm sends the SQL queries to PostgreSQL with the psql command (the client alone is enough, the server can be elsewhere)."
            ),
            Tool::Python => t!(
                "Le débogueur lance et suit les programmes Python avec debugpy, un module de Python.",
                "The debugger runs and follows the Python programs with debugpy, a module of Python."
            ),
        }
    }

    pub fn docs_url(self) -> &'static str {
        match self {
            Tool::Git => "https://git-scm.com/downloads",
            Tool::Gh => "https://github.com/cli/cli#installation",
            Tool::Claude => "https://docs.claude.com/en/docs/claude-code/setup",
            Tool::Docker => "https://docs.docker.com/engine/install/",
            Tool::Psql => "https://www.postgresql.org/download/",
            Tool::Python => "https://www.python.org/downloads/",
        }
    }

    /// Whether the command is installed. The answer is kept a moment (asked at every frame).
    pub fn installed(self) -> bool {
        let mut found = FOUND.lock().unwrap();
        let map = found.get_or_insert_with(HashMap::new);
        if let Some(&(yes, at)) = map.get(&self) {
            // once installed it stays so: no need to look again
            if yes || at.elapsed() < FRESH {
                return yes;
            }
        }
        let yes = locate(self.program());
        map.insert(self, (yes, Instant::now()));
        yes
    }

    /// Looks again at once (the "Check again" button).
    pub fn forget(self) {
        if let Some(map) = FOUND.lock().unwrap().as_mut() {
            map.remove(&self);
        }
    }

    /// The steps to install it on `sys`.
    pub fn steps(self, sys: System) -> Vec<Step> {
        let step = |label: &str, command: &str| Step { label: label.to_string(), command: Some(command.to_string()) };
        let note = |label: &str| Step { label: label.to_string(), command: None };
        let mut v = Vec::new();
        match (self, sys) {
            (Tool::Git, System::Arch) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo pacman -S --needed git")),
            (Tool::Git, System::Debian) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo apt update && sudo apt install -y git")),
            (Tool::Git, System::Fedora) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo dnf install -y git")),
            (Tool::Git, System::Suse) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo zypper install -y git")),
            (Tool::Git, System::OtherLinux) => v.push(note(t!(
                "Installe le paquet « git » de ta distribution : voir la documentation.",
                "Install the “git” package of your distribution: see the documentation."
            ))),
            (Tool::Git, System::Mac) => {
                v.push(step(t!("Installer avec Homebrew", "Install with Homebrew"), "brew install git"));
                v.push(note(t!(
                    "Sans Homebrew : la commande « xcode-select --install » installe git avec les outils de ligne de commande d'Apple.",
                    "Without Homebrew: the command “xcode-select --install” installs git with Apple's command line tools."
                )));
            }
            (Tool::Git, System::Windows) => {
                v.push(step(t!("Installer avec winget", "Install with winget"), "winget install --id Git.Git -e"));
                v.push(note(t!(
                    "Relance Florm après l'installation pour que git soit trouvé.",
                    "Restart Florm after the installation so that git is found."
                )));
            }
            (Tool::Gh, System::Arch) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo pacman -S --needed github-cli")),
            (Tool::Gh, System::Debian) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo apt update && sudo apt install -y gh")),
            (Tool::Gh, System::Fedora) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo dnf install -y gh")),
            (Tool::Gh, System::Suse) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo zypper install -y gh")),
            (Tool::Gh, System::OtherLinux) => v.push(note(t!(
                "Installe le paquet « gh » (ou « github-cli ») de ta distribution : voir la documentation.",
                "Install the “gh” (or “github-cli”) package of your distribution: see the documentation."
            ))),
            (Tool::Gh, System::Mac) => v.push(step(t!("Installer avec Homebrew", "Install with Homebrew"), "brew install gh")),
            (Tool::Gh, System::Windows) => v.push(step(t!("Installer avec winget", "Install with winget"), "winget install --id GitHub.cli -e")),
            // through powershell: the terminal may be cmd.exe
            (Tool::Claude, System::Windows) => v.push(step(t!("Installer", "Install"), "powershell -NoProfile -Command \"irm https://claude.ai/install.ps1 | iex\"")),
            (Tool::Claude, _) => v.push(step(t!("Installer", "Install"), "curl -fsSL https://claude.ai/install.sh | bash")),
            (Tool::Docker, System::Arch) => {
                v.push(step(t!("Installer les paquets", "Install the packages"), "sudo pacman -S --needed docker docker-compose docker-buildx"));
            }
            (Tool::Docker, System::Debian | System::Fedora) => {
                v.push(step(t!("Installer avec le script officiel de Docker", "Install with Docker's official script"), "curl -fsSL https://get.docker.com | sudo sh"));
            }
            (Tool::Docker, System::Suse) => v.push(step(t!("Installer les paquets", "Install the packages"), "sudo zypper install -y docker docker-compose")),
            (Tool::Docker, System::OtherLinux) => v.push(note(t!(
                "Installe Docker Engine et docker compose pour ta distribution : voir la documentation.",
                "Install Docker Engine and docker compose for your distribution: see the documentation."
            ))),
            (Tool::Docker, System::Mac) => {
                v.push(step(t!("Installer Docker Desktop avec Homebrew", "Install Docker Desktop with Homebrew"), "brew install --cask docker"));
                v.push(note(t!("Lance une fois l'application Docker (elle démarre le moteur).", "Start the Docker app once (it starts the engine).")));
            }
            (Tool::Psql, System::Arch) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo pacman -S --needed postgresql")),
            (Tool::Psql, System::Debian) => v.push(step(t!("Installer le client", "Install the client"), "sudo apt update && sudo apt install -y postgresql-client")),
            (Tool::Psql, System::Fedora) => v.push(step(t!("Installer le client", "Install the client"), "sudo dnf install -y postgresql")),
            (Tool::Psql, System::Suse) => v.push(step(t!("Installer le client", "Install the client"), "sudo zypper install -y postgresql")),
            (Tool::Psql, System::OtherLinux) => v.push(note(t!(
                "Installe le client PostgreSQL (psql) de ta distribution : voir la documentation.",
                "Install the PostgreSQL client (psql) of your distribution: see the documentation."
            ))),
            (Tool::Psql, System::Mac) => v.push(step(t!("Installer avec Homebrew", "Install with Homebrew"), "brew install libpq && brew link --force libpq")),
            (Tool::Psql, System::Windows) => {
                v.push(step(t!("Installer avec winget", "Install with winget"), "winget install --id PostgreSQL.PostgreSQL.17 -e"));
                v.push(note(t!("Ajoute le dossier bin de PostgreSQL au PATH, puis relance Florm.", "Add the bin folder of PostgreSQL to the PATH, then restart Florm.")));
            }
            (Tool::Python, System::Arch) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo pacman -S --needed python")),
            (Tool::Python, System::Debian) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo apt update && sudo apt install -y python3")),
            (Tool::Python, System::Fedora) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo dnf install -y python3")),
            (Tool::Python, System::Suse) => v.push(step(t!("Installer le paquet", "Install the package"), "sudo zypper install -y python3")),
            (Tool::Python, System::OtherLinux) => v.push(note(t!(
                "Installe le paquet « python3 » de ta distribution : voir la documentation.",
                "Install the “python3” package of your distribution: see the documentation."
            ))),
            (Tool::Python, System::Mac) => v.push(step(t!("Installer avec Homebrew", "Install with Homebrew"), "brew install python")),
            (Tool::Python, System::Windows) => {
                v.push(step(t!("Installer avec winget", "Install with winget"), "winget install --id Python.Python.3.12 -e"));
                v.push(note(t!("Relance Florm après l'installation pour que python soit trouvé.", "Restart Florm after the installation so that python is found.")));
            }
            (Tool::Docker, System::Windows) => {
                v.push(step(t!("Installer Docker Desktop avec winget", "Install Docker Desktop with winget"), "winget install --id Docker.DockerDesktop -e"));
                v.push(note(t!("Lance une fois Docker Desktop (il démarre le moteur).", "Start Docker Desktop once (it starts the engine).")));
            }
        }
        match self {
            Tool::Git => {
                v.push(step(t!("Dire à git ton nom (il apparaît dans les commits)", "Tell git your name (it appears in the commits)"), "git config --global user.name \"Prénom Nom\""));
                v.push(step(t!("Dire à git ton email", "Tell git your email"), "git config --global user.email \"toi@exemple.com\""));
            }
            Tool::Gh => v.push(step(t!("Te connecter à GitHub (par le navigateur)", "Log in to GitHub (through the browser)"), "gh auth login")),
            Tool::Claude => v.push(step(t!("Te connecter une fois, puis quitter avec /exit", "Log in once, then quit with /exit"), "claude")),
            Tool::Docker if !matches!(sys, System::Mac | System::Windows) => {
                v.push(step(t!("Démarrer Docker, et à chaque démarrage du système", "Start Docker, and at every start of the system"), "sudo systemctl enable --now docker"));
                v.push(step(t!("T'autoriser à utiliser Docker sans sudo", "Allow yourself to use Docker without sudo"), "sudo usermod -aG docker $USER"));
                v.push(note(t!(
                    "Ferme ta session et reconnecte-toi (ou redémarre) pour que le groupe docker compte, puis relance Florm.",
                    "Log out and back in (or restart) for the docker group to count, then start Florm again."
                )));
            }
            Tool::Docker | Tool::Psql | Tool::Python => {}
        }
        v
    }
}

/// The system Florm runs on.
pub fn system() -> System {
    if cfg!(windows) {
        System::Windows
    } else if cfg!(target_os = "macos") {
        System::Mac
    } else {
        linux_family(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
    }
}

/// The family of a distribution from its /etc/os-release (ID, then ID_LIKE: Omarchy and Manjaro
/// are Arch, Mint and Pop!_OS are Ubuntu…).
fn linux_family(os_release: &str) -> System {
    let field = |key: &str| {
        os_release
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches('"').to_lowercase())
            .unwrap_or_default()
    };
    let ids = format!("{} {}", field("ID"), field("ID_LIKE"));
    for id in ids.split_whitespace() {
        let sys = match id {
            "arch" | "manjaro" | "endeavouros" => System::Arch,
            "debian" | "ubuntu" => System::Debian,
            "fedora" | "rhel" | "centos" => System::Fedora,
            "opensuse" | "suse" | "opensuse-tumbleweed" | "opensuse-leap" | "sles" => System::Suse,
            _ => continue,
        };
        return sys;
    }
    System::OtherLinux
}

/// Whether `program` is in the PATH. Else it is looked for where the installers put it (the one
/// of Claude Code: ~/.local/bin), which then joins the PATH: Florm started from the desktop may
/// not have it.
pub fn locate(program: &str) -> bool {
    let names: Vec<String> = if cfg!(windows) { [".exe", ".cmd", ".bat"].iter().map(|e| format!("{program}{e}")).collect() } else { vec![program.to_string()] };
    let has = |d: &std::path::Path| names.iter().any(|n| d.join(n).is_file());
    let path = std::env::var_os("PATH").unwrap_or_default();
    if std::env::split_paths(&path).any(|d| has(&d)) {
        return true;
    }
    let home = crate::config::home();
    let usual: Vec<PathBuf> = vec![home.join(".local/bin"), "/usr/local/bin".into(), "/opt/homebrew/bin".into(), home.join(".cargo/bin")];
    match usual.into_iter().find(|d| has(d)) {
        Some(dir) => {
            let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
            dirs.push(dir);
            if let Ok(p) = std::env::join_paths(dirs) {
                std::env::set_var("PATH", p);
            }
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_families() {
        assert_eq!(linux_family("NAME=\"Omarchy\"\nID=omarchy\nID_LIKE=arch\n"), System::Arch);
        assert_eq!(linux_family("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"), System::Debian);
        assert_eq!(linux_family("ID=ubuntu\nID_LIKE=debian\n"), System::Debian);
        assert_eq!(linux_family("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n"), System::Fedora);
        assert_eq!(linux_family("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n"), System::Suse);
        assert_eq!(linux_family("ID=nixos\n"), System::OtherLinux);
        assert_eq!(linux_family(""), System::OtherLinux);
    }

    #[test]
    fn every_tool_has_steps_everywhere() {
        let all = [System::Arch, System::Debian, System::Fedora, System::Suse, System::OtherLinux, System::Mac, System::Windows];
        for tool in [Tool::Git, Tool::Gh, Tool::Claude, Tool::Docker, Tool::Psql, Tool::Python] {
            for sys in all {
                assert!(!tool.steps(sys).is_empty(), "{tool:?} {sys:?}");
            }
        }
        // logging in comes last
        assert_eq!(Tool::Gh.steps(System::Arch).last().unwrap().command.as_deref(), Some("gh auth login"));
    }
}
