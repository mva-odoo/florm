//! Folder tree, loaded lazily (a directory is read only when expanded).

use crate::icons;
use eframe::egui::{self, Color32, CursorIcon, DragAndDrop, FontId, Id, Pos2, Rect, RichText, Sense, Stroke, Ui};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub enum TreeAction {
    Open(PathBuf),
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
    /// Move a file or folder into a directory (drag & drop).
    Move(PathBuf, PathBuf),
    Delete(PathBuf),
    CopyPath(PathBuf),
    /// Show in the system file manager.
    Reveal(PathBuf),
    Terminal(PathBuf),
    /// Give the file or folder to Claude with the next message.
    Claude(PathBuf),
    Refresh,
    /// Leave the folder out of go to file, the symbol index and find in files (`true`), or
    /// take it back.
    Exclude(PathBuf, bool),
    /// Chosen in the menu of the folder name at the top.
    SwitchProject(PathBuf),
    OpenFolder,
}

/// Keyboard navigation in the tree: the entry under the keyboard cursor, and a focus request.
#[derive(Default)]
pub struct Nav {
    pub cursor: Option<PathBuf>,
    /// Give the keyboard focus to the tree at the next frame.
    pub focus: bool,
}

/// Focus id of the tree (arrows, Enter… move in it instead of the editor).
pub fn tree_id() -> Id {
    Id::new("florm_tree")
}

pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub children: Option<Vec<Node>>,
}

impl Node {
    pub fn new(path: PathBuf, is_dir: bool) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Node { path, name, is_dir, expanded: false, children: None }
    }

    fn load(&mut self) {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.path) {
            for e in rd.flatten() {
                if e.file_name() == ".git" {
                    continue;
                }
                let p = e.path();
                let is_dir = p.is_dir();
                v.push(Node::new(p, is_dir));
            }
        }
        v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        self.children = Some(v);
    }

    pub fn expanded_paths(&self, out: &mut Vec<PathBuf>) {
        for c in self.children.iter().flatten() {
            if c.is_dir && c.expanded {
                out.push(c.path.clone());
                c.expanded_paths(out);
            }
        }
    }

    pub fn expand_paths(&mut self, set: &HashSet<PathBuf>) {
        if self.children.is_none() {
            self.load();
        }
        for c in self.children.iter_mut().flatten() {
            if c.is_dir && set.contains(&c.path) {
                c.expanded = true;
                c.expand_paths(set);
            }
        }
    }

    /// Opens the folders down to `target` (loading them if needed); false if it is not inside.
    pub fn reveal(&mut self, target: &Path) -> bool {
        if !target.starts_with(&self.path) {
            return false;
        }
        if self.children.is_none() {
            self.load();
        }
        self.expanded = true;
        for c in self.children.iter_mut().flatten() {
            if c.path == target {
                return true;
            }
            if c.is_dir && target.starts_with(&c.path) {
                return c.reveal(target);
            }
        }
        false
    }

    /// The node at `path` (inside this one).
    pub fn find_mut(&mut self, path: &Path) -> Option<&mut Node> {
        if self.path == path {
            return Some(self);
        }
        if !path.starts_with(&self.path) {
            return None;
        }
        self.children.as_mut()?.iter_mut().find_map(|c| c.find_mut(path))
    }

    /// Entries shown below this node, in display order: (path, is_dir, expanded).
    fn visible(&self, out: &mut Vec<(PathBuf, bool, bool)>) {
        for c in self.children.iter().flatten() {
            out.push((c.path.clone(), c.is_dir, c.expanded));
            if c.is_dir && c.expanded {
                c.visible(out);
            }
        }
    }

    fn toggle(&mut self) {
        self.expanded = !self.expanded;
        if self.expanded && self.children.is_none() {
            self.load();
        }
    }

    /// Re-reads expanded directories while keeping what was open.
    pub fn refresh(&mut self) {
        let Some(old) = self.children.take() else { return };
        let mut old: HashMap<PathBuf, Node> = old.into_iter().map(|n| (n.path.clone(), n)).collect();
        self.load();
        for c in self.children.as_mut().unwrap() {
            if let Some(o) = old.remove(&c.path) {
                if o.expanded && c.is_dir {
                    c.expanded = true;
                    c.children = o.children;
                    c.refresh();
                }
            }
        }
    }
}

fn git_color(c: char) -> Option<Color32> {
    match c {
        'M' | 'R' => Some(crate::highlight::yellow()),
        'A' | 'U' | '?' => Some(crate::highlight::green()),
        'D' => Some(crate::highlight::red()),
        _ => None,
    }
}

/// `scroll_to`: entry to bring into view (cleared once shown); `projects`: (name, folder) offered
/// by a click on the folder name.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    root: &mut Node,
    projects: &[(String, PathBuf)],
    ex: &crate::walk::Exclusions,
    active: Option<&Path>,
    git: &HashMap<PathBuf, char>,
    repos: &HashMap<PathBuf, (String, String)>,
    scroll_to: &mut Option<PathBuf>,
    nav: &mut Nav,
) -> Option<TreeAction> {
    let mut act = None;
    if root.children.is_none() {
        root.load();
        root.expanded = true;
    }
    // the whole panel takes the keyboard focus (rows drawn over it get the clicks)
    let bg = ui.interact(ui.available_rect_before_wrap(), tree_id(), Sense::click());
    if std::mem::take(&mut nav.focus) || bg.clicked() {
        bg.request_focus();
        if nav.cursor.is_none() {
            nav.cursor = active.map(Path::to_path_buf).or_else(|| root.children.iter().flatten().next().map(|c| c.path.clone()));
        }
    }
    let focused = bg.has_focus();
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(tree_id(), egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: false })
        });
        if let Some(a) = keyboard(ui, root, nav, scroll_to) {
            act = Some(a);
        }
    }
    let cursor = nav.cursor.clone().filter(|_| focused);
    let mut clicked = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        // the folder name, centered: a click shows the projects to switch to
        ui.add_space(4.0);
        let r = ui
            .vertical_centered(|ui| {
                let name = RichText::new(root.name.to_uppercase()).size(11.5).strong().extra_letter_spacing(1.6);
                ui.add(egui::Label::new(name).sense(Sense::click()))
            })
            .inner;
        let r = r.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(t!("Changer de projet (Ctrl+Alt+P)", "Switch project (Ctrl+Alt+P)"));
        let popup = Id::new("project_menu");
        if r.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        // clicks in its filter field must not close it
        let shown = egui::popup_below_widget(ui, popup, &r, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(260.0);
            ui.label(RichText::new(t!("Projets", "Projects")).small().weak());
            let names: Vec<String> = projects.iter().map(|(n, _)| n.clone()).collect();
            let current = projects.iter().position(|(_, p)| *p == root.path);
            if let Some(k) = crate::pick::list(ui, popup, &names, current) {
                if Some(k) != current {
                    act = Some(TreeAction::SwitchProject(projects[k].1.clone()));
                }
                ui.memory_mut(|m| m.close_popup());
            }
            ui.separator();
            if ui.button(t!("Ouvrir un dossier…", "Open a folder…")).clicked() {
                act = Some(TreeAction::OpenFolder);
                ui.memory_mut(|m| m.close_popup());
            }
        });
        // its filter starts empty at the next opening
        if shown.is_none() || act.is_some() {
            crate::pick::closed(ui, popup);
        }
        if let Some((label, tip)) = repos.get(&root.path) {
            let color = mix(ui.visuals().hyperlink_color, ui.visuals().weak_text_color(), 0.35);
            ui.vertical_centered(|ui| ui.label(RichText::new(label.as_str()).size(11.0).color(color)).on_hover_text(tip.as_str()));
        }
        ui.add_space(6.0);
        drop_target(ui, &r, &root.path, &mut act);
        r.context_menu(|ui| menu(ui, &root.path, true, None, &mut act));
        if let Some(children) = root.children.as_mut() {
            for c in children {
                node_ui(ui, c, 0, active, git, repos, ex, false, &mut act, scroll_to, cursor.as_deref(), &mut clicked);
            }
        }
    });
    // a click moves the keyboard cursor there; on a folder, the tree keeps the keyboard
    if let Some((p, is_dir)) = clicked {
        nav.cursor = Some(p);
        if is_dir {
            bg.request_focus();
        }
    }
    if let Some(p) = DragAndDrop::payload::<PathBuf>(ui.ctx()) {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), Id::new("florm_dnd"), |ui| ui.label(tf!("Déplacer {name}", "Move {name}")));
    }
    act
}

/// Folders accept dropped tree entries (files drop into their parent folder).
fn drop_target(ui: &Ui, r: &egui::Response, dir: &Path, act: &mut Option<TreeAction>) {
    let valid = |src: &Path| src != dir && !dir.starts_with(src) && src.parent() != Some(dir);
    if let Some(src) = r.dnd_hover_payload::<PathBuf>() {
        if valid(&src) {
            ui.painter().rect_stroke(r.rect.expand(1.0), 2.0, Stroke::new(1.5_f32, ui.visuals().selection.stroke.color));
        }
    }
    if let Some(src) = r.dnd_release_payload::<PathBuf>() {
        if valid(&src) {
            *act = Some(TreeAction::Move((*src).clone(), dir.to_path_buf()));
        }
    }
}

fn node_ui(
    ui: &mut Ui,
    node: &mut Node,
    depth: usize,
    active: Option<&Path>,
    git: &HashMap<PathBuf, char>,
    repos: &HashMap<PathBuf, (String, String)>,
    ex: &crate::walk::Exclusions,
    parent_excluded: bool,
    act: &mut Option<TreeAction>,
    scroll_to: &mut Option<PathBuf>,
    cursor: Option<&Path>,
    clicked: &mut Option<(PathBuf, bool)>,
) {
    // left out of the index (exclusions of the preferences): a crossed out eye after the name of
    // what a rule excludes (the content of an excluded folder is excluded with it)
    let own_rule = ex.excludes(&node.path, node.is_dir);
    let excluded = parent_excluded || own_rule;
    let selected = !node.is_dir && active == Some(node.path.as_path());
    let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 23.0), Sense::click_and_drag());
    if scroll_to.as_deref() == Some(node.path.as_path()) {
        r.scroll_to_me(Some(egui::Align::Center));
        *scroll_to = None;
    }
    let v = ui.visuals();
    let bg = v.panel_fill;
    let light = bg.r() as u32 + bg.g() as u32 + bg.b() as u32 > 384;
    if selected {
        ui.painter().rect_filled(rect, 5.0, v.selection.bg_fill);
    } else if r.hovered() {
        ui.painter().rect_filled(rect, 5.0, v.widgets.hovered.weak_bg_fill);
    }
    if cursor == Some(node.path.as_path()) {
        ui.painter().rect_stroke(rect.shrink(0.5), 5.0, Stroke::new(1.0_f32, v.selection.stroke.color));
    }
    let painter = ui.painter().with_clip_rect(rect);
    let cy = rect.center().y;
    let mut x = rect.left() + 4.0 + depth as f32 * 14.0;
    if node.is_dir {
        icons::paint_chevron(&painter, Pos2::new(x + 5.0, cy), node.expanded, v.weak_text_color());
    }
    x += 14.0;
    let icon = Rect::from_center_size(Pos2::new(x + 8.0, cy), egui::vec2(16.0, 16.0));
    if node.is_dir {
        let folder = mix(v.hyperlink_color, v.weak_text_color(), 0.75);
        icons::paint_folder(&painter, icon, node.expanded, folder, bg);
    } else {
        icons::paint_file(&painter, icon, &node.name, bg, light);
    }
    x += 23.0;
    let color = git
        .get(&node.path)
        .and_then(|c| git_color(*c))
        .unwrap_or(if selected { v.strong_text_color() } else { v.text_color() });
    // the name first; a git repository's branch at the right of the row, in the room left
    // (shortened with "…", or not shown when there is too little)
    let name = painter.layout_no_wrap(node.name.clone(), FontId::proportional(13.5), color);
    let mut name_end = x + name.size().x;
    painter.galley(Pos2::new(x, cy - name.size().y / 2.0), name, color);
    if own_rule && !parent_excluded {
        icons::paint_eye_off(&painter, Pos2::new(name_end + 12.0, cy), v.weak_text_color());
        name_end += 20.0;
    }
    if let Some((full, _)) = node.is_dir.then(|| repos.get(&node.path)).flatten() {
        // at rest only what needs attention (↑ to push, ↓ to pull, changed files); the branch
        // when the row is hovered
        let label = if r.hovered() {
            full.clone()
        } else {
            [" ↑", " ↓", " · "]
                .iter()
                .filter_map(|p| full.find(p))
                .min()
                .map(|i| full[i..].trim_start_matches(" · ").trim().to_string())
                .unwrap_or_default()
        };
        let room = rect.right() - 6.0 - (name_end + 10.0);
        let font = FontId::proportional(11.0);
        let tint = mix(v.hyperlink_color, v.weak_text_color(), 0.35);
        let mut text = label.clone();
        let mut g = painter.layout_no_wrap(text.clone(), font.clone(), tint);
        while g.size().x > room && text.chars().count() > 4 {
            text = format!("{}…", text.chars().take(text.chars().count().saturating_sub(2)).collect::<String>().trim_end_matches('…'));
            g = painter.layout_no_wrap(text.clone(), font.clone(), tint);
        }
        if room >= 30.0 && g.size().x <= room {
            painter.galley(Pos2::new(rect.right() - 6.0 - g.size().x, cy - g.size().y / 2.0), g, tint);
        }
    }
    let repo = node.is_dir.then(|| repos.get(&node.path)).flatten();
    if r.drag_started() {
        DragAndDrop::set_payload(ui.ctx(), node.path.clone());
    }
    let target = if node.is_dir { node.path.clone() } else { node.path.parent().map(Path::to_path_buf).unwrap_or_default() };
    drop_target(ui, &r, &target, act);
    if r.clicked() {
        *clicked = Some((node.path.clone(), node.is_dir));
        if node.is_dir {
            node.toggle();
        } else {
            *act = Some(TreeAction::Open(node.path.clone()));
        }
    }
    let note = if excluded { t!("\n\nExclu de l'index : ni dans Ctrl+P, ni dans F12, ni dans la recherche", "\n\nExcluded from the index: not in Ctrl+P, F12 or the search") } else { "" };
    let r = match repo {
        Some((_, tip)) => r.on_hover_text_at_pointer(format!("{}{note}\n\n{tip}", node.path.display())),
        None => r.on_hover_text_at_pointer(format!("{}{note}", node.path.display())),
    };
    r.context_menu(|ui| menu(ui, &node.path, node.is_dir, node.is_dir.then_some(excluded), act));
    if node.is_dir && node.expanded {
        if let Some(children) = node.children.as_mut() {
            for c in children {
                node_ui(ui, c, depth + 1, active, git, repos, ex, excluded, act, scroll_to, cursor, clicked);
            }
        }
    }
}

/// Arrow keys, Enter, Home / End, F2, Delete while the tree has the keyboard.
fn keyboard(ui: &Ui, root: &mut Node, nav: &mut Nav, scroll_to: &mut Option<PathBuf>) -> Option<TreeAction> {
    let keys: Vec<(egui::Key, egui::Modifiers)> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    let mut act = None;
    for (key, m) in keys {
        if m.ctrl || m.alt || m.command {
            continue;
        }
        let mut flat = Vec::new();
        root.visible(&mut flat);
        if flat.is_empty() {
            return None;
        }
        let pos = nav.cursor.as_ref().and_then(|c| flat.iter().position(|(p, ..)| p == c));
        let (path, is_dir, expanded) = flat[pos.unwrap_or(0)].clone();
        let page = ((ui.available_height() / 23.0) as usize).max(1);
        let to = |k: usize| Some(flat[k.min(flat.len() - 1)].0.clone());
        let next = match key {
            egui::Key::ArrowDown => pos.map_or(to(0), |p| to(p + 1)),
            egui::Key::ArrowUp => pos.map_or(to(0), |p| to(p.saturating_sub(1))),
            egui::Key::PageDown => to(pos.unwrap_or(0) + page),
            egui::Key::PageUp => to(pos.unwrap_or(0).saturating_sub(page)),
            egui::Key::Home => to(0),
            egui::Key::End => to(flat.len() - 1),
            egui::Key::ArrowRight if is_dir && !expanded => {
                if let Some(n) = root.find_mut(&path) {
                    n.toggle();
                }
                None
            }
            egui::Key::ArrowRight if is_dir => to(pos.unwrap_or(0) + 1).filter(|c| c.starts_with(&path)),
            egui::Key::ArrowLeft if is_dir && expanded => {
                if let Some(n) = root.find_mut(&path) {
                    n.toggle();
                }
                None
            }
            // to the parent folder (not above the opened one)
            egui::Key::ArrowLeft => path.parent().filter(|p| *p != root.path.as_path()).map(Path::to_path_buf),
            egui::Key::Enter | egui::Key::Space if is_dir => {
                if let Some(n) = root.find_mut(&path) {
                    n.toggle();
                }
                None
            }
            egui::Key::Enter => {
                act = Some(TreeAction::Open(path.clone()));
                None
            }
            egui::Key::F2 => {
                act = Some(TreeAction::Rename(path.clone()));
                None
            }
            egui::Key::Delete => {
                act = Some(TreeAction::Delete(path.clone()));
                None
            }
            _ => None,
        };
        if pos.is_none() {
            nav.cursor = Some(path);
        }
        if let Some(n) = next {
            *scroll_to = Some(n.clone());
            nav.cursor = Some(n);
        }
    }
    act
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// `excluded`: for a folder other than the root, whether it is left out of the index.
fn menu(ui: &mut Ui, path: &Path, is_dir: bool, excluded: Option<bool>, act: &mut Option<TreeAction>) {
    let dir = if is_dir { path.to_path_buf() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
    let mut pick = |a: TreeAction, ui: &mut Ui| {
        *act = Some(a);
        ui.close_menu();
    };
    if ui.button(t!("Nouveau fichier…", "New file…")).clicked() {
        pick(TreeAction::NewFile(dir.clone()), ui);
    }
    if ui.button(t!("Nouveau dossier…", "New folder…")).clicked() {
        pick(TreeAction::NewFolder(dir.clone()), ui);
    }
    ui.separator();
    if ui.button(t!("Renommer…", "Rename…")).clicked() {
        pick(TreeAction::Rename(path.to_path_buf()), ui);
    }
    if ui.button(t!("Supprimer", "Delete")).clicked() {
        pick(TreeAction::Delete(path.to_path_buf()), ui);
    }
    ui.separator();
    if ui.button(t!("✳ Ajouter au contexte de Claude", "✳ Add to Claude's context")).clicked() {
        pick(TreeAction::Claude(path.to_path_buf()), ui);
    }
    if ui.button(t!("Copier le chemin", "Copy the path")).clicked() {
        pick(TreeAction::CopyPath(path.to_path_buf()), ui);
    }
    if ui.button(t!("Afficher dans le gestionnaire de fichiers", "Show in the file manager")).clicked() {
        pick(TreeAction::Reveal(path.to_path_buf()), ui);
    }
    if ui.button(t!("Ouvrir dans le terminal", "Open in the terminal")).clicked() {
        pick(TreeAction::Terminal(dir.clone()), ui);
    }
    if ui.button(t!("Rafraîchir", "Refresh")).clicked() {
        pick(TreeAction::Refresh, ui);
    }
    match excluded {
        Some(false) => {
            ui.separator();
            if ui.button(t!("Exclure de l'index", "Exclude from the index")).on_hover_text(t!("Plus dans Ctrl+P, F12 et la complétion, ni dans la recherche (Préférences → Avancé → Fichiers exclus)", "No longer in Ctrl+P, F12 and completion, nor in the search (Preferences → Advanced → Excluded files)")).clicked() {
                pick(TreeAction::Exclude(path.to_path_buf(), true), ui);
            }
        }
        Some(true) => {
            ui.separator();
            if ui.button(t!("Remettre dans l'index", "Put back in the index")).clicked() {
                pick(TreeAction::Exclude(path.to_path_buf(), false), ui);
            }
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_opens_the_folders() {
        let root = std::env::temp_dir().join(format!("florm_tree_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::create_dir_all(root.join("c")).unwrap();
        std::fs::write(root.join("a/b/f.py"), "").unwrap();
        let mut tree = Node::new(root.clone(), true);
        assert!(tree.reveal(&root.join("a/b/f.py")));
        let mut open = Vec::new();
        tree.expanded_paths(&mut open);
        assert_eq!(open, [root.join("a"), root.join("a/b")]);
        assert!(!tree.reveal(Path::new("/ailleurs/x.py")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keyboard_moves_in_the_tree() {
        let root = std::env::temp_dir().join(format!("florm_tree_keys_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/x.py"), "").unwrap();
        std::fs::write(root.join("z.txt"), "").unwrap();
        let mut tree = Node::new(root.clone(), true);
        tree.load();
        let mut nav = Nav::default();
        let mut scroll = None;
        let ctx = egui::Context::default();
        let mut press = |tree: &mut Node, nav: &mut Nav, key: egui::Key| {
            let mut input = egui::RawInput::default();
            input.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
            let mut out = None;
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| out = keyboard(ui, tree, nav, &mut scroll));
            });
            out
        };
        // first key: the first entry ("a"), then into it
        press(&mut tree, &mut nav, egui::Key::ArrowDown);
        assert_eq!(nav.cursor.as_deref(), Some(root.join("a").as_path()));
        press(&mut tree, &mut nav, egui::Key::ArrowRight);
        assert!(tree.find_mut(&root.join("a")).unwrap().expanded);
        press(&mut tree, &mut nav, egui::Key::ArrowRight);
        assert_eq!(nav.cursor.as_deref(), Some(root.join("a/b").as_path()));
        press(&mut tree, &mut nav, egui::Key::ArrowDown);
        assert_eq!(nav.cursor.as_deref(), Some(root.join("a/x.py").as_path()));
        assert!(matches!(press(&mut tree, &mut nav, egui::Key::Enter), Some(TreeAction::Open(p)) if p == root.join("a/x.py")));
        // left: to the folder, left again: folds it
        press(&mut tree, &mut nav, egui::Key::ArrowLeft);
        assert_eq!(nav.cursor.as_deref(), Some(root.join("a").as_path()));
        press(&mut tree, &mut nav, egui::Key::ArrowLeft);
        assert!(!tree.find_mut(&root.join("a")).unwrap().expanded);
        press(&mut tree, &mut nav, egui::Key::End);
        assert_eq!(nav.cursor.as_deref(), Some(root.join("z.txt").as_path()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
