//! The menus, under the F at the start of the bar at the top (as Zed's ☰).

use super::*;

impl App {
    /// The F of Florm, at the start of the bar at the top (as Zed's ☰): the menus (File, Edit,
    /// Git…) under it.
    pub(in crate::app) fn menu_logo(&mut self, ui: &mut egui::Ui) {
        let mut picked = Vec::new();
        let km = &self.keymap;
        let b = egui::Button::new("").frame(false).min_size(egui::vec2(22.0, 22.0));
        let r = egui::containers::menu::MenuButton::from_button(b).ui(ui, |ui| menus(ui, km, &mut picked)).0;
        let r = r.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t!("Menus (Fichier, Édition, Git…)", "Menus (File, Edit, Git…)"));
        if r.hovered() {
            ui.painter().rect_filled(r.rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        crate::logo::paint(ui.painter(), r.rect.shrink(4.0), false);
        self.pending.extend(picked);
    }

}

/// The menus (in the menu bar, or under the logo of the sidebar when the bar is hidden).
pub(in crate::app) fn menus(ui: &mut egui::Ui, km: &crate::keymap::Keymap, pending: &mut Vec<Cmd>) {
    let mut item = |ui: &mut egui::Ui, label: &str, cmd: Cmd| {
        // GitHub, Claude turned off: their entries go
        if !crate::features::cmd_on(cmd) {
            return;
        }
        if ui.add(egui::Button::new(label).shortcut_text(km.label(cmd))).clicked() {
            pending.push(cmd);
            ui.close();
        }
    };
    ui.menu_button(t!("Fichier", "File"), |ui| {
        item(ui, t!("Nouveau fichier", "New file"), Cmd::NewFile);
        item(ui, t!("Ouvrir un fichier…", "Open a file…"), Cmd::OpenFile);
        item(ui, t!("Ouvrir un dossier…", "Open a folder…"), Cmd::OpenFolder);
        item(ui, t!("Changer de projet…", "Switch project…"), Cmd::SwitchProject);
        item(ui, t!("Fermer le dossier", "Close the folder"), Cmd::CloseFolder);
        item(ui, t!("Rouvrir l'onglet fermé", "Reopen closed tab"), Cmd::ReopenClosed);
        ui.separator();
        item(ui, t!("Enregistrer", "Save"), Cmd::Save);
        item(ui, t!("Enregistrer sous…", "Save as…"), Cmd::SaveAs);
        item(ui, t!("Tout enregistrer", "Save all"), Cmd::SaveAll);
        ui.separator();
        item(ui, t!("Fermer l'onglet", "Close tab"), Cmd::CloseTab);
        item(ui, t!("Préférences", "Preferences"), Cmd::OpenSettings);
        item(ui, t!("Ouvrir settings.toml", "Open settings.toml"), Cmd::OpenSettingsFile);
        item(ui, t!("Quitter", "Quit"), Cmd::Quit);
    });
    ui.menu_button(t!("Édition", "Edit"), |ui| {
        item(ui, t!("Annuler", "Undo"), Cmd::Undo);
        item(ui, t!("Rétablir", "Redo"), Cmd::Redo);
        ui.separator();
        item(ui, t!("Dupliquer la ligne", "Duplicate line"), Cmd::DuplicateLine);
        item(ui, t!("Supprimer la ligne", "Delete line"), Cmd::DeleteLine);
        item(ui, t!("Monter la ligne", "Move line up"), Cmd::MoveLineUp);
        item(ui, t!("Descendre la ligne", "Move line down"), Cmd::MoveLineDown);
        item(ui, t!("Joindre les lignes", "Join lines"), Cmd::JoinLines);
        item(ui, t!("Commenter / décommenter", "Comment / uncomment"), Cmd::ToggleComment);
        item(ui, t!("Indenter", "Indent"), Cmd::Indent);
        item(ui, t!("Désindenter", "Unindent"), Cmd::Unindent);
        item(ui, t!("Trier les lignes", "Sort lines"), Cmd::SortLines);
        item(ui, t!("Supprimer les lignes en double", "Remove duplicate lines"), Cmd::UniqueLines);
        ui.menu_button(t!("Casse", "Case"), |ui| {
            item(ui, t!("MAJUSCULES", "UPPERCASE"), Cmd::UpperCase);
            item(ui, t!("minuscules", "lowercase"), Cmd::LowerCase);
            item(ui, "snake_case", Cmd::SnakeCase);
            item(ui, "camelCase", Cmd::CamelCase);
            item(ui, t!("Majuscule Au Début", "Title Case"), Cmd::TitleCase);
        });
        ui.separator();
        item(ui, t!("Enregistrer une macro / arrêter", "Record a macro / stop"), Cmd::MacroRecord);
        item(ui, t!("Rejouer la macro", "Play the macro"), Cmd::MacroPlay);
    });
    ui.menu_button(t!("Sélection", "Selection"), |ui| {
        item(ui, t!("Tout sélectionner", "Select all"), Cmd::SelectAll);
        item(ui, t!("Élargir la sélection", "Expand selection"), Cmd::ExpandSelection);
        item(ui, t!("Réduire la sélection", "Shrink selection"), Cmd::ShrinkSelection);
        item(ui, t!("Sélectionner la ligne", "Select line"), Cmd::SelectLine);
        item(ui, t!("Ajouter l'occurrence suivante", "Add next occurrence"), Cmd::SelectNext);
        item(ui, t!("Toutes les occurrences", "All occurrences"), Cmd::SelectAllOccurrences);
        item(ui, t!("Diviser en lignes", "Split into lines"), Cmd::SplitLines);
        item(ui, t!("Curseur au-dessus", "Cursor above"), Cmd::AddCursorAbove);
        item(ui, t!("Curseur en dessous", "Cursor below"), Cmd::AddCursorBelow);
        item(ui, t!("Crochet correspondant", "Matching bracket"), Cmd::GotoBracket);
    });
    ui.menu_button(t!("Rechercher", "Find"), |ui| {
        item(ui, t!("Rechercher…", "Find…"), Cmd::Find);
        item(ui, t!("Remplacer…", "Replace…"), Cmd::Replace);
        item(ui, t!("Suivant", "Next"), Cmd::FindNext);
        item(ui, t!("Rechercher dans le dossier…", "Find in folder…"), Cmd::FindInFiles);
        item(ui, t!("Résultat global suivant", "Next global result"), Cmd::NextResult);
        item(ui, t!("Précédent", "Previous"), Cmd::FindPrev);
        ui.separator();
        item(ui, t!("Aller au fichier…", "Go to file…"), Cmd::QuickOpen);
        item(ui, t!("Aller à la ligne…", "Go to line…"), Cmd::GotoLine);
    });
    ui.menu_button(t!("Affichage", "View"), |ui| {
        item(ui, t!("Palette de commandes", "Command palette"), Cmd::CommandPalette);
        item(ui, t!("Barre latérale", "Sidebar"), Cmd::ToggleSidebar);
        item(ui, "Terminal", Cmd::ToggleTerminal);
        item(ui, t!("Problèmes", "Problems"), Cmd::ShowProblems);
        item(ui, t!("Sortie", "Output"), Cmd::ShowOutput);
        item(ui, "Minimap", Cmd::ToggleMinimap);
        item(ui, t!("Retour à la ligne automatique", "Word wrap"), Cmd::ToggleWordWrap);
        item(ui, t!("Espaces et tabulations visibles", "Visible spaces and tabs"), Cmd::ToggleWhitespace);
        item(ui, t!("Mode sans distraction", "Distraction-free mode"), Cmd::ToggleZen);
        ui.separator();
        item(ui, t!("Syntaxe…", "Syntax…"), Cmd::SetSyntax);
        item(ui, t!("Thème…", "Theme…"), Cmd::SelectTheme);
        item(ui, t!("Zoom avant", "Zoom in"), Cmd::ZoomIn);
        item(ui, t!("Zoom arrière", "Zoom out"), Cmd::ZoomOut);
    });
    ui.menu_button("Git", |ui| {
        item(ui, t!("Panneau Git", "Git panel"), Cmd::ShowGit);
        item(ui, "Commit…", Cmd::GitCommit);
        item(ui, t!("Revue IA des changements", "AI review of the changes"), Cmd::GitAiReview);
        item(ui, "Pull", Cmd::GitPull);
        item(ui, "Push", Cmd::GitPush);
        item(ui, "Fetch", Cmd::GitFetch);
        ui.separator();
        item(ui, t!("Changer de branche…", "Switch branch…"), Cmd::GitCheckout);
        item(ui, t!("Nouvelle branche…", "New branch…"), Cmd::GitNewBranch);
        item(ui, t!("Diff du fichier", "Diff of the file"), Cmd::GitDiffFile);
        item(ui, t!("Historique (graphe)", "History (graph)"), Cmd::GitHistory);
        item(ui, t!("Pull requests GitHub", "GitHub pull requests"), Cmd::PullRequests);
        item(ui, t!("Créer une pull request…", "Create a pull request…"), Cmd::CreatePr);
        item(ui, t!("Diff dans l'éditeur (vert / rouge)", "Diff in the editor (green / red)"), Cmd::ToggleInlineDiff);
        item(ui, t!("Blame de la ligne", "Line blame"), Cmd::GitBlame);
        item(ui, t!("Blame de chaque ligne (marge)", "Blame of every line (gutter)"), Cmd::GitBlameGutter);
        item(ui, t!("Historique de la ligne", "Line history"), Cmd::GitLineHistory);
        item(ui, t!("Historique du fichier", "File history"), Cmd::GitFileHistory);
        item(ui, t!("Rechercher dans l'historique…", "Search the history…"), Cmd::GitSearchHistory);
        item(ui, t!("Reflog (retrouver un commit perdu)", "Reflog (find a lost commit)"), Cmd::GitReflog);
    });
    ui.menu_button(t!("Outils", "Tools"), |ui| {
        item(ui, t!("Linter le fichier", "Lint the file"), Cmd::Lint);
        item(ui, t!("Langages : outils et tree-sitter…", "Languages: tools and tree-sitter…"), Cmd::ShowLanguages);
        item(ui, t!("Extensions et catalogue…", "Extensions and catalogue…"), Cmd::ShowExtensions);
        ui.separator();
        item(ui, "✳ Claude", Cmd::Claude);
        ui.separator();
        item(ui, t!("Historique local du fichier…", "Local history of the file…"), Cmd::LocalHistory);
        item(ui, t!("Comparer avec le presse-papiers", "Compare with the clipboard"), Cmd::CompareClipboard);
        item(ui, t!("Comparer avec…", "Compare with…"), Cmd::CompareWith);
        ui.separator();
        item(ui, t!("Insérer un modèle…", "Insert a template…"), Cmd::InsertTemplate);
        item(ui, t!("Nouveau modèle…", "New template…"), Cmd::NewTemplate);
        item(ui, t!("Dossier des modèles", "Templates folder"), Cmd::OpenTemplatesFolder);
        ui.separator();
        ui.separator();
        item(ui, t!("Synchroniser la configuration partagée", "Synchronize the shared configuration"), Cmd::SharedSync);
        item(ui, t!("Publier la configuration partagée", "Publish the shared configuration"), Cmd::SharedPublish);
    });
    ui.menu_button(t!("Aide", "Help"), |ui| {
        item(ui, t!("Guide de Florm (recherche, Git, terminal, modèles…)", "Florm guide (search, Git, terminal, templates…)"), Cmd::ShowGuide);
        item(ui, t!("Accueil", "Welcome"), Cmd::ShowWelcome);
        item(ui, t!("Palette de commandes", "Command palette"), Cmd::CommandPalette);
        item(ui, t!("Raccourcis et préférences", "Shortcuts and preferences"), Cmd::OpenSettings);
        ui.separator();
        item(ui, t!("Rechercher une mise à jour", "Check for an update"), Cmd::CheckUpdate);
    });
}
