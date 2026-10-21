# Florm

Éditeur de texte léger et rapide en Rust, inspiré de Sublime Text.

- **Rendu GPU** (egui/glow) : seules les lignes visibles sont dessinées, quelle que soit la taille du fichier.
- **Buffer en rope** (`ropey`) : les insertions et suppressions restent rapides sur des fichiers de plusieurs Mo.
- **Coloration incrémentale** avec les grammaires de Sublime Text (`syntect`).
- Un seul binaire (~17 Mo sous Linux, sans dépendance système à part OpenGL).

## Compiler et lancer

```bash
cargo build --release
./target/release/florm              # rouvre le dernier projet utilisé
./target/release/florm .            # ouvre le dossier courant
./target/release/florm fichier.py   # ouvre un fichier
```

Fonctionne sous Linux (X11/Wayland, WSLg), macOS et Windows.

### Version Windows depuis WSL

```bash
sudo apt install mingw-w64          # une seule fois (ou sans sudo : pip install --user ziglang && cargo install cargo-zigbuild)
scripts/build-windows.sh            # compile florm.exe et le copie dans C:\Users\<toi>\Florm
```

Le `.exe` est autonome (pas d'installation) : double-clic pour le lancer. Git, les linters et les serveurs de langage utilisés sont ceux installés côté Windows.

## Fonctionnalités

| | |
|---|---|
| Dossiers | Arborescence à gauche (clic droit : nouveau, renommer, supprimer, afficher dans le gestionnaire de fichiers…), **glisser-déposer pour déplacer** fichiers et dossiers, couleurs selon le statut git. **Au clavier** : Ctrl+Shift+E puis ↑ ↓ ← → Entrée (F2 renommer, Suppr). **Double-clic sur le bord** pour masquer la barre (clic sur la bande à gauche pour la réafficher). Sur le dossier d'un dépôt : sa branche, la branche distante suivie (↑ / ↓) et ses remotes au survol |
| Recherche globale | Ctrl+Shift+F : tout le dossier (respecte `.gitignore`), regex / casse / mot entier, filtres `*.py`, `models/*.py` (dossiers « models » où qu'ils soient, sous-dossiers compris), `./chemin` ou chemin absolu, et exclusions, remplacement avec **aperçu** (ancien barré en rouge → nouveau en vert), remplacer tout / un fichier / une seule occurrence, × pour écarter un résultat, fichiers repliables, F4 / Maj+F4 pour parcourir les résultats. Historique des recherches et des filtres (bouton ⏷, ou ↑ / ↓ dans le champ), filtres mémorisés entre les redémarrages. Résultats **colorés comme dans l'éditeur**, recherche dans le panneau du bas ou **dans un onglet** (⇱ / ⇲) |
| Projets | Chaque dossier ouvert devient un projet (onglet « Projets », Ctrl+Alt+P pour changer). La session est restaurée : fichiers ouverts, position des curseurs, dossiers dépliés, filtres de recherche. Sans argument, Florm rouvre le dernier projet |
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
| Linters | XML et JSON en direct (intégrés). **Python pendant la frappe** avec flake8 (ou ruff, pyflakes, pylint : `python_linter` dans les préférences), lancé depuis la racine du dépôt pour lire `setup.cfg` / `.flake8` / `tox.ini` / `pyproject.toml`. Survol d'un soulignement = message, clic droit = « Ignorer (# noqa) ». Sans linter installé : bouton « Installer flake8 » dans Problèmes (installé dans `~/.local/share/florm/pytools`, sans toucher au Python système). JS via `eslint` → `node --check` |
| Terminal | Ctrl+` (ou Ctrl+Alt+T) : vrai terminal comme dans VS Code (shell interactif dans un pseudo-terminal : couleurs, complétion Tab, ↑ historique, `vim`, `htop`, `less`…). Plusieurs terminaux en onglets (＋), sélection à la souris, Ctrl+C interrompt (ou copie la sélection), Ctrl+V colle, molette pour l'historique. **Terminal dans un onglet de l'éditeur** (Ctrl+Shift+`, ou ⇱ Onglet) : il se place à côté d'un fichier comme tout onglet. Bouton « >_ Terminal » dans la barre d'état. Shell : `$SHELL` (ou `cmd` sous Windows), modifiable dans les préférences |
| Plugins | Commandes et hooks dans n'importe quel langage, voir [plugins/README.md](plugins/README.md) |
| Modèles | Ctrl+Alt+N : vues Odoo (formulaire, liste, recherche, héritage), action, menu, modèle Python, champ calculé, `__manifest__.py`, droits d'accès… insérés au curseur avec `${model}`, `${Model}`, `${module}` remplis d'après le fichier courant. Les siens dans Outils → Dossier des modèles |
| Aide | Aide → Guide de Florm : exemples de filtres de recherche, Git, terminal, modèles |
| Complétion, définition, renommage | Comme Sublime, sans rien installer : le projet est indexé en arrière-plan (classes, fonctions, méthodes, modèles Odoo `_name`, champs, records XML) ; F12 va à la définition (plusieurs résultats : liste des fichiers ; marche aussi sur `'sale.order'` ou `'module.xml_id'`) et les suggestions proposent les mots des fichiers ouverts et les symboles du projet. Avec un serveur de langage (pyright ou pylsp — bouton « installer pylsp » dans la barre d'état —, typescript-language-server, rust-analyzer, gopls, clangd) : complétion intelligente, définition exacte, F2 renommer dans tout le projet. Commande par langage dans `[lsp_servers]` |
| Symboles et fil d'Ariane | Ctrl+R : classes, fonctions, méthodes, records / templates Odoo, titres Markdown. Au-dessus de l'éditeur : `dossier › fichier › Classe › méthode`, cliquable |
| Repli de code | Flèches dans la marge, Ctrl+Shift+[ / Ctrl+Shift+], Ctrl+K Ctrl+0 tout replier, Ctrl+K Ctrl+J tout déplier ; « ⋯ N lignes » cliquable sur un bloc replié |
| Conflits de merge | Ta version en vert, la version entrante en bleu ; barre « Garder le mien / le leur / les deux » (annulable), puis « marquer comme résolu » ; bandeau merge / cherry-pick / rebase en cours (Continuer / Abandonner) dans la vue Git |
| Rechercher dans la sélection | Bouton « Sél. » de Ctrl+F / Ctrl+H ; « Garder la casse » au remplacement (chat → chien, Chat → Chien, CHAT → CHIEN), aussi dans la recherche globale |
| Aperçu Markdown, images | Ctrl+Shift+V (onglet) ou Ctrl+K V (à côté, suit la frappe) ; png, jpg, gif, webp, bmp, ico, svg s'ouvrent dans un onglet image |
| Travail non enregistré gardé | À la fermeture ou en changeant de projet, les modifications non enregistrées (et les onglets sans titre) sont gardées et rouvertes, sans question (désactivable) |
| Git avancé | Cherry-pick, revert, rebase interactif (réordonner, changer un message, fusionner, supprimer), comparer deux commits ou un commit avec une branche |
| Apparence | Thèmes avec aperçus, couleur d'accentuation, **texte adouci** (texte moins gras, surtout sur les thèmes clairs), barre de menus masquable (menus sous ☰) |
| Divers | Palette de commandes (accents facultatifs), Ctrl+P fuzzy, Tab pour compléter les chemins (ouvrir, enregistrer sous), minimap, guides d'indentation, crochets appariés, auto-fermeture, rechercher/remplacer (regex), polices au choix parmi celles installées (Préférences → Apparence ; par défaut Consolas / Segoe UI sous Windows), thèmes `.tmTheme` (intégrés : « Florm » sombre, « Florm Light » et « Light+ (VS Code) » clairs) |

## Raccourcis (comme Sublime)

| Raccourci | Action |
|---|---|
| Ctrl+Shift+P / F1 | Palette de commandes |
| Ctrl+P | Aller au fichier (`nom:42` pour aller à la ligne, `:42` dans le fichier courant) |
| Ctrl+G | Aller à la ligne |
| Alt+G | Aller à la ligne et la sélectionner : `42`, `10-20` pour plusieurs lignes, vide + Entrée = la ligne courante |
| Ctrl+R | Aller au symbole du fichier |
| Alt+L | Afficher le fichier courant dans l'arborescence (aussi : clic droit sur l'onglet) |
| Alt+- / Alt++ | Position précédente / suivante du curseur (aussi entre fichiers ; boutons précédent / suivant de la souris) |
| Ctrl+K Ctrl+Z | Annuler le changement git sous le curseur (Ctrl+Z pour le récupérer) |
| Ctrl+K Ctrl+Shift+Z | Remettre tout le fichier comme au dernier commit |
| Ctrl+Espace / F12 / F2 | Suggestions / aller à la définition / renommer le symbole |
| Ctrl+Shift+[ / Ctrl+Shift+] | Replier / déplier le bloc |
| Ctrl+\\ | Diviser l'éditeur à droite |
| Ctrl+Shift+V / Ctrl+K V | Aperçu Markdown / à côté |
| Ctrl+F / Ctrl+H | Rechercher / Remplacer (Entrée, Maj+Entrée, Alt+Entrée = tout sélectionner) |
| F3 / Maj+F3 | Résultat suivant / précédent |
| Ctrl+Shift+F | Rechercher dans le dossier |
| F4 / Maj+F4 | Résultat global suivant / précédent |
| Ctrl+, | Préférences |
| Ctrl+Alt+P | Changer de projet |
| Ctrl+Shift+E | Le clavier dans l'arborescence (↑ ↓ ← → Entrée) |
| Ctrl+Shift+` | Nouveau terminal dans un onglet |
| Ctrl+Alt+N | Insérer un modèle (vue Odoo, menu…) |
| Ctrl+Alt+G | Graphe des commits |
| Ctrl+Alt+D | Diff git dans l'éditeur (vert / rouge) |
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

## Outils recommandés

- flake8 : bouton « Installer flake8 » du panneau Problèmes (ou palette → « Lint : installer flake8 pour Florm »), ou ruff / flake8 déjà présents dans le PATH
- `eslint` dans le projet (`node_modules/.bin`) ou dans le PATH pour le JS

## Limites connues

- Pas de retour à la ligne automatique (soft wrap).
- Les caractères larges (CJK) peuvent être mal alignés dans l'éditeur.
- Pas de sélecteur de fichiers natif : ouverture via un chemin (pré-rempli) ou l'arborescence.

## Licence

MIT — voir [LICENSE](LICENSE).
