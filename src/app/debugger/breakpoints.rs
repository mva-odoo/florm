//! Breakpoints: kept with the project, shown in the margin, sent to the program (with their
//! conditions) and to the exceptions it should stop on.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::app) struct Bp {
    /// 0-based.
    pub(super) line: usize,
    pub(super) condition: String,
    /// What the debugger said of it: verified, why not.
    pub(super) state: Option<(bool, String)>,
}

impl Bp {
    pub(super) fn new(line: usize) -> Self {
        Bp { line, condition: String::new(), state: None }
    }
}

/// The breakpoints of a file once its lines moved or changed: the conditions follow the lines
/// in order when they only moved, else stay on the lines that are still there.
pub(super) fn merge_bps(old: &[Bp], lines: &[usize]) -> Vec<Bp> {
    if old.len() == lines.len() {
        return old.iter().zip(lines).map(|(b, &l)| Bp { line: l, ..b.clone() }).collect();
    }
    lines.iter().map(|&l| old.iter().find(|b| b.line == l).cloned().unwrap_or_else(|| Bp::new(l))).collect()
}

impl App {
    pub(in crate::app) fn debug_session_breakpoints(&self) -> Vec<SessionBreakpoint> {
        self.debug.bps.iter().flat_map(|(p, v)| v.iter().map(|b| SessionBreakpoint { path: p.clone(), line: b.line, condition: b.condition.clone() })).collect()
    }

    pub(in crate::app) fn debug_set_breakpoints(&mut self, list: Vec<SessionBreakpoint>) {
        self.debug.bps.clear();
        for b in list {
            let v = self.debug.bps.entry(b.path).or_default();
            if !v.iter().any(|x| x.line == b.line) {
                v.push(Bp { line: b.line, condition: b.condition, state: None });
            }
        }
        for v in self.debug.bps.values_mut() {
            v.sort_by_key(|b| b.line);
        }
    }

    /// The breakpoints of the open files follow their edits; a click in the gutter (or F9) adds
    /// or removes one.
    pub(super) fn debug_sync_bps(&mut self) {
        let ui = &mut self.debug;
        for d in self.docs.iter_mut().filter(|d| d.view == document::View::Text) {
            let Some(path) = d.path.clone().filter(|p| debuggable(p)) else { continue };
            if d.breakpoints.is_none() {
                let lines: Vec<usize> = ui.bps.get(&path).map(|v| v.iter().map(|b| b.line).collect()).unwrap_or_default();
                d.set_breakpoint_lines(&lines);
            }
            let mut lines = d.breakpoint_lines().unwrap_or_default();
            if let Some(l) = d.bp_pick.take() {
                match lines.iter().position(|&x| x == l) {
                    Some(k) => {
                        lines.remove(k);
                    }
                    None => lines.push(l),
                }
                lines.sort_unstable();
                d.set_breakpoint_lines(&lines);
                lines = d.breakpoint_lines().unwrap_or_default();
            }
            let old = ui.bps.get(&path).map_or(&[][..], |v| &v[..]);
            if old.len() == lines.len() && old.iter().zip(&lines).all(|(b, l)| b.line == *l) {
                continue;
            }
            let new = merge_bps(old, &lines);
            if new.is_empty() {
                ui.bps.remove(&path);
            } else {
                ui.bps.insert(path.clone(), new);
            }
            ui.dirty.insert(path);
        }
    }

    pub(in crate::app) fn debug_toggle_breakpoint(&mut self) {
        let Some(d) = self.docs.get_mut(self.active).filter(|d| d.view == document::View::Text) else { return };
        if !d.path.as_deref().is_some_and(debuggable) {
            self.set_status(t!("Points d'arrêt : dans les fichiers Python", "Breakpoints: in Python files"));
            return;
        }
        d.bp_pick = Some(d.rope.char_to_line(d.primary_sel().head.min(d.rope.len_chars())));
    }

    pub(in crate::app) fn debug_clear_breakpoints(&mut self) {
        let paths: Vec<PathBuf> = self.debug.bps.keys().cloned().collect();
        self.debug.dirty.extend(paths);
        self.debug.bps.clear();
        for d in self.docs.iter_mut().filter(|d| d.breakpoints.is_some()) {
            d.set_breakpoint_lines(&[]);
        }
        self.set_status(t!("Points d'arrêt retirés", "Breakpoints removed"));
    }

    pub(super) fn debug_remove_bp(&mut self, path: PathBuf, line: usize) {
        if let Some(v) = self.debug.bps.get_mut(&path) {
            v.retain(|b| b.line != line);
            if v.is_empty() {
                self.debug.bps.remove(&path);
            }
        }
        let lines: Vec<usize> = self.debug.bps.get(&path).map(|v| v.iter().map(|b| b.line).collect()).unwrap_or_default();
        if let Some(d) = self.docs.iter_mut().find(|d| d.path.as_ref() == Some(&path) && d.breakpoints.is_some()) {
            d.set_breakpoint_lines(&lines);
        }
        self.debug.dirty.insert(path);
    }

    pub(super) fn debug_send_bps(&mut self, path: &Path) {
        let list = self.debug.bps.get(path).cloned().unwrap_or_default();
        let Some(s) = self.debug.session.as_mut().filter(|s| s.configured) else { return };
        let remote = dap::remote_path(path, &s.cfg.target, &s.mounts);
        let bps: Vec<Value> = list
            .iter()
            .map(|b| {
                let mut v = json!({ "line": b.line + 1 });
                if !b.condition.trim().is_empty() {
                    v["condition"] = json!(b.condition.trim());
                }
                v
            })
            .collect();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let seq = s.client.request("setBreakpoints", json!({ "source": { "path": remote, "name": name }, "breakpoints": bps }));
        s.pending.insert(seq, Req::Bps(path.to_path_buf()));
    }

    pub(super) fn debug_send_exceptions(&mut self) {
        let filters: Vec<String> = self.settings.debug_exceptions.iter().filter(|f| ["raised", "uncaught", "userUnhandled"].contains(&f.as_str())).cloned().collect();
        let Some(s) = self.debug.session.as_mut() else { return };
        let seq = s.client.request("setExceptionBreakpoints", json!({ "filters": filters }));
        s.pending.insert(seq, Req::Other);
    }
}
