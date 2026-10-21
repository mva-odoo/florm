//! Find / replace in the current file.

use super::*;

impl App {
    pub(super) fn update_find(&mut self) {
        if !self.find.open {
            if self.find.in_sel {
                self.find.in_sel = false;
                self.docs.iter_mut().for_each(|d| d.find_scope = None);
            }
            return;
        }
        let Some(doc) = self.docs.get(self.active) else { return };
        let f = &self.find;
        let scope = if f.in_sel { doc.find_scope } else { None };
        let key = (doc.uid, doc.version, f.query.clone(), f.case, f.regex, f.whole, scope);
        if self.find.key.as_ref() == Some(&key) {
            return;
        }
        if f.query.is_empty() {
            self.find.matches.clear();
            self.find.error = None;
        } else {
            match document::build_regex(&f.query, f.case, f.regex, f.whole) {
                Ok(re) => {
                    let mut m = doc.find_all(&re);
                    if let Some((s, e)) = scope {
                        m.retain(|&(a, b)| a >= s && b <= e);
                    }
                    self.find.matches = m;
                    self.find.error = None;
                }
                Err(e) => {
                    self.find.matches.clear();
                    self.find.error = Some(e);
                }
            }
        }
        self.find.key = Some(key);
    }

    pub(super) fn find_step(&mut self, forward: bool, from_start: bool) {
        self.find.open = true;
        self.update_find();
        let m = &self.find.matches;
        if m.is_empty() {
            return;
        }
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        let p = doc.primary_sel();
        let idx = if from_start {
            m.iter().position(|&(a, _)| a >= p.start()).unwrap_or(0)
        } else if forward {
            m.iter().position(|&(a, _)| a >= p.end() && (a, a) != (p.start(), p.end())).unwrap_or(0)
        } else {
            m.iter().rposition(|&(a, _)| a < p.start()).unwrap_or(m.len() - 1)
        };
        let (a, b) = m[idx];
        doc.sels = vec![Sel::range(a, b)];
        doc.primary = 0;
        doc.scroll_to_cursor = true;
        doc.center_cursor = true;
    }

    pub(super) fn find_select_all(&mut self, ctx: &egui::Context) {
        self.update_find();
        if self.find.matches.is_empty() {
            return;
        }
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.sels = self.find.matches.iter().map(|&(a, b)| Sel::range(a, b)).collect();
            doc.primary = 0;
            doc.normalize();
            doc.scroll_to_cursor = true;
        }
        self.find.open = false;
        self.focus_editor(ctx);
    }

    pub(super) fn replace_one(&mut self) {
        self.update_find();
        let f = &self.find;
        let Ok(re) = document::build_regex(&f.query, f.case, f.regex, f.whole) else { return };
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        if doc.read_only {
            return;
        }
        let p = doc.primary_sel();
        if f.matches.iter().any(|&(a, b)| a == p.start() && b == p.end()) {
            let edits = document::replacements(&doc.rope, &re, &f.replacement, f.regex, f.preserve, Some((p.start(), p.end())));
            if let Some((a, b, new)) = edits.into_iter().next() {
                let delta = new.chars().count() as isize - (b - a) as isize;
                doc.replace_range(a, b, &new);
                if let Some((s, e)) = doc.find_scope.as_mut() {
                    *e = (*e as isize + delta).max(*s as isize) as usize;
                }
                let end = doc.primary_sel().end();
                doc.sels = vec![Sel::cursor(end)];
                doc.primary = 0;
            }
        }
        self.find_step(true, true);
    }

    pub(super) fn replace_all(&mut self) {
        let f = &self.find;
        let Ok(re) = document::build_regex(&f.query, f.case, f.regex, f.whole) else { return };
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        if doc.read_only || f.query.is_empty() {
            return;
        }
        let scope = if f.in_sel { doc.find_scope } else { None };
        let edits = document::replacements(&doc.rope, &re, &f.replacement, f.regex, f.preserve, scope);
        let n = edits.len();
        if n == 0 {
            return;
        }
        let delta: isize = edits.iter().map(|(a, b, t)| t.chars().count() as isize - (b - a) as isize).sum();
        doc.replace_keep(EditKind::Other, edits);
        if let Some((s, e)) = doc.find_scope.as_mut() {
            *e = (*e as isize + delta).max(*s as isize) as usize;
        }
        let where_ = if scope.is_some() { " dans la sélection" } else { "" };
        self.set_status(format!("{n} remplacement(s){where_}"));
    }
}
