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

Le fil d'Ariane au-dessus du code est cliquable : un dossier montre son contenu, le fichier
liste ses symboles, une classe ou une fonction y emmène.

## Barre latérale

- **Ctrl+Shift+E** : le clavier va dans l'arborescence. **↑ ↓** pour se déplacer, **→** ouvre un
  dossier, **←** le referme (ou remonte au dossier parent), **Entrée** ouvre le fichier,
  **F2** renomme, **Suppr** supprime, **Échap** revient à l'éditeur.
- **Double-clic sur le bord** entre la barre et l'éditeur : la barre se masque. Un clic sur la fine
  bande à gauche (ou **Ctrl+K Ctrl+B**) la réaffiche. Le bord se glisse pour changer la largeur.
- Sur le dossier d'un dépôt git : sa branche, la branche distante suivie (↑ à pousser, ↓ à
  récupérer) et le nombre de fichiers modifiés ; le survol montre les remotes
  (désactivable dans Préférences → Git).

## Rechercher dans le dossier (Ctrl+Shift+F)

Entrée lance la recherche. Les résultats sont colorés comme dans l'éditeur ; **F4 / Maj+F4**
passent au résultat suivant / précédent. **⇱ Onglet** met la recherche dans un onglet de
l'éditeur (plus de place), **⇲ Panneau du bas** la remet en bas
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
- **Ctrl+Shift+`** (ou clic droit sur **>_ Terminal**) : un terminal **dans un onglet** de
  l'éditeur. Comme tout onglet, il se glisse sur le bord droit ou bas de l'éditeur pour être à
  côté d'un fichier (ou clic droit sur l'onglet → Diviser à droite / en bas).
- **⇱ Onglet** dans le panneau du bas y envoie le terminal courant, **⇲ Panneau du bas** le
  ramène.

## Modèles (vues Odoo, menus…)

**Ctrl+Alt+N** (ou Outils → Insérer un modèle) : vue formulaire, liste, recherche, héritage de
vue, action, menu, modèle Python, champ calculé, `__manifest__.py`, droits d'accès…
Les modèles du type du fichier courant sont proposés en premier.

Les variables sont remplies d'après le fichier : dans `estate/views/estate_property_views.xml`,
`${model}` = `estate.property`, `${model_}` = `estate_property`, `${Model}` = `EstateProperty`,
`${name}` = `Estate Property`, `${module}` = `estate`. `$0` marque la place du curseur.

Pour ajouter les tiens : **Outils → Dossier des modèles**, un fichier par modèle
(son nom = le nom du modèle, son extension = le type de fichier). Un fichier qui porte le nom
d'un modèle intégré le remplace.

**Déclencheurs**, comme les snippets de Sublime : taper `ipdb` dans un fichier Python le
propose dans les suggestions, **Tab** (ou Entrée) le remplace par
`import ipdb; ipdb.set_trace()`. Aussi `pdb`, et `odoo_` pour tous les modèles Odoo
(`odoo_form`, `odoo_model`, `odoo_menu`…). Un modèle perso dont le nom est un seul mot
(`log.py`, `ifmain.py`) a ce mot pour déclencheur.

**Outils → Nouveau modèle…** (ou la palette) : tape le déclencheur (`log`), Florm crée
`log.py` (extension du fichier courant ; `log.xml` pour un autre type) dans le dossier des
modèles et l'ouvre. S'il y avait une sélection, elle devient le texte du modèle.

## Pull requests GitHub

**Ctrl+Alt+R** (ou Git → Pull requests GitHub, le bouton **⇄ PR** de la vue Git, la palette). Florm passe par
la commande `gh` : pas de token à créer, il suffit de s'être connecté une fois avec
`gh auth login` (connexion par le navigateur).

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
panneau du bas (⇱ Onglet pour lui donner tout un onglet de l'éditeur, ⇲ pour l'y remettre). Florm utilise la commande `claude` et ta connexion : pas de clé à configurer.

- En haut : le dépôt, le modèle, et ce que Claude a le droit de faire (**Modifie les fichiers**,
  **Plan seulement**, **Tout autoriser** pour qu'il lance aussi des commandes).
- **Fichier ouvert et sélection** : Claude sait quel fichier tu édites et ce que tu as
  sélectionné.
- **Donner des lignes ou des fichiers à Claude** : au survol d'un numéro de ligne, clic sur le
  **✳** à sa gauche (la ligne, ou les lignes sélectionnées) ; clic droit dans l'éditeur →
  **Demander à Claude à propos de la sélection…** (**Ctrl+Alt+K**) ou **Ajouter … au contexte** ;
  clic droit sur un fichier ou un dossier de l'arborescence → **Ajouter au contexte de Claude**.
  Ils apparaissent en pastilles au-dessus du message (clic : retirer) et partent avec lui.
- À droite, **ce que Claude a changé** depuis ton premier message, fichier par fichier et bloc
  par bloc : **✔ Garder** (le bloc quitte la revue et reste dans le fichier) ou **↺ Rejeter**
  (il est retiré du fichier). Tes propres changements d'avant ne sont pas dedans.
- **■ Arrêter** coupe la réponse en cours ; la conversation continue.

**✨ à côté du message de commit** (vue Git) : Claude propose le message d'après les changements
indexés (sinon tous), dans le style des derniers commits du dépôt et d'après ta première demande
dans l'onglet Claude.

## Apparence

**Préférences → Apparence** : thème (aperçus cliquables), couleur d'accentuation,
**texte adouci** (texte moins « gras », utile sur les thèmes clairs), polices de l'éditeur et
de l'interface, minimap, fil d'Ariane, **barre de menus** (masquée, les menus restent sous **☰**
en bas à gauche).
