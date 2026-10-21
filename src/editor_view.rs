//! Custom text area: only visible lines are laid out and painted, so the cost
//! per frame does not depend on the file size.

use crate::diff::RowMap;
use crate::document::{Document, Drag, GitMark, Sel};
use crate::highlight::{self, Highlighting};
use crate::icons::mix;
use crate::lint::Severity;
use eframe::egui::{
    self, text::LayoutJob, Align, Align2, Color32, CursorIcon, FontId, Id, Pos2, Rect, RichText, Sense, Shape,
    Stroke, TextFormat, Ui,
};
use std::collections::{HashMap, HashSet};

/// Real advance of one monospace character, as laid out by egui. `glyph_width` returns the
/// unrounded advance (e.g. 8.275 px) while laid out text uses pixel-rounded advances
/// (8 px): using the former makes the cursor drift right on long lines.
pub fn char_width(fonts: &egui::text::Fonts, font: &FontId) -> f32 {
    fonts.layout_no_wrap("M".repeat(100), font.clone(), Color32::WHITE).size().x / 100.0
}

/// Focus id of the editor of the active panel (keyboard input goes to `App::active`).
pub fn editor_id() -> Id {
    Id::new("florm_editor")
}

/// Editor of the other panel when split: clicking it makes that panel the active one.
pub fn other_editor_id() -> Id {
    Id::new("florm_editor_other")
}

fn added_bg(light: bool) -> Color32 {
    if light { Color32::from_rgba_unmultiplied(0x2e, 0xa0, 0x43, 0x2c) } else { Color32::from_rgba_premultiplied(0x1e, 0x2c, 0x16, 0x30) }
}

fn removed_bg(light: bool) -> Color32 {
    if light { Color32::from_rgba_unmultiplied(0xd0, 0x3a, 0x4c, 0x2c) } else { Color32::from_rgba_premultiplied(0x34, 0x16, 0x19, 0x38) }
}

/// How the editor shows a document (from the settings).
pub struct ViewOpts<'a> {
    pub font_size: f32,
    pub minimap: bool,
    /// Git changes inside the text (deleted lines in red, added in green).
    pub inline_diff: bool,
    /// The class / function lines around the top of the view stay pinned there.
    pub sticky: bool,
    /// Columns of the vertical rulers.
    pub rulers: &'a [usize],
}

pub fn show(ui: &mut Ui, doc: &mut Document, hl: &Highlighting, opts: &ViewOpts, matches: &[(usize, usize)], id: Id) -> egui::Response {
    let font = FontId::monospace(opts.font_size);
    let (rh, cw) = ui.fonts(|f| (f.row_height(&font), char_width(f, &font)));
    let bg = hl.colors.bg;
    if opts.minimap {
        egui::SidePanel::right(id.with("minimap"))
            .exact_width(110.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::none().fill(bg))
            .show_inside(ui, |ui| minimap_ui(ui, doc, hl, rh));
    }
    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(bg))
        .show_inside(ui, |ui| text_area(ui, doc, hl, &font, rh, cw, opts, matches, id))
        .inner
}

/// Line `l` with its syntax colors, from visual column `vstart` to `vend` (tabs expanded):
/// the column where it starts (None when nothing is visible) and the text to lay out.
fn line_job(doc: &Document, l: usize, font: &FontId, fg: Color32, vstart: usize, vend: usize) -> (Option<usize>, LayoutJob) {
    let ts = doc.tab_size;
    let spans = doc.highlight.spans(l);
    let mut job = LayoutJob::default();
    let mut buf = String::new();
    let mut cur = fg;
    let (mut v, mut byte, mut si) = (0usize, 0usize, 0usize);
    let mut start_v = None;
    for ch in doc.rope.line(l).chars() {
        if ch == '\n' || ch == '\r' {
            break;
        }
        let color = spans
            .and_then(|sp| {
                while si < sp.len() && sp[si].1.end <= byte {
                    si += 1;
                }
                sp.get(si).map(|s| s.0)
            })
            .unwrap_or(fg);
        let w = if ch == '\t' { ts - v % ts } else { 1 };
        if v >= vend {
            break;
        }
        if v + w > vstart {
            start_v.get_or_insert(v);
            if color != cur && !buf.is_empty() {
                job.append(&buf, 0.0, TextFormat::simple(font.clone(), cur));
                buf.clear();
            }
            cur = color;
            if ch == '\t' {
                buf.extend(std::iter::repeat(' ').take(w));
            } else {
                buf.push(ch);
            }
        }
        v += w;
        byte += ch.len_utf8();
    }
    if !buf.is_empty() {
        job.append(&buf, 0.0, TextFormat::simple(font.clone(), cur));
    }
    (start_v, job)
}

/// Lines of the classes / functions enclosing `top` whose header is above it (at most 3, the
/// innermost): they are pinned at the top of the view, hiding the first lines under them, so
/// the scopes are looked up again below the pinned band.
fn sticky_lines(doc: &mut Document, top: usize) -> Vec<usize> {
    let mut lines: Vec<usize> = Vec::new();
    for _ in 0..3 {
        let under = top + lines.len();
        let mut next: Vec<usize> = crate::symbols::path_at(doc.symbols(), under)
            .iter()
            .filter(|s| s.line < under)
            .map(|s| s.line)
            .collect();
        if next.len() > 3 {
            next.drain(..next.len() - 3);
        }
        if next.len() == lines.len() {
            return next;
        }
        lines = next;
    }
    lines
}

/// Screen rectangles covering chars a..b on the visible lines.
#[allow(clippy::too_many_arguments)]
fn span_rects(
    doc: &Document,
    rm: &RowMap,
    a: usize,
    b: usize,
    first: usize,
    last: usize,
    text_x: f32,
    top: f32,
    rh: f32,
    cw: f32,
) -> Vec<Rect> {
    let la = doc.rope.char_to_line(a);
    let lb = doc.rope.char_to_line(b);
    let mut v = Vec::new();
    for l in (la.max(first)..=lb.min(last)).filter(|&l| !rm.is_hidden(l)) {
        let ls = doc.rope.line_to_char(l);
        let ll = doc.line_len(l);
        let c0 = if l == la { a - ls } else { 0 };
        let (c1, newline) = if l == lb { (b - ls, false) } else { (ll, true) };
        let x0 = text_x + doc.vcol(l, c0.min(ll)) as f32 * cw;
        let mut x1 = text_x + doc.vcol(l, c1.min(ll)) as f32 * cw;
        if newline {
            x1 += cw * 0.6;
        }
        let y = top + rm.row(l) as f32 * rh;
        v.push(Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + rh)));
    }
    v
}

#[allow(clippy::too_many_arguments)]
fn text_area(
    ui: &mut Ui,
    doc: &mut Document,
    hl: &Highlighting,
    font: &FontId,
    rh: f32,
    cw: f32,
    opts: &ViewOpts,
    matches: &[(usize, usize)],
    id: Id,
) -> egui::Response {
    let inline_diff = opts.inline_diff;
    let c = hl.colors;
    let nlines = doc.rope.len_lines();
    let digits = nlines.to_string().len().max(3);
    // numbers, then the fold arrows, then the git marks
    let gutter_w = (digits as f32 + 1.0) * cw + 30.0;
    doc.update_max_cols();
    doc.reveal_cursors(); // whatever placed a cursor in a folded block, open it
    let hidden = doc.hidden_ranges();
    let rm = RowMap::with_folds(if inline_diff { &doc.hunks } else { &[] }, &hidden);
    let folded: HashSet<usize> = hidden.iter().map(|&(s, _)| s - 1).collect();
    let focused = ui.memory(|m| m.has_focus(id));

    let mut sa = egui::ScrollArea::both().id_salt(("editor_scroll", doc.uid)).auto_shrink([false, false]);
    if let Some(y) = doc.scroll_request.take() {
        sa = sa.vertical_scroll_offset(y.max(0.0));
    }
    let out = sa.show_viewport(ui, |ui, vp| {
        let content_w = gutter_w + (doc.max_cols as f32 + 8.0) * cw;
        let content_h = rm.rows(nlines) as f32 * rh + (vp.height() - 3.0 * rh).max(0.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(content_w.max(ui.available_width()), content_h), Sense::hover());
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        if resp.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                )
            });
        }
        let origin = rect.min;
        let gx = origin.x + vp.min.x; // gutter stays fixed horizontally
        let text_x = origin.x + gutter_w + 6.0;
        let first_row = (vp.min.y / rh).floor().max(0.0) as usize;
        let last_row = (vp.max.y / rh).ceil().max(0.0) as usize;
        let first = rm.line_at_row(first_row, nlines);
        let last = rm.line_at_row(last_row, nlines).min(nlines - 1);
        let vstart = ((vp.min.x - gutter_w).max(0.0) / cw).floor() as usize;
        let vend = vstart + (vp.width() / cw).ceil() as usize + 2;
        let line_y = |l: usize| origin.y + rm.row(l) as f32 * rh;
        // sticky scroll: the class / function lines enclosing the top of the view
        let sticky_top = origin.y + vp.min.y;
        let top_line = rm.line_at_row((vp.min.y / rh).ceil() as usize, nlines);
        let sticky = if opts.sticky && top_line > 0 { sticky_lines(doc, top_line) } else { Vec::new() };
        let sticky_h = sticky.len() as f32 * rh;

        // ---------------------------------------------------------------- mouse
        let hit = |doc: &Document, p: Pos2| -> usize {
            let line = rm.line_at_row(((p.y - origin.y) / rh).floor().max(0.0) as usize, nlines);
            let vx = ((p.x - text_x) / cw).round().max(0.0) as usize;
            doc.pos_of(line, doc.col_at_vcol(line, vx))
        };
        let (pressed, middle_pressed, down, ptr, mods) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.button_pressed(egui::PointerButton::Middle),
                i.pointer.primary_down() || i.pointer.middle_down(),
                i.pointer.interact_pos(),
                i.modifiers,
            )
        });
        // a click on a pinned line goes to it (not to the line hidden under it)
        let on_sticky = ptr.is_some_and(|p| !sticky.is_empty() && p.y >= sticky_top && p.y < sticky_top + sticky_h);
        if on_sticky && pressed && resp.hovered() {
            let k = ((ptr.unwrap().y - sticky_top) / rh) as usize;
            if let Some(&l) = sticky.get(k) {
                resp.request_focus();
                doc.goto_line(l, doc.indent_len(l));
            }
        }
        let (pressed, middle_pressed) = (pressed && !on_sticky, middle_pressed && !on_sticky);
        if let Some(p) = ptr {
            let in_gutter = p.x < gx + gutter_w;
            if (pressed || middle_pressed) && resp.hovered() {
                resp.request_focus();
                let pos = hit(doc, p);
                let line = doc.rope.char_to_line(pos);
                let on_marker = in_gutter && p.x >= gx + gutter_w - 8.0 && doc.hunks.iter().any(|h| h.contains(line));
                let on_fold = in_gutter
                    && p.x >= gx + gutter_w - 21.0
                    && p.x < gx + gutter_w - 8.0
                    && (folded.contains(&line) || doc.can_fold(line));
                let on_pill = !in_gutter && folded.contains(&line) && p.x >= text_x + (doc.vcol(line, doc.line_len(line)) as f32 + 1.0) * cw;
                if on_fold || on_pill {
                    doc.toggle_fold(line);
                } else if on_marker {
                    // Click on a git change mark: toggle the inline diff.
                    let same = doc.peek.is_some_and(|pl| doc.hunks.iter().any(|h| h.contains(pl) && h.contains(line)));
                    doc.peek = if same { None } else { doc.hunks.iter().find(|h| h.contains(line)).map(|h| h.mark_lines().start) };
                } else if in_gutter {
                    doc.sels = vec![Sel::range(doc.rope.line_to_char(line), doc.line_end_incl(line))];
                    doc.primary = 0;
                    doc.drag = Some(Drag::Line(line));
                } else if mods.alt || middle_pressed {
                    let vc = ((p.x - text_x) / cw).round().max(0.0) as usize;
                    doc.sels = vec![Sel::cursor(pos)];
                    doc.primary = 0;
                    doc.drag = Some(Drag::Column(line, vc));
                } else if mods.command || mods.ctrl {
                    doc.sels.push(Sel::cursor(pos));
                    doc.primary = doc.sels.len() - 1;
                    doc.normalize();
                    doc.drag = Some(Drag::Normal(pos));
                } else if mods.shift {
                    let a = doc.primary_sel().anchor;
                    doc.sels = vec![Sel::range(a, pos)];
                    doc.primary = 0;
                    doc.drag = Some(Drag::Normal(a));
                } else {
                    doc.sels = vec![Sel::cursor(pos)];
                    doc.primary = 0;
                    doc.drag = Some(Drag::Normal(pos));
                }
                doc.end_coalesce();
            }
            if resp.triple_clicked() && !in_gutter {
                let line = doc.rope.char_to_line(hit(doc, p));
                let i = doc.primary.min(doc.sels.len() - 1);
                doc.sels[i] = Sel::range(doc.rope.line_to_char(line), doc.line_end_incl(line));
                doc.normalize();
            } else if resp.double_clicked() && !in_gutter {
                let pos = hit(doc, p);
                let (a, b) = doc.word_at(pos);
                let (a, b) = if a == b { (pos, (pos + 1).min(doc.len())) } else { (a, b) };
                let i = doc.primary.min(doc.sels.len() - 1);
                doc.sels[i] = Sel::range(a, b);
                doc.normalize();
                doc.drag = None;
            }
            if resp.secondary_clicked() && !in_gutter {
                // like VS Code: right click moves the cursor unless it lands in a selection
                let pos = hit(doc, p);
                let inside = doc.sels.iter().any(|s| !s.is_empty() && s.start() <= pos && pos <= s.end());
                if !inside {
                    doc.sels = vec![Sel::cursor(pos)];
                    doc.primary = 0;
                }
                resp.request_focus();
            }
            if down && (resp.dragged() || resp.drag_started()) {
                if let Some(drag) = doc.drag {
                    let pos = hit(doc, p);
                    let line = doc.rope.char_to_line(pos);
                    match drag {
                        Drag::Normal(a) => {
                            let i = doc.primary.min(doc.sels.len() - 1);
                            doc.sels[i] = Sel::range(a, pos);
                            doc.normalize();
                        }
                        Drag::Line(l0) => {
                            let (lo, hi) = (l0.min(line), l0.max(line));
                            let (s, e) = (doc.rope.line_to_char(lo), doc.line_end_incl(hi));
                            doc.sels = vec![if line < l0 { Sel::range(e, s) } else { Sel::range(s, e) }];
                            doc.primary = 0;
                        }
                        Drag::Column(l0, v0) => {
                            let v1 = ((p.x - text_x) / cw).round().max(0.0) as usize;
                            let sels: Vec<Sel> = (l0.min(line)..=l0.max(line))
                                .map(|l| Sel::range(doc.pos_of(l, doc.col_at_vcol(l, v0)), doc.pos_of(l, doc.col_at_vcol(l, v1))))
                                .collect();
                            doc.sels = sels;
                            doc.primary = doc.sels.len() - 1;
                            doc.normalize();
                        }
                    }
                    ui.scroll_to_rect(Rect::from_center_size(p, egui::vec2(1.0, 1.0)), None);
                }
            }
            doc.hover_word = None;
            if on_sticky && resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            } else if resp.hovered() {
                if !in_gutter {
                    ui.ctx().set_cursor_icon(CursorIcon::Text);
                    // tooltip on the underlined (squiggled) text
                    let l = rm.line_at_row(((p.y - origin.y) / rh).floor().max(0.0) as usize, nlines);
                    let col = doc.col_at_vcol(l, ((p.x - text_x) / cw).max(0.0) as usize);
                    let ls = doc.rope.line_to_char(l);
                    // the symbol under the pointer (not past the end of the line): the app looks it up
                    if col < doc.line_len(l) {
                        doc.hover_word = doc.symbol_at(ls + col);
                    }
                    let msgs: Vec<String> = doc
                        .diagnostics
                        .iter()
                        .filter(|d| d.line == l)
                        .filter(|d| {
                            let start = d.col.min(doc.line_len(l));
                            let (_, we) = doc.word_at(ls + start);
                            let end = (we - ls).max(start + 1);
                            col >= start && col <= end
                        })
                        .map(|d| format!("{} : {}", d.source, d.message))
                        .collect();
                    let mut tip = msgs.join("\n");
                    if let Some((_, info)) = doc.hover_info.as_ref().filter(|(r, t)| Some(*r) == doc.hover_word && !t.is_empty()) {
                        if !tip.is_empty() {
                            tip.push_str("\n\n");
                        }
                        tip.push_str(info);
                    }
                    if !tip.is_empty() {
                        tooltip_under_line(ui, &resp, p, origin.y, rh, tip);
                    }
                } else {
                    let l = rm.line_at_row(((p.y - origin.y) / rh).floor().max(0.0) as usize, nlines);
                    let mut msgs: Vec<String> =
                        doc.diagnostics.iter().filter(|d| d.line == l).map(|d| format!("{} : {}", d.source, d.message)).collect();
                    if p.x >= gx + gutter_w - 8.0 && doc.hunks.iter().any(|h| h.contains(l)) {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        msgs.push("Modifié depuis le dernier commit — clic pour voir le diff".into());
                    }
                    if !msgs.is_empty() {
                        tooltip_under_line(ui, &resp, p, origin.y, rh, msgs.join("\n"));
                    }
                }
            }
        }
        if !down {
            doc.drag = None;
        }

        // ---------------------------------------------------------------- highlighting
        if !doc.highlight.ensure(hl, &doc.syntax, &doc.rope, last, 20_000) {
            ui.ctx().request_repaint();
        }

        let painter = ui.painter();
        let vis_start = doc.rope.line_to_char(first);
        let vis_end = doc.line_end_incl(last);

        // current line(s)
        for s in &doc.sels {
            if s.is_empty() {
                let l = doc.rope.char_to_line(s.head);
                if (first..=last).contains(&l) {
                    let r = Rect::from_min_size(Pos2::new(gx, line_y(l)), egui::vec2(vp.width(), rh));
                    painter.rect_filled(r, 0.0, c.line_hl);
                }
            }
        }

        // Merge conflicts: our side in green, theirs in blue, markers stronger
        let conflicts = doc.conflicts();
        let mut conflict_labels = Vec::new();
        for cf in conflicts.iter().filter(|cf| cf.end >= first && cf.start <= last) {
            let (green, blue) = (highlight::green(), highlight::blue());
            for l in (cf.start.max(first)..=cf.end.min(last)).filter(|&l| !rm.is_hidden(l)) {
                let marker = l == cf.start || l == cf.mid || l == cf.end || Some(l) == cf.base;
                let col = if l < cf.base.unwrap_or(cf.mid) || l == cf.start {
                    green
                } else if l <= cf.mid {
                    c.gutter_fg
                } else {
                    blue
                };
                let bg = mix(c.bg, col, if marker { 0.32 } else { 0.12 });
                painter.rect_filled(Rect::from_min_size(Pos2::new(gx, line_y(l)), egui::vec2(vp.width(), rh)), 0.0, bg);
            }
            conflict_labels.push((cf.start, "  ← ta version (actuelle)"));
            conflict_labels.push((cf.end, "  ← version entrante"));
        }

        // Diff views: full-line backgrounds for +/- lines.
        if doc.syntax == "Diff" {
            for l in (first..=last).filter(|&l| !rm.is_hidden(l)) {
                let bg = match (doc.char_at(doc.rope.line_to_char(l)), doc.char_at(doc.rope.line_to_char(l) + 1)) {
                    (Some('+'), Some(n)) if n != '+' => Some(added_bg(hl.is_light())),
                    (Some('+'), None) => Some(added_bg(hl.is_light())),
                    (Some('-'), Some(n)) if n != '-' => Some(removed_bg(hl.is_light())),
                    (Some('-'), None) => Some(removed_bg(hl.is_light())),
                    (Some('@'), Some('@')) => Some(Color32::from_rgba_unmultiplied(0x61, 0xaf, 0xef, 30)),
                    _ => None,
                };
                if let Some(bg) = bg {
                    painter.rect_filled(Rect::from_min_size(Pos2::new(gx, line_y(l)), egui::vec2(vp.width(), rh)), 0.0, bg);
                }
            }
        }

        // Inline git diff: added/modified lines in green, deleted HEAD lines as red phantom rows.
        let mut revert_clicked = None;
        if inline_diff {
            let text_w = vp.width();
            for (hi, h) in doc.hunks.iter().enumerate() {
                if h.new_end < first || h.new_start > last + 1 || rm.is_hidden(h.new_start) {
                    continue;
                }
                for l in (h.new_start.max(first)..h.new_end.min(last + 1)).filter(|&l| !rm.is_hidden(l)) {
                    painter.rect_filled(Rect::from_min_size(Pos2::new(gx, line_y(l)), egui::vec2(text_w, rh)), 0.0, added_bg(hl.is_light()));
                }
                let n_old = h.old_lines.len();
                let top_row = rm.row(h.new_start) - n_old;
                for (k, old) in h.old_lines.iter().enumerate() {
                    let y = origin.y + (top_row + k) as f32 * rh;
                    if y + rh < origin.y + vp.min.y || y > origin.y + vp.max.y {
                        continue;
                    }
                    painter.rect_filled(Rect::from_min_size(Pos2::new(gx, y), egui::vec2(text_w, rh)), 0.0, removed_bg(hl.is_light()));
                    let mut shown = String::new();
                    let mut v = 0;
                    for ch in old.chars().take_while(|c| *c != '\n' && *c != '\r') {
                        let w = if ch == '\t' { doc.tab_size - v % doc.tab_size } else { 1 };
                        if v >= vstart && v < vend {
                            if ch == '\t' {
                                shown.extend(std::iter::repeat(' ').take(w));
                            } else {
                                shown.push(ch);
                            }
                        }
                        v += w;
                    }
                    painter.text(
                        Pos2::new(text_x + vstart as f32 * cw, y),
                        Align2::LEFT_TOP,
                        shown,
                        font.clone(),
                        c.fg.gamma_multiply(0.75),
                    );
                }
                // small "revert" button at the right of the hunk's first row
                let y = origin.y + top_row as f32 * rh;
                if y + rh >= origin.y + vp.min.y && y <= origin.y + vp.max.y {
                    let label = "↺ Revenir";
                    let galley = painter.layout_no_wrap(label.into(), FontId::proportional(12.0), c.fg);
                    let br = Rect::from_min_size(
                        Pos2::new(gx + vp.width() - galley.size().x - 26.0, y + (rh - galley.size().y) / 2.0 - 1.0),
                        galley.size() + egui::vec2(10.0, 2.0),
                    );
                    let r = ui.interact(br, Id::new(("revert_hunk", doc.uid, h.new_start)), Sense::click());
                    let fill = if r.hovered() { c.selection } else { c.gutter_bg };
                    painter.rect_filled(br, 3.0, fill);
                    painter.galley(br.min + egui::vec2(5.0, 1.0), galley, c.fg);
                    if r.on_hover_text("Revenir à la version du dernier commit pour ce bloc (Ctrl+Z pour annuler)").clicked() {
                        revert_clicked = Some(hi);
                    }
                }
            }
        }

        // vertical rulers
        for &col in opts.rulers {
            let x = text_x + col as f32 * cw;
            painter.line_segment([Pos2::new(x, origin.y + vp.min.y), Pos2::new(x, origin.y + vp.max.y)], Stroke::new(1.0_f32, c.guide));
        }

        // indent guides
        let ts = doc.tab_size;
        for l in (first..=last).filter(|&l| !rm.is_hidden(l)) {
            let ind = doc.indent_len(l);
            if ind == 0 || ind == doc.line_len(l) {
                continue;
            }
            let lead = doc.vcol(l, ind);
            for g in (ts..lead).step_by(ts) {
                let x = text_x + g as f32 * cw;
                painter.line_segment([Pos2::new(x, line_y(l)), Pos2::new(x, line_y(l) + rh)], Stroke::new(1.0_f32, c.guide));
            }
        }

        // result picked in the search panel: tinted line, pulsing at first, while still selected
        let mut flash_rect = None;
        if let Some((a, b, t)) = doc.flash {
            let s = doc.primary_sel();
            if doc.sels.len() == 1 && s.start() == a && s.end() == b {
                let pulse = (1.0 - t.elapsed().as_secs_f32() / 1.2).max(0.0);
                if pulse > 0.0 {
                    ui.ctx().request_repaint();
                }
                let l = doc.rope.char_to_line(a);
                if (first..=last).contains(&l) {
                    let y = line_y(l);
                    let band = Rect::from_min_size(Pos2::new(gx, y), egui::vec2(vp.width(), rh));
                    painter.rect_filled(band, 0.0, mix(c.bg, c.accent, 0.14 + 0.3 * pulse));
                    painter.rect_filled(Rect::from_min_size(Pos2::new(text_x - 5.0, y), egui::vec2(3.0, rh)), 1.0, c.accent);
                    flash_rect = Some((a, b, pulse));
                }
            } else {
                doc.flash = None;
            }
        }

        // Ctrl+F "dans la sélection": the searched range
        if let Some((a, b)) = doc.find_scope {
            for r in span_rects(doc, &rm, a.min(doc.len()), b.min(doc.len()), first, last, text_x, origin.y, rh, cw) {
                painter.rect_filled(r, 0.0, mix(c.bg, c.accent, 0.09));
            }
        }

        // find matches
        let i0 = matches.partition_point(|m| m.1 < vis_start);
        for &(a, b) in matches[i0..].iter().take_while(|m| m.0 <= vis_end) {
            for r in span_rects(doc, &rm, a, b, first, last, text_x, origin.y, rh, cw) {
                painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, c.find));
            }
        }

        // selections
        for s in &doc.sels {
            if s.is_empty() || s.end() < vis_start || s.start() > vis_end {
                continue;
            }
            for r in span_rects(doc, &rm, s.start(), s.end(), first, last, text_x, origin.y, rh, cw) {
                painter.rect_filled(r, 2.0, c.selection);
            }
        }

        if let Some((a, b, pulse)) = flash_rect {
            for r in span_rects(doc, &rm, a, b, first, last, text_x, origin.y, rh, cw) {
                let r = r.expand(1.0 + 3.0 * pulse);
                painter.rect_filled(r, 3.0, mix(c.bg, c.accent, 0.35 + 0.25 * pulse));
                painter.rect_stroke(r, 3.0, Stroke::new(2.0_f32, c.accent));
            }
        }

        for &(l, label) in conflict_labels.iter().filter(|(l, _)| (first..=last).contains(l) && !rm.is_hidden(*l)) {
            let x = text_x + (doc.vcol(l, doc.line_len(l)) as f32 + 1.0) * cw;
            painter.text(Pos2::new(x, line_y(l) + rh / 2.0), Align2::LEFT_CENTER, label, FontId::proportional(font.size * 0.85), c.gutter_fg);
        }

        // text
        for l in first..=last {
            if rm.is_hidden(l) {
                continue;
            }
            if folded.contains(&l) {
                // "⋯ N lignes" after a folded line
                let n = hidden.iter().find(|h| h.0 == l + 1).map_or(0, |h| h.1 - h.0 + 1);
                let x = text_x + (doc.vcol(l, doc.line_len(l)) as f32 + 1.0) * cw;
                let label = format!("⋯ {n} ligne{}", if n > 1 { "s" } else { "" });
                let g = painter.layout_no_wrap(label, FontId::proportional(font.size * 0.8), c.fg);
                let pill = Rect::from_min_size(Pos2::new(x, line_y(l) + 2.0), egui::vec2(g.size().x + 12.0, rh - 4.0));
                painter.rect_filled(pill, 4.0, mix(c.bg, c.accent, 0.22));
                painter.galley(Pos2::new(pill.left() + 6.0, pill.center().y - g.size().y / 2.0), g, c.fg);
            }
            let (start_v, job) = line_job(doc, l, font, c.fg, vstart, vend);
            if let Some(sv) = start_v {
                let galley = ui.fonts(|f| f.layout_job(job));
                painter.galley(Pos2::new(text_x + sv as f32 * cw, line_y(l)), galley, c.fg);
            }
        }

        // inline blame at the end of its line
        if let Some((bl, _, text)) = &doc.blame {
            if (first..=last).contains(bl) {
                let x = text_x + (doc.vcol(*bl, doc.line_len(*bl)) as f32 + 4.0) * cw;
                painter.text(
                    Pos2::new(x, line_y(*bl) + rh / 2.0),
                    Align2::LEFT_CENTER,
                    text,
                    FontId::proportional(font.size * 0.9),
                    c.gutter_fg.gamma_multiply(1.2),
                );
            }
        }

        // diagnostics (squiggles)
        for d in doc.diagnostics.iter().filter(|d| (first..=last).contains(&d.line) && !rm.is_hidden(d.line)) {
            let ll = doc.line_len(d.line);
            let col = d.col.min(ll);
            let ls = doc.rope.line_to_char(d.line);
            let (_, we) = doc.word_at(ls + col);
            let end = if we > ls + col { we - ls } else { (col + 1).min(ll.max(col + 1)) };
            let x0 = text_x + doc.vcol(d.line, col) as f32 * cw;
            let x1 = (text_x + doc.vcol(d.line, end.min(ll)) as f32 * cw).max(x0 + cw);
            let y = line_y(d.line) + rh - 2.0;
            let color = if d.severity == Severity::Error { highlight::red() } else { highlight::yellow() };
            let mut pts = Vec::new();
            let mut x = x0;
            let mut up = true;
            while x <= x1 {
                pts.push(Pos2::new(x, if up { y - 1.5 } else { y + 1.0 }));
                x += 2.5;
                up = !up;
            }
            painter.add(Shape::line(pts, Stroke::new(1.0_f32, color)));
        }

        // matching brackets
        let prim = doc.primary_sel();
        if prim.is_empty() {
            if let Some((a, b)) = doc.matching_bracket(prim.head) {
                for p in [a, b] {
                    for r in span_rects(doc, &rm, p, p + 1, first, last, text_x, origin.y, rh, cw) {
                        painter.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.5_f32, c.caret));
                    }
                }
            }
        }

        // cursors
        let caret = if focused { c.caret } else { c.caret.gamma_multiply(0.4) };
        let mut caret_screen = None;
        for (k, s) in doc.sels.iter().enumerate() {
            let (l, col) = doc.line_col(s.head);
            if (first..=last).contains(&l) {
                let x = text_x + doc.vcol(l, col) as f32 * cw;
                painter.line_segment([Pos2::new(x, line_y(l)), Pos2::new(x, line_y(l) + rh)], Stroke::new(2.0_f32, caret));
                if k == doc.primary && focused {
                    caret_screen = Some((x, line_y(l) + rh));
                }
            }
        }
        doc.caret_screen = caret_screen;

        // gutter
        let gutter = Rect::from_min_size(Pos2::new(gx, origin.y + vp.min.y), egui::vec2(gutter_w, vp.height()));
        painter.rect_filled(gutter, 0.0, c.bg);
        let cursor_lines: HashSet<usize> = doc.sels.iter().map(|s| doc.rope.char_to_line(s.head)).collect();
        let bookmarks: HashSet<usize> = doc.bookmark_lines().into_iter().collect();
        let mut problems: HashMap<usize, Severity> = HashMap::new();
        for d in &doc.diagnostics {
            let e = problems.entry(d.line).or_insert(d.severity);
            if d.severity == Severity::Error {
                *e = Severity::Error;
            }
        }
        let gutter_hover = ui.rect_contains_pointer(gutter);
        for l in first..=last {
            if rm.is_hidden(l) {
                continue;
            }
            // the number of a line with a problem takes its color (no extra mark in the gutter)
            let color = match problems.get(&l) {
                Some(Severity::Error) => highlight::red(),
                Some(_) => highlight::yellow(),
                None if cursor_lines.contains(&l) => c.fg,
                None => c.gutter_fg.gamma_multiply(0.75),
            };
            painter.text(Pos2::new(gx + gutter_w - 22.0, line_y(l)), Align2::RIGHT_TOP, (l + 1).to_string(), font.clone(), color);
            if bookmarks.contains(&l) {
                // bookmark: a small ribbon on the left of the number
                let r = Rect::from_min_size(Pos2::new(gx + 4.0, line_y(l) + 3.0), egui::vec2(4.0, rh - 6.0));
                painter.rect_filled(r, 2.0, c.accent);
            }
            let arrow = Pos2::new(gx + gutter_w - 14.0, line_y(l) + rh / 2.0);
            if folded.contains(&l) {
                crate::icons::paint_chevron(painter, arrow, false, c.accent);
            } else if gutter_hover && doc.can_fold(l) {
                crate::icons::paint_chevron(painter, arrow, true, c.gutter_fg);
            }
            if let Some(m) = doc.git_marks.get(&l) {
                let col = match m {
                    GitMark::Added => highlight::green(),
                    GitMark::Modified => highlight::yellow(),
                    GitMark::Deleted => highlight::red(),
                };
                let r = if *m == GitMark::Deleted {
                    Rect::from_min_size(Pos2::new(gx + gutter_w - 6.0, line_y(l) + rh - 2.0), egui::vec2(5.0, 3.0))
                } else {
                    Rect::from_min_size(Pos2::new(gx + gutter_w - 5.0, line_y(l)), egui::vec2(3.0, rh))
                };
                painter.rect_filled(r, 0.0, col);
            }
        }

        // gutter markers of phantom rows (drawn after the gutter background)
        if inline_diff {
            for &(line, count) in rm.blocks() {
                let top_row = rm.row(line) - count;
                for k in 0..count {
                    let y = origin.y + (top_row + k) as f32 * rh;
                    if y + rh >= origin.y + vp.min.y && y <= origin.y + vp.max.y {
                        painter.text(Pos2::new(gx + gutter_w - 22.0, y), Align2::RIGHT_TOP, "−", font.clone(), highlight::red());
                        painter.rect_filled(Rect::from_min_size(Pos2::new(gx + gutter_w - 5.0, y), egui::vec2(3.0, rh)), 0.0, highlight::red());
                    }
                }
            }
        }
        if let Some(hi) = revert_clicked {
            doc.revert_hunk(hi);
        }

        // pinned lines, over the text and the gutter
        if !sticky.is_empty() {
            let band = Rect::from_min_size(Pos2::new(gx, sticky_top), egui::vec2(vp.width(), sticky_h));
            painter.rect_filled(band, 0.0, c.bg);
            for (k, &l) in sticky.iter().enumerate() {
                let y = sticky_top + k as f32 * rh;
                let (start_v, job) = line_job(doc, l, font, c.fg, vstart, vend);
                if let Some(sv) = start_v {
                    let galley = ui.fonts(|f| f.layout_job(job));
                    painter.galley(Pos2::new(text_x + sv as f32 * cw, y), galley, c.fg);
                }
                painter.rect_filled(Rect::from_min_size(Pos2::new(gx, y), egui::vec2(gutter_w, rh)), 0.0, c.bg);
                painter.text(Pos2::new(gx + gutter_w - 22.0, y), Align2::RIGHT_TOP, (l + 1).to_string(), font.clone(), c.gutter_fg);
            }
            if on_sticky {
                let k = ((ptr.unwrap().y - sticky_top) / rh) as usize;
                let row = Rect::from_min_size(Pos2::new(gx, sticky_top + k as f32 * rh), egui::vec2(vp.width(), rh));
                painter.rect_filled(row, 0.0, c.fg.gamma_multiply(0.06));
            }
            // a thin shadow under the band
            for i in 0..4 {
                let y = band.bottom() + i as f32;
                painter.line_segment([Pos2::new(gx, y), Pos2::new(band.right(), y)], Stroke::new(1.0_f32, Color32::from_black_alpha(40 - i * 10)));
            }
        }

        // inline diff of a git change (opened from the gutter mark or Alt+F5)
        if inline_diff {
            doc.peek = None;
        }
        if let Some(pl) = doc.peek {
            match doc.hunks.iter().position(|h| h.contains(pl)) {
                None => doc.peek = None,
                Some(hi) => {
                    let h = &doc.hunks[hi];
                    let y = line_y(h.new_end.max(h.mark_lines().end));
                    if y >= origin.y + vp.min.y && y <= origin.y + vp.max.y {
                        let action = diff_peek(ui, doc.uid, hi, doc.hunks.len(), h, Pos2::new(gx + gutter_w, y), (vp.width() - gutter_w - 24.0).max(240.0), font, rh, &c);
                        match action {
                            Some(Peek::Revert) => doc.revert_hunk(hi),
                            Some(Peek::Close) => doc.peek = None,
                            Some(Peek::Next) => doc.goto_change(true),
                            Some(Peek::Prev) => doc.goto_change(false),
                            Some(Peek::Inline) => {
                                doc.peek = None;
                                doc.request_inline_diff = true;
                            }
                            None => {}
                        }
                    }
                }
            }
        }

        // keep the primary cursor visible after keyboard actions
        if doc.scroll_to_cursor {
            doc.scroll_to_cursor = false;
            let (l, col) = doc.line_col(prim.head);
            let x = text_x + doc.vcol(l, col) as f32 * cw;
            let r = Rect::from_min_max(
                Pos2::new(x - gutter_w - 2.0 * cw, line_y(l) - rh),
                Pos2::new(x + 4.0 * cw, line_y(l) + 2.0 * rh),
            );
            ui.scroll_to_rect(r, if doc.center_cursor { Some(Align::Center) } else { None });
            doc.center_cursor = false;
        }
        resp
    });
    doc.scroll_y = out.state.offset.y;
    doc.view_h = out.inner_rect.height();
    doc.page_lines = ((doc.view_h / rh) as usize).saturating_sub(1).max(1);
    out.inner
}

enum Peek {
    Revert,
    Close,
    Next,
    Prev,
    Inline,
}

/// VS Code-like inline diff of one hunk, floating under the changed lines.
#[allow(clippy::too_many_arguments)]
fn diff_peek(
    ui: &Ui,
    uid: u64,
    index: usize,
    count: usize,
    h: &crate::diff::Hunk,
    pos: Pos2,
    width: f32,
    font: &FontId,
    rh: f32,
    c: &crate::highlight::Colors,
) -> Option<Peek> {
    let mut action = None;
    let kind = match h.kind() {
        GitMark::Added => "Ajouté",
        GitMark::Deleted => "Supprimé",
        GitMark::Modified => "Modifié",
    };
    let removed_bg = Color32::from_rgba_unmultiplied(0xe0, 0x6c, 0x75, 45);
    let added_bg = Color32::from_rgba_unmultiplied(0x98, 0xc3, 0x79, 40);
    egui::Area::new(Id::new(("diff_peek", uid)))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .constrain(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).fill(c.bg).inner_margin(6.0).show(ui, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{kind} depuis le dernier commit")).strong());
                    ui.label(RichText::new(format!("changement {}/{count}", index + 1)).weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("×").on_hover_text("Fermer (Échap)").clicked() {
                            action = Some(Peek::Close);
                        }
                        if ui.button("⏷").on_hover_text("Changement suivant (Alt+F5)").clicked() {
                            action = Some(Peek::Next);
                        }
                        if ui.button("⏶").on_hover_text("Changement précédent (Alt+Maj+F5)").clicked() {
                            action = Some(Peek::Prev);
                        }
                        if ui.button("Dans l'éditeur").on_hover_text("Afficher tous les changements dans l'éditeur, en vert et rouge (Ctrl+Alt+D)").clicked() {
                            action = Some(Peek::Inline);
                        }
                        if ui
                            .button(RichText::new("↺ Revenir en arrière").color(c.fg))
                            .on_hover_text("Remettre la version du dernier commit pour ce bloc (annulable avec Ctrl+Z)")
                            .clicked()
                        {
                            action = Some(Peek::Revert);
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::both().max_height(rh * 16.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let mut row = |sign: char, text: &str, bg: Color32| {
                        let t = format!("{sign} {}", text.trim_end_matches(['\n', '\r']).replace('\t', "    "));
                        egui::Frame::none().fill(bg).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.add(egui::Label::new(RichText::new(t).font(font.clone()).color(c.fg)).extend());
                        });
                    };
                    for l in &h.old_lines {
                        row('-', l, removed_bg);
                    }
                    for l in &h.new_lines {
                        row('+', l, added_bg);
                    }
                });
            });
        });
    action
}

fn minimap_ui(ui: &mut Ui, doc: &mut Document, hl: &Highlighting, rh: f32) {
    let rect = ui.max_rect();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let lh = 2.0;
    let nlines = doc.rope.len_lines();
    let total = nlines as f32 * lh;
    let content_h = nlines as f32 * rh + (doc.view_h - 3.0 * rh).max(0.0);
    let max_scroll = (content_h - doc.view_h).max(1.0);
    let frac = (doc.scroll_y / max_scroll).clamp(0.0, 1.0);
    let off = (total - rect.height()).max(0.0) * frac;
    let first = (off / lh) as usize;
    let last = (first + (rect.height() / lh) as usize + 1).min(nlines);
    if last > 0 && !doc.highlight.ensure(hl, &doc.syntax, &doc.rope, last - 1, 20_000) {
        ui.ctx().request_repaint();
    }
    let painter = ui.painter_at(rect);
    let top = doc.scroll_y / rh;
    let y0 = rect.min.y + top * lh - off;
    let view = Rect::from_min_size(Pos2::new(rect.min.x, y0), egui::vec2(rect.width(), doc.view_h / rh * lh));
    // Opaque, slightly tinted color: translucent fills are blended very differently by
    // some OpenGL drivers (WSLg's software renderer makes them much brighter).
    let (a, bg) = (hl.colors.accent, hl.colors.bg);
    let tint = |x: u8, y: u8| (x as f32 * 0.93 + y as f32 * 0.07) as u8;
    painter.rect_filled(view, 3.0, Color32::from_rgb(tint(bg.r(), a.r()), tint(bg.g(), a.g()), tint(bg.b(), a.b())));
    let ts = doc.tab_size;
    for l in first..last {
        let y = rect.min.y + l as f32 * lh - off;
        let spans = doc.highlight.spans(l);
        let (mut v, mut byte, mut si) = (0usize, 0usize, 0usize);
        let mut run: Option<(usize, Color32)> = None;
        let flush = |run: &mut Option<(usize, Color32)>, end: usize| {
            if let Some((s, col)) = run.take() {
                let r = Rect::from_min_size(Pos2::new(rect.min.x + 4.0 + s as f32, y), egui::vec2((end - s) as f32, lh * 0.75));
                painter.rect_filled(r, 0.0, col.gamma_multiply(0.7));
            }
        };
        for ch in doc.rope.line(l).chars() {
            if ch == '\n' || v > 100 {
                break;
            }
            let color = spans
                .and_then(|sp| {
                    while si < sp.len() && sp[si].1.end <= byte {
                        si += 1;
                    }
                    sp.get(si).map(|s| s.0)
                })
                .unwrap_or(hl.colors.fg);
            let w = if ch == '\t' { ts - v % ts } else { 1 };
            if ch.is_whitespace() {
                flush(&mut run, v);
            } else {
                match run {
                    Some((_, c)) if c == color => {}
                    _ => {
                        flush(&mut run, v);
                        run = Some((v, color));
                    }
                }
            }
            v += w;
            byte += ch.len_utf8();
        }
        flush(&mut run, v);
    }
    if resp.clicked() || resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            let line = (p.y - rect.min.y + off) / lh;
            doc.scroll_request = Some(line * rh - doc.view_h / 2.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cursor position (column × char_width) must match where the text is really drawn.
    #[test]
    fn pinned_lines_are_the_enclosing_scopes() {
        let text = "class A:\n    x = 1\n\n    def f(self):\n        a = 1\n        b = 2\n        c = 3\n        d = 4\n\n\nclass B:\n    pass\n";
        let mut d = Document::from_text(1, text, 4, true);
        d.syntax = "Python".into();
        // top of the view on "a = 1" (line 4): class A and def f are above it
        assert_eq!(sticky_lines(&mut d, 4), [0, 3]);
        // on "x = 1": only the class
        assert_eq!(sticky_lines(&mut d, 1), [0]);
        // at the class line itself, or after it: nothing
        assert!(sticky_lines(&mut d, 0).is_empty());
        assert_eq!(sticky_lines(&mut d, 11), [10]);
    }

    #[test]
    fn cursor_matches_drawn_text() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |_| {});
        let _ = ctx.run(Default::default(), |ctx| {
            for size in [12.0, 13.0, 14.0, 15.5, 18.0] {
                let font = FontId::monospace(size);
                ctx.fonts(|f| {
                    let cw = char_width(f, &font);
                    for line in ["        return f\"{self.name} <{self.email}>\"", "x = 'é à ’ « » … €'", &"y".repeat(200)] {
                        let drawn = f.layout_no_wrap(line.to_string(), font.clone(), Color32::WHITE).size().x;
                        let computed = line.chars().count() as f32 * cw;
                        assert!((drawn - computed).abs() < 0.5, "size {size}: drawn {drawn} vs cursor {computed} for {line:?}");
                    }
                });
            }
        });
    }
}

/// Tooltip just under the hovered line, after the usual delay: egui places the tooltip of a
/// widget under it, which for the whole editor means at the top or bottom of the window.
fn tooltip_under_line(ui: &Ui, resp: &egui::Response, p: Pos2, top: f32, rh: f32, text: String) {
    let delay = ui.style().interaction.tooltip_delay;
    let still = ui.ctx().input(|i| i.pointer.time_since_last_movement());
    if still < delay {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f32(delay - still + 0.01));
        return;
    }
    let bottom = top + (((p.y - top) / rh).floor() + 1.0) * rh;
    egui::show_tooltip_at(ui.ctx(), ui.layer_id(), resp.id.with("line_tip"), egui::pos2(p.x, bottom + 2.0), |ui| {
        ui.set_max_width(600.0);
        ui.label(text);
    });
}
