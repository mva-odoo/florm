//! Claude Code in a terminal (the Claude tab of Florm runs `claude` itself): what Florm learns of it
//! without reading its screen. `claude` is started with settings of Florm (`--settings`) whose
//! hooks and status line run Florm again (`florm --claude-hook`, `florm --claude-status`): they
//! write in a small file whether it works, waits or is done, its session, its first message, its
//! model and cost. If Claude Code changes them, only these extras go: the conversation still works.

use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

/// `florm --claude-hook <file> <busy|waiting|done>`: a hook of Claude Code (its event on stdin).
pub const HOOK_FLAG: &str = "--claude-hook";
/// `florm --claude-status <file>`: the status line of Claude Code (its session on stdin).
pub const STATUS_FLAG: &str = "--claude-status";

/// What Florm knows of a conversation in a terminal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// "busy", "waiting", "done" (empty: nothing asked yet).
    pub state: String,
    /// Its session in Claude Code (to go on with it: `claude --resume`).
    pub session_id: String,
    /// Its first message (the title of the conversation).
    pub title: String,
    /// The model at work ("Opus 4.6"…), what the session cost, the tokens read by its last answer.
    pub model: String,
    pub cost: f64,
    pub context: u64,
    /// The quotas of the plan: (window "five_hour" / "seven_day", fraction used, reset time).
    pub limits: Vec<(String, f32, i64)>,
}

/// The folder of the files of the conversations in a terminal.
pub fn dir() -> PathBuf {
    crate::config::data_dir().join("claude_term")
}

/// The file of conversation `key` (its state), and the settings given to its `claude`.
pub fn files(key: u64) -> (PathBuf, PathBuf) {
    let d = dir();
    (d.join(format!("{key}.json")), d.join(format!("{key}.settings.json")))
}

fn read_json(file: &Path) -> Value {
    std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}))
}

fn write_json(file: &Path, v: &Value) {
    if let Some(d) = file.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    // whole or not at all: Florm may read it at the same time
    let tmp = file.with_extension("tmp");
    if std::fs::write(&tmp, v.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, file);
    }
}

/// The state of a conversation, as its hooks and status line left it.
pub fn read(file: &Path) -> State {
    let v = read_json(file);
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    State { state: s("state"), session_id: s("session_id"), title: s("title"), model: s("model"), cost: v["cost"].as_f64().unwrap_or(0.0), context: v["context"].as_u64().unwrap_or(0), limits: limits_of(&v["limits"]) }
}

/// The quotas of the status line (`rate_limits`: each window its percentage used and its reset).
fn limits_of(v: &Value) -> Vec<(String, f32, i64)> {
    ["five_hour", "seven_day"]
        .iter()
        .filter_map(|w| {
            let l = &v[*w];
            let used = l["used_percentage"].as_f64()?;
            Some((w.to_string(), (used / 100.0) as f32, l["resets_at"].as_i64().unwrap_or(0)))
        })
        .collect()
}

/// The settings of Florm for `claude`: the hooks and the status line that write in `file`, and
/// its sandbox for the commands (`sandbox`).
pub fn settings(exe: &Path, file: &Path, sandbox: bool) -> Value {
    let exe = exe.to_string_lossy();
    let file = file.to_string_lossy();
    let hook = |what: &str| json!([{ "hooks": [{ "type": "command", "command": format!("\"{exe}\" {HOOK_FLAG} \"{file}\" {what}") }] }]);
    let mut v = json!({
        "hooks": {
            "UserPromptSubmit": hook("busy"),
            "Notification": hook("waiting"),
            "Stop": hook("done"),
        },
        "statusLine": { "type": "command", "command": format!("\"{exe}\" {STATUS_FLAG} \"{file}\"") },
    });
    if sandbox {
        v["sandbox"] = json!({ "enabled": true });
    }
    v
}

/// How a conversation starts: going on with its session (or a copy of it, `fork`), else with a
/// first message; and what Florm tells Claude of the project (its system prompt).
#[derive(Default)]
pub struct Start<'a> {
    pub session_id: &'a str,
    pub fork: bool,
    pub prompt: Option<&'a str>,
    pub system: &'a str,
    /// The plugin of the user's skills (`crate::skills`).
    pub plugin: Option<&'a Path>,
}

/// The command of a conversation: `claude` with the settings of Florm.
pub fn argv(settings_file: &Path, s: &Start) -> Vec<String> {
    let mut v = vec!["claude".to_string(), "--settings".into(), settings_file.to_string_lossy().into_owned()];
    if !s.system.trim().is_empty() {
        v.extend(["--append-system-prompt".into(), s.system.to_string()]);
    }
    if let Some(p) = s.plugin {
        v.extend(["--plugin-dir".into(), p.to_string_lossy().into_owned()]);
    }
    if !s.session_id.is_empty() {
        v.extend(["--resume".into(), s.session_id.to_string()]);
        if s.fork {
            v.push("--fork-session".into());
        }
    } else if let Some(p) = s.prompt.filter(|p| !p.trim().is_empty()) {
        v.push(p.to_string());
    }
    v
}

fn stdin_json() -> Value {
    let mut s = String::new();
    let _ = std::io::stdin().read_to_string(&mut s);
    serde_json::from_str(&s).unwrap_or(json!({}))
}

/// A hook: `what` happened (its event from Claude Code on stdin); the first message is kept as
/// the title.
pub fn hook(file: &Path, what: &str) {
    let event = stdin_json();
    let mut v = read_json(file);
    v["state"] = json!(what);
    if let Some(id) = event["session_id"].as_str().filter(|s| !s.is_empty()) {
        v["session_id"] = json!(id);
    }
    let prompt = event["prompt"].as_str().unwrap_or("").trim();
    if what == "busy" && v["title"].as_str().unwrap_or("").is_empty() && !prompt.is_empty() && !prompt.starts_with('/') {
        v["title"] = json!(prompt.lines().next().unwrap_or("").chars().take(80).collect::<String>());
    }
    write_json(file, &v);
}

/// The tokens read by the last answer of a session: its file of messages (`transcript_path`),
/// read from the end.
fn context_of(transcript: &str) -> u64 {
    let Ok(text) = std::fs::read_to_string(transcript) else { return 0 };
    text.lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find_map(|v| {
            let u = &v["message"]["usage"];
            u.is_object().then(|| ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"].iter().map(|k| u[*k].as_u64().unwrap_or(0)).sum())
        })
        .unwrap_or(0)
}

/// The status line: what Claude Code gives of its session (on stdin) kept in `file`; the line it
/// shows under its field (printed).
pub fn status(file: &Path) {
    let s = stdin_json();
    let mut v = read_json(file);
    let model = s["model"]["display_name"].as_str().unwrap_or("").to_string();
    let cost = s["cost"]["total_cost_usd"].as_f64().unwrap_or(0.0);
    v["model"] = json!(model);
    v["cost"] = json!(cost);
    if let Some(id) = s["session_id"].as_str().filter(|s| !s.is_empty()) {
        v["session_id"] = json!(id);
    }
    let context = context_of(s["transcript_path"].as_str().unwrap_or(""));
    if context > 0 {
        v["context"] = json!(context);
    }
    if s["rate_limits"].is_object() {
        v["limits"] = s["rate_limits"].clone();
    }
    write_json(file, &v);
    if cost > 0.0 {
        println!("{model} · {cost:.2} $");
    } else {
        println!("{model}");
    }
}

/// `florm --claude-hook …` / `florm --claude-status …`: done, and quit (true) — not Florm itself.
pub fn run_flag(args: &[String]) -> bool {
    match (args.get(1).map(String::as_str), args.get(2)) {
        (Some(HOOK_FLAG), Some(file)) => {
            hook(Path::new(file), args.get(3).map_or("", String::as_str));
            true
        }
        (Some(STATUS_FLAG), Some(file)) => {
            status(Path::new(file));
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The command, the settings, and a state written by the hooks then read back.
    #[test]
    fn hooks_and_command() {
        let f = std::env::temp_dir().join(format!("florm_claude_term_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&f);
        assert!(!settings(Path::new("/opt/florm"), &f, false).to_string().contains("sandbox"));
        let s = settings(Path::new("/opt/florm"), &f, true).to_string();
        assert!(s.contains(r#"\"/opt/florm\" --claude-hook"#) && s.contains("UserPromptSubmit") && s.contains("statusLine") && s.contains(r#""sandbox":{"enabled":true}"#), "{s}");
        let s = Path::new("/s.json");
        assert_eq!(argv(s, &Start { prompt: Some("fais ça"), ..Default::default() }), ["claude", "--settings", "/s.json", "fais ça"]);
        assert_eq!(argv(s, &Start { session_id: "abc", prompt: Some("ignored"), ..Default::default() }), ["claude", "--settings", "/s.json", "--resume", "abc"]);
        assert_eq!(argv(s, &Start { session_id: "abc", fork: true, system: "règles", ..Default::default() }), ["claude", "--settings", "/s.json", "--append-system-prompt", "règles", "--resume", "abc", "--fork-session"]);
        assert_eq!(read(&f), State::default());
        let mut v = json!({});
        v["state"] = json!("busy");
        v["title"] = json!("corrige le bug");
        v["session_id"] = json!("s1");
        v["cost"] = json!(1.5);
        v["limits"] = json!({"five_hour": {"used_percentage": 42.0, "resets_at": 1792619400}});
        write_json(&f, &v);
        let st = read(&f);
        assert_eq!((st.state.as_str(), st.title.as_str(), st.session_id.as_str(), st.cost), ("busy", "corrige le bug", "s1", 1.5));
        assert_eq!(st.limits, vec![("five_hour".to_string(), 0.42, 1792619400)]);
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn context_from_the_last_answer() {
        let f = std::env::temp_dir().join(format!("florm_transcript_{}.jsonl", std::process::id()));
        let lines = [
            r#"{"type":"user","message":{"content":"x"}}"#,
            r#"{"type":"assistant","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":1000,"cache_creation_input_tokens":5,"output_tokens":3}}}"#,
            r#"{"type":"user","message":{"content":"y"}}"#,
        ];
        std::fs::write(&f, lines.join("\n")).unwrap();
        assert_eq!(context_of(&f.to_string_lossy()), 1015);
        let _ = std::fs::remove_file(&f);
    }
}
