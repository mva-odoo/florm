//! The bottom of a conversation with Claude: where the message is written.

use super::*;

/// What the field needs besides the conversation.
pub(super) struct InputData<'a> {
    pub(super) cache: &'a mut egui_commonmark::CommonMarkCache,
    pub(super) hl: &'a Highlighting,
    pub(super) commands: &'a [(String, String)],
    pub(super) models: &'a [claude::ModelInfo],
    /// The cursor in the field, and the `@` being typed there.
    pub(super) cursor: Option<usize>,
    pub(super) field_id: egui::Id,
    pub(super) mention: Option<(usize, String)>,
}

/// The bottom of the conversation: the field, its buttons, the questions of Claude, the queue.
pub(super) fn input(ui: &mut egui::Ui, c: &mut ClaudeUi, s: &Shared, d: InputData<'_>, a: &mut Acts) {
    let Shared { accent, fill, in_tab, conv, .. } = *s;
    let cid = |name: &str| egui::Id::new(name).with(conv);
    let input_id = cid("claude_input");
    let InputData { cache, hl, commands, models, cursor, field_id, mention } = d;
    let mut send = std::mem::take(&mut a.send);
    let mut stop = std::mem::take(&mut a.stop);
    let mut new_conv = std::mem::take(&mut a.new_conv);
    let mut later = std::mem::take(&mut a.later);
    let mut compact = std::mem::take(&mut a.compact);
    let mut ask_act = std::mem::take(&mut a.ask_act);
    let mut resume = std::mem::take(&mut a.resume);
    let mut ctx_add = std::mem::take(&mut a.ctx_add);
    let mut queue_act = std::mem::take(&mut a.queue_act);
    let mut read_commits = std::mem::take(&mut a.read_commits);
    let mut image_add = std::mem::take(&mut a.image_add);
    let mut open_image = std::mem::take(&mut a.open_image);
    let answer = std::mem::take(&mut a.answer);
    let mut shot = std::mem::take(&mut a.shot);
    let mut mem_act = std::mem::take(&mut a.mem_act);
    let mut mode_cycle = std::mem::take(&mut a.mode_cycle);
    egui::TopBottomPanel::bottom(input_id).show_inside(ui, |ui| {
        ui.add_space(6.0);
        if !c.queue.tasks.is_empty() {
            queue_act = claude_queue::queue_ui(ui, &mut c.queue, accent);
        }
        if !c.attach.is_empty() {
            let (mut remove, mut toggle) = (None, None);
            // the chip whose text is shown below (a pasted text can be read and corrected)
            let open_id = input_id.with("attach_open");
            let open: Option<usize> = ui.data(|d| d.get_temp(open_id)).filter(|&k: &usize| k < c.attach.len());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(t!("Contexte :", "Context:")).small().weak());
                for (k, a) in c.attach.iter().enumerate() {
                    let has_text = a.note.is_some() || a.lines.is_some();
                    let arrow = match (has_text, open == Some(k)) {
                        (false, _) => "",
                        (true, true) => " ▴",
                        (true, false) => " ▾",
                    };
                    if let Some((_, p)) = &a.image {
                        if image_thumb(ui, p, 28.0).clicked() {
                            open_image = Some(p.clone());
                        }
                    }
                    let chip = egui::Button::new(RichText::new(format!("✳ {}{arrow}", a.label(c.dir.as_deref()))).small().color(accent))
                        .fill(icons::mix(fill, accent, if open == Some(k) { 0.25 } else { 0.12 }))
                        .rounding(10.0);
                    let tip = match (&a.note, &a.lines) {
                        (Some((_, t)), _) | (None, Some((_, _, t))) => tf!("{}\n\nClic : voir et modifier tout le texte", "{}\n\nClick: see and edit the whole text", t.lines().take(12).collect::<Vec<_>>().join("\n")),
                        (None, None) if a.image.as_ref().is_some_and(|(_, p)| claude::is_pdf(p)) => t!("PDF envoyé avec le message : Claude lit son texte et ses pages", "PDF sent with the message: Claude reads its text and its pages").into(),
                        (None, None) if a.image.is_some() => t!("Image envoyée avec le message", "Image sent with the message").into(),
                        (None, None) => a.path.display().to_string(),
                    };
                    if ui.add(chip).on_hover_text(tip).clicked() && has_text {
                        toggle = Some(k);
                    }
                    if ui.small_button("×").on_hover_text(t!("Retirer du contexte", "Remove from the context")).clicked() {
                        remove = Some(k);
                    }
                }
            });
            let mut open = match toggle {
                Some(k) if open == Some(k) => None,
                Some(k) => Some(k),
                None => open,
            };
            if let Some(k) = open {
                let a = &mut c.attach[k];
                egui::Frame::none().fill(icons::mix(fill, accent, 0.06)).rounding(6.0).inner_margin(6.0).show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt(input_id.with(("attach_text", k))).max_height(240.0).show(ui, |ui| {
                        match (&mut a.note, &a.lines) {
                            // a pasted text: what is sent, editable
                            (Some((label, t)), _) => {
                                let pasted = label.starts_with(t!("Texte collé", "Pasted text"));
                                if ui.add(egui::TextEdit::multiline(t).code_editor().desired_width(f32::INFINITY)).changed() && pasted {
                                    *label = tf!("Texte collé ({} lignes)", "Pasted text ({} lines)", t.lines().count());
                                }
                            }
                            (None, Some((_, _, t))) => {
                                ui.add(egui::Label::new(RichText::new(t.as_str()).monospace()).wrap());
                            }
                            (None, None) => {}
                        }
                    });
                });
            }
            if let Some(k) = remove {
                c.attach.remove(k);
                open = open.filter(|&o| o != k).map(|o| if o > k { o - 1 } else { o });
            }
            ui.data_mut(|d| match open {
                Some(k) => d.insert_temp(open_id, k),
                None => d.remove::<usize>(open_id),
            });
        }
        let rows = match (c.input.lines().count() > 2, in_tab) {
            (true, true) => 6,
            (true, false) => 4,
            (false, true) => 3,
            (false, false) => 2,
        };
        // cut by the closing of Florm: to go on where it stopped
        if c.interrupted && !c.busy {
            egui::Frame::none().fill(icons::mix(fill, warn(), 0.12)).stroke(egui::Stroke::new(1.0_f32, icons::mix(fill, warn(), 0.5))).rounding(6.0).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(RichText::new(t!("⚠ Florm a été fermé pendant que Claude travaillait : sa tâche a été coupée en cours.", "⚠ Florm was closed while Claude was working: its task was cut midway.")).strong()).wrap());
                ui.label(RichText::new(t!("Ce qu'il avait déjà fait est gardé ; seule l'étape en cours (une commande, une modification) est perdue.", "What it had already done is kept; only the step in progress (a command, a change) is lost.")).small().weak());
                ui.horizontal_wrapped(|ui| {
                    if ui.add(filled_button(t!("▶ Reprendre", "▶ Go on"), accent)).on_hover_text(t!("Claude reprend la session et continue là où il s'était arrêté", "Claude resumes the session and goes on where it stopped")).clicked() {
                        resume = Some(true);
                    }
                    if ui.button(t!("Laisser comme ça", "Leave it")).clicked() {
                        resume = Some(false);
                    }
                });
            });
            ui.add_space(4.0);
        }
        // « Demander avant »: what Claude proposes, waiting for the user
        if !c.asks.is_empty() {
            ask_act = asks::asks_ui(ui, &mut c.asks, c.dir.as_deref(), fill, accent);
        }
        // « # note »: where to remember it
        if let Some(note) = &c.memory {
            egui::Frame::none().fill(icons::mix(fill, accent, 0.08)).rounding(6.0).inner_margin(8.0).show(ui, |ui| {
                ui.label(RichText::new(tf!("Retenir : « {note} »", "Remember: “{note}”")).strong());
                ui.label(RichText::new(t!("Une ligne ajoutée au CLAUDE.md, que Claude lit au début de chaque conversation.", "A line added to the CLAUDE.md, which Claude reads at the start of every conversation.")).small().weak());
                ui.horizontal_wrapped(|ui| {
                    if ui.button(t!("CLAUDE.md du projet", "CLAUDE.md of the project")).on_hover_text(t!("Pour ce projet, partagé avec ceux qui l'ont", "For this project, shared with whoever has it")).clicked() {
                        mem_act = Some(Some(false));
                    }
                    if ui.button(t!("Ma mémoire perso", "My personal memory")).on_hover_text(t!("~/.claude/CLAUDE.md : pour tous tes projets", "~/.claude/CLAUDE.md: for all your projects")).clicked() {
                        mem_act = Some(Some(true));
                    }
                    if ui.small_button(t!("Annuler (le texte revient dans le champ)", "Cancel (the text goes back to the field)")).clicked() {
                        mem_act = Some(None);
                    }
                });
            });
        }
        // the context is over 60 %: compact it, or start again, until closed
        let mut hide_context = false;
        if c.context_high && !c.context_hidden && !c.busy {
            egui::Frame::none().fill(icons::mix(fill, accent, 0.08)).rounding(6.0).inner_margin(8.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(t!("Contexte rempli à plus de 60 %", "Context over 60% full")).strong());
                    if ui.button("/compact").on_hover_text(t!("Claude résume la conversation pour libérer du contexte", "Claude summarizes the conversation to free up context")).clicked() {
                        compact = true;
                    }
                    if ui.button(t!("Nouvelle conversation", "New conversation")).on_hover_text(t!("Repart de zéro ; l'ancienne reste dans l'historique", "Starts again from zero; the old one stays in the history")).clicked() {
                        new_conv = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text(t!("Fermer cette barre", "Close this bar")).clicked() {
                            hide_context = true;
                        }
                    });
                });
            });
        }
        if hide_context {
            c.context_hidden = true;
        }
        // /btw, /status: the answer above the field, until closed
        let mut close_side = false;
        if let Some((asked, answer)) = &c.side {
            egui::Frame::none().fill(icons::mix(fill, accent, 0.08)).rounding(6.0).inner_margin(8.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(asked).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text(t!("Fermer (ce n'est pas dans la conversation)", "Close (it is not in the conversation)")).clicked() {
                            close_side = true;
                        }
                    });
                });
                match answer {
                    None => {
                        ui.horizontal(|ui| {
                            spinner(ui);
                            ui.label(RichText::new(t!("Claude répond…", "Claude is answering…")).weak());
                        });
                    }
                    Some(Ok(t)) => {
                        egui::ScrollArea::vertical().id_salt(input_id.with("side")).max_height(260.0).show(ui, |ui| super::md::markdown(ui, cache, Some(hl), t));
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(format!("⚠ {e}")).color(err()));
                    }
                }
            });
        }
        if close_side {
            c.side = None;
        }
        // a slash command being typed: the matching ones; Enter or a click completes it
        // Shift+Tab: the next mode, as in Claude Code
        if ui.memory(|m| m.has_focus(field_id)) && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::Tab)) {
            mode_cycle = true;
        }
        // a long text pasted goes in as a chip (« Texte collé »), a copied image too
        if ui.memory(|m| m.has_focus(field_id)) {
            let (mut long, mut text_pasted, mut paste_key) = (None, false, false);
            ui.input_mut(|i| {
                i.events.retain(|e| match e {
                    egui::Event::Paste(t) if t.lines().count() > 6 || t.chars().count() > 500 => {
                        long = Some(t.clone());
                        text_pasted = true;
                        false
                    }
                    egui::Event::Paste(_) => {
                        text_pasted = true;
                        true
                    }
                    // egui keeps Ctrl+V pressed for itself (a Paste of the text only, none
                    // for an image): its release says it was pressed
                    egui::Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command => {
                        paste_key = true;
                        true
                    }
                    egui::Event::Key { key: Key::Paste, pressed: false, .. } => {
                        paste_key = true;
                        true
                    }
                    _ => true,
                })
            });
            // a text was just pasted: the clipboard held text, not an image
            let text_id = field_id.with("text_pasted");
            let now = ui.input(|i| i.time);
            if text_pasted {
                ui.data_mut(|d| d.insert_temp(text_id, now));
            }
            let recent_text = ui.data(|d| d.get_temp::<f64>(text_id)).is_some_and(|t| now - t < 2.0);
            if let Some(t) = long {
                let lines = t.lines().count();
                c.attach.push(Attach { path: PathBuf::new(), lines: None, note: Some((tf!("Texte collé ({} lignes)", "Pasted text ({} lines)", lines), t)), image: None });
            }
            if paste_key && !recent_text {
                image_add = clipboard_image().map(|png| (png, t!("Image collée", "Pasted image").to_string()));
            }
        }
        let mut to_end = false;
        let mut cursor_to = None;
        // a list of completions shown: the field keeps Tab (else egui gives the focus to the
        // next button before the list reads the key, and the next letters go nowhere)
        let mut completing = false;
        let dismissed = c.dismissed.as_deref() == Some(c.input.as_str());
        if !dismissed {
            c.dismissed = None;
        }
        // `@` being typed: the matching files of the project; Enter or a click completes it
        if let Some((start, typed)) = mention.as_ref().filter(|_| !c.tools.mention.1.is_empty() && !dismissed) {
            let matching = &c.tools.mention.1;
            completing = true;
            let mut pick = None;
            let choice_id = field_id.with(("mention_choice", typed.clone()));
            let mut choice = ui.data(|d| d.get_temp::<usize>(choice_id)).unwrap_or(0).min(matching.len() - 1);
            if ui.memory(|m| m.has_focus(field_id)) {
                // Tab and ↓ go to the next one, ↑ to the one before, Enter completes
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) | i.consume_key(egui::Modifiers::NONE, Key::Tab)) {
                    choice = (choice + 1) % matching.len();
                }
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)) {
                    choice = (choice + matching.len() - 1) % matching.len();
                }
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                    pick = Some(matching[choice].clone());
                }
            }
            ui.data_mut(|d| d.insert_temp(choice_id, choice));
            egui::Frame::none().fill(icons::mix(fill, accent, 0.05)).rounding(6.0).inner_margin(4.0).show(ui, |ui| {
                for (k, rel) in matching.iter().enumerate() {
                    if ui.selectable_label(k == choice, format!("@{rel}")).on_hover_text(t!("Tab ou ↑ ↓ pour choisir, Entrée : compléter. Le fichier part avec le message", "Tab or ↑ ↓ to choose, Enter: complete. The file goes with the message")).clicked() {
                        pick = Some(rel.clone());
                    }
                }
            });
            if let Some(rel) = pick {
                let chars: Vec<char> = c.input.chars().collect();
                let end = cursor.unwrap_or(chars.len()).min(chars.len());
                let inserted = format!("@{rel} ");
                c.input = chars[..*start].iter().collect::<String>() + &inserted + &chars[end..].iter().collect::<String>();
                cursor_to = Some(start + inserted.chars().count());
                c.focus = true;
            }
        }
        if let Some(typed) = c.input.strip_prefix('/').filter(|t| !t.contains(char::is_whitespace) && !dismissed) {
            let typed = typed.to_lowercase();
            let first = commands.iter().filter(|(n, _)| n.starts_with(&typed));
            let inside = commands.iter().filter(|(n, _)| !n.starts_with(&typed) && n.contains(&typed));
            let matching: Vec<&(String, String)> = first.chain(inside).take(8).collect();
            if !matching.is_empty() && !(matching.len() == 1 && matching[0].0 == typed) {
                completing = true;
                let mut pick = None;
                // the choice: ↑ ↓ move it, Tab or Enter completes it (the keys are taken here,
                // before the field, so the cursor does not move); the choice restarts at each letter
                let choice_id = field_id.with(("command_choice", typed.clone()));
                let mut choice = ui.data(|d| d.get_temp::<usize>(choice_id)).unwrap_or(0).min(matching.len() - 1);
                if ui.memory(|m| m.has_focus(field_id)) {
                    // Tab and ↓ go to the next one, ↑ to the one before, Enter completes
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) | i.consume_key(egui::Modifiers::NONE, Key::Tab)) {
                        choice = (choice + 1) % matching.len();
                    }
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)) {
                        choice = (choice + matching.len() - 1) % matching.len();
                    }
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                        pick = Some(matching[choice].0.clone());
                    }
                }
                ui.data_mut(|d| d.insert_temp(choice_id, choice));
                egui::Frame::none().fill(icons::mix(fill, accent, 0.05)).rounding(6.0).inner_margin(4.0).show(ui, |ui| {
                    for (k, (name, hint)) in matching.iter().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            if ui.selectable_label(k == choice, format!("/{name}")).on_hover_text(t!("Tab ou ↑ ↓ pour choisir, Entrée : compléter", "Tab or ↑ ↓ to choose, Enter: complete")).clicked() {
                                pick = Some(name.clone());
                            }
                            if !hint.is_empty() {
                                ui.label(RichText::new(hint.as_str()).weak().small());
                            }
                        });
                    }
                });
                if let Some(name) = pick {
                    c.input = format!("/{name} ");
                    c.focus = true;
                    to_end = true;
                }
            }
        }
        // the field and its buttons in one rounded box (as in Claude Desktop)
        let field_bg = ui.visuals().extreme_bg_color;
        let edge = ui.visuals().widgets.noninteractive.bg_stroke.color;
        egui::Frame::none().fill(field_bg).stroke(egui::Stroke::new(1.0_f32, edge)).rounding(14.0).inner_margin(egui::Margin { left: 12.0, right: 8.0, top: 10.0, bottom: 6.0 }).show(ui, |ui| {
        let resp = ui.add(
            egui::TextEdit::multiline(&mut c.input)
                .frame(false)
                .id(field_id)
                .hint_text(t!("Demande à Claude… (Ctrl+Entrée pour envoyer, / les commandes, @ un fichier, # retenir, Ctrl+V une image, Maj+Tab le mode)", "Ask Claude… (Ctrl+Enter to send, / commands, @ a file, # remember, Ctrl+V an image, Shift+Tab the mode)"))
                .desired_rows(rows)
                .desired_width(f32::INFINITY)
                .lock_focus(completing)
                .layouter(&mut layout_cutting),
        );
        // a list of completions opened by what was just typed: the field keeps Tab from now
        // on (else egui moves the focus with the next Tab, before the list takes it)
        c.completing = completing && resp.has_focus();
        if resp.has_focus() && opens_completion(&c.input) && c.dismissed.as_deref() != Some(c.input.as_str()) {
            ui.memory_mut(|m| m.set_focus_lock_filter(field_id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false }));
        }
        if to_end {
            cursor_to = Some(c.input.chars().count());
        }
        if let Some(at) = cursor_to {
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), resp.id) {
                let end = egui::text::CCursor::new(at);
                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                st.store(ui.ctx(), resp.id);
            }
        }
        if std::mem::take(&mut c.focus) {
            resp.request_focus();
        }
        if resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter)) {
            send = true;
            later = ui.input(|i| i.modifiers.shift);
        }
        ui.horizontal_wrapped(|ui| {
            let has_text = !c.input.trim().is_empty();
            // a pasted text (or another context) alone can be sent too
            let can_send = has_text || !c.attach.is_empty();
            let ctx_menu = ui.menu_button(t!("+ Contexte", "+ Context"), |ui| {
                ui.set_min_width(260.0);
                for (label, tip, what) in [
                    (t!("Fichier ouvert", "Open file"), t!("Le fichier en entier (Claude le lit)", "The whole file (Claude reads it)"), CtxAdd::File),
                    (t!("Sélection (ou ligne du curseur)", "Selection (or cursor line)"), t!("Les lignes sélectionnées dans l'éditeur, avec leur texte", "The lines selected in the editor, with their text"), CtxAdd::Selection),
                    (t!("Tous les onglets ouverts", "All open tabs"), t!("Chaque fichier ouvert dans l'éditeur", "Each file open in the editor"), CtxAdd::Tabs),
                    (t!("Changements non commités", "Uncommitted changes"), t!("Le diff de ce qui n'est pas commité (git diff HEAD, nouveaux fichiers compris)", "The diff of what is not committed (git diff HEAD, new files included)"), CtxAdd::Diff),
                    (t!("Problèmes du fichier", "Problems of the file"), t!("Les erreurs et avertissements du linter dans le fichier ouvert", "The linter errors and warnings in the open file"), CtxAdd::Problems),
                    (t!("Image ou PDF…", "Image or PDF…"), t!("Des images ou des PDF choisis sur le disque, plusieurs d'un coup avec Ctrl ou Maj (aussi : glisse-les sur la conversation ; une image se colle aussi avec Ctrl+V)", "Images or PDFs chosen on the disk, several at once with Ctrl or Shift (also: drop them on the conversation; an image can also be pasted with Ctrl+V)"), CtxAdd::Image),
                ] {
                    if ui.button(label).on_hover_text(tip).clicked() {
                        ctx_add = Some(what);
                        ui.close_menu();
                    }
                }
                // the last commits of the folder of the conversation, read when the menu opens
                ui.menu_button(t!("Un commit", "A commit"), |ui| {
                    ui.set_min_width(420.0);
                    if c.recent_commits.is_empty() {
                        ui.label(RichText::new(t!("Aucun commit", "No commit")).weak());
                    }
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (hash, short, subject, ago) in &c.recent_commits {
                            let text = format!("{short}  {subject}");
                            if ui.button(text).on_hover_text(tf!("{ago} — son message et son diff partent avec le prochain message\n(aussi : clic droit sur un commit dans la vue Git)", "{ago} — its message and diff go with the next message\n(also: right click on a commit in the Git view)")).clicked() {
                                ctx_add = Some(CtxAdd::Commit(hash.clone()));
                                ui.close_menu();
                            }
                        }
                    });
                });
                ui.separator();
                if ui.button(t!("Capture de Florm", "Picture of Florm")).on_hover_text(t!("La fenêtre de Florm telle qu'elle est, en image (aussi : Ctrl+Alt+I depuis n'importe quelle vue, sans revenir ici)", "Florm's window as it is, as a picture (also: Ctrl+Alt+I from any view, without coming back here)")).clicked() {
                    shot = Some(Duration::ZERO);
                    ui.close_menu();
                }
                // a capture of the screen made with the system's tool (Impr on Omarchy copies it):
                // any zone, any window, without a timer
                let clip = ui
                    .add_enabled(c.clip_image, egui::Button::new(t!("Image du presse-papiers", "Image from the clipboard")))
                    .on_hover_text(t!("L'image copiée (une capture faite avec Impr, une image copiée ailleurs) part avec le prochain message. Aussi : Ctrl+V dans le champ", "The copied image (a capture made with Print, an image copied elsewhere) goes with the next message. Also: Ctrl+V in the field"))
                    .on_disabled_hover_text(t!("Pas d'image dans le presse-papiers : fais une capture (Impr, puis choisis la zone) et rouvre ce menu, ou colle-la avec Ctrl+V dans le champ", "No image in the clipboard: take a capture (Print, then choose the zone) and open this menu again, or paste it with Ctrl+V in the field"));
                if clip.clicked() {
                    image_add = clipboard_image().map(|png| (png, t!("Image collée", "Pasted image").to_string()));
                    ui.close_menu();
                }
                ui.separator();
                ui.label(RichText::new(t!("Aussi : clic droit dans l'éditeur, ou ✳ dans la marge pour des lignes", "Also: right click in the editor, or ✳ in the margin for lines")).small().weak());
            });
            if ctx_menu.response.clicked() {
                read_commits = true;
                c.clip_image = clipboard_has_image();
            }
            ctx_menu.response.on_hover_text(t!("Donner à Claude un fichier, la sélection, le diff, un commit, les problèmes, une image…", "Give Claude a file, the selection, the diff, a commit, the problems, an image…"));
            // what goes with each message: in a menu, the options stay one click away
            let on = [c.with_context, !c.no_defs].iter().filter(|b| **b).count();
            ui.menu_button(tf!("Options ({on}/2) ▾", "Options ({on}/2) ▾"), |ui| {
                ui.set_min_width(300.0);
                ui.checkbox(&mut c.with_context, t!("Fichier ouvert et sélection", "Open file and selection")).on_hover_text(t!("Dit à Claude quel fichier tu édites, la ligne du curseur et le texte sélectionné", "Tells Claude which file you are editing, the cursor line and the selected text"));
                let mut defs = !c.no_defs;
                if ui.checkbox(&mut defs, t!("Définitions citées", "Mentioned definitions")).on_hover_text(t!("Les classes et fonctions du projet nommées dans le message (SaleOrder, _compute_total, `foo`…) partent avec lui, leurs premières lignes comprises : Claude n'a pas à les chercher (moins de tokens)", "The classes and functions of the project named in the message (SaleOrder, _compute_total, `foo`…) go with it, their first lines included: Claude does not have to look for them (fewer tokens)")).changed() {
                    c.no_defs = !defs;
                }
            })
            .response
            .on_hover_text(t!("Ce qui part avec chaque message : le fichier ouvert, les questions d'abord, les définitions citées", "What goes with each message: the open file, questions first, the mentioned definitions"));
            // on the right, as in Claude Desktop: the model and the mode, then send (a
            // round button) or stop; on a line of their own when the box is narrow.
            // The room asked does not depend on Claude working (Stop, After and Send are
            // wider than ↑), nor on what it is doing: the buttons do not jump from a
            // line to the other while it works
            // the room left on this row (in a wrapping row `available_width` is the whole row)
            if ui.max_rect().right() - ui.cursor().min.x < 520.0 {
                ui.end_row();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let send_btn = |ui: &mut egui::Ui, enabled: bool, glyph: &str, fill: Color32| {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), Sense::click());
                    let color = if enabled { fill } else { fill.gamma_multiply(0.35) };
                    let color = if enabled && resp.hovered() { icons::mix(color, Color32::BLACK, 0.12) } else { color };
                    ui.painter().circle_filled(rect.center(), 15.0, color);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, egui::FontId::proportional(15.0), Color32::WHITE);
                    resp.clicked() && enabled
                };
                // the round button keeps its place and its size (↑, ■ while Claude works): the
                // model, effort and mode do not move when it starts or stops
                if c.busy {
                    if send_btn(ui, true, "■", ui.visuals().text_color()) {
                        stop = true;
                    }
                } else if send_btn(ui, can_send, "↑", accent) {
                    send = true;
                }
                ui.add_space(4.0);
                // the alias chosen, with the version Claude Code really runs once it is known
                let alias = !c.model.contains('-');
                let shown = if c.model_id.is_empty() || !alias || !claude::model_is(&c.model_id, &c.model) {
                    model_label(models, &c.model)
                } else if c.model.is_empty() {
                    tf!("Défaut ({})", "Default ({})", claude::model_name(&c.model_id))
                } else {
                    claude::model_name(&c.model_id)
                };
                let tip = if c.model_id.is_empty() {
                    t!("Les modèles proposés par Claude Code : les premiers suivent toujours la dernière version, « Autres versions » en fixe une", "The models Claude Code offers: the first ones always follow the latest version, « Other versions » pins one").to_string()
                } else {
                    tf!("Modèle utilisé : {}", "Model in use: {}", c.model_id)
                };
                let choices = model_choices(models);
                let describe = |v: &str| models.iter().find(|m| m.value == v).map(|m| m.description.clone()).unwrap_or_default();
                // light pills, without the arrows of a list (right to left: model, effort, mode)
                let pill_fill = icons::mix(ui.visuals().extreme_bg_color, ui.visuals().text_color(), 0.07);
                // their text is drawn by Florm, at a place rounded from the pill's edge: egui
                // centers it, and on a half pixel a tiny change between two frames put it a
                // pixel up or down (the pills shook while Claude worked)
                let (pill_ctx, pill_font, pill_text) = (ui.ctx().clone(), egui::TextStyle::Small.resolve(ui.style()), ui.visuals().widgets.inactive.text_color());
                let pad = ui.spacing().button_padding.x;
                let pill = |text: String| {
                    let galley = pill_ctx.fonts(|f| f.layout_no_wrap(text, pill_font.clone(), pill_text));
                    let button = egui::Button::new("").rounding(10.0).fill(pill_fill).stroke(egui::Stroke::NONE).min_size(egui::vec2(galley.size().x + 2.0 * pad, 22.0));
                    (button, galley)
                };
                let pill_label = |ui: &egui::Ui, rect: egui::Rect, galley: std::sync::Arc<egui::Galley>| {
                    let x = (rect.left() + 0.25).round() + ((rect.width() - galley.size().x) / 2.0).round();
                    let y = (rect.top() + 0.25).round() + ((rect.height() - galley.size().y) / 2.0).round();
                    ui.painter().galley(egui::pos2(x, y), galley, pill_text);
                };
                let (button, galley) = pill(shown);
                let r = egui::menu::menu_custom_button(ui, button, |ui| {
                    ui.set_min_width(220.0);
                    for (value, label) in choices.iter().filter(|(v, _)| !v.contains('-')) {
                        let mut r = ui.selectable_value(&mut c.model, value.clone(), label);
                        let d = describe(value);
                        if !d.is_empty() {
                            r = r.on_hover_text(d);
                        }
                        if r.clicked() {
                            ui.close_menu();
                        }
                    }
                    let pinned: Vec<_> = choices.iter().filter(|(v, _)| v.contains('-')).collect();
                    if !pinned.is_empty() {
                        ui.separator();
                        let open = pinned.iter().any(|(v, _)| *v == c.model);
                        egui::CollapsingHeader::new(t!("Autres versions", "Other versions")).id_salt(("claude_model_pinned", conv)).default_open(open).show(ui, |ui| {
                            for (value, label) in pinned {
                                if ui.selectable_value(&mut c.model, value.clone(), label).on_hover_text(format!("{value}\n{}", describe(value))).clicked() {
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                });
                pill_label(ui, r.response.rect, galley);
                r.response.on_hover_text(tip);
                // the effort levels of this model (none: no setting); one it does not take goes back to auto
                let efforts = model_efforts(models, &c.model);
                if !c.effort.is_empty() && !efforts.contains(&c.effort) {
                    c.effort.clear();
                }
                if !efforts.is_empty() {
                    let ordered: Vec<String> = claude::EFFORTS.iter().map(|(v, _)| v.to_string()).filter(|v| efforts.contains(v)).chain(efforts.iter().filter(|e| !claude::EFFORTS.iter().any(|(v, _)| v == e)).cloned()).collect();
                    let (button, galley) = pill(tf!("Effort {}", "Effort {}", claude::effort_label(&c.effort)));
                    let r = egui::menu::menu_custom_button(ui, button, |ui| {
                        if ui.selectable_value(&mut c.effort, String::new(), claude::effort_label("")).on_hover_text(t!("Claude Code choisit (son réglage habituel)", "Claude Code chooses (its usual setting)")).clicked() {
                            ui.close_menu();
                        }
                        for e in ordered {
                            let label = claude::effort_label(&e);
                            if ui.selectable_value(&mut c.effort, e, label).clicked() {
                                ui.close_menu();
                            }
                        }
                    });
                    pill_label(ui, r.response.rect, galley);
                    r.response.on_hover_text(t!("Combien Claude réfléchit avant d'agir : plus d'effort, des réponses plus soignées mais plus lentes et plus de quota", "How much Claude thinks before acting: more effort, more careful answers but slower and more quota"));
                }
                let (button, galley) = pill(claude::label(claude::MODES[c.mode].1).to_string());
                let r = egui::menu::menu_custom_button(ui, button, |ui| {
                    for (k, (_, label, tip)) in claude::MODES.iter().enumerate() {
                        if ui.selectable_value(&mut c.mode, k, claude::label(label)).on_hover_text(claude::label(tip)).clicked() {
                            ui.close_menu();
                        }
                    }
                });
                pill_label(ui, r.response.rect, galley);
                r.response.on_hover_text(t!("Maj+Tab dans le champ : mode suivant", "Shift+Tab in the field: next mode"));
                // what Claude is doing, in the room left (cut, it never pushes the buttons), and
                // where a message written meanwhile goes: at once, or after
                if c.busy {
                    ui.add_space(8.0);
                    let tip = t!("Le message attend dans la file et part quand Claude a fini ce qu'il fait (Ctrl+Maj+Entrée)", "The message waits in the queue and goes when Claude is done with what it does (Ctrl+Shift+Enter)");
                    if ui.add_enabled(has_text, egui::Button::new(t!("⏭ Après", "⏭ After"))).on_hover_text(tip).clicked() {
                        send = true;
                        later = true;
                    }
                    let tip = t!("Envoie le message tout de suite : Claude le lit entre deux étapes et en tient compte dans le travail en cours (Ctrl+Entrée)", "Sends the message at once: Claude reads it between two steps and takes it into account in the work going on (Ctrl+Enter)");
                    if ui.add_enabled(can_send, egui::Button::new(t!("Envoyer", "Send"))).on_hover_text(tip).clicked() {
                        send = true;
                    }
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(RichText::new(working_label(c.activity.as_deref(), c.news)).weak().small()).truncate());
                    spinner(ui);
                }
            });
        });
        });
        ui.add_space(6.0);
    });
    a.send = send;
    a.stop = stop;
    a.new_conv = new_conv;
    a.later = later;
    a.compact = compact;
    a.ask_act = ask_act;
    a.resume = resume;
    a.ctx_add = ctx_add;
    a.queue_act = queue_act;
    a.read_commits = read_commits;
    a.image_add = image_add;
    a.open_image = open_image;
    a.answer = answer;
    a.shot = shot;
    a.mem_act = mem_act;
    a.mode_cycle = mode_cycle;
}
