//! Folder tree, loaded lazily (a directory is read only when expanded).

use crate::icons;
use eframe::egui::{self, Align2, Color32, CursorIcon, DragAndDrop, FontId, Id, Pos2, Rect, RichText, Sense, Stroke, Ui};
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
    Refresh,
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

/// `scroll_to`: entry to bring into view (cleared once shown).
pub fn show(
    ui: &mut Ui,
    root: &mut Node,
    active: Option<&Path>,
    git: &HashMap<PathBuf, char>,
    scroll_to: &mut Option<PathBuf>,
) -> Option<TreeAction> {
    let mut act = None;
    if root.children.is_none() {
        root.load();
        root.expanded = true;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let r = ui.add(egui::Label::new(RichText::new(root.name.to_uppercase()).small().strong()).sense(Sense::click()));
        drop_target(ui, &r, &root.path, &mut act);
        r.context_menu(|ui| menu(ui, &root.path, true, &mut act));
        if let Some(children) = root.children.as_mut() {
            for c in children {
                node_ui(ui, c, 0, active, git, &mut act, scroll_to);
            }
        }
    });
    if let Some(p) = DragAndDrop::payload::<PathBuf>(ui.ctx()) {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), Id::new("florm_dnd"), |ui| ui.label(format!("Déplacer {name}")));
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
    act: &mut Option<TreeAction>,
    scroll_to: &mut Option<PathBuf>,
) {
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
    let painter = ui.painter().with_clip_rect(rect);
    let cy = rect.center().y;
    let mut x = rect.left() + 4.0 + depth as f32 * 14.0;
    if node.is_dir {
        icons::paint_chevron(&painter, Pos2::new(x + 5.0, cy), node.expanded, v.weak_text_color());
    }
    x += 14.0;
    let icon = Rect::from_center_size(Pos2::new(x + 8.0, cy), egui::vec2(16.0, 16.0));
    if node.is_dir {
        let folder = mix(v.hyperlink_color, v.weak_text_color(), 0.45);
        icons::paint_folder(&painter, icon, node.expanded, folder, bg);
    } else {
        icons::paint_file(&painter, icon, &node.name, bg, light);
    }
    x += 23.0;
    let color = git
        .get(&node.path)
        .and_then(|c| git_color(*c))
        .unwrap_or(if selected { v.strong_text_color() } else { v.text_color() });
    painter.text(Pos2::new(x, cy), Align2::LEFT_CENTER, &node.name, FontId::proportional(13.5), color);
    if r.drag_started() {
        DragAndDrop::set_payload(ui.ctx(), node.path.clone());
    }
    let target = if node.is_dir { node.path.clone() } else { node.path.parent().map(Path::to_path_buf).unwrap_or_default() };
    drop_target(ui, &r, &target, act);
    if r.clicked() {
        if node.is_dir {
            node.expanded = !node.expanded;
            if node.expanded && node.children.is_none() {
                node.load();
            }
        } else {
            *act = Some(TreeAction::Open(node.path.clone()));
        }
    }
    let r = r.on_hover_text_at_pointer(node.path.display().to_string());
    r.context_menu(|ui| menu(ui, &node.path, node.is_dir, act));
    if node.is_dir && node.expanded {
        if let Some(children) = node.children.as_mut() {
            for c in children {
                node_ui(ui, c, depth + 1, active, git, act, scroll_to);
            }
        }
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn menu(ui: &mut Ui, path: &Path, is_dir: bool, act: &mut Option<TreeAction>) {
    let dir = if is_dir { path.to_path_buf() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
    let mut pick = |a: TreeAction, ui: &mut Ui| {
        *act = Some(a);
        ui.close_menu();
    };
    if ui.button("Nouveau fichier…").clicked() {
        pick(TreeAction::NewFile(dir.clone()), ui);
    }
    if ui.button("Nouveau dossier…").clicked() {
        pick(TreeAction::NewFolder(dir.clone()), ui);
    }
    ui.separator();
    if ui.button("Renommer…").clicked() {
        pick(TreeAction::Rename(path.to_path_buf()), ui);
    }
    if ui.button("Supprimer").clicked() {
        pick(TreeAction::Delete(path.to_path_buf()), ui);
    }
    ui.separator();
    if ui.button("Copier le chemin").clicked() {
        pick(TreeAction::CopyPath(path.to_path_buf()), ui);
    }
    if ui.button("Afficher dans le gestionnaire de fichiers").clicked() {
        pick(TreeAction::Reveal(path.to_path_buf()), ui);
    }
    if ui.button("Ouvrir dans le terminal").clicked() {
        pick(TreeAction::Terminal(dir.clone()), ui);
    }
    if ui.button("Rafraîchir").clicked() {
        pick(TreeAction::Refresh, ui);
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
}
