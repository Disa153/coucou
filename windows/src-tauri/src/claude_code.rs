// The chat through the user's own Claude Code: each turn runs the installed
// `claude` CLI in print mode, so it answers on the Claude subscription the CLI
// is signed in with (Pro, Max) and no API key is needed.
//
// The question goes to `claude` on stdin, never on the command line, so nothing
// the user types is ever parsed by a shell. ANTHROPIC_API_KEY is removed from
// the child's environment: with it set, Claude Code would bill the API instead
// of the subscription. A conversation continues with `--resume <session id>`,
// the id kept next to the answer in the chat history.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::chat::{self, Chat, ChatContext, ChatReply, ModelInfo};
use crate::island::WINDOW_LABEL;
use crate::settings::Settings;

pub const PROVIDER: &str = "claude-code";
/// "default": no --model, so the chat uses the model Claude Code itself is set to
/// (the one `claude` shows in the terminal, e.g. Opus on Pro).
pub const DEFAULT_MODEL: &str = "default";
/// The model aliases Claude Code takes on `--model`; only these are ever passed.
const MODELS: [(&str, &str); 4] = [("default", "Claude Code default"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")];
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

/// A Claude Code session id is a UUID; anything else is never passed on.
pub(crate) fn valid_session(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The folder Claude Code (and Gemini CLI, gemini_cli.rs) works in: the one set in Settings → Chat, or home.
pub fn work_dir(settings: &Settings) -> PathBuf {
    let set = settings.claude_code_dir.trim();
    if !set.is_empty() {
        let p = PathBuf::from(set);
        if p.is_dir() {
            return p;
        }
    }
    crate::platform::home_dir()
}

/// The session to resume: the one the last answer came from, if this provider gave it.
pub(crate) fn last_session(history: &[Value]) -> Option<String> {
    history
        .iter()
        .rev()
        .find(|m| m["role"] == "assistant")
        .and_then(|m| m.get("session_id"))
        .and_then(Value::as_str)
        .filter(|s| valid_session(s))
        .map(str::to_string)
}

/// What goes on stdin. A fresh session after another provider answered carries
/// the earlier turns as a transcript, so the conversation still holds together.
pub(crate) fn prompt_text(
    first: bool,
    resuming: bool,
    history: &[Value],
    context: Option<&ChatContext>,
    query: &str,
) -> String {
    let mut out = String::new();
    if !resuming && !history.is_empty() {
        out.push_str("Earlier in this conversation:\n\n");
        for m in history {
            let who = if m["role"] == "user" {
                "User"
            } else {
                "Assistant"
            };
            let text = m["content"].as_str().unwrap_or_default();
            out.push_str(&format!("{who}: {text}\n\n"));
        }
        out.push_str("Now:\n\n");
    }
    match context.filter(|_| first) {
        Some(ChatContext::File { name, path }) => {
            out.push_str(&format!("The user dropped a file on you: {name} (at {path}). Read it if the question needs it.\n\n"));
        }
        Some(ChatContext::Window {
            app_name,
            title,
            url,
        }) => {
            out.push_str(&chat::window_line(app_name, title, url.as_deref()));
            out.push_str("\n\n");
        }
        None => {}
    }
    out.push_str(query);
    out
}

const STYLE: &str = "You are answering in Coucou, a small chat window at the top of the user's screen. \
Respond in the user's language. Keep answers compact; use light Markdown, no tables or big headings.";

/// Where `claude` is. The native installer puts `claude.exe` in ~/.local/bin, which a
/// GUI app launched at login may not have on its PATH; npm installs a `claude.cmd` shim.
fn command() -> Command {
    #[cfg(windows)]
    {
        let native = crate::platform::home_dir()
            .join(".local")
            .join("bin")
            .join("claude.exe");
        let mut cmd = if native.is_file() {
            Command::new(native)
        } else if let Some(exe) = on_path("claude.exe") {
            Command::new(exe)
        } else {
            // npm's claude.cmd needs cmd.exe; the arguments below are fixed strings,
            // a checked model alias and a checked UUID, so nothing typed reaches it.
            let mut c = Command::new("cmd.exe");
            c.args(["/D", "/C", "claude.cmd"]);
            c
        };
        crate::platform::no_console(&mut cmd);
        cmd
    }
    #[cfg(not(windows))]
    {
        let native = crate::platform::home_dir()
            .join(".local")
            .join("bin")
            .join("claude");
        Command::new(if native.is_file() {
            native
        } else {
            PathBuf::from("claude")
        })
    }
}

#[cfg(windows)]
fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

fn args(model: &str, session: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--append-system-prompt",
        STYLE,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if model != DEFAULT_MODEL {
        a.push("--model".into());
        a.push(model.to_string());
    }
    if let Some(id) = session {
        a.push("--resume".into());
        a.push(id.to_string());
    }
    a
}

enum Line {
    Delta(String),
    Done {
        text: String,
        session: Option<String>,
        error: bool,
    },
    Other,
}

/// One line of `--output-format stream-json`.
fn parse_line(line: &str) -> Line {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Line::Other;
    };
    match v["type"].as_str() {
        Some("stream_event") => {
            let ev = &v["event"];
            if ev["type"] == "content_block_delta" && ev["delta"]["type"] == "text_delta" {
                if let Some(t) = ev["delta"]["text"].as_str() {
                    return Line::Delta(t.to_string());
                }
            }
            Line::Other
        }
        Some("result") => Line::Done {
            text: v["result"].as_str().unwrap_or_default().trim().to_string(),
            session: v["session_id"].as_str().map(str::to_string),
            error: v["is_error"].as_bool().unwrap_or(false)
                || v["subtype"].as_str().is_some_and(|s| s != "success"),
        },
        _ => Line::Other,
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
    let mut child = command()
        .args(args(model, session))
        .current_dir(dir)
        .env_remove("ANTHROPIC_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Claude Code is not installed or not on PATH ({e}). Install it and run `claude` once to sign in."))?;

    let mut stdin = child.stdin.take().expect("piped stdin");
    std::thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });
    let mut stderr = child.stderr.take().expect("piped stderr");
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let started = Instant::now();
    let mut visible = String::new();
    let mut done: Option<(String, Option<String>, bool)> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => match parse_line(&line) {
                Line::Delta(t) => {
                    visible.push_str(&t);
                    on_delta(visible.clone());
                }
                Line::Done {
                    text,
                    session,
                    error,
                } => done = Some((text, session, error)),
                Line::Other => {}
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if started.elapsed() > TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Claude Code took more than 10 minutes and was stopped.".into());
                }
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let stderr = err_reader.join().unwrap_or_default();

    match done {
        Some((text, session, false)) if !text.is_empty() => Ok((text, session)),
        Some((text, _, _)) if !text.is_empty() => Err(text),
        _ if !visible.trim().is_empty() && status.success() => {
            Ok((visible.trim().to_string(), None))
        }
        _ => {
            let msg: String = stderr.trim().chars().take(MAX_ERROR).collect();
            Err(if msg.is_empty() {
                "Claude Code gave no answer. Run `claude` in a terminal once to check it is signed in.".into()
            } else {
                msg
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
    let prompt = prompt_text(
        turn.first,
        session.is_some(),
        &turn.history,
        context.as_ref(),
        &query,
    );
    let dir = work_dir(settings);
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
/// CLI on the same subscription, the streamed text going to the phone.
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

    #[test]
    fn only_known_models_and_uuids_reach_the_command_line() {
        assert_eq!(checked_model("opus"), "opus");
        assert_eq!(checked_model("x & del *"), DEFAULT_MODEL);
        assert!(!args(DEFAULT_MODEL, None).contains(&"--model".to_string()));
        assert!(args("opus", None).windows(2).any(|w| w == ["--model", "opus"]));
        assert!(valid_session("0b6f1f5e-2c1d-4a7b-9b3e-1f2a3b4c5d6e"));
        assert!(!valid_session("0b6f1f5e-2c1d-4a7b-9b3e-1f2a3b4c5d6e & x"));
        assert!(!valid_session(""));
        let a = args("opus", Some("0b6f1f5e-2c1d-4a7b-9b3e-1f2a3b4c5d6e"));
        assert!(a.ends_with(&[
            "--resume".to_string(),
            "0b6f1f5e-2c1d-4a7b-9b3e-1f2a3b4c5d6e".to_string()
        ]));
    }

    #[test]
    fn stream_lines_are_read() {
        let d = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}}"#;
        assert!(matches!(parse_line(d), Line::Delta(t) if t == "Hel"));
        let r = r#"{"type":"result","subtype":"success","is_error":false,"result":" Hello ","session_id":"0b6f1f5e-2c1d-4a7b-9b3e-1f2a3b4c5d6e"}"#;
        match parse_line(r) {
            Line::Done {
                text,
                session,
                error,
            } => {
                assert_eq!(text, "Hello");
                assert!(!error);
                assert_eq!(
                    session.as_deref(),
                    Some("0b6f1f5e-2c1d-4a7b-9b3e-1f2a3b4c5d6e")
                );
            }
            _ => panic!(),
        }
        assert!(matches!(parse_line("not json"), Line::Other));
    }

    #[test]
    fn a_conversation_resumes_its_own_session_only() {
        let h = vec![
            json!({"role":"user","content":"hi"}),
            json!({"role":"assistant","content":"yo","session_id":"0b6f1f5e-2c1d-4a7b-9b3e-1f2a3b4c5d6e"}),
        ];
        assert_eq!(
            last_session(&h).as_deref(),
            Some("0b6f1f5e-2c1d-4a7b-9b3e-1f2a3b4c5d6e")
        );
        let plain = vec![
            json!({"role":"user","content":"hi"}),
            json!({"role":"assistant","content":"yo"}),
        ];
        assert_eq!(last_session(&plain), None);
        let p = prompt_text(false, false, &plain, None, "next?");
        assert!(p.starts_with("Earlier in this conversation:"));
        assert!(p.contains("Assistant: yo") && p.ends_with("next?"));
        assert_eq!(prompt_text(false, true, &h, None, "next?"), "next?");
    }
}
