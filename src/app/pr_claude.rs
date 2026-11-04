//! The comments of a pull request handled by Claude: the conversations on lines ticked, then
//! « Envoyer à Claude » gives them to a new conversation in the repository, to change the code.

use super::*;
use crate::github;

/// Most lines of the commented code given for a comment (its end: the lines commented).
const HUNK_LINES: usize = 12;

/// The box that ticks a conversation for Claude.
pub(super) fn to_claude_tick(ui: &mut egui::Ui, ticked: &mut HashSet<String>, id: &str) {
    let mut on = ticked.contains(id);
    if ui.checkbox(&mut on, "").on_hover_text(t!("À traiter par Claude (« Envoyer à Claude » en haut)", "To be handled by Claude (“Send to Claude” at the top)")).changed() {
        if on {
            ticked.insert(id.to_string());
        } else {
            ticked.remove(id);
        }
    }
}

/// Above the conversations: tick them all, and send those ticked to Claude.
pub(super) fn to_claude_bar(ui: &mut egui::Ui, d: &github::PrDetail, ticked: &mut HashSet<String>, checked_out: bool, accent: Color32, act: &mut Option<super::pulls::PrAct>) {
    let open: Vec<&str> = d.threads.iter().filter(|t| !t.resolved).map(|t| t.id.as_str()).collect();
    // those resolved since are no longer handled
    ticked.retain(|id| open.contains(&id.as_str()));
    ui.add_space(4.0);
    egui::Frame::none().fill(ui.visuals().faint_bg_color).rounding(6.0).inner_margin(egui::Margin::symmetric(10.0, 6.0)).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let mut all = ticked.len() == open.len();
            if ui.checkbox(&mut all, t!("Tous", "All")).on_hover_text(t!("Cocher toutes les conversations non résolues", "Tick every unresolved conversation")).changed() {
                ticked.clear();
                if all {
                    ticked.extend(open.iter().map(|s| s.to_string()));
                }
            }
            ui.label(RichText::new(tf!("{} / {} commentaire(s) à traiter par Claude", "{} / {} comment(s) to be handled by Claude", ticked.len(), open.len())).small().weak());
            let label = tf!("✳ Envoyer à Claude ({})", "✳ Send to Claude ({})", ticked.len());
            let r = ui
                .add_enabled(!ticked.is_empty() && checked_out, filled_button(label, accent))
                .on_hover_text(t!("Une nouvelle conversation Claude dans le dépôt reçoit ces commentaires (fichier, ligne, code commenté, réponses) et change le code en conséquence ; tu relis ses changements dans la revue", "A new Claude conversation in the repository gets these comments (file, line, commented code, replies) and changes the code accordingly; you check its changes in the review"))
                .on_disabled_hover_text(if checked_out { t!("Coche d'abord des commentaires", "First tick some comments") } else { t!("Fais d'abord « ⤵ Checkout » : Claude doit travailler sur la branche de la pull request", "First “⤵ Checkout”: Claude must work on the branch of the pull request") });
            if r.clicked() {
                *act = Some(super::pulls::PrAct::ToClaude);
            }
        });
    });
}

/// The message that asks Claude to handle the conversations `ids` of pull request `d`.
pub(super) fn request(d: &github::PrDetail, ids: &HashSet<String>) -> String {
    let mut out = tf!(
        "Traite ces commentaires de revue de la pull request #{} « {} » (branche {} → {}). Pour chacun : fais le changement demandé dans le code ; s'il ne faut rien changer, ou si tu n'es pas d'accord, dis-le avec la raison. Ne réponds pas sur GitHub et ne commite pas : je relis tes changements.",
        "Handle these review comments of pull request #{} “{}” (branch {} → {}). For each one: make the change asked in the code; if nothing should change, or you disagree, say so with the reason. Do not answer on GitHub and do not commit: I check your changes.",
        d.number,
        d.title,
        d.head,
        d.base
    );
    let mut k = 0;
    for t in d.threads.iter().filter(|t| ids.contains(&t.id)) {
        let Some(first) = t.comments.first() else { continue };
        k += 1;
        let line = t.line.or(t.original_line).map(|l| format!(":{l}")).unwrap_or_default();
        let lines = match (t.start_line, t.line) {
            (Some(a), Some(b)) if a != b => format!(":{a}-{b}"),
            _ => line,
        };
        let outdated = if t.outdated { t!(" (le code a changé depuis)", " (the code changed since)") } else { "" };
        out += &format!("\n\n{k}. `{}{lines}`{outdated} — @{} :\n{}", t.path, first.author, quote(&first.body));
        for r in &t.comments[1..] {
            out += &format!("\n   {} @{} :\n{}", t!("Réponse de", "Reply of"), r.author, quote(&r.body));
        }
        let hunk: Vec<&str> = first.diff_hunk.lines().collect();
        if !hunk.is_empty() {
            let end = &hunk[hunk.len().saturating_sub(HUNK_LINES)..];
            out += &format!("\n   {}\n   ```diff\n{}\n   ```", t!("Code commenté :", "Commented code:"), end.iter().map(|l| format!("   {l}")).collect::<Vec<_>>().join("\n"));
        }
    }
    out
}

/// A comment quoted, under its item.
fn quote(body: &str) -> String {
    body.trim().lines().map(|l| format!("   > {l}")).collect::<Vec<_>>().join("\n")
}

impl App {
    /// « Envoyer à Claude »: a new conversation in the repository of the pull request handles the
    /// comments ticked.
    pub(super) fn pr_to_claude(&mut self, repo: PathBuf) {
        let Some(Ok(d)) = &self.pr.detail else { return };
        if self.pr.to_claude.is_empty() {
            return;
        }
        let text = request(d, &self.pr.to_claude);
        let n = self.pr.to_claude.len();
        // the conversation shown, if nothing was said in it yet in this folder; else a new one
        let empty = self.claude.session.is_none() && self.claude.items.iter().all(|i| matches!(i, claude_ui::Item::Info(_)));
        if !(empty && self.claude.dir.as_deref() == Some(repo.as_path())) {
            self.claude_new_in(repo);
        }
        self.open_claude();
        if self.claude_send_text(text, false) {
            self.pr.to_claude.clear();
            self.set_status(tf!("{n} commentaire(s) envoyé(s) à Claude", "{n} comment(s) sent to Claude"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(author: &str, body: &str, hunk: &str) -> github::LineComment {
        github::LineComment { id: String::new(), db_id: 0, author: author.into(), body: body.into(), created_at: String::new(), diff_hunk: hunk.into(), mine: false, reactions: Vec::new() }
    }

    #[test]
    fn the_comments_ticked_make_the_request() {
        let thread = |id: &str, path: &str, line: usize, comments| github::Thread { id: id.into(), path: path.into(), line: Some(line), start_line: None, original_line: Some(line), side: "RIGHT".into(), resolved: false, outdated: false, comments };
        let d = github::PrDetail {
            number: 42,
            title: "Fix".into(),
            head: "17.0-fix".into(),
            base: "17.0".into(),
            threads: vec![
                thread("a", "models/x.py", 10, vec![comment("rev", "Use a set here", "@@ -1,2 +1,2 @@\n-a\n+b"), comment("me", "ok", "")]),
                thread("b", "views/y.xml", 3, vec![comment("rev", "Not this one", "")]),
            ],
            ..Default::default()
        };
        let r = request(&d, &HashSet::from(["a".to_string()]));
        assert!(r.contains("#42") && r.contains("17.0-fix"), "{r}");
        assert!(r.contains("1. `models/x.py:10`") && r.contains("> Use a set here") && r.contains("@me") && r.contains("+b"), "{r}");
        assert!(!r.contains("Not this one"), "{r}");
    }
}
