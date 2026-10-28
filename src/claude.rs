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
pub const MODELS: [(&str, &str); 5] = [("", "Modèle par défaut"), ("opus", "Opus 5.5"), ("sonnet", "Sonnet 5.5"), ("haiku", "Haiku 5.5 (rapide)"), ("fable", "Fable 5.1")];

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
        "Haiku 5.5 (rapide)" => "Haiku 5.5 (fast)",
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

/// The value of `--permission-mode` of mode `mode` (index in `MODES`), also used to change it
/// while Claude works.
pub fn mode_value(mode: usize) -> &'static str {
    match MODES.get(mode).map_or("custom", |m| m.0) {
        "custom" => "dontAsk",
        m => m,
    }
}

/// Arguments of `claude` for mode `mode` (index in `MODES`), the permissions checked, the rules
/// of `/permissions` (`allow`, `deny`: `Bash(git *)`, `Edit(src/**)`, `mcp__github`…) and the
/// budget (0: none). Nothing is ever asked: what is not allowed is refused. A rule allowing a
/// part of a tool not checked (`Bash(git *)` without « Lancer des commandes ») allows that part.
pub fn permission_args(mode: usize, perms: &[String], allow: &[String], deny: &[String], budget: f64) -> Vec<String> {
    let has = |p: &str| perms.iter().any(|x| x == p);
    let ruled = |tool: &str| allow.iter().any(|r| r.starts_with(&format!("{tool}(")));
    let mut a: Vec<String> = vec!["--permission-prompts".into(), "none".into()];
    let rules = |list: &mut Vec<String>, extra: &[String]| list.extend(extra.iter().map(|r| r.trim().to_string()).filter(|r| !r.is_empty()));
    match MODES.get(mode).map_or("custom", |m| m.0) {
        "custom" => {
            let (mut allowed, mut denied, mut tools): (Vec<String>, Vec<String>, Vec<String>) = (vec!["Read".into(), "Grep".into(), "Glob".into()], Vec::new(), vec!["Read".into(), "Grep".into(), "Glob".into()]);
            for p in ["edit", "bash", "web", "agents"] {
                let (names, current) = perm_tools(p);
                if has(p) {
                    allowed.extend(names.iter().map(|n| n.to_string()));
                    tools.extend(current.iter().map(|n| n.to_string()));
                } else {
                    denied.extend(names.iter().filter(|n| !ruled(n)).map(|n| n.to_string()));
                    tools.extend(current.iter().filter(|n| ruled(n)).map(|n| n.to_string()));
                }
            }
            rules(&mut allowed, allow);
            rules(&mut denied, deny);
            a.extend(["--permission-mode".into(), "dontAsk".into(), "--allowedTools".into(), allowed.join(",")]);
            if !denied.is_empty() {
                a.extend(["--disallowedTools".into(), denied.join(",")]);
            }
            if has("confine") {
                a.extend(["--restricted".into(), "--tools".into(), tools.join(",")]);
            }
        }
        m => {
            a.extend(["--permission-mode".into(), m.to_string()]);
            let (mut allowed, mut denied) = (Vec::new(), Vec::new());
            rules(&mut allowed, allow);
            rules(&mut denied, deny);
            if !allowed.is_empty() {
                a.extend(["--allowedTools".into(), allowed.join(",")]);
            }
            if !denied.is_empty() {
                a.extend(["--disallowedTools".into(), denied.join(",")]);
            }
        }
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
    /// The session started: its id and model, the version of Claude Code, and its slash commands
    /// (`commands`: sent as a message; `terminal`: only in its terminal interface).
    Init { session: String, model: String, version: String, commands: Vec<String>, terminal: Vec<String> },
    /// The session of the answer that just ended (another one after `/clear`).
    Session(String),
    /// Id of a message of the answer: the conversation can be taken back to just after it
    /// (`--resume-session-at`).
    Uuid(String),
    /// `/clear`: Claude starts again without the conversation.
    Reset,
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
        Some("system") if v["subtype"] == "init" => {
            let names = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|c| c.as_str().map(String::from)).collect()).unwrap_or_default();
            vec![Event::Init { session: s(&v["session_id"]), model: s(&v["model"]), version: s(&v["claude_code_version"]), commands: names("slash_commands"), terminal: names("terminal_slash_commands") }]
        }
        Some("conversation_reset") => vec![Event::Reset],
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
            if let Some(id) = v["uuid"].as_str().filter(|i| !i.is_empty()) {
                events.push(Event::Uuid(id.to_string()));
            }
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
            let session = s(&v["session_id"]);
            let session = (!session.is_empty()).then_some(Event::Session(session));
            session.into_iter().chain([Event::Done {
                error: v["is_error"].as_bool().unwrap_or(false),
                text: s(&v["result"]),
                cost: v["total_cost_usd"].as_f64().unwrap_or(0.0),
                window: v["modelUsage"].as_object().map_or(0, |m| m.values().filter_map(|u| u["contextWindow"].as_u64()).max().unwrap_or(0)),
                tokens,
            }])
            .collect()
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
    /// `resume`: the id of an earlier session to continue (its context is kept), and `at`: the
    /// message of it after which it goes on (a copy of it, what came after is left out).
    /// `add_dirs`: folders Claude may use besides `dir`.
    pub fn start(dir: &Path, model: &str, perms: &[String], resume: Option<&str>, at: Option<&str>, add_dirs: &[std::path::PathBuf], repaint: impl Fn() + Send + 'static) -> Result<Session, String> {
        let mut args = vec!["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--include-partial-messages"];
        args.extend(perms.iter().map(String::as_str));
        if let Some(id) = resume {
            args.extend(["--resume", id]);
            if let Some(at) = at {
                args.extend(["--resume-session-at", at, "--fork-session"]);
            }
        }
        let dirs: Vec<String> = add_dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
        for d in &dirs {
            args.extend(["--add-dir", d.as_str()]);
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

    /// Sends a message of the user with images (PNG bytes), after its text.
    pub fn send_with_images(&mut self, text: &str, images: &[Vec<u8>]) -> Result<(), String> {
        let mut content = vec![json!({"type": "text", "text": text})];
        content.extend(images.iter().map(|png| json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": base64_encode(png)}})));
        self.write(json!({"type": "user", "message": {"role": "user", "content": content}}))
    }

    /// Stops the answer being written (the conversation goes on).
    pub fn interrupt(&mut self) -> Result<(), String> {
        self.write(json!({"type": "control_request", "request_id": format!("stop-{}", std::process::id()), "request": {"subtype": "interrupt"}}))
    }

    /// The permission mode changed while Claude works (`mode_value`).
    pub fn set_mode(&mut self, mode: &str) -> Result<(), String> {
        self.write(json!({"type": "control_request", "request_id": format!("mode-{}", std::process::id()), "request": {"subtype": "set_permission_mode", "mode": mode}}))
    }

    /// The model changed while Claude works (empty: the default one).
    pub fn set_model(&mut self, model: &str) -> Result<(), String> {
        let model = if model.is_empty() { Value::Null } else { json!(model) };
        self.write(json!({"type": "control_request", "request_id": format!("model-{}", std::process::id()), "request": {"subtype": "set_model", "model": model}}))
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

/// An MCP server known to Claude Code: its name, what it runs (command or URL), and its state
/// (connected, failed, waiting for approval…).
#[derive(Clone, Debug, PartialEq)]
pub struct Mcp {
    pub name: String,
    pub target: String,
    pub status: String,
}

/// The MCP servers of `dir` (`claude mcp list`: it starts each one to check it, so it takes a
/// few seconds).
pub fn mcp_list(dir: &Path) -> Result<Vec<Mcp>, String> {
    let out = crate::os::command("claude").args(["mcp", "list"]).current_dir(dir).output().map_err(|_| not_installed())?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() && text.trim().is_empty() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(parse_mcp_list(&text))
}

/// The lines `name: command or URL - status` of `claude mcp list`.
fn parse_mcp_list(text: &str) -> Vec<Mcp> {
    text.lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(": ")?;
            let (target, status) = rest.rsplit_once(" - ")?;
            let name = name.trim();
            (!name.is_empty() && !name.contains(char::is_whitespace)).then(|| Mcp { name: name.into(), target: target.trim().into(), status: status.trim().into() })
        })
        .collect()
}

/// Adds the MCP server `name` to Claude Code: an URL (http) or a command with its arguments;
/// `scope`: "local" (this project, for you), "project" (`.mcp.json`, shared) or "user" (all projects).
pub fn mcp_add(dir: &Path, name: &str, target: &str, scope: &str) -> Result<(), String> {
    let target = target.trim();
    let mut args: Vec<&str> = vec!["mcp", "add", "--scope", scope];
    if target.starts_with("http://") || target.starts_with("https://") {
        args.extend(["--transport", "http", name, target]);
    } else {
        args.extend([name, "--"]);
        args.extend(target.split_whitespace());
    }
    run_cli(dir, &args)
}

/// Removes the MCP server `name` from Claude Code.
pub fn mcp_remove(dir: &Path, name: &str) -> Result<(), String> {
    run_cli(dir, &["mcp", "remove", name])
}

fn run_cli(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = crate::os::command("claude").args(args).current_dir(dir).output().map_err(|_| not_installed())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout)).trim().to_string())
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
    let diff: String = diff.chars().take(30_000).collect();
    run_once(dir, &["-p", &prompt, "--output-format", "text", "--model", "haiku", "--tools", "", "--no-session-persistence"], diff).map(|t| clean_message(&t))
}

/// The description of a pull request (Markdown), from the commits of its branch (`log`) and its
/// diff: what changes and why, and what the reviewer should check.
pub fn pr_description(dir: &Path, log: &str, diff: &str) -> Result<String, String> {
    let prompt = String::from(t!(
        "Écris la description de cette pull request. L'entrée standard donne ses commits, puis son diff. Format Markdown : \
         une section « ## Résumé » avec quelques puces sur ce qui change et pourquoi, puis, si c'est utile au relecteur, \
         une section « ## À vérifier » avec les points à regarder de près. Pas de compliments, pas de bloc de code autour de \
         la réponse. Réponds uniquement par la description.",
        "Write the description of this pull request. The standard input gives its commits, then its diff. Markdown format: \
         a section « ## Summary » with a few bullet points on what changes and why, then, if it helps the reviewer, \
         a section « ## To check » with the points to look at closely. No compliments, no code block around \
         the answer. Answer only with the description."
    ));
    let log: String = log.chars().take(10_000).collect();
    let diff: String = diff.chars().take(30_000).collect();
    let input = format!("Commits :\n{log}\n\nDiff :\n{diff}");
    run_once(dir, &["-p", &prompt, "--output-format", "text", "--model", "haiku", "--tools", "", "--no-session-persistence"], input).map(|t| clean_message(&t))
}

/// The changes not committed (`blocks`: numbered B1, B2…) split by theme into several commits:
/// for each, its message and its blocks (their numbers, from 1), in the order to commit them.
pub fn commit_plan(dir: &Path, blocks: &str, recent: &str, wish: &str) -> Result<Vec<(String, Vec<usize>)>, String> {
    let mut prompt = String::from(t!(
        "L'entrée standard donne les changements non commités d'un dépôt, en blocs numérotés (B1, B2…). Répartis-les en \
         plusieurs commits, un par thème (une correction, une fonctionnalité, un renommage, de la doc…) ; un seul commit si \
         tout va ensemble. Chaque bloc va dans exactement un commit ; les blocs d'un même fichier peuvent aller dans des \
         commits différents. Ordonne les commits pour que chacun tienne seul (ce dont un autre dépend d'abord). Pour chaque \
         commit, écris son message dans le style des derniers messages du dépôt (préfixes comme [FIX] / [IMP] / [ADD] \
         module: … s'ils en ont), une première ligne courte, puis une ligne vide et le pourquoi en quelques lignes si ce \
         n'est pas évident. Réponds uniquement par un tableau JSON : [{\"message\": \"…\", \"blocks\": [1, 4]}, …].",
        "The standard input gives the uncommitted changes of a repository, in numbered blocks (B1, B2…). Split them into \
         several commits, one per theme (a fix, a feature, a renaming, docs…); a single commit if everything goes \
         together. Each block goes into exactly one commit; blocks of the same file may go into different commits. Order \
         the commits so that each stands on its own (what another depends on first). For each commit, write its message \
         in the style of the last messages of the repository (prefixes like [FIX] / [IMP] / [ADD] module: … if they have \
         some), a short first line, then an empty line and the why in a few lines if it is not obvious. Answer only with \
         a JSON array: [{\"message\": \"…\", \"blocks\": [1, 4]}, …]."
    ));
    if !recent.trim().is_empty() {
        prompt.push_str(t!("\n\nDerniers messages du dépôt :\n", "\n\nLast messages of the repository:\n"));
        prompt.push_str(recent.trim());
    }
    if !wish.trim().is_empty() {
        prompt.push_str(t!("\n\nCe que le développeur avait demandé pour ces changements :\n", "\n\nWhat the developer had asked for these changes:\n"));
        prompt.push_str(wish.trim());
    }
    let text = run_once(dir, &["-p", &prompt, "--output-format", "text", "--model", "sonnet", "--tools", "", "--no-session-persistence"], blocks.to_string())?;
    parse_plan(&text)
}

fn parse_plan(text: &str) -> Result<Vec<(String, Vec<usize>)>, String> {
    let v = json_array(text)?;
    let number = |b: &Value| b.as_u64().map(|n| n as usize).or_else(|| b.as_str().and_then(|s| s.trim().trim_start_matches(['B', 'b']).parse().ok()));
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .map(|c| (clean_message(c["message"].as_str().unwrap_or("")), c["blocks"].as_array().map(|b| b.iter().filter_map(number).collect()).unwrap_or_default()))
                .filter(|(m, b): &(String, Vec<usize>)| !m.is_empty() && !b.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

/// `/btw`: a quick question on the side, answered from the conversation of `session` (a copy of
/// it: the conversation does not get it), without tools, even while Claude is working.
pub fn btw(dir: &Path, session: Option<&str>, model: &str, question: &str) -> Result<String, String> {
    let mut args = vec!["-p", question, "--output-format", "text", "--tools", "", "--no-session-persistence"];
    if let Some(id) = session {
        args.extend(["--resume", id, "--fork-session"]);
    }
    if !model.is_empty() {
        args.extend(["--model", model]);
    }
    run_once(dir, &args, String::new())
}

/// Before a message starts work: the model that suits it better than `current` (index in `MODELS`),
/// with the reason; None when the current one does. Haiku reads only the message, without tools.
pub fn model_check(dir: &Path, message: &str, current: usize) -> Result<Option<(usize, String)>, String> {
    let label = MODELS[current.min(MODELS.len() - 1)].1;
    let prompt = tf!(
        "Tu choisis le modèle de Claude Code pour une demande, avant qu'il commence. Modèles :\n\
         - opus : Opus 5.5, pour les travaux difficiles (bug subtil, architecture, grosse refactorisation).\n\
         - sonnet : Sonnet 5.5, équilibré, pour la plupart des modifications.\n\
         - haiku : Haiku 5.5, rapide et économique, pour les petites tâches simples (renommer, une ligne, expliquer).\n\
         - fable : Fable 5.1, le plus capable, pour les tâches très longues ou très difficiles.\n\
         Le modèle actuel est {label}. La demande est sur l'entrée standard. Si le modèle actuel convient, réponds seulement \
         par : MODELE: garder. Sinon réponds sur deux lignes, sans rien d'autre :\n\
         MODELE: <opus, sonnet, haiku ou fable>\n\
         RAISON: <une phrase courte, en français, pourquoi>",
        "You choose the Claude Code model for a request, before it starts. Models:\n\
         - opus: Opus 5.5, for hard work (subtle bug, architecture, large refactoring).\n\
         - sonnet: Sonnet 5.5, balanced, for most changes.\n\
         - haiku: Haiku 5.5, fast and cheap, for small simple tasks (rename, one line, explain).\n\
         - fable: Fable 5.1, the most capable, for very long or very hard tasks.\n\
         The current model is {label}. The request is on the standard input. If the current model suits, answer only \
         with: MODELE: garder. Otherwise answer on two lines, nothing else:\n\
         MODELE: <opus, sonnet, haiku or fable>\n\
         RAISON: <one short sentence, in English, why>"
    );
    let text = run_once(dir, &["-p", &prompt, "--output-format", "text", "--model", "haiku", "--tools", "", "--no-session-persistence"], message.to_string())?;
    parse_model_answer(&text, current)
}

/// The answer of `model_check`: the model suggested (index in `MODELS`, and the reason), None when
/// the current one suits or the answer names no model of the list.
pub fn parse_model_answer(answer: &str, current: usize) -> Result<Option<(usize, String)>, String> {
    let line = |key: &str| answer.lines().find_map(|l| l.trim().strip_prefix(key)).map(|v| v.trim().to_string());
    let Some(name) = line("MODELE:") else { return Err(answer.trim().to_string()) };
    let Some(m) = MODELS.iter().position(|(v, _)| !v.is_empty() && *v == name.to_lowercase()) else { return Ok(None) };
    if m == current {
        return Ok(None);
    }
    Ok(Some((m, line("RAISON:").unwrap_or_default())))
}

/// Whether a file given to Claude is an image (it costs many tokens to read).
pub fn is_image(file: &str) -> bool {
    let lower = file.to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|e| lower.ends_with(e))
}

/// Whether the tokens read by the model fill more than 60 % of the window (0: unknown).
pub fn over_context(used: u64, window: u64) -> bool {
    window > 0 && used * 10 >= window * 6
}

/// The longest side of an image sent to Claude: a bigger one only costs more tokens.
const MAX_IMAGE_SIDE: u32 = 1568;

/// An image (RGBA pixels) as PNG, its longest side at most `MAX_IMAGE_SIDE`.
pub fn png_of(width: usize, height: usize, rgba: Vec<u8>) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(width as u32, height as u32, rgba).ok_or("image")?;
    let side = width.max(height) as u32;
    let img = if side > MAX_IMAGE_SIDE {
        let s = MAX_IMAGE_SIDE as f32 / side as f32;
        let (w, h) = (((width as f32 * s) as u32).max(1), ((height as f32 * s) as u32).max(1));
        image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// Standard base64 of `bytes` (with the padding).
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
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

/// The JSON array of an answer (the model sometimes puts text or a code fence around it).
fn json_array(text: &str) -> Result<Value, String> {
    let end = text.rfind(']').unwrap_or(0);
    // the first '[' starting a valid array up to the last ']' (a "[R12]" may come before it)
    text.match_indices('[')
        .filter(|(a, _)| *a < end)
        .find_map(|(a, _)| serde_json::from_str::<Value>(&text[a..=end]).ok())
        .ok_or_else(|| tf!("Réponse inattendue de Claude : {}", "Unexpected answer from Claude: {}", text.chars().take(300).collect::<String>()))
}

fn parse_review(text: &str) -> Result<Vec<ReviewNote>, String> {
    let v = json_array(text)?;
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
    fn plan_of_commits() {
        let text = "Voici :\n```json\n[{\"message\": \"[FIX] a\", \"blocks\": [1, \"B3\"]}, {\"message\": \"[IMP] b\", \"blocks\": [2]}, {\"message\": \"\", \"blocks\": [4]}]\n```";
        assert_eq!(parse_plan(text).unwrap(), vec![("[FIX] a".to_string(), vec![1, 3]), ("[IMP] b".to_string(), vec![2])]);
    }

    #[test]
    fn events_of_a_conversation() {
        assert_eq!(
            parse_line(r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-haiku-5-5"}"#),
            vec![Event::Init { session: "s1".into(), model: "claude-haiku-5-5".into(), version: String::new(), commands: Vec::new(), terminal: Vec::new() }]
        );
        let init = r#"{"type":"system","subtype":"init","session_id":"s1","model":"m","claude_code_version":"2.1","slash_commands":["compact","usage"],"terminal_slash_commands":["doctor"]}"#;
        assert_eq!(
            parse_line(init),
            vec![Event::Init { session: "s1".into(), model: "m".into(), version: "2.1".into(), commands: vec!["compact".into(), "usage".into()], terminal: vec!["doctor".into()] }]
        );
        assert_eq!(parse_line(r#"{"type":"conversation_reset","session_id":"s1"}"#), vec![Event::Reset]);
        let done = parse_line(r#"{"type":"result","is_error":false,"result":"","session_id":"s2","total_cost_usd":0}"#);
        assert_eq!(done.first(), Some(&Event::Session("s2".into())));
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
        // the id of a message of the answer: where the conversation can be taken back to
        let a = r#"{"type":"assistant","parent_tool_use_id":null,"uuid":"u-1","message":{"content":[{"type":"text","text":"ok"}]}}"#;
        assert_eq!(parse_line(a), vec![Event::Text("ok".into()), Event::Uuid("u-1".into())]);
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
        let a = permission_args(0, &p(&["edit", "sandbox"]), &[], &[], 0.0).join(" ");
        assert!(a.contains("--permission-mode dontAsk"), "{a}");
        assert!(a.contains("--allowedTools Read,Grep,Glob,Edit,MultiEdit,Write,NotebookEdit"), "{a}");
        assert!(a.contains("--disallowedTools Bash,WebFetch,WebSearch,Agent,Task"), "{a}");
        assert!(a.contains(r#"--settings {"sandbox":{"enabled":true}}"#), "{a}");
        assert!(!a.contains("--restricted") && !a.contains("--max-budget-usd"), "{a}");
        let a = permission_args(0, &p(&["edit", "bash", "web", "agents", "confine"]), &[], &[], 2.5).join(" ");
        assert!(!a.contains("--disallowedTools") && !a.contains("--settings"), "{a}");
        assert!(a.contains("--restricted --tools Read,Grep,Glob,Edit,Write,Bash,WebFetch,WebSearch,Agent"), "{a}");
        assert!(a.ends_with("--max-budget-usd 2.5"), "{a}");
        assert_eq!(permission_args(1, &[], &[], &[], 0.0), p(&["--permission-prompts", "none", "--permission-mode", "plan"]));
        assert!(permission_args(2, &p(&["sandbox"]), &[], &[], 0.0).join(" ").contains("bypassPermissions --settings"));
        // the rules: added to the lists; one allowing a part of a tool not checked lets that part in
        let a = permission_args(0, &p(&["edit", "confine"]), &p(&["Bash(git *)", "mcp__github"]), &p(&["Read(./.env)"]), 0.0).join(" ");
        assert!(a.contains("--allowedTools Read,Grep,Glob,Edit,MultiEdit,Write,NotebookEdit,Bash(git *),mcp__github"), "{a}");
        assert!(a.contains("--disallowedTools WebFetch,WebSearch,Agent,Task,Read(./.env)"), "{a}");
        assert!(a.contains("--tools Read,Grep,Glob,Edit,Write,Bash"), "{a}");
        let a = permission_args(1, &[], &[], &p(&["Bash(rm *)"]), 0.0).join(" ");
        assert!(a.ends_with("--permission-mode plan --disallowedTools Bash(rm *)"), "{a}");
        assert_eq!((mode_value(0), mode_value(1), mode_value(2)), ("dontAsk", "plan", "bypassPermissions"));
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

    #[test]
    fn the_model_suggested_is_read_from_the_answer() {
        // the current model suits, or the answer says so
        assert_eq!(parse_model_answer("MODELE: garder", 2).unwrap(), None);
        // another model: its index, with the reason
        assert_eq!(parse_model_answer("MODELE: opus\nRAISON: refonte transversale", 2).unwrap(), Some((1, "refonte transversale".into())));
        // a name that is not in the list, or the default one: no suggestion
        assert_eq!(parse_model_answer("MODELE: gpt", 2).unwrap(), None);
        // no MODELE line at all: an error with the answer
        assert!(parse_model_answer("désolé", 2).is_err());
    }

    #[test]
    fn mcp_servers_are_listed() {
        let text = "Checking MCP server health…\n\ngithub: npx -y @modelcontextprotocol/server-github - ✓ Connected\n\
                    sentry: https://mcp.sentry.dev/mcp (HTTP) - ✗ Failed to connect\nNo MCP servers configured. Use `claude mcp add` to add a server.";
        assert_eq!(
            parse_mcp_list(text),
            vec![
                Mcp { name: "github".into(), target: "npx -y @modelcontextprotocol/server-github".into(), status: "✓ Connected".into() },
                Mcp { name: "sentry".into(), target: "https://mcp.sentry.dev/mcp (HTTP)".into(), status: "✗ Failed to connect".into() },
            ]
        );
    }

    #[test]
    fn base64_is_the_standard_one() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"hel"), "aGVs");
    }

    #[test]
    fn images_and_full_context_are_recognised() {
        assert!(is_image("assets/Logo.PNG") && is_image("a.webp"));
        assert!(!is_image("src/main.rs"));
        assert!(over_context(600, 1000) && !over_context(599, 1000));
        assert!(!over_context(900, 0));
    }
}
