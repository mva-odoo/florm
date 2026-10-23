//! Commit graph: `git log` parsed into rows with lanes, drawn like Git Graph.

use crate::git;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub refs: Vec<String>,
    pub subject: String,
}

/// Lanes touched by one row, in lane indices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub col: usize,
    /// Lanes ending on this commit's dot (children above pointing to it).
    pub merge_in: Vec<usize>,
    /// Lanes going straight through the row.
    pub pass: Vec<usize>,
    /// Lanes leaving the dot towards the parents.
    pub out: Vec<usize>,
}

pub struct Graph {
    pub commits: Vec<Commit>,
    pub rows: Vec<Row>,
    pub lanes: usize,
    /// Local branch names (to tell them from remote refs in the commit labels).
    pub local: Vec<String>,
    /// A rebase is stopped in the repository (e.g. a commit split left unfinished).
    pub rebasing: bool,
}

#[derive(Clone, Debug)]
pub struct Details {
    pub author: String,
    pub date: String,
    pub message: String,
    /// (status letter, path, lines added / removed — None for binary files)
    pub files: Vec<(char, String, Option<(Option<u32>, Option<u32>)>)>,
}

fn free_slot(lanes: &mut Vec<Option<String>>, except: usize) -> usize {
    match lanes.iter().enumerate().position(|(i, l)| l.is_none() && i != except) {
        Some(i) => i,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

pub fn layout(commits: &[Commit]) -> (Vec<Row>, usize) {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());
    let mut max = 0;
    for c in commits {
        let incoming: Vec<usize> =
            lanes.iter().enumerate().filter(|(_, l)| l.as_deref() == Some(c.hash.as_str())).map(|(i, _)| i).collect();
        let col = match incoming.first() {
            Some(&i) => i,
            None => free_slot(&mut lanes, usize::MAX),
        };
        for &i in &incoming {
            lanes[i] = None;
        }
        let pass: Vec<usize> = lanes.iter().enumerate().filter(|(_, l)| l.is_some()).map(|(i, _)| i).collect();
        let mut out = Vec::new();
        for (k, p) in c.parents.iter().enumerate() {
            // The first parent keeps this lane (several lanes may then wait for the same
            // commit and converge on it, like Git Graph). Merged parents join an existing lane.
            if k == 0 && lanes[col].is_none() {
                lanes[col] = Some(p.clone());
                out.push(col);
                continue;
            }
            if let Some(j) = lanes.iter().position(|l| l.as_deref() == Some(p.as_str())) {
                out.push(j);
                continue;
            }
            let slot = free_slot(&mut lanes, col);
            lanes[slot] = Some(p.clone());
            out.push(slot);
        }
        while lanes.last().is_some_and(|l| l.is_none()) {
            lanes.pop();
        }
        max = max.max(lanes.len()).max(col + 1);
        rows.push(Row { col, merge_in: incoming, pass, out });
    }
    (rows, max)
}

pub fn load(repo: &Path, all: bool, limit: usize) -> Result<Graph, String> {
    let n = limit.to_string();
    let mut args = vec![
        "log",
        "--topo-order",
        "--date=format:%Y-%m-%d %H:%M",
        "--format=%H%x1f%P%x1f%an%x1f%ad%x1f%D%x1f%s",
        "-n",
        &n,
    ];
    if all {
        args.push("--all");
    }
    let out = git::stdout(repo, &args)?;
    let commits: Vec<Commit> = out
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\x1f').collect();
            if f.len() < 6 {
                return None;
            }
            Some(Commit {
                hash: f[0].to_string(),
                parents: f[1].split_whitespace().map(String::from).collect(),
                author: f[2].to_string(),
                date: f[3].to_string(),
                refs: f[4].split(", ").filter(|r| !r.is_empty()).map(String::from).collect(),
                subject: f[5].to_string(),
            })
        })
        .collect();
    let (rows, lanes) = layout(&commits);
    let local = git::stdout(repo, &["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .map(|s| s.lines().map(String::from).collect())
        .unwrap_or_default();
    Ok(Graph { commits, rows, lanes, local, rebasing: crate::split::rebase_in_progress(repo) })
}

pub fn details(repo: &Path, hash: &str) -> Result<Details, String> {
    let head = git::stdout(repo, &["show", "-s", "--date=format:%Y-%m-%d %H:%M", "--format=%an <%ae>%x1f%ad%x1f%B", hash])?;
    let mut f = head.splitn(3, '\x1f');
    let author = f.next().unwrap_or("").to_string();
    let date = f.next().unwrap_or("").to_string();
    let message = f.next().unwrap_or("").trim().to_string();
    let q = ["-c", "core.quotePath=false"];
    let mut names = git::stdout(repo, &[&q[..], &["diff-tree", "-r", "--root", "--no-commit-id", "--no-renames", "--name-status", hash]].concat())?;
    let mut stats = git::numstat(repo, &[&q[..], &["diff-tree", "-r", "--root", "--no-commit-id", "--no-renames", "--numstat", hash]].concat());
    if names.trim().is_empty() {
        // merge commit: changes brought by the merge, against the first parent
        let parent = format!("{hash}^1");
        names = git::stdout(repo, &[&q[..], &["diff", "--no-renames", "--name-status", &parent, hash]].concat()).unwrap_or_default();
        stats = git::numstat(repo, &[&q[..], &["diff", "--no-renames", "--numstat", &parent, hash]].concat());
    }
    let files = names
        .lines()
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let st = parts.next()?.chars().next()?;
            let path = parts.last()?.to_string();
            let s = stats.get(&path).copied();
            Some((st, path, s))
        })
        .collect();
    Ok(Details { author, date, message, files })
}

/// Files that differ between two commits / branches (`git diff a b`), with their +/- lines.
pub fn compare(repo: &Path, a: &str, b: &str) -> Result<Details, String> {
    let q = ["-c", "core.quotePath=false"];
    let names = git::stdout(repo, &[&q[..], &["diff", "--no-renames", "--name-status", a, b]].concat())?;
    let stats = git::numstat(repo, &[&q[..], &["diff", "--no-renames", "--numstat", a, b]].concat());
    let files = names
        .lines()
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let st = parts.next()?.chars().next()?;
            let path = parts.last()?.to_string();
            let s = stats.get(&path).copied();
            Some((st, path, s))
        })
        .collect();
    let count = git::stdout(repo, &["rev-list", "--left-right", "--count", &format!("{a}...{b}")]).unwrap_or_default();
    let mut it = count.split_whitespace();
    let message = match (it.next(), it.next()) {
        (Some(l), Some(r)) => tf!("{r} commit(s) dans la deuxième qui ne sont pas dans la première, {l} dans l'autre sens", "{r} commit(s) in the second that are not in the first, {l} the other way"),
        _ => String::new(),
    };
    Ok(Details { author: String::new(), date: String::new(), message, files })
}

const LANE_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x61, 0xaf, 0xef),
    Color32::from_rgb(0x98, 0xc3, 0x79),
    Color32::from_rgb(0xe5, 0xc0, 0x7b),
    Color32::from_rgb(0xc6, 0x78, 0xdd),
    Color32::from_rgb(0xe0, 0x6c, 0x75),
    Color32::from_rgb(0x56, 0xb6, 0xc2),
    Color32::from_rgb(0xd1, 0x9a, 0x66),
    Color32::from_rgb(0xab, 0xb2, 0xbf),
];

pub fn lane_color(l: usize) -> Color32 {
    LANE_COLORS[l % LANE_COLORS.len()]
}

pub const LANE_W: f32 = 14.0;
const MAX_DRAWN_LANES: usize = 24;

/// Paints one row of the graph in `rect` (graph column only).
pub fn paint_row(ui: &Ui, rect: Rect, row: &Row, with_lines: bool) {
    let p = ui.painter();
    let x = |l: usize| rect.left() + LANE_W * l.min(MAX_DRAWN_LANES) as f32 + LANE_W / 2.0;
    let (top, mid, bottom) = (rect.top(), rect.center().y, rect.bottom());
    if with_lines {
        let line = |a: Pos2, b: Pos2, l: usize| {
            p.line_segment([a, b], Stroke::new(2.0_f32, lane_color(l)));
        };
        for &l in &row.pass {
            line(Pos2::new(x(l), top), Pos2::new(x(l), bottom), l);
        }
        for &l in &row.merge_in {
            line(Pos2::new(x(l), top), Pos2::new(x(row.col), mid), l);
        }
        for &l in &row.out {
            let c = if l == row.col { row.col } else { l };
            line(Pos2::new(x(row.col), mid), Pos2::new(x(l), bottom), c);
        }
    }
    p.circle(Pos2::new(x(row.col), mid), 4.0, lane_color(row.col), Stroke::new(1.5_f32, ui.visuals().panel_fill));
}

/// Small rounded badges for branch / tag names; returns the x after them.
pub fn paint_refs(ui: &Ui, mut x: f32, y: f32, refs: &[String], font: &FontId) -> f32 {
    let p = ui.painter();
    for r in refs {
        let (text, color) = if let Some(b) = r.strip_prefix("HEAD -> ") {
            (b.to_string(), Color32::from_rgb(0x2f, 0x80, 0x4a))
        } else if let Some(t) = r.strip_prefix("tag: ") {
            (t.to_string(), Color32::from_rgb(0x8a, 0x6d, 0x1f))
        } else if r.contains('/') {
            (r.clone(), Color32::from_rgb(0x6b, 0x3b, 0x44))
        } else {
            (r.clone(), Color32::from_rgb(0x2b, 0x5b, 0x8a))
        };
        let galley = p.layout_no_wrap(text, font.clone(), Color32::WHITE);
        let r = Rect::from_min_size(Pos2::new(x, y - galley.size().y / 2.0 - 1.0), galley.size() + egui::vec2(10.0, 2.0));
        p.rect_filled(r, 4.0, color);
        p.galley(Pos2::new(x + 5.0, y - galley.size().y / 2.0), galley, Color32::WHITE);
        x = r.right() + 4.0;
    }
    x
}

/// One clickable list row: graph, refs, subject, author/date/hash on the right.
pub fn row_ui(ui: &mut Ui, c: &Commit, row: &Row, graph_w: f32, selected: bool, with_lines: bool) -> egui::Response {
    let h = 22.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    paint_row(ui, Rect::from_min_size(rect.min, egui::vec2(graph_w, h)), row, with_lines);
    let font = FontId::proportional(13.0);
    let small = FontId::proportional(11.0);
    let weak = ui.visuals().weak_text_color();
    let right = format!("{}   {}   {}", c.author, c.date, &c.hash[..c.hash.len().min(8)]);
    let right_galley = ui.painter().layout_no_wrap(right, small.clone(), weak);
    let right_w = right_galley.size().x + 12.0;
    ui.painter().galley(
        Pos2::new(rect.right() - right_w + 4.0, rect.center().y - right_galley.size().y / 2.0),
        right_galley,
        weak,
    );
    let clip = Rect::from_min_max(rect.min, Pos2::new(rect.right() - right_w, rect.bottom()));
    let painter = ui.painter().with_clip_rect(clip);
    let x = paint_refs(ui, rect.left() + graph_w + 6.0, rect.center().y, &c.refs, &small);
    painter.text(Pos2::new(x + 2.0, rect.center().y), Align2::LEFT_CENTER, &c.subject, font, ui.visuals().text_color());
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_hunk_or_a_file() {
        let d = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n@@ -9 +9 @@\n-c\n+d\ndiff --git a/y b/y\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-e\n+f";
        assert_eq!(without(d, 3, false), "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -9 +9 @@\n-c\n+d\ndiff --git a/y b/y\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-e\n+f");
        assert_eq!(without(d, 0, true), "diff --git a/y b/y\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-e\n+f");
        // the only hunk of y: the file goes
        assert_eq!(without(d, 12, false), "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n@@ -9 +9 @@\n-c\n+d");
        assert_eq!(without(&without(d, 0, true), 0, true), "(aucune différence)");
    }

    fn c(h: &str, parents: &[&str]) -> Commit {
        Commit {
            hash: h.into(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            author: String::new(),
            date: String::new(),
            refs: Vec::new(),
            subject: String::new(),
        }
    }

    #[test]
    fn lanes_for_branch_and_merge() {
        // m merges b into a; a and b both come from root r.
        let commits = vec![c("m", &["a", "b"]), c("b", &["r"]), c("a", &["r"]), c("r", &[])];
        let (rows, lanes) = layout(&commits);
        assert_eq!(lanes, 2);
        assert_eq!(rows[0], Row { col: 0, merge_in: vec![], pass: vec![], out: vec![0, 1] });
        assert_eq!(rows[1].col, 1); // b on the second lane
        assert_eq!(rows[1].pass, vec![0]);
        assert_eq!(rows[2].col, 0); // a continues the first lane
        assert_eq!(rows[2].out, vec![0]);
        assert_eq!((rows[3].col, rows[3].merge_in.clone()), (0, vec![0, 1])); // both lanes converge on r
    }
}

/// One line of a unified diff, with its line numbers before / after.
pub struct DiffLine {
    pub kind: DiffKind,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub text: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffKind {
    Context,
    Added,
    Removed,
    Hunk,
    Meta,
}

pub fn parse_diff(text: &str) -> Vec<DiffLine> {
    let (mut old, mut new, mut in_hunk) = (0usize, 0usize, false);
    let num = |s: &str| s.split(',').next().and_then(|n| n.trim_start_matches(['-', '+']).parse::<usize>().ok()).unwrap_or(1);
    text.lines()
        .map(|l| {
            let (kind, o, n) = if l.starts_with("@@") {
                let mut parts = l.split_whitespace().skip(1);
                old = parts.next().map(num).unwrap_or(1);
                new = parts.next().map(num).unwrap_or(1);
                in_hunk = true;
                (DiffKind::Hunk, None, None)
            } else if l.starts_with("diff --git") || l.starts_with("commit ") {
                in_hunk = false;
                (DiffKind::Meta, None, None)
            } else if !in_hunk {
                (DiffKind::Meta, None, None)
            } else if l.starts_with('+') {
                new += 1;
                (DiffKind::Added, None, Some(new - 1))
            } else if l.starts_with('-') {
                old += 1;
                (DiffKind::Removed, Some(old - 1), None)
            } else if l.starts_with('\\') {
                (DiffKind::Meta, None, None)
            } else {
                old += 1;
                new += 1;
                (DiffKind::Context, Some(old - 1), Some(new - 1))
            };
            DiffLine { kind, old: o, new: n, text: l.replace('\t', "    ") }
        })
        .collect()
}

/// What a button of a diff does (on a hunk, or on the header of a file).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffAct {
    Open,
    Stage,
    Unstage,
    /// Remove the change from the working tree (asks for confirmation).
    Discard,
    /// Undo, in the working tree, what a commit changed.
    Revert,
}

/// A button of a diff: label, tooltip, action.
pub type DiffButton<'a> = (&'a str, &'a str, DiffAct);

/// What was clicked in a diff: a line (double click), a button of the hunk starting at a line,
/// or a button of the header of the file starting at a line (stacked view).
#[derive(Clone, Copy)]
pub enum DiffClick {
    Line(usize),
    Hunk(usize, DiffAct),
    File(usize, DiffAct),
}

/// A file of a diff with several files.
pub struct DiffFile {
    /// Index of its "diff --git" line.
    pub start: usize,
    pub path: String,
    /// 'A' new, 'D' deleted, 'R' renamed, 'M' modified.
    pub status: char,
    pub adds: usize,
    pub dels: usize,
}

pub fn diff_files(lines: &[DiffLine]) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some(rest) = l.text.strip_prefix("diff --git ") {
            let path = rest.split_once(" b/").map_or(rest, |(_, b)| b).to_string();
            files.push(DiffFile { start: i, path, status: 'M', adds: 0, dels: 0 });
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        match l.kind {
            DiffKind::Added => f.adds += 1,
            DiffKind::Removed => f.dels += 1,
            DiffKind::Meta if l.text.starts_with("new file mode") || l.text == "--- /dev/null" => f.status = 'A',
            DiffKind::Meta if l.text.starts_with("deleted file mode") => f.status = 'D',
            DiffKind::Meta if l.text.starts_with("rename from") => f.status = 'R',
            _ => {}
        }
    }
    files
}

/// Lines of a file section that are only git's bookkeeping (shown in the file header instead).
fn is_file_header(text: &str) -> bool {
    ["diff --git", "index ", "--- ", "+++ ", "new file mode", "deleted file mode", "old mode", "new mode", "similarity index", "rename from", "rename to"]
        .iter()
        .any(|p| text.starts_with(p))
}

/// Move in a stacked diff (arrow keys of the graph view).
#[derive(Clone, Copy)]
pub enum FileJump {
    Next,
    Prev,
    /// To the file with this index in `files`.
    To(usize),
    PageDown,
    PageUp,
}

enum StackRow {
    Gap,
    Header(usize),
    Line(usize),
    /// What is shown under line `i` (comments of a pull request…).
    Note(usize),
}

/// Blocks shown under some lines of a stacked diff, like the comments on GitHub.
pub struct Notes<'a> {
    /// Indexes (in the diff lines) of the lines with a block under them.
    pub at: &'a std::collections::HashSet<usize>,
    /// Draws the block of a line.
    pub show: &'a mut dyn FnMut(&mut Ui, usize),
}

/// Every file of a diff one under the other, like the "Files changed" page of a GitHub pull
/// request: a header per file (status, path, +/−, `file_buttons`, click to fold), then its hunks
/// (with `hunk_buttons` on each "@@" line).
#[allow(clippy::too_many_arguments)]
pub fn stacked_diff_view(
    ui: &mut Ui,
    lines: &[DiffLine],
    files: &[DiffFile],
    collapsed: &mut std::collections::HashSet<usize>,
    id: &str,
    hunk_buttons: &[DiffButton],
    file_buttons: &[DiffButton],
    jump: Option<FileJump>,
    mut notes: Option<Notes>,
) -> Option<DiffClick> {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let (row_h, cw) = ui.fonts(|f| (f.row_height(&font) + 2.0, crate::editor_view::char_width(f, &font)));
    let row_h = row_h.max(24.0);
    // rows: lines before the first file (messages), then each file
    let mut rows: Vec<StackRow> = Vec::new();
    let first = files.first().map_or(lines.len(), |f| f.start);
    rows.extend((0..first).filter(|&i| !lines[i].text.trim().is_empty()).map(StackRow::Line));
    for (k, f) in files.iter().enumerate() {
        if k > 0 || first > 0 {
            rows.push(StackRow::Gap);
        }
        rows.push(StackRow::Header(k));
        if collapsed.contains(&f.start) {
            continue;
        }
        let end = files.get(k + 1).map_or(lines.len(), |n| n.start);
        let mut in_hunk = false;
        for i in f.start + 1..end {
            in_hunk |= lines[i].kind == DiffKind::Hunk;
            if !in_hunk && is_file_header(&lines[i].text) {
                continue;
            }
            rows.push(StackRow::Line(i));
            if notes.as_ref().is_some_and(|n| n.at.contains(&i)) {
                rows.push(StackRow::Note(i));
            }
        }
    }
    // the height of a block is the one it took at the last frame
    let note_id = |i: usize| egui::Id::new(id).with(("note", i));
    let heights: Vec<f32> = rows
        .iter()
        .map(|row| match row {
            StackRow::Note(i) => ui.ctx().data(|d| d.get_temp::<f32>(note_id(*i))).unwrap_or(120.0),
            _ => row_h,
        })
        .collect();
    let mut tops = Vec::with_capacity(rows.len() + 1);
    let mut y = 0.0;
    for h in &heights {
        tops.push(y);
        y += h;
    }
    let total = y;
    let max_len = lines.iter().map(|l| l.text.chars().count()).max().unwrap_or(0);
    let nums_w = cw * 12.0;
    let width = nums_w + (max_len as f32 + 4.0) * cw;
    let v = ui.visuals().clone();
    let (weak, fg, light) = (v.weak_text_color(), v.text_color(), !v.dark_mode);
    let mut click = None;
    let mut area = egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]);
    if let Some(j) = jump {
        let sid = ui.make_persistent_id(id);
        let st = egui::scroll_area::State::load(ui.ctx(), sid);
        let top = st.map_or(0.0, |s| s.offset.y);
        let view_h = ui.available_height();
        let header_rows: Vec<(usize, usize)> =
            rows.iter().enumerate().filter_map(|(r, row)| if let StackRow::Header(k) = row { Some((r, *k)) } else { None }).collect();
        let current = tops.partition_point(|t| *t < top - 1.0);
        let target = match j {
            FileJump::Next => header_rows.iter().find(|(r, _)| *r > current).map(|(r, _)| tops[*r]),
            FileJump::Prev => header_rows.iter().rev().find(|(r, _)| *r < current).map(|(r, _)| tops[*r]),
            FileJump::To(k) => header_rows.iter().find(|(_, f)| *f == k).map(|(r, _)| tops[*r]),
            FileJump::PageDown => Some(top + view_h * 0.9),
            FileJump::PageUp => Some((top - view_h * 0.9).max(0.0)),
        };
        if let Some(y) = target {
            area = area.vertical_scroll_offset(y.max(0.0));
        }
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    area.show_viewport(ui, |ui, viewport| {
        let w = width.max(ui.available_width());
        let origin = ui.min_rect().min;
        ui.allocate_space(egui::vec2(w, total));
        let visible = ui.clip_rect();
        let first = tops.partition_point(|t| *t <= viewport.min.y).saturating_sub(1);
        for r in first..rows.len() {
            if tops[r] > viewport.max.y {
                break;
            }
            let rect = Rect::from_min_size(origin + egui::vec2(0.0, tops[r]), egui::vec2(w, heights[r]));
            if let StackRow::Note(i) = rows[r] {
                // the block, in the visible width (it does not scroll sideways with the code)
                let Some(n) = notes.as_mut() else { continue };
                let inner = Rect::from_min_size(Pos2::new(visible.left() + nums_w * 0.5, rect.top()), egui::vec2((visible.width() - nums_w * 0.5 - 12.0).max(200.0), 10_000.0));
                let used = ui
                    .allocate_new_ui(egui::UiBuilder::new().max_rect(inner).id_salt(note_id(i)), |ui| {
                        ui.add_space(4.0);
                        (n.show)(ui, i);
                        ui.add_space(6.0);
                    })
                    .response
                    .rect
                    .height();
                if (used - heights[r]).abs() > 0.5 {
                    ui.ctx().data_mut(|d| d.insert_temp(note_id(i), used));
                    ui.ctx().request_repaint();
                }
                continue;
            }
            let resp = ui.interact(rect, egui::Id::new(id).with(("row", r)), Sense::click());
            let p = ui.painter();
            match rows[r] {
                StackRow::Note(_) => {}
                StackRow::Gap => {}
                StackRow::Header(k) => {
                    let f = &files[k];
                    // the header stays in the visible part when the diff is scrolled sideways
                    let bar = Rect::from_min_max(Pos2::new(visible.left(), rect.top() + 1.0), Pos2::new(visible.right() - 2.0, rect.bottom() - 1.0));
                    let fill = if resp.hovered() { v.widgets.hovered.bg_fill } else { v.faint_bg_color };
                    p.rect_filled(bar, 6.0, fill);
                    p.rect_stroke(bar, 6.0, Stroke::new(1.0_f32, v.widgets.noninteractive.bg_stroke.color.gamma_multiply(2.0)));
                    let cy = bar.center().y;
                    let folded = collapsed.contains(&f.start);
                    crate::icons::paint_chevron(p, Pos2::new(bar.left() + 12.0, cy), !folded, weak);
                    let (letter_c, letter) = match f.status {
                        'A' => (crate::highlight::green(), "A"),
                        'D' => (crate::highlight::red(), "D"),
                        'R' => (crate::highlight::blue(), "R"),
                        _ => (crate::highlight::yellow(), "M"),
                    };
                    p.text(Pos2::new(bar.left() + 26.0, cy), Align2::LEFT_CENTER, letter, FontId::monospace(12.5), letter_c);
                    let name = p.text(Pos2::new(bar.left() + 42.0, cy), Align2::LEFT_CENTER, &f.path, FontId::proportional(13.5), v.strong_text_color());
                    let a = p.text(Pos2::new(name.right() + 12.0, cy), Align2::LEFT_CENTER, format!("+{}", f.adds), FontId::monospace(12.0), crate::highlight::green());
                    p.text(Pos2::new(a.right() + 6.0, cy), Align2::LEFT_CENTER, format!("−{}", f.dels), FontId::monospace(12.0), crate::highlight::red());
                    let mut right = bar.right() - 6.0;
                    let mut on_button = false;
                    for (label, tip, act) in file_buttons.iter().rev() {
                        let w = button_width(ui, label);
                        let br = Rect::from_min_max(Pos2::new(right - w, bar.top() + 2.0), Pos2::new(right, bar.bottom() - 2.0));
                        right -= w + 4.0;
                        let bresp = ui.put(br, egui::Button::new(egui::RichText::new(*label).size(12.5))).on_hover_text(*tip);
                        on_button |= bresp.hovered();
                        if bresp.clicked() {
                            click = Some(DiffClick::File(f.start, *act));
                        }
                    }
                    if resp.clicked() && !on_button {
                        if folded {
                            collapsed.remove(&f.start);
                        } else {
                            collapsed.insert(f.start);
                        }
                    }
                }
                StackRow::Line(i) => {
                    let l = &lines[i];
                    let (bg, color) = match l.kind {
                        DiffKind::Added if light => (Some(Color32::from_rgba_unmultiplied(0x2e, 0xa0, 0x43, 34)), crate::highlight::green()),
                        DiffKind::Removed if light => (Some(Color32::from_rgba_unmultiplied(0xd0, 0x3a, 0x4c, 34)), crate::highlight::red()),
                        DiffKind::Added => (Some(Color32::from_rgba_unmultiplied(0x5e, 0xe6, 0xc4, 28)), Color32::from_rgb(0x9f, 0xf0, 0xd8)),
                        DiffKind::Removed => (Some(Color32::from_rgba_unmultiplied(0xff, 0x6b, 0x81, 30)), Color32::from_rgb(0xff, 0xb0, 0xbc)),
                        DiffKind::Hunk => (Some(Color32::from_rgba_unmultiplied(0x5b, 0x8c, 0xf8, 22)), crate::highlight::blue()),
                        DiffKind::Meta => (None, weak),
                        DiffKind::Context => (None, fg),
                    };
                    if let Some(bg) = bg {
                        p.rect_filled(rect, 0.0, bg);
                    }
                    if resp.hovered() && l.kind != DiffKind::Hunk {
                        p.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, weak.gamma_multiply(0.4)));
                    }
                    let y = rect.center().y;
                    let n = |v: Option<usize>| v.map(|v| v.to_string()).unwrap_or_default();
                    p.text(Pos2::new(rect.left() + cw * 5.0, y), Align2::RIGHT_CENTER, n(l.old), font.clone(), weak);
                    p.text(Pos2::new(rect.left() + cw * 10.5, y), Align2::RIGHT_CENTER, n(l.new), font.clone(), weak);
                    p.text(Pos2::new(rect.left() + nums_w, y), Align2::LEFT_CENTER, &l.text, font.clone(), color);
                    if l.kind == DiffKind::Hunk && !hunk_buttons.is_empty() {
                        let mut right = rect.right().min(visible.right()) - 6.0;
                        for (label, tip, act) in hunk_buttons.iter().rev() {
                            let w = button_width(ui, label);
                            let br = Rect::from_min_max(Pos2::new(right - w, rect.top() + 2.0), Pos2::new(right, rect.bottom() - 2.0));
                            right -= w + 4.0;
                            if ui.put(br, egui::Button::new(egui::RichText::new(*label).size(12.5))).on_hover_text(*tip).clicked() {
                                click = Some(DiffClick::Hunk(i, *act));
                            }
                        }
                    } else if l.kind != DiffKind::Hunk && resp.on_hover_text_at_pointer(t!("Double-clic : ouvrir le fichier à cette ligne", "Double-click: open the file at this line")).double_clicked() {
                        click = Some(DiffClick::Line(i));
                    }
                }
            }
        }
    });
    click
}

/// Width of a diff button (12.5 pt label + the style's padding): the button must fit in the
/// rect given to `ui.put`, or it overflows onto its neighbour.
fn button_width(ui: &Ui, label: &str) -> f32 {
    let text = ui.fonts(|f| f.layout_no_wrap(label.to_string(), FontId::proportional(12.5), Color32::WHITE).size().x);
    text + 2.0 * ui.spacing().button_padding.x + 2.0
}

/// Commit message body for display: the lines of a paragraph (hard-wrapped at ~72 columns by
/// git habit) are joined so the text fills the width; blank lines, list items and indented
/// lines are kept.
pub fn reflow(body: &str) -> String {
    let mut out = String::new();
    let mut prev_text = false;
    for line in body.lines() {
        let t = line.trim_start();
        let item = t.starts_with(['-', '*', '+', '•']) || t.split_once(['.', ')']).is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if prev_text && !t.is_empty() && !item && t.len() == line.len() {
            out.push(' ');
        } else if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line.trim_end());
        prev_text = !t.is_empty();
    }
    out
}

/// Unified diff with green / red line backgrounds and old / new line numbers. `hunk_buttons`
/// (label, tooltip) are shown on every "@@" line; double-clicking a line reports it.
pub fn diff_view(ui: &mut Ui, lines: &[DiffLine], id: &str, hunk_buttons: &[DiffButton]) -> Option<DiffClick> {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let (row_h, cw) = ui.fonts(|f| (f.row_height(&font) + 2.0, crate::editor_view::char_width(f, &font)));
    let row_h = if hunk_buttons.is_empty() { row_h } else { row_h.max(22.0) };
    let max_len = lines.iter().map(|l| l.text.chars().count()).max().unwrap_or(0);
    let nums_w = cw * 12.0;
    let width = nums_w + (max_len as f32 + 4.0) * cw;
    let weak = ui.visuals().weak_text_color();
    let fg = ui.visuals().text_color();
    let light = !ui.visuals().dark_mode;
    let mut click = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]).show_rows(ui, row_h, lines.len(), |ui, range| {
        let visible_right = ui.clip_rect().right();
        for (k, l) in lines[range.clone()].iter().enumerate() {
            let i = range.start + k;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(width.max(ui.available_width()), row_h), Sense::click());
            let p = ui.painter();
            let (bg, color) = match l.kind {
                DiffKind::Added if light => (Some(Color32::from_rgba_unmultiplied(0x2e, 0xa0, 0x43, 34)), crate::highlight::green()),
                DiffKind::Removed if light => (Some(Color32::from_rgba_unmultiplied(0xd0, 0x3a, 0x4c, 34)), crate::highlight::red()),
                DiffKind::Added => (Some(Color32::from_rgba_unmultiplied(0x5e, 0xe6, 0xc4, 28)), Color32::from_rgb(0x9f, 0xf0, 0xd8)),
                DiffKind::Removed => (Some(Color32::from_rgba_unmultiplied(0xff, 0x6b, 0x81, 30)), Color32::from_rgb(0xff, 0xb0, 0xbc)),
                DiffKind::Hunk => (Some(Color32::from_rgba_unmultiplied(0x5b, 0x8c, 0xf8, 22)), crate::highlight::blue()),
                DiffKind::Meta => (None, weak),
                DiffKind::Context => (None, fg),
            };
            if let Some(bg) = bg {
                p.rect_filled(rect, 0.0, bg);
            }
            if resp.hovered() && l.kind != DiffKind::Hunk {
                p.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, weak.gamma_multiply(0.4)));
            }
            let y = rect.center().y;
            let n = |v: Option<usize>| v.map(|v| v.to_string()).unwrap_or_default();
            p.text(Pos2::new(rect.left() + cw * 5.0, y), Align2::RIGHT_CENTER, n(l.old), font.clone(), weak);
            p.text(Pos2::new(rect.left() + cw * 10.5, y), Align2::RIGHT_CENTER, n(l.new), font.clone(), weak);
            p.text(Pos2::new(rect.left() + nums_w, y), Align2::LEFT_CENTER, &l.text, font.clone(), color);
            if l.kind == DiffKind::Hunk && !hunk_buttons.is_empty() {
                // buttons on the right of the visible area
                let mut right = rect.right().min(visible_right) - 6.0;
                for (label, tip, act) in hunk_buttons.iter().rev() {
                    let w = button_width(ui, label);
                    let r = Rect::from_min_max(Pos2::new(right - w, rect.top() + 1.0), Pos2::new(right, rect.bottom() - 1.0));
                    right -= w + 4.0;
                    if ui.put(r, egui::Button::new(egui::RichText::new(*label).size(12.5))).on_hover_text(*tip).clicked() {
                        click = Some(DiffClick::Hunk(i, *act));
                    }
                }
            } else if resp.on_hover_text_at_pointer(t!("Double-clic : ouvrir le fichier à cette ligne", "Double-click: open the file at this line")).double_clicked() {
                click = Some(DiffClick::Line(i));
            }
        }
    });
    click
}

/// Patch made of the file header and the single hunk starting at line `hunk` of `text`
/// (line numbers as in `parse_diff`), ready for `git apply`.
pub fn hunk_patch(text: &str, hunk: usize) -> Option<String> {
    // split('\n') keeps the '\r' of CRLF files, unlike lines()
    let lines: Vec<&str> = text.split('\n').collect();
    if !lines.get(hunk)?.starts_with("@@") {
        return None;
    }
    let start = (0..hunk).rev().find(|&i| lines[i].starts_with("diff --git"))?;
    let header_end = (start..hunk).find(|&i| lines[i].starts_with("@@")).unwrap_or(hunk);
    let end = (hunk + 1..lines.len())
        .find(|&i| lines[i].starts_with("@@") || lines[i].starts_with("diff --git"))
        .unwrap_or(lines.len());
    let body: Vec<&str> = lines[hunk..end].iter().copied().filter(|l| !l.is_empty()).collect();
    Some(format!("{}\n{}\n", lines[start..header_end].join("\n"), body.join("\n")))
}

/// The diff without the hunk at line `idx` (`whole_file`: without its file section), as shown
/// right after staging / discarding it, before git gives the new diff.
pub fn without(text: &str, idx: usize, whole_file: bool) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(start) = (0..=idx.min(lines.len().saturating_sub(1))).rev().find(|&i| lines[i].starts_with("diff --git")) else {
        return text.to_string();
    };
    let end = (start + 1..lines.len()).find(|&i| lines[i].starts_with("diff --git")).unwrap_or(lines.len());
    let (a, b) = if whole_file || !lines.get(idx).is_some_and(|l| l.starts_with("@@")) {
        (start, end)
    } else {
        let hunk_end = (idx + 1..end).find(|&i| lines[i].starts_with("@@")).unwrap_or(end);
        // its last hunk: the file goes too
        let others = (start..end).filter(|&i| lines[i].starts_with("@@")).count() > 1;
        if others { (idx, hunk_end) } else { (start, end) }
    };
    let rest: Vec<&str> = lines[..a].iter().chain(&lines[b..]).copied().collect();
    let t = rest.join("\n");
    if t.trim().is_empty() { t!("(aucune différence)", "(no difference)").to_string() } else { t }
}

/// Patch of the whole file section containing line `idx`.
pub fn file_patch(text: &str, idx: usize) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let start = (0..=idx.min(lines.len().checked_sub(1)?)).rev().find(|&i| lines[i].starts_with("diff --git"))?;
    let end = (start + 1..lines.len()).find(|&i| lines[i].starts_with("diff --git")).unwrap_or(lines.len());
    let body = lines[start..end].join("\n");
    Some(format!("{}\n", body.trim_end_matches('\n')))
}

/// File of the diff section containing line `idx` (new path, or the old one for a deletion).
pub fn file_at(text: &str, idx: usize) -> Option<String> {
    let mut path = None;
    for l in text.split('\n').take(idx + 1) {
        if let Some(rest) = l.strip_prefix("diff --git ") {
            path = rest.split_once(" b/").map(|(_, b)| b.to_string());
        } else if let Some(p) = l.strip_prefix("+++ b/") {
            path = Some(p.trim_end_matches(['\t', '\r']).to_string());
        }
    }
    path
}

/// Line (1-based) of the new file to show for diff line `idx`: its own, or the start of its hunk.
pub fn target_line(lines: &[DiffLine], idx: usize) -> usize {
    (0..=idx.min(lines.len().saturating_sub(1)))
        .rev()
        .find_map(|i| lines[i].new.or(lines[i].old).or_else(|| (lines[i].kind == DiffKind::Hunk).then(|| lines.get(i + 1).and_then(|n| n.new)).flatten()))
        .unwrap_or(1)
}

#[cfg(test)]
mod diff_tests {
    use super::*;

    #[test]
    fn reflow_joins_paragraph_lines() {
        let body = "Une phrase coupée\nà 72 colonnes.\n\n- point un\n- point deux\n  suite indentée\n1. étape";
        assert_eq!(reflow(body), "Une phrase coupée à 72 colonnes.\n\n- point un\n- point deux\n  suite indentée\n1. étape");
    }

    #[test]
    fn line_numbers() {
        let d = parse_diff("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -10,3 +10,3 @@ fn\n ctx\n-old\n+new\n ctx2\n");
        let k: Vec<DiffKind> = d.iter().map(|l| l.kind).collect();
        assert_eq!(k[..4], [DiffKind::Meta, DiffKind::Meta, DiffKind::Meta, DiffKind::Hunk]);
        assert_eq!((d[4].old, d[4].new), (Some(10), Some(10)));
        assert_eq!((d[5].kind, d[5].old), (DiffKind::Removed, Some(11)));
        assert_eq!((d[6].kind, d[6].new), (DiffKind::Added, Some(11)));
        assert_eq!((d[7].old, d[7].new), (Some(12), Some(12)));
    }

    #[test]
    fn files_of_a_diff() {
        let text = "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-a\n+A\n b\ndiff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1 @@\n+n\n";
        let lines = parse_diff(text);
        let f = diff_files(&lines);
        assert_eq!(f.iter().map(|f| (f.start, f.path.as_str(), f.status, f.adds, f.dels)).collect::<Vec<_>>(), [(0, "x", 'M', 1, 1), (8, "n", 'A', 1, 0)]);
        assert!(is_file_header("index 1..2 100644") && !is_file_header(" b"));
    }

    #[test]
    fn stacked_view_draws_and_jumps() {
        let text = "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-a\n+A\n b\ndiff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1 @@\n+n\n";
        let lines = parse_diff(text);
        let files = diff_files(&lines);
        let mut folded = std::collections::HashSet::new();
        folded.insert(0);
        let ctx = egui::Context::default();
        for jump in [None, Some(FileJump::To(1)), Some(FileJump::Next), Some(FileJump::PageUp)] {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let click = stacked_diff_view(ui, &lines, &files, &mut folded, "t", &[("+", "", DiffAct::Stage)], &[("Ouvrir", "", DiffAct::Open)], jump, None);
                    assert!(click.is_none());
                });
            });
        }
        assert!(folded.contains(&0));
    }

    #[test]
    fn patches_and_targets() {
        let text = "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-a\n+A\n b\n@@ -10,2 +10,2 @@\n c\n-d\n+D\ndiff --git a/y b/y\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-q\n+Q\n";
        let second = hunk_patch(text, 8).unwrap();
        assert_eq!(second, "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -10,2 +10,2 @@\n c\n-d\n+D\n");
        assert!(hunk_patch(text, 5).is_none());
        assert_eq!(file_at(text, 9).as_deref(), Some("x"));
        assert_eq!(file_at(text, 16).as_deref(), Some("y"));
        assert!(file_patch(text, 16).unwrap().starts_with("diff --git a/y b/y\n"));
        assert!(!file_patch(text, 3).unwrap().contains("a/y"));
        let lines = parse_diff(text);
        assert_eq!(target_line(&lines, 9), 10);
        assert_eq!(target_line(&lines, 8), 10);
        assert_eq!(target_line(&lines, 6), 1);
    }

    #[test]
    fn hunk_actions_on_a_real_repo() {
        let repo = std::env::temp_dir().join(format!("florm_hunks_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let g = |args: &[&str]| git::run(&repo, args).unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        let apply = |patch: &str, flags: &[&str]| {
            let mut a = vec!["apply"];
            a.extend(flags);
            a.extend(["--recount", "-"]);
            git::run_with(&repo, &a, &[], Some(patch)).unwrap();
        };
        g(&["init", "-q"]);
        g(&["config", "user.name", "t"]);
        g(&["config", "user.email", "t@t"]);
        let base: Vec<String> = (0..30).map(|i| format!("l{i}")).collect();
        std::fs::write(repo.join("f.txt"), base.join("\n") + "\n").unwrap();
        g(&["add", "."]);
        g(&["commit", "-qm", "base"]);
        let mut v = base.clone();
        v[1] = "TOP".into();
        v[25] = "BOTTOM".into();
        std::fs::write(repo.join("f.txt"), v.join("\n") + "\n").unwrap();
        let hunks = |text: &str| -> Vec<usize> { text.split('\n').enumerate().filter(|(_, l)| l.starts_with("@@")).map(|(i, _)| i).collect() };

        // stage only the bottom block
        let diff = g(&["diff", "--", "f.txt"]);
        let h = hunks(&diff);
        assert_eq!(h.len(), 2);
        apply(&hunk_patch(&diff, h[1]).unwrap(), &["--cached"]);
        let staged = g(&["diff", "--cached"]);
        assert!(staged.contains("+BOTTOM") && !staged.contains("+TOP"));
        // unstage it again
        apply(&hunk_patch(&staged, hunks(&staged)[0]).unwrap(), &["--cached", "-R"]);
        assert!(g(&["diff", "--cached"]).is_empty());
        // discard the top block from the file
        let diff = g(&["diff", "--", "f.txt"]);
        apply(&hunk_patch(&diff, hunks(&diff)[0]).unwrap(), &["-R"]);
        let now = std::fs::read_to_string(repo.join("f.txt")).unwrap();
        assert!(!now.contains("TOP") && now.contains("BOTTOM"));

        // undo one block of a past commit in the current files
        g(&["commit", "-qam", "bottom"]);
        let show = g(&["show", "--format=", "HEAD", "--", "f.txt"]);
        let patch = hunk_patch(&show, hunks(&show)[0]).unwrap();
        apply(&patch, &["-R"]);
        assert_eq!(std::fs::read_to_string(repo.join("f.txt")).unwrap(), base.join("\n") + "\n");
        let _ = std::fs::remove_dir_all(&repo);
    }
}
