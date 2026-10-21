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

Plusieurs filtres se séparent par des virgules.

| Filtre « Fichiers » | Cherche dans… |
|---|---|
| `*.py` | tous les fichiers Python |
| `*.py, *.xml` | les fichiers Python et XML |
| `models/*.py` | les `.py` de tout dossier `models` (où qu'il soit) et de ses sous-dossiers |
| `tests` | tout dossier (ou fichier) nommé `tests` |
| `./src/*.rs` | à partir de la racine du projet seulement |
| `views/**/*.xml` | les `.xml` sous n'importe quel dossier `views` |

| Filtre « Exclure » | Ignore… |
|---|---|
| `node_modules, *.min.js` | ces dossiers et ces fichiers |
| `tests, i18n` | les dossiers de tests et de traductions |

Le `.gitignore` est respecté.

### Texte, regex, remplacement

- **Aa** respecte la casse, **« »** cherche le mot entier, **.*** active les expressions régulières :
  `def _compute_\w+` trouve toutes les méthodes de calcul, `^\s*import ` les imports.
- Avec ⏵, le remplacement s'affiche ; en regex `$1`, `$2` reprennent les groupes :
  `fields\.(\w+)\(` → `Fields.$1(`.
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

## Apparence

**Préférences → Apparence** : thème (aperçus cliquables), couleur d'accentuation,
**texte adouci** (texte moins « gras », utile sur les thèmes clairs), polices de l'éditeur et
de l'interface, minimap, fil d'Ariane, **barre de menus** (masquée, les menus restent sous **☰**
en bas à gauche).
