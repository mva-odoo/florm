# Plugins Florm

Un plugin est un dossier contenant un `plugin.toml`, placé dans :

- `~/.config/florm/plugins/<nom>/` (Linux/macOS) ou `%APPDATA%\florm\plugins\<nom>\` (Windows)
- `<projet>/.florm/plugins/<nom>/` (plugins propres à un projet)

Les commandes sont n'importe quel programme (Python, Node, Bash, Rust…), lancé avec
`sh -c` (ou `cmd /C` sous Windows) **depuis le dossier du plugin**.

## Commandes

```toml
[[commands]]
title  = "Mon outil"              # affiché dans la palette (Ctrl+Shift+P)
run    = "python3 outil.py"
input  = "selection"              # selection | buffer | none
output = "replace_selection"      # replace_selection | replace_buffer | insert | panel | new_tab | none
keys   = "ctrl+alt+x"             # raccourci optionnel
```

- **stdin** : le texte sélectionné (une exécution par curseur en multi-sélection),
  ou tout le fichier si `input = "buffer"` ou si rien n'est sélectionné.
- **stdout** : le résultat, utilisé selon `output`.
- Code de sortie ≠ 0 : l'erreur s'affiche dans le panneau Sortie.

Variables d'environnement disponibles :

| Variable | Contenu |
|---|---|
| `FLORM_FILE` | chemin du fichier courant |
| `FLORM_PROJECT` | dossier ouvert |
| `FLORM_LINE`, `FLORM_COLUMN` | position du curseur principal (1-based) |
| `FLORM_SYNTAX` | syntaxe (« Python », « XML »…) |
| `FLORM_PLUGIN_DIR` | dossier du plugin |

## Hooks

```toml
[[hooks]]
event = "on_save"          # lancé après chaque enregistrement
extensions = ["py"]        # vide = tous les fichiers
run = "ruff format \"$FLORM_FILE\""
reload = true              # recharger le fichier après (formateurs)
```

Après modification d'un `plugin.toml`, enregistrer le fichier dans Florm recharge
les plugins (ou palette → « Recharger les plugins »).
