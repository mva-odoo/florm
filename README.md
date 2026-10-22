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

Téléchargez `florm-<version>-windows-x86_64.exe`. Il est **autonome** : pas d'installation, double-clic pour le lancer. Rangez-le où vous voulez (par exemple `C:\Users\<vous>\Florm\florm.exe`) et épinglez-le à la barre des tâches.

Au premier lancement, **Windows SmartScreen** peut afficher « Windows a protégé votre ordinateur » : le fichier n'est pas signé. Cliquez sur **Informations complémentaires** puis **Exécuter quand même**.

Les préférences sont dans `%APPDATA%\florm`.

### macOS

Téléchargez `Florm-<version>-macos-universal.dmg` (Mac Intel et Apple Silicon), ouvrez-le et glissez **Florm** dans **Applications**.

Florm n'est pas signé par un compte développeur Apple : au **premier lancement**, faites **clic droit sur Florm → Ouvrir**, puis **Ouvrir** dans la fenêtre d'avertissement. Ensuite, il se lance normalement.

Si macOS dit que l'application « est endommagée », retirez la quarantaine :

```bash
xattr -dr com.apple.quarantine /Applications/Florm.app
```

### Depuis les sources

Avec [Rust](https://rustup.rs) installé :

```bash
git clone https://github.com/mva-odoo/florm.git
cd florm
cargo build --release
./target/release/florm .
```

### Mettre à jour

Quand une nouvelle version sort, Florm l'indique au démarrage par **⬆ Florm x.y.z** en bas à droite. Un clic propose de **télécharger le fichier pour votre système** (le `.deb` sous Ubuntu, le paquet Arch sous Arch, le `.exe` sous Windows, le `.dmg` sous macOS…) ou d'ouvrir les notes de version. Pour vérifier à la main : **Aide → Rechercher une mise à jour**. Pour ne plus chercher : Préférences → Avancé → « Chercher les mises à jour ».

Inutile de désinstaller l'ancienne version : la nouvelle s'installe **par-dessus**, et les préférences sont gardées.

| Système | Mettre à jour |
|---|---|
| Debian / Ubuntu | `sudo apt install ./florm_<version>_amd64.deb` |
| Fedora | `sudo dnf install ./florm-<version>.x86_64.rpm` |
| openSUSE | `sudo zypper install ./florm-<version>.x86_64.rpm` |
| Arch | `sudo pacman -U florm-<version>-1-x86_64.pkg.tar.zst` |
| Binaire seul | remplacer `~/.local/bin/florm` par le nouveau fichier |
| Windows | fermer Florm, remplacer l'ancien `.exe` par le nouveau |
| macOS | fermer Florm, glisser le nouveau Florm dans Applications → **Remplacer** |

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
| Lint Python | `flake8` ou `ruff` (ou `pyflakes`, `pylint`) | Soulignements dans les fichiers Python |
| Formatage Python | `ruff` | Ctrl+Alt+F, « Corriger automatiquement » |
| Lint JavaScript | `eslint` (sinon `node`) | Soulignements dans les fichiers JS |
| Complétion intelligente, F2 renommer | un serveur de langage | Voir plus bas |
| Plugins d'exemple | `python3` | Les plugins écrits en Python |
| Icônes du terminal | une Nerd Font | Les symboles des prompts (starship, powerline) |

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
| Dossiers | Arborescence à gauche (clic droit : nouveau, renommer, supprimer, afficher dans le gestionnaire de fichiers…), **glisser-déposer pour déplacer** fichiers et dossiers, couleurs selon le statut git. **Au clavier** : Ctrl+Shift+E puis ↑ ↓ ← → Entrée (F2 renommer, Suppr). **Double-clic sur le bord** pour masquer la barre (clic sur la bande à gauche pour la réafficher). Sur le dossier d'un dépôt : sa branche, la branche distante suivie (↑ / ↓) et ses remotes au survol. En bas de la barre, une icône par vue (recherche, problèmes, terminal, sortie, Claude, Git, GitHub) : clic = dans le panneau du bas, clic droit = en grand dans l'éditeur. Clic droit sur un dossier → « Exclure de l'index » (marqué d'un œil barré : plus dans Ctrl+P, F12 ni la recherche) |
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
| Claude | Ctrl+Alt+C : conversation avec Claude Code dans le panneau du bas, ou dans un onglet avec ⇱ (commande `claude`, sans clé), et à côté **la revue de ce qu'il a changé, bloc par bloc : garder ou rejeter**. ✨ dans la vue Git : message de commit proposé d'après le diff et la demande. **File de tâches** : une liste de demandes (+ File) envoyées une à une, un commit après chacune si coché. **Moins de tokens** : questions à choix avant de coder, définitions citées jointes, un commit comme contexte, conversation neuve et modèle par tâche |
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
| Apparence | Thèmes avec aperçus, couleur d'accentuation, **texte adouci** (texte moins gras, surtout sur les thèmes clairs), barre de menus masquable (menus sous ☰), **thème à soi** (Préférences → Apparence : toutes les couleurs de l'éditeur et du code, appliquées en direct) |
| Divers | Palette de commandes (accents facultatifs), Ctrl+P fuzzy, retour à la ligne automatique (Alt+Z), Tab pour compléter les chemins (ouvrir, enregistrer sous), minimap, guides d'indentation, crochets appariés, auto-fermeture, rechercher/remplacer (regex), polices au choix parmi celles installées (Préférences → Apparence ; par défaut Consolas / Segoe UI sous Windows), thèmes `.tmTheme` (intégrés : « Florm » sombre, « Florm Light » et « Light+ (VS Code) » clairs) |

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
| Ctrl+Alt+N | Insérer un modèle (vue Odoo, menu…) |
| Ctrl+Alt+G | Graphe des commits |
| Ctrl+Alt+R | Pull requests GitHub |
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
Les changements s'appliquent à l'enregistrement. Le thème choisi via « Affichage → Thème… », la minimap, le diff dans l'éditeur et le zoom y sont enregistrés automatiquement.

```toml
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

## Publier une release

```bash
gh workflow run release.yml -f version=0.2.0    # ou Actions → Release → Run workflow
```

Compile sur GitHub Actions et publie dans la release `v0.2.0` (créée si besoin) : `.deb`, `.rpm`, `.pkg.tar.zst` (Arch), binaire Linux, `.exe` Windows et `.dmg` macOS (universel Intel + Apple Silicon). Sans `version` : celle de `Cargo.toml`. Un push sur `main` ne crée pas de release.

## Licence

MIT — voir [LICENSE](LICENSE).
