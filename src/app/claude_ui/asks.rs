//! Mode « Demander avant » : Claude changes nothing by itself. Each change of a file and each
//! command waits here, above the field: applied as it is, changed first, or refused (with a word
//! for Claude).

use super::*;
use serde_json::Value;

/// A tool call waiting for the user (`Event::Ask`).
pub(in crate::app) struct Ask {
    /// Id of the request (for the answer), the tool, its input as proposed, the id of its call.
    request: String,
    tool: String,
    input: Value,
    /// The texts the user can change (the new text of each replacement, the content of a file
    /// written, the command), in the order of `editable`.
    texts: Vec<String>,
    /// Shown as a text to change (else as a diff).
    editing: bool,
    /// Why it is refused (Claude reads it), optional.
    reason: String,
}

/// What the user decided for an ask (its index).
pub(super) enum AskAct {
    Allow(usize),
    Deny(usize),
}

/// The fields of `input` the user can change: (path in the input, as JSON pointer).
fn editable(tool: &str, input: &Value) -> Vec<String> {
    match tool {
        "Edit" => vec!["/new_string".into()],
        "MultiEdit" => (0..input["edits"].as_array().map_or(0, Vec::len)).map(|k| format!("/edits/{k}/new_string")).collect(),
        "Write" => vec!["/content".into()],
        "NotebookEdit" => vec!["/new_source".into()],
        "Bash" => vec!["/command".into()],
        _ => Vec::new(),
    }
}

impl Ask {
    pub(super) fn new(request: String, tool: String, input: Value) -> Ask {
        let texts = editable(&tool, &input).iter().map(|p| input.pointer(p).and_then(Value::as_str).unwrap_or("").to_string()).collect();
        Ask { request, tool, input, texts, editing: false, reason: String::new() }
    }

    /// The user changed what Claude proposed.
    fn changed(&self) -> bool {
        editable(&self.tool, &self.input).iter().zip(&self.texts).any(|(p, t)| self.input.pointer(p).and_then(Value::as_str).unwrap_or("") != t)
    }

    /// The input the tool runs with: the one proposed, with the texts of the user.
    fn decided(&self) -> Value {
        let mut v = self.input.clone();
        for (p, t) in editable(&self.tool, &self.input).iter().zip(&self.texts) {
            if let Some(x) = v.pointer_mut(p) {
                *x = Value::String(t.clone());
            }
        }
        v
    }

    fn path(&self) -> String {
        let s = |k: &str| self.input[k].as_str().unwrap_or("").to_string();
        let p = s("file_path");
        if p.is_empty() {
            s("notebook_path")
        } else {
            p
        }
    }

    /// What Claude is told when its proposal was changed before it ran: the tool succeeded, but
    /// not with what it wrote.
    fn note(&self) -> String {
        let what = match self.tool.as_str() {
            "Bash" => tf!("la commande a été lancée sous cette forme :\n```\n{}\n```", "the command was run as:\n```\n{}\n```", self.texts.join("\n")),
            _ => {
                let parts: Vec<String> = self.texts.iter().map(|t| format!("```\n{t}\n```")).collect();
                tf!("{} a été écrit avec ce texte à la place du tien :\n{}", "{} was written with this text instead of yours:\n{}", self.path(), parts.join("\n"))
            }
        };
        tf!("[Florm] L'utilisateur a modifié ta proposition avant de l'accepter : {what}\nTiens-en compte pour la suite (relis le fichier si besoin).", "[Florm] The user changed your proposal before accepting it: {what}\nTake it into account from now on (read the file again if needed).")
    }
}

/// Root of `path` relative to `dir` for the title.
fn short(path: &str, dir: Option<&Path>) -> String {
    dir.and_then(|d| Path::new(path).strip_prefix(d).ok()).map_or_else(|| path.to_string(), |p| p.to_string_lossy().into_owned())
}

/// The asks waiting, one card each, above the field. Returns what was decided.
pub(super) fn asks_ui(ui: &mut egui::Ui, asks: &mut [Ask], dir: Option<&Path>, fill: Color32, accent: Color32) -> Option<AskAct> {
    let mut act = None;
    let n = asks.len();
    egui::ScrollArea::vertical().id_salt("claude_asks").max_height(ui.ctx().screen_rect().height() * 0.5).auto_shrink([false, true]).show(ui, |ui| {
        for (k, a) in asks.iter_mut().enumerate() {
            let id = egui::Id::new(("ask", a.request.as_str()));
            ui.push_id(id, |ui| {
                egui::Frame::none().fill(icons::mix(fill, accent, 0.10)).stroke(egui::Stroke::new(1.0_f32, icons::mix(fill, accent, 0.5))).rounding(6.0).inner_margin(8.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(a) = card(ui, a, dir, accent) {
                        act = Some(match a {
                            true => AskAct::Allow(k),
                            false => AskAct::Deny(k),
                        });
                    }
                });
            });
            if k + 1 < n {
                ui.add_space(4.0);
            }
        }
    });
    ui.add_space(4.0);
    act
}

/// One ask: what Claude wants to do, what can be changed, and the buttons. Some(true): allowed.
fn card(ui: &mut egui::Ui, a: &mut Ask, dir: Option<&Path>, accent: Color32) -> Option<bool> {
    let path = short(&a.path(), dir);
    let exists = Path::new(&a.path()).exists();
    let title = match a.tool.as_str() {
        "Edit" | "MultiEdit" | "NotebookEdit" => tf!("Claude propose de modifier {path}", "Claude proposes to change {path}"),
        "Write" if exists => tf!("Claude propose de réécrire {path}", "Claude proposes to rewrite {path}"),
        "Write" => tf!("Claude propose de créer {path}", "Claude proposes to create {path}"),
        "Bash" => t!("Claude propose de lancer une commande", "Claude proposes to run a command").to_string(),
        t => tf!("Claude demande à utiliser {t}", "Claude asks to use {t}"),
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("✳").color(accent));
        ui.label(RichText::new(title).strong());
    });
    let code_rows = |t: &str| t.lines().count().clamp(1, 14);
    match a.tool.as_str() {
        "Bash" => {
            if let Some(d) = a.input["description"].as_str().filter(|d| !d.is_empty()) {
                ui.label(RichText::new(d).weak().small());
            }
            // a command is short: always editable
            let rows = code_rows(&a.texts[0]);
            crate::input::add(ui, egui::TextEdit::multiline(&mut a.texts[0]).code_editor().desired_rows(rows).desired_width(f32::INFINITY));
        }
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
            let olds: Vec<String> = match a.tool.as_str() {
                "Edit" => vec![a.input["old_string"].as_str().unwrap_or("").to_string()],
                "MultiEdit" => a.input["edits"].as_array().map(|e| e.iter().map(|x| x["old_string"].as_str().unwrap_or("").to_string()).collect()).unwrap_or_default(),
                // the file as it is now (empty: new)
                "Write" => vec![std::fs::read_to_string(a.path()).unwrap_or_default()],
                _ => vec![a.input["old_source"].as_str().unwrap_or("").to_string()],
            };
            if a.editing {
                for (k, t) in a.texts.iter_mut().enumerate() {
                    if k > 0 {
                        ui.add_space(4.0);
                    }
                    let rows = code_rows(t);
                    egui::ScrollArea::vertical().id_salt(("ask_edit", k)).max_height(300.0).show(ui, |ui| {
                        crate::input::add(ui, egui::TextEdit::multiline(t).code_editor().desired_rows(rows).desired_width(f32::INFINITY));
                    });
                }
            } else {
                egui::ScrollArea::both().id_salt("ask_diff").max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (k, (old, new)) in olds.iter().zip(&a.texts).enumerate() {
                        if k > 0 {
                            ui.add_space(4.0);
                        }
                        super::super::inline_ai::diff_lines(ui, old, new);
                    }
                });
            }
        }
        _ => {
            let summary = claude::tool_summary(&a.tool, &a.input);
            if !summary.is_empty() {
                ui.label(RichText::new(summary).monospace());
            }
            egui::CollapsingHeader::new(RichText::new(t!("Détails", "Details")).small()).id_salt("ask_json").show(ui, |ui| {
                let json = serde_json::to_string_pretty(&a.input).unwrap_or_default();
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| ui.add(egui::Label::new(RichText::new(json).monospace().small()).wrap()));
            });
        }
    }
    let mut decided = None;
    ui.horizontal_wrapped(|ui| {
        let changed = a.changed();
        let label = match (a.tool.as_str(), changed) {
            ("Bash", false) => t!("▶ Lancer", "▶ Run"),
            ("Bash", true) => t!("▶ Lancer ma version", "▶ Run my version"),
            ("Edit" | "MultiEdit" | "Write" | "NotebookEdit", false) => t!("✔ Appliquer", "✔ Apply"),
            ("Edit" | "MultiEdit" | "Write" | "NotebookEdit", true) => t!("✔ Appliquer ma version", "✔ Apply my version"),
            _ => t!("✔ Autoriser", "✔ Allow"),
        };
        let tip = if changed { t!("Claude est prévenu de ce que tu as changé", "Claude is told what you changed") } else { t!("Tel que Claude le propose", "As Claude proposes it") };
        if ui.add(filled_button(label, accent)).on_hover_text(tip).clicked() {
            decided = Some(true);
        }
        if !a.texts.is_empty() && a.tool != "Bash" {
            let (label, tip) = if a.editing {
                (t!("± Voir le diff", "± See the diff"), t!("Revoir la proposition (avec tes changements) en diff", "See the proposal (with your changes) as a diff"))
            } else {
                (t!("✏ Modifier", "✏ Edit"), t!("Changer le texte avant de l'appliquer", "Change the text before applying it"))
            };
            if ui.button(label).on_hover_text(tip).clicked() {
                a.editing = !a.editing;
            }
        }
        if changed && ui.small_button(t!("↺ Version de Claude", "↺ Claude's version")).clicked() {
            *a = Ask::new(std::mem::take(&mut a.request), std::mem::take(&mut a.tool), a.input.take());
        }
        if ui.button(RichText::new(t!("✖ Refuser", "✖ Refuse")).color(err())).on_hover_text(t!("Rien n'est fait ; Claude lit la raison, s'il y en a une, et propose autre chose ou s'arrête", "Nothing is done; Claude reads the reason, if any, and proposes something else or stops")).clicked() {
            decided = Some(false);
        }
        crate::input::add(ui, egui::TextEdit::singleline(&mut a.reason).hint_text(t!("Pourquoi refuser (facultatif)", "Why refuse (optional)")).desired_width(ui.available_width().clamp(120.0, 260.0)));
    });
    decided
}

impl App {
    /// The decision of the user on an ask: sent to Claude (with a note when the proposal was changed).
    pub(super) fn claude_ask_act(&mut self, act: AskAct) {
        let c = &mut self.claude;
        let (k, allow) = match act {
            AskAct::Allow(k) => (k, true),
            AskAct::Deny(k) => (k, false),
        };
        if k >= c.asks.len() {
            return;
        }
        let a = c.asks.remove(k);
        let Some(s) = c.session.as_mut() else { return };
        let res = if allow {
            let r = s.answer(&a.request, Ok(a.decided()));
            if r.is_ok() && a.changed() {
                c.items.push(Item::Info(t!("✏ Ta version a été appliquée : Claude en est prévenu.", "✏ Your version was applied: Claude is told.").into()));
                s.send(&a.note())
            } else {
                r
            }
        } else {
            let why = a.reason.trim();
            let msg = if why.is_empty() {
                t!("L'utilisateur a refusé.", "The user refused.").to_string()
            } else {
                tf!("L'utilisateur a refusé : {why}", "The user refused: {why}")
            };
            s.answer(&a.request, Err(msg))
        };
        if let Err(e) = res {
            c.items.push(Item::Error(e));
        }
        if c.asks.is_empty() {
            c.activity = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_user_changes_the_proposal() {
        let mut a = Ask::new("r".into(), "MultiEdit".into(), json!({"file_path": "/p/a.py", "edits": [{"old_string": "a", "new_string": "b"}, {"old_string": "c", "new_string": "d"}]}));
        assert_eq!(a.texts, ["b", "d"]);
        assert!(!a.changed());
        a.texts[1] = "D".into();
        assert!(a.changed());
        let v = a.decided();
        assert_eq!((v["edits"][0]["new_string"].as_str(), v["edits"][1]["new_string"].as_str(), v["edits"][1]["old_string"].as_str()), (Some("b"), Some("D"), Some("c")));
        assert!(a.note().contains("/p/a.py") && a.note().contains("D"));
        // a command: changed as a whole; a tool without text: as proposed
        let mut b = Ask::new("r".into(), "Bash".into(), json!({"command": "rm -rf build", "description": "x"}));
        b.texts[0] = "rm -rf build/tmp".into();
        assert_eq!(b.decided()["command"], "rm -rf build/tmp");
        let w = Ask::new("r".into(), "WebFetch".into(), json!({"url": "https://x"}));
        assert!(w.texts.is_empty() && !w.changed() && w.decided() == json!({"url": "https://x"}));
    }
}
