//! Claude Code driven from Florm: one `claude -p` process per conversation, talking JSON lines
//! on stdin / stdout (no API key: the user's own Claude Code login is used, like `gh` for GitHub).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

pub fn not_installed() -> String {
    t!(
        "La commande claude (Claude Code) est introuvable. Installe-la (https://claude.com/claude-code), puis lance une fois « claude » dans un terminal pour te connecter.",
        "The claude command (Claude Code) was not found. Install it (https://claude.com/claude-code), then run “claude” once in a terminal to log in."
    )
    .to_string()
}

/// Models offered: (value of --model, label). Empty: the default of Claude Code.
pub const MODELS: [(&str, &str); 5] = [("", "Modèle par défaut"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku (rapide)"), ("fable", "Fable")];

/// Permission modes: (value of --permission-mode, label, explanation). "custom": what is checked
/// in the permissions.
pub const MODES: [(&str, &str, &str); 3] = [
    ("custom", "Selon les permissions", "Claude fait seulement ce qui est coché dans 🔒 Permissions ; tu gardes ou rejettes ses changements dans la revue"),
    ("plan", "Plan seulement", "Claude lit et propose un plan, sans rien modifier"),
    ("bypassPermissions", "Tout autoriser", "Claude peut tout faire sans demander (le bac à sable, s'il est coché, isole quand même ses commandes)"),
];

/// The permissions that can be checked: (key in the settings, label, explanation).
pub const PERMS: [(&str, &str, &str); 6] = [
    ("edit", "Modifier les fichiers", "Créer et modifier des fichiers (tu gardes ou rejettes chaque bloc dans la revue)"),
    ("bash", "Lancer des commandes", "Tests, git, scripts… dans un terminal caché"),
    ("web", "Accès à internet", "Lire des pages web et faire des recherches"),
    ("agents", "Sous-agents", "Confier des recherches à d'autres instances de Claude"),
    ("sandbox", "Bac à sable pour les commandes", "Les commandes tournent isolées (bubblewrap) : écriture seulement dans le projet, réseau filtré"),
    ("confine", "Rester dans le projet", "Les fichiers hors du dossier du projet sont inaccessibles, et tes réglages Claude Code personnels sont ignorés"),
];

/// A label or an explanation of `MODELS`, `MODES` and `PERMS` in the language of the interface.
pub fn label(fr: &'static str) -> &'static str {
    if !crate::i18n::en() {
        return fr;
    }
    match fr {
        "Modèle par défaut" => "Default model",
        "Haiku (rapide)" => "Haiku (fast)",
        "Selon les permissions" => "By permissions",
        "Claude fait seulement ce qui est coché dans 🔒 Permissions ; tu gardes ou rejettes ses changements dans la revue" => {
            "Claude only does what is ticked in 🔒 Permissions; you keep or reject its changes in the review"
        }
        "Plan seulement" => "Plan only",
        "Claude lit et propose un plan, sans rien modifier" => "Claude reads and suggests a plan, without changing anything",
        "Tout autoriser" => "Allow everything",
        "Claude peut tout faire sans demander (le bac à sable, s'il est coché, isole quand même ses commandes)" => {
            "Claude may do anything without asking (the sandbox, if ticked, still isolates its commands)"
        }
        "Modifier les fichiers" => "Edit files",
        "Créer et modifier des fichiers (tu gardes ou rejettes chaque bloc dans la revue)" => "Create and edit files (you keep or reject each block in the review)",
        "Lancer des commandes" => "Run commands",
        "Tests, git, scripts… dans un terminal caché" => "Tests, git, scripts… in a hidden terminal",
        "Accès à internet" => "Internet access",
        "Lire des pages web et faire des recherches" => "Read web pages and search",
        "Sous-agents" => "Subagents",
        "Confier des recherches à d'autres instances de Claude" => "Hand searches to other instances of Claude",
        "Bac à sable pour les commandes" => "Sandbox for the commands",
        "Les commandes tournent isolées (bubblewrap) : écriture seulement dans le projet, réseau filtré" => {
            "Commands run isolated (bubblewrap): writing only in the project, filtered network"
        }
        "Rester dans le projet" => "Stay in the project",
        "Les fichiers hors du dossier du projet sont inaccessibles, et tes réglages Claude Code personnels sont ignorés" => {
            "Files outside the project folder are out of reach, and your personal Claude Code settings are ignored"
        }
        _ => fr,
    }
}

/// Tools of each permission: those named in the allowed / denied lists (old names included) and
/// those of `--tools` (current names only, an unknown one is an error).
fn perm_tools(perm: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match perm {
        "edit" => (&["Edit", "MultiEdit", "Write", "NotebookEdit"], &["Edit", "Write"]),
        "bash" => (&["Bash"], &["Bash"]),
        "web" => (&["WebFetch", "WebSearch"], &["WebFetch", "WebSearch"]),
        "agents" => (&["Agent", "Task"], &["Agent"]),
        _ => (&[], &[]),
    }
}

/// Arguments of `claude` for mode `mode` (index in `MODES`), the permissions checked and the
/// budget (0: none). Nothing is ever asked: what is not allowed is refused.
pub fn permission_args(mode: usize, perms: &[String], budget: f64) -> Vec<String> {
    let has = |p: &str| perms.iter().any(|x| x == p);
    let mut a: Vec<String> = vec!["--permission-prompts".into(), "none".into()];
    match MODES.get(mode).map_or("custom", |m| m.0) {
        "custom" => {
            let (mut allowed, mut denied, mut tools) = (vec!["Read", "Grep", "Glob"], Vec::<&str>::new(), vec!["Read", "Grep", "Glob"]);
            for p in ["edit", "bash", "web", "agents"] {
                let (names, current) = perm_tools(p);
                if has(p) {
                    allowed.extend(names);
                    tools.extend(current);
                } else {
                    denied.extend(names);
                }
            }
            a.extend(["--permission-mode".into(), "dontAsk".into(), "--allowedTools".into(), allowed.join(",")]);
            if !denied.is_empty() {
                a.extend(["--disallowedTools".into(), denied.join(",")]);
            }
            if has("confine") {
                a.extend(["--restricted".into(), "--tools".into(), tools.join(",")]);
            }
        }
        m => a.extend(["--permission-mode".into(), m.to_string()]),
    }
    if has("sandbox") {
        a.extend(["--settings".into(), r#"{"sandbox":{"enabled":true}}"#.into()]);
    }
    if budget > 0.0 {
        a.extend(["--max-budget-usd".into(), format!("{budget}")]);
    }
    a
}

/// What Claude sends during a conversation.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The session started: its id and model.
    Init { session: String, model: String },
    /// A piece of the answer being written.
    Delta(String),
    /// A whole text block of the answer (replaces the pieces).
    Text(String),
    /// A tool used: its id, name and what it is about (file, command…).
    Tool { id: String, name: String, summary: String },
    /// The result of a tool.
    ToolResult { id: String, text: String, error: bool },
    /// End of the answer: error or not, its cost in dollars, the size of the model's context
    /// window (tokens, 0: unknown), and the tokens it used.
    Done { error: bool, text: String, cost: f64, window: u64, tokens: Tokens },
    /// Tokens of the conversation the model read for its last message (its context).
    Context(u64),
    /// Use of the plan's quotas: (window "five_hour" / "seven_day", fraction used, reset time in
    /// unix seconds).
    Limits(Vec<(String, f32, i64)>),
    /// The process ended (its error output, if any).
    Exited(String),
}

/// The tokens a turn used: `input` all the input (`cached` of it read from the cache), `output`
/// what the model wrote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

/// One line of `--output-format stream-json`.
pub fn parse_line(line: &str) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return Vec::new() };
    let s = |x: &Value| x.as_str().unwrap_or("").to_string();
    match v["type"].as_str() {
        Some("system") if v["subtype"] == "init" => vec![Event::Init { session: s(&v["session_id"]), model: s(&v["model"]) }],
        Some("stream_event") => {
            let e = &v["event"];
            if e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta" && v["parent_tool_use_id"].is_null() {
                vec![Event::Delta(s(&e["delta"]["text"]))]
            } else {
                Vec::new()
            }
        }
        // what subagents write is left out (only their tool calls count)
        Some("assistant") if v["parent_tool_use_id"].is_null() => {
            let mut events: Vec<Event> = v["message"]["content"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|c| match c["type"].as_str() {
                            Some("text") => Some(Event::Text(s(&c["text"]))),
                            Some("tool_use") => Some(Event::Tool { id: s(&c["id"]), name: s(&c["name"]), summary: tool_summary(&s(&c["name"]), &c["input"]) }),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let u = &v["message"]["usage"];
            let tokens: u64 = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"].iter().filter_map(|k| u[*k].as_u64()).sum();
            if tokens > 0 {
                events.push(Event::Context(tokens));
            }
            events
        }
        Some("rate_limit_event") => {
            let windows = &v["rate_limit_info"]["unifiedWindows"];
            let limits: Vec<(String, f32, i64)> = ["five_hour", "seven_day"]
                .iter()
                .filter_map(|k| {
                    let w = &windows[*k];
                    let used = w["utilization"].as_f64()?;
                    // a fraction; a percentage in case the format changes
                    let used = if used > 1.0 { used / 100.0 } else { used };
                    Some((k.to_string(), used.clamp(0.0, 1.0) as f32, w["resetsAt"].as_i64().unwrap_or(0)))
                })
                .collect();
            if limits.is_empty() {
                Vec::new()
            } else {
                vec![Event::Limits(limits)]
            }
        }
        Some("user") => v["message"]["content"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|c| c["type"] == "tool_result")
                    .map(|c| {
                        let text = match &c["content"] {
                            Value::String(t) => t.clone(),
                            Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
                            _ => String::new(),
                        };
                        Event::ToolResult { id: s(&c["tool_use_id"]), text, error: c["is_error"].as_bool().unwrap_or(false) }
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Some("result") => {
            let u = &v["usage"];
            let n = |k: &str| u[k].as_u64().unwrap_or(0);
            let cached = n("cache_read_input_tokens");
            let tokens = Tokens { input: n("input_tokens") + n("cache_creation_input_tokens") + cached, cached, output: n("output_tokens") };
            vec![Event::Done {
                error: v["is_error"].as_bool().unwrap_or(false),
                text: s(&v["result"]),
                cost: v["total_cost_usd"].as_f64().unwrap_or(0.0),
                window: v["modelUsage"].as_object().map_or(0, |m| m.values().filter_map(|u| u["contextWindow"].as_u64()).max().unwrap_or(0)),
                tokens,
            }]
        }
        _ => Vec::new(),
    }
}

/// What a tool call is about, in a few words: the file, the command, the pattern…
pub fn tool_summary(name: &str, input: &Value) -> String {
    let s = |k: &str| input[k].as_str().unwrap_or("").to_string();
    let short = |p: String| p.rsplit('/').take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/");
    match name {
        "Edit" | "MultiEdit" | "Write" | "Read" | "NotebookEdit" => short(s("file_path")).to_string() + &s("notebook_path"),
        "Bash" => s("command").lines().next().unwrap_or("").to_string(),
        "Grep" => format!("{} {}", s("pattern"), s("path")).trim().to_string(),
        "Glob" => s("pattern"),
        "WebFetch" => s("url"),
        "WebSearch" => s("query"),
        "Task" | "Agent" => s("description"),
        "TodoWrite" => t!("liste de tâches", "todo list").into(),
        _ => String::new(),
    }
}

/// A tool that changes files: the review is read again after it.
pub fn changes_files(name: &str) -> bool {
    matches!(name, "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "Bash")
}

/// A running conversation with Claude Code.
pub struct Session {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Event>,
}

impl Session {
    /// Starts `claude` in `dir`; `repaint` is called when something arrives.
    /// `perms`: the arguments of `permission_args`.
    /// `resume`: the id of an earlier session to continue (its context is kept).
    pub fn start(dir: &Path, model: &str, perms: &[String], resume: Option<&str>, repaint: impl Fn() + Send + 'static) -> Result<Session, String> {
        let mut args = vec!["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--include-partial-messages"];
        args.extend(perms.iter().map(String::as_str));
        if let Some(id) = resume {
            args.extend(["--resume", id]);
        }
        if !model.is_empty() {
            args.extend(["--model", model]);
        }
        let mut child = crate::os::command("claude")
            .args(&args)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| not_installed())?;
        let stdin = child.stdin.take().ok_or("stdin")?;
        let stdout = child.stdout.take().ok_or("stdout")?;
        let mut stderr = child.stderr.take().ok_or("stderr")?;
        let errors = Arc::new(Mutex::new(String::new()));
        let e2 = errors.clone();
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = stderr.read_to_string(&mut s);
            *e2.lock().unwrap() = s;
        });
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                for e in parse_line(&line) {
                    if tx.send(e).is_err() {
                        return;
                    }
                }
                repaint();
            }
            // stderr is read to its end by its thread: give it a moment
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = tx.send(Event::Exited(errors.lock().unwrap().trim().to_string()));
            repaint();
        });
        Ok(Session { child, stdin, rx })
    }

    fn write(&mut self, v: Value) -> Result<(), String> {
        writeln!(self.stdin, "{v}").and_then(|_| self.stdin.flush()).map_err(|e| tf!("Claude ne répond plus : {e}", "Claude no longer answers: {e}"))
    }

    /// Sends a message of the user.
    pub fn send(&mut self, text: &str) -> Result<(), String> {
        self.write(json!({"type": "user", "message": {"role": "user", "content": text}}))
    }

    /// Stops the answer being written (the conversation goes on).
    pub fn interrupt(&mut self) -> Result<(), String> {
        self.write(json!({"type": "control_request", "request_id": format!("stop-{}", std::process::id()), "request": {"subtype": "interrupt"}}))
    }

    /// What arrived since the last call.
    pub fn poll(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A commit message for `diff`, in the style of the last messages of the repository; `wish`:
/// what was asked to Claude for these changes, if known.
pub fn commit_message(dir: &Path, diff: &str, recent: &str, wish: &str) -> Result<String, String> {
    let mut prompt = String::from(t!(
        "Écris le message de commit de ce diff (reçu sur l'entrée standard). Suis le style des derniers messages du dépôt \
         (préfixes comme [FIX] / [IMP] / [ADD] module: … s'ils en ont), une première ligne courte, puis une ligne vide et \
         en quelques lignes le pourquoi si ce n'est pas évident. Réponds uniquement par le message, sans guillemets ni bloc de code.",
        "Write the commit message of this diff (given on the standard input). Follow the style of the last messages of the repository \
         (prefixes like [FIX] / [IMP] / [ADD] module: … if they have some), a short first line, then an empty line and \
         in a few lines the why if it is not obvious. Answer only with the message, without quotes or code block."
    ));
    if !recent.trim().is_empty() {
        prompt.push_str(t!("\n\nDerniers messages du dépôt :\n", "\n\nLast messages of the repository:\n"));
        prompt.push_str(recent.trim());
    }
    if !wish.trim().is_empty() {
        prompt.push_str(t!("\n\nCe que le développeur avait demandé pour ces changements :\n", "\n\nWhat the developer had asked for these changes:\n"));
        prompt.push_str(wish.trim());
    }
    // a very large diff is cut: the beginning says enough
    let diff: String = diff.chars().take(120_000).collect();
    run_once(dir, &["-p", &prompt, "--output-format", "text", "--model", "haiku", "--tools", "", "--no-session-persistence"], diff).map(|t| clean_message(&t))
}

/// Runs `claude` once with `args`, `input` on its standard input; returns its answer (its
/// spaces at the start kept: the indentation of code).
fn run_once(dir: &Path, args: &[&str], input: String) -> Result<String, String> {
    let mut child = crate::os::command("claude")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| not_installed())?;
    if let Some(mut stdin) = child.stdin.take() {
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    if out.status.success() && !text.trim().is_empty() {
        Ok(text)
    } else {
        Err(format!("{text}{}", String::from_utf8_lossy(&out.stderr)).trim().to_string())
    }
}

/// The answer of Claude about a selection: the code replacing it, or an explanation.
#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Code(String),
    Explain(String),
}

/// Claude on a piece of a file (`selection`, with the code `before` and `after` it for the
/// context): the new code for `ask`, or an explanation if it is a question.
pub fn inline(dir: &Path, file: &str, before: &str, selection: &str, after: &str, ask: &str) -> Result<Inline, String> {
    let prompt = tf!(
        "Tu travailles sur un extrait du fichier {file}. L'entrée standard contient le code autour, pour le contexte, \
         et l'extrait lui-même entre les lignes <extrait> et </extrait>.\n\
         Demande : {ask}\n\n\
         Si la demande est de modifier, réponds uniquement par le nouveau code qui remplace l'extrait (lui seul, pas le code \
         autour), avec la même indentation, sans bloc de code, sans balise, sans explication.\n\
         Si c'est une question (expliquer, trouver un bug…), réponds par la ligne EXPLICATION: puis ta réponse, courte, en \
         Markdown, en français.",
        "You work on an excerpt of the file {file}. The standard input holds the code around it, for the context, \
         and the excerpt itself between the lines <extrait> and </extrait>.\n\
         Request: {ask}\n\n\
         If the request is to change it, answer only with the new code replacing the excerpt (it alone, not the code \
         around), with the same indentation, without code block, without tag, without explanation.\n\
         If it is a question (explain, find a bug…), answer with the line EXPLICATION: then your answer, short, in \
         Markdown, in English."
    );
    let input = format!("{before}\n<extrait>\n{selection}\n</extrait>\n{after}");
    let text = run_once(dir,&["-p", &prompt, "--output-format", "text", "--model", "sonnet", "--tools", "", "--no-session-persistence"], input)?;
    Ok(parse_inline(&text))
}

/// Code (its fence or tags removed) or explanation.
fn parse_inline(text: &str) -> Inline {
    let t = text.trim_start();
    if let Some(rest) = t.strip_prefix("EXPLICATION:").or_else(|| t.strip_prefix("EXPLICATION :")) {
        return Inline::Explain(rest.trim().to_string());
    }
    // the indentation of the first line is kept: only blank lines are taken off
    let blank = |s: &str| s.trim_matches(['\n', '\r']).to_string();
    let mut code = blank(text.trim_end());
    if let Some(r) = code.trim_start().strip_prefix("<extrait>").and_then(|r| r.strip_suffix("</extrait>")) {
        code = blank(r);
    }
    if let Some(r) = code.trim_start().strip_prefix("```") {
        let body = r.split_once('\n').map_or("", |(_, b)| b);
        code = blank(body.trim_end().strip_suffix("```").unwrap_or(body));
    }
    Inline::Code(code)
}

/// A remark of an AI review, about a line of the diff (`line` 0 / `path` empty: about the whole).
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewNote {
    pub path: String,
    pub line: usize,
    /// "RIGHT": the new version of the file, "LEFT": the old one (a removed line).
    pub side: String,
    /// "bug", "risk", "suggestion" or "nit".
    pub severity: String,
    pub body: String,
}

/// Remarks of Claude on `diff`: nothing is sent anywhere, the user picks the ones to keep.
/// `about`: what the changes are (title and description of the pull request…); `language`: of
/// the remarks. Claude may read the files of `dir` (only read) for the context.
pub fn review(dir: &Path, diff: &str, about: &str, language: &str) -> Result<Vec<ReviewNote>, String> {
    let mut prompt = String::from(t!(
        "Fais la revue de code de ce diff (reçu sur l'entrée standard). Chaque ligne du diff commence par sa position : \
         [R12] = ligne 12 de la nouvelle version du fichier (lignes ajoutées « + » et lignes de contexte), \
         [L7] = ligne 7 de l'ancienne version (lignes supprimées « - »). Tu peux lire les fichiers du dépôt pour \
         comprendre le contexte (ils ne sont pas forcément dans la version du diff).\n\
         Relève seulement ce qui mérite vraiment un commentaire : bugs, cas oubliés, risques (sécurité, performance, \
         données), incohérences avec le reste du code, puis les améliorations nettes. Pas de compliments, pas de \
         remarques de style quand le code suit celui du fichier. Au plus 15 remarques, les plus importantes d'abord.\n\
         Réponds uniquement par un tableau JSON, sans texte autour ni bloc de code : \
         [{\"path\": \"chemin/du/fichier\", \"line\": 12, \"side\": \"RIGHT\", \"severity\": \"bug\", \"body\": \"…\"}] \
         où line et side viennent de la position de la ligne commentée (R → \"RIGHT\", L → \"LEFT\"), severity est \
         \"bug\", \"risk\", \"suggestion\" ou \"nit\", et body la remarque en Markdown, courte, adressée à l'auteur ; \
         quand c'est utile, un bloc ```suggestion avec le code qui remplace la ligne commentée (une seule ligne, côté RIGHT). \
         Une remarque sur l'ensemble : path \"\" et line 0. [] s'il n'y a rien à redire.",
        "Review the code of this diff (given on the standard input). Each line of the diff starts with its position: \
         [R12] = line 12 of the new version of the file (added lines “+” and context lines), \
         [L7] = line 7 of the old version (removed lines “-”). You may read the files of the repository to \
         understand the context (they are not necessarily at the version of the diff).\n\
         Point out only what really deserves a comment: bugs, forgotten cases, risks (security, performance, \
         data), inconsistencies with the rest of the code, then the clear improvements. No compliments, no \
         style remarks when the code follows the one of the file. At most 15 remarks, the most important first.\n\
         Answer only with a JSON array, without text around it or code block: \
         [{\"path\": \"path/of/the/file\", \"line\": 12, \"side\": \"RIGHT\", \"severity\": \"bug\", \"body\": \"…\"}] \
         where line and side come from the position of the commented line (R → \"RIGHT\", L → \"LEFT\"), severity is \
         \"bug\", \"risk\", \"suggestion\" or \"nit\", and body the remark in Markdown, short, addressed to the author; \
         when useful, a ```suggestion block with the code replacing the commented line (a single line, RIGHT side). \
         A remark on the whole: path \"\" and line 0. [] if there is nothing to say."
    ));
    prompt.push_str(&tf!("\nLangue des remarques : {language}.", "\nLanguage of the remarks: {language}."));
    if !about.trim().is_empty() {
        prompt.push_str(t!("\n\nCe que font ces changements :\n", "\n\nWhat these changes do:\n"));
        prompt.push_str(about.trim());
    }
    let numbered: String = number_diff(diff).chars().take(200_000).collect();
    let text = run_once(dir, &["-p", &prompt, "--output-format", "text", "--tools", "Read,Grep,Glob", "--no-session-persistence"], numbered)?;
    parse_review(&text)
}

/// The diff with the position of each line in front of it, so the model does not count lines.
fn number_diff(diff: &str) -> String {
    let mut out = String::with_capacity(diff.len() + diff.len() / 4);
    for l in crate::history::parse_diff(diff) {
        match (l.kind, l.old, l.new) {
            (crate::history::DiffKind::Removed, Some(o), _) => out.push_str(&format!("[L{o}] ")),
            (_, _, Some(n)) => out.push_str(&format!("[R{n}] ")),
            _ => {}
        }
        out.push_str(&l.text);
        out.push('\n');
    }
    out
}

/// The JSON array of the answer (the model sometimes puts text or a code fence around it).
fn parse_review(text: &str) -> Result<Vec<ReviewNote>, String> {
    let end = text.rfind(']').unwrap_or(0);
    // the first '[' starting a valid array up to the last ']' (a "[R12]" may come before it)
    let v = text
        .match_indices('[')
        .filter(|(a, _)| *a < end)
        .find_map(|(a, _)| serde_json::from_str::<Value>(&text[a..=end]).ok())
        .ok_or_else(|| tf!("Réponse inattendue de Claude : {}", "Unexpected answer from Claude: {}", text.chars().take(300).collect::<String>()))?;
    let s = |x: &Value| x.as_str().unwrap_or("").trim().to_string();
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .map(|n| ReviewNote {
                    path: s(&n["path"]),
                    line: n["line"].as_u64().or_else(|| n["line"].as_str().and_then(|l| l.trim_start_matches(['R', 'L']).parse().ok())).unwrap_or(0) as usize,
                    side: if s(&n["side"]).eq_ignore_ascii_case("LEFT") { "LEFT".into() } else { "RIGHT".into() },
                    severity: s(&n["severity"]).to_lowercase(),
                    body: s(&n["body"]),
                })
                .filter(|n| !n.body.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

/// Without the code fence or quotes a model sometimes puts around the message.
fn clean_message(t: &str) -> String {
    let t = t.trim();
    let t = t.strip_prefix("```").map(|r| r.split_once('\n').map_or(r, |(_, b)| b)).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    t.trim_matches('"').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_of_a_conversation() {
        assert_eq!(
            parse_line(r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-haiku-5-5"}"#),
            vec![Event::Init { session: "s1".into(), model: "claude-haiku-5-5".into() }]
        );
        assert_eq!(
            parse_line(r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Bon"}},"parent_tool_use_id":null}"#),
            vec![Event::Delta("Bon".into())]
        );
        let a = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"Je regarde."},{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/home/x/src/app/main.rs","old_string":"a"}}]}}"#;
        assert_eq!(
            parse_line(a),
            vec![Event::Text("Je regarde.".into()), Event::Tool { id: "t1".into(), name: "Edit".into(), summary: "src/app/main.rs".into() }]
        );
        // what a subagent writes is not shown
        assert!(parse_line(r#"{"type":"assistant","parent_tool_use_id":"t9","message":{"content":[{"type":"text","text":"x"}]}}"#).is_empty());
        let u = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"ok"}],"is_error":true}]}}"#;
        assert_eq!(parse_line(u), vec![Event::ToolResult { id: "t1".into(), text: "ok".into(), error: true }]);
        assert_eq!(
            parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"fini","total_cost_usd":0.012}"#),
            vec![Event::Done { error: false, text: "fini".into(), cost: 0.012, window: 0, tokens: Tokens::default() }]
        );
        // the tokens of the turn: the whole input (cache included) and what was written
        let r = r#"{"type":"result","is_error":false,"result":"ok","total_cost_usd":0.002,"usage":{"input_tokens":5,"cache_creation_input_tokens":100,"cache_read_input_tokens":900,"output_tokens":40}}"#;
        assert_eq!(parse_line(r), vec![Event::Done { error: false, text: "ok".into(), cost: 0.002, window: 0, tokens: Tokens { input: 1005, cached: 900, output: 40 } }]);
        assert!(parse_line("pas du json").is_empty());
        // usage: the context of the last message, the window of the model, the plan's quotas
        let a = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[],"usage":{"input_tokens":2,"cache_creation_input_tokens":4005,"cache_read_input_tokens":3262,"output_tokens":4}}}"#;
        assert_eq!(parse_line(a), vec![Event::Context(7273)]);
        let r = r#"{"type":"result","is_error":false,"result":"ok","total_cost_usd":0.001,"modelUsage":{"claude-haiku":{"contextWindow":1000000}}}"#;
        assert_eq!(parse_line(r), vec![Event::Done { error: false, text: "ok".into(), cost: 0.001, window: 1_000_000, tokens: Tokens::default() }]);
        let l = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","unifiedWindows":{"five_hour":{"utilization":0.8,"resetsAt":1792619400},"seven_day":{"utilization":0.5,"resetsAt":1792688400}}}}"#;
        assert_eq!(parse_line(l), vec![Event::Limits(vec![("five_hour".into(), 0.8, 1792619400), ("seven_day".into(), 0.5, 1792688400)])]);
    }

    #[test]
    fn summaries_and_messages() {
        assert_eq!(tool_summary("Bash", &json!({"command": "cargo test\necho"})), "cargo test");
        assert_eq!(tool_summary("Grep", &json!({"pattern": "fn main"})), "fn main");
        assert_eq!(clean_message("```\n[FIX] a: b\n```"), "[FIX] a: b");
        assert_eq!(clean_message("\"[IMP] x\""), "[IMP] x");
    }

    #[test]
    fn permissions() {
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let a = permission_args(0, &p(&["edit", "sandbox"]), 0.0).join(" ");
        assert!(a.contains("--permission-mode dontAsk"), "{a}");
        assert!(a.contains("--allowedTools Read,Grep,Glob,Edit,MultiEdit,Write,NotebookEdit"), "{a}");
        assert!(a.contains("--disallowedTools Bash,WebFetch,WebSearch,Agent,Task"), "{a}");
        assert!(a.contains(r#"--settings {"sandbox":{"enabled":true}}"#), "{a}");
        assert!(!a.contains("--restricted") && !a.contains("--max-budget-usd"), "{a}");
        let a = permission_args(0, &p(&["edit", "bash", "web", "agents", "confine"]), 2.5).join(" ");
        assert!(!a.contains("--disallowedTools") && !a.contains("--settings"), "{a}");
        assert!(a.contains("--restricted --tools Read,Grep,Glob,Edit,Write,Bash,WebFetch,WebSearch,Agent"), "{a}");
        assert!(a.ends_with("--max-budget-usd 2.5"), "{a}");
        assert_eq!(permission_args(1, &[], 0.0), p(&["--permission-prompts", "none", "--permission-mode", "plan"]));
        assert!(permission_args(2, &p(&["sandbox"]), 0.0).join(" ").contains("bypassPermissions --settings"));
    }

    #[test]
    fn review_answers() {
        let diff = "diff --git a/x.py b/x.py\n--- a/x.py\n+++ b/x.py\n@@ -1,3 +1,3 @@\n a = 1\n-b = 2\n+b = 3\n c = 4";
        let n = number_diff(diff);
        assert!(n.contains("[R1]  a = 1\n[L2] -b = 2\n[R2] +b = 3\n[R3]  c = 4"), "{n}");
        assert!(n.starts_with("diff --git"), "{n}");
        let t = "Voici :\n```json\n[{\"path\":\"x.py\",\"line\":2,\"side\":\"RIGHT\",\"severity\":\"Bug\",\"body\":\"b vaut 3 ?\"},\
                 {\"path\":\"x.py\",\"line\":\"L2\",\"side\":\"left\",\"severity\":\"nit\",\"body\":\"ok\"},{\"path\":\"\",\"line\":0,\"body\":\"\"}]\n```";
        assert_eq!(
            parse_review(t).unwrap(),
            vec![
                ReviewNote { path: "x.py".into(), line: 2, side: "RIGHT".into(), severity: "bug".into(), body: "b vaut 3 ?".into() },
                ReviewNote { path: "x.py".into(), line: 2, side: "LEFT".into(), severity: "nit".into(), body: "ok".into() },
            ]
        );
        assert_eq!(parse_inline("    x = 1\n    y = 2\n"), Inline::Code("    x = 1\n    y = 2".into()));
        assert_eq!(parse_inline("```python\n    x = 1\n```"), Inline::Code("    x = 1".into()));
        assert_eq!(parse_inline("<extrait>\n  a\n</extrait>"), Inline::Code("  a".into()));
        assert_eq!(parse_inline("EXPLICATION: Ça trie la liste."), Inline::Explain("Ça trie la liste.".into()));
        assert_eq!(parse_review("Rien à redire (voir [R12]) : []").unwrap(), vec![]);
        assert!(parse_review("désolé").is_err());
    }
}
