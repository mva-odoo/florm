//! Soft wrap ("retour à la ligne automatique"): a line longer than the editor is shown on several
//! rows, cut after a space when there is one, its continuation rows indented like the line.
//! Only the display changes: the buffer keeps its lines.

use crate::document::Document;

/// How one long line is cut.
#[derive(Clone, Debug, PartialEq)]
pub struct Wrap {
    /// Visual column (tabs expanded) where each row after the first starts.
    pub starts: Vec<usize>,
    /// Columns of indentation in front of the continuation rows.
    pub indent: usize,
}

impl Wrap {
    /// (row of the line, column on that row) of visual column `v`.
    pub fn place(&self, v: usize) -> (usize, usize) {
        let k = self.starts.partition_point(|s| *s <= v);
        if k == 0 {
            (0, v)
        } else {
            (k, v - self.starts[k - 1] + self.indent)
        }
    }

    /// Visual column shown at column `x` of row `k` (past the end of a row: its last column).
    pub fn unplace(&self, k: usize, x: usize) -> usize {
        let k = k.min(self.starts.len());
        let (start, x) = if k == 0 { (0, x) } else { (self.starts[k - 1], x.saturating_sub(self.indent)) };
        match self.starts.get(k) {
            Some(&next) => (start + x).min(next.saturating_sub(1).max(start)),
            None => start + x,
        }
    }

    /// Columns taken on the screen by row `k`, its indentation included (not the last row).
    fn row_width(&self, k: usize) -> usize {
        let (start, end) = self.row_span(k);
        end.saturating_sub(start) + if k > 0 { self.indent } else { 0 }
    }

    /// (first visual column, end) of row `k`.
    pub fn row_span(&self, k: usize) -> (usize, usize) {
        let start = if k == 0 { 0 } else { self.starts[k - 1] };
        (start, self.starts.get(k).copied().unwrap_or(usize::MAX))
    }
}

/// The long lines of a document cut for a width, kept until the text or the width changes.
#[derive(Clone, Debug, Default)]
pub struct Wraps {
    key: (u64, usize, usize),
    /// (line, how it is cut), sorted by line; the lines that fit are not there.
    lines: Vec<(usize, Wrap)>,
}

impl Wraps {
    /// No line is cut (the soft wrap is off, or every line fits).
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn of(&self, l: usize) -> Option<&Wrap> {
        self.lines.binary_search_by_key(&l, |(n, _)| *n).ok().map(|i| &self.lines[i].1)
    }

    /// Rows of line `l` on the screen.
    pub fn rows(&self, l: usize) -> usize {
        self.of(l).map_or(1, |w| w.starts.len() + 1)
    }

    /// (line, rows added under it) of each cut line, for the row map.
    pub fn extra_rows(&self) -> Vec<(usize, usize)> {
        self.lines.iter().map(|(l, w)| (*l, w.starts.len())).collect()
    }

    /// (row of the line, column on that row) of visual column `v` of line `l`.
    pub fn place(&self, l: usize, v: usize) -> (usize, usize) {
        self.of(l).map_or((0, v), |w| w.place(v))
    }

    /// The pieces of visual columns `v0..v1` of line `l`, one per row: (row, first column on the
    /// row, end column). A span ending where a row starts ends at the end of the row before.
    pub fn spans(&self, l: usize, v0: usize, v1: usize) -> Vec<(usize, usize, usize)> {
        let Some(w) = self.of(l) else { return vec![(0, v0, v1)] };
        let (k0, x0) = w.place(v0);
        let (mut k1, mut x1) = w.place(v1);
        if v1 > v0 && k1 > k0 && x1 == w.indent && w.starts.get(k1 - 1) == Some(&v1) {
            k1 -= 1;
            x1 = w.row_width(k1);
        }
        (k0..=k1).map(|k| (k, if k == k0 { x0 } else { w.indent }, if k == k1 { x1 } else { w.row_width(k) })).collect()
    }

    /// Visual column of line `l` shown at column `x` of its row `k`.
    pub fn unplace(&self, l: usize, k: usize, x: usize) -> usize {
        self.of(l).map_or(x, |w| w.unplace(k, x))
    }

    /// Cuts the lines of `doc` longer than `width` columns (again only when needed).
    pub fn update(&mut self, doc: &Document, width: usize) {
        let key = (doc.version, width, doc.tab_size);
        if key == self.key {
            return;
        }
        self.key = key;
        self.lines.clear();
        for l in 0..doc.rope.len_lines() {
            // at most tab_size columns per char: most lines are known to fit without counting
            if doc.line_len(l) * doc.tab_size.max(1) <= width && doc.line_len(l) <= width {
                continue;
            }
            if let Some(w) = cut(doc, l, width) {
                self.lines.push((l, w));
            }
        }
    }
}

/// How line `l` is cut to fit `width` columns, None when it fits.
fn cut(doc: &Document, l: usize, width: usize) -> Option<Wrap> {
    let ts = doc.tab_size.max(1);
    let width = width.max(8);
    let lead = doc.vcol(l, doc.indent_len(l));
    // a deep indentation would leave no room: the continuation rows start at the left then
    let indent = if lead <= width / 2 { lead } else { 0 };
    let (mut starts, mut start, mut room) = (Vec::new(), 0usize, width);
    // visual column just after the last space of the row being filled
    let mut after_space = None;
    let mut v = 0;
    for ch in doc.rope.line(l).chars().take(doc.line_len(l)) {
        let w = if ch == '\t' { ts - v % ts } else { 1 };
        while v + w - start > room && v > start {
            let at = after_space.filter(|a| *a > start && *a <= v).unwrap_or(v);
            starts.push(at);
            start = at;
            room = width - indent;
            after_space = None;
        }
        v += w;
        if ch == ' ' || ch == '\t' {
            after_space = Some(v);
        }
    }
    (!starts.is_empty()).then_some(Wrap { starts, indent })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut d = Document::new(1, 4, true);
        d.rope = ropey::Rope::from_str(text);
        d.version += 1;
        d
    }

    #[test]
    fn long_lines_are_cut_after_a_space() {
        let d = doc("short\n    aaaa bbbb cccc dddd\nxxxxxxxxxxxxxxxxxxxxxxxx\n");
        let mut w = Wraps::default();
        w.update(&d, 16);
        assert_eq!(w.of(0), None);
        // "    aaaa bbbb " fits in 16, then "cccc dddd" on a row indented by 4
        assert_eq!(w.of(1), Some(&Wrap { starts: vec![14], indent: 4 }));
        // no space: cut at the width, then every 16 columns
        assert_eq!(w.of(2), Some(&Wrap { starts: vec![16], indent: 0 }));
        assert_eq!(w.extra_rows(), vec![(1, 1), (2, 1)]);
        assert_eq!(w.rows(1), 2);
        let line = w.of(1).unwrap();
        assert_eq!(line.place(4), (0, 4));
        assert_eq!(line.place(14), (1, 4));
        assert_eq!(line.place(20), (1, 10));
        assert_eq!(line.unplace(1, 4), 14);
        assert_eq!(line.unplace(1, 0), 14);
        // past the end of the first row: its last column
        assert_eq!(line.unplace(0, 40), 13);
        assert_eq!(line.row_span(1), (14, usize::MAX));
        // a selection over the cut: the end of the first row, then the start of the second
        assert_eq!(w.spans(1, 10, 16), vec![(0, 10, 14), (1, 4, 6)]);
        assert_eq!(w.spans(1, 10, 14), vec![(0, 10, 14)]);
        assert_eq!(w.spans(0, 1, 3), vec![(0, 1, 3)]);
    }

    #[test]
    fn arrows_move_by_rows_of_the_screen() {
        let mut d = doc("    aaaa bbbb cccc dddd\nend\n");
        let mut w = Wraps::default();
        w.update(&d, 16);
        d.wraps = w;
        // on "bbbb" (column 10), down: same column of the second row ("cccc dddd" from 14, at 4)
        d.sels = vec![crate::document::Sel::cursor(10)];
        d.move_sels(crate::document::Motion::Down, false);
        assert_eq!(d.sels[0].head, 20);
        d.move_sels(crate::document::Motion::Down, false);
        assert_eq!(d.line_col(d.sels[0].head), (1, 3));
        d.move_sels(crate::document::Motion::Up, false);
        d.move_sels(crate::document::Motion::Up, false);
        assert_eq!(d.sels[0].head, 10);
    }

    #[test]
    fn deep_indentation_restarts_at_the_left() {
        let d = doc(&format!("{}{}\n", " ".repeat(12), "word ".repeat(6)));
        let mut w = Wraps::default();
        w.update(&d, 20);
        let line = w.of(0).unwrap();
        assert_eq!(line.indent, 0);
        assert!(line.starts.windows(2).all(|p| p[1] - p[0] <= 20), "{:?}", line.starts);
    }
}
