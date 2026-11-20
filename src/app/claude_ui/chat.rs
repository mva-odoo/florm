//! The messages of a conversation with Claude.

use super::*;

/// The messages of the conversation, in a list that follows the last one.
pub(super) fn chat(ui: &mut egui::Ui, c: &mut ClaudeUi, s: &Shared, cache: &mut egui_commonmark::CommonMarkCache, hl: &Highlighting, a: &mut Acts) {
    let Shared { accent, fill, conv, .. } = *s;
    let cid = |name: &str| egui::Id::new(name).with(conv);
    let chat_id = cid("claude_chat");
    let mut open_image = std::mem::take(&mut a.open_image);
    let mut answer = std::mem::take(&mut a.answer);
    let mut fold = std::mem::take(&mut a.fold);
    let mut fold_all = std::mem::take(&mut a.fold_all);
    let mut turn_req = std::mem::take(&mut a.turn_req);
    let mut rewind = std::mem::take(&mut a.rewind);
    // "⬇ Dernier message" asked at the previous frame
    let to_end_id = egui::Id::new(("claude_to_end", chat_id));
    let to_end = ui.data_mut(|d| d.remove_temp::<bool>(to_end_id)).unwrap_or(false);
    let chat = egui::ScrollArea::vertical().id_salt(chat_id).stick_to_bottom(true).auto_shrink([false, false]);
    let out = chat.show(ui, |ui| {
        // the whole width: long commands and outputs wrap less
        ui.set_max_width(ui.available_width());
        if c.items.is_empty() {
            ui.add_space(20.0);
            ui.label(RichText::new(t!("Demande quelque chose à Claude : il lit le projet et modifie les fichiers. Tu revois ses changements à droite, bloc par bloc.", "Ask Claude something: it reads the project and changes the files. You review its changes on the right, block by block.")).weak());
            ui.add_space(6.0);
            ui.label(RichText::new(t!("Florm utilise la commande claude (Claude Code) et ta connexion : pas de clé à configurer.", "Florm uses the claude command (Claude Code) and your login: no key to set up.")).weak().small());
        }
        // who speaks is said once, at the start of their turn
        let mut claude_said = true;
        // the questions of the last answer can be answered (not those of before)
        let last_user = c.items.iter().rposition(|i| matches!(i, Item::User(..)));
        // only the messages in sight are drawn (a long conversation would be laid out
        // at every frame): the others take the height they had when last drawn
        let ppp = ui.ctx().pixels_per_point();
        let sight = ui.clip_rect().expand(300.0);
        c.heights.resize(c.items.len(), (0, 0.0, false));
        // a folded question: only its message is shown
        let mut hidden = false;
        // a band on the left from each question to the end of its answer, in the color of
        // the questions: what goes with it, and what folds with it (a click folds it)
        let mut turn: Option<(usize, f32)> = None;
        let band_x = ui.max_rect().left();
        // true: clicked
        let band = |ui: &mut egui::Ui, u: usize, from: f32, to: f32| -> bool {
            if to <= from + 1.0 {
                return false;
            }
            let r = egui::Rect::from_min_max(egui::pos2(band_x, from), egui::pos2(band_x + 4.0, to));
            let resp = ui.interact(r.expand2(egui::vec2(3.0, 0.0)), ui.id().with(("turn_band", u)), Sense::click());
            let color = if resp.hovered() { accent } else { icons::mix(fill, accent, 0.40) };
            ui.painter().rect_filled(r, 2.0, color);
            resp.on_hover_text(t!("Replier / déplier cette question et sa réponse", "Fold / unfold this question and its answer")).clicked()
        };
        let n_items = c.items.len();
        // the steps between two things Claude says (searches, reads, commands, thinking):
        // one folded line, so that what it says stands out, as in Claude Desktop
        let groups = step_groups(&c.items);
        let last_text = c.items.iter().rposition(|i| matches!(i, Item::Text(_)));
        let mut toggle_steps = None;
        for (k, it) in c.items.iter().enumerate() {
            // a thinking with nothing to read (not shown by the model): not a line of
            // its own, « Claude réfléchit… » at the end says it while it lasts
            if matches!(it, Item::Thinking(t) if t.trim().is_empty()) {
                continue;
            }
            // what an answer cost: in the panel of the session (« Dernière réponse »), not
            // a grey line under each answer
            if matches!(it, Item::Usage { .. }) {
                continue;
            }
            if matches!(it, Item::User(..)) {
                hidden = c.folded.contains(&k);
            } else if hidden {
                if matches!(it, Item::Text(_) | Item::Streaming(_)) {
                    claude_said = true;
                }
                continue;
            }
            if let Some(&(gs, ge)) = groups.iter().find(|(a, b)| (*a..=*b).contains(&k)) {
                let open = c.open_steps.contains(&gs);
                if k == gs {
                    ui.add_space(6.0);
                    let running = c.busy && ge + 1 >= n_items;
                    let r = ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        if running {
                            spinner(ui);
                        }
                        let text = steps_summary(&c.items[gs..=ge], open, running);
                        ui.add(egui::Label::new(RichText::new(text).small().weak()).sense(Sense::click()))
                    }).inner;
                    if r.on_hover_text(t!("Les recherches, lectures, commandes et la réflexion de Claude : clic pour les voir ou les replier", "Claude's searches, reads, commands and thinking: click to see or fold them")).clicked() {
                        toggle_steps = Some(gs);
                    }
                }
                if !open {
                    continue;
                }
            }
            // the tools used one after the other: a block of lines, not spread out
            let after_tool = k > 0 && matches!(c.items[k - 1], Item::Tool { .. });
            ui.add_space(if after_tool && matches!(it, Item::Tool { .. }) { 0.0 } else { 6.0 });
            let extra = match it {
                Item::User(..) => u8::from(c.folded.contains(&k)) | u8::from(c.confirm_rewind == Some(k)) << 1,
                Item::Tool { id, .. } => u8::from(c.live.open.contains(id)) << 2,
                _ => 0,
            };
            // working or not only changes the last answer (its questions): the others
            // are not laid out again each time Claude starts or ends
            let last_turn = last_user.is_none_or(|u| k > u);
            let key = item_key(it, claude_said, c.busy && last_turn, last_turn, extra, ui.available_width(), ppp);
            let top = ui.cursor().top();
            if matches!(it, Item::User(..)) {
                if let Some((u, from)) = turn.take() {
                    if band(ui, u, from, top - 6.0) {
                        fold = Some(u);
                    }
                }
                // from under its card (set once it is laid out)
                turn = Some((k, top));
            }
            let (known, h, stable) = c.heights[k];
            // only a height measured twice the same stands for the message: one still
            // changing (an output unfolding, an image loading) would make the
            // conversation jump each time the message goes in and out of sight
            if known == key && stable && (top + h < sight.top() || top > sight.bottom()) {
                ui.allocate_space(egui::vec2(ui.available_width(), h));
                if let (Item::User(..), Some(t)) = (it, turn.as_mut()) {
                    t.1 = ui.cursor().top() - ui.spacing().item_spacing.y;
                }
                match it {
                    Item::User(..) => claude_said = false,
                    Item::Text(_) | Item::Streaming(_) => claude_said = true,
                    _ => {}
                }
                continue;
            }
            // the answer a little to the right: the band of its question runs on its left
            let inset = if matches!(it, Item::User(..)) { 0.0 } else { 14.0 };
            egui::Frame::none().inner_margin(egui::Margin { left: inset, ..Default::default() }).show(ui, |ui| ui.push_id(k, |ui| match it {
                Item::User(t, given, notes, images) => {
                    claude_said = false;
                    ui.add_space(6.0);
                    let folded = c.folded.contains(&k);
                    let cp = c.checkpoints.iter().find(|p| p.item == k);
                    // a bubble on the right, as in Claude Desktop: as wide as its text (85 % at
                    // most), a neutral color, the answer below in plain text
                    let max_w = (ui.available_width() * 0.85).max(240.0);
                    let natural = ui.fonts(|f| f.layout_no_wrap(t.lines().max_by_key(|l| l.len()).unwrap_or("").to_string(), egui::TextStyle::Body.resolve(ui.style()), Color32::WHITE).size().x);
                    let roomy = c.confirm_rewind == Some(k) || !notes.is_empty() || !images.is_empty();
                    // the bar above the text (fold, copy, rewind) must fit too: in a monospace
                    // font it is wider than a short message, and its buttons covered the label
                    let header = {
                        let width = |s: &str, style: egui::TextStyle| ui.fonts(|f| f.layout_no_wrap(s.to_string(), style.resolve(ui.style()), Color32::WHITE).size().x);
                        let fold_label = if folded { t!("▸ Déplier la réponse", "▸ Unfold the answer") } else { t!("▾ Replier la réponse", "▾ Fold the answer") };
                        let pad = ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x;
                        let mut w = width(fold_label, egui::TextStyle::Small) + ui.spacing().item_spacing.x;
                        w += width(&format!("🗐 {}", t!("Copier", "Copy")), egui::TextStyle::Body).max(width(t!("✔ Copié", "✔ Copied"), egui::TextStyle::Body)) + pad;
                        if cp.is_some() {
                            w += width("↶", egui::TextStyle::Body) + pad;
                        }
                        if cp.is_some_and(|p| p.tree.is_some()) {
                            w += width("±", egui::TextStyle::Body) + pad;
                        }
                        w
                    };
                    let bubble_w = if roomy { max_w } else { (natural.max(header) + 28.0).clamp(220.0, max_w) };
                    let text_color = ui.visuals().text_color();
                    let push = (ui.available_width() - bubble_w).max(0.0);
                    egui::Frame::none()
                        .outer_margin(egui::Margin { left: push, ..Default::default() })
                        .fill(icons::mix(fill, text_color, if ui.visuals().dark_mode { 0.10 } else { 0.06 }))
                        .rounding(egui::Rounding { nw: 16.0, ne: 16.0, sw: 16.0, se: 4.0 })
                        .inner_margin(egui::Margin { left: 14.0, right: 12.0, top: 8.0, bottom: 10.0 })
                        .show(ui, |ui| {
                            ui.set_width(bubble_w - 26.0);
                            ui.horizontal(|ui| {
                                // says what it folds: the answer under the message, not the message
                                let label = if folded { t!("▸ Déplier la réponse", "▸ Unfold the answer") } else { t!("▾ Replier la réponse", "▾ Fold the answer") };
                                let arrow = ui.add(egui::Label::new(RichText::new(label).small().color(icons::mix(text_color, accent, 0.5))).sense(Sense::click()));
                                if arrow.clicked() {
                                    fold = Some(k);
                                }
                                arrow.context_menu(|ui| {
                                    if ui.button(t!("Tout replier sauf la dernière question", "Fold all but the last question")).clicked() {
                                        fold_all = Some(true);
                                        ui.close_menu();
                                    }
                                    if ui.button(t!("Tout déplier", "Unfold all")).clicked() {
                                        fold_all = Some(false);
                                        ui.close_menu();
                                    }
                                });
                                arrow.on_hover_text(t!("Replier / déplier la réponse à cette question (clic droit : toutes)", "Fold / unfold the answer to this question (right click: all)"));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    super::md::copy_button(ui, t, t!("Copier", "Copy"));
                                    if cp.is_some() {
                                        let tip = t!("Revenir avant ce message : les fichiers et / ou la conversation", "Go back to before this message: the files and / or the conversation");
                                        if ui.add_enabled(!c.busy, egui::Button::new("↶").small()).on_hover_text(tip).on_disabled_hover_text(t!("Claude travaille", "Claude is working")).clicked() {
                                            rewind = Some((k, None));
                                        }
                                    }
                                    if cp.is_some_and(|p| p.tree.is_some()) && ui.small_button("±").on_hover_text(t!("Les changements de fichiers de cette question seule", "The file changes of this question alone")).clicked() {
                                        turn_req = Some(k);
                                    }
                                });
                            });
                            if !t.is_empty() {
                                ui.add(egui::Label::new(t.as_str()).wrap());
                            }
                            // the texts and images given are below: not named twice
                            let shown = |g: &str| notes.iter().any(|(l, _)| l == g) || images.iter().any(|(l, _)| l == g);
                            let others: Vec<&str> = given.iter().map(String::as_str).filter(|g| !shown(g)).collect();
                            if !others.is_empty() {
                                ui.label(RichText::new(format!("✳ {}", others.join(", "))).small().weak());
                            }
                            if !images.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    for (label, p) in images {
                                        if !p.exists() {
                                            ui.label(RichText::new(tf!("✳ {label} (plus disponible)", "✳ {label} (no longer available)")).small().weak());
                                        } else if image_thumb(ui, p, 120.0).clicked() {
                                            open_image = Some(p.clone());
                                        }
                                    }
                                });
                            }
                            for (n, (label, text)) in notes.iter().enumerate() {
                                egui::CollapsingHeader::new(RichText::new(format!("✳ {label}")).small().color(accent)).id_salt(("note", n)).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| super::md::copy_button(ui, text, t!("Copier", "Copy")));
                                    });
                                    egui::ScrollArea::vertical().id_salt(("note_text", n)).max_height(240.0).show(ui, |ui| {
                                        ui.add(egui::Label::new(RichText::new(text.as_str()).monospace()).wrap());
                                    });
                                });
                            }
                            if c.confirm_rewind == Some(k) {
                                ui.separator();
                                ui.label(RichText::new(t!("Revenir avant ce message ?", "Go back to before this message?")).strong());
                                ui.label(RichText::new(t!("Les fichiers redeviennent comme à l'envoi du message (ce que Claude ou toi avez changé depuis est perdu) ; la conversation reprend sans ce message ni la suite, et le message revient dans le champ.", "The files go back to how they were when the message was sent (what Claude or you changed since is lost); the conversation goes on without this message and what followed, and the message goes back to the field.")).small().weak());
                                ui.horizontal_wrapped(|ui| {
                                    let files = cp.is_some_and(|p| p.tree.is_some());
                                    if files && ui.add(filled_button(t!("↶ Fichiers et conversation", "↶ Files and conversation"), accent)).clicked() {
                                        rewind = Some((k, Some(Rewind::Both)));
                                    }
                                    if ui.button(t!("Conversation seulement", "Conversation only")).clicked() {
                                        rewind = Some((k, Some(Rewind::Conversation)));
                                    }
                                    if files && ui.button(t!("Fichiers seulement", "Files only")).clicked() {
                                        rewind = Some((k, Some(Rewind::Files)));
                                    }
                                    if ui.small_button(t!("Annuler", "Cancel")).clicked() {
                                        rewind = Some((usize::MAX, None));
                                    }
                                });
                            }
                        });
                    if folded {
                        let (steps, files) = super::claude_turns::turn_summary(&c.items, k);
                        let text = tf!("▸ {steps} étape(s) · {files} fichier(s) modifié(s) — clic pour déplier", "▸ {steps} step(s) · {files} file(s) changed — click to unfold");
                        if ui.add(egui::Label::new(RichText::new(text).small().weak()).sense(Sense::click())).clicked() {
                            fold = Some(k);
                        }
                    }
                }
                Item::Text(t) | Item::Streaming(t) => {
                    if !claude_said {
                        claude_said = true;
                        ui.label(RichText::new("✳ Claude").small().strong().color(accent));
                    }
                    {
                        super::md::markdown_checks(ui, cache, hl, t, &mut c.checks, k);
                        // the last answer proposes things to tick: what is ticked is sent back
                        let last = matches!(it, Item::Text(_)) && !c.busy && last_user.is_none_or(|u| k > u) && last_text == Some(k);
                        if last {
                            if let Some(msg) = ticked_reply(t, &c.checks, k) {
                                ui.add_space(4.0);
                                let n = msg.1;
                                let b = ui.add(filled_button(&tf!("✔ Envoyer ma réponse ({n} cochée(s))", "✔ Send my answer ({n} ticked)"), accent));
                                if b.on_hover_text(&msg.0).clicked() {
                                    answer = Some(msg.0);
                                }
                            }
                        }
                    }
                }
                // a subagent: a card of its own, its steps and its report inside
                Item::Tool { name, summary, result, steps, time, agent, .. } if matches!(name.as_str(), "Agent" | "Task") => {
                    agent_card(ui, k, summary, result.as_ref(), steps, *time, agent.as_ref(), c.busy || c.session.is_some(), cache, hl, fill);
                }
                Item::Tool { id: tool_id, name, summary, result, steps, time, .. } => {
                    // how long it took, or has been running
                    let took = match *time {
                        (0, _) => None,
                        (a, 0) if result.is_none() && c.busy => Some(now_ms().saturating_sub(a)),
                        (a, b) if b >= a => Some(b - a),
                        _ => None,
                    };
                    let took_ms = took;
                    let took = took.map(|ms| RichText::new(duration(ms)).small().weak());
                    let error = result.as_ref().is_some_and(|(_, e)| *e);
                    // a command: copied to run it again in a terminal
                    let command = (name == "Bash" && !summary.is_empty()).then_some(summary.as_str());
                    // sent to the background: its output goes on in a file
                    let background = name == "Bash" && result.as_ref().is_some_and(|(t, _)| live::background_file(t).is_some());
                    let output = result.as_ref().map(|(t, _)| t.as_str()).filter(|t| !t.trim().is_empty());
                    // one quiet line, as in the editors that work with Claude: what it did, on
                    // what (cut to the line), how long; a click shows the whole command and
                    // what it printed
                    let weak = ui.visuals().weak_text_color();
                    let open_id = ui.id().with(("tool_open", tool_id.as_str()));
                    let can_open = output.is_some() || command.is_some();
                    let mut open = can_open && ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
                    let mut toggle = false;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let arrow = match (can_open, open) {
                            (false, _) => "·",
                            (true, true) => "▾",
                            (true, false) => "▸",
                        };
                        ui.label(RichText::new(arrow).color(weak));
                        if result.is_none() && c.busy {
                            spinner(ui);
                        }
                        let verb_color = if error { err() } else { ui.visuals().text_color() };
                        let verb = ui.add(egui::Label::new(RichText::new(tool_verb(name)).color(verb_color)).sense(Sense::click()));
                        // the room left for the target: the duration and the button after it, measured
                        // (a fixed guess was too short in a monospace font, and they overlapped)
                        let width = |text: &str, style: egui::TextStyle| ui.fonts(|f| f.layout_no_wrap(text.to_string(), style.resolve(ui.style()), Color32::WHITE).size().x);
                        let mut after = 8.0;
                        if let Some(ms) = took_ms {
                            after += width(&duration(ms), egui::TextStyle::Small) + 6.0;
                        }
                        if name == "Bash" && (result.is_none() || background) {
                            after += width(live_label(c.live.open.contains(tool_id.as_str())), egui::TextStyle::Body) + ui.spacing().button_padding.x * 2.0 + 6.0;
                        }
                        let room = (ui.available_width() - after).max(40.0);
                        let target_color = if error { err() } else { weak };
                        let target = ui
                            .allocate_ui(egui::vec2(room, 18.0), |ui| ui.add(egui::Label::new(RichText::new(shown_summary(name, summary)).monospace().size(12.0).color(target_color)).truncate().sense(Sense::click())))
                            .inner;
                        let tip = if can_open { t!("Clic : voir la commande entière et ce qu'elle a affiché", "Click: see the whole command and what it printed") } else { "" };
                        let target = if tip.is_empty() { target } else { target.on_hover_text(format!("{summary}\n\n{tip}")) };
                        toggle = can_open && (verb.clicked() || target.clicked());
                        if let Some(t) = took.clone() {
                            ui.label(t).on_hover_text(t!("Durée", "Time it took"));
                        }
                        if name == "Bash" && (result.is_none() || background) {
                            live_toggle(ui, &mut c.live, tool_id);
                        }
                    });
                    if toggle {
                        open = !open;
                        ui.data_mut(|d| d.insert_temp(open_id, open));
                    }
                    // an image (or a PDF) Claude opened: shown under its line, as the ones given;
                    // its file gone, the copy Florm kept of what Claude saw
                    // looked for once the tool is done, then remembered (not asked to the disk at each frame)
                    let shown = if name == "Read" && claude::shown_read(summary) {
                        let key = egui::Id::new(("read_image", tool_id.as_str()));
                        match ui.data(|d| d.get_temp::<Option<PathBuf>>(key)) {
                            Some(p) => p,
                            None => {
                                let p = Some(PathBuf::from(summary)).filter(|p| p.exists()).or_else(|| claude::kept_image(tool_id));
                                if result.is_some() {
                                    ui.data_mut(|d| d.insert_temp(key, p.clone()));
                                }
                                p
                            }
                        }
                    } else {
                        None
                    };
                    if let Some(p) = shown {
                        egui::Frame::none().outer_margin(egui::Margin { left: 18.0, top: 2.0, bottom: 4.0, ..Default::default() }).show(ui, |ui| {
                            if image_thumb(ui, &p, 120.0).clicked() {
                                open_image = Some(p.clone());
                            }
                        });
                    }
                    if open {
                        egui::Frame::none()
                            .fill(icons::mix(fill, ui.visuals().text_color(), 0.04))
                            .rounding(6.0)
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .outer_margin(egui::Margin { left: 18.0, ..Default::default() })
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if let Some(cmd) = command {
                                    ui.horizontal_top(|ui| {
                                        super::md::copy_button(ui, cmd, t!("Copier", "Copy"));
                                        ui.add(egui::Label::new(RichText::new(cmd).monospace().size(12.0)).wrap());
                                    });
                                }
                                if let Some(t) = output {
                                    let t: String = t.lines().take(80).collect::<Vec<_>>().join("\n");
                                    ui.add(egui::Label::new(RichText::new(t).monospace().size(11.5).color(if error { err() } else { weak })).wrap());
                                }
                            });
                    }
                    // what the command prints, read while it runs
                    // (a command in the foreground: until its result comes, Claude Code removes the file then)
                    if c.live.open.contains(tool_id) && name == "Bash" && (result.is_none() || background) {
                        let running = result.is_none() && c.busy;
                        live_output(ui, &mut c.live, tool_id, time.0, result.as_ref().map(|(t, _)| t.as_str()), c.session_id.as_deref(), running || background);
                    }
                    // an agent: the tools it used, folded under its call
                    if !steps.is_empty() {
                        let label = tf!("{} étape(s) de l'agent", "{} step(s) of the agent", steps.len());
                        ui.indent(("agent_steps", k), |ui| {
                            egui::CollapsingHeader::new(RichText::new(label).small().weak()).id_salt(("agent_steps", k)).show(ui, |ui| {
                                for (name, what) in steps {
                                    ui.add(egui::Label::new(RichText::new(format!("· {name}  {what}")).monospace().size(11.5).weak()).wrap());
                                }
                            });
                        });
                    }
                }
                Item::Thinking(t) => {
                    // while it is written: said in its title
                    let title = if c.busy && k + 1 == n_items { t!("Réflexion…", "Thinking…") } else { t!("Réflexion", "Thinking") };
                    egui::CollapsingHeader::new(RichText::new(title).small().weak()).id_salt(("thinking", k)).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(t.as_str()).weak().italics()).wrap());
                    });
                }
                Item::Info(t) => {
                    ui.label(RichText::new(t).weak().italics());
                }
                Item::Error(t) => {
                    ui.label(RichText::new(format!("⚠ {t}")).color(err()));
                }
                // not shown (skipped above): in the panel of the session
                Item::Usage { .. } => {}
            }));
            // allocate_space adds the spacing after it again
            let new_h = ui.cursor().top() - top - ui.spacing().item_spacing.y;
            c.heights[k] = (key, new_h, known == key && (new_h - h).abs() < 0.5);
            if let (Item::User(..), Some(t)) = (it, turn.as_mut()) {
                t.1 = top + new_h;
            }
        }
        if let Some((u, from)) = turn.take() {
            let to = ui.cursor().top() - ui.spacing().item_spacing.y;
            if band(ui, u, from, to) {
                fold = Some(u);
            }
        }
        if let Some(g) = toggle_steps {
            if !c.open_steps.remove(&g) {
                c.open_steps.insert(g);
            }
        }
        // working without anything new in sight (thinking, a subagent, a tool running):
        // said at the end of the conversation, where the eye is
        let quiet = !matches!(c.items.last(), Some(Item::Streaming(_) | Item::Tool { result: None, .. }));
        if c.busy && quiet {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                spinner(ui);
                ui.label(RichText::new(working_label(c.activity.as_deref(), c.news)).weak().italics());
            });
        }
        ui.add_space(10.0);
        // the end of the conversation: scrolled to here, not to an offset past the end
        // (the messages out of sight are not laid out, so that offset broke the view)
        let end = ui.allocate_exact_size(egui::vec2(1.0, 1.0), Sense::hover()).0;
        if to_end {
            ui.scroll_to_rect(end, Some(egui::Align::BOTTOM));
        }
    });
    // scrolled up: a button to go back down to the last message
    let below = out.content_size.y - out.state.offset.y - out.inner_rect.height();
    if below > 40.0 {
        let r = out.inner_rect;
        // as wide as its text: a fixed width wrapped it on two lines in a monospace font
        let label = RichText::new(t!("⬇ Dernier message", "⬇ Last message")).strong();
        let w = ui.fonts(|f| f.layout_no_wrap(label.text().to_string(), egui::TextStyle::Button.resolve(ui.style()), Color32::WHITE).size().x) + 28.0;
        let at = egui::Rect::from_min_size(egui::pos2(r.right() - w - 20.0, r.bottom() - 36.0), egui::vec2(w, 26.0));
        let b = egui::Button::new(label).fill(ui.visuals().widgets.inactive.weak_bg_fill).rounding(13.0);
        if ui.put(at, b).on_hover_text(t!("Descendre à la fin de la conversation", "Go down to the end of the conversation")).clicked() {
            ui.data_mut(|d| d.insert_temp(to_end_id, true));
            ui.ctx().request_repaint();
        }
    }
    a.open_image = open_image;
    a.answer = answer;
    a.fold = fold;
    a.fold_all = fold_all;
    a.turn_req = turn_req;
    a.rewind = rewind;
}
