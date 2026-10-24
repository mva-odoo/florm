//! Cursor history and moves between places.

use super::*;

impl App {
    /// The bookmarks of all the open files, in a list to jump to one.
    pub(super) fn list_bookmarks(&mut self) {
        let root = self.root.clone();
        let mut items = Vec::new();
        for d in &self.docs {
            let Some(p) = &d.path else { continue };
            let rel = root.as_ref().and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p).display().to_string();
            for l in d.bookmark_lines() {
                items.push(Item { label: format!("{rel}:{}", l + 1), detail: d.line_text(l).trim().to_string(), action: Action::Location(p.clone(), l) });
            }
        }
        if items.is_empty() {
            self.set_status(t!("Aucun signet dans les fichiers ouverts (Ctrl+F2 pour en poser un)", "No bookmark in the open files (Ctrl+F2 to set one)"));
            return;
        }
        self.open_overlay_with(Mode::Bookmarks, items, String::new());
    }

    /// Records the previous place when the cursor jumps (another file, or 8+ lines away).
    pub(super) fn track_nav(&mut self) {
        let Some(d) = self.docs.get(self.active).filter(|d| d.view == document::View::Text) else { return };
        let pos = d.primary_sel().head;
        let line = d.rope.char_to_line(pos);
        let cur = NavPos { uid: d.uid, path: d.path.clone(), pos };
        if let Some((last, last_line)) = self.nav.last.take() {
            let jumped = last.uid != cur.uid || line.abs_diff(last_line) >= 8;
            if jumped && !std::mem::take(&mut self.nav.jumping) {
                let same = self.nav.back.last().is_some_and(|b| b.uid == last.uid && b.pos == last.pos);
                if !same {
                    self.nav.back.push(last);
                    if self.nav.back.len() > 100 {
                        self.nav.back.remove(0);
                    }
                }
                self.nav.fwd.clear();
            }
        }
        self.nav.jumping = false;
        self.nav.last = Some((cur, line));
    }

    /// Alt+- (back) / Alt++ (forward).
    pub(super) fn nav_go(&mut self, back: bool) {
        let here = self.docs.get(self.active).map(|d| NavPos { uid: d.uid, path: d.path.clone(), pos: d.primary_sel().head });
        loop {
            let target = if back { self.nav.back.pop() } else { self.nav.fwd.pop() };
            let Some(t) = target else {
                self.set_status(if back { t!("Pas de position précédente", "No previous position") } else { t!("Pas de position suivante", "No next position") });
                return;
            };
            // the document may have been closed: reopen it from its path
            let idx = match self.docs.iter().position(|d| d.uid == t.uid) {
                Some(i) => Some(i),
                None => t.path.clone().filter(|p| p.is_file()).and_then(|p| self.open_path(p).then_some(self.active)),
            };
            let Some(i) = idx else { continue };
            if let Some(h) = here.clone() {
                if back { self.nav.fwd.push(h) } else { self.nav.back.push(h) }
            }
            self.activate_tab(i);
            let d = &mut self.docs[i];
            let p = t.pos.min(d.len());
            let (l, c) = d.line_col(p);
            d.goto_line(l, c);
            self.nav.jumping = true;
            return;
        }
    }

    pub(super) fn open_at(&mut self, p: PathBuf, line: usize) {
        if self.open_path(p) {
            if let Some(d) = self.docs.get_mut(self.active) {
                let ind = d.indent_len(line.min(d.rope.len_lines().saturating_sub(1)));
                d.goto_line(line, ind);
            }
            self.pending_focus = true;
        }
    }

}
