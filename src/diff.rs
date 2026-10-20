//! Live line diff between the buffer and the file's HEAD version (gutter marks,
//! inline diff peek and "revert change").

use crate::document::GitMark;
use similar::{Algorithm, DiffOp, TextDiff};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Hunk {
    /// First HEAD line of `old_lines`.
    pub old_start: usize,
    /// Buffer lines [new_start, new_end) replaced the HEAD lines `old_lines`.
    pub new_start: usize,
    pub new_end: usize,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
}

impl Hunk {
    pub fn kind(&self) -> GitMark {
        if self.old_lines.is_empty() {
            GitMark::Added
        } else if self.new_lines.is_empty() {
            GitMark::Deleted
        } else {
            GitMark::Modified
        }
    }

    /// Line where the gutter mark of this hunk is drawn.
    pub fn mark_lines(&self) -> std::ops::Range<usize> {
        if self.new_lines.is_empty() {
            let l = self.new_start.saturating_sub(1);
            l..l + 1
        } else {
            self.new_start..self.new_end
        }
    }

    pub fn contains(&self, line: usize) -> bool {
        self.mark_lines().contains(&line)
    }
}

pub fn compute(base: &str, current: &str) -> Vec<Hunk> {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .timeout(Duration::from_millis(100))
        .diff_lines(base, current);
    let (old, new) = (diff.old_slices(), diff.new_slices());
    let mut hunks = Vec::new();
    let mut cur: Option<(usize, usize, usize, usize)> = None; // old start/end, new start/end
    let mut flush = |cur: &mut Option<(usize, usize, usize, usize)>| {
        if let Some((os, oe, ns, ne)) = cur.take() {
            hunks.push(Hunk {
                old_start: os,
                new_start: ns,
                new_end: ne,
                old_lines: old[os..oe].iter().map(|s| s.to_string()).collect(),
                new_lines: new[ns..ne].iter().map(|s| s.to_string()).collect(),
            });
        }
    };
    for op in diff.ops() {
        if let DiffOp::Equal { .. } = op {
            flush(&mut cur);
            continue;
        }
        let (o, n) = (op.old_range(), op.new_range());
        cur = Some(match cur {
            Some((os, _, ns, _)) => (os, o.end, ns, n.end),
            None => (o.start, o.end, n.start, n.end),
        });
    }
    flush(&mut cur);
    hunks
}

/// Display rows of the buffer lines: deleted HEAD lines shown inline ("phantom" rows placed above
/// the buffer line where they used to be) add rows, folded blocks remove theirs.
#[derive(Default, Clone)]
pub struct RowMap {
    /// (buffer line the block sits above, number of phantom rows), sorted by line
    blocks: Vec<(usize, usize)>,
    /// cum[i] = phantom rows in blocks[..=i]
    cum: Vec<usize>,
    /// hidden (folded) lines, inclusive ranges, sorted and disjoint
    hidden: Vec<(usize, usize)>,
    /// hcum[i] = hidden lines in hidden[..=i]
    hcum: Vec<usize>,
}

impl RowMap {
    pub fn with_folds(hunks: &[Hunk], hidden: &[(usize, usize)]) -> Self {
        let is_hidden = |l: usize| hidden.iter().any(|&(s, e)| s <= l && l <= e);
        let blocks: Vec<(usize, usize)> = hunks
            .iter()
            .filter(|h| !h.old_lines.is_empty() && !is_hidden(h.new_start))
            .map(|h| (h.new_start, h.old_lines.len()))
            .collect();
        let cum = blocks
            .iter()
            .scan(0, |acc, b| {
                *acc += b.1;
                Some(*acc)
            })
            .collect();
        let hcum = hidden
            .iter()
            .scan(0, |acc, (s, e)| {
                *acc += e - s + 1;
                Some(*acc)
            })
            .collect();
        RowMap { blocks, cum, hidden: hidden.to_vec(), hcum }
    }

    /// Phantom rows minus hidden lines (display rows = buffer lines + this).
    pub fn total(&self) -> isize {
        self.cum.last().copied().unwrap_or(0) as isize - self.hcum.last().copied().unwrap_or(0) as isize
    }

    pub fn rows(&self, nlines: usize) -> usize {
        (nlines as isize + self.total()).max(1) as usize
    }

    /// Folded block containing line `l`: its range.
    fn fold_of(&self, l: usize) -> Option<(usize, usize)> {
        let i = self.hidden.partition_point(|h| h.1 < l);
        self.hidden.get(i).copied().filter(|h| h.0 <= l)
    }

    pub fn is_hidden(&self, l: usize) -> bool {
        self.fold_of(l).is_some()
    }

    /// Display row of buffer line `l` (a hidden line is on the row of its folded block's first line).
    pub fn row(&self, l: usize) -> usize {
        if let Some((s, _)) = self.fold_of(l) {
            return self.row(s.saturating_sub(1));
        }
        let i = self.blocks.partition_point(|b| b.0 <= l);
        let phantom = if i == 0 { 0 } else { self.cum[i - 1] };
        let h = self.hidden.partition_point(|h| h.1 < l);
        let hidden = if h == 0 { 0 } else { self.hcum[h - 1] };
        l + phantom - hidden
    }

    /// Buffer line shown at display row `r` (a phantom row maps to the line above it).
    pub fn line_at_row(&self, r: usize, nlines: usize) -> usize {
        let (mut lo, mut hi) = (0, nlines.saturating_sub(1));
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            if self.row(mid) <= r {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        match self.fold_of(lo) {
            Some((s, _)) => s.saturating_sub(1),
            None => lo,
        }
    }

    /// (line, rows) of each phantom block.
    pub fn blocks(&self) -> &[(usize, usize)] {
        &self.blocks
    }
}

pub fn marks(hunks: &[Hunk]) -> HashMap<usize, GitMark> {
    let mut m = HashMap::new();
    for h in hunks {
        let kind = h.kind();
        for l in h.mark_lines() {
            if kind != GitMark::Deleted || !m.contains_key(&l) {
                m.insert(l, kind);
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_map() {
        let h = compute("a\nb\nc\nd\n", "a\nB\nc\nd\n");
        let rm = RowMap::with_folds(&h, &[]);
        assert_eq!(rm.total(), 1);
        assert_eq!((rm.row(0), rm.row(1), rm.row(2)), (0, 2, 3)); // old "b" shown at row 1
        assert_eq!(rm.line_at_row(1, 5), 0);
        assert_eq!(rm.line_at_row(2, 5), 1);
        assert_eq!(rm.line_at_row(3, 5), 2);
    }

    #[test]
    fn folded_rows() {
        // lines 2..=4 folded under line 1
        let rm = RowMap::with_folds(&[], &[(2, 4)]);
        assert_eq!(rm.total(), -3);
        assert_eq!(rm.rows(8), 5);
        assert_eq!((rm.row(0), rm.row(1), rm.row(3), rm.row(5), rm.row(7)), (0, 1, 1, 2, 4));
        assert!(rm.is_hidden(3) && !rm.is_hidden(5));
        assert_eq!(rm.line_at_row(1, 8), 1);
        assert_eq!(rm.line_at_row(2, 8), 5);
        assert_eq!(rm.line_at_row(4, 8), 7);
        // with an inline diff block above line 6
        let h = compute("a\nb\nc\nd\ne\nf\nX\ng\n", "a\nb\nc\nd\ne\nf\ng\n");
        let rm = RowMap::with_folds(&h, &[(2, 4)]);
        assert_eq!(rm.row(6), 6 + 1 - 3);
    }

    #[test]
    fn hunks_kinds() {
        let base = "a\nb\nc\nd\n";
        let h = compute(base, "a\nB\nc\nd\ne\n");
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].kind(), GitMark::Modified);
        assert_eq!((h[0].new_start, h[0].new_end), (1, 2));
        assert_eq!(h[0].old_lines, vec!["b\n"]);
        assert_eq!(h[1].kind(), GitMark::Added);
        let h = compute(base, "a\nd\n");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].kind(), GitMark::Deleted);
        assert!(h[0].contains(0));
    }
}
