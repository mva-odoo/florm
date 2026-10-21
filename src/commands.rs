use crate::document::Motion;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cmd {
    // --- editor (need the text area to have focus) ---
    Move(Motion, bool),
    Backspace,
    Delete,
    DeleteWordBack,
    DeleteWordForward,
    Newline,
    Tab,
    Indent,
    Unindent,
    SelectAll,
    SelectLine,
    SelectNext,
    SelectAllOccurrences,
    SplitLines,
    AddCursorAbove,
    AddCursorBelow,
    DuplicateLine,
    DeleteLine,
    MoveLineUp,
    MoveLineDown,
    ToggleComment,
    ToggleBookmark,
    NextBookmark,
    PrevBookmark,
    ClearBookmarks,
    ListBookmarks,
    JoinLines,
    InsertLineAfter,
    InsertLineBefore,
    UpperCase,
    LowerCase,
    SortLines,
    GotoBracket,
    GitNextChange,
    GitPrevChange,
    GitRevertChange,
    GitRevertFile,
    Undo,
    Redo,
    // --- application ---
    Escape,
    CommandPalette,
    QuickOpen,
    GotoLine,
    Find,
    Replace,
    FindInFiles,
    FindInFilesTab,
    NextResult,
    PrevResult,
    SwitchProject,
    ShowProjects,
    CloseFolder,
    FindNext,
    FindPrev,
    FindAll,
    Save,
    SaveAs,
    SaveAll,
    NewFile,
    OpenFile,
    OpenFolder,
    CloseTab,
    ReopenClosed,
    NextTab,
    PrevTab,
    ToggleSidebar,
    ToggleMenuBar,
    ToggleTerminal,
    NewTerminalTab,
    SplitRight,
    GotoSymbol,
    OpenFolderPath,
    GotoLineSelect,
    RevealInTree,
    NavBack,
    NavForward,
    Complete,
    GotoDefinition,
    RenameSymbol,
    InstallPylsp,
    LspRestart,
    MarkdownPreview,
    MarkdownPreviewSide,
    Fold,
    Unfold,
    FoldAll,
    UnfoldAll,
    ToggleBreadcrumbs,
    SplitDown,
    Unsplit,
    FocusOtherPane,
    ShowProblems,
    ShowOutput,
    ShowGit,
    ShowFiles,
    ShowWelcome,
    ToggleMinimap,
    ZoomIn,
    ZoomOut,
    SetSyntax,
    SelectTheme,
    Lint,
    OpenSettings,
    OpenSettingsFile,
    ReloadPlugins,
    InsertTemplate,
    ShowGuide,
    OpenTemplatesFolder,
    NewTemplate,
    OpenPluginsFolder,
    RefreshTree,
    Quit,
    GitRefresh,
    GitPull,
    GitPush,
    GitFetch,
    GitStageAll,
    GitCommit,
    GitCheckout,
    GitNewBranch,
    GitDiffFile,
    GitHistory,
    PullRequests,
    ToggleInlineDiff,
    GitBlame,
    GitLineHistory,
    ClearSearchHistory,
    Copy,
    Cut,
    Paste,
    CopyFilePath,
    RevealInOs,
    CopyFileRef,
    CopyGithubLink,
    FindWordInFiles,
    ShowChange,
    NoqaLine,
    FormatDocument,
    FindReferences,
    FixDocument,
    InstallFlake8,
    Plugin(usize),
}

impl Cmd {
    pub fn is_editor(self) -> bool {
        use Cmd::*;
        matches!(
            self,
            Move(..)
                | Backspace
                | Delete
                | DeleteWordBack
                | DeleteWordForward
                | Newline
                | Tab
                | Indent
                | Unindent
                | SelectAll
                | SelectLine
                | SelectNext
                | SelectAllOccurrences
                | SplitLines
                | AddCursorAbove
                | AddCursorBelow
                | DuplicateLine
                | DeleteLine
                | MoveLineUp
                | MoveLineDown
                | ToggleComment
                | JoinLines
                | InsertLineAfter
                | InsertLineBefore
                | UpperCase
                | LowerCase
                | SortLines
                | GotoBracket
                | GitNextChange
                | GitPrevChange
                | GitRevertChange
                | GitRevertFile
                | Undo
                | Redo
        )
    }

    /// Editor commands that change the text (refused on read-only buffers).
    pub fn modifies(self) -> bool {
        use Cmd::*;
        self.is_editor()
            && !matches!(
                self,
                Move(..)
                    | SelectAll
                    | SelectLine
                    | SelectNext
                    | SelectAllOccurrences
                    | SplitLines
                    | AddCursorAbove
                    | AddCursorBelow
                    | GotoBracket
                    | GitNextChange
                    | GitPrevChange
            )
    }
}

use Cmd::*;
use Motion::*;

/// (id, label, command). Commands with an empty label are hidden from the palette.
pub const ALL: &[(&str, &str, Cmd)] = &[
    ("command_palette", "Palette de commandes", CommandPalette),
    ("quick_open", "Aller au fichier…", QuickOpen),
    ("goto_line", "Aller à la ligne…", GotoLine),
    ("find", "Rechercher…", Find),
    ("replace", "Remplacer…", Replace),
    ("find_next", "Rechercher : suivant", FindNext),
    ("find_prev", "Rechercher : précédent", FindPrev),
    ("find_all", "Rechercher : sélectionner tous les résultats", FindAll),
    ("find_in_files", "Rechercher dans le dossier…", FindInFiles),
    ("find_in_files_tab", "Rechercher dans le dossier, dans un onglet…", FindInFilesTab),
    ("next_result", "Recherche globale : résultat suivant", NextResult),
    ("prev_result", "Recherche globale : résultat précédent", PrevResult),
    ("switch_project", "Projet : changer…", SwitchProject),
    ("show_projects", "Projet : afficher la liste", ShowProjects),
    ("close_project", "Projet : fermer le dossier", CloseFolder),
    ("save", "Enregistrer", Save),
    ("save_as", "Enregistrer sous…", SaveAs),
    ("save_all", "Tout enregistrer", SaveAll),
    ("new_file", "Nouveau fichier", NewFile),
    ("open_file", "Ouvrir un fichier…", OpenFile),
    ("open_folder", "Ouvrir un dossier…", OpenFolder),
    ("close_tab", "Fermer l'onglet", CloseTab),
    ("reopen_closed", "Rouvrir l'onglet fermé", ReopenClosed),
    ("next_tab", "Onglet suivant", NextTab),
    ("prev_tab", "Onglet précédent", PrevTab),
    ("toggle_sidebar", "Afficher/masquer la barre latérale", ToggleSidebar),
    ("toggle_menu_bar", "Afficher/masquer la barre de menus (Fichier, Édition…)", ToggleMenuBar),
    ("toggle_terminal", "Afficher/masquer le terminal", ToggleTerminal),
    ("new_terminal_tab", "Terminal : nouveau terminal dans un onglet (à placer à côté d'un fichier)", NewTerminalTab),
    ("fold", "Replier le bloc", Fold),
    ("unfold", "Déplier le bloc", Unfold),
    ("fold_all", "Tout replier (classes, fonctions…)", FoldAll),
    ("unfold_all", "Tout déplier", UnfoldAll),
    ("markdown_preview", "Markdown : aperçu dans un onglet", MarkdownPreview),
    ("markdown_preview_side", "Markdown : aperçu à côté", MarkdownPreviewSide),
    ("complete", "Suggestions de complétion", Complete),
    ("goto_definition", "Aller à la définition", GotoDefinition),
    ("rename_symbol", "Renommer le symbole (dans tout le projet)…", RenameSymbol),
    ("install_pylsp", "LSP : installer pylsp pour Python (Florm)", InstallPylsp),
    ("lsp_restart", "LSP : redémarrer les serveurs de langage", LspRestart),
    ("nav_back", "Revenir à la position précédente du curseur", NavBack),
    ("nav_forward", "Aller à la position suivante du curseur", NavForward),
    ("reveal_in_tree", "Afficher le fichier dans l'arborescence", RevealInTree),
    ("goto_line_select", "Aller à la ligne et la sélectionner (ou plusieurs : 10-20)…", GotoLineSelect),
    ("open_folder_path", "Ouvrir un dossier en tapant son chemin…", OpenFolderPath),
    ("goto_symbol", "Aller au symbole du fichier (classe, fonction, record…)…", GotoSymbol),
    ("toggle_breadcrumbs", "Afficher/masquer le fil d'Ariane", ToggleBreadcrumbs),
    ("split_right", "Diviser l'éditeur : fichier courant à droite", SplitRight),
    ("split_down", "Diviser l'éditeur : fichier courant en bas", SplitDown),
    ("unsplit", "Fusionner les panneaux de l'éditeur", Unsplit),
    ("focus_other_pane", "Aller à l'autre panneau de l'éditeur", FocusOtherPane),
    ("show_problems", "Afficher les problèmes (lint)", ShowProblems),
    ("show_output", "Afficher la sortie", ShowOutput),
    ("show_git", "Git : afficher la vue Git", ShowGit),
    ("show_files", "Afficher les fichiers", ShowFiles),
    ("show_welcome", "Afficher l'accueil de Florm", ShowWelcome),
    ("toggle_minimap", "Afficher/masquer la minimap", ToggleMinimap),
    ("zoom_in", "Zoom avant", ZoomIn),
    ("zoom_out", "Zoom arrière", ZoomOut),
    ("set_syntax", "Définir la syntaxe…", SetSyntax),
    ("select_theme", "Choisir le thème…", SelectTheme),
    ("lint", "Linter le fichier courant", Lint),
    ("open_settings", "Préférences", OpenSettings),
    ("open_settings_file", "Préférences : ouvrir settings.toml", OpenSettingsFile),
    ("reload_plugins", "Recharger les plugins", ReloadPlugins),
    ("insert_template", "Insérer un modèle / snippet (vue Odoo, menu, action…)…", InsertTemplate),
    ("show_guide", "Aide : guide de Florm (exemples de recherche, Git, terminal…)", ShowGuide),
    ("open_templates_folder", "Ouvrir le dossier des modèles", OpenTemplatesFolder),
    ("new_template", "Nouveau modèle / snippet (déclencheur comme ipdb, depuis la sélection)…", NewTemplate),
    ("open_plugins_folder", "Ouvrir le dossier des plugins", OpenPluginsFolder),
    ("refresh_tree", "Rafraîchir l'arborescence", RefreshTree),
    ("quit", "Quitter", Quit),
    ("escape", "", Escape),
    ("git_refresh", "Git : rafraîchir", GitRefresh),
    ("git_pull", "Git : pull", GitPull),
    ("git_push", "Git : push", GitPush),
    ("git_fetch", "Git : fetch", GitFetch),
    ("git_stage_all", "Git : tout indexer", GitStageAll),
    ("git_commit", "Git : commit…", GitCommit),
    ("git_checkout", "Git : changer de branche…", GitCheckout),
    ("git_new_branch", "Git : nouvelle branche…", GitNewBranch),
    ("git_diff_file", "Git : diff du fichier courant", GitDiffFile),
    ("git_history", "Git : historique (graphe des commits)", GitHistory),
    ("pull_requests", "GitHub : pull requests (commentaires, suggestions, commits)", PullRequests),
    ("copy", "Copier", Copy),
    ("cut", "Couper", Cut),
    ("paste", "Coller", Paste),
    ("copy_file_path", "Copier le chemin du fichier", CopyFilePath),
    ("reveal_in_os", "Afficher le fichier dans le gestionnaire de fichiers", RevealInOs),
    ("copy_file_ref", "Copier la référence fichier:ligne", CopyFileRef),
    ("copy_github_link", "Git : copier le lien GitHub de la ligne", CopyGithubLink),
    ("find_word_in_files", "Rechercher le mot sous le curseur dans le dossier", FindWordInFiles),
    ("noqa_line", "Lint : ignorer les erreurs de la ligne (# noqa)", NoqaLine),
    ("find_references", "Rechercher les usages du symbole (serveur de langage, sinon le mot entier dans le dossier)", FindReferences),
    ("format_document", "Python : formater le fichier, ou les lignes sélectionnées (ruff format)", FormatDocument),
    ("fix_document", "Python : corriger automatiquement (ruff --fix : imports inutilisés, ordre des imports…)", FixDocument),
    ("install_flake8", "Lint : installer flake8 pour Florm", InstallFlake8),
    ("show_change", "Git : voir le changement de la ligne", ShowChange),
    ("clear_search_history", "Recherche : effacer l'historique", ClearSearchHistory),
    ("git_blame", "Git : blame de la ligne (afficher / masquer)", GitBlame),
    ("git_line_history", "Git : historique de la ligne", GitLineHistory),
    ("toggle_inline_diff", "Git : afficher le diff dans l'éditeur (vert / rouge)", ToggleInlineDiff),
    ("git_next_change", "Git : changement suivant (voir le diff)", GitNextChange),
    ("git_prev_change", "Git : changement précédent (voir le diff)", GitPrevChange),
    ("git_revert_change", "Git : revenir en arrière sur ce changement", GitRevertChange),
    ("git_revert_file", "Git : annuler toutes les modifications du fichier", GitRevertFile),
    ("undo", "Annuler", Undo),
    ("redo", "Rétablir", Redo),
    ("select_all", "Tout sélectionner", SelectAll),
    ("select_line", "Sélectionner la ligne", SelectLine),
    ("select_next", "Ajouter l'occurrence suivante", SelectNext),
    ("select_all_occurrences", "Sélectionner toutes les occurrences", SelectAllOccurrences),
    ("split_selection_into_lines", "Diviser la sélection en lignes", SplitLines),
    ("add_cursor_above", "Ajouter un curseur au-dessus", AddCursorAbove),
    ("add_cursor_below", "Ajouter un curseur en dessous", AddCursorBelow),
    ("duplicate_line", "Dupliquer la ligne", DuplicateLine),
    ("delete_line", "Supprimer la ligne", DeleteLine),
    ("move_line_up", "Monter la ligne", MoveLineUp),
    ("move_line_down", "Descendre la ligne", MoveLineDown),
    ("toggle_comment", "Commenter / décommenter", ToggleComment),
    ("toggle_bookmark", "Signet : ajouter / retirer sur la ligne", ToggleBookmark),
    ("next_bookmark", "Signet : suivant", NextBookmark),
    ("prev_bookmark", "Signet : précédent", PrevBookmark),
    ("clear_bookmarks", "Signet : tout retirer dans le fichier", ClearBookmarks),
    ("list_bookmarks", "Signet : liste (tous les fichiers ouverts)…", ListBookmarks),
    ("indent", "Indenter", Indent),
    ("unindent", "Désindenter", Unindent),
    ("join_lines", "Joindre les lignes", JoinLines),
    ("insert_line_after", "Insérer une ligne après", InsertLineAfter),
    ("insert_line_before", "Insérer une ligne avant", InsertLineBefore),
    ("upper_case", "Convertir en MAJUSCULES", UpperCase),
    ("lower_case", "Convertir en minuscules", LowerCase),
    ("sort_lines", "Trier les lignes", SortLines),
    ("goto_bracket", "Aller au crochet correspondant", GotoBracket),
    ("backspace", "", Backspace),
    ("delete", "", Delete),
    ("delete_word_back", "", DeleteWordBack),
    ("delete_word_forward", "", DeleteWordForward),
    ("newline", "", Newline),
    ("tab", "", Tab),
    ("move_left", "", Move(Left, false)),
    ("move_right", "", Move(Right, false)),
    ("move_up", "", Move(Up, false)),
    ("move_down", "", Move(Down, false)),
    ("move_word_left", "", Move(WordLeft, false)),
    ("move_word_right", "", Move(WordRight, false)),
    ("move_home", "", Move(Home, false)),
    ("move_end", "", Move(End, false)),
    ("move_page_up", "", Move(PageUp, false)),
    ("move_page_down", "", Move(PageDown, false)),
    ("move_doc_start", "", Move(DocStart, false)),
    ("move_doc_end", "", Move(DocEnd, false)),
    ("select_left", "", Move(Left, true)),
    ("select_right", "", Move(Right, true)),
    ("select_up", "", Move(Up, true)),
    ("select_down", "", Move(Down, true)),
    ("select_word_left", "", Move(WordLeft, true)),
    ("select_word_right", "", Move(WordRight, true)),
    ("select_home", "", Move(Home, true)),
    ("select_end", "", Move(End, true)),
    ("select_page_up", "", Move(PageUp, true)),
    ("select_page_down", "", Move(PageDown, true)),
    ("select_doc_start", "", Move(DocStart, true)),
    ("select_doc_end", "", Move(DocEnd, true)),
];

pub fn by_id(id: &str) -> Option<Cmd> {
    ALL.iter().find(|(i, _, _)| *i == id).map(|(_, _, c)| *c)
}

pub fn id_of(cmd: Cmd) -> Option<&'static str> {
    ALL.iter().find(|(_, _, c)| *c == cmd).map(|(id, _, _)| *id)
}
