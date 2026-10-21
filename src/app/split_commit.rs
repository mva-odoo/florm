//! Assistant that splits a commit in several.

use super::*;

impl App {
    pub(super) fn start_split(&mut self, hash: String) {
        let Some(repo) = self.history.repo.clone() else { return };
        if self.docs.iter().any(|d| d.is_dirty()) {
            self.history.notice = Some(("Enregistre d'abord les fichiers modifiés avant de découper un commit.".into(), err()));
            return;
        }
        let editor = match split::self_editor() {
            Ok(e) => e,
            Err(e) => {
                self.history.notice = Some((e, err()));
                return;
            }
        };
        self.set_status("Préparation du découpage…");
        self.history.loading = true;
        self.jobs.spawn(move || {
            let r = split::start(&repo, &hash, &editor).and_then(|info| {
                let files = split::changes(&repo, &info.added)?;
                Ok((info, files))
            });
            JobResult::SplitStarted(repo, r)
        });
    }

    /// Reopens the assistant on a rebase left stopped (e.g. Florm was closed during a split).
    pub(super) fn resume_split(&mut self) {
        let Some(repo) = self.history.repo.clone() else { return };
        self.jobs.spawn(move || {
            let r = split::stopped_commit(&repo)
                .ok_or_else(|| "Le rebase en cours n'est pas arrêté sur un commit (conflit ?) : utilise le terminal.".to_string())
                .and_then(|hash| split::info(&repo, &hash))
                .and_then(|info| {
                    let files = split::changes(&repo, &info.added)?;
                    Ok((info, files))
                });
            JobResult::SplitStarted(repo, r)
        });
    }

    fn split_commit_part(&mut self) {
        let Some(s) = self.split.as_mut() else { return };
        let (repo, files, sel, msg, added) =
            (s.repo.clone(), s.files.clone(), s.sel.clone(), s.message.trim().to_string(), s.info.added.clone());
        s.busy = true;
        s.error = None;
        self.jobs.spawn(move || match split::commit_part(&repo, &files, &sel, &msg) {
            Ok(c) => JobResult::SplitChanges { result: split::changes(&repo, &added), committed: Some(c) },
            Err(e) => JobResult::SplitChanges { result: Err(e), committed: None },
        });
    }

    fn split_end(&mut self, aborted: bool) {
        let Some(s) = self.split.as_mut() else { return };
        let repo = s.repo.clone();
        s.busy = true;
        s.error = None;
        self.jobs.spawn(move || JobResult::SplitEnded {
            aborted,
            result: if aborted { split::abort(&repo) } else { split::finish(&repo) },
        });
    }

    pub(super) fn after_split_change(&mut self) {
        self.git_refresh();
        if let Some(t) = self.tree.as_mut() {
            t.refresh();
        }
        // files rewritten by git: reload the unmodified ones right away
        self.last_disk_check = Instant::now() - Duration::from_secs(10);
    }

    pub(super) fn split_ui(&mut self, ui: &mut egui::Ui) {
        let accent = self.hl.colors.accent;
        let Some(s) = self.split.as_mut() else { return };
        let part = s.parts.len() + 1;
        // hunks with at least one chosen line
        let n_sel: usize = s
            .files
            .iter()
            .zip(&s.sel)
            .map(|(f, fs)| f.hunks.iter().zip(fs).filter(|(h, hs)| split::hunk_flags(h, hs).iter().any(|&b| b)).count())
            .sum();
        let n_hunks: usize = s.sel.iter().map(Vec::len).sum();
        let (mut commit, mut finish, mut abort) = (false, false, false);
        let bg = ui.visuals().panel_fill;

        egui::TopBottomPanel::top("split_top").show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("✂ Découper le commit").size(17.0).strong());
                ui.label(RichText::new(&s.info.short).monospace().color(accent).size(15.0));
                ui.label(RichText::new(&s.info.subject).size(15.0));
                if s.busy {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if s.confirm_abort {
                        if ui.button("Non, continuer").clicked() {
                            s.confirm_abort = false;
                        }
                        let b = filled_button("Oui, tout annuler", err());
                        if ui.add_enabled(!s.busy, b).clicked() {
                            abort = true;
                        }
                        ui.label(RichText::new("Remettre le commit d'origine et supprimer les parties ?").color(err()));
                    } else if ui.add_enabled(!s.busy, egui::Button::new("Annuler le découpage")).clicked() {
                        s.confirm_abort = true;
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let step = |ui: &mut egui::Ui, n: &str, t: &str, on: bool| {
                    let c = if on { accent } else { ui.visuals().weak_text_color() };
                    ui.label(RichText::new(n).strong().color(c));
                    ui.label(RichText::new(t).color(if on { ui.visuals().strong_text_color() } else { c }));
                };
                let left = !s.files.is_empty();
                step(ui, "1.", &format!("Coche les blocs (ou les lignes + / −) de la partie {part}"), left && n_sel == 0);
                ui.label(RichText::new("›").weak());
                step(ui, "2.", "Écris son message", left && n_sel > 0 && s.message.trim().is_empty());
                ui.label(RichText::new("›").weak());
                step(ui, "3.", &format!("« Valider la partie {part} »"), left && n_sel > 0 && !s.message.trim().is_empty());
                ui.label(RichText::new("›").weak());
                step(ui, "4.", "Recommence, puis « Terminer » quand tout est validé", !left);
            });
            if let Some(e) = &s.error {
                ui.label(RichText::new(format!("⚠ {e}")).color(err()));
            }
            ui.add_space(4.0);
        });

        egui::SidePanel::left("split_parts").resizable(true).default_width(260.0).show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("PARTIES").small().strong().color(accent));
            ui.add_space(4.0);
            for (i, (h, subj)) in s.parts.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("✔ {}", i + 1)).color(ok()).strong());
                    ui.label(RichText::new(h).monospace().weak());
                    ui.label(subj);
                });
            }
            if !s.files.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("● {part}")).color(accent).strong());
                    ui.label(RichText::new("en cours").italics());
                });
            }
            ui.add_space(10.0);
            ui.label(RichText::new("COMMIT D'ORIGINE").small().strong().weak());
            egui::ScrollArea::vertical().id_salt("split_orig_msg").show(ui, |ui| {
                ui.label(RichText::new(&s.info.message).weak());
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Les commits qui suivaient seront rejoués automatiquement à la fin. \
                         « Annuler le découpage » remet tout exactement comme avant.",
                    )
                    .small()
                    .weak(),
                );
            });
        });

        egui::TopBottomPanel::bottom("split_commit").show_inside(ui, |ui| {
            ui.add_space(6.0);
            if s.files.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("✔ Tout est validé").color(ok()).size(16.0).strong());
                    ui.label(format!("— le commit d'origine sera remplacé par {} commit(s).", s.parts.len()));
                });
                ui.add_space(4.0);
                let b = filled_button("Terminer le découpage", ok())
                    .min_size(egui::vec2(220.0, 30.0));
                if ui.add_enabled(!s.busy, b).clicked() {
                    finish = true;
                }
            } else {
                ui.add(
                    egui::TextEdit::multiline(&mut s.message)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text(format!("Message du commit de la partie {part}"))
                        .font(egui::TextStyle::Monospace),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let ready = n_sel > 0 && !s.message.trim().is_empty() && !s.busy;
                    let label = format!("Valider la partie {part}  ({n_sel} / {n_hunks} bloc(s))");
                    let b = filled_button(label, accent).min_size(egui::vec2(0.0, 28.0));
                    let r = ui.add_enabled(ready, b);
                    if r.clicked() {
                        commit = true;
                    }
                    if !ready && !s.busy {
                        let why = if n_sel == 0 { "coche au moins un bloc" } else { "écris un message" };
                        ui.label(RichText::new(format!("({why})")).weak());
                    }
                    ui.add_space(12.0);
                    if ui.button("Tout cocher").on_hover_text("Le reste va dans cette partie (la dernière)").clicked() {
                        s.sel.iter_mut().flatten().flatten().for_each(|b| *b = true);
                    }
                    if ui.button("Rien").clicked() {
                        s.sel.iter_mut().flatten().flatten().for_each(|b| *b = false);
                    }
                });
            }
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if s.files.is_empty() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Il ne reste aucun changement à répartir.").size(15.0));
                });
                return;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("CHANGEMENTS RESTANTS").small().strong().color(accent));
                ui.label(RichText::new(format!("{} fichier(s), {n_hunks} bloc(s)", s.files.len())).small().weak());
            });
            ui.add_space(4.0);
            let mono = egui::TextStyle::Monospace.resolve(ui.style());
            let fg = ui.visuals().text_color();
            egui::ScrollArea::vertical().id_salt("split_changes").auto_shrink([false, false]).show(ui, |ui| {
                for (fi, f) in s.files.iter().enumerate() {
                    let sel = &mut s.sel[fi];
                    let chosen = split::hunk_flags;
                    let flags: Vec<bool> = f.hunks.iter().zip(sel.iter()).flat_map(|(h, hs)| chosen(h, hs)).collect();
                    let all = !flags.is_empty() && flags.iter().all(|&b| b);
                    let any = flags.iter().any(|&b| b);
                    let folded = s.folded.contains(&f.path);
                    let partial = f.partial_ok();
                    ui.horizontal(|ui| {
                        let mut v = all;
                        if ui.add(egui::Checkbox::new(&mut v, "").indeterminate(any && !all)).changed() {
                            sel.iter_mut().flatten().for_each(|b| *b = v);
                        }
                        let arrow = if folded { "⏵" } else { "⏷" };
                        if ui.add(egui::Button::new(arrow).frame(false)).clicked() {
                            if folded {
                                s.folded.remove(&f.path);
                            } else {
                                s.folded.insert(f.path.clone());
                            }
                        }
                        let (kc, kt) = match f.kind {
                            'A' => (ok(), "nouveau"),
                            'D' => (err(), "supprimé"),
                            _ => (warn(), "modifié"),
                        };
                        ui.label(RichText::new(&f.path).strong());
                        ui.label(RichText::new(kt).small().color(kc));
                        if f.binary {
                            ui.label(RichText::new("binaire").small().weak());
                        }
                        if !partial {
                            ui.label(RichText::new("(se valide en entier)").small().weak());
                        } else if f.hunks.len() > 1 {
                            ui.label(RichText::new(format!("{} blocs", f.hunks.len())).small().weak());
                        }
                    });
                    if folded {
                        continue;
                    }
                    for (hi, h) in f.hunks.iter().enumerate() {
                        let hflags = chosen(h, &sel[hi]);
                        let on = hflags.iter().any(|&b| b);
                        let whole = !hflags.is_empty() && hflags.iter().all(|&b| b);
                        let stroke = if on { egui::Stroke::new(2.0_f32, accent) } else { ui.visuals().widgets.noninteractive.bg_stroke };
                        let fill = if on { icons::mix(bg, accent, 0.10) } else { ui.visuals().faint_bg_color };
                        let mut set_all: Option<bool> = None;
                        let mut toggle_line: Option<usize> = None;
                        let resp = egui::Frame::none()
                            .fill(fill)
                            .stroke(stroke)
                            .rounding(6.0)
                            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                            .outer_margin(egui::Margin { left: 26.0, right: 4.0, top: 2.0, bottom: 4.0 })
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let mut v = whole;
                                    let label = match (whole, on) {
                                        (true, _) => "tout le bloc dans cette partie",
                                        (false, true) => "une partie des lignes dans cette partie",
                                        _ => "pas dans cette partie",
                                    };
                                    if ui.add(egui::Checkbox::new(&mut v, RichText::new(label).small()).indeterminate(on && !whole)).changed() {
                                        set_all = Some(v);
                                    }
                                    ui.label(RichText::new(&h.lines[0]).monospace().small().color(accent));
                                    if partial {
                                        ui.label(RichText::new("· clic sur une ligne + / − pour la prendre ou la laisser").small().weak());
                                    }
                                });
                                let shown = h.lines.len().min(300);
                                for (li, l) in h.lines.iter().enumerate().take(shown).skip(1) {
                                    let change = h.is_change(li);
                                    let line_on = sel[hi][li];
                                    let (c, lbg, mark) = match l.chars().next() {
                                        Some('+') if line_on => (fg, Some(icons::mix(fill, ok(), 0.22)), "☑ "),
                                        Some('-') if line_on => (fg, Some(icons::mix(fill, err(), 0.22)), "☑ "),
                                        // not chosen: an added line is left out, a removed one stays as it is
                                        Some('+') | Some('-') => (ui.visuals().weak_text_color(), None, "☐ "),
                                        _ => (ui.visuals().weak_text_color(), None, "   "),
                                    };
                                    let mut fmt = TextFormat::simple(mono.clone(), c);
                                    if let Some(b) = lbg {
                                        fmt.background = b;
                                    }
                                    if change && !line_on && l.starts_with('+') {
                                        fmt.strikethrough = egui::Stroke::new(1.0_f32, c);
                                    }
                                    let text = if partial && change { format!("{mark}{}", l.replace('\t', "    ")) } else { l.replace('\t', "    ") };
                                    let mut job = LayoutJob::single_section(text, fmt);
                                    job.wrap.max_rows = 1;
                                    let sense = if partial && change { Sense::click() } else { Sense::hover() };
                                    let r = ui.add(egui::Label::new(job).sense(sense));
                                    if partial && change && r.clicked() {
                                        toggle_line = Some(li);
                                    }
                                }
                                if h.lines.len() > shown {
                                    ui.label(RichText::new(format!("… {} ligne(s) de plus", h.lines.len() - shown)).weak().small());
                                }
                            })
                            .response;
                        // clicking the block elsewhere than on a line toggles it whole
                        if toggle_line.is_none() && set_all.is_none() && resp.interact(Sense::click()).clicked() {
                            set_all = Some(!on);
                        }
                        if let Some(li) = toggle_line {
                            sel[hi][li] = !sel[hi][li];
                        }
                        if let Some(v) = set_all {
                            if partial {
                                sel[hi].iter_mut().for_each(|b| *b = v);
                            } else {
                                sel.iter_mut().flatten().for_each(|b| *b = v);
                            }
                        }
                    }
                    ui.add_space(6.0);
                }
            });
        });

        if commit {
            self.split_commit_part();
        }
        if finish {
            self.split_end(false);
        }
        if abort {
            self.split_end(true);
        }
    }
}
