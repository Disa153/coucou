// The chat through the user's own Gemini CLI: each turn runs the installed
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
use crate::claude_code::{last_session, prompt_text, valid_session, work_dir};
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

/// Where `gemini` is. npm installs a `gemini.cmd` shim in %APPDATA%\npm, which a
/// GUI app launched at login may not have on its PATH.
fn command() -> Command {
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
        let mut cmd = match found {
            Some(path) => Command::new(path),
            None => {
                let mut c = Command::new("cmd.exe");
                c.args(["/D", "/C", "gemini.cmd"]);
                c
            }
        };
        crate::platform::no_console(&mut cmd);
        cmd
    }
    #[cfg(not(windows))]
    {
        Command::new("gemini")
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
    let mut child = command()
        .args(args(model, session))
        .current_dir(dir)
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .spawn()
        .map_err(|e| format!("Gemini CLI is not installed or not on PATH ({e}). Install it with `npm install -g @google/gemini-cli` and run `gemini` once to sign in with Google."))?;

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
    let turn = chat.begin(PROVIDER);
    let session = last_session(&turn.history);
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
    let (answer, new_session) = tokio::task::spawn_blocking(move || {
        run(&dir, &model, session.as_deref(), prompt, on_delta)
    })
    .await
    .map_err(|e| e.to_string())??;

    let plain = chat::plain_question(turn.first, context.as_ref(), &query);
    let mut assistant = json!({ "role": "assistant", "content": answer });
    if let Some(id) = new_session.filter(|s| valid_session(s)) {
        assistant["session_id"] = json!(id);
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
