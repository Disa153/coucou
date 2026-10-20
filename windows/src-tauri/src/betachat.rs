// The beta chat: the owner and the beta testers, signed in with GitHub, talk
// in the island, in a window of its own, and on their phones through this
// computer (phone_link). The chat is one issue of Disa153/coucou ("Beta chat"),
// each message a comment of the account that wrote it — no server of our own.
// The repository is public, so the chat can be read on GitHub too; Coucou says so.
//
// Photos, videos and files go to a secret Gist of the sender (the sign-in may
// write Gists), base64 text that the readers fetch and turn back into the file;
// the message names them. A secret Gist is reachable by its address, which the
// public chat carries: attachments are as public as the messages.
//
// Coucou reads the chat every 10 s while it is on screen (only what changed)
// and every 5 minutes otherwise, and only for an account that may see it
// (account::beta_allowed). A message from someone else not seen yet is told to
// the island ("beta-chat"), which plays the sound picked (BetaChatPrefs).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::reviews::{call, ReviewError};

const ISSUES: &str = "https://api.github.com/repos/Disa153/coucou/issues";
pub const TITLE: &str = "Beta chat";
const MARK: &str = "<!-- coucou-chat:";
pub const TEXT_MAX: usize = 2000;
const SHOWN: u64 = 100;
const POLL_EVERY: Duration = Duration::from_secs(5 * 60);
/// One attachment, as sent (a raw Gist file above 10 MB cannot be read back).
pub const FILE_MAX: usize = 7 * 1024 * 1024;
pub const FILES_MAX: usize = 4;
const RAW_PREFIX: &str = "https://gist.githubusercontent.com/";
const SOUND_MAX: usize = 2 * 1024 * 1024;
pub const WINDOW_LABEL: &str = "betachat";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRef {
    pub name: String,
    pub mime: String,
    pub size: u64,
    /// The raw Gist file (base64 text of the file).
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: u64,
    pub login: String,
    pub text: String,
    /// The build the writer ran, when they attached it.
    pub build: Option<u64>,
    pub files: Vec<FileRef>,
    /// Unix seconds.
    pub at: u64,
    pub owner: bool,
    pub mine: bool,
    #[serde(skip)]
    pub updated: String,
}

fn clean_name(name: &str) -> String {
    let n: String = name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .take(80)
        .collect();
    let n = n.trim().trim_matches('.').to_string();
    if n.is_empty() { "file".into() } else { n }
}

/// A message as a comment: a hidden line with the build and the files, then
/// the same for people reading on GitHub.
pub fn comment_body(text: &str, build: Option<u64>, files: &[FileRef]) -> String {
    let text: String = text.trim().chars().take(TEXT_MAX).collect();
    if build.is_none() && files.is_empty() {
        return text;
    }
    let meta = json!({ "v": 2, "build": build, "files": files }).to_string().replace("--", "-\\u002d");
    let mut out = format!("{MARK}{meta} -->\n");
    if let Some(b) = build {
        out.push_str(&format!("🧪 build {b}\n\n"));
    }
    out.push_str(&text);
    if !files.is_empty() {
        let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
        out.push_str(&format!("\n\n📎 {}", names.join(" · ")));
    }
    out
}

/// The text, the build and the files back from a comment (written by Coucou or on GitHub).
pub fn parse_body(body: &str) -> (String, Option<u64>, Vec<FileRef>) {
    let plain = || (body.trim().to_string(), None, Vec::new());
    let Some(start) = body.find(MARK) else { return plain() };
    let rest = &body[start + MARK.len()..];
    let Some(end) = rest.find("-->") else { return plain() };
    let meta = serde_json::from_str::<Value>(rest[..end].trim()).unwrap_or(Value::Null);
    let build = meta["build"].as_u64();
    let files: Vec<FileRef> = meta["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| serde_json::from_value::<FileRef>(f.clone()).ok())
        .filter(|f| f.url.starts_with(RAW_PREFIX) && !f.url.contains(".."))
        .map(|f| FileRef { name: clean_name(&f.name), ..f })
        .take(FILES_MAX)
        .collect();
    let mut text = rest[end + 3..].trim_start();
    // The lines for people on GitHub are not part of the message.
    if let Some(b) = build {
        if let Some(after) = text.strip_prefix(&format!("🧪 build {b}")) {
            text = after;
        }
    }
    let mut text = text.trim().to_string();
    if !files.is_empty() {
        if let Some(at) = text.rfind("📎 ") {
            if !text[at..].contains('\n') {
                text.truncate(at);
            }
        }
    }
    (text.trim().to_string(), build, files)
}

// ── Kept on this computer ─────────────────────────────────────────────────────

/// The sound of a new message: one of Coucou's own, or the user's file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BetaChatPrefs {
    /// "hee", "chime", "quack", "knock", "mochi", "own" or "none".
    pub sound: String,
    /// 0…1.
    pub volume: f64,
    /// No sound while an agent works or music plays.
    pub quiet: bool,
    /// The user's own sound's file name, when there is one.
    pub own_name: String,
}

impl Default for BetaChatPrefs {
    fn default() -> Self {
        BetaChatPrefs { sound: "chime".into(), volume: 0.8, quiet: false, own_name: String::new() }
    }
}

const SOUNDS: &[&str] = &["hee", "chime", "quack", "knock", "mochi", "own", "none"];

pub fn clean_prefs(p: BetaChatPrefs) -> BetaChatPrefs {
    let mut p = p;
    if !SOUNDS.contains(&p.sound.as_str()) {
        p.sound = BetaChatPrefs::default().sound;
    }
    p.volume = if p.volume.is_finite() { p.volume.clamp(0.0, 1.0) } else { 0.8 };
    p.own_name = clean_name(&p.own_name);
    p
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Seen {
    /// The last message id seen (GitHub's comment ids only grow).
    last: u64,
    /// The last one the island was told about, so it is told once.
    told: u64,
    /// The chat's issue, once found.
    issue: u64,
    prefs: Option<BetaChatPrefs>,
}

fn seen_path() -> PathBuf {
    crate::settings::local_dir().join("beta-chat.json")
}

static SEEN: Mutex<Option<Seen>> = Mutex::new(None);

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

/// The messages known, kept between reads so a read asks only for what changed.
struct Cache {
    messages: Vec<Message>,
    /// GitHub's updated_at of the newest change seen.
    since: String,
    /// Reads since the last full one (a full read also sees deletions).
    reads: u32,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
/// Every this many reads, the whole page again.
const FULL_EVERY: u32 = 12;

/// `fresh` on top of `known`, by id, oldest first, at most SHOWN.
pub fn merge(known: &[Message], fresh: Vec<Message>) -> Vec<Message> {
    let mut out: Vec<Message> = known.iter().filter(|k| !fresh.iter().any(|f| f.id == k.id)).cloned().collect();
    out.extend(fresh);
    out.sort_by_key(|m| m.id);
    let skip = out.len().saturating_sub(SHOWN as usize);
    out.split_off(skip)
}

// ── GitHub ────────────────────────────────────────────────────────────────────

pub(crate) fn allowed(app: &AppHandle) -> Result<String, ReviewError> {
    let token = crate::account::user_token().ok_or(ReviewError::NotSignedIn)?;
    if !crate::account::beta_allowed(app) {
        return Err(ReviewError::NotAllowed);
    }
    Ok(token)
}

/// The chat's issue number. Opened if there is none yet and `open`.
async fn issue(token: &str, open: bool) -> Result<Option<u64>, ReviewError> {
    let kept = with_seen(|s| s.issue);
    if kept > 0 {
        return Ok(Some(kept));
    }
    let (status, body) =
        call(reqwest::Method::GET, &format!("{ISSUES}?state=all&per_page=100&sort=created&direction=asc"), token, None).await?;
    if status != 200 {
        return Err(if status == 401 { ReviewError::SignInAgain } else { ReviewError::Network });
    }
    let v: Value = serde_json::from_slice(&body).map_err(|_| ReviewError::Network)?;
    let found = v
        .as_array()
        .into_iter()
        .flatten()
        .find(|i| i.get("pull_request").is_none() && i["title"].as_str() == Some(TITLE))
        .and_then(|i| i["number"].as_u64());
    if let Some(n) = found {
        with_seen(|s| s.issue = n);
        return Ok(Some(n));
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
    with_seen(|s| s.issue = n);
    Ok(Some(n))
}

fn message_of(c: &Value, me: &str) -> Option<Message> {
    let id = c["id"].as_u64()?;
    let body = c["body"].as_str()?;
    let login = c["user"]["login"].as_str().unwrap_or("").to_string();
    let (text, build, files) = parse_body(body);
    Some(Message {
        id,
        owner: login.eq_ignore_ascii_case(crate::account::OWNER),
        mine: !me.is_empty() && login.eq_ignore_ascii_case(me),
        login,
        text,
        build,
        files,
        at: c["created_at"].as_str().and_then(crate::updater::unix_from_iso).unwrap_or(0),
        updated: c["updated_at"].as_str().unwrap_or("").to_string(),
    })
}

async fn comments(token: &str, url: &str, me: &str) -> Result<Vec<Message>, ReviewError> {
    let (status, body) = call(reqwest::Method::GET, url, token, None).await?;
    match status {
        200 => {}
        401 => return Err(ReviewError::SignInAgain),
        404 => {
            // The issue was deleted: look for it again next time.
            with_seen(|s| s.issue = 0);
            return Ok(Vec::new());
        }
        _ => return Err(ReviewError::Network),
    }
    let v: Value = serde_json::from_slice(&body).map_err(|_| ReviewError::Network)?;
    Ok(v.as_array().into_iter().flatten().filter_map(|c| message_of(c, me)).collect())
}

/// The last SHOWN messages: only what changed since the last read, or all of them now and then.
async fn fetch(app: &AppHandle, token: &str, full: bool) -> Result<Vec<Message>, ReviewError> {
    let Some(n) = issue(token, false).await? else { return Ok(Vec::new()) };
    let me = crate::account::login(app);
    let (known, since, reads) = {
        let c = CACHE.lock().unwrap();
        match c.as_ref() {
            Some(c) => (c.messages.clone(), c.since.clone(), c.reads),
            None => (Vec::new(), String::new(), 0),
        }
    };
    let full = full || since.is_empty() || reads >= FULL_EVERY;
    let messages = if full {
        // The issue's comment count says which page holds the newest.
        let (status, body) = call(reqwest::Method::GET, &format!("{ISSUES}/{n}"), token, None).await?;
        let count = if status == 200 {
            serde_json::from_slice::<Value>(&body).ok().and_then(|v| v["comments"].as_u64()).unwrap_or(0)
        } else {
            0
        };
        let last_page = count.div_ceil(SHOWN).max(1);
        let mut out = Vec::new();
        if last_page > 1 && count % SHOWN != 0 {
            out.extend(comments(token, &format!("{ISSUES}/{n}/comments?per_page={SHOWN}&page={}", last_page - 1), &me).await?);
        }
        out.extend(comments(token, &format!("{ISSUES}/{n}/comments?per_page={SHOWN}&page={last_page}"), &me).await?);
        merge(&[], out)
    } else {
        let mut url = reqwest::Url::parse(&format!("{ISSUES}/{n}/comments")).map_err(|_| ReviewError::Network)?;
        url.query_pairs_mut().append_pair("since", &since).append_pair("per_page", "100");
        merge(&known, comments(token, url.as_str(), &me).await?)
    };
    let newest = messages.iter().map(|m| m.updated.clone()).max().unwrap_or(since);
    *CACHE.lock().unwrap() = Some(Cache { messages: messages.clone(), since: newest, reads: if full { 0 } else { reads + 1 } });
    Ok(messages)
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

/// Someone wrote, and the island (and the phone) were not told yet.
fn tell_if_new(app: &AppHandle, messages: &[Message]) {
    let n = news(messages);
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
        crate::phone_link::beta_chat_news(app, &n);
    }
}

/// The app, for the phone's beta chat (phone_link/beta_chat.rs), which has no handle of its own.
static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

/// This computer's account may see the chat: a phone may then use it through here.
pub fn phone_allowed() -> bool {
    APP.get().is_some_and(|a| allowed(a).is_ok())
}

fn app() -> Result<&'static AppHandle, ReviewError> {
    APP.get().ok_or(ReviewError::NotSignedIn)
}

pub async fn phone_page(full: bool) -> Result<ChatPage, ReviewError> {
    page(app()?, full).await
}

pub async fn phone_send(text: &str, files: &[OutFile]) -> Result<Message, ReviewError> {
    send(app()?, text, false, files).await
}

pub async fn phone_delete(id: u64) -> Result<(), ReviewError> {
    delete(app()?, id).await
}

pub async fn phone_file(url: &str) -> Result<String, ReviewError> {
    file(app()?, url).await
}

/// In the background, every 5 minutes.
pub fn setup(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            if let Ok(token) = allowed(&app) {
                if let Ok(messages) = fetch(&app, &token, false).await {
                    tell_if_new(&app, &messages);
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
    pub build: u64,
}

/// The chat as it is (the chat on screen asks every 10 s); what is shown counts as read.
pub async fn page(app: &AppHandle, full: bool) -> Result<ChatPage, ReviewError> {
    let token = allowed(app)?;
    let messages = fetch(app, &token, full).await?;
    if let Some(last) = messages.last() {
        let id = last.id;
        with_seen(|s| {
            s.last = s.last.max(id);
            s.told = s.told.max(id);
        });
    }
    let n = with_seen(|s| s.issue);
    let page = if n > 0 { format!("https://github.com/Disa153/coucou/issues/{n}") } else { String::new() };
    Ok(ChatPage { messages, page, owner: crate::account::is_owner(app), build: crate::updater::build() })
}

#[tauri::command]
pub async fn beta_chat_messages(app: AppHandle, full: Option<bool>) -> Result<ChatPage, ReviewError> {
    page(&app, full.unwrap_or(false)).await
}

/// Unread, without marking anything (the header's badge).
#[tauri::command]
pub async fn beta_chat_unread(app: AppHandle) -> Result<ChatNews, ReviewError> {
    let token = allowed(&app)?;
    Ok(news(&fetch(&app, &token, false).await?))
}

/// A file to attach: its bytes as base64.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutFile {
    pub name: String,
    pub mime: String,
    pub data: String,
}

/// The decoded size of base64 text, without decoding it.
pub fn b64_len(data: &str) -> usize {
    let n = data.trim_end_matches('=').len();
    n * 3 / 4
}

fn valid_b64(data: &str) -> bool {
    !data.is_empty() && data.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

/// Puts one file in a new secret Gist of the sender; the reference to it.
async fn upload(token: &str, f: &OutFile) -> Result<FileRef, ReviewError> {
    let name = clean_name(&f.name);
    let size = b64_len(&f.data);
    if size > FILE_MAX || !valid_b64(&f.data) {
        return Err(ReviewError::TooBig);
    }
    let mime: String = f.mime.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '+')).take(80).collect();
    let file = format!("{name}.b64");
    let mut files = Map::new();
    files.insert(file.clone(), json!({ "content": f.data }));
    let body = json!({ "description": format!("Coucou beta chat: {name}"), "public": false, "files": files });
    let (status, created) = call(reqwest::Method::POST, "https://api.github.com/gists", token, Some(body)).await?;
    match status {
        201 => {}
        401 | 403 | 404 => return Err(ReviewError::SignInAgain),
        413 | 422 => return Err(ReviewError::TooBig),
        _ => return Err(ReviewError::Network),
    }
    let v: Value = serde_json::from_slice(&created).map_err(|_| ReviewError::Network)?;
    let url = v["files"][&file]["raw_url"].as_str().filter(|u| u.starts_with(RAW_PREFIX)).ok_or(ReviewError::Network)?;
    Ok(FileRef { name, mime: if mime.is_empty() { "application/octet-stream".into() } else { mime }, size: size as u64, url: url.to_string() })
}

/// Sends a message (with its files, if any) and returns it as GitHub has it.
pub async fn send(app: &AppHandle, text: &str, with_build: bool, files: &[OutFile]) -> Result<Message, ReviewError> {
    let token = allowed(app)?;
    if text.trim().is_empty() && files.is_empty() {
        return Err(ReviewError::Network);
    }
    if files.len() > FILES_MAX {
        return Err(ReviewError::TooBig);
    }
    let mut refs = Vec::new();
    for f in files {
        refs.push(upload(&token, f).await?);
    }
    let build = Some(crate::updater::build()).filter(|b| with_build && *b > 0);
    let n = issue(&token, true).await?.ok_or(ReviewError::Network)?;
    let body = json!({ "body": comment_body(text, build, &refs) });
    let (status, created) = call(reqwest::Method::POST, &format!("{ISSUES}/{n}/comments"), &token, Some(body)).await?;
    match status {
        201 => {}
        401 | 403 | 404 => return Err(ReviewError::SignInAgain),
        _ => return Err(ReviewError::Network),
    }
    let v: Value = serde_json::from_slice(&created).map_err(|_| ReviewError::Network)?;
    let message = message_of(&v, &crate::account::login(app)).ok_or(ReviewError::Network)?;
    {
        let mut c = CACHE.lock().unwrap();
        if let Some(c) = c.as_mut() {
            c.messages = merge(&c.messages, vec![message.clone()]);
        }
    }
    with_seen(|s| {
        s.last = s.last.max(message.id);
        s.told = s.told.max(message.id);
    });
    // The other windows and the phone see it at once.
    let _ = app.emit("beta-chat-changed", ());
    crate::phone_link::beta_chat_changed(app);
    Ok(message)
}

#[tauri::command]
pub async fn beta_chat_send(app: AppHandle, text: String, with_build: bool, files: Option<Vec<OutFile>>) -> Result<Message, ReviewError> {
    send(&app, &text, with_build, &files.unwrap_or_default()).await
}

/// One's own message; the owner may delete any (GitHub checks it either way).
pub async fn delete(app: &AppHandle, id: u64) -> Result<(), ReviewError> {
    let token = allowed(app)?;
    let (status, _) = call(reqwest::Method::DELETE, &format!("{ISSUES}/comments/{id}"), &token, None).await?;
    match status {
        204 | 404 => {}
        401 | 403 => return Err(ReviewError::NotAllowed),
        _ => return Err(ReviewError::Network),
    }
    if let Some(c) = CACHE.lock().unwrap().as_mut() {
        c.messages.retain(|m| m.id != id);
    }
    let _ = app.emit("beta-chat-changed", ());
    crate::phone_link::beta_chat_changed(app);
    Ok(())
}

#[tauri::command]
pub async fn beta_chat_delete(app: AppHandle, id: u64) -> Result<(), ReviewError> {
    delete(&app, id).await
}

/// An attachment's bytes, as base64 (the Gist holds them so).
pub async fn file(app: &AppHandle, url: &str) -> Result<String, ReviewError> {
    if !url.starts_with(RAW_PREFIX) || url.contains("..") {
        return Err(ReviewError::NotAllowed);
    }
    let token = allowed(app)?;
    let (status, body) = call(reqwest::Method::GET, url, &token, None).await?;
    if status != 200 {
        return Err(ReviewError::Network);
    }
    let text: String = String::from_utf8_lossy(&body).chars().filter(|c| !c.is_whitespace()).collect();
    if !valid_b64(&text) {
        return Err(ReviewError::Network);
    }
    Ok(text)
}

#[tauri::command]
pub async fn beta_chat_file(app: AppHandle, url: String) -> Result<String, ReviewError> {
    file(&app, &url).await
}

/// Saves an attachment in Downloads; the path it went to.
#[tauri::command]
pub async fn beta_chat_save(app: AppHandle, url: String, name: String) -> Result<String, ReviewError> {
    let data = file(&app, &url).await?;
    let bytes = crate::recap::decode_base64(&data).ok_or(ReviewError::Network)?;
    let dir = crate::platform::downloads_dir();
    let _ = std::fs::create_dir_all(&dir);
    let name = clean_name(&name);
    let mut path = dir.join(&name);
    let mut n = 1;
    while path.exists() && n < 100 {
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (name.clone(), String::new()),
        };
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    std::fs::write(&path, bytes).map_err(|_| ReviewError::Network)?;
    Ok(path.to_string_lossy().to_string())
}

// ── The sound and the window ─────────────────────────────────────────────────

#[tauri::command]
pub fn beta_chat_prefs() -> BetaChatPrefs {
    with_seen(|s| s.prefs.clone().map(clean_prefs).unwrap_or_default())
}

#[tauri::command]
pub fn beta_chat_set_prefs(app: AppHandle, prefs: BetaChatPrefs) -> BetaChatPrefs {
    let mut prefs = clean_prefs(prefs);
    // The file's name is Rust's to say (beta_chat_set_own_sound).
    prefs.own_name = with_seen(|s| s.prefs.as_ref().map(|p| p.own_name.clone()).unwrap_or_default());
    with_seen(|s| s.prefs = Some(prefs.clone()));
    let _ = app.emit("beta-chat-prefs", &prefs);
    prefs
}

fn own_sound_path() -> PathBuf {
    crate::settings::local_dir().join("beta-chat-sound")
}

/// The user's own sound (a few seconds of WAV, MP3 or OGG), kept on this computer.
#[tauri::command]
pub fn beta_chat_set_own_sound(app: AppHandle, name: String, data: String) -> Result<BetaChatPrefs, ReviewError> {
    if b64_len(&data) > SOUND_MAX || !valid_b64(&data) {
        return Err(ReviewError::TooBig);
    }
    let bytes = crate::recap::decode_base64(&data).ok_or(ReviewError::Network)?;
    std::fs::write(own_sound_path(), bytes).map_err(|_| ReviewError::Network)?;
    let prefs = with_seen(|s| {
        let mut p = s.prefs.clone().map(clean_prefs).unwrap_or_default();
        p.own_name = clean_name(&name);
        p.sound = "own".into();
        s.prefs = Some(p.clone());
        p
    });
    let _ = app.emit("beta-chat-prefs", &prefs);
    Ok(prefs)
}

#[tauri::command]
pub fn beta_chat_own_sound() -> Option<String> {
    let bytes = std::fs::read(own_sound_path()).ok()?;
    Some(crate::claude::base64_for(&bytes))
}

fn window_url(app: &AppHandle) -> WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path("/chat.html");
        return WebviewUrl::External(base);
    }
    let _ = app;
    WebviewUrl::App("chat.html".into())
}

/// The chat's own window, made hidden at launch (a WebView2 window made later
/// comes up blank in this app, see create_settings_window) and only shown and
/// hidden after.
pub fn create_window(app: &AppHandle) {
    match WebviewWindowBuilder::new(app, WINDOW_LABEL, window_url(app))
        .additional_browser_args(crate::BROWSER_ARGS)
        .title("Coucou — Beta chat")
        .inner_size(420.0, 600.0)
        .min_inner_size(320.0, 380.0)
        .resizable(true)
        .visible(false)
        .center()
        // Files dropped on it go to the page, which attaches them.
        .disable_drag_drop_handler()
        .build()
    {
        Ok(win) => {
            let hidden = win.clone();
            win.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = hidden.hide();
                }
            });
        }
        Err(err) => crate::log::line(format!("beta chat window failed: {err}")),
    }
}

#[tauri::command]
pub fn beta_chat_window(app: AppHandle) {
    if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        let _ = win.emit("beta-chat-shown", ());
    }
}

#[tauri::command]
pub fn beta_chat_pin(app: AppHandle, on: bool) {
    if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
        let _ = win.set_always_on_top(on);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> FileRef {
        FileRef { name: name.into(), mime: "image/jpeg".into(), size: 1000, url: format!("{RAW_PREFIX}u/abc/raw/x/{name}.b64") }
    }

    #[test]
    fn a_message_comes_back_with_its_build_and_files() {
        let body = comment_body("  после 65 микшер не открывается  ", Some(65), &[]);
        assert!(body.contains("🧪 build 65"));
        assert_eq!(parse_body(&body), ("после 65 микшер не открывается".to_string(), Some(65), vec![]));
        assert_eq!(comment_body("hi", None, &[]), "hi");
        assert_eq!(parse_body("written on GitHub"), ("written on GitHub".to_string(), None, vec![]));
        assert_eq!(parse_body(&format!("{MARK}junk -->\ntext")).0, "text");
        assert_eq!(comment_body(&"x".repeat(5000), None, &[]).len(), TEXT_MAX);

        let files = vec![file("screen.jpg"), file("clip.mp4")];
        let body = comment_body("look -- here", None, &files);
        assert!(body.contains("📎 screen.jpg · clip.mp4"));
        assert_eq!(body.lines().next().unwrap().matches("-->").count(), 1);
        assert_eq!(parse_body(&body), ("look -- here".to_string(), None, files.clone()));
        // a photo alone
        assert_eq!(parse_body(&comment_body("", None, &files[..1])), (String::new(), None, files[..1].to_vec()));
    }

    #[test]
    fn a_file_is_only_ever_read_from_a_gist() {
        let mut evil = file("x");
        evil.url = "https://example.com/x".into();
        let body = comment_body("t", None, &[evil, file("ok.png")]);
        let (_, _, files) = parse_body(&body);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "ok.png");
        assert_eq!(clean_name("../../a<b>.png"), "ab.png");
        assert_eq!(clean_name("..."), "file");
    }

    #[test]
    fn only_others_newer_messages_are_unread() {
        let m = |id, mine| Message { id, login: "a".into(), text: String::new(), build: None, files: vec![], at: 0, owner: false, mine, updated: String::new() };
        let list = [m(1, false), m(2, true), m(3, false), m(4, false)];
        assert_eq!(unread(&list, 0), 3);
        assert_eq!(unread(&list, 3), 1);
        assert_eq!(unread(&list, 4), 0);
        // what changed replaces what was known, in order
        let mut edited = m(3, false);
        edited.text = "edited".into();
        let merged = merge(&list, vec![m(5, true), edited]);
        assert_eq!(merged.iter().map(|x| x.id).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
        assert_eq!(merged[2].text, "edited");
    }

    #[test]
    fn sizes_and_prefs_are_kept_in_bounds() {
        assert_eq!(b64_len("AAAA"), 3);
        assert_eq!(b64_len("AAA="), 2);
        assert!(!valid_b64("not base64!"));
        let p = clean_prefs(BetaChatPrefs { sound: "thriller".into(), volume: 7.0, quiet: true, own_name: "a/b.mp3".into() });
        assert_eq!(p.sound, "chime");
        assert_eq!(p.volume, 1.0);
        assert_eq!(p.own_name, "ab.mp3");
    }
}
