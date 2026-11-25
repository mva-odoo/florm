# Guide de Florm

Ce guide s'ouvre avec **Aide → Guide de Florm** (ou la palette : « guide »).
Tous les raccourcis se changent dans **Préférences → Raccourcis** (Ctrl+,).

---

## Trouver et ouvrir

| Raccourci | Action |
|---|---|
| **Ctrl+P** | Aller à un fichier du projet. `nom:42` ouvre à la ligne 42, `:42` va à la ligne 42 du fichier courant |
| **Ctrl+Shift+P** | Palette de commandes : d'abord les vues (Claude, Git, Pull requests, Recherche, Terminal…, les terminaux et les conversations), puis les commandes. Tape pour filtrer (les accents sont facultatifs : « theme » trouve « Choisir le thème… ») ; une vue dont le nom contient les mots tapés reste en tête (« pull request » → la vue des pull requests, puis les commandes). **`#`** en tête ne liste que les vues : `#git`, `#term`, `#claude`. |
| **Alt** seul | Tout au clavier : une ou deux lettres sur chaque bouton, onglet, case ou champ visible ; les taper le clique (Échap ferme). Dans un menu ouvert, ↓ ↑ passent d'une entrée à l'autre et Entrée choisit. Se désactive dans Préférences → Éditeur |
| **Ctrl+R** | Aller à une classe, une fonction du fichier |
| **Ctrl+O** / **Ctrl+Shift+S** | Ouvrir / enregistrer sous : **Tab complète le chemin** comme dans un terminal |
| **F12** | Aller à la définition |
| **Ctrl+.** | Corrections rapides du problème sous le curseur (aussi l'ampoule en bout de ligne) : importer un nom du projet, ignorer (`# noqa`), `ruff --fix`, corriger avec Claude, et les corrections du serveur de langage dans tous les langages (importer un nom en Go, Rust, TypeScript…), en tête de liste ; le curseur peut être n'importe où sur la ligne |
| **Alt+Z** | Retour à la ligne automatique des lignes trop longues (l'affichage seulement ; ↑ ↓ suivent les rangées) |
| **Maj+F12** | Les usages du symbole (serveur de langage, sinon le mot entier dans le dossier) |
| **Alt+-** / **Alt++** | Revenir à la position précédente du curseur / aller à la suivante (aussi d'un fichier à l'autre) |
| **Ctrl+Alt+P** | Changer de projet (chaque dossier ouvert en est un, retrouvé comme tu l'as laissé) |
| **Ctrl+\** | Diviser l'éditeur : le fichier courant à droite ; **Ctrl+1** passe d'un côté à l'autre |

Le fil d'Ariane au-dessus du code est cliquable : un dossier montre son contenu, le fichier
liste ses symboles, une classe ou une fonction y emmène. L'icône à sa droite copie le chemin du
fichier (clic droit : la référence fichier:ligne ou le lien GitHub de la ligne). Les vues (Claude, Git, Pull requests (GitHub),
terminal…) n'en ont pas : elles gardent toute la hauteur.

Deux fichiers ouverts du même nom : leur dossier s'affiche en gris après le titre (`input.rs …/app`). Un fichier avec des erreurs montre leur nombre en rouge (`9+` au-delà de neuf).

À droite de la barre d'onglets : **+** ouvre un onglet texte vide (Ctrl+N), **▾** une vue dans un onglet, chacune avec son icône : Recherche, Claude, Git, Pull requests (GitHub), puis Terminal, Préférences et Accueil. Un titre trop long (une conversation, une pull request) est coupé par « … », entier au survol. Quand les onglets ne tiennent plus dans la barre, ‹ › les font défiler et le même **▾** liste en haut tous les onglets ouverts.

Au redémarrage (ou en revenant sur un projet), tout revient comme tu l'as laissé : les onglets et
les vues, la position du curseur, l'éditeur divisé avec ses onglets de chaque côté, le panneau du
bas ouvert et la barre latérale affichée ou non.

## Éditer

- **Plusieurs curseurs** : **Ctrl+D** sélectionne le mot puis ajoute l'occurrence suivante,
  **Alt+F3** les prend toutes, **Ctrl+Maj+L** met un curseur sur chaque ligne sélectionnée,
  **Ctrl+Alt+↑ / ↓** en ajoute un au-dessus / en dessous, **Ctrl+clic** où tu veux.
  **Alt+glisser** (ou glisser avec le clic du milieu) sélectionne en colonne. Ce qui est collé
  est réparti sur les curseurs ; Échap revient à un seul.
- **Signets** : **Ctrl+F2** pose ou retire un signet sur la ligne, **F8 / Maj+F8** vont au
  suivant / précédent, « Signet : liste » (palette) les montre dans tous les fichiers ouverts.
- **Replier** : **Ctrl+Maj+[** replie le bloc du curseur, **Ctrl+Maj+]** le déplie ; **Ctrl+K
  Ctrl+0** replie tout (classes, fonctions…), **Ctrl+K Ctrl+J** déplie tout. Aussi les flèches
  dans la marge.
- **Balises XML et HTML** : taper `>` à la fin d'une balise ouvrante (`<field name="x">`) ajoute
  sa fermeture (`</field>`) après le curseur, et `</` complète celle de l'élément encore ouvert.
  Rien pour `<br/>`, `<?xml ?>`, `<!-- -->` ni les balises HTML sans fermeture (`<br>`, `<img>`…).
  Suit la préférence « Fermeture automatique » (Préférences → Éditeur).

## Barre latérale

- En haut : le nom du projet (un clic pour changer de projet) et quatre petits boutons, nouveau
  fichier, nouveau dossier, **tout replier** et ⟳ rafraîchir.
- **Ctrl+Shift+E** (ou un clic sur un fichier) : le clavier va dans l'arborescence. **↑ ↓** pour se
  déplacer, **→** ouvre un dossier, **←** le referme (ou remonte au dossier parent), **Espace**
  affiche le fichier en gardant le clavier dans l'arborescence, **Entrée** l'ouvre et passe à
  l'éditeur, **F2** renomme, **Suppr** supprime, **Échap** revient à l'éditeur.
- En défilant, les dossiers qui contiennent le haut de la liste restent figés en haut ; un clic
  remonte au dossier.
- **Double-clic sur le bord** entre la barre et l'éditeur : la largeur s'ajuste au nom le plus long
  de l'arborescence. Le bord se glisse pour changer la largeur ; **Ctrl+B** (ou Ctrl+K Ctrl+B) masque la barre,
  et un clic sur la fine bande à gauche la réaffiche.
- Sur le dossier d'un dépôt git rangé dans le dossier ouvert (celui du dossier ouvert est dans la barre d'état) : sa branche, la branche distante suivie (↑ à pousser, ↓ à
  récupérer) et le nombre de fichiers modifiés ; le survol montre les remotes
  (désactivable dans Préférences → Git).
- La barre latérale ne montre que l'arborescence : les vues (Recherche, Terminal, Claude, Git…) se
  trouvent dans le panneau du bas et la barre d'état (voir plus bas).
- **Copier / coller des fichiers** : clic droit → **Copier** met le fichier ou le dossier sur le presse-papiers du système ; **Coller** (ou Ctrl+C / Ctrl+V dans l'arborescence) y dépose les fichiers copiés depuis le gestionnaire de fichiers de ton PC, ou depuis l'arborescence. Un nom déjà pris devient « nom (copie).ext » : rien n'est remplacé.
- Clic droit sur un dossier hors de tout dépôt → **Initialiser un dépôt git ici…**, pour un nouveau
  projet : une fenêtre demande le **remote** à ajouter (« origin », vide : aucun), la branche
  (`main`), puis **tout ajouter** (`git add .`) et faire le premier commit, ou un **commit vide**
  sans aucun fichier, ou rien. Avec un remote, le commit est poussé (`git push -u`). Si une étape
  échoue, la fenêtre reste ouverte avec l'erreur (les étapes d'avant restent faites).

## Panneau du bas et barre d'état

Une règle simple : **les fichiers et les vues qui demandent de la place (Claude, Git, Pull requests
(GitHub), la recherche) vont dans les onglets ; le panneau du bas garde ce qui accompagne le
travail** : terminaux, débogueur, problèmes, sortie. Pour voir deux onglets côte à côte, divise
l'éditeur.

- **Le panneau du bas a ses onglets** : un par terminal, puis **Débogueur** (● pendant une session,
  ⏸ en pause), **Problèmes** (leur nombre) et **Sortie**. Un clic passe de l'un à l'autre, **+**
  ouvre un nouveau terminal, **×** au survol d'un terminal le ferme. **Ctrl+Shift+J** affiche ou
  masque le panneau.
- **La barre d'état ne montre que ce qui se passe** : à gauche l'icône des **Préférences**, puis celles de **Claude**, **Git**,
  **Pull requests** (un clic ouvre son onglet, un autre revient à l'onglet d'avant ; en couleur quand
  il est affiché), **Terminal** (le terminal du panneau du bas) et **Débogueur** (sa vue dans le
  panneau du bas ; verte pendant une session, orange quand le programme est en pause) ; à droite la branche (**↑** commits à
  pousser, **↓** à récupérer, **●** fichiers modifiés) du dépôt du fichier affiché (il suit les
  onglets ; un dépôt choisi dans la vue Git reste jusqu'au prochain fichier affiché), puis, seulement quand c'est le cas, Claude
  (**au travail**, ou **✔** quand il a fini : un clic montre la réponse), et les problèmes. Au repos, elle est presque vide.
- Les Préférences : **Ctrl+,**, le menu ou la palette.

## Rechercher dans le dossier (Ctrl+Shift+F)

Entrée lance la recherche. Les résultats sont colorés comme dans l'éditeur ; **F4 / Maj+F4**
passent au résultat suivant / précédent. La recherche est une page à part, dans un onglet de
l'éditeur : toute la place pour les résultats.

### Filtrer les fichiers

Plusieurs règles se séparent par des virgules ; un `!` devant une règle l'exclut. Le nom du
projet ouvert peut se mettre devant : dans le projet `florm`, `florm/test` est son dossier `test`.

| Filtre « Fichiers » | Cherche dans… |
|---|---|
| `*.py` | tous les fichiers Python |
| `*.py, *.xml` | les fichiers Python et XML |
| `models/*.py` | les `.py` de tout dossier `models` (où qu'il soit) et de ses sous-dossiers |
| `tests` | tout dossier (ou fichier) nommé `tests` |
| `./src/*.rs` | à partir de la racine du projet seulement |
| `views/**/*.xml` | les `.xml` sous n'importe quel dossier `views` |
| `*.py, !tests` | les `.py`, sauf ceux des dossiers `tests` |
| `florm/*.py, !florm/test` | les `.py` du projet `florm`, sauf son dossier `test` |

| Filtre « Exclure » | Ignore… |
|---|---|
| `node_modules, *.min.js` | ces dossiers et ces fichiers |
| `tests, i18n` | les dossiers de tests et de traductions |
| `addons/sale/tests` | ce dossier seulement |
| `./test` (ou `florm/test` dans le projet `florm`) | le dossier `test` à la racine |

Les deux champs se combinent : **Fichiers** `*.py` + **Exclure** `test` cherche dans les `.py`
du projet, sauf le dossier `test`.

Le `.gitignore` est respecté.

### Texte, regex, remplacement

- **Aa** respecte la casse, **« »** cherche le mot entier, **.*** active les expressions régulières :
  `def _compute_\w+` trouve toutes les méthodes de calcul, `^\s*import ` les imports.
- Avec **Remplacer** (à droite de **Rechercher**), le remplacement s'affiche ; en regex `$1`, `$2` reprennent les groupes :
  `fields\.(\w+)\(` → `Fields.$1(`.
- **Retour à la ligne** dans le remplacement : **Maj+Entrée** dans le champ, ou `\n` en regex
  (`\t` tabulation, `\\` barre oblique inverse). Pour changer les `\n` écrits dans le texte en
  vrais retours à la ligne : regex, chercher `\\n`, remplacer par `\n`. Aussi dans Ctrl+H.
- **Garder la casse** : chat → chien, Chat → Chien, CHAT → CHIEN.
- **×** sur un résultat (ou un fichier) l'écarte : il ne sera pas remplacé.

### Comparer, historique local

- **Comparer deux fichiers ou deux dossiers** : clic droit sur le premier dans la barre latérale
  → **⇄ Choisir … pour comparer**, puis clic droit sur l'autre → **⇄ Comparer avec « … »**.
- **Comparer avec le presse-papiers** et **Comparer avec…** (un autre fichier, ou un dossier) :
  clic droit sur l'onglet, ou le menu Outils. Le texte de l'éditeur compte, même non enregistré.
- Le résultat s'ouvre dans un onglet : en rouge ce qui est à gauche, en vert ce qui est à droite.
- **Historique local** (clic droit sur l'onglet, ou Outils) : Florm garde une copie du fichier à
  chaque enregistrement, hors de git (les 50 dernières). **Comparer** une version au texte
  actuel, ou la **Restaurer** dans l'éditeur (Ctrl+Z pour revenir).

## Git

Florm passe par la commande `git` (votre configuration, vos identifiants). Sans `git`, la vue
explique comment l'installer (le système est détecté) et reprend toute seule dès qu'elle est trouvée.
Un dossier reçu d'ailleurs (un zip) peut avoir dans son `.git/config` des commandes : Florm lance
git sans `core.fsmonitor` ni les programmes de diff du dépôt (`textconv`, diff externe), ouvrir le
dossier ou afficher un diff ne les exécute pas. Les hooks des commits et des push tournent comme
d'habitude.

- **Les changements dans l'éditeur** : une marque colorée dans la marge montre chaque bloc changé
  depuis le dernier commit. **Un clic sur la marque** (ou **Ctrl+K Ctrl+D** sur la ligne) affiche
  ce changement seul, dans le texte, comme dans Zed : les lignes supprimées sur fond rouge au-dessus,
  les nouvelles sur fond vert (marge comprise), les mots qui ont changé sur un fond plus fort, et une
  petite barre (précédent, suivant, ↺ annuler, Tous, ×). Un nouveau clic, Échap ou
  Ctrl+K Ctrl+D le masque. **Alt+F5 / Alt+Maj+F5** passent au changement suivant / précédent en
  l'affichant ; **Ctrl+K Ctrl+Z** annule celui de la ligne. **Ctrl+Alt+D** (ou la préférence
  « Diff dans l'éditeur ») les affiche tous en permanence.
- **Ctrl+Shift+G** (ou clic sur la branche dans la barre d'état) : vue Git. En haut le dépôt (en grand : le
  nom est cliquable quand le projet en a plusieurs), la branche (**🔀 nom ▾** : changer de branche ou en créer une),
  pull / push / fetch et le message du commit (Ctrl+Entrée) ; en dessous le graphe, dont les
  premières lignes sont les modifications à indexer / indexées, avec leurs fichiers un par ligne
  (comme dans Atom) : la lettre de l'état (**M** modifié, **A** ajouté, **D** supprimé, **U**
  nouveau), le nom, le dossier en gris et les lignes +/− ; au survol, **+** indexe le fichier,
  **−** le retire de l'index. Sous le message, la longueur de sa première ligne (orange après 50,
  rouge après 72).
  Un clic sur un fichier ouvre ses changements dans la vue des commits.
- **Ctrl+Alt+G** : la vue des commits. En haut la liste (avec les modifications non indexées
  et indexées), en dessous **tous les fichiers du commit et leurs changements, l'un sous l'autre**,
  comme une pull request GitHub.
  - **↑ ↓** : commit précédent / suivant · **← →** : fichier précédent / suivant ·
    **Page ↑ / Page ↓** : faire défiler les changements
  - clic sur l'en-tête d'un fichier : le replier ; les puces au-dessus emmènent à un fichier
  - sur les modifications : **+ Indexer ce bloc**, **↺ Annuler ce bloc**, et par fichier
    **+ Indexer**, **− Désindexer**, **↺ Annuler**
  - double-clic sur une ligne : ouvrir le fichier à cette ligne
- Les changements faits ailleurs (commit dans un terminal…) sont repris tout seuls.
- **⋯ → Stash** dans la barre : mettre les changements de côté (avec un message, nouveaux
  fichiers compris ou non), puis pour chaque stash **Voir**, **Appliquer** (il reste),
  **Reprendre** (appliqué puis supprimé) ou **Supprimer**. Aussi dans la palette :
  « Git : mettre les changements de côté », « Git : reprendre le dernier stash ».
- **🔀 branche ▾** : aussi **fusionner une branche** dans la branche courante, **rebaser** la branche
  courante sur une autre, **renommer** une branche (sa branche distante du même nom suit),
  **supprimer** une branche locale (et sa branche distante si tu le choisis) ou distante, et
  **modifier le dernier commit**.
- **Rebase interactif** : **🔀 branche ▾ → Rebase interactif…** (ou « Git : rebase interactif »
  dans la palette) demande la branche de départ (`master`, `origin/17.0`…) et liste les commits de
  la branche courante qui n'y sont pas ; ou clic droit sur un commit du graphe → **Rebase
  interactif à partir d'ici…**. Pour chaque commit : le garder, changer son message, le fusionner
  avec le précédent (avec ou sans son message) ou le supprimer, et ↑ ↓ pour changer l'ordre.
- **Blame** (**Ctrl+Maj+B**) : l'auteur, la date et le message du dernier commit de la ligne, en
  gris au bout de celle-ci ; à nouveau pour le masquer.
- **Blame de chaque ligne** (**Ctrl+K B**, ou clic droit → Blame de chaque ligne), comme dans Zed :
  une colonne de la marge donne l'auteur et l'ancienneté de chaque bloc de lignes du même commit ;
  au survol, le commit et son message, un clic l'ouvre dans la vue Git. Les lignes changées depuis
  le dernier commit disent « pas encore commité ». Un seul `git blame` par fichier et par commit
  (quelques secondes sur un très long historique, en arrière-plan) : la frappe ne le relance pas.
- **Modifier le dernier commit** (case sous le message, ou menu de la branche) : le message du
  dernier commit revient dans la zone ; le bouton le remplace (avec les fichiers indexés en plus).
  S'il était déjà poussé, il faudra un push --force-with-lease.
- **↕ Synchroniser** : Pull, **Pull --rebase** (tes commits rejoués par-dessus, sans commit de
  merge), Push, **Push --force-with-lease** (après un rebase ou un amend : refusé si quelqu'un a
  poussé entre-temps), **Pousser les tags**, ⟳ Fetch, et **⤵ Rebaser sur… (après fetch)** : récupère les commits du serveur, puis propose la
  branche sur laquelle rejouer les tiens (celles du serveur d'abord, master ou main en tête). Quand un push est refusé, le message d'erreur
  propose aussi **⬆ Forcer (--force-with-lease)**.
- Clic droit sur un commit du graphe : en plus du checkout, du cherry-pick, du revert…
  - **Créer un tag ici**  (« v1.2 », ou « v1.2 message » pour un tag annoté) ; sur un tag :
    **Pousser le tag**, **Supprimer le tag** (en local, ou aussi sur le remote)
  - sur chaque branche du commit : **Fusionner** dans la branche courante, **Renommer**,
    **Supprimer** ; **Fusionner ce commit**
  - **Ramener la branche ici (reset)** : soft (changements gardés, indexés), mixed (gardés, non
    indexés) ou hard (tout jeté, demande confirmation)
  - **Bisect** : marque un commit mauvais et un bon ; un bandeau propose ensuite **✔ Bon**,
    **× Mauvais**, **Passer** sur chaque commit que git sort, jusqu'au premier mauvais, puis **Terminer**.
- **⋯** dans la barre :
  - **Rechercher dans l'historique** : par message, auteur, texte ajouté ou supprimé (`-S`),
    regex dans les lignes changées (`-G`) ou fichier (avec ses renommages). Tab dans la question
    change de critère. Les commits trouvés s'affichent avec leur diff.
  - **Reflog** : où HEAD est passée (commits, checkouts, resets, rebases). Un commit perdu y est
    encore : **Récupérer ▾** crée une branche dessus, le cherry-pick ou y ramène la branche.
  - **Remotes** : ajouter (« nom URL ») ou retirer un remote. **Sous-modules** : les initialiser
    et mettre à jour, suivre leur branche distante, reprendre les URL, en ajouter un.
  - **Supprimer les fichiers non suivis** (`git clean`) : la liste est montrée avant.
  - **✨ Consignes des messages de commit** : ouvre `.florm/commit.md`, ce que Claude doit suivre
    pour ✨ dans ce projet (langue, préfixes, longueur…). À commiter pour le partager.
    Sans ce fichier, Claude prend la section « Commits » du CLAUDE.md du projet s'il y en a une.
  - **✨ Mes consignes des messages de commit** : les tiennes, pour tous les dépôts qui n'ont pas
    les leurs (`commit.md` du dossier de configuration) : rien à ajouter dans un dépôt partagé.
    Par exemple : en anglais, `[FIX] module: …`, le pourquoi dans le corps, `task-…` à la fin.
  - Les deux s'ouvrent aussi d'un **clic droit sur ✨**, à côté du message du commit.
- **Historique du fichier** (clic droit dans l'éditeur, menu Git) : tous ses commits, à travers
  ses renommages. **Historique de la ligne** (Ctrl+Alt+B) prend aussi plusieurs lignes sélectionnées ;
  chaque commit montre tout le bloc qui a changé autour de ces lignes, avec un peu de contexte, et le dernier est la
  création du fichier (✚).
  L'historique s'ouvre dans un onglet (↑ / ↓ pour passer d'un commit à l'autre, Échap pour revenir).
- **Diffs côte à côte** : le bouton **◫ Côte à côte**, en bas à droite de chaque diff (Git,
  historique, revue de Claude, pull requests), met l'ancienne version à gauche et la nouvelle à
  droite, chaque ligne remplacée en face de la nouvelle, avec les mots changés plus foncés.
  **≡ Une colonne** revient à l'affichage habituel. Le choix vaut pour tous les diffs et est retenu.

### Résoudre des conflits (rebase, merge, cherry-pick)

Comme dans VS Code, les conflits se résolvent **dans le fichier**. Quand un rebase, une fusion, un
cherry-pick ou un revert s'arrête sur des conflits, le premier fichier en conflit s'ouvre sur son
premier bloc (aussi avec **⇄ Aller au premier conflit** dans le bandeau de la vue Git).

- Chaque bloc a un côté **vert** et un côté **bleu**. Après sa ligne `<<<<<<<`, des liens :
  **Garder le vert · Garder le bleu · Garder les deux · Comparer** (les deux côtés en diff, dans
  un onglet). Ctrl+Z revient en arrière.
- **La barre au-dessus du fichier** dit ce qui se passe (« Rebase de feature sur master — commit 3
  sur 7 ») et **le vrai nom de chaque couleur** : dans un rebase, le vert est la base sur laquelle
  tes commits sont rejoués, le bleu ton commit. ◀ ▶ passent d'un bloc à l'autre ; les mêmes choix
  s'y trouvent pour le bloc sous le curseur, avec **✳ Résoudre avec Claude** (Claude reçoit le
  fichier, les deux côtés et ce que le commit y change, et retire les marqueurs sans toucher à git)
  et **Abandonner**.
- Quand le fichier n'a plus de marqueur : **✔ Marquer résolu, fichier suivant ›** (enregistre, git
  add, ouvre le fichier en conflit suivant), ou pour le dernier **✔ Marquer résolu et continuer le
  rebase ▶**. Si le commit suivant a des conflits, son premier fichier s'ouvre à son tour.
- Un fichier supprimé d'un côté et changé de l'autre se **garde** ou se **supprime** dans le
  bandeau de la vue Git, qui a aussi **Continuer**, **Passer ce commit** et **Abandonner**.

Les opérations lancées depuis Florm mettent la version d'origine entre les deux côtés
(`merge.conflictStyle=diff3`).

## Terminal

- **Ctrl+`** : le terminal dans le panneau du bas.
- **Les raccourcis dans un terminal** (et dans Claude) : Ctrl+lettre (Ctrl+C, Ctrl+R, Ctrl+D…),
  Alt+lettre et Ctrl+Maj+C / V / F vont au shell ; les touches F (F2, F12…), les Ctrl+Alt+…, les
  autres Ctrl+Maj+… (Ctrl+Maj+G, Ctrl+Maj+J…) et Ctrl+Tab, Ctrl+PgUp / PgDn restent à Florm.
- **Maj+Entrée** (ou Alt / Option+Entrée) va à la ligne sans envoyer, dans Claude comme dans zsh
  et fish.
- **Liens** : **Ctrl+clic** (⌘+clic sur Mac) sur une adresse web l'ouvre dans le navigateur ; sur un
  chemin de fichier affiché par un programme (`src/app.rs:12`, `~/notes.txt`, un chemin complet,
  le `b/…` d'un diff), il l'ouvre dans Florm, à la ligne. Avec Ctrl enfoncé, le lien sous la souris
  est souligné. Valable dans Claude comme dans le shell.
- **Défiler** : la molette, l'ascenseur à droite, **Maj+↑ / Maj+↓** (une ligne) et **Maj+PgUp /
  Maj+PgDn** (une page) parcourent l'historique ; ↑ et ↓ restent au programme (l'historique des
  commandes, les choix des menus de Claude). Claude y écrit toute sa conversation, qu'on remonte
  ainsi. Un programme qui gère lui-même la souris (htop…) reçoit la molette.
- **Double clic** sélectionne un mot entier (un chemin, une adresse), **triple clic** la ligne.
- **Glisser un fichier** sur un terminal écrit son chemin à l'invite (entre guillemets s'il le
  faut) : Claude lit le fichier, une image comprise.
- Les liens qu'un programme place lui-même sur son texte (« PR #12 » qui mène à la page de la
  PR…) s'ouvrent de la même façon. Seules les adresses `http` et `https` vont au navigateur ; une
  adresse `file://` ouvre le fichier dans Florm, les autres (`javascript:`, `mailto:`…) ne sont pas
  ouvertes : la barre d'état le dit.
- **Ctrl+Maj+F** (⌘F sur Mac, ou clic droit → Rechercher…) cherche dans tout l'historique du
  terminal : **Entrée** remonte au résultat plus ancien, **Maj+Entrée** redescend, **Échap**
  ferme. Les majuscules ne comptent que si tu en tapes une.
- Le gras, l'italique, le souligné et le texte grisé des programmes s'affichent, ainsi que la forme
  du curseur qu'ils demandent (la barre du champ de Claude, un curseur qui clignote). Un programme
  qui copie du texte (`/copy` de Claude, vim, tmux, même par ssh) le met dans le presse-papiers.
- **Couleurs du thème** : le noir, les gris et le blanc des programmes sont ceux du thème de
  Florm, et les autres couleurs s'y accordent ; un programme qui demande les couleurs du terminal
  (Claude Code, vim) reçoit celles du thème et choisit ses couleurs claires ou sombres en
  conséquence.
- Un programme qui sonne (la cloche du terminal) affiche 🔔 devant le nom de son onglet jusqu'à ce
  que tu y reviennes ; si Florm est derrière une autre fenêtre, son icône le signale.
- **Ctrl+Shift+`** : un terminal **dans un onglet** de
  l'éditeur. Comme tout onglet, il se glisse sur le bord droit ou bas de l'éditeur pour être à
  côté d'un fichier (ou clic droit sur l'onglet → Diviser à droite / en bas).
- Dans le panneau du bas, **chaque terminal a son onglet** : son dossier à droite, **+** en ouvre
  un autre, **⇱** l'envoie dans un onglet de l'éditeur (**en bas** dans l'onglet le ramène), **×**
  au survol de son onglet le ferme.
- Toutes les vues (Recherche, Git, Pull requests (GitHub), Claude, Terminal) ont à droite de
  leur en-tête les mêmes boutons, dans le même ordre : **⟳** relire, **en onglet / en bas**
  (une petite fenêtre : avec un onglet en haut, ou une bande en bas ; retenu pour la prochaine fois), **⋯** les autres actions et
  options. Les boutons de disposition se ressemblent tous : les quatre coins agrandissent un
  panneau dans sa vue (Échap pour revenir), une fenêtre avec une colonne à droite affiche ou
  masque un panneau latéral (la revue de Claude), pleine quand il est affiché. Git et les Préférences
  sont des onglets comme les autres : Échap revient à l'onglet d'avant, Ctrl+W les ferme.

## Langages : installer les outils

**Préférences → Langages** (clic droit sur la roue dentée de la barre d'état, menu Outils, ou
« Langages : installer les outils d'un langage » dans la palette)
liste chaque langage avec ses outils : serveur de langage (LSP), linter, formateur, débogueur.
✓ : installé ; ✖ : il manque.

- **Installer** met ce qui manque dans le dossier des outils de Florm, sans toucher au système :
  `pip` dans un environnement virtuel à lui (pylsp, ruff, debugpy, yamllint, clangd…), `npm`
  (typescript-language-server, eslint, les serveurs YAML, Bash, HTML, CSS, Dockerfile…),
  `go install` (gopls, dlv), ou le programme tout prêt de leur release GitHub (rust-analyzer,
  marksman, hadolint, lua-language-server). **Rien d'autre à installer** : sans Python, Node.js ou
  Go sur l'ordinateur, Florm télécharge le sien la première fois (dans son dossier, vérifié par sa
  somme SHA-256). Un clic sur un outil ✖ installe celui-là seul.
- Ce que seul le système installe (gdb, Ruby, luacheck) est donné en commandes pour ton système :
  un clic la tape dans un terminal (Entrée pour la lancer), un clic droit la copie.
- **Tout seul** : en ouvrant un fichier d'un langage auquel il manque des outils, Florm les installe
  en arrière-plan, sans rien demander (une fois par session ; la barre d'état dit où il en est). Le
  fichier s'ouvre tout de suite, coloré ; la complétion du serveur de langage et le lint arrivent
  quand l'installation est finie. Seuls les outils de ce langage sont installés, et seulement ceux
  que tu as cochés en haut de la page : serveur de langage, linter, formateur (par défaut), débogueur
  (sinon installé au premier **F5**, puis la session démarre toute seule). Un outil décoché
  s'installe toujours d'un clic sur sa pastille. Si ça échoue (ou
  si la case « Installer tout seul » est décochée), une barre au-dessus du fichier propose de les
  installer ; **Ne plus proposer** l'arrête pour ce langage.
- **Syntaxe · tree-sitter** (Python, JavaScript, TypeScript, Rust, Go, C, C++, JSON, Shell, CSS,
  HTML, YAML, XML, TOML) : la coloration vient de l'**arbre de syntaxe**
  du fichier, comme dans Zed, au lieu d'expressions régulières : elle comprend le code (appels,
  fonctions intégrées, chaînes et leurs séquences d'échappement…), et Alt+Maj+→ élargit la sélection
  bloc logique par bloc logique. L'arbre suit chaque frappe sans tout relire (moins d'une
  milliseconde sur un fichier de 7 000 lignes). La grammaire est intégrée à Florm, sur tous les
  systèmes ; un clic sur la ligne du pack l'active ou la désactive.
- **+ Nouveau pack…** : un langage que Florm ne connaît pas, ou d'autres outils pour un langage, dans
  un fichier TOML de `~/.config/florm/packs/` (un pack du même nom remplace celui de Florm). Un pack
  peut déclarer un serveur de langage (`cmd` d'un outil `lsp`) et un linter (`cmd` d'un outil `lint`,
  qui écrit `fichier:ligne:col: message`) : Florm les lance pour ses fichiers, dans un dossier de
  confiance.
  ```toml
  name = "Nim"
  ext = ["nim", "nims"]
  [[tool]]
  kind = "lsp"
  bin = "nimlangserver"
  cmd = "nimlangserver"
  hint = "nimble install nimlangserver"
  ```

## Erreurs dans le code

Les problèmes sont soulignés dans le code et listés dans **Problèmes** (panneau du bas) :

- **Python** : le premier linter installé (`flake8`, sinon `ruff`, `pyflakes`, `pylint` ; Préférences →
  Lint), pendant la frappe. **JavaScript** : `eslint`, à l'enregistrement.
- **Serveurs de langage** (Préférences → Éditeur → Serveurs de langage) : leurs erreurs aussi, pour
  tous les langages qu'ils connaissent (TypeScript, Rust, Go, C…). **Formater le fichier** (palette)
  passe par `ruff` pour Python, par le serveur de langage pour les autres. Pour formater chaque
  fichier Python à l'enregistrement : Préférences → Éditeur.
- **XML et JSON** : un fichier mal formé (balise non fermée, mal imbriquée, guillemet oublié), en direct.
- **Les autres langages**, pendant la frappe, avec le linter s'il est installé : `shellcheck` (shell),
  `yamllint`, `markdownlint`, `hadolint` (Dockerfile), `luacheck`, `rubocop`, `stylelint` (CSS).
  N'importe quel autre se déclare dans `settings.toml` : une commande qui écrit des lignes
  `fichier:ligne:col: message` (le texte du fichier lui arrive sur l'entrée standard, `{file}` est
  remplacé par son chemin). Une commande vide coupe un linter de Florm, la même clé le remplace :
  ```toml
  [linters.nimcheck]
  cmd = "nim check --stdin"
  ext = ["nim"]
  [linters.markdownlint]
  cmd = ""
  ```
  Pour une autre sortie, `pattern` : une regex avec les groupes `line`, `col`, `msg`, `severity`.
- **Serveurs de langage de tous les langages** : Florm trouve d'office ceux de YAML, Bash, CSS, HTML,
  Lua, Ruby, PHP, Markdown (`marksman`), TOML (`taplo`), Dockerfile, Terraform, Java, Kotlin, Swift,
  Zig, C#, Haskell, OCaml, Elixir quand ils sont installés (la barre d'état dit lequel installer).
  Un autre langage se déclare avec les extensions de ses fichiers :
  ```toml
  [lsp_servers]
  nim = { cmd = "nimlangserver", ext = ["nim", "nims"] }
  ```

**🔒 Mode restreint.** Un serveur de langage, eslint, les plugins flake8 déclarés par un projet ou la
configuration d'un dépôt git (`.git/config`, filtres de `.gitattributes`) peuvent exécuter son code.
Dans un dossier qui n'est pas encore de confiance, une barre au-dessus du fichier le dit : Git, les
serveurs de langage, eslint et les linters des autres langages ne tournent pas, et flake8 ignore la configuration du projet
(`node --check` et le lint Python restent) ; la vue Git le dit aussi, avec le même bouton. **Faire confiance à « dossier »** les lance, pour
ce dossier et ceux qu'il contient (clé `trusted_folders` de `settings.toml`) ; **Plus tard** cache la
barre jusqu'à la fermeture de Florm. **Préférences → Lint → Dossiers de confiance** les liste ; × retire
la confiance d'un dossier, qui repasse en mode restreint.

## Tests (pytest, jest, cargo, go)

Les tests de n'importe quel projet se lancent dans le terminal du panneau du bas :

- **pytest** pour `test_*.py` et `*_test.py` (le `pytest` du `.venv` du projet s'il y en a un),
  **jest** (ou **vitest**, si `package.json` le demande) pour `*.test.js`, `*.spec.ts`…,
  **cargo test** pour les `#[test]` d'un fichier Rust, **go test** pour `*_test.go`.
- Le **▶** dans la marge d'un test le lance ; **Ctrl+Maj+F6** lance le test sous le curseur,
  **Ctrl+F6** relance le dernier. La commande est montrée telle quelle (`pytest tests/test_a.py::TestX::test_y`,
  `cargo test app::calc::tests::adds`…) : dans « Tests : lancer des tests… », on peut la modifier avant
  Entrée. Un projet dans un sous-dossier commence par `cd sous-dossier && `.
- La sortie s'affiche dans le panneau du bas, le résumé (✔ ou ⚠, réussis et en échec) dans la barre d'état.
- Un `ipdb` dans un test s'arrête dans ce terminal, où il se tape comme d'habitude.

## Déboguer (F5)

Le débogueur de Florm parle le protocole des débogueurs (DAP), comme Zed et VS Code : **debugpy**
pour Python, **dlv** pour Go, **lldb-dap** ou **gdb** (14 ou plus) pour Rust, C et C++.

- **Point d'arrêt** : un clic dans la colonne à gauche des numéros de ligne (un point rouge pâle
  la montre au survol), ou **F9** sur la ligne. Ils suivent les lignes quand le texte change.
- **Une commande, comme dans un terminal** : la vue **Débogueur** (🐞 dans la barre d'état) s'ouvre
  sur **Commande** (la liste en haut à gauche) ; tape
  la ligne qui lance ton programme (`python app.py --port 8000`, `python -m flask run`,
  `pytest tests/test_a.py`, `go run .`, `./mon_programme --verbose`, des variables devant comme
  `DEBUG=1 ./mon_programme`, le Python d'un environnement virtuel comme
  `~/venvs/app/bin/python app.py`) et son **dossier** : vide, le projet ; un chemin relatif part du
  projet ; **📁** ouvre le sélecteur de dossiers du système. Un dossier qui n'existe pas s'affiche en
  rouge (le survol dit lequel) et F5 le refuse. Puis **Entrée** ou **F5**. Florm choisit le débogueur d'après la commande ; vide, F5 débogue le fichier ouvert. Chaque commande lancée reste dans **🕘**, par projet ;
  la dernière revient à la réouverture. Une commande enveloppée (`make`, `docker`…) ne se débogue
  pas directement : lance le programme avec `debugpy --listen` et attache-toi (voir plus bas).
- **F5** lance ce qui est choisi dans la vue **Débogueur** (panneau du bas) : la **commande**, le **fichier
  ouvert**, le **test sous le curseur** (pytest, go test, cargo test ; aussi « Débogueur : déboguer
  le test sous le curseur » dans la palette) ou une configuration du projet. Les fichiers modifiés
  sont enregistrés avant. Rust : `cargo build` d'abord, puis le binaire du paquet.
- Arrêté, la ligne est sur fond jaune avec une flèche dans la marge. **F5** continue, **F10** passe à
  la ligne suivante, **F11** entre dans la fonction, **Maj+F11** en sort, **Maj+F5** arrête,
  **Ctrl+Maj+F5** relance (aussi les boutons ▶ ⏸ ↷ ↓ ↑ ⟳ ■ du panneau).
- La vue **Débogueur** : la **pile d'appels** (un clic montre la ligne d'un appel et ses variables),
  les **variables** (▸ ouvre une liste, un objet), la **console** : la sortie du programme, et une
  expression tapée en bas (Entrée) est évaluée là où il est arrêté.
- **Configurations du projet** : « Débogueur : configurations du projet » crée et ouvre
  `.florm/debug.toml`, avec des exemples. Une par `[[config]]` : `adapter` (python, go, lldb, gdb),
  `program` ou `module` (Python), `args`, `cwd`, `env`, `build` (une commande avant, comme
  `cargo build`) ; `{file}`, `{dir}` et `{root}` sont remplacés. **Attacher** à un programme Python
  qui tourne déjà, dans un conteneur par exemple (`python -m debugpy --listen 0.0.0.0:5678 …`) :
  `request = "attach"`, `port = 5678`, `path_mappings = { "/app" = "{root}" }`.
- debugpy manquant : « Débogueur : installer debugpy » (palette) l'installe pour Florm seul. Comme
  les serveurs de langage, le débogueur ne tourne que dans un dossier de confiance.

**Ctrl+clic** sur une ligne `File "…", line N` d'une trace d'erreur ouvre le fichier.

## Modèles

**Ctrl+Alt+N** (ou Outils → Insérer un modèle) : les modèles du type du fichier courant sont
proposés en premier. Florm en livre deux, `ipdb` et `pdb` (Python : taper `ipdb` puis Tab) ; ajoute les tiens (voir plus bas).

`$0` marque la place du curseur.

Pour ajouter les tiens : **Outils → Dossier des modèles**, un fichier par modèle
(son nom = le nom du modèle, son extension = le type de fichier). Un fichier qui porte le nom
d'un modèle intégré le remplace, avec « · » à la place de « : » (interdit dans un nom de fichier
sous Windows) : `Python · pdb.py` remplace « Python : pdb ».

**Déclencheurs**, comme les snippets de Sublime : taper `ipdb` dans un fichier Python le
propose dans les suggestions, **Tab** (ou Entrée) le remplace par
`import ipdb; ipdb.set_trace()`. Aussi `pdb`. Un modèle perso dont le nom est un seul mot
(`log.py`, `ifmain.py`) a ce mot pour déclencheur.

**Préférences → Modèles** : la liste des tiens et des intégrés ; un clic en ouvre un pour
changer son nom (un mot = son déclencheur), les fichiers pour lesquels il est proposé et son
texte. **+ Nouveau modèle** en crée un ; un intégré modifié devient le tien et le remplace.

**Outils → Nouveau modèle…** (ou la palette) : tape le déclencheur (`log`), Florm crée
`log.py` (extension du fichier courant ; `log.xml` pour un autre type) dans le dossier des
modèles et l'ouvre. S'il y avait une sélection, elle devient le texte du modèle.

## Extensions

Une extension ajoute des commandes à la palette (avec un raccourci si elle veut) et réagit quand un
fichier s'ouvre, s'enregistre, se ferme ou s'affiche. C'est un programme, en Python ou dans n'importe
quel langage, dans un dossier de `~/.config/florm/extensions/` avec son `extension.toml`. Elle tourne
avec tes droits : n'en installe que de sources sûres. Sans Python sur l'ordinateur, Florm télécharge
le sien à la première commande d'une extension Python (une seule fois, ~30 Mo).

- **Préférences → Extensions** (clic droit sur la roue dentée de la barre d'état, ou menu Outils) : la liste (une case les coupe), **+ Nouvelle extension…** crée un
  exemple en Python et l'ouvre, **⟳ Recharger** après un changement, **📁** le dossier.
- Leurs commandes sont dans la palette, précédées de ✚. Ce qu'elles écrivent (`print`) et leurs
  erreurs vont dans **Sortie**.
- `extension.toml` :
  ```toml
  name = "Mon extension"
  cmd = "python3 main.py"        # lancée dans son dossier, au premier besoin
  events = ["save"]              # open, save, close, focus
  [[command]]
  id = "upper"
  title = "Mettre la sélection en majuscules"
  key = "ctrl+alt+u"
  ```
- `main.py`, avec la bibliothèque que Florm lui donne (`florm.py`) :
  ```python
  import florm

  @florm.command("upper")
  def upper(ctx):                # ctx.file, ctx.root, ctx.selection, ctx.line, ctx.col
      florm.replace_selection(ctx.selection.upper())

  @florm.on("save")
  def saved(ctx):
      florm.status(f"{ctx.file} : {florm.text().count(chr(10))} lignes")

  florm.main()
  ```
- Ce qu'une extension peut faire : `status`, `message` (info, warning, error), `output`, `text()`,
  `replace_selection`, `insert`, `set_text` (annulé par Ctrl+Z), `open(chemin, ligne)`,
  `pick(liste, titre)` (l'indice choisi, None sur Échap), `input(titre)`, `run("save")` (une
  commande de Florm, les identifiants de `[keys]`), `terminal("make test")`.
- Dans un autre langage : Florm lui parle en JSON-RPC sur son entrée et sa sortie, comme à un serveur
  de langage (en-tête `Content-Length`) ; les méthodes sont celles de `florm.py`.

**Catalogue** (Préférences → Extensions, en bas ; ou « Extensions : installées et catalogue » dans
la palette) : les extensions et packs de langage livrés avec Florm, ceux de ta configuration
partagée (ses dossiers `extensions/` et `packs/`), et ceux des dépôts de catalogue que tu ajoutes
(`propriétaire/dépôt` ou une adresse git : un dossier par extension, et `packs/*.toml`). **Installer**
copie dans tes dossiers, **Mettre à jour** quand le catalogue a une autre version (l'état que
l'extension garde reste), **Désinstaller** retire. Une extension qui ne vient pas de Florm demande
confirmation : elle tournera avec tes droits. **⟳ Mettre à jour les catalogues** les récupère à
nouveau.

**L'extension Odoo** (dans le catalogue de Florm) :
- **Ctrl+Alt+O** : lance le test sous le curseur (`--test-tags /sale:TestOrder.test_confirm`, la
  classe, ou le module hors d'un test) dans un terminal. La première fois, la commande s'affiche pour
  la compléter (la base `-d`, un `docker compose exec …` devant) ; ensuite seuls les tags changent.
  « Odoo : modifier la commande des tests » pour en changer.
- **Alt+F12** : sur `'sale.order'`, va au modèle (`_name`, puis ses `_inherit` au choix) ; sur
  `'module.xml_id'` ou `ref="…"`, va au record (XML, CSV).
- « Odoo : mettre à jour le module (-u) », « modèles et records du fichier », « ouvrir le
  `__manifest__.py` du module ».

## GitHub

Florm passe par la commande `gh` : pas de token à créer, il suffit de s'être connecté une fois
avec `gh auth login` (connexion par le navigateur). Sans `gh`, la vue explique comment l'installer
(le système est détecté) et reprend tout seul dès qu'elle est trouvée.

### Pull requests

**Ctrl+Alt+R** (ou Git → Pull requests GitHub, **⋯ → Pull requests GitHub** dans la vue Git, la palette).

- À gauche, les PR du dépôt (ouvertes, fusionnées, fermées, les tiennes), filtrables. Chaque
  ligne : les initiales de l'auteur (le point dessus dit si la PR est ouverte, brouillon, fusionnée
  ou fermée), le titre sur deux lignes au plus, `#numéro · branche · âge`, et à droite l'état de
  la revue (**👍** approuvée, **✖** à corriger, **👁** revue demandée) ; les checks sont dans la PR
  ouverte (la liste se charge ainsi bien plus vite). Au départ,
  **les tiennes** (préférence « Auteur affiché au départ » pour voir toutes les PR).
- Une PR que tu viens de créer s'ouvre dès que GitHub la connaît (quelques secondes).
- **La vue sert à lire** : pour écrire (commenter, répondre, approuver, résoudre une
  conversation), **Ouvrir sur GitHub** dans l'en-tête. Les aides de Claude restent (✳ Revue IA,
  🗺 Expliquer, ✳ Envoyer à Claude), ainsi que l'application des suggestions.
- **Conversation** : la description en haut, puis tout ce qui s'est passé du plus ancien au plus
  récent : les commentaires et revues (dans un cadre « bob a commenté il y a 2 jours »), les
  **commentaires sur le code** (fichier:ligne cliquable, le code commenté, les réponses) et les
  commits (un clic les montre). En bas : si la PR peut être fusionnée.
- **Traiter les commentaires avec Claude** : coche les commentaires non résolus à traiter
  (case à gauche de chacun, dans Conversation ou Fichiers ; « Tous » en haut), puis
  **✳ Envoyer à Claude**. Une conversation Claude s'ouvre dans le dépôt avec chaque
  commentaire (fichier:ligne, code commenté, réponses) et change le code ; tu relis dans sa
  revue. Il faut d'abord **⤵ Checkout** la branche de la PR (bouton dans l'en-tête).
- **Appliquer la suggestion** : un commentaire avec un bloc `suggestion` remplace les lignes
  dans ton fichier et l'enregistre (Ctrl+Z dans le fichier pour annuler). Il faut d'abord
  **⤵ Checkout** pour avoir la branche de la PR (bouton dans l'en-tête).
- Branche de la PR active : les commentaires non résolus sont **dans l'éditeur**, une petite
  bulle dans la marge, le texte au survol.
- **Commits** : un clic montre le message complet du commit et ses changements à lui seul ;
  **double-clic** (ou **Voir dans la vue Git ↗**, ou clic droit) l'ouvre sélectionné dans le
  graphe de la vue Git (les commits de la PR sont d'abord récupérés dans `remote/pr/N` s'ils ne
  sont pas dans ton dépôt). **Fichiers** : tout le
  diff de la PR, les conversations **sous les lignes commentées** comme sur GitHub (appliquer
  une suggestion), et **double-clic sur une ligne** pour ouvrir le fichier là.
- Une case avant chaque fichier le marque **vu** : il se replie et GitHub s'en souvient pour
  ta revue (« 3/10 vu(s) » en haut).
- **✳ Revue IA** : Claude relit la PR et propose des remarques sous les lignes du diff ; rien
  n'est envoyé. Chacune se **garde**, se modifie (✏) ou s'écarte (×), puis **Publier** envoie
  celles gardées en une revue GitHub. **💬** sur une remarque en discute avec Claude pour
  l'affiner (« plus court », « en anglais », « tu es sûr ? regarde `_compute_total` ») : il
  répond sous la remarque et la réécrit si besoin, **↶ Version d'avant** revient en arrière.
  Aussi dans la vue Git, pour les changements pas encore commités.
- **🗺 Expliquer** (en-tête de la PR) : Claude explique la PR idée par idée, dans l'onglet Claude
  (voir plus bas). **🗺 Expliquer ce commentaire**, sous chaque conversation : Claude reformule ce que
  le relecteur demande, dit s'il a raison et pourquoi, et ce que ça changerait.
- En bas de la conversation, le cadre de fusion, dans la couleur de l'état (rouge bloquée, orange
  s'il manque quelque chose, vert prête) : où en est la PR (**Revue requise**, **Conflits**, **En
  retard**, **Prête à fusionner**…), les **checks** (ceux en échec toujours visibles, un clic pour
  tous ; **Logs** ouvre le log d'un job GitHub Actions dans un onglet, **↻ Relancer les
  échecs**), et les boutons : **Fusionner** (commit de fusion, squash ou rebase, supprimer la
  branche), **⟳ Mettre à jour la branche** (merge ou rebase de la base), **Prête pour la
  revue** / **Repasser en brouillon**, **Fermer** / **Rouvrir**. Fusionner, fermer et rebaser
  demandent confirmation.
- À droite, pour information : les relecteurs avec leur avis (✔ approuvé, ✖ changements demandés,
  💬 commenté, ⏳ en attente), « Relecture requise » quand le dépôt exige une approbation, et les
  assignés. Ils se changent sur GitHub, comme les étiquettes et le jalon.
- Clic droit sur une PR de la liste : ouvrir, ouvrir sur GitHub, checkout, copier le lien ou le
  numéro. Dans la PR ouverte, **Ouvrir sur GitHub** et **⤵ Checkout** sont des boutons de
  l'en-tête, et **⋯** copie le lien ou le numéro.

## Claude

**Ctrl+Alt+C** (ou Outils → ✳ Claude, la palette) : **Claude Code lui-même**, dans un terminal au
centre de l'onglet Claude.
Florm lance la commande `claude` avec ta connexion : pas de clé à configurer. Ses commandes
(`/model`, `/usage`, `/mcp`, `/memory`, `/agents`…), ses modes (**Maj+Tab**), ses permissions et
ses nouveautés marchent tels quels. Les changements de Claude se revoient dans la vue **Git**.

- **En haut** : le dossier (choisi avant le premier message : un dépôt du projet, un projet
  récent, ou **📁 Un autre dossier…**), **+ Contexte**, le compte 👤,
  l'**Historique** (les conversations d'avant, gardées entre les sessions) et **⟳** pour relancer
  Claude (la conversation reprend où elle en était).
- **La conversation affichée** s'agrandit dans la liste de gauche : son **contexte** en % et son
  **coût**, son **modèle** (et son compte s'il y en a plusieurs), lus dans la barre d'état de
  Claude Code. Florm recharge les fichiers et rafraîchit Git dès que Claude les change.
- **Utilisation**, en bas de la liste : les **quotas** 5 h et 7 j de ton abonnement (au survol,
  quand ils repartent à zéro).
- **Ce que Claude attend** : quand il demande une permission ou pose une question, son message
  (« veut utiliser Bash »…) s'affiche au survol de la conversation dans la liste et de **❓ attend**
  dans la barre d'état. Si Florm n'est pas au premier plan, une notification du système te prévient
  tout de suite, de même quand Claude a fini, même si la fenêtre de Florm est cachée (macOS ;
  Windows ; Linux avec `notify-send`). **Un clic sur la notification** ramène Florm devant, sur
  cette conversation : sous Windows et Linux (Hyprland, Sway ; ailleurs Florm le demande lui-même,
  sans garantie selon le bureau), et sous macOS si `terminal-notifier` est installé
  (`brew install terminal-notifier`).
- **Plusieurs conversations, un seul onglet** : toutes les conversations sont listées à gauche
  de l'onglet Claude, la plus récente en haut, avec leur titre (leur premier message), ce qu'elles
  font (⟳ travaille, ❓ attend ta réponse, ✔ a fini) et depuis quand elles travaillent ; en
  dessous, leur projet dans sa couleur et leur branche. **×** au survol d'une ligne la ferme. Un clic en affiche une, les
  autres continuent derrière ; **+** en commence une nouvelle. Clic droit : **⇉ Dupliquer** (une
  copie de sa session, pour essayer autre chose), **⇱ Ouvrir dans son propre onglet**, **× Fermer**
  (Claude s'arrête, elle reste dans l'historique). **Ctrl+Alt+W** : la suivante qui attend ta
  réponse. La barre d'état dit combien travaillent ou attendent.
- **Bac à sable** (Préférences → Claude, activé par défaut) : les commandes de Claude tournent
  isolées, elles n'écrivent que dans le projet et le réseau est filtré. Claude ne peut lancer
  **aucune commande hors du bac à sable**, même en mode auto : ce qui en a besoin (un `git push`
  avec ta clé SSH…), tu le lances toi-même (`! git push` dans Claude, ou un terminal). Linux : paquets
  `bubblewrap` et `socat` ; macOS : intégré (Seatbelt) ; Windows : non disponible. Désactivé,
  il n'y a aucun bac à sable, même si tes réglages de Claude Code en activent un. Le changer ne
  demande pas de redémarrer Florm : les conversations ouvertes repartent avec le nouveau réglage
  (celles qui travaillent, quand elles ont fini). Activé, `/sandbox` dans Claude règle le détail.
- **Après un redémarrage de Florm**, chaque conversation reprend où elle en était
  (`claude --resume`).
- **Fin de réponse** : si la conversation n'est pas affichée, son onglet (ou, dans le panneau du
  bas, ✳ ● à côté de son nom et ✔ dans la barre d'état) clignote quelques secondes puis reste vert
  jusqu'à ce que tu la regardes, et un son est joué. **Préférences → Claude** : clignoter ou non,
  et le son (Carillon, Clochette, Goutte, Bip, Accord, Micro-ondes, ou aucun ; ▶ pour écouter).
- **Donner des lignes ou des fichiers à Claude** : c'est écrit dans son champ, sans l'envoyer,
  pour que tu continues ta demande. Au survol d'un numéro de ligne, clic sur le **✳** à sa gauche
  (la ligne, ou les lignes sélectionnées) ; clic droit dans l'éditeur → **Demander à Claude à
  propos de la sélection…** (**Ctrl+Alt+K**) ou **Ajouter … au contexte** ; clic droit sur un
  fichier ou un dossier de l'arborescence → **Ajouter au contexte de Claude** (`@fichier`, que
  Claude lit). **+ Contexte** : le fichier ouvert, la sélection, tous les onglets, les changements
  non commités, les problèmes du fichier, une image ou un PDF, **Un commit** (aussi : clic droit
  sur un commit dans la vue Git → **Donner ce commit à Claude**). **Reprendre la conversation de ce
  fichier** (palette) rouvre la dernière conversation à qui ce fichier a été donné.
- **✳ Envoyer à Claude** depuis une pull request (ses commentaires) ou la barre d'un fichier en conflit :
  une conversation dans le dépôt, qui démarre avec la demande.
- **Images** : une capture faite avec l'outil du système (**Impr** ; **Cmd+Ctrl+Maj+4** sur Mac,
  **Win+Maj+S** sous Windows, rappelés dans + Contexte) va dans le presse-papiers ;
  **Ctrl+V** dans Claude (Cmd+V sur Mac), ou **+ Contexte → Image du presse-papiers**, la donne à
  Claude. Une image ou un PDF se glisse aussi
  depuis le gestionnaire de fichiers sur Claude, ou se choisit avec **+ Contexte → Image ou PDF…**.
  Une image est réduite à 1568 pixels pour coûter moins de tokens.
- **Plusieurs comptes Claude** (pro, perso…) : **👤 → + Ajouter un compte…** en haut de Claude, ou **Préférences → Claude → Comptes Claude**,
  « + Ajouter un compte », puis **Se connecter…** (la connexion se fait une fois, dans un terminal
  de Florm ; chaque compte a son dossier, `~/.claude-perso` par exemple). Le menu **👤** en haut
  d'une conversation choisit son compte : changé en cours de route, Claude redémarre avec lui et la
  conversation garde son contexte. Le compte « par défaut » sert aux nouvelles conversations et aux
  messages de commit, descriptions de PR et revues. **+ Dossier…** sur un compte : les nouvelles
  conversations de ce dossier (et de ses sous-dossiers) l'utilisent ; **📁** change son dossier de
  configuration. Les quotas suivent le compte de la conversation affichée.
- **Contexte Florm** : au démarrage de Claude, Florm lui dit ce qu'il sait du projet : les
  **consignes des messages de commit** (celles du projet, sinon les tiennes) et la branche où tu
  travailles. Claude écrit alors ses commits comme ✨. Pour lui apprendre à lancer tes
  tests (un conteneur, des tags…), un **skill** de Claude Code s'en charge mieux : il ne sert que
  quand c'est utile.
- **Mes skills** (Préférences → Claude) : un dossier à toi pour tous tes skills (un dépôt GitHub à
  part, par exemple), donné à chaque conversation de Claude quel que soit le compte : aucun projet
  n'a à les contenir. **Choisir…** le désigne ; range-le comme tu veux, chaque `SKILL.md` trouvé est
  un skill, même dans des sous-dossiers par thème. Claude les voit sous le nom `florm:nom`.
  **+ Nouveau skill** crée un `SKILL.md` à remplir (sa `description` dit à Claude quand s'en
  servir). Sans dossier choisi : `skills/` de la configuration partagée.
- **Complétion par IA** (Préférences → Éditeur, ou la palette : « complétion par IA pendant la
  frappe ») : quand tu t'arrêtes de taper en fin de ligne, **GitHub Copilot** propose la suite en
  gris après le curseur, en général en moins d'une seconde (abonnement Copilot). **Tab** la prend,
  **Échap** ou une autre touche l'ignore ; taper ses premières lettres garde le reste. **Alt+\\**
  demande une proposition tout de suite, même désactivée. **Installer Copilot** télécharge son
  serveur officiel (environ 160 Mo) dans le dossier de Florm, sans npm ni droits administrateur ;
  **Se connecter à GitHub** copie un code et ouvre la page où le taper, une fois pour toutes.
- **Fermer Florm pendant que Claude travaille** : Florm le dit et propose **Fermer quand il aura
  fini** (Florm se ferme tout seul à la fin ; « Ne pas fermer » pour rester) ou **Fermer
  maintenant**. Une mise à jour attend aussi que Claude ait fini pour redémarrer Florm.

**✨ à côté du message de commit** (vue Git) : Claude propose le message d'après les changements
indexés (sinon tous), dans le style des derniers commits du dépôt et d'après ta première demande
dans l'onglet Claude.

### 🗺 Expliquer un changement

Pour comprendre **ce que Claude propose et pourquoi**, idée par idée plutôt que fichier par
fichier. **🗺 Expliquer** dans la vue Git (à côté de ✳ Revue IA), ou la palette (« Git :
expliquer les changements non commités ») : la demande part dans l'onglet Claude, dans la
conversation qui travaille dans ce dossier (elle sait pourquoi elle a fait ces changements), sinon
dans une nouvelle. Claude donne le problème et l'idée, puis chaque idée avec les endroits du code
(chemin:ligne, cliquables) et ce qui pourrait casser ; il ne modifie rien. On continue la
discussion avec lui comme d'habitude.

## Markdown

Au-dessus du texte d'un fichier `.md`, une **barre de mise en forme** : **B** gras, *I* italique,
~~S~~ barré, `c` code, Lien, titres H1 à H3, listes (• et 1.), case à cocher ☐, citation ❝, bloc
de code, ligne horizontale `――` et tableau `▦`. Le bouton met la syntaxe autour de la sélection, ou
sur les lignes sélectionnées ; un second clic enlève le préfixe. Le texte reste modifiable à la main.
Pour masquer la barre : Préférences → Apparence → Barre de mise en forme Markdown. L'aperçu rendu reste
Ctrl+Shift+V (onglet) ou Ctrl+K V (à côté).

Dans l'aperçu (et dans les réponses de Claude), les **diagrammes de séquence Mermaid**
(` ```mermaid ` puis `sequenceDiagram`) sont dessinés par Florm : participants, acteurs, messages,
notes, `loop` / `alt` / `opt`, `autonumber`. Les autres types de diagrammes restent affichés en code.
Le **HTML** courant s'affiche aussi : `<br>`, `<b>`, `<i>`, `<kbd>`, `<img>`, `<a href>`,
`<details>` / `<summary>`, `<h1>`…, `<p align>` ; les commentaires `<!-- -->` sont masqués.

Les images de l'aperçu sont des adresses `http(s)` ou des fichiers du projet (un chemin relatif,
20 Mo au plus) ; un lien symbolique, un fichier hors du projet ou un chemin absolu n'est pas lu,
son texte s'affiche à la place. Dans une pull request ou un commentaire, seules les images `http(s)`
sont gardées. Une image SVG ne lit jamais d'autre fichier du disque (ses `<image>` intégrées en
`data:` s'affichent). Les liens (Markdown, pull requests, Claude, terminal) ne s'ouvrent dans le
navigateur que s'ils sont en `http` ou `https`.

## Écrire

- **Orthographe** : les mots inconnus du Markdown, des commentaires du code et des messages de
  commit sont soulignés de points bleus, en français et en anglais à la fois (les noms propres,
  le code, les adresses et les identifiants sont laissés de côté). **Ctrl+.** sur un mot propose
  ses corrections, ou **l'ajoute à ton dictionnaire** (`dictionary.txt`, dans la configuration
  partagée s'il y en a une). Les dictionnaires (Grammalecte pour le français, SCOWL pour
  l'anglais) sont téléchargés la première fois, environ 550 ko. Se désactive dans Préférences →
  Éditeur → Orthographe.
- **Alt+Maj+→** élargit la sélection pas à pas : le mot, l'intérieur de la chaîne, la chaîne avec
  ses guillemets, l'intérieur des parenthèses (crochets, accolades), avec elles, la ligne, le bloc
  de même indentation, tout le fichier. **Alt+Maj+←** revient au pas d'avant. Dans un langage qui a
  son **arbre de syntaxe** (Python, JavaScript, TypeScript, Rust, Go, C, C++, JSON…), chaque pas suit le code : le nom, l'argument,
  la liste des arguments, l'appel, l'instruction, le bloc, la fonction, la classe.
- **Édition → Casse** : MAJUSCULES, minuscules, **snake_case**, **camelCase**, **Majuscule Au
  Début** des mots, sur la sélection ou le mot du curseur (`parseHTTPRequest` → `parse_http_request`).
  **Édition → Supprimer les lignes en double** garde la première de chaque ligne répétée (la
  sélection, ou tout le fichier).
- **Macros** : **Ctrl+Alt+Q** commence l'enregistrement de ce que tu tapes et des commandes
  d'édition, **Ctrl+Alt+Q** à nouveau l'arrête ; **Ctrl+Maj+Q** le rejoue, au curseur.
- **Paramètres d'une fonction** : avec un serveur de langage, taper `(` ou `,` dans un appel
  affiche au-dessus du curseur la signature de la fonction, le paramètre en cours souligné, et sa
  documentation. Elle disparaît à `)`, avec Échap ou en quittant la ligne.
- **Enregistrement automatique** (Préférences → Éditeur) : **En quittant** enregistre les
  fichiers modifiés quand Florm perd le focus ou qu'on change d'onglet ; **Après une pause**, une
  seconde après la dernière touche. Les nouveaux fichiers sans nom ne sont pas concernés.
- **Travail non enregistré** : à la fermeture, il est gardé et rouvert la fois suivante, même
  sans dossier ouvert. Il est aussi copié toutes les 30 secondes : si Florm plante, on le retrouve
  au démarrage suivant, avec un message qui indique où l'erreur est notée (`crash.log`, dans le
  dossier de configuration).
- **Crochets colorés** : chaque niveau de parenthèses, crochets et accolades a sa couleur (or,
  violet, bleu), sans compter ceux des chaînes et des commentaires. Le guide d'indentation du bloc
  du curseur est plus marqué que les autres.
- **Espaces visibles** (Affichage, ou Préférences → Apparence) : un point par espace, une flèche
  par tabulation.
- **Mode sans distraction** (**Ctrl+K Z**, ou Affichage) : plein écran, seulement le texte,
  centré ; Échap ou Ctrl+K Z pour en sortir.
- Sans retour à la ligne, une ligne qui dépasse le bord droit s'estompe : on voit qu'elle continue.

## Configuration partagée

Les préférences, raccourcis, modèles, thèmes et skills de Claude peuvent venir d'un dépôt GitHub,
le même sur chaque ordinateur : **Préférences → Avancé → Configuration partagée**, avec ton login
(le dépôt `login/florm-config`), `owner/repo` ou une adresse git. **Créer sur GitHub** fait le
dépôt (privé) à partir de cet ordinateur ; **Publier** envoie tes changements, **Synchroniser**
récupère ceux des autres (aussi à chaque démarrage). Ce qui est propre à un ordinateur (les
commandes des tests, des serveurs de langage et du linter, les dossiers de confiance, les
comptes Claude…) n'est jamais partagé.

## Apparence

L'interface suit le design de Zed : les panneaux, la barre d'onglets et les menus un cran plus clairs
(plus foncés sur un thème clair) que l'éditeur, la barre de titre et la barre d'état un cran au-delà, de
fines lignes entre les parties, l'onglet actif sur le fond de l'éditeur. Ces teintes viennent du thème
choisi : elles suivent tous les thèmes.

Le texte est en IBM Plex, intégrée à Florm : la même police sur Linux, Windows et macOS. Sur un écran
peu dense (un portable 1080p à 100 %), Florm prend sa graisse « Text », un peu plus pleine, pour que
le texte ne paraisse pas plus fin et plus gris que sur l'écran Retina d'un Mac.

**Préférences → Apparence** : langue (Français / English), thème (aperçus cliquables : Florm et
Florm Light, Florm Minimal et Florm Minimal Light, plus sobres (un fond profond ou crème, une
palette douce), et Florm Monokai, One Dark, Dracula, GitHub Light d'après les thèmes habituels des autres
éditeurs), couleur d'accentuation,
**texte adouci** (texte moins « gras », utile sur les thèmes clairs), polices de l'éditeur et
de l'interface, minimap, fil d'Ariane, **barre de menus** (masquée par défaut, comme dans Zed : les menus
sont sous le logo F en haut de la barre latérale, et leurs commandes dans la palette, **Ctrl+Shift+P**).

**Couleurs du thème → Créer un thème à partir de « … »** : toutes les couleurs de l'éditeur
(fond, texte, accent, curseur, sélection, marge…) et du code (commentaires, mots-clés, chaînes,
fonctions, classes, balises…, en italique ou non), appliquées tout de suite. **Enregistrer le
thème** l'écrit dans le dossier `themes` de la configuration ; **Modifier ce thème** /
**Supprimer ce thème** quand c'est un des tiens.

**Git se désactive aussi** (Préférences → Git), et il l'est tout seul quand la commande `git` n'est
pas installée : plus de vue Git, d'historique, de blame, de marques de changement dans la marge ni
de pull requests, et `git` n'est plus appelé. Sa section dit alors comment l'installer.

**GitHub et Claude se désactivent** : un interrupteur en haut de leur section des
préférences (Pull requests, Claude). Désactivés, leurs vues, commandes, menus, icônes,
raccourcis et boutons disparaissent (✨ de Git, revue IA, colonne ✳ de la
marge, PR liées dans la vue Git…), et `gh` n'est plus appelé. Le même interrupteur les ramène,
sans redémarrer. **À l'installation, les deux sont désactivés** : leur section dit alors ce qu'il
leur faut (`gh` ; `claude`), coché ✔ si c'est installé, sinon avec les commandes
d'installation pour ton système (« Dans le terminal », « Copier ») et
« Vérifier à nouveau ».
