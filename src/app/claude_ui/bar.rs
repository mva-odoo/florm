//! The bar of settings above a conversation with Claude.

use super::*;

/// What the bar shows besides the conversation.
pub(super) struct BarData<'a> {
    pub(super) dirs: &'a [(PathBuf, String)],
    pub(super) dir_name: &'a str,
    pub(super) history: &'a [(u64, String, String)],
    pub(super) accounts: &'a [claude_account::Account],
    pub(super) account_name: &'a str,
    pub(super) limits: &'a [(String, f32, i64)],
    pub(super) allow: &'a [String],
    pub(super) deny: &'a [String],
    pub(super) perms: &'a mut Vec<String>,
    pub(super) budget: &'a mut f64,
}

/// The bar above the conversation: folder, account, model, mode, permissions, conversations kept.
pub(super) fn bar(ui: &mut egui::Ui, c: &mut ClaudeUi, s: &Shared, d: BarData<'_>, a: &mut Acts) {
    let Shared { accent, fill, in_tab, conv, .. } = *s;
    let cid = |name: &str| egui::Id::new(name).with(conv);
    let bar_id = cid("claude_bar");
    let BarData { dirs, dir_name, history, accounts, account_name, limits, allow, deny, perms, budget } = d;
    let mut new_conv = std::mem::take(&mut a.new_conv);
    let mut chosen_dir = std::mem::take(&mut a.chosen_dir);
    let mut move_to = std::mem::take(&mut a.move_to);
    let mut pick_dir = std::mem::take(&mut a.pick_dir);
    let mut tool_act = std::mem::take(&mut a.tool_act);
    let mut close_turn = std::mem::take(&mut a.close_turn);
    let mut open_saved = std::mem::take(&mut a.open_saved);
    let mut forget_saved = std::mem::take(&mut a.forget_saved);
    let mut chosen_account = std::mem::take(&mut a.chosen_account);
    let mut perms_changed = std::mem::take(&mut a.perms_changed);
    let mut show_review = std::mem::take(&mut a.show_review);
    let mut side_review = std::mem::take(&mut a.side_review);
    // ------------------------------------------------ settings of the conversation
    egui::TopBottomPanel::top(bar_id)
        .frame(egui::Frame::none().fill(fill).inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 8.0, bottom: 6.0 }))
        .show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("✳ Claude").strong().size(15.0).color(accent));
                let running = c.session.is_some();
                ui.add_enabled_ui(!running, |ui| {
                    let mut labels: Vec<String> = dirs.iter().map(|(_, l)| l.clone()).collect();
                    labels.push(t!("📁 Un autre dossier…", "📁 Another folder…").to_string());
                    let current = dirs.iter().position(|(d, _)| c.dir.as_ref() == Some(d));
                    let name = match &c.worktree {
                        Some(w) => format!("🌿 {}", w.branch),
                        None => dir_name.to_string(),
                    };
                    // in the color of its project, as in the list of the conversations
                    let color = hub::project_color(dir_name, ui.visuals().dark_mode);
                    let text = RichText::new(format!("● {name}")).strong().size(14.0).color(color);
                    match crate::pick::combo(ui, ("claude_dir", conv), text, &labels, current) {
                        Some(k) if k < dirs.len() => chosen_dir = Some(dirs[k].0.clone()),
                        Some(_) => pick_dir = true,
                        None => {}
                    }
                })
                .response
                .on_disabled_hover_text(t!("Pour changer : « Nouvelle conversation »", "To change: “New conversation”"))
                .on_hover_text(t!("Le dossier où Claude travaille : un dépôt du projet, un projet récent ou n'importe quel dossier", "The folder Claude works in: a repository of the project, a recent project or any folder"));
                if ui.button(t!("+ Nouvelle", "+ New")).on_hover_text(t!("Dans un nouvel onglet : celle-ci continue dans le sien (l'onglet devient vert quand Claude a fini)", "In a new tab: this one goes on in its own (the tab turns green when Claude is done)")).clicked() {
                    new_conv = true;
                }
                // the rest of the settings: buttons of their own on a wide screen, else in one menu
                // (the bar stays on one line, even narrow)
                // about 560 points for the buttons, 300 for the gauges and the panel buttons on the right
                let wide = ui.available_width() > 1000.0;
                let mut items = |ui: &mut egui::Ui| {
                // changed during the conversation: Claude goes on with it (at once while it works,
                // else from the next message, the conversation kept)
                // the account it runs with (from the next message, the conversation kept)
                if accounts.len() > 1 {
                    ui.menu_button(format!("👤 {account_name}"), |ui| {
                            for a in accounts {
                                if ui.selectable_label(a.name == account_name, &a.name).clicked() && a.name != account_name {
                                    chosen_account = Some(a.name.clone());
                                    ui.close_menu();
                                }
                            }
                        })
                        .response
                        .on_hover_text(t!("Le compte Claude de cette conversation (son abonnement et ses quotas) ; changé, il s'applique au prochain message, la conversation continue", "The Claude account of this conversation (its subscription and quotas); changed, it applies from the next message, the conversation goes on"));
                }
                let n_perms = perms.iter().filter(|p| claude::PERMS.iter().any(|(k, ..)| k == p)).count();
                ui.menu_button(format!("🔒 Permissions ({n_perms})"), |ui| {
                    ui.set_min_width(330.0);
                    if running {
                        ui.label(RichText::new(t!("S'applique au prochain message : Claude redémarre en gardant la conversation", "Applies from the next message: Claude starts again, the conversation kept")).small().color(warn()));
                    }
                    if claude::MODES[c.mode].0 == "ask" {
                        ui.label(RichText::new(t!("Mode « Demander avant » : modifier les fichiers et lancer des commandes te sont toujours demandés ; les autres cases comptent", "“Ask first” mode: editing files and running commands are always asked; the other boxes count")).small().weak());
                    } else if c.mode != 0 {
                        ui.label(RichText::new(tf!("Mode « {} » : seules les deux dernières cases comptent", "“{}” mode: only the last two boxes count", claude::label(claude::MODES[c.mode].1))).small().weak());
                    }
                    ui.add_enabled(false, egui::Checkbox::new(&mut true, t!("Lire les fichiers du projet", "Read the project files"))).on_disabled_hover_text(t!("Toujours : Claude doit lire pour aider", "Always: Claude has to read to help"));
                    for (key, label, tip) in claude::PERMS {
                        let mut on = perms.iter().any(|p| p == key);
                        if key == "sandbox" {
                            ui.separator();
                        }
                        if ui.checkbox(&mut on, claude::label(label)).on_hover_text(claude::label(tip)).changed() {
                            perms.retain(|p| p != key);
                            if on {
                                perms.push(key.to_string());
                            }
                            perms_changed = true;
                        }
                    }
                    if perms.iter().any(|p| p == "sandbox") && crate::lsp::in_path("bwrap").is_none() {
                        ui.label(RichText::new(t!("⚠ Le bac à sable demande bubblewrap (paquet bubblewrap) et socat", "⚠ The sandbox needs bubblewrap (bubblewrap package) and socat")).small().color(warn()));
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(t!("Budget max par conversation", "Max budget per conversation"));
                        let r = ui.add(egui::DragValue::new(budget).speed(0.1).range(0.0..=100.0).prefix("$ ").max_decimals(2));
                        if r.changed() {
                            perms_changed = true;
                        }
                        r.on_hover_text(t!("La conversation s'arrête au-delà (0 : sans limite)", "The conversation stops beyond it (0: no limit)"));
                    });
                    ui.separator();
                    if let Some(a) = super::claude_tools::rules_ui(ui, &mut c.tools, allow, deny) {
                        tool_act = Some(a);
                    }
                })
                .response
                .on_hover_text(t!("Ce que Claude a le droit de faire", "What Claude is allowed to do"));
                let in_git = c.dir.as_ref().is_some_and(|d| d.join(".git").exists());
                if let Some(a) = super::claude_tools::worktree_menu(ui, &mut c.tools, in_git) {
                    tool_act = Some(a);
                }
                ui.menu_button(tf!("Historique ({})", "History ({})", history.len()), |ui| {
                    ui.set_min_width(420.0);
                    if history.is_empty() {
                        ui.label(RichText::new(t!("Les conversations sont gardées ici, même après avoir fermé Florm (50 au plus).", "Conversations are kept here, even after closing Florm (50 at most).")).weak());
                    }
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        for (key, title, info) in history {
                            ui.horizontal(|ui| {
                                if ui.button(title).on_hover_text(t!("La rouvrir : la suite continue avec son contexte", "Reopen it: what follows goes on with its context")).clicked() {
                                    open_saved = Some(*key);
                                    ui.close_menu();
                                }
                                ui.label(RichText::new(info).small().weak());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("×").on_hover_text(t!("L'oublier", "Forget it")).clicked() {
                                        forget_saved = Some(*key);
                                    }
                                });
                            });
                        }
                    });
                })
                .response
                .on_hover_text(t!("Les conversations d'avant, gardées entre les sessions", "The earlier conversations, kept between sessions"));
                let agents_on = perms.iter().any(|p| p == "agents");
                if let Some(a) = super::claude_tools::tools_menu(ui, c, allow, agents_on, dirs) {
                    tool_act = Some(a);
                }
                if c.cost > 0.0 {
                    ui.label(RichText::new(format!("{:.2} $", c.cost)).weak().small()).on_hover_text(t!("Coût estimé de la conversation", "Estimated cost of the conversation"));
                }
                };
                if wide {
                    ui.add_space(6.0);
                    items(ui);
                } else {
                    ui.menu_button("⋯", |ui| {
                        ui.set_min_width(240.0);
                        items(ui);
                    })
                    .response
                    .on_hover_text(t!("Compte, permissions, worktree, historique, outils (MCP, mémoire…)", "Account, permissions, worktree, history, tools (MCP, memory…)"));
                }
                // the gauges, « Revue » and the view buttons need about 300 px: else on a line of their own
                if ui.max_rect().right() - ui.cursor().min.x < 300.0 {
                    ui.end_row();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(crate::view_bar::BarAct::Move(b)) = crate::view_bar::controls(ui, in_tab, None, None) {
                        move_to = Some(b);
                    }
                    // the review of the changes on the right, or the whole width for the conversation
                    let n = c.review.as_ref().map_or(0, |d| d.files.len());
                    // shown also as the changes of one question, or maximized: hidden in one click,
                    // the conversation gets its whole width back
                    let view = cid("claude_view");
                    let shown = show_review || c.turn_diff.is_some() || maximize::maxed(ui.ctx(), view).is_some();
                    let tip = if shown {
                        t!("Masquer le panneau de droite : la conversation prend toute la largeur", "Hide the panel on the right: the conversation takes the whole width").to_string()
                    } else if n > 0 {
                        tf!("Afficher le panneau de droite : {n} fichier(s) modifié(s) par Claude à garder ou rejeter, et la session", "Show the panel on the right: {n} file(s) changed by Claude to keep or reject, and the session")
                    } else {
                        t!("Afficher le panneau de droite : la session (contexte, quotas, outils) et les fichiers modifiés par Claude", "Show the panel on the right: the session (context, quotas, tools) and the files changed by Claude").to_string()
                    };
                    // a panel on the right, filled when shown: like the other layout buttons; a dot
                    // in the accent when Claude changed files and the panel is hidden
                    let b = crate::icons::layout_button(ui, crate::icons::Layout::SidePanel(shown), &tip);
                    if n > 0 && !shown {
                        ui.painter().circle_filled(b.rect.right_top() + egui::vec2(-3.0, 3.0), 3.5, accent);
                    }
                    if b.clicked() && shown {
                        show_review = false;
                        close_turn = true;
                        maximize::restore(ui.ctx(), view);
                    } else if b.clicked() {
                        show_review = true;
                        // files to look at: the panel opens on them
                        side_review |= n > 0;
                    }
                    ui.add_space(8.0);
                    // the panel shows the session: the gauges are there, not twice
                    if shown && !side_review {
                        return;
                    }
                    // the plan's quota over 5 hours always; over 7 days and the context only
                    // past 70 % (right to left: they come after the 5 hours)
                    let mut gauges: Vec<(String, f32, String)> = Vec::new();
                    for window in ["five_hour", "seven_day"] {
                        let (label, what) = match window {
                            "five_hour" => ("5 h", t!("sur 5 heures", "over 5 hours")),
                            _ => ("7 j", t!("sur 7 jours", "over 7 days")),
                        };
                        match limits.iter().find(|l| l.0 == window) {
                            // its window is over since: back to zero
                            Some((_, _, reset)) if *reset > 0 && *reset <= now_secs() => {
                                gauges.push((label.into(), 0.0, tf!("Quota de ton abonnement {what} : remis à zéro depuis la dernière réponse de Claude", "Quota of your subscription {what}: reset since Claude's last answer")));
                            }
                            Some((_, used, reset)) => {
                                gauges.push((label.into(), *used, tf!("Quota de ton abonnement {what} : {:.0} % utilisé, remis à zéro {}", "Quota of your subscription {what}: {:.0} % used, reset {}", used * 100.0, until(*reset))));
                            }
                            None => gauges.push((label.into(), -1.0, tf!("Quota de ton abonnement {what} : connu à la première réponse de Claude", "Quota of your subscription {what}: known at Claude's first answer"))),
                        }
                    }
                    let (tokens, window) = c.context;
                    if tokens > 0 && window > 0 {
                        let used = (tokens as f32 / window as f32).min(1.0);
                        gauges.push((
                            t!("Contexte", "Context").into(),
                            used,
                            tf!("Contexte de la conversation : {} / {} tokens.\nPlein, Claude résume le début de la conversation ; « Nouvelle conversation » repart de zéro.", "Context of the conversation: {} / {} tokens.\nWhen full, Claude summarizes the start of the conversation; “New conversation” starts from scratch.", thousands(tokens), thousands(window)),
                        ));
                    }
                    // hovering the 5 h one shows them all, the hidden ones too
                    let all = gauges.clone();
                    let all_tip = |ui: &mut egui::Ui| gauges_tip(ui, &all, accent);
                    for (k, (label, used, tip)) in gauges.into_iter().enumerate().rev() {
                        match k {
                            0 if used < 0.0 => {
                                ui.label(RichText::new("5 h – %").small().weak()).on_hover_ui(all_tip);
                                ui.add_space(6.0);
                            }
                            0 => gauge(ui, &label, used, accent, all_tip),
                            _ if used > 0.7 => gauge(ui, &label, used, accent, |ui: &mut egui::Ui| {
                                ui.label(tip);
                            }),
                            _ => {}
                        }
                    }
                });
            });
        });

    a.new_conv = new_conv;
    a.chosen_dir = chosen_dir;
    a.move_to = move_to;
    a.pick_dir = pick_dir;
    a.tool_act = tool_act;
    a.close_turn = close_turn;
    a.open_saved = open_saved;
    a.forget_saved = forget_saved;
    a.chosen_account = chosen_account;
    a.perms_changed = perms_changed;
    a.show_review = show_review;
    a.side_review = side_review;
}
