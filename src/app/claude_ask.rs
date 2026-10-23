//! « Questions d'abord »: when a request leaves important choices open, Claude asks 1 to 3
//! questions with options instead of coding (a question costs little, a wrong implementation a
//! lot). Florm shows them as choices to click; « Répondre » sends the answers.

use super::*;

/// What is added to a message when the questions are asked for.
pub(super) fn instruction() -> &'static str {
    t!(INSTRUCTION, INSTRUCTION_EN)
}

const INSTRUCTION: &str = "(Avant de commencer : si la demande laisse des choix importants ouverts — où mettre quelque chose, quel \
comportement, quel périmètre —, ne modifie rien et réponds seulement par 1 à 3 questions, chacune dans ce format exact :\n\
[[QUESTION]] La question ?\n- première option\n- deuxième option\n[[/QUESTION]]\n\
Mets d'abord l'option que tu conseilles. Si la demande est assez claire, fais directement la tâche, sans question.)";

const INSTRUCTION_EN: &str = "(Before starting: if the request leaves important choices open — where to put something, which \
behavior, which scope —, change nothing and answer only with 1 to 3 questions, each in this exact format:\n\
[[QUESTION]] The question?\n- first option\n- second option\n[[/QUESTION]]\n\
Put the option you recommend first. If the request is clear enough, do the task directly, without questions.)";

const OPEN: &str = "[[QUESTION]]";
const CLOSE: &str = "[[/QUESTION]]";

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Question {
    pub(super) text: String,
    pub(super) options: Vec<String>,
}

/// The text of an answer without its questions, and the questions.
pub(super) fn parse(text: &str) -> (String, Vec<Question>) {
    let (mut rest, mut questions) = (String::new(), Vec::new());
    let mut s = text;
    while let Some(a) = s.find(OPEN) {
        rest.push_str(&s[..a]);
        let after = &s[a + OPEN.len()..];
        let (block, next) = match after.find(CLOSE) {
            Some(b) => (&after[..b], &after[b + CLOSE.len()..]),
            // still being written
            None => (after, ""),
        };
        let mut lines = block.lines();
        let q = lines.next().unwrap_or("").trim().to_string();
        let mut options: Vec<String> = Vec::new();
        let mut more: Vec<String> = Vec::new();
        for l in lines {
            let t = l.trim();
            match t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
                Some(o) => options.push(o.trim().to_string()),
                None if !t.is_empty() && options.is_empty() => more.push(t.to_string()),
                None => {}
            }
        }
        let text = if more.is_empty() { q } else { format!("{q} {}", more.join(" ")) };
        if !text.is_empty() {
            questions.push(Question { text, options });
        }
        s = next;
    }
    rest.push_str(s);
    // the place of the questions leaves no hole
    while rest.contains("\n\n\n") {
        rest = rest.replace("\n\n\n", "\n\n");
    }
    (rest.trim().to_string(), questions)
}

/// The choices of each question: the option checked, or what was written in « Autre ».
#[derive(Default)]
pub(super) struct Answers {
    /// Index of the item of the conversation they are for.
    pub(super) item: usize,
    pub(super) picked: Vec<Option<usize>>,
    pub(super) other: Vec<String>,
}

impl Answers {
    pub(super) fn for_item(item: usize, n: usize) -> Self {
        // the first option is the advised one: checked already
        Answers { item, picked: vec![Some(0); n], other: vec![String::new(); n] }
    }

    /// The message sent back: each question with its answer.
    pub(super) fn message(&self, questions: &[Question]) -> String {
        let mut out = String::from(t!("Mes réponses :", "My answers:"));
        for (k, q) in questions.iter().enumerate() {
            let other = self.other.get(k).map(|o| o.trim()).unwrap_or("");
            let a = if !other.is_empty() {
                other.to_string()
            } else {
                self.picked.get(k).copied().flatten().and_then(|i| q.options.get(i)).cloned().unwrap_or_else(|| t!("comme tu le juges bon", "as you see fit").into())
            };
            out.push_str(&format!("\n- {} → {a}", q.text));
        }
        out
    }
}

/// The questions to answer (`answers`: their choices, None when it is too late to answer them),
/// returns true when « Répondre » is clicked.
pub(super) fn questions_ui(ui: &mut egui::Ui, questions: &[Question], answers: Option<&mut Answers>, accent: Color32) -> bool {
    let mut send = false;
    let fill = ui.visuals().panel_fill;
    egui::Frame::none()
        .fill(icons::mix(fill, accent, 0.08))
        .stroke(egui::Stroke::new(1.0_f32, icons::mix(fill, accent, 0.4)))
        .rounding(8.0)
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(t!("Claude a besoin de précisions", "Claude needs more details")).small().strong().color(accent));
            match answers {
                Some(a) => {
                    for (k, q) in questions.iter().enumerate() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(&q.text).strong());
                        for (i, o) in q.options.iter().enumerate() {
                            let on = a.picked[k] == Some(i) && a.other[k].trim().is_empty();
                            if ui.radio(on, o.as_str()).clicked() {
                                a.picked[k] = Some(i);
                                a.other[k].clear();
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(t!("Autre :", "Other:")).small().weak());
                            ui.add(egui::TextEdit::singleline(&mut a.other[k]).hint_text(t!("ta réponse", "your answer")).desired_width(320.0));
                        });
                    }
                    ui.add_space(4.0);
                    send = ui.add(filled_button(t!("Répondre", "Answer"), accent)).on_hover_text(t!("Envoie tes choix ; Claude continue la tâche", "Sends your choices; Claude goes on with the task")).clicked();
                }
                None => {
                    for q in questions {
                        ui.label(format!("• {}", q.text));
                    }
                }
            }
        });
    send
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn questions_read_and_answered() {
        let t = "Deux points à préciser.\n[[QUESTION]] Où mettre le réglage ?\n- Préférences\n- settings.toml\n[[/QUESTION]]\n[[QUESTION]] Par défaut\nactivé ?\n- oui\n- non\n[[/QUESTION]]\nMerci.";
        let (rest, qs) = parse(t);
        assert_eq!(rest, "Deux points à préciser.\n\nMerci.");
        assert_eq!(qs, vec![
            Question { text: "Où mettre le réglage ?".into(), options: vec!["Préférences".into(), "settings.toml".into()] },
            Question { text: "Par défaut activé ?".into(), options: vec!["oui".into(), "non".into()] },
        ]);
        let mut a = Answers::for_item(3, 2);
        a.other[1] = "seulement pour Python".into();
        assert_eq!(a.message(&qs), "Mes réponses :\n- Où mettre le réglage ? → Préférences\n- Par défaut activé ? → seulement pour Python");
        // being written: what came so far
        assert_eq!(parse("Bon.\n[[QUESTION]] Lequel ?\n- a").1, vec![Question { text: "Lequel ?".into(), options: vec!["a".into()] }]);
        assert_eq!(parse("Rien à demander").1, vec![]);
    }
}
