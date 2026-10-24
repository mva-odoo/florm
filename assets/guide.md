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
liste ses symboles, une classe ou une fonction y emmène.

## Barre latérale

- **Ctrl+Shift+E** (ou un clic sur un fichier) : le clavier va dans l'arborescence. **↑ ↓** pour se
  déplacer, **→** ouvre un dossier, **←** le referme (ou remonte au dossier parent), **Espace**
  affiche le fichier en gardant le clavier dans l'arborescence, **Entrée** l'ouvre et passe à
  l'éditeur, **F2** renomme, **Suppr** supprime, **Échap** revient à l'éditeur.
- En défilant, les dossiers qui contiennent le haut de la liste restent figés en haut ; un clic
  remonte au dossier.
- **Double-clic sur le bord** entre la barre et l'éditeur : la barre se masque. Un clic sur la fine
  bande à gauche (ou **Ctrl+K Ctrl+B**) la réaffiche. Le bord se glisse pour changer la largeur.
- Sur le dossier d'un dépôt git : sa branche, la branche distante suivie (↑ à pousser, ↓ à
  récupérer) et le nombre de fichiers modifiés ; le survol montre les remotes
  (désactivable dans Préférences → Git).
- Les icônes en bas (Préférences, Recherche, Terminal, Claude…) : **▾** au bout de la ligne choisit
  celles qui sont affichées, et les met en haut de la barre latérale si on le souhaite. Une vue masquée reste dans la palette (Ctrl+Shift+P) et les menus.

## Rechercher dans le dossier (Ctrl+Shift+F)

Entrée lance la recherche. Les résultats sont colorés comme dans l'éditeur ; **F4 / Maj+F4**
passent au résultat suivant / précédent. **⇱** met la recherche dans un onglet de
l'éditeur (plus de place), **⇲** la remet en bas
(Préférences → Éditeur pour choisir où elle s'ouvre).

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

## Git

Florm passe par la commande `git` (votre configuration, vos identifiants). Sans `git`, la vue
explique comment l'installer (le système est détecté) et reprend toute seule dès qu'elle est trouvée.

- **Ctrl+Shift+G** (ou clic sur la branche dans la barre d'état) : vue Git. En haut la branche,
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
- **⇱** dans le panneau du bas y envoie le terminal courant, **⇲** le ramène.
- Toutes les vues (Recherche, Git, GitHub, Docker, Claude, Débogage, Terminal) ont à droite de
  leur en-tête les mêmes boutons, dans le même ordre : **⟳** relire, **⇱ / ⇲** onglet ou panneau
  du bas, **⋯** les autres actions et options. Git, les Préférences et le découpage d'un commit
  sont des onglets comme les autres : Échap revient à l'onglet d'avant, Ctrl+W les ferme.

## Docker

**Docker** dans la palette (« docker ») ou **Outils → Docker (conteneurs et logs)** : la liste des conteneurs à
gauche, leurs logs à droite. **⇱** la met en grand dans l'éditeur, **⇲** la ramène en bas. La liste
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
**Débogage** dans la palette, ou l'icône de l'insecte en bas de la barre latérale.

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
  - **S'attacher** : à un Odoo déjà lancé avec `python3 -m debugpy --listen 5678 ./odoo-bin …`
    (dans un conteneur : `--listen 0.0.0.0:5678` dans la `command:` du `docker-compose.yml`).
- **En pause** : la ligne est surlignée en jaune, la fenêtre passe devant. À gauche les
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

## Claude

**Ctrl+Alt+C** (ou Outils → ✳ Claude, la palette) : une conversation avec Claude Code, dans le
panneau du bas (⇱ pour lui donner tout un onglet de l'éditeur, ⇲ pour l'y remettre). Florm utilise la commande `claude` et ta connexion : pas de clé à configurer.

- En haut : le dépôt, le modèle, et ce que Claude a le droit de faire (**Modifie les fichiers**,
  **Plan seulement**, **Tout autoriser** pour qu'il lance aussi des commandes).
- **Options ▾**, à côté de « + Contexte », regroupe ce qui part avec chaque message :
- **Fichier ouvert et sélection** : Claude sait quel fichier tu édites et ce que tu as
  sélectionné.
- **Questions d'abord** : si la demande laisse des choix ouverts, Claude ne code rien et pose
  1 à 3 questions avec des options (la conseillée cochée d'avance, **Autre** pour écrire la
  tienne) ; **Répondre** les envoie et il continue. Dans la file de tâches, la tâche attend tes
  réponses avant la suivante.
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
- **■ Arrêter** coupe la réponse en cours ; la conversation continue.
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

## Apparence

**Préférences → Apparence** : langue (Français / English), thème (aperçus cliquables), couleur d'accentuation,
**texte adouci** (texte moins « gras », utile sur les thèmes clairs), polices de l'éditeur et
de l'interface, minimap, fil d'Ariane, **barre de menus** (masquée, les menus restent sous **☰**
en bas à gauche).

**Couleurs du thème → Créer un thème à partir de « … »** : toutes les couleurs de l'éditeur
(fond, texte, accent, curseur, sélection, marge…) et du code (commentaires, mots-clés, chaînes,
fonctions, classes, balises…, en italique ou non), appliquées tout de suite. **Enregistrer le
thème** l'écrit dans le dossier `themes` de la configuration ; **Modifier ce thème** /
**Supprimer ce thème** quand c'est un des tiens.
