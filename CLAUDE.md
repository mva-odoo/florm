# CLAUDE.md

Florm : éditeur de texte en Rust (egui/eframe 0.29), inspiré de Sublime Text, un seul binaire.
Intègre Git, GitHub (pull requests), Claude Code, terminal, LSP, linters, orthographe, tests (pytest,
jest, cargo, go, dans le terminal), débogueur DAP (`src/dap.rs`, `src/app/debugger.rs` : debugpy, dlv,
lldb-dap, gdb), packs de langage (`src/packs.rs`, `assets/packs.toml` : installer les outils d'un
langage), extensions (`src/ext.rs`, `src/app/extensions.rs`, bibliothèque `assets/florm_ext.py`) et leur
catalogue (`src/catalog.rs` ; extensions livrées dans `assets/extensions/`, ex. Odoo). Ce qui manque
(Python, Node.js, Go, binaires des releases GitHub) est téléchargé et vérifié par `src/runtimes.rs`.
Coloration par arbre de syntaxe (tree-sitter, grammaires compilées dans Florm, `src/syntax_tree.rs`,
branché dans `HlCache` de `src/highlight.rs`) pour les langages qui en ont une (choisie par l'extension du fichier) ; syntect pour les autres.

## Commandes

```bash
cargo build                 # debug (les dépendances sont optimisées même en debug, voir Cargo.toml)
cargo run -- .              # lancer sur le dossier courant
cargo test                  # tests unitaires (#[cfg(test)] dans les modules)
cargo test -- --ignored     # tests qui lancent des outils installés (ruff, flake8, clangd, gopls, go, debugpy, gdb)
cargo build --release       # LTO fat, long : seulement si nécessaire
scripts/build-windows.sh    # binaire Windows depuis Linux/WSL
FLORM_PROFILE=1 cargo run -- .                              # temps de démarrage, temps par image et causes des redessins (coin + stderr)
FLORM_BENCH=1 cargo test --release bench_views -- --nocapture # temps d'une image des vues lourdes, sans fenêtre
```

- Ne pas lancer `cargo fmt` sur tout le projet : le code n'est pas formaté par rustfmt
  (lignes jusqu'à ~120 colonnes) et cela produirait un énorme diff.
- Les releases se font à la main : `gh workflow run release.yml -f version=x.y.z`. Ne pas en lancer sans demande.

## Architecture

- `src/main.rs` : démarrage, options de fenêtre, déclaration des modules.
- `src/app.rs` + `src/app/*.rs` : l'application egui (`impl App` réparti par thème : `claude_ui.rs`,
  `git_view.rs`, `pulls.rs`, `panels.rs`, `prefs.rs`, `job_results.rs`…). Les plus
  gros ont des sous-modules dans un dossier du même nom (`claude_ui/`, `panels/`) :
  un fichier par partie, avec `use super::*;`.
- `src/*.rs` hors `app/` : la logique sans UI (`git.rs`, `github.rs`, `claude.rs`, `lsp.rs`,
  `terminal.rs`, `document.rs` (buffer `ropey`), `highlight.rs` (`syntect`)…).
  La division de l'éditeur est dans `src/app/panes.rs`.
- Travail en arrière-plan : `src/jobs.rs`. Les tâches tournent sur des threads et renvoient un
  `JobResult` par un channel lu à chaque image ; le traitement est dans `src/app/job_results.rs` (un aiguillage, les traitements longs dans `job_results/`).
  Pour une nouvelle tâche : une variante de `JobResult`, son lancement, son traitement. Ne jamais
  bloquer le thread de l'UI (git, réseau, processus externes).
- Commandes : enum `Cmd` dans `src/commands.rs` (identifiants utilisés dans `[keys]` de settings.toml),
  raccourcis par défaut dans `src/keymap.rs`.
- Préférences : `Settings` dans `src/config.rs` (`~/.config/florm/settings.toml`, `%APPDATA%\florm`),
  page des préférences dans `src/app/prefs.rs`.
- Les outils externes (git, gh, claude, ruff, docker…) sont appelés comme processus ; aucune
  dépendance système autre qu'OpenGL. Préférer éviter d'ajouter des crates (taille du binaire).
- Multiplateforme : Linux (X11/Wayland), Windows, macOS. Le code spécifique passe par `#[cfg(...)]`
  et `src/os.rs`.

## Conventions

- **Textes de l'interface** : toujours bilingues avec les macros de `src/i18n.rs` :
  `t!("Fichier", "File")` (`&'static str`) et `tf!("{n} fichiers", "{n} files")` (`String`, variables capturées dans le texte ou arguments
  positionnels comme `format!`).
  Le français d'abord.
- **Commentaires et doc du code** : en anglais, courts, comme le code existant.
- **Guide intégré** : une fonctionnalité visible par l'utilisateur se documente dans
  `assets/guide.md` **et** `assets/guide.en.md` (inclus dans le binaire), et dans `README.md`
  **et** `README.en.md` si elle concerne l'installation, les outils externes ou la configuration.
- **Style des textes** : phrases simples et concrètes, sans jargon ; symboles déjà utilisés
  dans l'UI (⟳, ⇱ / ⇲, ⋯, ✨, ✂…) plutôt que de nouveaux libellés.

## Commits

En français, sujet `[TAG] Zone : ce qui change, du point de vue de l'utilisateur`, tags `[ADD]`
(nouveauté), `[IMP]` (amélioration), `[FIX]` (correction). Corps : un ou deux paragraphes qui
expliquent le comportement avant / après et le pourquoi. Exemple :

```
[FIX] Pull requests : choisir un dépôt marche aussi quand le dossier n'en est pas un
```
