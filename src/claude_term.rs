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
/// `florm --claude-notify <file> <title> <body> <pid>`: a notification whose click brings Florm
/// (process `pid`) to the front on the conversation of `file`; it waits for the click.
const NOTIFY_FLAG: &str = "--claude-notify";

/// What Florm knows of a conversation in a terminal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// "busy", "waiting", "done", "ended" (`claude` stopped; empty: nothing asked yet).
    pub state: String,
    /// What it waits for ("Claude needs your permission to use Bash"…), while it waits.
    pub message: String,
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

/// The files changed by the conversation of `file`, one path a line, appended by its hook (apart
/// from its state: two hooks at once would lose a line rewriting the same file).
pub fn files_of(file: &Path) -> PathBuf {
    file.with_extension("files")
}

/// The files changed so far (each once, in the order of their first change).
pub fn changed_files(file: &Path) -> Vec<PathBuf> {
    let text = std::fs::read_to_string(files_of(file)).unwrap_or_default();
    let mut out: Vec<PathBuf> = Vec::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let p = PathBuf::from(l);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
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
    State { state: s("state"), message: s("message"), session_id: s("session_id"), title: s("title"), model: s("model"), cost: v["cost"].as_f64().unwrap_or(0.0), context: v["context"].as_u64().unwrap_or(0), limits: limits_of(&v["limits"]) }
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
/// its sandbox for the commands (`sandbox`: on or off, whatever the user's own settings say).
pub fn settings(exe: &Path, file: &Path, sandbox: bool) -> Value {
    let exe = exe.to_string_lossy();
    let file = file.to_string_lossy();
    let hook = |what: &str| json!([{ "hooks": [{ "type": "command", "command": format!("\"{exe}\" {HOOK_FLAG} \"{file}\" {what}") }] }]);
    let mut v = json!({
        "hooks": {
            "SessionStart": hook("start"),
            "SessionEnd": hook("end"),
            "UserPromptSubmit": hook("busy"),
            "Notification": hook("waiting"),
            "Stop": hook("done"),
            // the files it changes: reloaded at once in Florm, listed beside the conversation
            "PostToolUse": [{ "matcher": "Edit|MultiEdit|Write|NotebookEdit", "hooks": [{ "type": "command", "command": format!("\"{exe}\" {HOOK_FLAG} \"{file}\" edit") }] }],
        },
        "statusLine": { "type": "command", "command": format!("\"{exe}\" {STATUS_FLAG} \"{file}\"") },
    });
    // said both ways: off, a sandbox turned on in the user's own settings of Claude Code stays off too;
    // on, no command gets out of it (Claude Code would otherwise ask to run one outside)
    v["sandbox"] = if sandbox { json!({ "enabled": true, "allowUnsandboxedCommands": false }) } else { json!({ "enabled": false }) };
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

/// A hook: `what` happened (its event from Claude Code on stdin). `start`: its session (at the
/// start, after /clear); `edit`: a file changed, appended to `files_of`; the others its state,
/// what it waits for, and its first message kept as the title.
pub fn hook(file: &Path, what: &str) {
    on_event(file, what, &stdin_json());
}

fn on_event(file: &Path, what: &str, event: &Value) {
    if what == "edit" {
        let input = &event["tool_input"];
        let path = input["file_path"].as_str().or(input["notebook_path"].as_str()).unwrap_or("");
        if !path.is_empty() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(files_of(file)) {
                let _ = writeln!(f, "{path}");
            }
        }
        return;
    }
    // « Claude is waiting for your input », a minute after it finished: the end was told then
    // (Stop), this late one would only come too late
    let message = event["message"].as_str().unwrap_or("").trim();
    if what == "waiting" && (event["notification_type"] == "idle_prompt" || message.starts_with("Claude is waiting for your input")) {
        return;
    }
    let mut v = read_json(file);
    if what != "start" {
        v["state"] = json!(if what == "end" { "ended" } else { what });
        v["message"] = json!(if what == "waiting" { message } else { "" });
    }
    if let Some(id) = event["session_id"].as_str().filter(|s| !s.is_empty()) {
        v["session_id"] = json!(id);
    }
    let prompt = event["prompt"].as_str().unwrap_or("").trim();
    if what == "busy" && v["title"].as_str().unwrap_or("").is_empty() && !prompt.is_empty() && !prompt.starts_with('/') {
        v["title"] = json!(prompt.lines().next().unwrap_or("").chars().take(80).collect::<String>());
    }
    write_json(file, &v);
    if matches!(what, "waiting" | "done") {
        tell_if_away(file, what, &v);
    }
}

/// Name of the file where Florm says if it is in front (`0`) or not (`1`), its language and its
/// process (to bring it to the front from a notification).
const AWAY: &str = "away";

/// Florm in front or not, for the hooks (written when it changes): they tell the system at once
/// that Claude is done or waits, even while Florm is hidden and not drawn.
pub fn set_away(away: bool) {
    use std::sync::atomic::{AtomicU8, Ordering};
    static LAST: AtomicU8 = AtomicU8::new(u8::MAX);
    let now = u8::from(away) + if crate::i18n::en() { 2 } else { 0 };
    if LAST.swap(now, Ordering::Relaxed) != now {
        let _ = std::fs::create_dir_all(dir());
        let _ = std::fs::write(dir().join(AWAY), format!("{} {} {}", u8::from(away), if crate::i18n::en() { "en" } else { "fr" }, std::process::id()));
    }
}

/// From the hook: a notification of the system when Florm is not in front (Claude done, or
/// waiting for an answer: a permission, a question).
fn tell_if_away(file: &Path, what: &str, v: &Value) {
    let flag = file.parent().map(|d| std::fs::read_to_string(d.join(AWAY)).unwrap_or_default()).unwrap_or_default();
    let mut words = flag.split_whitespace();
    if words.next() != Some("1") {
        return;
    }
    let en = words.next() == Some("en");
    let pid = words.next().unwrap_or("").to_string();
    let title = v["title"].as_str().filter(|t| !t.is_empty()).unwrap_or("Claude");
    let message = v["message"].as_str().unwrap_or("");
    let body = match (what, en) {
        ("waiting", _) if !message.is_empty() => message,
        ("waiting", false) => "Claude attend ta réponse",
        ("waiting", true) => "Claude waits for your answer",
        (_, false) => "Claude a fini",
        (_, true) => "Claude is done",
    };
    let title = format!("Florm · {title}");
    // a click brings Florm to the front on this conversation: a process of its own waits for it
    // (the hook ends at once, Claude goes on)
    if let Ok(exe) = std::env::current_exe() {
        let spawned = crate::os::command(exe)
            .args([NOTIFY_FLAG, &file.to_string_lossy(), &title, body, &pid, if en { "en" } else { "fr" }])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if spawned.is_ok() {
            return;
        }
    }
    crate::os::notify(&title, body);
}

/// `--claude-notify`: the notification, waiting for a click (`os::notify_clickable`); clicked, the
/// conversation of `file` is asked to Florm (`open_asked`).
fn notify_and_wait(file: &Path, title: &str, body: &str, pid: &str, en: bool) {
    let pid = pid.parse().unwrap_or(0);
    crate::os::notify_clickable(title, body, if en { "Open" } else { "Ouvrir" }, pid, &file.with_extension("open"));
}

/// A notification was clicked for the conversation of `file` (once: the request is taken away).
pub fn open_asked(file: &Path) -> bool {
    std::fs::remove_file(file.with_extension("open")).is_ok()
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
    // not its session: given at the start, before anything was asked (the hooks give it)
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
        (Some(NOTIFY_FLAG), Some(file)) => {
            let arg = |k: usize| args.get(k).map_or("", String::as_str);
            notify_and_wait(Path::new(file), arg(3), arg(4), arg(5), arg(6) == "en");
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// « Claude is waiting for your input », a minute after the end: left aside (the end was
    /// told then); a permission asked is a wait.
    #[test]
    fn the_late_idle_notification_is_left_aside() {
        let d = std::env::temp_dir().join(format!("florm_claude_idle_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // Florm in front: nothing told to the system from the test
        std::fs::write(d.join(AWAY), "0 fr").unwrap();
        let f = d.join("c.json");
        on_event(&f, "done", &json!({}));
        on_event(&f, "waiting", &json!({"message": "Claude is waiting for your input", "notification_type": "idle_prompt"}));
        assert_eq!(read(&f).state, "done");
        on_event(&f, "waiting", &json!({"message": "Claude needs your permission to use Bash", "notification_type": "permission_prompt"}));
        assert_eq!((read(&f).state.as_str(), read(&f).message.as_str()), ("waiting", "Claude needs your permission to use Bash"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The command, the settings, and a state written by the hooks then read back.
    #[test]
    fn hooks_and_command() {
        let f = std::env::temp_dir().join(format!("florm_claude_term_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&f);
        // off is said too: the user's own settings cannot turn it back on
        assert!(settings(Path::new("/opt/florm"), &f, false).to_string().contains(r#""sandbox":{"enabled":false}"#));
        let s = settings(Path::new("/opt/florm"), &f, true).to_string();
        assert!(s.contains(r#"\"/opt/florm\" --claude-hook"#) && s.contains("UserPromptSubmit") && s.contains("SessionStart") && s.contains("SessionEnd") && s.contains(r#""matcher":"Edit|MultiEdit|Write|NotebookEdit""#) && s.contains("statusLine") && s.contains(r#""sandbox":{"allowUnsandboxedCommands":false,"enabled":true}"#), "{s}");
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

    /// What each hook leaves: the session, what it waits for, its end, the files changed.
    #[test]
    fn what_the_hooks_leave() {
        let f = std::env::temp_dir().join(format!("florm_claude_hooks_{}.json", std::process::id()));
        let _ = (std::fs::remove_file(&f), std::fs::remove_file(files_of(&f)));
        on_event(&f, "start", &json!({"session_id": "s1", "source": "startup"}));
        assert_eq!((read(&f).state.as_str(), read(&f).session_id.as_str()), ("", "s1"));
        on_event(&f, "busy", &json!({"session_id": "s1", "prompt": "corrige le bug"}));
        on_event(&f, "waiting", &json!({"session_id": "s1", "message": "Claude needs your permission to use Bash"}));
        let st = read(&f);
        assert_eq!((st.state.as_str(), st.message.as_str(), st.title.as_str()), ("waiting", "Claude needs your permission to use Bash", "corrige le bug"));
        on_event(&f, "busy", &json!({"session_id": "s1"}));
        assert_eq!(read(&f).message, "");
        on_event(&f, "edit", &json!({"tool_input": {"file_path": "/p/a.py"}}));
        on_event(&f, "edit", &json!({"tool_input": {"file_path": "/p/b.py"}}));
        on_event(&f, "edit", &json!({"tool_input": {"file_path": "/p/a.py"}}));
        on_event(&f, "edit", &json!({"tool_input": {"notebook_path": "/p/n.ipynb"}}));
        assert_eq!(changed_files(&f), ["/p/a.py", "/p/b.py", "/p/n.ipynb"].map(PathBuf::from));
        // an edit does not touch the state
        assert_eq!(read(&f).state, "busy");
        on_event(&f, "end", &json!({"session_id": "s1"}));
        assert_eq!(read(&f).state, "ended");
        let _ = (std::fs::remove_file(&f), std::fs::remove_file(files_of(&f)));
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
