// The chat through Google's own command line, on the Google account it is signed in
// with: Antigravity CLI (`agy`) when it is installed, else the older Gemini CLI.
// Since June 2026 Gemini CLI no longer signs in personal Google accounts ("migrate
// to the Antigravity suite"), so `agy` is the way for most people; both run headless.
//
// Gemini CLI: each turn runs the installed
// `gemini` CLI headless, so it answers on the Google account the CLI is signed
// in with ("Login with Google", free tier or a Google AI plan) and no API key
// is needed. The twin of claude_code.rs.
//
// The question goes to `gemini` on stdin, never on the command line (a piped
// stdin is what puts the CLI in headless mode and is taken as the prompt), so
// nothing the user types is ever parsed by a shell. GEMINI_API_KEY and
// GOOGLE_API_KEY are removed from the child's environment, so the CLI uses the
// Google sign-in rather than billing a key. A conversation continues with
// `--resume <session id>`, the id kept next to the answer in the chat history.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::chat::{self, Chat, ChatContext, ChatReply, ModelInfo};
use crate::claude_code::{prompt_text, work_dir};
use crate::island::WINDOW_LABEL;
use crate::settings::Settings;

pub const PROVIDER: &str = "gemini-cli";
pub const DEFAULT_MODEL: &str = "auto";
/// The model aliases Gemini CLI takes on `--model`; only these are ever passed.
const MODELS: [(&str, &str); 4] = [
    ("auto", "Auto"),
    ("pro", "Pro"),
    ("flash", "Flash"),
    ("flash-lite", "Flash-Lite"),
];
/// A turn may read files and run tools: it gets longer than a plain chat answer.
const TIMEOUT: Duration = Duration::from_secs(600);
const MAX_ERROR: usize = 400;

pub fn models() -> Vec<ModelInfo> {
    // Antigravity's model slugs change with its releases: it picks its own.
    if matches!(find_cli(), Some(Cli::Agy(_))) {
        return vec![ModelInfo { id: DEFAULT_MODEL.into(), label: "Antigravity (default)".into() }];
    }
    MODELS
        .iter()
        .map(|(id, label)| ModelInfo {
            id: id.to_string(),
            label: label.to_string(),
        })
        .collect()
}

fn checked_model(model: &str) -> &'static str {
    MODELS
        .iter()
        .find(|(id, _)| *id == model)
        .map(|(id, _)| *id)
        .unwrap_or(DEFAULT_MODEL)
}

/// Gemini CLI has no flag for extra system instructions: a new session starts with them.
const STYLE: &str = "(You are answering in Coucou, a small chat window at the top of the user's screen. \
Respond in the user's language. Keep answers compact; use light Markdown, no tables or big headings.)";

fn full_prompt(resuming: bool, body: String) -> String {
    if resuming {
        body
    } else {
        format!("{STYLE}\n\n{body}")
    }
}

/// Which command line answers.
#[derive(Debug, Clone)]
enum Cli {
    /// Antigravity CLI, a native executable.
    Agy(PathBuf),
    /// The older Gemini CLI (Node, npm's gemini.cmd on Windows).
    Gemini,
}

fn find_cli() -> Option<Cli> {
    if let Some(agy) = find_agy() {
        return Some(Cli::Agy(agy));
    }
    command().ok().map(|_| Cli::Gemini)
}

/// Where `agy` is: on PATH, or where its installers put it (a GUI app started at
/// login may not have the updated PATH yet).
fn find_agy() -> Option<PathBuf> {
    #[cfg(windows)]
    let name = "agy.exe";
    #[cfg(not(windows))]
    let name = "agy";
    let home = crate::platform::home_dir();
    let mut places = vec![home.join(".local").join("bin").join(name)];
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        places.push(local.join("Programs").join("agy").join(name));
        places.push(local.join("Programs").join("antigravity-cli").join(name));
        places.push(local.join("agy").join(name));
    }
    places.into_iter().find(|p| p.is_file()).or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

/// The longest prompt put on agy's command line (Windows caps a command line at 32 767).
const MAX_AGY_PROMPT: usize = 24_000;

fn agy_args(prompt: &str, conversation: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = vec!["-p".into(), prompt.to_string(), "--output-format".into(), "stream-json".into()];
    if let Some(id) = conversation {
        a.push("--conversation".into());
        a.push(id.to_string());
    }
    a
}

/// A session or conversation id passed back on the command line: a plain token.
fn valid_id(id: &str) -> bool {
    (1..=128).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The id to continue from: the last answer's, if the same command line gave it.
fn last_session(history: &[Value], cli: &str) -> Option<String> {
    let last = history.iter().rev().find(|m| m["role"] == "assistant")?;
    let from = last.get("session_cli").and_then(Value::as_str).unwrap_or("gemini");
    if from != cli {
        return None;
    }
    last.get("session_id").and_then(Value::as_str).filter(|s| valid_id(s)).map(str::to_string)
}

/// What one line of `agy --output-format stream-json` says. The events are read
/// leniently (the payload under its event name, or at the top level).
#[derive(Debug, Default, PartialEq)]
struct AgyLine {
    conversation: Option<String>,
    delta: Option<String>,
    tool: bool,
    /// The result: Ok(response) or Err(message).
    result: Option<Result<String, String>>,
}

fn find_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).or_else(|| {
        ["init", "step_update", "result"].iter().find_map(|k| v.get(*k)?.get(key)?.as_str())
    })
}

fn parse_agy(line: &str) -> AgyLine {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return AgyLine::default() };
    let mut out = AgyLine { conversation: find_str(&v, "conversation_id").map(str::to_string), ..Default::default() };
    let kind = v.get("type").or_else(|| v.get("event")).and_then(Value::as_str).unwrap_or("");
    let step = v.get("step_update").filter(|s| s.is_object()).or((kind == "step_update").then_some(&v));
    if let Some(step) = step {
        match step.get("step_type").and_then(Value::as_str) {
            Some("agent_response") => out.delta = step.get("text_delta").and_then(Value::as_str).map(str::to_string),
            Some("tool") => out.tool = true,
            _ => {}
        }
    }
    let result = v.get("result").filter(|r| r.is_object()).or(
        (kind == "result" || (v.get("status").is_some() && v.get("response").is_some())).then_some(&v),
    );
    if let Some(r) = result {
        let status = r.get("status").and_then(Value::as_str).unwrap_or("");
        out.result = Some(if status.eq_ignore_ascii_case("success") {
            Ok(r.get("response").and_then(Value::as_str).unwrap_or_default().trim().to_string())
        } else {
            let e = &r["error"];
            Err(e.get("message").and_then(Value::as_str).or(e.as_str()).map(str::to_string)
                .unwrap_or_else(|| format!("Antigravity CLI stopped ({}).", if status.is_empty() { "no status" } else { status })))
        });
    }
    out
}

/// Runs one turn with agy, blocking. `on_delta` gets the text so far as it streams.
fn run_agy(
    agy: &Path,
    dir: &Path,
    conversation: Option<&str>,
    prompt: String,
    mut on_delta: impl FnMut(String),
) -> Result<(String, Option<String>), String> {
    if prompt.chars().count() > MAX_AGY_PROMPT {
        return Err("This message is too long for Antigravity CLI. Start a new chat, or send a shorter one.".into());
    }
    let spool = Spool::new().map_err(|e| format!("Coucou could not make a temporary file ({e})."))?;
    let out_file = std::fs::OpenOptions::new().append(true).open(&spool.out).map_err(|e| e.to_string())?;
    let err_file = std::fs::OpenOptions::new().append(true).open(&spool.err).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(agy);
    #[cfg(windows)]
    crate::platform::no_console(&mut cmd);
    let mut child = cmd
        .args(agy_args(&prompt, conversation))
        .current_dir(dir)
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .spawn()
        .map_err(|e| format!("Antigravity CLI could not start ({e})."))?;
    let mut tail = Tail::open(&spool.out).map_err(|e| e.to_string())?;

    let started = Instant::now();
    let mut visible = String::new();
    let mut conv: Option<String> = None;
    let mut after_tool = false;
    let mut result: Option<Result<String, String>> = None;
    let status = loop {
        let exited = child.try_wait().map_err(|e| e.to_string())?;
        for line in tail.lines(exited.is_some()) {
            let l = parse_agy(&line);
            if l.conversation.is_some() {
                conv = l.conversation;
            }
            if l.tool {
                after_tool = true;
            }
            if let Some(d) = l.delta.filter(|d| !d.is_empty()) {
                if after_tool && !visible.trim().is_empty() {
                    visible.push_str("\n\n");
                }
                after_tool = false;
                // Whether the deltas are pieces or the text so far, show the text so far.
                let tail_start = visible.rfind("\n\n").map(|i| i + 2).unwrap_or(0);
                if d.starts_with(&visible[tail_start..]) && visible.len() > tail_start {
                    visible.truncate(tail_start);
                }
                visible.push_str(&d);
                on_delta(visible.clone());
            }
            if l.result.is_some() {
                result = l.result;
            }
        }
        if let Some(status) = exited {
            break status;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Antigravity CLI took more than 10 minutes and was stopped.".into());
        }
        std::thread::sleep(Duration::from_millis(150));
    };
    let stderr = std::fs::read_to_string(&spool.err).unwrap_or_default();
    match result {
        Some(Ok(text)) if !text.is_empty() => Ok((text, conv)),
        Some(Ok(_)) if !visible.trim().is_empty() => Ok((visible.trim().to_string(), conv)),
        Some(Err(e)) => Err(e.chars().take(MAX_ERROR).collect()),
        _ if !visible.trim().is_empty() && status.success() => Ok((visible.trim().to_string(), conv)),
        _ => {
            let e = stderr.trim();
            Err(if e.is_empty() {
                format!(
                    "Antigravity CLI gave no answer (exit code {}). Run `agy` in a terminal once to sign in with Google.",
                    status.code().map(|c| c.to_string()).unwrap_or_else(|| "none".into())
                )
            } else {
                e.chars().take(MAX_ERROR).collect()
            })
        }
    }
}

const NOT_INSTALLED: &str = "Antigravity CLI is not installed on this computer. Install it from antigravity.google/download, run `agy` once in a terminal to sign in with Google, and restart Coucou. (Gemini CLI no longer signs in personal Google accounts.)";

/// Where `gemini` is. npm installs a `gemini.cmd` shim in %APPDATA%\npm, which a
/// GUI app launched at login may not have on its PATH.
fn command() -> Result<Command, String> {
    #[cfg(windows)]
    {
        let npm = std::env::var_os("APPDATA")
            .map(|d| PathBuf::from(d).join("npm").join("gemini.cmd"));
        let found = npm
            .filter(|p| p.is_file())
            .or_else(|| on_path("gemini.cmd"))
            .or_else(|| on_path("gemini.exe"));
        // Rust runs a .cmd through cmd.exe with its arguments escaped; the arguments
        // below are fixed strings, a checked model alias and a checked UUID anyway.
        let mut cmd = Command::new(found.ok_or_else(|| NOT_INSTALLED.to_string())?);
        crate::platform::no_console(&mut cmd);
        Ok(cmd)
    }
    #[cfg(not(windows))]
    {
        Ok(Command::new("gemini"))
    }
}

#[cfg(windows)]
fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

fn args(model: &str, session: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = ["--output-format", "stream-json", "--model", model]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(id) = session {
        a.push("--resume".into());
        a.push(id.to_string());
    }
    a
}

#[derive(Debug, PartialEq)]
enum Line {
    Session(String),
    Delta(String),
    /// A tool ran: the text after it starts a new paragraph.
    Tool,
    Error(String),
    Done { error: Option<String> },
    Other,
}

/// One line of `--output-format stream-json`.
fn parse_line(line: &str) -> Line {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Line::Other;
    };
    match v["type"].as_str() {
        Some("init") => v["session_id"]
            .as_str()
            .map(|s| Line::Session(s.to_string()))
            .unwrap_or(Line::Other),
        Some("message") if v["role"] == "assistant" => v["content"]
            .as_str()
            .map(|t| Line::Delta(t.to_string()))
            .unwrap_or(Line::Other),
        Some("tool_use") => Line::Tool,
        Some("error") if v["severity"] == "error" => {
            Line::Error(v["message"].as_str().unwrap_or_default().to_string())
        }
        Some("result") => Line::Done {
            error: (v["status"] != "success").then(|| {
                v["error"]["message"]
                    .as_str()
                    .unwrap_or("Gemini CLI reported an error.")
                    .to_string()
            }),
        },
        _ => Line::Other,
    }
}

/// Where one turn's output goes. Gemini CLI is a Node program, and on Windows Node
/// writes to a pipe asynchronously: when the CLI ends with process.exit, what it had
/// not flushed yet is lost, and Coucou got no answer at all. Writes to a file are
/// synchronous, so the CLI writes to files here and Coucou reads them as they grow.
struct Spool {
    out: PathBuf,
    err: PathBuf,
}

impl Spool {
    fn new() -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("coucou-gemini-{}-{n}", std::process::id()));
        let spool = Spool { out: base.with_extension("out"), err: base.with_extension("err") };
        std::fs::File::create(&spool.out)?;
        std::fs::File::create(&spool.err)?;
        Ok(spool)
    }
}

impl Drop for Spool {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.out);
        let _ = std::fs::remove_file(&self.err);
    }
}

/// The complete lines added to a growing file since the last call.
struct Tail {
    file: std::fs::File,
    carry: Vec<u8>,
}

impl Tail {
    fn open(path: &Path) -> std::io::Result<Self> {
        Ok(Tail { file: std::fs::File::open(path)?, carry: Vec::new() })
    }

    fn lines(&mut self, flush: bool) -> Vec<String> {
        let mut buf = Vec::new();
        let _ = self.file.read_to_end(&mut buf);
        self.carry.extend_from_slice(&buf);
        let mut out = Vec::new();
        while let Some(i) = self.carry.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.carry.drain(..=i).collect();
            out.push(String::from_utf8_lossy(&line).trim_end().to_string());
        }
        if flush && !self.carry.is_empty() {
            out.push(String::from_utf8_lossy(&self.carry).trim_end().to_string());
            self.carry.clear();
        }
        out
    }
}

/// Runs one turn, blocking. `on_delta` gets the text so far as it streams.
fn run(
    dir: &Path,
    model: &str,
    session: Option<&str>,
    prompt: String,
    mut on_delta: impl FnMut(String),
) -> Result<(String, Option<String>), String> {
    let spool = Spool::new().map_err(|e| format!("Coucou could not make a temporary file for Gemini CLI ({e})."))?;
    let out_file = std::fs::OpenOptions::new().append(true).open(&spool.out).map_err(|e| e.to_string())?;
    let err_file = std::fs::OpenOptions::new().append(true).open(&spool.err).map_err(|e| e.to_string())?;
    let mut child = command()?
        .args(args(model, session))
        .current_dir(dir)
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                NOT_INSTALLED.to_string()
            } else {
                format!("Gemini CLI could not start ({e}).")
            }
        })?;

    let mut stdin = child.stdin.take().expect("piped stdin");
    std::thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
        // Dropping stdin closes it: the CLI reads it to the end before it starts.
    });
    let mut tail = Tail::open(&spool.out).map_err(|e| e.to_string())?;

    let started = Instant::now();
    let mut visible = String::new();
    let mut new_session: Option<String> = None;
    let mut after_tool = false;
    let mut errors: Vec<String> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    let mut done: Option<Option<String>> = None;
    let status = loop {
        let exited = child.try_wait().map_err(|e| e.to_string())?;
        for line in tail.lines(exited.is_some()) {
            match parse_line(&line) {
                Line::Session(id) => new_session = Some(id),
                Line::Delta(t) => {
                    if after_tool && !visible.trim().is_empty() {
                        visible.push_str("\n\n");
                    }
                    after_tool = false;
                    visible.push_str(&t);
                    on_delta(visible.clone());
                }
                Line::Tool => after_tool = true,
                Line::Error(m) if !m.is_empty() => errors.push(m),
                Line::Done { error } => done = Some(error),
                // Not an event: the CLI printed something else (a warning, an error).
                Line::Other if !line.trim().is_empty() && !line.trim_start().starts_with('{') => {
                    other.push(line.trim().to_string())
                }
                _ => {}
            }
        }
        if let Some(status) = exited {
            break status;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Gemini CLI took more than 10 minutes and was stopped.".into());
        }
        std::thread::sleep(Duration::from_millis(150));
    };
    let stderr = std::fs::read_to_string(&spool.err).unwrap_or_default();
    let answer = visible.trim().to_string();

    match done {
        Some(None) if !answer.is_empty() => Ok((answer, new_session)),
        Some(Some(err)) => Err(err.chars().take(MAX_ERROR).collect()),
        None if !answer.is_empty() && status.success() => Ok((answer, new_session)),
        _ => {
            let said = errors
                .last()
                .cloned()
                .or_else(|| {
                    let e = stderr.trim();
                    (!e.is_empty()).then(|| e.to_string())
                })
                .or_else(|| (!other.is_empty()).then(|| other.join("\n")));
            Err(match said {
                Some(msg) => msg.chars().take(MAX_ERROR).collect(),
                None => format!(
                    "Gemini CLI gave no answer (exit code {}). Run `gemini` in a terminal once to check it is signed in with Google.",
                    status.code().map(|c| c.to_string()).unwrap_or_else(|| "none".into())
                ),
            })
        }
    }
}

async fn turn(
    chat: &Chat,
    settings: &Settings,
    model: &str,
    query: String,
    context: Option<ChatContext>,
    on_delta: impl FnMut(String) + Send + 'static,
) -> Result<ChatReply, String> {
    let cli = find_cli().ok_or_else(|| NOT_INSTALLED.to_string())?;
    let cli_name = if matches!(cli, Cli::Agy(_)) { "agy" } else { "gemini" };
    let turn = chat.begin(PROVIDER);
    let session = last_session(&turn.history, cli_name);
    let prompt = full_prompt(
        session.is_some(),
        prompt_text(
            turn.first,
            session.is_some(),
            &turn.history,
            context.as_ref(),
            &query,
        ),
    );
    let dir: PathBuf = work_dir(settings);
    let model = checked_model(model).to_string();
    let (answer, new_session) = tokio::task::spawn_blocking(move || match cli {
        Cli::Agy(agy) => run_agy(&agy, &dir, session.as_deref(), prompt, on_delta),
        Cli::Gemini => run(&dir, &model, session.as_deref(), prompt, on_delta),
    })
    .await
    .map_err(|e| e.to_string())??;

    let plain = chat::plain_question(turn.first, context.as_ref(), &query);
    let mut assistant = json!({ "role": "assistant", "content": answer });
    if let Some(id) = new_session.filter(|s| valid_id(s)) {
        assistant["session_id"] = json!(id);
        assistant["session_cli"] = json!(cli_name);
    }
    chat.commit(
        &turn,
        json!({ "role": "user", "content": plain }),
        assistant,
        &plain,
        &answer,
    );
    Ok(ChatReply { text: answer })
}

/// One island turn; the streamed text goes to the island as `chat-delta`.
pub async fn send(
    app: &AppHandle,
    chat: &Chat,
    settings: &Settings,
    model: &str,
    query: String,
    context: Option<ChatContext>,
) -> Result<ChatReply, String> {
    let app = app.clone();
    turn(chat, settings, model, query, context, move |visible| {
        let _ = app.emit_to(WINDOW_LABEL, "chat-delta", visible);
    })
    .await
}

/// One turn of the phone's conversation (phone_link/chat_backend.rs): the same
/// CLI on the same Google account, the streamed text going to the phone.
pub async fn send_for_phone(
    chat: &Chat,
    settings: &Settings,
    model: &str,
    query: String,
    on_delta: impl FnMut(String) + Send + 'static,
) -> Result<ChatReply, String> {
    turn(chat, settings, model, query, None, on_delta).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b6f1f5e-2c1d-4a7b-9b3e-1f2a3b4c5d6e";

    #[test]
    fn only_known_models_and_uuids_reach_the_command_line() {
        assert_eq!(checked_model("flash"), "flash");
        assert_eq!(checked_model("x & del *"), DEFAULT_MODEL);
        let a = args("pro", Some(ID));
        assert_eq!(a[..4], ["--output-format", "stream-json", "--model", "pro"]);
        assert!(a.ends_with(&["--resume".to_string(), ID.to_string()]));
        assert!(!args("pro", None).contains(&"--resume".to_string()));
    }

    #[test]
    fn stream_lines_are_read() {
        let init = format!(r#"{{"type":"init","timestamp":"t","session_id":"{ID}","model":"auto"}}"#);
        assert_eq!(parse_line(&init), Line::Session(ID.into()));
        let me = r#"{"type":"message","timestamp":"t","role":"user","content":"hi"}"#;
        assert_eq!(parse_line(me), Line::Other);
        let d = r#"{"type":"message","timestamp":"t","role":"assistant","content":"Hel","delta":true}"#;
        assert_eq!(parse_line(d), Line::Delta("Hel".into()));
        let tool = r#"{"type":"tool_use","timestamp":"t","tool_name":"read_file","tool_id":"1","parameters":{}}"#;
        assert_eq!(parse_line(tool), Line::Tool);
        let warn = r#"{"type":"error","timestamp":"t","severity":"warning","message":"slow"}"#;
        assert_eq!(parse_line(warn), Line::Other);
        let ok = r#"{"type":"result","timestamp":"t","status":"success","stats":{}}"#;
        assert_eq!(parse_line(ok), Line::Done { error: None });
        let bad = r#"{"type":"result","timestamp":"t","status":"error","error":{"type":"x","message":"Quota exceeded"}}"#;
        assert_eq!(parse_line(bad), Line::Done { error: Some("Quota exceeded".into()) });
        assert_eq!(parse_line("not json"), Line::Other);
    }

    #[test]
    fn agy_stream_lines_are_read_whatever_their_nesting() {
        let init = r#"{"type":"init","init":{"conversation_id":"conv-1"}}"#;
        assert_eq!(parse_agy(init).conversation.as_deref(), Some("conv-1"));
        let step = r#"{"type":"step_update","step_update":{"conversation_id":"conv-1","step_index":2,"state":"ACTIVE","step_type":"agent_response","text_delta":"Hel"}}"#;
        assert_eq!(parse_agy(step).delta.as_deref(), Some("Hel"));
        let flat = r#"{"type":"step_update","step_type":"tool","state":"DONE","tool_info":{"name":"read_file"}}"#;
        assert!(parse_agy(flat).tool);
        let ok = r#"{"type":"result","result":{"conversation_id":"conv-1","status":"SUCCESS","response":" Hello "}}"#;
        assert_eq!(parse_agy(ok).result, Some(Ok("Hello".into())));
        let bad = r#"{"result":{"status":"ERROR","error":{"message":"authentication required"}}}"#;
        assert_eq!(parse_agy(bad).result, Some(Err("authentication required".into())));
        let envelope = r#"{"conversation_id":"c","status":"SUCCESS","response":"Hi"}"#;
        assert_eq!(parse_agy(envelope).result, Some(Ok("Hi".into())));
        assert_eq!(parse_agy("Signing in…"), AgyLine::default());
    }

    #[test]
    fn a_conversation_resumes_only_with_the_cli_that_started_it() {
        let h = vec![json!({"role":"assistant","content":"yo","session_id":"conv-1","session_cli":"agy"})];
        assert_eq!(last_session(&h, "agy").as_deref(), Some("conv-1"));
        assert_eq!(last_session(&h, "gemini"), None);
        let old = vec![json!({"role":"assistant","content":"yo","session_id":"0b6f1f5e-2c1d-4a7b-9b3e-1f2a3b4c5d6e"})];
        assert!(last_session(&old, "gemini").is_some());
        assert!(!valid_id("x & del *"));
        let a = agy_args("hi", Some("conv-1"));
        assert_eq!(a, ["-p", "hi", "--output-format", "stream-json", "--conversation", "conv-1"]);
    }

    #[test]
    fn a_growing_file_is_read_line_by_line() {
        let spool = Spool::new().unwrap();
        let mut tail = Tail::open(&spool.out).unwrap();
        let mut w = std::fs::OpenOptions::new().append(true).open(&spool.out).unwrap();
        w.write_all(b"{\"a\":1}\n{\"b\"").unwrap();
        assert_eq!(tail.lines(false), vec!["{\"a\":1}".to_string()]);
        w.write_all(b":2}\r\nlast").unwrap();
        assert_eq!(tail.lines(false), vec!["{\"b\":2}".to_string()]);
        assert_eq!(tail.lines(true), vec!["last".to_string()]);
        let path = spool.out.clone();
        drop(spool);
        assert!(!path.exists());
    }

    #[test]
    fn a_new_session_carries_the_style_a_resumed_one_does_not() {
        assert!(full_prompt(false, "hi".into()).starts_with("(You are answering in Coucou"));
        assert!(full_prompt(false, "hi".into()).ends_with("hi"));
        assert_eq!(full_prompt(true, "hi".into()), "hi");
    }
}
