//! The panel next to a conversation: the review of the changes, or the session.

use super::*;

/// The review of the changes (or the session), on the right or taking all the room. True when
/// it takes all the room (the conversation is not drawn).
pub(super) fn side(ui: &mut egui::Ui, c: &mut ClaudeUi, s: &Shared, session_info: &session::SessionInfo, a: &mut Acts) -> bool {
    let Shared { accent, conv, .. } = *s;
    let cid = |name: &str| egui::Id::new(name).with(conv);
    let (review_id, side_id) = (cid("claude_review"), cid("claude_side_review"));
    let mut review_click = std::mem::take(&mut a.review_click);
    let mut all = std::mem::take(&mut a.all);
    let mut tool_act = std::mem::take(&mut a.tool_act);
    let mut close_turn = std::mem::take(&mut a.close_turn);
    let mut turn_click = std::mem::take(&mut a.turn_click);
    let show_review = std::mem::take(&mut a.show_review);
    let mut side_review = std::mem::take(&mut a.side_review);
    // ------------------------------------------------ the review, on the right (or taking all the room)
    let view = cid("claude_view");
    let review_max = maximize::maxed(ui.ctx(), view) == Some("review");
    let turn_shown = c.turn_diff.is_some();
    // the changes of one question (± on a message) are in the review
    side_review |= turn_shown;
    let mut review_ui = |ui: &mut egui::Ui, c: &mut ClaudeUi| {
        ui.add_space(4.0);
        // the changes of one question (± on a message), instead of the review
        let turn_title = c.turn_diff.as_ref().and_then(|t| match c.items.get(t.item) {
            Some(Item::User(text, ..)) => Some(text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(60).collect::<String>()),
            _ => None,
        });
        if let Some(td) = c.turn_diff.as_mut() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("Changements de la question", "Changes of the question")).size(14.0).strong());
                if let Some(d) = &td.diff {
                    ui.label(RichText::new(tf!("{} fichier(s)", "{} file(s)", d.files.len())).small().color(accent));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    maximize::button(ui, view, "review");
                    if ui.button(t!("× Tous les fichiers modifiés", "× All the changed files")).on_hover_text(t!("Revenir à la revue de tous les changements de Claude", "Back to the review of all of Claude's changes")).clicked() {
                        close_turn = true;
                    }
                });
            });
            ui.label(RichText::new(format!("« {} »", turn_title.unwrap_or_default())).small().weak());
            ui.label(RichText::new(t!("Ce qui a changé dans les fichiers entre ce message et le suivant (tes propres modifications comprises).", "What changed in the files between this message and the next one (your own edits included).")).small().weak());
            ui.separator();
            match &td.diff {
                None => {
                    spinner(ui);
                }
                Some(d) if d.files.is_empty() => {
                    ui.label(RichText::new(t!("Aucun fichier changé pendant cette question.", "No file changed during this question.")).weak());
                }
                Some(d) => {
                    let file_buttons: &[history::DiffButton] = &[(t!("Ouvrir", "Open"), t!("Ouvrir le fichier", "Open the file"), DiffAct::Open)];
                    turn_click = history::stacked_diff_view(ui, &d.lines, &d.files, &mut td.collapsed, "claude_turn", &[], file_buttons, None, None);
                }
            }
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("Changements de Claude", "Claude's changes")).size(14.0).strong());
            let n = c.review.as_ref().map_or(0, |d| d.files.len());
            if n > 0 {
                ui.label(RichText::new(tf!("{n} fichier(s)", "{n} file(s)")).small().color(accent));
            }
            if c.review_loading {
                spinner(ui);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                maximize::button(ui, view, "review");
                if n > 0 {
                    let label = if c.confirm_reject { t!("Confirmer : tout rejeter", "Confirm: reject all") } else { t!("↺ Tout rejeter", "↺ Reject all") };
                    if ui.add(egui::Button::new(RichText::new(label).color(err()))).on_hover_text(t!("Remettre les fichiers comme au début de la revue (les nouveaux fichiers sont supprimés)", "Put the files back as at the start of the review (the new files are deleted)")).clicked() {
                        if c.confirm_reject {
                            all = Some(false);
                        } else {
                            c.confirm_reject = true;
                        }
                    }
                    if ui.add(egui::Button::new(t!("✔ Tout garder", "✔ Keep all"))).on_hover_text(t!("Tout accepter : la revue repart de là", "Accept all: the review starts again from there")).clicked() {
                        all = Some(true);
                    }
                }
            });
        });
        // what the buttons of each block do, once
        if c.review.as_ref().is_some_and(|d| !d.files.is_empty()) {
            ui.label(RichText::new(t!("Garder : accepter le bloc, il reste dans le fichier. Rejeter : le retirer du fichier.", "Keep: accept the block, it stays in the file. Reject: remove it from the file.")).small().weak());
        }
        ui.separator();
        match &c.review {
            _ if c.review_index.is_none() => {
                ui.label(RichText::new(t!("Les fichiers modifiés par Claude apparaîtront ici, bloc par bloc : tu gardes ou tu rejettes chaque partie.", "The files changed by Claude will show here, block by block: you keep or reject each part.")).weak());
            }
            Some(d) if d.files.is_empty() => {
                ui.label(RichText::new(t!("Aucun changement à revoir.", "No change to review.")).weak());
            }
            Some(d) => {
                let hunk_buttons: &[history::DiffButton] = &[
                    (t!("✔ Garder", "✔ Keep"), t!("Accepter ce bloc (il quitte la revue, il reste dans le fichier)", "Accept this block (it leaves the review, it stays in the file)"), DiffAct::Stage),
                    (t!("↺ Rejeter", "↺ Reject"), t!("Retirer ce bloc du fichier", "Remove this block from the file"), DiffAct::Discard),
                ];
                let file_buttons: &[history::DiffButton] = &[
                    (t!("Ouvrir", "Open"), t!("Ouvrir le fichier", "Open the file"), DiffAct::Open),
                    (t!("✔ Garder", "✔ Keep"), t!("Accepter tout le fichier", "Accept the whole file"), DiffAct::Stage),
                    (t!("↺ Rejeter", "↺ Reject"), t!("Remettre le fichier comme avant (supprimé s'il est nouveau)", "Put the file back as before (deleted if it is new)"), DiffAct::Discard),
                ];
                let jump = c.jump.take();
                review_click = history::stacked_diff_view(ui, &d.lines, &d.files, &mut c.collapsed, "claude_review", hunk_buttons, file_buttons, jump, None);
            }
            None => {
                spinner(ui);
            }
        }
    };
    // « Session » chosen while the changes of a question were shown: they are closed
    let mut to_session = false;
    if review_max {
        egui::CentralPanel::default().show_inside(ui, |ui| review_ui(ui, c));
    } else if show_review || turn_shown {
        // each its own width: the review needs room for its diffs, the session little (dragged
        // wider for the review, the session would stay as wide)
        let (panel_id, width, range) = if side_review { (review_id, ui.available_width() * 0.5, 260.0..=1400.0) } else { (review_id.with("session"), 230.0, 200.0..=420.0) };
        egui::SidePanel::right(panel_id).resizable(true).default_width(width).width_range(range).show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.selectable_label(!side_review, t!("Session", "Session")).on_hover_text(t!("Contexte, quotas, projet, modèle, mémoire, serveurs MCP", "Context, quotas, project, model, memory, MCP servers")).clicked() && side_review {
                    side_review = false;
                    to_session = true;
                }
                let n = c.review.as_ref().map_or(0, |d| d.files.len());
                let label = if n > 0 { tf!("Fichiers modifiés ({n})", "Changed files ({n})") } else { t!("Fichiers modifiés", "Changed files").to_string() };
                let text = if n > 0 && !side_review { RichText::new(label).color(accent).strong() } else { RichText::new(label) };
                if ui.selectable_label(side_review, text).on_hover_text(t!("Ce que Claude a changé dans les fichiers, bloc par bloc", "What Claude changed in the files, block by block")).clicked() {
                    side_review = true;
                }
            });
            ui.separator();
            if side_review {
                review_ui(ui, c);
            } else if let Some(a) = session::session_ui(ui, c, session_info, accent) {
                tool_act = Some(a);
            }
        });
    }
    close_turn |= to_session;
    ui.data_mut(|d| d.insert_temp(side_id, side_review));
    a.review_click = review_click;
    a.all = all;
    a.tool_act = tool_act;
    a.close_turn = close_turn;
    a.turn_click = turn_click;
    a.show_review = show_review;
    a.side_review = side_review;
    review_max
}
