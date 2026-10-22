//! Drop-down lists you can type in: with many repositories or branches, a few letters find the
//! right one (fuzzy, like « Aller au fichier »), ↑ ↓ and Entrée choose it without the mouse.

use crate::fuzzy;
use eframe::egui::{self, Id, Key, RichText};

/// Indexes of the `items` matching `filter`, best first (all of them, in order, without filter).
pub fn matches(filter: &str, items: &[String]) -> Vec<usize> {
    if filter.trim().is_empty() {
        return (0..items.len()).collect();
    }
    let mut found: Vec<(i32, usize)> = items.iter().enumerate().filter_map(|(i, s)| fuzzy::score(filter, s).map(|sc| (sc, i))).collect();
    // stable: equal scores keep the order of the list
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, i)| i).collect()
}

/// State of the list of popup `id`: (filter, highlighted row, shown at the last frame).
fn state(id: Id) -> Id {
    id.with("pick")
}

/// To call when the popup is not shown: the next opening starts with an empty filter.
pub fn closed(ui: &egui::Ui, id: Id) {
    ui.data_mut(|d| d.get_temp_mut_or_default::<(String, usize, bool)>(state(id)).2 = false);
}

/// Inside an open popup: a filter field (focused when the popup opens) and the items matching it.
/// `id`: the popup's own; `current`: the item selected now, marked. Returns the item chosen
/// (click, or Entrée on the highlighted one); the caller closes the popup.
pub fn list(ui: &mut egui::Ui, id: Id, items: &[String], current: Option<usize>) -> Option<usize> {
    let state = state(id);
    let (mut filter, mut hi, shown_before) = ui.data(|d| d.get_temp::<(String, usize, bool)>(state)).unwrap_or_default();
    let opening = !shown_before;
    if opening {
        filter.clear();
    }
    let edit = ui.add(egui::TextEdit::singleline(&mut filter).hint_text("🔍 taper pour chercher…").desired_width(f32::INFINITY));
    if opening {
        edit.request_focus();
    }
    let shown = matches(&filter, items);
    if opening {
        hi = current.and_then(|c| shown.iter().position(|&i| i == c)).unwrap_or(0);
    } else if edit.changed() {
        hi = 0;
    }
    let (down, up, enter) = ui.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::Enter)));
    if down {
        hi += 1;
    }
    if up {
        hi = hi.saturating_sub(1);
    }
    hi = hi.min(shown.len().saturating_sub(1));
    let mut chosen = None;
    if shown.is_empty() {
        ui.label(RichText::new("Aucun résultat").weak());
    }
    egui::ScrollArea::vertical().id_salt(state.with("scroll")).max_height(340.0).auto_shrink([false, true]).show(ui, |ui| {
        for (k, &i) in shown.iter().enumerate().take(500) {
            let text = if Some(i) == current { RichText::new(format!("● {}", items[i])).strong() } else { RichText::new(&items[i]) };
            let r = ui.selectable_label(k == hi, text);
            if k == hi && (up || down || opening) {
                r.scroll_to_me(None);
            }
            if r.clicked() {
                chosen = Some(i);
            }
        }
    });
    if enter {
        chosen = chosen.or(shown.get(hi).copied());
    }
    ui.data_mut(|d| d.insert_temp(state, (filter, hi, true)));
    chosen
}

/// A button showing `selected` whose list, below it, can be filtered by typing; returns the index
/// (in `items`) of the item chosen.
pub fn combo(ui: &mut egui::Ui, id_salt: impl std::hash::Hash, selected: impl Into<egui::WidgetText>, items: &[String], current: Option<usize>) -> Option<usize> {
    let popup = ui.make_persistent_id(id_salt);
    let r = ui.add(egui::Button::new(selected).shortcut_text("▾"));
    if r.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup));
    }
    let width = r.rect.width().max(240.0);
    // clicks in the filter field must not close it
    let shown = egui::popup_below_widget(ui, popup, &r, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(width);
        list(ui, popup, items, current)
    });
    let chosen = shown.flatten();
    if chosen.is_some() {
        ui.memory_mut(|m| m.close_popup());
    }
    if shown.is_none() || chosen.is_some() {
        closed(ui, popup);
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_order() {
        let items: Vec<String> = ["17.0", "saas-17.2", "17.0-fix-invoice-mva", "master"].iter().map(|s| s.to_string()).collect();
        assert_eq!(matches("", &items), vec![0, 1, 2, 3]);
        assert_eq!(matches("  ", &items), vec![0, 1, 2, 3]);
        assert_eq!(matches("mst", &items), vec![3]);
        assert_eq!(matches("fixinv", &items), vec![2]);
        assert!(matches("zzz", &items).is_empty());
        // the best match first
        assert_eq!(matches("saas", &items)[0], 1);
    }
}
