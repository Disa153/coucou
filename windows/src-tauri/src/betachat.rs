// The beta chat: the owner and the beta testers, signed in with GitHub, talk
// in the island. The chat is one issue of Disa153/coucou ("Beta chat"), each
// message a comment of the account that wrote it — no server of our own. The
// repository is public, so the chat can be read on GitHub too; Coucou says so.
//
// Coucou reads it every 30 s while the chat is on screen (the island asks) and
// every 5 minutes otherwise, and only for an account that may see it
// (account::beta_allowed). A message from someone else that was not seen yet
// is told to the island ("beta-chat").

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::reviews::{call, ReviewError};

const ISSUES: &str = "https://api.github.com/repos/Disa153/coucou/issues";
pub const TITLE: &str = "Beta chat";
const MARK: &str = "<!-- coucou-chat:";
pub const TEXT_MAX: usize = 2000;
const SHOWN: u64 = 100;
const POLL_EVERY: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: u64,
    pub login: String,
    pub text: String,
    /// The build the writer ran, when they attached it.
    pub build: Option<u64>,
    /// Unix seconds.
    pub at: u64,
    pub owner: bool,
    pub mine: bool,
}

/// A message as a comment: a hidden line with the build when attached, then the text.
pub fn comment_body(text: &str, build: Option<u64>) -> String {
    let text: String = text.trim().chars().take(TEXT_MAX).collect();
    match build {
        Some(b) => format!("{MARK}{} -->\n🧪 build {b}\n\n{text}", json!({ "v": 1, "build": b })),
        None => text,
    }
}

/// The text and the build back from a comment (written by Coucou or on GitHub).
pub fn parse_body(body: &str) -> (String, Option<u64>) {
    let Some(start) = body.find(MARK) else { return (body.trim().to_string(), None) };
    let rest = &body[start + MARK.len()..];
    let Some(end) = rest.find("-->") else { return (body.trim().to_string(), None) };
    let build = serde_json::from_str::<Value>(rest[..end].trim()).ok().and_then(|v| v["build"].as_u64());
    let mut text = rest[end + 3..].trim_start();
    // The line for people on GitHub is not part of the message.
    if let Some(b) = build {
        if let Some(after) = text.strip_prefix(&format!("🧪 build {b}")) {
            text = after;
        }
    }
    (text.trim().to_string(), build)
}

// ── What was seen ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Seen {
    /// The last message id seen (GitHub's comment ids only grow).
    last: u64,
    /// The last one the island was told about, so it is told once.
    told: u64,
}

fn seen_path() -> PathBuf {
    crate::settings::local_dir().join("beta-chat.json")
}

static SEEN: Mutex<Option<Seen>> = Mutex::new(None);
static ISSUE: Mutex<Option<u64>> = Mutex::new(None);

fn with_seen<T>(f: impl FnOnce(&mut Seen) -> T) -> T {
    let mut guard = SEEN.lock().unwrap();
    let seen = guard.get_or_insert_with(|| {
        std::fs::read_to_string(seen_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    });
    let out = f(seen);
    if let Ok(text) = serde_json::to_string(&*seen) {
        let path = seen_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
    out
}

/// How many of `messages` are someone else's and newer than `last`.
pub fn unread(messages: &[Message], last: u64) -> usize {
    messages.iter().filter(|m| !m.mine && m.id > last).count()
}

// ── GitHub ────────────────────────────────────────────────────────────────────

fn allowed(app: &AppHandle) -> Result<String, ReviewError> {
    let token = crate::account::user_token().ok_or(ReviewError::NotSignedIn)?;
    if !crate::account::beta_allowed(app) {
        return Err(ReviewError::NotAllowed);
    }
    Ok(token)
}

/// The chat's issue: (number, comments). Opened if there is none yet.
async fn issue(token: &str, open: bool) -> Result<Option<(u64, u64)>, ReviewError> {
    let cached = *ISSUE.lock().unwrap();
    if let Some(n) = cached {
        let (status, body) = call(reqwest::Method::GET, &format!("{ISSUES}/{n}"), token, None).await?;
        if status == 200 {
            let v: Value = serde_json::from_slice(&body).map_err(|_| ReviewError::Network)?;
            return Ok(Some((n, v["comments"].as_u64().unwrap_or(0))));
        }
    }
    let (status, body) =
        call(reqwest::Method::GET, &format!("{ISSUES}?state=all&per_page=100&sort=created&direction=asc"), token, None).await?;
    if status != 200 {
        return Err(if status == 401 { ReviewError::SignInAgain } else { ReviewError::Network });
    }
    let v: Value = serde_json::from_slice(&body).map_err(|_| ReviewError::Network)?;
    let found = v.as_array().into_iter().flatten().find(|i| i.get("pull_request").is_none() && i["title"].as_str() == Some(TITLE));
    if let Some(i) = found {
        let n = i["number"].as_u64().ok_or(ReviewError::Network)?;
        *ISSUE.lock().unwrap() = Some(n);
        return Ok(Some((n, i["comments"].as_u64().unwrap_or(0))));
    }
    if !open {
        return Ok(None);
    }
    let body = json!({
        "title": TITLE,
        "body": "The beta testers' chat, written from Coucou (the 💬 button in the island). Each comment is a message. This repository is public: no passwords or keys here.",
    });
    let (status, created) = call(reqwest::Method::POST, ISSUES, token, Some(body)).await?;
    match status {
        201 => {}
        401 | 403 | 404 | 410 => return Err(ReviewError::SignInAgain),
        _ => return Err(ReviewError::Network),
    }
    let v: Value = serde_json::from_slice(&created).map_err(|_| ReviewError::Network)?;
    let n = v["number"].as_u64().ok_or(ReviewError::Network)?;
    *ISSUE.lock().unwrap() = Some(n);
    Ok(Some((n, 0)))
}

async fn fetch(app: &AppHandle, token: &str) -> Result<Vec<Message>, ReviewError> {
    let Some((n, count)) = issue(token, false).await? else { return Ok(Vec::new()) };
    let me = crate::account::login(app);
    // The last page holds the newest; the one before tops it up to SHOWN.
    let last_page = count.div_ceil(SHOWN).max(1);
    let mut pages = vec![last_page];
    if last_page > 1 && count % SHOWN != 0 {
        pages.insert(0, last_page - 1);
    }
    let mut out: Vec<Message> = Vec::new();
    for page in pages {
        let url = format!("{ISSUES}/{n}/comments?per_page={SHOWN}&page={page}");
        let (status, body) = call(reqwest::Method::GET, &url, token, None).await?;
        if status == 401 {
            return Err(ReviewError::SignInAgain);
        }
        if status != 200 {
            return Err(ReviewError::Network);
        }
        let v: Value = serde_json::from_slice(&body).map_err(|_| ReviewError::Network)?;
        for c in v.as_array().into_iter().flatten() {
            let (Some(id), Some(body)) = (c["id"].as_u64(), c["body"].as_str()) else { continue };
            let login = c["user"]["login"].as_str().unwrap_or("").to_string();
            let (text, build) = parse_body(body);
            out.push(Message {
                id,
                owner: login.eq_ignore_ascii_case(crate::account::OWNER),
                mine: !me.is_empty() && login.eq_ignore_ascii_case(&me),
                login,
                text,
                build,
                at: c["created_at"].as_str().and_then(crate::updater::unix_from_iso).unwrap_or(0),
            });
        }
    }
    out.sort_by_key(|m| m.id);
    out.dedup_by_key(|m| m.id);
    let skip = out.len().saturating_sub(SHOWN as usize);
    Ok(out.split_off(skip))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatNews {
    pub unread: usize,
    /// The newest unread message, for the island's line.
    pub last: Option<Message>,
}

fn news(messages: &[Message]) -> ChatNews {
    let last = with_seen(|s| s.last);
    ChatNews { unread: unread(messages, last), last: messages.iter().rev().find(|m| !m.mine && m.id > last).cloned() }
}

/// In the background: someone wrote, and the island was not told yet.
pub fn setup(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            if let Ok(token) = allowed(&app) {
                if let Ok(messages) = fetch(&app, &token).await {
                    let n = news(&messages);
                    let newest = n.last.as_ref().map(|m| m.id).unwrap_or(0);
                    let tell = with_seen(|s| {
                        let fresh = newest > s.told;
                        if fresh {
                            s.told = newest;
                        }
                        fresh
                    });
                    if tell {
                        let _ = app.emit("beta-chat", &n);
                    }
                }
            }
            tokio::time::sleep(POLL_EVERY).await;
        }
    });
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPage {
    pub messages: Vec<Message>,
    pub page: String,
    pub owner: bool,
}

/// The last messages (the chat on screen asks every 30 s); what is shown counts as read.
#[tauri::command]
pub async fn beta_chat_messages(app: AppHandle) -> Result<ChatPage, ReviewError> {
    let token = allowed(&app)?;
    let messages = fetch(&app, &token).await?;
    if let Some(last) = messages.last() {
        let id = last.id;
        with_seen(|s| {
            s.last = s.last.max(id);
            s.told = s.told.max(id);
        });
    }
    let page = ISSUE.lock().unwrap().map(|n| format!("https://github.com/Disa153/coucou/issues/{n}")).unwrap_or_default();
    Ok(ChatPage { messages, page, owner: crate::account::is_owner(&app) })
}

/// Unread, without marking anything (the header's badge).
#[tauri::command]
pub async fn beta_chat_unread(app: AppHandle) -> Result<ChatNews, ReviewError> {
    let token = allowed(&app)?;
    Ok(news(&fetch(&app, &token).await?))
}

#[tauri::command]
pub async fn beta_chat_send(app: AppHandle, text: String, with_build: bool) -> Result<(), ReviewError> {
    let token = allowed(&app)?;
    if text.trim().is_empty() {
        return Ok(());
    }
    let build = Some(crate::updater::build()).filter(|b| with_build && *b > 0);
    let (n, _) = issue(&token, true).await?.ok_or(ReviewError::Network)?;
    let body = json!({ "body": comment_body(&text, build) });
    let (status, _) = call(reqwest::Method::POST, &format!("{ISSUES}/{n}/comments"), &token, Some(body)).await?;
    match status {
        201 => Ok(()),
        401 | 403 | 404 => Err(ReviewError::SignInAgain),
        _ => Err(ReviewError::Network),
    }
}

/// One's own message; the owner may delete any (GitHub checks it either way).
#[tauri::command]
pub async fn beta_chat_delete(app: AppHandle, id: u64) -> Result<(), ReviewError> {
    let token = allowed(&app)?;
    let (status, _) = call(reqwest::Method::DELETE, &format!("{ISSUES}/comments/{id}"), &token, None).await?;
    match status {
        204 | 404 => Ok(()),
        401 | 403 => Err(ReviewError::NotAllowed),
        _ => Err(ReviewError::Network),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_comes_back_with_its_build() {
        let body = comment_body("  после 65 микшер не открывается  ", Some(65));
        assert!(body.contains("🧪 build 65"));
        assert_eq!(parse_body(&body), ("после 65 микшер не открывается".to_string(), Some(65)));
        assert_eq!(comment_body("hi", None), "hi");
        assert_eq!(parse_body("written on GitHub"), ("written on GitHub".to_string(), None));
        assert_eq!(parse_body(&format!("{MARK}junk -->\ntext")), ("text".to_string(), None));
        assert_eq!(comment_body(&"x".repeat(5000), None).len(), TEXT_MAX);
    }

    #[test]
    fn only_others_newer_messages_are_unread() {
        let m = |id, mine| Message { id, login: "a".into(), text: String::new(), build: None, at: 0, owner: false, mine };
        let list = [m(1, false), m(2, true), m(3, false), m(4, false)];
        assert_eq!(unread(&list, 0), 3);
        assert_eq!(unread(&list, 3), 1);
        assert_eq!(unread(&list, 4), 0);
    }
}
