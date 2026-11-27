//! Changing the results of a search in place (Zed's multibuffer): « ✏ Modifier les résultats »
//! opens a tab with the lines found and one line around each, under a header « ▌ file:line »;
//! Ctrl+S writes each excerpt back into its file, if the file still has the lines it had then.

use super::*;
use crate::document::{join_excerpts, split_excerpts, Excerpt};

/// Lines of context kept around each line found.
const CONTEXT: usize = 1;

impl App {
    /// The results of the search in a tab where they can be changed.
    pub(super) fn open_multibuffer(&mut self) {
        let mut excerpts = Vec::new();
        for f in &self.search.files {
            // the text as Florm has it (an open tab, even unsaved), else as on disk
            let text = match self.docs.iter().find(|d| d.path.as_ref() == Some(&f.path)) {
                Some(d) => d.rope.to_string(),
                None => match std::fs::read_to_string(&f.path) {
                    Ok(t) => t,
                    Err(_) => continue,
                },
            };
            let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
            let n = lines.len().saturating_sub(usize::from(text.ends_with('\n')));
            // the lines found and their context, joined when they touch
            let mut ranges: Vec<(usize, usize)> = Vec::new();
            for h in &f.hits {
                let (a, b) = (h.line.saturating_sub(CONTEXT), (h.line + CONTEXT).min(n.saturating_sub(1)));
                match ranges.last_mut() {
                    Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
                    _ => ranges.push((a, b)),
                }
            }
            for (a, b) in ranges {
                if a > b || b >= lines.len() {
                    continue;
                }
                excerpts.push(Excerpt { path: f.path.clone(), rel: f.rel.clone(), start: a, lines: lines[a..=b].iter().map(|s| s.to_string()).collect() });
            }
        }
        if excerpts.is_empty() {
            self.set_status(t!("Aucun résultat à modifier", "No result to change"));
            return;
        }
        let syntax = self.hl.detect(Some(&excerpts[0].path), &excerpts[0].lines[0]);
        let uid = self.next_uid();
        let mut d = Document::from_text(uid, &join_excerpts(&excerpts), self.settings.tab_size, self.settings.translate_tabs_to_spaces);
        d.syntax = syntax;
        d.title_override = Some(tf!("✏ « {} »", "✏ “{}”", self.search.done.pattern));
        d.multi = Some(excerpts);
        d.sels = vec![Sel::cursor(d.rope.line_to_char(1.min(d.rope.len_lines() - 1)))];
        self.docs.push(d);
        self.active = self.docs.len() - 1;
        self.pending_focus = true;
        self.set_status(t!("Modifie les lignes, puis Ctrl+S les écrit dans leurs fichiers (sans toucher aux lignes « ▌ »)", "Change the lines, then Ctrl+S writes them into their files (do not touch the lines “▌”)"));
    }

    /// Ctrl+S in a tab of excerpts: each one written back into its file. A file changed since
    /// (its lines no longer those of the excerpt) is left as it is.
    pub(super) fn multibuffer_save(&mut self, idx: usize) -> bool {
        let Some(d) = self.docs.get(idx) else { return false };
        let Some(excerpts) = d.multi.clone() else { return false };
        let new = match split_excerpts(&d.rope.to_string(), &excerpts) {
            Ok(n) => n,
            Err(e) => {
                self.set_error(tf!("Rien n'est écrit : {e} (Ctrl+Z pour le remettre)", "Nothing is written: {e} (Ctrl+Z to put it back)"));
                return false;
            }
        };
        let uid = d.uid;
        // by file, the last excerpts first: the line numbers of the others stay right
        let mut order: Vec<usize> = (0..excerpts.len()).filter(|&k| new[k] != excerpts[k].lines).collect();
        order.sort_by(|&a, &b| excerpts[a].path.cmp(&excerpts[b].path).then(excerpts[b].start.cmp(&excerpts[a].start)));
        let open_before: HashSet<u64> = self.docs.iter().map(|d| d.uid).collect();
        let (mut done, mut skipped) = (Vec::new(), Vec::new());
        for k in order {
            let e = &excerpts[k];
            let i = match self.docs.iter().position(|d| d.path.as_ref() == Some(&e.path)) {
                Some(i) => i,
                None if self.open_path(e.path.clone()) => self.active,
                None => {
                    skipped.push(e.rel.clone());
                    continue;
                }
            };
            let f = &mut self.docs[i];
            let still = e.lines.iter().enumerate().all(|(j, l)| e.start + j < f.rope.len_lines() && f.line_text(e.start + j).trim_end_matches(['\n', '\r']) == l);
            if !still {
                skipped.push(e.rel.clone());
                continue;
            }
            let a = f.rope.line_to_char(e.start);
            let last = e.start + e.lines.len() - 1;
            let b = f.rope.line_to_char(last) + f.line_len(last);
            f.replace_keep(EditKind::Other, vec![(a, b, new[k].join("\n"))]);
            done.push(k);
        }
        // the files opened only for this: saved and closed again
        let opened: Vec<u64> = self.docs.iter().map(|d| d.uid).filter(|u| !open_before.contains(u)).collect();
        for u in opened {
            if let Some(i) = self.docs.iter().position(|d| d.uid == u) {
                if self.save_opts(i, false, false) {
                    if let Some(i) = self.docs.iter().position(|d| d.uid == u) {
                        self.close_tab(i, true);
                    }
                }
            }
        }
        // the excerpts as they are now (the next lines of a file move with a changed line count)
        let mut updated = excerpts.clone();
        for &k in &done {
            let delta = new[k].len() as isize - excerpts[k].lines.len() as isize;
            for (j, u) in updated.iter_mut().enumerate() {
                if j != k && u.path == excerpts[k].path && u.start > excerpts[k].start {
                    u.start = (u.start as isize + delta) as usize;
                }
            }
            updated[k].lines = new[k].clone();
        }
        let files: HashSet<&PathBuf> = done.iter().map(|&k| &excerpts[k].path).collect();
        let nfiles = files.len();
        if let Some(i) = self.docs.iter().position(|d| d.uid == uid) {
            let text = join_excerpts(&updated);
            let d = &mut self.docs[i];
            // the headers follow the new line numbers
            if d.rope.to_string() != text {
                d.set_text(&text);
            }
            d.multi = Some(updated);
            d.mark_saved();
            self.active = i;
        }
        if skipped.is_empty() {
            self.set_status(tf!("Écrit dans {nfiles} fichier(s) (ceux ouverts restent à enregistrer)", "Written into {nfiles} file(s) (the open ones are still to save)"));
        } else {
            skipped.dedup();
            self.set_error(tf!(
                "Écrit dans {nfiles} fichier(s) ; laissés tels quels, changés depuis la recherche : {}",
                "Written into {nfiles} file(s); left as they are, changed since the search: {}",
                skipped.join(", ")
            ));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::document::{join_excerpts, split_excerpts, Excerpt};
    use std::path::PathBuf;

    #[test]
    fn excerpts_round_trip() {
        let ex = vec![
            Excerpt { path: PathBuf::from("/p/a.py"), rel: "a.py".into(), start: 3, lines: vec!["x = 1".into(), "y = 2".into()] },
            Excerpt { path: PathBuf::from("/p/b.py"), rel: "b.py".into(), start: 0, lines: vec!["".into(), "z".into()] },
        ];
        let text = join_excerpts(&ex);
        assert_eq!(text, "▌ a.py:4\nx = 1\ny = 2\n\n▌ b.py:1\n\nz\n");
        assert_eq!(split_excerpts(&text, &ex).unwrap(), vec![ex[0].lines.clone(), ex[1].lines.clone()]);
        // a line added in the first excerpt, the second changed
        let edited = "▌ a.py:4\nx = 1\nnew\ny = 2\n\n▌ b.py:1\n\nzz\n";
        assert_eq!(split_excerpts(edited, &ex).unwrap(), vec![vec!["x = 1".to_string(), "new".into(), "y = 2".into()], vec!["".to_string(), "zz".into()]]);
        // a header touched: nothing is written
        assert!(split_excerpts("▌ a.py:5\nx = 1\n\n▌ b.py:1\nz\n", &ex).is_err());
        assert!(split_excerpts("x = 1\n▌ b.py:1\nz\n", &ex).is_err());
    }
}
