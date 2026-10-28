# Guide de Florm

Ce guide s'ouvre avec **Aide → Guide de Florm** (ou la palette : « guide »).
Tous les raccourcis se changent dans **Préférences → Raccourcis** (Ctrl+,).

---

## Trouver et ouvrir

| Raccourci | Action |
|---|---|
| **Ctrl+P** | Aller à un fichier du projet. `nom:42` ouvre à la ligne 42, `:42` va à la ligne 42 du fichier courant |
| **Ctrl+Shift+P** | Palette de commandes (les accents sont facultatifs : « theme » trouve « Choisir le thème… ») |
| **Ctrl+R** | Aller à une classe, une fonction, un record Odoo du fichier |
| **Ctrl+O** / **Ctrl+Shift+S** | Ouvrir / enregistrer sous : **Tab complète le chemin** comme dans un terminal |
| **F12** | Aller à la définition (aussi sur `'sale.order'` ou `'module.xml_id'`) |
| **Ctrl+.** | Corrections rapides du problème sous le curseur (aussi l'ampoule en bout de ligne) : importer un nom du projet, ignorer (`# noqa`), `ruff --fix`, corriger avec Claude, et les actions du serveur de langage |
| **Alt+Z** | Retour à la ligne automatique des lignes trop longues (l'affichage seulement ; ↑ ↓ suivent les rangées) |

Le fil d'Ariane au-dessus du code est cliquable : un dossier montre son contenu, le fichier
liste ses symboles, une classe ou une fonction y emmène. Les vues (Claude, Git, GitHub,
Docker, terminal…) n'en ont pas : elles gardent toute la hauteur.

**Groupes d'onglets** : clic droit sur un onglet → **Ajouter à un groupe** (un existant ou
**+ Nouveau groupe**). Les onglets d'un groupe restent côte à côte, avec une bande de sa couleur
et son nom devant eux : un clic sur le nom replie le groupe (seul son nom reste, avec le nombre
d'onglets), clic droit pour le renommer, fermer ses onglets ou le dissoudre. Dans une pull
request, **📑 Ouvrir les fichiers** les ouvre dans le groupe « PR #… ». Les groupes sont gardés
avec la session.

Au redémarrage (ou en revenant sur un projet), tout revient comme tu l'as laissé : les onglets et
les vues, la position du curseur, l'éditeur divisé avec ses onglets de chaque côté, le panneau du
bas ouvert et la barre latérale affichée ou non.

## Barre latérale

- **Ctrl+Shift+E** (ou un clic sur un fichier) : le clavier va dans l'arborescence. **↑ ↓** pour se
  déplacer, **→** ouvre un dossier, **←** le referme (ou remonte au dossier parent), **Espace**
  affiche le fichier en gardant le clavier dans l'arborescence, **Entrée** l'ouvre et passe à
  l'éditeur, **F2** renomme, **Suppr** supprime, **Échap** revient à l'éditeur.
- En défilant, les dossiers qui contiennent le haut de la liste restent figés en haut ; un clic
  remonte au dossier.
- **Double-clic sur le bord** entre la barre et l'éditeur : la largeur s'ajuste au nom le plus long
  de l'arborescence. Le bord se glisse pour changer la largeur ; **Ctrl+K Ctrl+B** masque la barre,
  et un clic sur la fine bande à gauche la réaffiche.
- Sur le dossier d'un dépôt git : sa branche, la branche distante suivie (↑ à pousser, ↓ à
  récupérer) et le nombre de fichiers modifiés ; le survol montre les remotes
  (désactivable dans Préférences → Git).
- Les icônes des vues (Préférences, Recherche, Terminal, Claude, Git…) sont **dans la barre d'état, en bas à gauche**, comme dans Zed. **▾** au bout de la ligne choisit celles qui sont affichées, et permet de les remettre dans la barre latérale (en haut ou en bas). Une vue masquée reste dans la palette (Ctrl+Shift+P) et les menus.
- **Copier / coller des fichiers** : clic droit → **Copier** met le fichier ou le dossier sur le presse-papiers du système ; **Coller** (ou Ctrl+C / Ctrl+V dans l'arborescence) y dépose les fichiers copiés depuis le gestionnaire de fichiers de ton PC, ou depuis l'arborescence. Un nom déjà pris devient « nom (copie).ext » : rien n'est remplacé.
- Clic droit sur un dossier → **Initialiser un dépôt git ici** (`git init`), pour un nouveau projet.

## Rechercher dans le dossier (Ctrl+Shift+F)

Entrée lance la recherche. Les résultats sont colorés comme dans l'éditeur ; **F4 / Maj+F4**
passent au résultat suivant / précédent. Le bouton **en onglet** met la recherche dans
un onglet de l'éditeur (plus de place), **en bas** la remet en bas
(Préférences → Claude pour choisir où elle s'ouvre).

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
- Avec ⏵, le remplacement s'affiche ; en regex `$1`, `$2` reprennent les groupes :
  `fields\.(\w+)\(` → `Fields.$1(`.
- **Retour à la ligne** dans le remplacement : **Maj+Entrée** dans le champ, ou `\n` en regex
  (`\t` tabulation, `\\` barre oblique inverse). Pour changer les `\n` écrits dans le texte en
  vrais retours à la ligne : regex, chercher `\\n`, remplacer par `\n`. Aussi dans Ctrl+H.
- **Garder la casse** : chat → chien, Chat → Chien, CHAT → CHIEN.
- **×** sur un résultat (ou un fichier) l'écarte : il ne sera pas remplacé.
- **TODO / FIXME** (à droite de la barre, ou Outils → TODO / FIXME du projet) : tous les
  `TODO`, `FIXME`, `XXX` et `HACK` du projet, rangés par fichier.

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

- **Ctrl+Shift+G** (ou clic sur la branche dans la barre d'état) : vue Git. En haut le dépôt (en grand : le
  nom est cliquable quand le projet en a plusieurs), la branche (**🔀 nom ▾** : changer de branche ou en créer une),
  pull / push / fetch et le message du commit (Ctrl+Entrée) ; en dessous le graphe, dont les
  premières lignes sont les modifications à indexer / indexées.
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
- **Modifier le dernier commit** (case sous le message, ou menu de la branche) : le message du
  dernier commit revient dans la zone ; le bouton le remplace (avec les fichiers indexés en plus).
  S'il était déjà poussé, il faudra un push --force-with-lease.
- **↕ Synchroniser** : Pull, **Pull --rebase** (tes commits rejoués par-dessus, sans commit de
  merge), Push, **Push --force-with-lease** (après un rebase ou un amend : refusé si quelqu'un a
  poussé entre-temps), **Pousser les tags**, Fetch. Quand un push est refusé, le message d'erreur
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
    pour ✨ et ✂ dans ce projet (langue, préfixes, longueur…). À commiter pour le partager.
    Sans ce fichier, Claude prend la section « Commits » du CLAUDE.md du projet s'il y en a une.
- **Historique du fichier** (clic droit dans l'éditeur, menu Git) : tous ses commits, à travers
  ses renommages. **Historique de la ligne** (Ctrl+Alt+B) prend aussi plusieurs lignes sélectionnées.

### Découper un commit

Clic droit sur un commit → **Découper ce commit en plusieurs…**, ou dans le **rebase
interactif** (clic droit → « Rebase interactif à partir d'ici… »), l'action
**Découper en plusieurs commits** sur l'un des commits.

1. Coche les blocs qui vont dans la première partie — ou **clique sur des lignes + / −** pour n'en
   prendre qu'une partie (comme « e » dans `git add -p`) : une ligne + non prise n'est pas
   ajoutée, une ligne − non prise reste dans le fichier.
2. Écris le message, puis **Valider la partie 1**.
3. Recommence avec le reste ; **Terminer** rejoue les commits suivants.
   **Annuler le découpage** remet tout exactement comme avant.

## Terminal

- **Ctrl+`** (ou le bouton **>_ Terminal** de la barre d'état) : le terminal en bas.
- **Ctrl+Shift+`** (ou clic sur l'icône Terminal en bas de la barre latérale) : un terminal **dans un onglet** de
  l'éditeur. Comme tout onglet, il se glisse sur le bord droit ou bas de l'éditeur pour être à
  côté d'un fichier (ou clic droit sur l'onglet → Diviser à droite / en bas).
- **En onglet** dans le panneau du bas y envoie le terminal courant, **en bas** le ramène.
- Toutes les vues (Recherche, Git, GitHub, Docker, Claude, Débogage, Terminal) ont à droite de
  leur en-tête les mêmes boutons, dans le même ordre : **⟳** relire, **en onglet / en bas**
  (une petite fenêtre : avec un onglet en haut, ou une bande en bas), **⋯** les autres actions et
  options. Les boutons de disposition se ressemblent tous : les quatre coins agrandissent un
  panneau dans sa vue (Échap pour revenir), une fenêtre avec une colonne à droite affiche ou
  masque un panneau latéral (la revue de Claude), pleine quand il est affiché. Git, les Préférences et le découpage d'un commit
  sont des onglets comme les autres : Échap revient à l'onglet d'avant, Ctrl+W les ferme.

## SQL (PostgreSQL)

**Outils → SQL** (ou « sql » dans la palette) : un onglet pour interroger une base PostgreSQL
avec la commande `psql` (le client seul suffit).

- En haut, la **connexion** : le nom d'une base (`odoo17`), `host=… user=… dbname=…` ou
  `postgresql://…`. **Bases ▾** liste celles du serveur. Le mot de passe va dans `~/.pgpass` (ou
  la variable `PGPASSWORD`) plutôt que dans le champ.
- La requête, colorée ; **▶ Exécuter** ou **Ctrl+Entrée**. Le résultat s'affiche en tableau
  (colonnes redimensionnables, `NULL` en gris, clic sur une valeur pour la copier, **Copier
  (tableur)** pour tout coller dans un tableur), avec le nombre de lignes et le temps. **■ Arrêter**
  interrompt une requête trop longue.
- **⏱ EXPLAIN ANALYZE** : le plan de la requête avec ses vrais temps. Elle est exécutée dans une
  transaction annulée ensuite : un `UPDATE` ou un `DELETE` ne change rien. Les étapes les plus
  lentes sont en rouge (orange pour les suivantes).
- La connexion et la requête sont gardées avec le projet.

## Docker

**Docker** dans la palette (« docker ») ou **Outils → Docker (conteneurs et logs)** : la liste des conteneurs à
gauche, leurs logs à droite. **En onglet** la met en grand dans l'éditeur, **en bas** la ramène en bas. La liste
est relue toutes les quelques secondes.

- **Liste** : les conteneurs sont groupés par projet `docker compose`. Le rond à côté du nom est
  vert quand il tourne, jaune s'il redémarre, est en pause ou n'est pas *healthy*, rouge s'il
  est mort, gris s'il est arrêté.
- **■ / ▶** à côté d'un conteneur : l'arrêter ou le démarrer. Pour un projet, **■ tout** arrête
  tous ses conteneurs, du bas de la liste vers le haut ; **▶ tout** démarre les arrêtés dans
  l'ordre de la liste, chacun une fois le précédent prêt (*healthy*). La poignée à points à gauche
  se glisse pour changer cet ordre.
- **⬆ compose up** (ou **⬆** sur un projet) : `docker compose up -d` dans le panneau du bas. Crée et démarre
  aussi les services qui n'ont pas encore de conteneur, et recrée ceux dont la configuration a changé.
- **Logs** : clique sur un conteneur, ses logs s'affichent à droite. S'il redémarre, les logs
  repartent tout seuls. Le bouton en haut à droite des logs les agrandit (la liste reprend sa place
  en cliquant à nouveau).
  - Dans sa barre : **⟳ Redémarrer**, **■ Arrêter** (ou **▶ Démarrer**), **>_ Terminal** (un shell
    dans le conteneur, `docker exec`, dans le panneau du bas).
  - **Attaché** : un conteneur qui attend une saisie (ipdb…) reçoit ce que tu tapes, et
    **Ctrl+C** l'interrompt. Sinon les logs sont en lecture seule.
- **🧪 Tests** : lance des tests Odoo dans le conteneur. Les tags se tapent dans le champ
  (`:MaClasse`, `/mon_module`, `:MaClasse.test_x`) ; **▾** reprend ceux déjà lancés.

## Débogueur

Comme celui de VS Code, pour Python et Odoo, sur cet ordinateur ou dans un conteneur Docker.
**Débogage** dans la palette, ou l'onglet **Débogage** du panneau du bas.

- **Point d'arrêt** : clic à gauche des numéros de ligne d'un fichier Python (ou **F9**). Un rond
  rouge. Dans la liste **Points d'arrêt**, une **condition** (`self.id == 7`) ne s'arrête que
  quand elle est vraie. Ils suivent les lignes quand le fichier change et sont gardés avec le projet.
- **F5** démarre. En haut de la vue : **Où** (cet ordinateur ou un conteneur), **Lancer** ou
  **S'attacher**, et la commande.
  - **Lancer** ici : la commande (par défaut l'`odoo-bin` du dossier, `--workers 0`) tourne dans
    le panneau du bas, avec ses logs. Il faut `debugpy` dans ce Python : sinon, **Installer
    debugpy**.
  - **Lancer** dans un conteneur : la commande tourne dans le conteneur, à côté de l'Odoo qui y
    tourne (par défaut `odoo --workers 0 --http-port 8070`). Florm parle au débogueur par
    `docker exec` : **aucun port à publier**. Les fichiers montés s'ouvrent ici ; ceux de l'image
    (le code d'Odoo installé dans le conteneur) s'ouvrent en lecture seule.
  - **S'attacher** : à un Odoo lancé avec debugpy devant la commande habituelle, avec
    `--workers 0` (sinon les requêtes passent par d'autres processus, que le débogueur ne suit pas) :
    `python3 -m debugpy --listen 5678 ./odoo-bin --addons-path=… -d ma_base --workers 0`.
    Dans un conteneur : `python3 -m debugpy --listen 0.0.0.0:5678 /usr/bin/odoo … --workers 0`
    dans la `command:` du `docker-compose.yml`, debugpy installé dans l'image, et **Où** sur ce
    conteneur.
    **Tout est automatique, quel que soit le mode** : dès qu'un point d'arrêt est posé, Florm
    attend qu'un programme écoute avec debugpy et s'y attache, sans F5 ni rien à choisir ; quand le
    programme s'arrête, il attend le lancement suivant.
    Lance Odoo comme tu veux (terminal de Florm, le tien, `docker compose up`) : **Copier la
    commande** donne la ligne à lancer. **■ Arrêter l'attente** (ou Maj+F5) arrête d'attendre,
    F5 reprend.
- **En pause** : la ligne est surlignée en jaune, la fenêtre passe devant et le débogage s'ouvre dans le panneau du bas, sous le fichier (sauf s'il est dans l'autre moitié d'un éditeur divisé). À gauche les
  **Variables** (clic pour déplier `self`, un recordset…), l'**Espion** (des expressions
  réévaluées à chaque arrêt), la **Pile d'appels** (clic sur un appel : sa ligne et ses
  variables, en vert). Le survol d'un nom montre sa valeur (`self.partner_id.name`).
- **Console** (à droite) : du Python évalué dans le cadre en pause,
  `self.env['res.partner'].search([])`, `x = 2`. ↑ ↓ reprennent les précédents.
- **F10** ligne suivante, **F11** entrer dans l'appel, **Maj+F11** finir la fonction, **F5**
  continuer, **F6** pause, **Maj+F5** arrêter, **Ctrl+Maj+F5** redémarrer.
- **Tests Odoo** : clic droit sur le **▶** d'un test dans la marge (ou **Alt+F6**) le lance sous le
  débogueur, dans le conteneur choisi (sinon celui des tests).
- **Exceptions** : s'arrêter sur celles que rien n'intercepte, sur toutes (Odoo en lève beaucoup),
  ou quand une sort du code du projet. **Mon code seulement** n'entre pas dans le code d'Odoo ni
  des bibliothèques.

## Modèles

**Ctrl+Alt+N** (ou Outils → Insérer un modèle) : les modèles du type du fichier courant sont
proposés en premier. Florm n'en livre aucun : ajoutez les vôtres (voir plus bas).

Les variables sont remplies d'après le fichier : dans `estate/views/estate_property_views.xml`,
`${model}` = `estate.property`, `${model_}` = `estate_property`, `${Model}` = `EstateProperty`,
`${name}` = `Estate Property`, `${module}` = `estate`. `$0` marque la place du curseur.

Pour ajouter les tiens : **Outils → Dossier des modèles**, un fichier par modèle
(son nom = le nom du modèle, son extension = le type de fichier). Un fichier qui porte le nom
d'un modèle intégré le remplace.

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

## GitHub

Florm passe par la commande `gh` : pas de token à créer, il suffit de s'être connecté une fois
avec `gh auth login` (connexion par le navigateur). Sans `gh`, la vue explique comment l'installer
(le système est détecté) et reprend tout seul dès qu'elle est trouvée.

### Pull requests

**Ctrl+Alt+R** (ou Git → Pull requests GitHub, **⋯ → Pull requests GitHub** dans la vue Git, la palette).

- À gauche, les PR du dépôt (ouvertes, fusionnées, fermées, les tiennes), filtrables.
- **Conversation** : la description, les **commentaires sur le code** (fichier:ligne
  cliquable, le code commenté, les réponses) puis la discussion et les revues.
- **Appliquer la suggestion** : un commentaire avec un bloc `suggestion` remplace les lignes
  dans ton fichier et l'enregistre (Ctrl+Z dans le fichier pour annuler). Il faut d'abord
  **⤵ Checkout** pour avoir la branche de la PR.
- Branche de la PR active : les commentaires non résolus sont **dans l'éditeur**, une petite
  bulle dans la marge, le texte au survol.
- **Commits** : un clic montre les changements de ce commit seul ; **Fichiers** : tout le
  diff de la PR, les conversations **sous les lignes commentées** comme sur GitHub (répondre,
  résoudre, appliquer une suggestion), et **double-clic sur une ligne** pour la commenter.
- Une case avant chaque fichier le marque **vu** : il se replie et GitHub s'en souvient pour
  ta revue (« 3/10 vu(s) » en haut).
- En haut de la conversation : les **checks** (ceux en échec toujours visibles, un clic pour
  tous ; **Logs** ouvre le log d'un job GitHub Actions dans un onglet, **↻ Relancer les
  échecs**), puis si la PR peut être fusionnée (conflits, en retard, approbations ou checks
  manquants) et les boutons : **Fusionner** (commit de fusion, squash ou rebase, supprimer la
  branche), **⟳ Mettre à jour la branche** (merge ou rebase de la base), **Prête pour la
  revue** / **Repasser en brouillon**, **Fermer** / **Rouvrir**. Fusionner, fermer et rebaser
  demandent confirmation.
- Sous le message : **✔ Approuver** ou **✖ Demander des changements** (le message dit quoi).
- À droite : relecteurs, assignés (`me` pour toi), étiquettes et jalon, à ajouter ou retirer.
- **✏** à côté du titre : changer le titre, la description ou la branche de base.
- Sous chaque message, ses **réactions** (un clic donne ou retire la tienne, 🙂 pour en
  ajouter) ; tes commentaires ont **✏** (modifier) et **🗑** (supprimer).
- Clic droit sur une PR de la liste (ou **⋯** dans l'en-tête) : ouvrir, ouvrir sur GitHub,
  checkout, copier le lien ou le numéro.

## Claude

**Ctrl+Alt+C** (ou Outils → ✳ Claude, la palette) : une conversation avec Claude Code, dans le
panneau du bas (**en onglet** pour lui donner tout un onglet de l'éditeur, **en bas** pour l'y remettre). Florm utilise la commande `claude` et ta connexion : pas de clé à configurer.

- En haut : le dépôt, le modèle, et ce que Claude a le droit de faire (**Modifie les fichiers**,
  **Plan seulement**, **Tout autoriser** pour qu'il lance aussi des commandes).
  Le modèle et le mode se changent en pleine conversation : tout de suite si Claude travaille,
  sinon au prochain message, la conversation gardée. **Maj+Tab** dans le champ : mode suivant,
  comme dans Claude Code.
- **Fin de réponse** : si la conversation n'est pas affichée, son onglet (ou, dans le panneau
  du bas, l'onglet Claude et son icône de la barre latérale) clignote quelques secondes (tout l'onglet s'allume) puis
  reste vert jusqu'à ce que tu la regardes, et un son est joué. **Préférences → Claude** :
  clignoter ou non, et le son (Carillon, Clochette, Goutte, Bip, Accord, Micro-ondes, ou aucun ; ▶ pour
  écouter).
- **/** au début du champ : les commandes de Claude Code (`/usage`, `/context`, `/compact`,
  `/clear`, `/model`, tes skills…), **Tab** ou un clic complète. Elles partent seules, sans le
  contexte. **/btw question** pose une question à côté, même pendant que Claude travaille : la
  réponse s'affiche au-dessus du champ et n'entre pas dans la conversation. **/status** : version,
  modèle, session, dossier. Celles de l'interface terminal seulement (`/doctor`…) s'ouvrent dans
  un terminal en bas.
- **🔒 Permissions → Règles** : en plus des cases, des règles précises autorisées ou interdites,
  comme dans `/permissions` de Claude Code : `Bash(git *)`, `Edit(src/**)`, `Read(./.env)`,
  `WebFetch(domain:github.com)`, `mcp__github` (les outils d'un serveur MCP). Une règle qui
  autorise une partie d'un outil non coché (`Bash(git *)` sans « Lancer des commandes ») autorise
  cette partie. Un changement s'applique au prochain message, la conversation gardée.
- **⋯** dans la barre : les **serveurs MCP** (état, ajouter une URL ou une commande, pour ce
  projet, partagé dans `.mcp.json` ou pour tous tes projets, retirer), les **sous-agents**
  (ouvrir, en créer un), les **hooks** des fichiers de réglages, la **mémoire** (CLAUDE.md) et
  les **dossiers en plus** que Claude peut utiliser (`--add-dir`). Aussi en commandes :
  **/mcp**, **/agents**, **/permissions**, **/memory**, **/add-dir chemin**.
- **# au début d'un message** : la ligne n'est pas envoyée, elle est retenue dans le CLAUDE.md du
  projet ou dans ta mémoire perso (`~/.claude/CLAUDE.md`), au choix.
- **@ dans le champ** : les fichiers du projet qui correspondent à ce qui est tapé ; ↑ ↓ pour
  choisir, **Tab** ou **Entrée** complète, et le fichier part avec le message.
- **🌿** (worktree) : une nouvelle conversation sur une branche à elle, dans son propre dossier (un
  worktree git), pour faire travailler plusieurs Claude en même temps sans qu'ils se gênent.
  Chaque conversation a son onglet et continue de travailler même quand elle n'est pas affichée.
- **Options ▾**, à côté de « + Contexte », regroupe ce qui part avec chaque message :
- **Fichier ouvert et sélection** : Claude sait quel fichier tu édites et ce que tu as
  sélectionné.
- **Coller** : un long texte collé (plus de 6 lignes ou 500 caractères) devient une puce « Texte
  collé (N lignes) » au-dessus du champ, envoyée en entier avec le message ; le champ reste court.
  Un clic sur la puce montre tout le texte en dessous, modifiable ; un autre clic le replie.
  Le texte collé peut partir seul : **Envoyer** marche même si le champ est vide.
  Une image copiée (capture d'écran) se colle avec Ctrl+V et part avec le message, réduite à
  1568 pixels pour coûter moins de tokens. Une puce se retire avec son ×.
- **Questions d'abord** : si la demande laisse des choix ouverts, Claude ne code rien et pose
  1 à 3 questions avec des options (la conseillée cochée d'avance, **Autre** pour écrire la
  tienne) ; **Répondre** les envoie et il continue. Dans la file de tâches, la tâche attend tes
  réponses avant la suivante.
- **Modèle d'abord** : avant qu'un message parte, Haiku regarde si la demande demande un autre
  modèle (Opus pour un travail difficile, Haiku pour une petite tâche) et te le propose. Tu
  choisis **Passer à…** ou **Garder…** ; **Annuler** remet le message dans le champ. Les tâches de
  la file sont vérifiées aussi (sauf celles qui ont leur propre modèle) ; **Annuler** là-bas remet
  la tâche en attente et met la file en pause. Un message écrit pendant la vérification part
  dans la file, après la réponse.
- **Avertissements** : le chat signale une fois quand la conversation remplit plus de 60 % du
  contexte (`/compact` la résume), et quand Claude ouvre une image (elle coûte beaucoup de tokens à
  chaque message suivant). La taille du contexte joint à un message s'affiche dans la barre d'état.
  Au-dessus de 60 %, une barre propose **/compact** ou **Nouvelle conversation**. Les options du
  menu restent avec la conversation quand tu la reprends.
- **Définitions citées** : les classes et fonctions du projet nommées dans le message
  (`SaleOrder`, `_compute_total`, `foo()`…) partent avec lui, leurs premières lignes comprises,
  trouvées dans l'index de Florm : Claude n'a pas à les chercher (moins de tokens). Un nom
  défini à plus de deux endroits n'est pas donné.
- **Donner des lignes ou des fichiers à Claude** : au survol d'un numéro de ligne, clic sur le
  **✳** à sa gauche (la ligne, ou les lignes sélectionnées) ; clic droit dans l'éditeur →
  **Demander à Claude à propos de la sélection…** (**Ctrl+Alt+K**) ou **Ajouter … au contexte** ;
  clic droit sur un fichier ou un dossier de l'arborescence → **Ajouter au contexte de Claude**.
  Ils apparaissent en pastilles au-dessus du message (clic : retirer) et partent avec lui.
  **+ Contexte → Un commit** (ou clic droit sur un commit dans la vue Git → **Donner ce commit
  à Claude**) : son message et son diff, pour partir de ce qu'il fait (« fais pareil pour… »).
- À droite, **ce que Claude a changé** depuis ton premier message, fichier par fichier et bloc
  par bloc : **✔ Garder** (le bloc quitte la revue et reste dans le fichier) ou **↺ Rejeter**
  (il est retiré du fichier). Tes propres changements d'avant ne sont pas dedans.
- **Question par question** : le **▾** devant un de tes messages replie la réponse à cette
  question (il reste une ligne « N étapes · N fichiers modifiés ») ; clic droit sur le ▾ : tout
  replier sauf la dernière, ou tout déplier. **±** sur le message montre à droite les seuls
  changements de fichiers de cette question (**× Revue complète** pour revenir à la revue).
- **↶ Revenir en arrière** : avant chaque message, Florm garde l'état des fichiers (dans git, sans
  commit ni rien d'indexé). **↶** sur un message (ou **/rewind** pour le dernier) propose de
  remettre **les fichiers et la conversation** comme avant ce message, **la conversation
  seulement** ou **les fichiers seulement**. Ce qui a changé depuis est perdu ; le message revient
  dans le champ pour être réécrit.
- **■ Arrêter** coupe la réponse en cours ; la conversation continue.
- **Pendant qu'il travaille**, tu choisis où part ton message :
  - **Envoyer** (Ctrl+Entrée) : tout de suite. Claude le lit entre deux étapes et en tient compte dans
    le travail en cours (« utilise plutôt tel fichier », « ajoute aussi un test »…).
  - **⏭ Après** (Ctrl+Maj+Entrée) : le message attend dans la file et part tout seul quand Claude a fini.
    Une liste reste un seul message (pas de découpage) ; la file ne fait pas de commit pour lui.
- **File de tâches** : écris plusieurs demandes en liste (`- …`, `1. …`) et clique **+ File**
  au lieu d'Envoyer : chaque élément devient une tâche (la phrase avant la liste va avec
  chacune). **Lancer** les envoie une à une dans la même conversation, chacune quand Claude a
  fini la précédente ; **Commit après chaque tâche** fait un commit (message écrit par Claude)
  entre deux. Une erreur ou **■ Arrêter** met la file en pause ; une tâche en attente se
  modifie ou se remonte (↑) ; toute tâche sauf celle en cours se retire (×), et le **×** de la
  file la ferme.
  Pour moins de tokens : **Conversation neuve par tâche** (Claude ne relit pas tout l'historique,
  la conclusion de la tâche d'avant lui est donnée) et un **modèle pour la file** (Haiku ou
  Sonnet pour des petites tâches, le modèle de la conversation sinon), qu'une tâche en attente
  peut remplacer par le sien (menu à côté de la tâche).

**✨ à côté du message de commit** (vue Git) : Claude propose le message d'après les changements
indexés (sinon tous), dans le style des derniers commits du dépôt et d'après ta première demande
dans l'onglet Claude.

**✂ à côté de ✨** : au lieu d'un seul commit, Claude découpe tous les changements (nouveaux
fichiers compris) en plusieurs commits par thème, chacun avec son message. Le plan remplace la
liste de la vue Git : relis et modifie les messages, déplace un bloc d'un commit à l'autre (**→**),
laisse-le hors des commits, change l'ordre (**↑**). Rien n'est commité avant **Commiter les N
commits** ; **Annuler** laisse tout comme avant.

## Markdown

Au-dessus du texte d'un fichier `.md`, une **barre de mise en forme** : **B** gras, *I* italique,
~~S~~ barré, `c` code, Lien, titres H1 à H3, listes (• et 1.), case à cocher ☐, citation ❝, bloc
de code, ligne horizontale `――` et tableau `▦`. Le bouton met la syntaxe autour de la sélection, ou
sur les lignes sélectionnées ; un second clic enlève le préfixe. Le texte reste modifiable à la main.
Pour masquer la barre : `markdown_toolbar = false` dans `settings.toml`. L'aperçu rendu reste
Ctrl+Shift+V (onglet) ou Ctrl+K V (à côté).

## Apparence

**Préférences → Apparence** : langue (Français / English), thème (aperçus cliquables : Florm et
Florm Light, et Florm Monokai, One Dark, Dracula, GitHub Light d'après les thèmes habituels des autres
éditeurs), couleur d'accentuation,
**texte adouci** (texte moins « gras », utile sur les thèmes clairs), polices de l'éditeur et
de l'interface, minimap, fil d'Ariane, **barre de menus** (masquée, les menus restent sous **☰**
en bas à gauche).

**Couleurs du thème → Créer un thème à partir de « … »** : toutes les couleurs de l'éditeur
(fond, texte, accent, curseur, sélection, marge…) et du code (commentaires, mots-clés, chaînes,
fonctions, classes, balises…, en italique ou non), appliquées tout de suite. **Enregistrer le
thème** l'écrit dans le dossier `themes` de la configuration ; **Modifier ce thème** /
**Supprimer ce thème** quand c'est un des tiens.

**GitHub, Claude et le débogueur se désactivent** : un interrupteur en haut de leur section des
préférences (Pull requests, Claude, Débogueur). Désactivés, leurs vues, commandes, menus, icônes,
raccourcis et boutons disparaissent (✨ et ✂ de Git, revue IA, colonne ✳ et points d'arrêt de la
marge, PR liées dans la vue Git…), et `gh` n'est plus appelé. Le même interrupteur les ramène,
sans redémarrer.
