//! Mermaid diagrams of the Markdown, drawn by Florm (nothing leaves the computer): the sequence
//! diagrams for now. Another kind, or a diagram not understood, stays shown as its code.

use super::*;

/// How a message is drawn: its line (dashed or not) and its end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Head {
    /// `->>`: a filled arrow.
    Arrow,
    /// `->`: no arrow.
    None,
    /// `-x`: a cross.
    Cross,
    /// `-)`: an open arrow (asynchronous).
    Open,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Step {
    /// A message: from, to (indices of the participants), dashed, its end, its text.
    Msg { from: usize, to: usize, dashed: bool, head: Head, text: String },
    /// A note: the first and last participants it covers, where (0 over, -1 left of, 1 right of), its text.
    Note { from: usize, to: usize, side: i8, text: String },
    /// `loop` / `alt` / `opt` / `par` / `critical` / `break` / `rect`: a frame opened, its kind and label.
    Open { kind: String, label: String },
    /// `else` / `and` / `option`: a new part of the open frame, its label.
    Part { label: String },
    /// `end`: the frame closed.
    Close,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Sequence {
    /// The participants: (id, label, an actor).
    pub(super) actors: Vec<(String, String, bool)>,
    pub(super) steps: Vec<Step>,
    pub(super) numbered: bool,
    pub(super) title: String,
}

/// `<br/>` and `<br>` in a text of the diagram: a new line.
fn lines_of(t: &str) -> String {
    let mut s = t.trim().to_string();
    for br in ["<br/>", "<br />", "<br>", "<BR>", "<BR/>"] {
        s = s.replace(br, "\n");
    }
    s.replace("#59;", ";")
}

impl Sequence {
    fn actor(&mut self, id: &str) -> usize {
        let id = id.trim();
        if let Some(k) = self.actors.iter().position(|a| a.0 == id) {
            return k;
        }
        self.actors.push((id.to_string(), id.to_string(), false));
        self.actors.len() - 1
    }
}

/// A sequence diagram read from its code; None when it is not one, or a line is not understood.
pub(super) fn parse_sequence(code: &str) -> Option<Sequence> {
    let mut lines = code.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("%%"));
    if lines.next()? != "sequenceDiagram" {
        return None;
    }
    let mut s = Sequence::default();
    // the arrows, longest first (`-->>` before `->>` and `->`)
    const ARROWS: [(&str, bool, Head); 8] = [
        ("-->>", true, Head::Arrow),
        ("->>", false, Head::Arrow),
        ("--x", true, Head::Cross),
        ("-x", false, Head::Cross),
        ("--)", true, Head::Open),
        ("-)", false, Head::Open),
        ("-->", true, Head::None),
        ("->", false, Head::None),
    ];
    for line in lines {
        let (word, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(w, r)| (w, r.trim()));
        match word {
            "participant" | "actor" => {
                let (id, label) = rest.split_once(" as ").map_or((rest, rest), |(i, l)| (i.trim(), l.trim()));
                let k = s.actor(id);
                s.actors[k].1 = lines_of(label);
                s.actors[k].2 = word == "actor";
            }
            "autonumber" => s.numbered = true,
            "title" => s.title = rest.trim_start_matches(':').trim().to_string(),
            "activate" | "deactivate" | "create" | "destroy" | "box" => {}
            "loop" | "alt" | "opt" | "par" | "critical" | "break" | "rect" => s.steps.push(Step::Open { kind: word.to_string(), label: lines_of(rest) }),
            "else" | "and" | "option" => s.steps.push(Step::Part { label: lines_of(rest) }),
            "end" => s.steps.push(Step::Close),
            _ if word.eq_ignore_ascii_case("note") => {
                let (place, text) = rest.split_once(':')?;
                let place = place.trim();
                let (side, who) = if let Some(w) = place.strip_prefix("over ") {
                    (0, w)
                } else if let Some(w) = place.strip_prefix("left of ") {
                    (-1, w)
                } else {
                    (1, place.strip_prefix("right of ")?)
                };
                let mut ids = who.split(',').map(|w| s.actor(w));
                let a = ids.next()?;
                let b = ids.next().unwrap_or(a);
                s.steps.push(Step::Note { from: a.min(b), to: a.max(b), side, text: lines_of(text) });
            }
            _ => {
                let (head_part, text) = line.split_once(':').map_or((line, ""), |(h, t)| (h, t));
                let (at, arrow, dashed, head) = ARROWS.iter().filter_map(|(a, d, h)| head_part.find(a).map(|i| (i, *a, *d, *h))).min_by_key(|(i, a, ..)| (*i, usize::MAX - a.len()))?;
                let from = head_part[..at].trim();
                // `+` / `-` before the target: its activation, not drawn
                let to = head_part[at + arrow.len()..].trim().trim_start_matches(['+', '-']).trim();
                if from.is_empty() || to.is_empty() {
                    return None;
                }
                let (from, to) = (s.actor(from), s.actor(to));
                s.steps.push(Step::Msg { from, to, dashed, head, text: lines_of(text) });
            }
        }
    }
    (!s.actors.is_empty()).then_some(s)
}

/// Draws a Mermaid block; false when it is not drawn (another kind of diagram, not understood).
pub(super) fn show(ui: &mut egui::Ui, code: &str) -> bool {
    let Some(seq) = parse_sequence(code) else { return false };
    egui::ScrollArea::horizontal().id_salt(("mermaid", code)).auto_shrink([false, true]).show(ui, |ui| draw_sequence(ui, &seq));
    true
}

fn draw_sequence(ui: &mut egui::Ui, s: &Sequence) {
    let font = egui::FontId::proportional(13.0);
    let small = egui::FontId::proportional(11.5);
    let v = ui.visuals().clone();
    let fg = v.text_color();
    let weak = v.weak_text_color();
    let box_fill = v.widgets.inactive.weak_bg_fill;
    let line = egui::Stroke::new(1.2_f32, fg);
    let note_fill = egui::Color32::from_rgba_unmultiplied(230, 200, 90, if v.dark_mode { 40 } else { 70 });
    let ctx = ui.ctx().clone();
    let size = |t: &str, f: &egui::FontId| ctx.fonts(|fs| fs.layout_no_wrap(t.to_string(), f.clone(), fg).size());
    let n = s.actors.len();

    // the space between each two lifelines: their boxes, then each message over the gaps it crosses
    // (the shortest first)
    let box_w: Vec<f32> = s.actors.iter().map(|a| size(&a.1, &font).x + 24.0).collect();
    let extra = if s.numbered { 24.0 } else { 0.0 };
    let mut gaps: Vec<f32> = (1..n).map(|i| ((box_w[i - 1] + box_w[i]) / 2.0 + 24.0).max(110.0)).collect();
    let mut needs: Vec<(usize, usize, f32)> = Vec::new();
    for st in &s.steps {
        match st {
            Step::Msg { from, to, text, .. } if from != to => needs.push((*from.min(to), *from.max(to), size(text, &font).x + 30.0 + extra)),
            Step::Note { from, to, side: 0, text } if from != to => needs.push((*from, *to, size(text, &font).x + 20.0)),
            _ => {}
        }
    }
    needs.sort_by_key(|(a, b, _)| b - a);
    for (a, b, w) in needs {
        let have: f32 = gaps[a..b].iter().sum();
        if have < w {
            let add = (w - have) / (b - a) as f32;
            gaps[a..b].iter_mut().for_each(|g| *g += add);
        }
    }
    let mut xs = vec![0.0_f32];
    for g in &gaps {
        xs.push(xs.last().copied().unwrap_or(0.0) + g);
    }
    // where a note goes, from its text width
    let note_x = |from: usize, to: usize, side: i8, w: f32| match side {
        -1 => (xs[from] - 12.0 - w, xs[from] - 12.0),
        1 => (xs[to] + 12.0, xs[to] + 12.0 + w),
        _ => {
            let w = w.max(xs[to] - xs[from] + 40.0);
            let c = (xs[from] + xs[to]) / 2.0;
            (c - w / 2.0, c + w / 2.0)
        }
    };
    // the extent of everything: boxes, notes, messages to oneself (their text on the right)
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for k in 0..n {
        lo = lo.min(xs[k] - box_w[k] / 2.0);
        hi = hi.max(xs[k] + box_w[k] / 2.0);
    }
    for st in &s.steps {
        match st {
            Step::Note { from, to, side, text } => {
                let (l, r) = note_x(*from, *to, *side, size(text, &font).x + 20.0);
                lo = lo.min(l);
                hi = hi.max(r);
            }
            Step::Msg { from, to, text, .. } if from == to => hi = hi.max(xs[*from] + size(text, &font).x + 50.0 + extra),
            _ => {}
        }
    }
    let box_h = s.actors.iter().map(|a| size(&a.1, &font).y).fold(0.0, f32::max) + 12.0;
    let figure_h = if s.actors.iter().any(|a| a.2) { 26.0 } else { 0.0 };
    let head_h = box_h + figure_h + 4.0;
    let title_h = if s.title.is_empty() { 0.0 } else { 26.0 };
    // the height of each step
    let heights: Vec<f32> = s
        .steps
        .iter()
        .map(|st| match st {
            Step::Msg { from, to, text, .. } => size(text, &font).y + if from == to { 34.0 } else { 18.0 },
            Step::Note { text, .. } => size(text, &font).y + 22.0,
            Step::Open { .. } | Step::Part { .. } => 28.0,
            Step::Close => 12.0,
        })
        .collect();
    let body_h: f32 = heights.iter().sum::<f32>() + 16.0;
    let pad = 10.0;
    let width = hi - lo + 2.0 * pad;
    let height = title_h + head_h * 2.0 + body_h;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let p = ui.painter_at(rect.expand(2.0));
    let shift = rect.left() + pad - lo;
    let x = |k: usize| shift + xs[k];
    if !s.title.is_empty() {
        p.text(egui::pos2(rect.center().x, rect.top() + 10.0), Align2::CENTER_TOP, &s.title, egui::FontId::proportional(14.5), fg);
    }

    // participants at the top and at the bottom (all their boxes on one line), their lifelines between
    let top = rect.top() + title_h;
    let bottom = top + head_h + body_h;
    for (k, (_, label, actor)) in s.actors.iter().enumerate() {
        for y in [top, bottom] {
            let box_top = y + figure_h;
            if *actor {
                // a small figure above its name
                let c = egui::pos2(x(k), box_top - 21.0);
                p.circle_stroke(c, 5.0, line);
                p.line_segment([egui::pos2(c.x, c.y + 5.0), egui::pos2(c.x, c.y + 14.0)], line);
                p.line_segment([egui::pos2(c.x - 7.0, c.y + 9.0), egui::pos2(c.x + 7.0, c.y + 9.0)], line);
                p.line_segment([egui::pos2(c.x, c.y + 14.0), egui::pos2(c.x - 6.0, c.y + 20.0)], line);
                p.line_segment([egui::pos2(c.x, c.y + 14.0), egui::pos2(c.x + 6.0, c.y + 20.0)], line);
            }
            let r = egui::Rect::from_min_size(egui::pos2(x(k) - box_w[k] / 2.0, box_top), egui::vec2(box_w[k], box_h));
            p.rect(r, 5.0, box_fill, egui::Stroke::new(1.0_f32, weak));
            p.text(r.center(), Align2::CENTER_CENTER, label, font.clone(), fg);
        }
        dashed(&p, egui::pos2(x(k), top + head_h), egui::pos2(x(k), bottom + figure_h), egui::Stroke::new(1.0_f32, weak));
    }

    // the steps, top to bottom; the frames drawn when they close
    let mut y = top + head_h + 8.0;
    let mut frames: Vec<(f32, String, String, Vec<(f32, String)>)> = Vec::new();
    let mut number = 0;
    let (left, right) = (rect.left() + 4.0, rect.left() + width - 4.0);
    for (st, h) in s.steps.iter().zip(&heights) {
        match st {
            Step::Msg { from, to, dashed: d, head, text } => {
                number += 1;
                let shown = if s.numbered { format!("{number}. {text}") } else { text.clone() };
                let ty = size(&shown, &font).y;
                let (a, b) = (x(*from), x(*to));
                let stroke = egui::Stroke::new(1.3_f32, fg);
                if from == to {
                    // a message to itself: a loop on the right
                    let (y1, y2) = (y + ty + 6.0, y + ty + 24.0);
                    p.text(egui::pos2(a + 8.0, y + 2.0), Align2::LEFT_TOP, &shown, font.clone(), fg);
                    let pts = [egui::pos2(a, y1), egui::pos2(a + 34.0, y1), egui::pos2(a + 34.0, y2), egui::pos2(a + 3.0, y2)];
                    for w in pts.windows(2) {
                        segment(&p, w[0], w[1], stroke, *d);
                    }
                    arrow_head(&p, egui::pos2(a + 34.0, y2), egui::pos2(a + 1.0, y2), *head, stroke);
                } else {
                    let ly = y + ty + 8.0;
                    p.text(egui::pos2((a + b) / 2.0, y + 2.0), Align2::CENTER_TOP, &shown, font.clone(), fg);
                    let end = if b > a { b - 2.0 } else { b + 2.0 };
                    segment(&p, egui::pos2(a, ly), egui::pos2(end, ly), stroke, *d);
                    arrow_head(&p, egui::pos2(a, ly), egui::pos2(end, ly), *head, stroke);
                }
            }
            Step::Note { from, to, side, text } => {
                let sz = size(text, &font);
                let w = sz.x + 20.0;
                let (l, r) = note_x(*from, *to, *side, w);
                let r = egui::Rect::from_min_max(egui::pos2(shift + l, y + 4.0), egui::pos2(shift + r, y + h - 6.0));
                p.rect(r, 3.0, note_fill, egui::Stroke::new(1.0_f32, weak));
                p.text(r.center(), Align2::CENTER_CENTER, text, font.clone(), fg);
            }
            Step::Open { kind, label } => frames.push((y + 4.0, kind.clone(), label.clone(), Vec::new())),
            Step::Part { label } => {
                if let Some(f) = frames.last_mut() {
                    f.3.push((y + 4.0, label.clone()));
                }
            }
            Step::Close => {
                if let Some((fy, kind, label, parts)) = frames.pop() {
                    let depth = frames.len() as f32 * 6.0;
                    let r = egui::Rect::from_min_max(egui::pos2(left + depth, fy), egui::pos2(right - depth, y + 6.0));
                    p.rect_stroke(r, 2.0, egui::Stroke::new(1.0_f32, weak));
                    if kind != "rect" {
                        let tag = size(&kind, &small);
                        let t = egui::Rect::from_min_size(r.min, egui::vec2(tag.x + 14.0, tag.y + 6.0));
                        p.rect(t, egui::Rounding { se: 6.0, ..Default::default() }, box_fill, egui::Stroke::new(1.0_f32, weak));
                        p.text(t.center(), Align2::CENTER_CENTER, &kind, small.clone(), fg);
                        if !label.is_empty() {
                            p.text(egui::pos2(t.right() + 8.0, t.center().y), Align2::LEFT_CENTER, format!("[{label}]"), small.clone(), weak);
                        }
                    }
                    for (py, pl) in parts {
                        dashed(&p, egui::pos2(r.left(), py), egui::pos2(r.right(), py), egui::Stroke::new(1.0_f32, weak));
                        if !pl.is_empty() {
                            p.text(egui::pos2(r.left() + 8.0, py + 3.0), Align2::LEFT_TOP, format!("[{pl}]"), small.clone(), weak);
                        }
                    }
                }
            }
        }
        y += h;
    }
}

fn segment(p: &egui::Painter, a: egui::Pos2, b: egui::Pos2, stroke: egui::Stroke, dash: bool) {
    if dash {
        dashed(p, a, b, stroke);
    } else {
        p.line_segment([a, b], stroke);
    }
}

fn dashed(p: &egui::Painter, a: egui::Pos2, b: egui::Pos2, stroke: egui::Stroke) {
    p.extend(egui::Shape::dashed_line(&[a, b], stroke, 5.0, 4.0));
}

/// The end of a message going from `a` to `b`.
fn arrow_head(p: &egui::Painter, a: egui::Pos2, b: egui::Pos2, head: Head, stroke: egui::Stroke) {
    let dir = (b - a).normalized();
    let side = egui::vec2(-dir.y, dir.x);
    match head {
        Head::None => {}
        Head::Arrow => {
            p.add(egui::Shape::convex_polygon(vec![b, b - dir * 9.0 + side * 4.5, b - dir * 9.0 - side * 4.5], stroke.color, egui::Stroke::NONE));
        }
        Head::Open => {
            p.line_segment([b, b - dir * 9.0 + side * 4.5], stroke);
            p.line_segment([b, b - dir * 9.0 - side * 4.5], stroke);
        }
        Head::Cross => {
            let c = b - dir * 5.0;
            p.line_segment([c + egui::vec2(-4.0, -4.0), c + egui::vec2(4.0, 4.0)], stroke);
            p.line_segment([c + egui::vec2(-4.0, 4.0), c + egui::vec2(4.0, -4.0)], stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sequence_diagram_is_read() {
        let code = "sequenceDiagram\n    actor U as Utilisateur\n    participant S as Formulaire Session\n    participant W as Wizard (TransientModel)\n    participant DB as Session (Model)\n    U->>S: Action → Ajouter des participants\n    S->>W: ouvre la fenêtre (context: active_id = 7)\n    W->>W: default : session_id = browse(active_id)\n    W-->>U: la fenêtre se ferme\n    Note over W: l'enregistrement du wizard<br/>sera supprimé automatiquement";
        let s = parse_sequence(code).unwrap();
        assert_eq!(s.actors.len(), 4);
        assert_eq!(s.actors[0], ("U".into(), "Utilisateur".into(), true));
        assert_eq!(s.steps[0], Step::Msg { from: 0, to: 1, dashed: false, head: Head::Arrow, text: "Action → Ajouter des participants".into() });
        // only the first ":" ends the arrow part
        assert_eq!(s.steps[1], Step::Msg { from: 1, to: 2, dashed: false, head: Head::Arrow, text: "ouvre la fenêtre (context: active_id = 7)".into() });
        assert!(matches!(&s.steps[2], Step::Msg { from: 2, to: 2, .. }));
        assert!(matches!(&s.steps[3], Step::Msg { from: 2, to: 0, dashed: true, head: Head::Arrow, .. }));
        assert_eq!(s.steps[4], Step::Note { from: 2, to: 2, side: 0, text: "l'enregistrement du wizard\nsera supprimé automatiquement".into() });
    }

    /// The diagram of the example, laid out without a window: narrow enough for a preview beside the code.
    #[test]
    fn the_layout_stays_narrow() {
        let code = "sequenceDiagram\n    actor U as Utilisateur\n    participant S as Formulaire Session\n    participant W as Wizard (TransientModel)\n    participant DB as Session (Model)\n    U->>S: Action → Ajouter des participants\n    S->>W: ouvre la fenêtre (context: active_id = 7)\n    W->>W: default : session_id = browse(active_id)\n    U->>W: choisit des contacts, clique \"Ajouter\"\n    W->>DB: session.attendee_ids |= contacts\n    W-->>U: la fenêtre se ferme\n    Note over W: l'enregistrement du wizard<br/>sera supprimé automatiquement";
        let ctx = egui::Context::default();
        let mut size = egui::Vec2::ZERO;
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2000.0, 2000.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let r = ui.scope(|ui| draw_sequence(ui, &parse_sequence(code).unwrap())).response.rect;
                    size = r.size();
                });
            });
        }
        println!("diagram: {size:?}");
        assert!(size.x > 300.0 && size.x < 1100.0, "{size:?}");
    }

    #[test]
    fn frames_arrows_and_new_participants() {
        let s = parse_sequence("sequenceDiagram\nautonumber\nA-)+B: go\nloop every minute\nB--xA: no\nelse later\nA->B: x\nend\nNote left of A: n").unwrap();
        assert!(s.numbered);
        assert_eq!(s.actors.iter().map(|a| a.0.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert!(matches!(&s.steps[0], Step::Msg { head: Head::Open, dashed: false, to: 1, .. }));
        assert_eq!(s.steps[1], Step::Open { kind: "loop".into(), label: "every minute".into() });
        assert!(matches!(&s.steps[2], Step::Msg { head: Head::Cross, dashed: true, .. }));
        assert!(matches!(&s.steps[4], Step::Msg { head: Head::None, dashed: false, .. }));
        assert_eq!(s.steps[5], Step::Close);
        assert!(matches!(&s.steps[6], Step::Note { side: -1, .. }));
        // not a sequence diagram, or a line not understood: shown as code
        assert!(parse_sequence("graph TD\nA-->B").is_none());
        assert!(parse_sequence("sequenceDiagram\nwhat is this").is_none());
    }
}
