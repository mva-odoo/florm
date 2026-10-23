# Florm

Éditeur de texte léger et rapide en Rust, inspiré de Sublime Text.

- **Rendu GPU** (egui/glow) : seules les lignes visibles sont dessinées, quelle que soit la taille du fichier.
- **Buffer en rope** (`ropey`) : les insertions et suppressions restent rapides sur des fichiers de plusieurs Mo.
- **Coloration incrémentale** avec les grammaires de Sublime Text (`syntect`).
- Un seul binaire (~17 Mo sous Linux, sans dépendance système à part OpenGL).

## Installation

Tous les fichiers sont dans la dernière [release](https://github.com/mva-odoo/florm/releases). Remplacez `<version>` par le numéro de la release (ex. `0.1.0`).

Florm ne demande rien d'autre qu'OpenGL pour s'afficher. Les fonctions avancées (Git, GitHub, Claude, linters…) utilisent des programmes externes : voir [Outils et connexions](#outils-et-connexions).

### Linux

Florm marche sous X11 comme sous Wayland. Les paquets installent `florm` dans `/usr/bin`, une entrée « Florm » dans le menu des applications et son icône.

#### Debian, Ubuntu, Linux Mint, Pop!_OS

```bash
sudo apt install ./florm_<version>_amd64.deb
```

`apt` installe aussi les bibliothèques graphiques nécessaires (OpenGL, Wayland, X11). Désinstaller : `sudo apt remove florm`.

#### Fedora, openSUSE, RHEL

```bash
sudo dnf install ./florm-<version>.x86_64.rpm        # Fedora, RHEL
sudo zypper install ./florm-<version>.x86_64.rpm     # openSUSE
```

#### Arch Linux, Manjaro, EndeavourOS, Omarchy

```bash
sudo pacman -U florm-<version>-1-x86_64.pkg.tar.zst
```

Désinstaller : `sudo pacman -R florm`.

#### Autres distributions : binaire seul

```bash
chmod +x florm-<version>-linux-x86_64
mv florm-<version>-linux-x86_64 ~/.local/bin/florm
```

Il faut les bibliothèques OpenGL (`libGL`, `libEGL`) et `libxkbcommon-x11`, presque toujours déjà présentes sur un bureau.

### Windows

**Le plus simple, sans avertissement de sécurité ni droits admin** : dans PowerShell (menu Démarrer → « PowerShell »), collez

```powershell
irm https://raw.githubusercontent.com/mva-odoo/florm/main/packaging/install.ps1 | iex
```

Le script télécharge la dernière version, la vérifie (SHA-256) et l'installe dans `%LOCALAPPDATA%\Programs\Florm`, avec **Florm** dans le menu Démarrer et la commande `florm` dans le terminal. Désinstaller : Paramètres → Applications → Florm. Relancer la commande met à jour.

Sans PowerShell, la release contient aussi :

- `florm-<version>-windows-x86_64.msi` : installateur classique (double-clic), même emplacement, même désinstallation ;
- `florm-<version>-windows-x86_64.exe` : le programme seul, **autonome**, à ranger où vous voulez.

Ces deux fichiers ne sont pas signés : téléchargés par le navigateur, **Windows SmartScreen** affiche « Windows a protégé votre ordinateur » au premier lancement. Cliquez sur **Informations complémentaires** puis **Exécuter quand même**. Le script PowerShell évite ce message : SmartScreen ne contrôle que les fichiers marqués par le navigateur.

Les préférences sont dans `%APPDATA%\florm`. Pour avoir Git, GitHub, Python… voir [Tout installer sous Windows](#tout-installer-sous-windows).

### macOS

**Le plus simple, sans avertissement ni `xattr`** : dans le Terminal, collez

```bash
curl -fsSL https://raw.githubusercontent.com/mva-odoo/florm/main/packaging/install.sh | sh
```

Le script télécharge la dernière version (Mac Intel et Apple Silicon), la vérifie (SHA-256) et installe **Florm** dans Applications (`~/Applications` si le dossier n'est pas modifiable). Relancer la commande met à jour.

Ou à la main : téléchargez `Florm-<version>-macos-universal.dmg`, ouvrez-le et glissez **Florm** dans **Applications**. Florm n'est pas signé par un compte développeur Apple : macOS met en quarantaine ce que télécharge le navigateur et peut dire que l'application « est endommagée ». Il faut alors retirer la quarantaine une fois :

```bash
xattr -dr com.apple.quarantine /Applications/Florm.app
```

Le script n'en a pas besoin : `curl` ne met pas la quarantaine. Les mises à jour intégrées (⬆ Florm x.y.z) non plus.

### Depuis les sources

Avec [Rust](https://rustup.rs) installé :

```bash
git clone https://github.com/mva-odoo/florm.git
cd florm
cargo build --release
./target/release/florm .
```

### Mettre à jour

Quand une nouvelle version sort, Florm l'indique au démarrage par **⬆ Florm x.y.z** en bas à droite. Un clic propose **Installer et redémarrer** : Florm télécharge la version de votre système, la vérifie (SHA-256) et remplace celle installée, puis **⟳ Redémarrer** la relance. Avec un paquet Linux (`.deb`, `.rpm`, Arch), la fenêtre système demande le mot de passe, comme pour installer un paquet. Si ça n'est pas possible, le menu propose toujours de **télécharger le fichier dans le navigateur** (le `.deb` sous Ubuntu, le paquet Arch sous Arch, le `.exe` sous Windows, le `.dmg` sous macOS…) ou d'ouvrir les notes de version. Pour vérifier à la main : **Aide → Rechercher une mise à jour**. Pour ne plus chercher : Préférences → Avancé → « Chercher les mises à jour ».

Inutile de désinstaller l'ancienne version : la nouvelle s'installe **par-dessus**, et les préférences sont gardées.

| Système | Mettre à jour |
|---|---|
| Debian / Ubuntu | `sudo apt install ./florm_<version>_amd64.deb` |
| Fedora | `sudo dnf install ./florm-<version>.x86_64.rpm` |
| openSUSE | `sudo zypper install ./florm-<version>.x86_64.rpm` |
| Arch | `sudo pacman -U florm-<version>-1-x86_64.pkg.tar.zst` |
| Binaire seul | remplacer `~/.local/bin/florm` par le nouveau fichier |
| Windows | relancer la commande `irm … \| iex`, ou ouvrir le nouveau `.msi`, ou remplacer l'ancien `.exe` |
| macOS | relancer la commande `curl … \| sh`, ou glisser le nouveau Florm dans Applications → **Remplacer** |

### Premier lancement

```bash
florm                # rouvre le dernier projet
florm .              # ouvre le dossier courant
florm fichier.py     # ouvre un fichier
```

Les préférences s'ouvrent avec **Ctrl+,** et sont enregistrées dans `~/.config/florm/settings.toml` (`%APPDATA%\florm\settings.toml` sous Windows). Le guide intégré est dans le menu Aide.

## Outils et connexions

L'édition, la recherche, la coloration, l'index des symboles (F12, complétion) et le lint XML / JSON marchent **sans rien installer**. Les autres fonctions s'appuient sur des programmes externes, cherchés dans le `PATH`. Aucun n'est obligatoire : sans lui, seule la fonction concernée est indisponible.

| Fonction | Programme | Obligatoire pour |
|---|---|---|
| Vue Git, commit, push, diff, rebase | `git` | Tout ce qui touche à Git |
| Pull requests GitHub | `gh` (GitHub CLI), connecté | Onglet Pull requests, « Créer une PR » |
| Claude dans l'éditeur | `claude` (Claude Code), connecté | Onglet Claude, ✨ message de commit |
| Vue Docker | `docker` | Onglet Docker (absent sans docker) |
| Dossiers distants SSH | `ssh` et `sshfs` (SSHFS-Win sous Windows) | « Se connecter à une autre machine » |
| Lint Python | `flake8` ou `ruff` (ou `pyflakes`, `pylint`) | Soulignements dans les fichiers Python |
| Formatage Python | `ruff` | Ctrl+Alt+F, « Corriger automatiquement » |
| Lint JavaScript | `eslint` (sinon `node`) | Soulignements dans les fichiers JS |
| Complétion intelligente, F2 renommer | un serveur de langage | Voir plus bas |
| Plugins d'exemple | `python3` | Les plugins écrits en Python |
| Icônes du terminal | une Nerd Font | Les symboles des prompts (starship, powerline) |

### Tout installer sous Windows

Dans PowerShell, d'un coup (chaque ligne est facultative) :

```powershell
winget install Git.Git                  # Git (aussi demandé par Claude Code)
winget install GitHub.cli               # Pull requests ; puis : gh auth login
winget install Python.Python.3.13       # lint Python, plugins
winget install OpenJS.NodeJS.LTS        # lint JavaScript, serveurs de langage npm
winget install Docker.DockerDesktop     # vue Docker
irm https://claude.ai/install.ps1 | iex # Claude Code ; puis : claude (pour se connecter)
```

Puis, **dans un nouveau terminal** (pour qu'il voie le nouveau `PATH`) : `pip install ruff flake8`. Relancez Florm après ces installations : il lit le `PATH` au démarrage. Le détail de chaque outil est ci-dessous.

### Git

Indispensable pour la vue Git (Ctrl+Alt+G), les couleurs de l'arborescence et le diff dans la marge.

| Système | Installation |
|---|---|
| Debian / Ubuntu | `sudo apt install git` |
| Fedora | `sudo dnf install git` |
| Arch | `sudo pacman -S git` |
| Windows | [git-scm.com](https://git-scm.com/download/win) (laisser « Git from the command line » coché) ou `winget install Git.Git` |
| macOS | `xcode-select --install` ou `brew install git` |

Configurez votre identité une fois, sinon les commits échouent :

```bash
git config --global user.name "Votre Nom"
git config --global user.email "vous@exemple.com"
```

Pour **pousser** vers GitHub, il faut une clé SSH ajoutée à votre compte ([guide GitHub](https://docs.github.com/fr/authentication/connecting-to-github-with-ssh)), ou bien `gh auth login` (ci-dessous), qui configure aussi Git en HTTPS.

### GitHub : pull requests

L'onglet **Pull requests** (Ctrl+Alt+R), les pastilles de PR et « Créer une PR » passent par **GitHub CLI** (`gh`). Florm ne demande aucun token : il utilise la connexion de `gh`.

| Système | Installation |
|---|---|
| Debian / Ubuntu | `sudo apt install gh` (ou le [dépôt officiel](https://github.com/cli/cli/blob/trunk/docs/install_linux.md) pour une version récente) |
| Fedora | `sudo dnf install gh` |
| Arch | `sudo pacman -S github-cli` |
| Windows | `winget install GitHub.cli` |
| macOS | `brew install gh` |

Puis connectez-vous, une fois :

```bash
gh auth login
```

Choisissez **GitHub.com**, **HTTPS** ou **SSH**, puis **Login with a web browser** : copiez le code affiché, collez-le sur la page qui s'ouvre et cliquez sur **Authorize**. Vérifiez avec `gh auth status`.

Pour des dépôts d'une organisation qui utilise le SSO, autorisez aussi l'organisation sur la page d'autorisation.

### Claude dans l'éditeur

L'onglet **Claude** (Ctrl+Alt+C) et le bouton ✨ (proposer un message de commit) lancent **Claude Code** (`claude`). Il faut un compte Claude (abonnement Pro / Max ou clé API).

```bash
# Linux, macOS
curl -fsSL https://claude.ai/install.sh | bash
```

```powershell
# Windows (PowerShell)
irm https://claude.ai/install.ps1 | iex
```

Puis connectez-vous, une fois, dans un terminal :

```bash
claude
```

et suivez la connexion dans le navigateur (ou `/login` dans Claude Code). Une fois `claude` connecté dans le terminal, Florm l'utilise tel quel.

### Python : lint et formatage

Le lint Python tourne pendant la frappe. Florm prend le premier disponible : `flake8`, sinon `ruff`, `pyflakes`, `pylint` (au choix dans Préférences → Lint, `python_linter`). Il est lancé depuis la racine du dépôt et lit donc `setup.cfg`, `.flake8`, `tox.ini` ou `pyproject.toml`.

Le plus simple : le bouton **« Installer flake8 »** du panneau Problèmes. Florm l'installe dans son propre dossier (`~/.local/share/florm/pytools`) sans toucher au Python du système. Il faut seulement `python3` et `pip`.

Ou à la main :

```bash
pipx install ruff flake8        # partout
sudo apt install flake8         # Debian / Ubuntu
sudo pacman -S flake8 ruff      # Arch
brew install ruff flake8        # macOS
```

Le **formatage** (Ctrl+Alt+F, `format_on_save`) et « Corriger automatiquement » utilisent `ruff`.

### JavaScript

`eslint` du projet (`node_modules/.bin/eslint`) ou du `PATH`. Sans eslint, Florm vérifie la syntaxe avec `node --check`.

### Serveurs de langage (LSP)

Sans serveur de langage, Florm complète et va à la définition grâce à son propre index. Un serveur ajoute la complétion exacte, la documentation au survol, les usages (Maj+F12) et le renommage dans tout le projet (F2). Florm détecte automatiquement :

| Langage | Serveur | Installation |
|---|---|---|
| Python | `pyright-langserver` ou `pylsp` | Bouton **« installer pylsp »** de la barre d'état, ou `npm i -g pyright`, ou `pipx install python-lsp-server` |
| JavaScript / TypeScript | `typescript-language-server` | `npm i -g typescript typescript-language-server` |
| Rust | `rust-analyzer` | `rustup component add rust-analyzer` |
| Go | `gopls` | `go install golang.org/x/tools/gopls@latest` |
| C / C++ | `clangd` | paquet `clangd` (Linux), `brew install llvm` (macOS), LLVM (Windows) |

Pour choisir une autre commande, dans `settings.toml` :

```toml
[lsp_servers]
python = "pyright-langserver --stdio"
```

### Terminal intégré

Il lance votre shell (`$SHELL` sous Linux et macOS, `cmd.exe` ou `%COMSPEC%` sous Windows ; à changer dans Préférences → Terminal). Pour les icônes des prompts (starship, powerline), installez une [Nerd Font](https://www.nerdfonts.com) : Florm la trouve tout seul.

### Se connecter en SSH ou à WSL

Palette de commandes → **Se connecter à une autre machine : SSH, WSL…** La liste propose les hôtes de `~/.ssh/config` (et des fichiers qu'il inclut) et, sous Windows, les distributions WSL. Pour un hôte absent de la liste, tapez `user@hôte` ou `user@hôte:port` puis Entrée. Florm demande ensuite le dossier à ouvrir, en partant de votre dossier personnel là-bas (Tab complète le chemin).

Le dossier s'ouvre comme un projet normal : fichiers, recherche, Git et linters marchent dessus, mais ils tournent sur votre PC. **Le terminal**, lui, ouvre un shell sur l'autre machine, dans le dossier du projet (`ssh -t` ou `wsl.exe --cd`). Le projet reste dans « Projet : changer… » et se reconnecte quand vous le rouvrez.

- **SSH (Linux, macOS)** : Florm monte le `/` du serveur avec `sshfs` dans `~/.local/share/florm/ssh/<hôte>/`. La connexion se fait **sans mot de passe** : il faut une clé acceptée par le serveur (`ssh-copy-id hôte`) ou chargée dans `ssh-agent`. La première fois, connectez-vous à la main avec `ssh hôte` pour accepter la clé du serveur. **Se déconnecter de la machine SSH** ferme le projet et démonte le dossier.
  Installer sshfs : `sudo pacman -S sshfs`, `sudo apt install sshfs`, `sudo dnf install fuse-sshfs` ; sur macOS `brew install --cask macfuse && brew install gromgit/fuse/sshfs-mac`.
- **SSH (Windows)** : avec [SSHFS-Win](https://github.com/winfsp/sshfs-win) (`winget install -e --id WinFsp.WinFsp` puis `winget install -e --id SSHFS-Win.SSHFS-Win`), le dossier est `\\sshfs.kr\user@hôte\…` (authentification par la clé `%USERPROFILE%\.ssh\id_rsa`).
- **WSL (Windows)** : Florm ouvre le dossier par `\\wsl.localhost\<distribution>\…` ; Git y est utilisé tel quel.

Les fichiers passent par le réseau : sur un gros projet distant, la première indexation (Aller au fichier, F12) prend plus de temps qu'en local.

### Vérifier que tout est en place

```bash
git --version
gh auth status
claude --version
flake8 --version || ruff --version
```

## Fonctionnalités

| | |
|---|---|
| Dossiers | Arborescence à gauche (clic droit : nouveau, renommer, supprimer — vers la corbeille, ou définitivement —, afficher dans le gestionnaire de fichiers…), **glisser-déposer pour déplacer** fichiers et dossiers, couleurs selon le statut git. **Au clavier** : Ctrl+Shift+E puis ↑ ↓ ← → Entrée (F2 renommer, Suppr). **Double-clic sur le bord** pour masquer la barre (clic sur la bande à gauche pour la réafficher). Sur le dossier d'un dépôt : sa branche, la branche distante suivie (↑ / ↓) et ses remotes au survol. En bas de la barre, une icône par vue (recherche, problèmes, terminal, sortie, Claude, Git, GitHub) : clic = en grand dans l'éditeur, clic droit = dans le panneau du bas (problèmes et sortie : toujours en bas). Clic droit sur un dossier → « Exclure de l'index » (marqué d'un œil barré : plus dans Ctrl+P, F12 ni la recherche) |
| Recherche globale | Ctrl+Shift+F : tout le dossier (respecte `.gitignore`), regex / casse / mot entier, filtres `*.py`, `models/*.py` (dossiers « models » où qu'ils soient, sous-dossiers compris), `./chemin` ou chemin absolu, et exclusions, remplacement avec **aperçu** (ancien barré en rouge → nouveau en vert), remplacer tout / un fichier / une seule occurrence, × pour écarter un résultat, fichiers repliables, F4 / Maj+F4 pour parcourir les résultats. Historique des recherches et des filtres (bouton ⏷, ou ↑ / ↓ dans le champ), filtres mémorisés entre les redémarrages. Résultats **colorés comme dans l'éditeur**, recherche dans le panneau du bas ou **dans un onglet** (⇱ / ⇲) |
| Projets | Chaque dossier ouvert devient un projet (Ctrl+Alt+P pour changer, « Projet : renommer… » dans la palette). La session est restaurée : fichiers ouverts, position des curseurs, dossiers dépliés, filtres de recherche. Sans argument, Florm rouvre le dernier projet |
| Éditeur divisé | Deux panneaux côte à côte ou l'un sous l'autre, chacun avec ses onglets : clic droit sur un onglet → « Diviser à droite / en bas », ou glisser l'onglet sur le bord droit / bas de l'éditeur (ou dans l'autre panneau). Ctrl+\\ divise à droite. Les onglets se réordonnent par glisser-déposer. Clic droit : déplacer dans l'autre panneau, fusionner, changer l'orientation, fermer les autres |
| Multi-curseur | Ctrl+D, Alt+F3, Ctrl+clic, Ctrl+Alt+↑/↓, Ctrl+Shift+L, Alt+glisser (sélection en colonne), coller réparti sur les curseurs |
| Git | Vue Git (Ctrl+Shift+G, ou clic sur la branche dans la barre d'état), barre en haut : branche avec ↑ commits à pousser / ↓ à récupérer, pull/push/fetch, branches, commit (Ctrl+Entrée) ; les fichiers indexés et modifiés sont les premières lignes du graphe. Les erreurs git restent affichées jusqu'à ce qu'on les ferme. Marques dans la gouttière |
| Diff en ligne | Marques vertes / jaunes / rouges dans la marge, mises à jour pendant la frappe. Clic sur une marque (ou Alt+F5 / Alt+Maj+F5) : l'ancienne et la nouvelle version s'affichent sous le bloc, avec **Revenir en arrière** (annulable par Ctrl+Z) |
| Diff dans l'éditeur | Ctrl+Alt+D : les lignes ajoutées/modifiées sur fond vert, les lignes supprimées réaffichées en rouge à leur place, bouton « ↺ Revenir » par bloc. Les onglets de diff ont aussi les fonds vert/rouge |
| Blame & historique de ligne | Ctrl+B : auteur, date relative et message du dernier commit de la ligne, affichés en bout de ligne (tient compte des modifications non enregistrées). Ctrl+Alt+B : tous les commits qui ont touché la ligne avec leur diff, et « Voir dans le graphe » |
| Clic droit dans l'éditeur | Couper / copier / coller, toutes les occurrences du mot, rechercher le mot dans le dossier, commenter / dupliquer / supprimer / déplacer la ligne, blame et historique de la ligne, voir ou annuler le changement git, copier le lien GitHub de la ligne, le chemin ou la référence `fichier:ligne` |
| Graphe des commits | Ctrl+Alt+G ou Ctrl+Shift+G : historique avec branches et merges dessinés ; en haut, comme dans tig, les modifications non indexées et indexées ; **en dessous, comme une pull request GitHub, le détail du commit puis tous ses fichiers et leurs changements l'un sous l'autre** (↑ ↓ : commit précédent / suivant, ← → : fichier précédent / suivant, Page ↑ ↓ : défiler, clic sur un en-tête : le replier). Dans un diff : « Ouvrir le fichier » (double-clic sur une ligne : à cette ligne), et par bloc ou pour tout le fichier : indexer / désindexer / annuler le changement (comme tig ; demande confirmation quand le changement serait perdu), tags et branches en étiquettes, filtre, détail du commit et fichiers modifiés, diff d'un fichier ou du commit complet, « Créer une branche ici ». Clic droit sur un commit : checkout (de ses branches ou en HEAD détachée), créer une branche, copier hash / message |
| Découper un commit | Clic droit sur un commit → « Découper ce commit en plusieurs… » (ou l'action « Découper » du rebase interactif) : un assistant liste ses changements par bloc, tu coches ce qui va dans la partie 1 — **ou seulement certaines lignes + / −**, comme « e » dans `git add -p` —, tu écris le message, « Valider la partie 1 », et ainsi de suite ; « Terminer » rejoue les commits suivants, « Annuler » remet tout comme avant (via `git rebase -i`) |
| Plusieurs dépôts | Les dépôts git présents dans le dossier ouvert (jusqu'à 3 niveaux) sont tous détectés ; la vue Git a un sélecteur de dépôt et suit automatiquement le dépôt du fichier actif |
| Linters | XML et JSON en direct (intégrés). **Python pendant la frappe** avec flake8 (ou ruff, pyflakes, pylint : `python_linter` dans les préférences), lancé depuis la racine du dépôt pour lire `setup.cfg` / `.flake8` / `tox.ini` / `pyproject.toml`. Survol d'un soulignement = message, clic droit = « Ignorer (# noqa) », **Ctrl+.** (ou l'ampoule) = corrections rapides : importer le nom manquant (index du projet), `# noqa`, `ruff --fix`, Claude, actions du serveur de langage. **Codes à ignorer par linter** (flake8, ruff, pylint, eslint) dans Préférences → Lint. Sans linter installé : bouton « Installer flake8 » dans Problèmes (installé dans `~/.local/share/florm/pytools`, sans toucher au Python système). JS via `eslint` → `node --check`. **Formatage** : Ctrl+Alt+F (`ruff format`, seulement les lignes sélectionnées s'il y a une sélection), « Corriger automatiquement » (`ruff --fix`), `format_on_save` dans les préférences |
| Terminal | Ctrl+` (ou Ctrl+Alt+T) : vrai terminal comme dans VS Code (shell interactif dans un pseudo-terminal : couleurs, complétion Tab, ↑ historique, `vim`, `htop`, `less`…). Plusieurs terminaux en onglets (＋), sélection à la souris, Ctrl+C interrompt (ou copie la sélection), Ctrl+V colle, molette pour l'historique. **Terminal dans un onglet de l'éditeur** (Ctrl+Shift+`, ou ⇱ Onglet) : il se place à côté d'un fichier comme tout onglet. Bouton « >_ Terminal » dans la barre d'état. Shell : `$SHELL` (ou `cmd` sous Windows), modifiable dans les préférences |
| Plugins | Commandes et hooks dans n'importe quel langage, voir [plugins/README.md](plugins/README.md) |
| Modèles | Ctrl+Alt+N : vues Odoo (formulaire, liste, recherche, héritage), action, menu, modèle Python, champ calculé, `__manifest__.py`, droits d'accès… insérés au curseur avec `${model}`, `${Model}`, `${module}` remplis d'après le fichier courant. Déclencheurs comme les snippets : `ipdb` + Tab → `import ipdb; ipdb.set_trace()`, `odoo_form`, `odoo_model`… Les siens avec Outils → Nouveau modèle… (depuis la sélection, rangé tout seul dans le dossier des modèles : `log` → `log.py`, déclencheur `log`). **Préférences → Modèles** pour les créer et les modifier |
| Pull requests GitHub | Ctrl+Alt+R. Par `gh` (sans token : `gh auth login` une fois) : liste des PR, description, commits, diff, commentaires sous les lignes du diff comme sur GitHub (aussi dans la marge de l'éditeur), répondre, résoudre, commenter une ligne, **suggestions appliquées** dans les fichiers en un clic, checkout |
| Claude | Ctrl+Alt+C : conversation avec Claude Code dans le panneau du bas, ou dans un onglet avec ⇱ (commande `claude`, sans clé), et à côté **la revue de ce qu'il a changé, bloc par bloc : garder ou rejeter**. ✨ dans la vue Git : message de commit proposé d'après le diff et la demande. **File de tâches** : une liste de demandes (+ File) envoyées une à une, un commit après chacune si coché. **Moins de tokens** : questions à choix avant de coder, définitions citées jointes, un commit comme contexte, conversation neuve par tâche, modèle choisi pour la file ou par tâche |
| Docker | Bouton Docker en bas de la barre latérale (clic : en grand, clic droit : dans le panneau du bas) : les conteneurs par projet compose avec leur état (● vert : en marche), ▶ ■ ⟳ par conteneur ou pour tout le projet. **La poignée à points** à gauche : glisser un conteneur pour changer l'ordre (gardé) ; **▶ tout** démarre dans cet ordre, chacun une fois le précédent prêt (healthy), **■ tout** arrête dans l'ordre inverse. **⬆** (en-tête du projet) lance `docker compose up -d` dans un onglet : crée et démarre tous les services, même ceux sans conteneur. Un fichier compose (`compose.yaml`, `docker-compose.yml`…) du dossier ouvert ou d'un de ses sous-dossiers dont aucun conteneur n'existe encore apparaît en haut de la liste avec **⬆ compose up**. À droite, les derniers logs puis la suite en direct. Si le service a `stdin_open: true` et `tty: true`, Florm s'y **attache** : un `ipdb` arrêté dans le conteneur se pilote là, sans `docker attach`. **>_ Terminal** : un shell dans le conteneur (`docker exec`). **🧪 Tests** : tags Odoo (`:MaClasse`, `/mon_module`…) remplis d'après le curseur, les derniers gardés, lancés par `docker compose exec <service> odoo --stop-after-init --test-enable --test-tags …` (commande modifiable) dans un onglet ; le résumé (« ✓ 12 tests ») dans la barre d'état. **Ctrl+F6** relance les derniers tests de n'importe où, **Ctrl+Maj+F6** le test sous le curseur, **▶** dans la marge des fichiers de test lance celui-là. **Ctrl+clic** sur une ligne `File "…", line N` d'un terminal ouvre le fichier |
| Aide | Aide → Guide de Florm : exemples de filtres de recherche, Git, terminal, modèles |
| Complétion, définition, survol, usages | Comme Sublime, sans rien installer : le projet est indexé en arrière-plan (classes, fonctions, méthodes, modèles Odoo `_name`, champs, records XML) ; F12 va à la définition (plusieurs résultats : liste des fichiers ; marche aussi sur `'sale.order'` ou `'module.xml_id'`) et les suggestions proposent les mots des fichiers ouverts et les symboles du projet. Avec un serveur de langage (pyright ou pylsp — bouton « installer pylsp » dans la barre d'état —, typescript-language-server, rust-analyzer, gopls, clangd) : complétion intelligente, définition exacte, F2 renommer dans tout le projet. Commande par langage dans `[lsp_servers]`. **Survol** d'un symbole : sa documentation (serveur de langage) ou ses définitions dans le projet. **Maj+F12** : ses usages (serveur de langage, sinon le mot entier dans le dossier) |
| Symboles et fil d'Ariane | Ctrl+R : classes, fonctions, méthodes, records / templates Odoo, titres Markdown. Au-dessus de l'éditeur : `dossier › fichier › Classe › méthode`, cliquable |
| Repli de code | Flèches dans la marge, Ctrl+Shift+[ / Ctrl+Shift+], Ctrl+K Ctrl+0 tout replier, Ctrl+K Ctrl+J tout déplier ; « ⋯ N lignes » cliquable sur un bloc replié |
| En-tête figé, règles | En défilant, les lignes `class` / `def` (titres, records…) qui contiennent le haut de l'écran y restent ; clic pour y aller (`sticky_scroll`). Règle verticale à 80 caractères par défaut, réglable dans Préférences → Apparence (`rulers`, ex. `[79, 120]`) |
| Conflits de merge | Ta version en vert, la version entrante en bleu ; barre « Garder le mien / le leur / les deux » (annulable), puis « marquer comme résolu » ; bandeau merge / cherry-pick / rebase en cours (Continuer / Abandonner) dans la vue Git |
| Rechercher dans la sélection | Bouton « Sél. » de Ctrl+F / Ctrl+H ; « Garder la casse » au remplacement (chat → chien, Chat → Chien, CHAT → CHIEN), aussi dans la recherche globale |
| Aperçu Markdown, images | Ctrl+Shift+V (onglet) ou Ctrl+K V (à côté, suit la frappe) ; png, jpg, gif, webp, bmp, ico, svg s'ouvrent dans un onglet image |
| Travail non enregistré gardé | À la fermeture ou en changeant de projet, les modifications non enregistrées (et les onglets sans titre) sont gardées et rouvertes, sans question (désactivable) |
| Git avancé | Cherry-pick, revert, rebase interactif (réordonner, changer un message, fusionner, supprimer), comparer deux commits ou un commit avec une branche, **stash** (mettre de côté, voir, appliquer, reprendre, supprimer) |
| Apparence | Interface en français ou en anglais (Préférences → Apparence → Langue), thèmes avec aperçus, couleur d'accentuation, **texte adouci** (texte moins gras, surtout sur les thèmes clairs), barre de menus masquable (menus sous ☰), **thème à soi** (Préférences → Apparence : toutes les couleurs de l'éditeur et du code, appliquées en direct) |
| Divers | Palette de commandes (accents facultatifs, les dernières utilisées en tête), Ctrl+P fuzzy, retour à la ligne automatique (Alt+Z), Tab pour compléter les chemins (ouvrir, enregistrer sous), minimap, guides d'indentation, crochets appariés, auto-fermeture, rechercher/remplacer (regex), polices au choix parmi celles installées (Préférences → Apparence ; par défaut Consolas / Segoe UI sous Windows), thèmes `.tmTheme` (intégrés : « Florm » sombre, « Florm Light » et « Light+ (VS Code) » clairs). Un fichier non enregistré modifié ou supprimé par un autre programme : un bandeau demande quelle version garder, Ctrl+S n'écrase pas l'autre sans demander. Clic sur le message de la barre d'état : les derniers messages (erreurs en rouge) |

## Raccourcis (comme Sublime)

| Raccourci | Action |
|---|---|
| Ctrl+Shift+P / F1 | Palette de commandes |
| Ctrl+P | Aller au fichier, les récents d'abord (`nom:42` pour aller à la ligne, `:42` dans le fichier courant) |
| Ctrl+G | Aller à la ligne |
| Alt+G | Aller à la ligne et la sélectionner : `42`, `10-20` pour plusieurs lignes, vide + Entrée = la ligne courante |
| Ctrl+R | Aller au symbole du fichier |
| Alt+L | Afficher le fichier courant dans l'arborescence (aussi : clic droit sur l'onglet) |
| Alt+- / Alt++ | Position précédente / suivante du curseur (aussi entre fichiers ; boutons précédent / suivant de la souris) |
| Ctrl+K Ctrl+Z | Annuler le changement git sous le curseur (Ctrl+Z pour le récupérer) |
| Ctrl+K Ctrl+Shift+Z | Remettre tout le fichier comme au dernier commit |
| Ctrl+Espace / F12 / F2 | Suggestions / aller à la définition / renommer le symbole |
| Maj+F12 | Rechercher les usages du symbole |
| Ctrl+F2 / F8 / Maj+F8 | Signet sur la ligne / suivant / précédent (Ctrl+Maj+F2 : tout retirer ; « Signet : liste » dans la palette) |
| Ctrl+Shift+[ / Ctrl+Shift+] | Replier / déplier le bloc |
| Ctrl+\\ | Diviser l'éditeur à droite |
| Ctrl+Shift+V / Ctrl+K V | Aperçu Markdown / à côté |
| Ctrl+F / Ctrl+H | Rechercher / Remplacer (Entrée, Maj+Entrée, Alt+Entrée = tout sélectionner) |
| F3 / Maj+F3 | Résultat suivant / précédent |
| Ctrl+Shift+F | Rechercher dans le dossier |
| F4 / Maj+F4 | Résultat global suivant / précédent |
| Ctrl+, | Préférences |
| Ctrl+. | Corrections rapides |
| Alt+Z | Retour à la ligne automatique |
| Ctrl+Alt+P | Changer de projet |
| Ctrl+Shift+E | Le clavier dans l'arborescence (↑ ↓ ← → Entrée) |
| Ctrl+Shift+` | Nouveau terminal dans un onglet |
| Ctrl+Alt+N | Insérer un modèle |
| Ctrl+Alt+G | Graphe des commits |
| Ctrl+Alt+R | Pull requests GitHub |
| Ctrl+F6 / Ctrl+Maj+F6 | Tests Odoo : relancer les derniers / lancer celui sous le curseur (conteneur Docker) |
| Ctrl+Alt+C | Claude |
| Ctrl+Alt+D | Diff git dans l'éditeur (vert / rouge) |
| Ctrl+Alt+F | Formater le fichier Python, ou les lignes sélectionnées (ruff) |
| Alt+F5 / Alt+Maj+F5 | Changement git suivant / précédent (diff en ligne) |
| Ctrl+D | Sélectionner le mot, puis ajouter l'occurrence suivante |
| Alt+F3 | Sélectionner toutes les occurrences |
| Ctrl+Alt+↑/↓, Alt+Shift+↑/↓ | Ajouter un curseur au-dessus / en dessous |
| Ctrl+Shift+L | Diviser la sélection en lignes |
| Ctrl+L | Sélectionner la ligne |
| Ctrl+Shift+D / Ctrl+Shift+K | Dupliquer / supprimer la ligne |
| Ctrl+Shift+↑/↓ | Déplacer la ligne |
| Ctrl+/ | Commenter |
| Ctrl+] / Ctrl+[ | Indenter / désindenter |
| Ctrl+J | Joindre les lignes |
| Ctrl+Entrée / Ctrl+Shift+Entrée | Insérer une ligne après / avant |
| Ctrl+M | Aller au crochet correspondant |
| F9 | Trier les lignes |
| Ctrl+K Ctrl+B | Afficher/masquer la barre latérale |
| Ctrl+K Ctrl+U / Ctrl+K Ctrl+L | Majuscules / minuscules |
| Ctrl+S, Ctrl+Shift+S, Ctrl+Alt+S | Enregistrer, enregistrer sous, tout enregistrer |
| Ctrl+W, Ctrl+Shift+T | Fermer l'onglet, rouvrir le dernier fermé |
| Ctrl+Tab, Ctrl+PageUp/PageDown | Changer d'onglet |
| Ctrl+Shift+M | Problèmes (lint) |
| Ctrl+B | Blame de la ligne (suit le curseur ; Ctrl+B à nouveau pour masquer) |
| Ctrl+Alt+B | Historique complet de la ligne (tous les commits qui l'ont modifiée, avec leur diff) |
| F5 | (plugin d'exemple) exécuter le fichier Python |

Sous macOS, Ctrl = Cmd. **Tous les raccourcis se modifient** dans Préférences → Raccourcis : clic sur un raccourci, puis la nouvelle combinaison (Échap annule) ; les conflits sont signalés, « ↺ » rétablit le raccourci par défaut et « × » le supprime. Ils sont enregistrés dans la section `[keys]` de `settings.toml` (`"none"` désactive une combinaison).

## Configuration

**Ctrl+,** (ou Fichier → Préférences) ouvre la page des préférences : sections Apparence, Éditeur, Lint, Git, Terminal, Raccourcis, Avancé, avec une recherche. Chaque changement s'applique immédiatement et est enregistré dans `~/.config/florm/settings.toml` (`%APPDATA%\florm` sous Windows), que l'on peut aussi éditer directement (Fichier → Ouvrir settings.toml).
La **langue** de l'interface (français ou anglais) se choisit dans Préférences → Apparence → Langue, ou avec `language = "fr"` / `"en"` (vide : celle du système) ; elle change tout de suite, menus, panneaux, messages et guide compris. En anglais, Claude reçoit aussi ses consignes en anglais.
Les changements s'appliquent à l'enregistrement. Le thème choisi via « Affichage → Thème… », la minimap, le diff dans l'éditeur et le zoom y sont enregistrés automatiquement.

```toml
language = "fr"
font_size = 14.0
tab_size = 4
translate_tabs_to_spaces = true
theme = "base16-ocean.dark"
show_minimap = true
lint_on_save = true

[keys]
"ctrl+shift+r" = "git_push"
"ctrl+k ctrl+t" = "toggle_terminal"
```

Les identifiants de commande sont listés dans `src/commands.rs`. Les thèmes `.tmTheme` (ex. Monokai)
déposés dans `~/.config/florm/themes/` sont chargés au démarrage.

### Configuration partagée entre plusieurs ordinateurs

Les préférences, modèles, thèmes et plugins peuvent venir d'un dépôt GitHub, le même sur chaque PC. Le dépôt a une forme fixe, chaque partie est facultative :

```text
florm-config/
├── settings.toml   # préférences appliquées par-dessus celles de l'ordinateur
├── templates/      # modèles (un fichier par modèle, comme le dossier local)
├── themes/         # thèmes .tmTheme
└── plugins/        # plugins
```

Dans Préférences → Avancé → **Configuration partagée** (ou `shared_config` dans settings.toml), indiquez `login` (le dépôt `login/florm-config`), `owner/repo` ou une adresse git. Florm le clone dans `~/.config/florm/shared/` (avec `gh` s'il est installé, donc aussi les dépôts privés) et le met à jour à chaque démarrage.

- **Créer sur GitHub** fait le dépôt (privé, avec `gh`) à partir de la configuration de cet ordinateur.
- `settings.toml` partagé ne change que les clés qu'il contient (`[keys]`, `[lsp_servers]`… sont fusionnés). Une préférence modifiée dans Florm et présente dans ce fichier y est écrite aussi ; les nouveaux modèles et thèmes sont créés dans le dépôt.
- `shared_config`, `shell`, `docker_order`, `odoo_test_container` et `odoo_test_history` restent propres à chaque ordinateur.
- **Publier** (ou Outils → Publier la configuration partagée) fait le commit et le push ; **Synchroniser** récupère les changements des autres PC.

## Publier une release

```bash
gh workflow run release.yml -f version=0.2.0    # ou Actions → Release → Run workflow
```

Compile sur GitHub Actions et publie dans la release `v0.2.0` (créée si besoin) : `.deb`, `.rpm`, `.pkg.tar.zst` (Arch), binaire Linux, `.exe` et `.msi` Windows, `.dmg` macOS (universel Intel + Apple Silicon). Sans `version` : celle de `Cargo.toml`. Un push sur `main` ne crée pas de release.

### Signer les versions

Les scripts `packaging/install.sh` et `packaging/install.ps1` évitent les avertissements sans signature. Pour que les fichiers téléchargés **dans le navigateur** s'ouvrent aussi sans avertissement, il faut les signer, et ce n'est pas gratuit partout :

- **macOS** : un compte [Apple Developer](https://developer.apple.com/programs/) (99 $ par an). Créez un certificat « Developer ID Application », exportez-le en `.p12`, et un [mot de passe d'application](https://account.apple.com) pour votre identifiant Apple. Ajoutez ces secrets au dépôt (Settings → Secrets and variables → Actions) : `APPLE_CERTIFICATE_P12` (le `.p12` en base64 : `base64 -i cert.p12 | pbcopy`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`. La release signe et notarise alors `Florm.app` et le `.dmg` toute seule ; sans ces secrets, elle garde la signature ad hoc.
- **Windows** : un certificat de signature de code. Florm est open source (MIT) : la [SignPath Foundation](https://signpath.org) en fournit un gratuitement aux projets open source qui le demandent, signé depuis GitHub Actions. Les autres options sont payantes (certificat d'une autorité, environ 200 à 400 € par an ; Azure Artifact Signing est réservé aux particuliers des États-Unis et du Canada). Même signé, SmartScreen peut encore avertir pour les premières versions, le temps que le certificat se fasse une réputation.

## Licence

MIT — voir [LICENSE](LICENSE).
