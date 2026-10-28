# CLAUDE.md

Florm : éditeur de texte en Rust (egui/eframe 0.29), inspiré de Sublime Text, un seul binaire.
Intègre Git, GitHub (pull requests), Claude Code, Docker, terminal, LSP, linters, débogueur (DAP),
tests Odoo, connexions SSH/WSL, plugins.

## Commandes

```bash
cargo build                 # debug (les dépendances sont optimisées même en debug, voir Cargo.toml)
cargo run -- .              # lancer sur le dossier courant
cargo test                  # tests unitaires (#[cfg(test)] dans les modules)
cargo build --release       # LTO fat, long : seulement si nécessaire
scripts/build-windows.sh    # binaire Windows depuis Linux/WSL
```

- Ne pas lancer `cargo fmt` sur tout le projet : le code n'est pas formaté par rustfmt
  (lignes jusqu'à ~120 colonnes) et cela produirait un énorme diff.
- Les releases se font à la main : `gh workflow run release.yml -f version=x.y.z`. Ne pas en lancer sans demande.

## Architecture

- `src/main.rs` : démarrage, options de fenêtre, déclaration des modules.
- `src/app.rs` + `src/app/*.rs` : l'application egui (`impl App` réparti par thème : `claude_ui.rs`,
  `git_view.rs`, `pulls.rs`, `debugger.rs`, `panels.rs`, `prefs.rs`, `job_results.rs`…). Les plus
  gros ont des sous-modules dans un dossier du même nom (`claude_ui/`, `debugger/`, `panels/`) :
  un fichier par partie, avec `use super::*;`.
- `src/*.rs` hors `app/` : la logique sans UI (`git.rs`, `github.rs`, `claude.rs`, `dap.rs`, `lsp.rs`,
  `docker.rs`, `terminal.rs`, `document.rs` (buffer `ropey`), `highlight.rs` (`syntect`), `split.rs`…).
- Travail en arrière-plan : `src/jobs.rs`. Les tâches tournent sur des threads et renvoient un
  `JobResult` par un channel lu à chaque image ; le traitement est dans `src/app/job_results.rs`.
  Pour une nouvelle tâche : une variante de `JobResult`, son lancement, son traitement. Ne jamais
  bloquer le thread de l'UI (git, réseau, processus externes).
- Commandes : enum `Cmd` dans `src/commands.rs` (identifiants utilisés dans `[keys]` de settings.toml),
  raccourcis par défaut dans `src/keymap.rs`.
- Préférences : `Settings` dans `src/config.rs` (`~/.config/florm/settings.toml`, `%APPDATA%\florm`),
  page des préférences dans `src/app/prefs.rs`.
- Les outils externes (git, gh, claude, ruff, docker…) sont appelés comme processus ; aucune
  dépendance système autre qu'OpenGL. Préférer éviter d'ajouter des crates (taille du binaire).
- Multiplateforme : Linux (X11/Wayland, WSL), Windows, macOS. Le code spécifique passe par `#[cfg(...)]`
  et `src/os.rs`.

## Conventions

- **Textes de l'interface** : toujours bilingues avec les macros de `src/i18n.rs` :
  `t!("Fichier", "File")` (`&'static str`) et `tf!("{n} fichiers", "{n} files")` (`String`, variables capturées dans le texte ou arguments
  positionnels comme `format!`).
  Le français d'abord.
- **Commentaires et doc du code** : en anglais, courts, comme le code existant.
- **Guide intégré** : une fonctionnalité visible par l'utilisateur se documente dans
  `assets/guide.md` **et** `assets/guide.en.md` (inclus dans le binaire), et dans `README.md`
  si elle concerne l'installation, les outils externes ou la configuration.
- **Style des textes** : phrases simples et concrètes, sans jargon ; symboles déjà utilisés
  dans l'UI (⟳, ⇱ / ⇲, ⋯, ✨, ✂…) plutôt que de nouveaux libellés.

## Commits

En français, sujet `[TAG] Zone : ce qui change, du point de vue de l'utilisateur`, tags `[ADD]`
(nouveauté), `[IMP]` (amélioration), `[FIX]` (correction). Corps : un ou deux paragraphes qui
expliquent le comportement avant / après et le pourquoi. Exemple :

```
[FIX] Pull requests : choisir un dépôt marche aussi quand le dossier n'en est pas un
```

## Réponses (affichées dans Florm)

- Une liste de choses que l'utilisateur doit faire ou décider, et qu'il va cocher, s'écrit en tâches : `- [ ] …`.
- Une liste ordinaire (points, étapes, exemples) s'écrit en puces simples `- …`, sans case.
- Ne pas mettre de tâches `- [ ]` quand il n'y a rien à cocher.
