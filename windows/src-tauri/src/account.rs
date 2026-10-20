// Accounts (Settings → Account): sign in with GitHub, keep one's saves in a
// secret Gist of one's own, and the beta testers the owner invites.
//
//   * Sign-in is GitHub's device flow: Coucou shows a short code, the user types
//     it on github.com and approves. The only scope asked is `gist`; the token
//     lives in the system's credential store (secrets.rs, "account-token") and
//     never reaches a webview. Coucou knows the login and the display name.
//   * Saves: the settings the user picks (by part, see PARTS) go to a secret Gist
//     of theirs, a while after each change and on "Save now"; "Restore" brings
//     them back on another computer, only when the user clicks. Keys and tokens
//     are not settings and never go; nor does anything tied to this computer
//     (folders, local servers, the phone link, where the island sits).
//   * Beta testers: `testers.json` on the repository's `beta-access` branch. The
//     owner edits it here with the release token (updater.rs), which then also
//     needs Contents: Read and write. A signed-in tester is offered the beta
//     channel without the password; one taken off the list goes back to stable.
//     The list only decides what Coucou offers: beta installers are public
//     release files, as before.
//
// Nothing here touches the network until the user signs in.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager};

/// The repository's owner: the only one who sees and edits the testers.
pub const OWNER: &str = "Disa153";
/// The owner's "Coucou Mochi" GitHub App (Device Flow on; Gists and this
/// repository's Issues only). Not a secret: GitHub shows it on every sign-in
/// page. Empty: this build has no sign-in.
const CLIENT_ID: &str = "Iv23lic3qaGqtxUYJQK1";
pub const TOKEN_KEY: &str = "account-token";
const TESTERS_RAW: &str = "https://raw.githubusercontent.com/Disa153/coucou/beta-access/testers.json";
const TESTERS_API: &str = "https://api.github.com/repos/Disa153/coucou/contents/testers.json";
const TESTERS_BRANCH: &str = "beta-access";
const GIST_FILE: &str = "coucou-sync.json";
const GIST_DESCRIPTION: &str = "Coucou saves (settings synced by Coucou)";
/// After a change, the saves go up once things have been quiet this long.
const UPLOAD_AFTER: Duration = Duration::from_secs(20);
const REFRESH_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const LIMIT: usize = 512 * 1024;

fn client_id() -> Option<&'static str> {
    option_env!("COUCOU_GITHUB_CLIENT_ID")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(Some(CLIENT_ID).filter(|s| !s.is_empty()))
}

// ── What is saved, by part ────────────────────────────────────────────────────

/// The settings each part carries, by their names in settings.json. Anything
/// not listed stays on this computer.
pub const PARTS: &[(&str, &[&str])] = &[
    (
        "general",
        &[
            "autoCloseInterval", "openOnHover", "absenceInterval", "activeIntegrations", "mainPill", "screen",
            "model", "showPlanInNotch", "showCodexPlanInNotch", "chatProvider", "chatModels", "saveChats",
            "shortcuts", "pillColors", "language", "lyrics",
        ],
    ),
    ("mochi", &["pet", "mochiBreed"]),
    ("outfits", &["mochiOutfit"]),
    ("glowSound", &["glow", "soundEnabled", "soundVolume"]),
];

fn all_parts() -> Vec<String> {
    PARTS.iter().map(|(p, _)| p.to_string()).collect()
}

/// The parts picked, as { part: { setting: value } }.
pub fn snapshot(settings: &Value, parts: &[String]) -> Value {
    let mut out = Map::new();
    for (part, keys) in PARTS {
        if !parts.iter().any(|p| p == part) {
            continue;
        }
        let mut values = Map::new();
        for key in *keys {
            if let Some(v) = settings.get(*key) {
                values.insert(key.to_string(), v.clone());
            }
        }
        out.insert(part.to_string(), Value::Object(values));
    }
    Value::Object(out)
}

/// Puts the saved parts picked back into `settings`; only keys a part owns.
pub fn apply(settings: &mut Value, saved: &Value, parts: &[String]) {
    let Some(target) = settings.as_object_mut() else { return };
    for (part, keys) in PARTS {
        if !parts.iter().any(|p| p == part) {
            continue;
        }
        let Some(values) = saved.get(*part).and_then(Value::as_object) else { continue };
        for key in *keys {
            if let Some(v) = values.get(*key) {
                target.insert(key.to_string(), v.clone());
            }
        }
    }
}

/// The parts a saved file holds.
pub fn saved_parts(saved: &Value) -> Vec<String> {
    PARTS
        .iter()
        .filter(|(p, _)| saved.get(*p).is_some_and(Value::is_object))
        .map(|(p, _)| p.to_string())
        .collect()
}

// ── Testers ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub login: String,
    #[serde(default)]
    pub name: String,
}

/// testers.json as written by hand or by Coucou: { "testers": ["login" | {login, name}] }.
pub fn parse_testers(text: &str) -> Vec<Person> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let mut out: Vec<Person> = Vec::new();
    for item in v.get("testers").and_then(Value::as_array).into_iter().flatten() {
        let person = match item {
            Value::String(login) => Person { login: login.clone(), name: String::new() },
            Value::Object(o) => Person {
                login: o.get("login").and_then(Value::as_str).unwrap_or("").to_string(),
                name: o.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            },
            _ => continue,
        };
        let login = person.login.trim();
        if valid_login(login) && !out.iter().any(|p| p.login.eq_ignore_ascii_case(login)) {
            out.push(Person { login: login.to_string(), name: person.name.trim().chars().take(80).collect() });
        }
    }
    out
}

/// When Coucou asks for a review of the build running (reviews.rs), as the owner set it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Survey {
    /// Days after the build was first started.
    pub days: u32,
    /// "testers" (and the owner) or "everyone" signed in.
    pub who: String,
}

impl Default for Survey {
    fn default() -> Self {
        Survey { days: 7, who: "testers".into() }
    }
}

/// The survey part of testers.json; anything odd reads as the default.
pub fn parse_survey(text: &str) -> Survey {
    let v = serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
    let d = Survey::default();
    let days = v["survey"]["days"].as_u64().filter(|n| (1..=60).contains(n)).map(|n| n as u32).unwrap_or(d.days);
    let who = match v["survey"]["who"].as_str() {
        Some("everyone") => "everyone",
        _ => "testers",
    };
    Survey { days, who: who.into() }
}

pub fn testers_json(testers: &[Person], survey: &Survey) -> String {
    let mut text = serde_json::to_string_pretty(&json!({ "testers": testers, "survey": survey })).unwrap_or_default();
    text.push('\n');
    text
}

/// A GitHub login: letters, digits and single hyphens, 1–39 characters.
pub fn valid_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= 39
        && login.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !login.starts_with('-')
        && !login.ends_with('-')
        && !login.contains("--")
}

pub fn is_tester(testers: &[Person], login: &str) -> bool {
    testers.iter().any(|p| p.login.eq_ignore_ascii_case(login))
}

// ── Kept on this computer ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Stored {
    /// The app the sign-in was made with; another one (a new build) means signing in again.
    client: String,
    login: String,
    name: String,
    gist: String,
    sync: bool,
    parts: Vec<String>,
    /// Unix ms of the last save that went up.
    last_sync: u64,
}

impl Default for Stored {
    fn default() -> Self {
        Stored { client: String::new(), login: String::new(), name: String::new(), gist: String::new(), sync: true, parts: all_parts(), last_sync: 0 }
    }
}

fn stored_path() -> PathBuf {
    crate::settings::local_dir().join("account.json")
}

fn load_stored() -> Option<Stored> {
    let text = std::fs::read_to_string(stored_path()).ok()?;
    serde_json::from_str::<Stored>(&text)
        .ok()
        .filter(|s| valid_login(&s.login))
        // A sign-in made with an earlier app cannot do what this build asks (reviews): sign in again.
        .filter(|s| client_id().is_some_and(|id| s.client == id))
}

fn write_stored(s: Option<&Stored>) {
    let path = stored_path();
    match s {
        Some(s) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(text) = serde_json::to_string_pretty(s) {
                let _ = std::fs::write(path, text);
            }
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct Account {
    me: Mutex<Option<Stored>>,
    /// The list last read; None until it was.
    testers: Mutex<Option<Vec<Person>>>,
    survey: Mutex<Survey>,
    cancel: AtomicBool,
    /// Bumped on every settings change; an upload only goes if it is still the last one.
    changes: AtomicU64,
    pending: Mutex<Option<Value>>,
}

/// Kept by the builder (lib.rs) before any window exists, so a page asking
/// early always finds it; the fallback only guards against that ever changing.
fn state(app: &AppHandle) -> &Account {
    static FALLBACK: std::sync::LazyLock<Account> = std::sync::LazyLock::new(Account::default);
    match app.try_state::<Account>() {
        Some(s) => s.inner(),
        None => &FALLBACK,
    }
}

impl Account {
    /// The account as this computer last knew it.
    pub fn loaded() -> Account {
        Account { me: Mutex::new(load_stored()), ..Default::default() }
    }
}

/// Why something did not happen; the page words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountError {
    /// This build has no OAuth app to sign in with.
    NotConfigured,
    /// The user said no on GitHub, or cancelled here.
    Denied,
    /// The code ran out before it was typed.
    Expired,
    Network,
    NotSignedIn,
    /// GitHub no longer takes the sign-in (revoked on github.com): sign in again.
    SignedOut,
    NotOwner,
    /// The release token is missing (Settings → Updates).
    NoReleaseToken,
    /// GitHub turned the release token down (expired, deleted or revoked).
    ReleaseTokenRefused,
    /// The release token cannot write the repository's files.
    NeedsContents,
    NoSaves,
    BadLogin,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountState {
    pub configured: bool,
    pub signed_in: bool,
    pub login: String,
    pub name: String,
    pub owner: bool,
    pub tester: bool,
    pub sync: bool,
    pub parts: Vec<String>,
    pub last_sync: u64,
    pub survey: Survey,
}

fn account_state_of(app: &AppHandle) -> AccountState {
    let a = state(app);
    let me = a.me.lock().unwrap().clone();
    let testers = a.testers.lock().unwrap().clone().unwrap_or_default();
    let survey = a.survey.lock().unwrap().clone();
    match me {
        Some(me) => AccountState {
            configured: client_id().is_some(),
            signed_in: true,
            owner: me.login.eq_ignore_ascii_case(OWNER),
            tester: is_tester(&testers, &me.login),
            login: me.login,
            name: me.name,
            sync: me.sync,
            parts: me.parts,
            last_sync: me.last_sync,
            survey,
        },
        None => AccountState {
            configured: client_id().is_some(),
            signed_in: false,
            login: String::new(),
            name: String::new(),
            owner: false,
            tester: false,
            sync: false,
            parts: all_parts(),
            last_sync: 0,
            survey,
        },
    }
}

/// The signed-in user is the owner, or on the testers list: the beta channel is theirs.
pub fn beta_allowed(app: &AppHandle) -> bool {
    let s = account_state_of(app);
    s.signed_in && (s.owner || s.tester)
}

pub fn is_owner(app: &AppHandle) -> bool {
    account_state_of(app).owner
}

fn changed(app: &AppHandle) {
    crate::updater::account_changed(app);
    let _ = app.emit("account-changed", account_state_of(app));
}

pub fn setup(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            if state(&app).me.lock().unwrap().is_some() {
                refresh_testers(&app).await;
            }
            tokio::time::sleep(REFRESH_EVERY).await;
        }
    });
}

// ── GitHub ────────────────────────────────────────────────────────────────────

async fn call(method: reqwest::Method, url: &str, token: Option<&str>, body: Option<Value>) -> Result<(u16, Vec<u8>), AccountError> {
    let url = reqwest::Url::parse(url).map_err(|_| AccountError::Network)?;
    let client = crate::updater::client(Duration::from_secs(30)).map_err(|_| AccountError::Network)?;
    let mut req = client
        .request(method, url)
        .header("User-Agent", "Coucou")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    if let Some(body) = body {
        req = req.json(&body);
    }
    let response = req.send().await.map_err(|_| AccountError::Network)?;
    let status = response.status().as_u16();
    let bytes = crate::net::read_capped(response, LIMIT).await.map_err(|_| AccountError::Network)?;
    Ok((status, bytes))
}

async fn form(url: &str, fields: &[(&str, &str)]) -> Result<Value, AccountError> {
    let client = crate::updater::client(Duration::from_secs(30)).map_err(|_| AccountError::Network)?;
    let response = client
        .post(url)
        .header("User-Agent", "Coucou")
        .header("Accept", "application/json")
        .form(fields)
        .send()
        .await
        .map_err(|_| AccountError::Network)?;
    let bytes = crate::net::read_capped(response, 64 * 1024).await.map_err(|_| AccountError::Network)?;
    serde_json::from_slice(&bytes).map_err(|_| AccountError::Network)
}

fn token() -> Result<String, AccountError> {
    crate::secrets::get_internal(TOKEN_KEY).ok_or(AccountError::NotSignedIn)
}

/// The token was turned down: forget the account, as if signed out.
fn drop_account(app: &AppHandle) {
    let _ = crate::secrets::set_internal(TOKEN_KEY, "");
    *state(app).me.lock().unwrap() = None;
    write_stored(None);
    crate::log::line("account: GitHub no longer accepts the sign-in; signed out");
    changed(app);
}

// ── Signing in ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
}

struct Flow {
    device_code: String,
    interval: u64,
    expires_in: u64,
}

static FLOW: Mutex<Option<Flow>> = Mutex::new(None);

#[tauri::command]
pub fn account_state(app: AppHandle) -> AccountState {
    account_state_of(&app)
}

/// Asks GitHub for a code and opens its page. Then `account_finish` waits.
#[tauri::command]
pub async fn account_begin() -> Result<DeviceCode, AccountError> {
    let id = client_id().ok_or(AccountError::NotConfigured)?;
    let v = form("https://github.com/login/device/code", &[("client_id", id), ("scope", "gist")]).await?;
    let (Some(device_code), Some(user_code), Some(uri)) =
        (v["device_code"].as_str(), v["user_code"].as_str(), v["verification_uri"].as_str())
    else {
        crate::log::line(format!("account: GitHub gave no code ({})", v["error"].as_str().unwrap_or("?")));
        return Err(AccountError::NotConfigured);
    };
    // Only GitHub's own page is opened.
    let uri = if uri.starts_with("https://github.com/") { uri.to_string() } else { "https://github.com/login/device".to_string() };
    *FLOW.lock().unwrap() = Some(Flow {
        device_code: device_code.to_string(),
        interval: v["interval"].as_u64().unwrap_or(5).clamp(1, 60),
        expires_in: v["expires_in"].as_u64().unwrap_or(900).min(1800),
    });
    crate::platform::open_url(&uri);
    Ok(DeviceCode { user_code: user_code.to_string(), verification_uri: uri })
}

/// Waits until the code is approved on GitHub (or refused, cancelled, or out of time).
#[tauri::command]
pub async fn account_finish(app: AppHandle) -> Result<AccountState, AccountError> {
    let id = client_id().ok_or(AccountError::NotConfigured)?;
    let flow = FLOW.lock().unwrap().take().ok_or(AccountError::Expired)?;
    let a = state(&app);
    a.cancel.store(false, Ordering::SeqCst);
    let token = poll(id, &flow, a).await?;
    let (status, body) = call(reqwest::Method::GET, "https://api.github.com/user", Some(&token), None).await?;
    if status != 200 {
        return Err(AccountError::Network);
    }
    let user: Value = serde_json::from_slice(&body).map_err(|_| AccountError::Network)?;
    let login = user["login"].as_str().unwrap_or("").to_string();
    if !valid_login(&login) {
        return Err(AccountError::Network);
    }
    crate::secrets::set_internal(TOKEN_KEY, &token).map_err(|_| AccountError::Network)?;
    let me = Stored { client: id.to_string(), login: login.clone(), name: user["name"].as_str().unwrap_or("").to_string(), ..Default::default() };
    write_stored(Some(&me));
    *a.me.lock().unwrap() = Some(me);
    crate::log::line("account: signed in with GitHub");
    refresh_testers(&app).await;
    Ok(account_state_of(&app))
}

async fn poll(id: &str, flow: &Flow, a: &Account) -> Result<String, AccountError> {
    let mut interval = flow.interval;
    let deadline = std::time::Instant::now() + Duration::from_secs(flow.expires_in);
    loop {
        for _ in 0..interval * 4 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if a.cancel.load(Ordering::SeqCst) {
                return Err(AccountError::Denied);
            }
        }
        if std::time::Instant::now() > deadline {
            return Err(AccountError::Expired);
        }
        let v = match form(
            "https://github.com/login/oauth/access_token",
            &[
                ("client_id", id),
                ("device_code", flow.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ],
        )
        .await
        {
            Ok(v) => v,
            // A dropped connection is not a refusal: ask again.
            Err(_) => continue,
        };
        if let Some(token) = v["access_token"].as_str().filter(|t| !t.is_empty()) {
            return Ok(token.to_string());
        }
        match v["error"].as_str().unwrap_or("") {
            "authorization_pending" => {}
            "slow_down" => interval = v["interval"].as_u64().unwrap_or(interval + 5).clamp(1, 60),
            "expired_token" => return Err(AccountError::Expired),
            "access_denied" => return Err(AccountError::Denied),
            other => {
                crate::log::line(format!("account: sign-in refused by GitHub ({other})"));
                return Err(AccountError::NotConfigured);
            }
        }
    }
}

#[tauri::command]
pub fn account_cancel(app: AppHandle) {
    state(&app).cancel.store(true, Ordering::SeqCst);
    FLOW.lock().unwrap().take();
}

#[tauri::command]
pub fn account_sign_out(app: AppHandle) -> AccountState {
    let _ = crate::secrets::set_internal(TOKEN_KEY, "");
    *state(&app).me.lock().unwrap() = None;
    *state(&app).testers.lock().unwrap() = None;
    write_stored(None);
    crate::log::line("account: signed out");
    changed(&app);
    account_state_of(&app)
}

// ── Testers: reading the list ─────────────────────────────────────────────────

/// Reads the list everybody sees (the published file) and acts on it.
async fn refresh_testers(app: &AppHandle) {
    let read = match call(reqwest::Method::GET, TESTERS_RAW, None, None).await {
        Ok((200, body)) => {
            let text = String::from_utf8_lossy(&body).to_string();
            Some((parse_testers(&text), parse_survey(&text)))
        }
        Ok((404, _)) => Some((Vec::new(), Survey::default())),
        _ => None,
    };
    // Unreachable: keep what was known, so a bad connection takes nobody's beta away.
    if let Some((list, survey)) = read {
        *state(app).testers.lock().unwrap() = Some(list);
        *state(app).survey.lock().unwrap() = survey;
    }
    changed(app);
}

/// The owner's view of the list: the file as it is now on the branch, with its version.
async fn read_list(token: &str) -> Result<(Vec<Person>, Survey, Option<String>), AccountError> {
    let (status, body) =
        call(reqwest::Method::GET, &format!("{TESTERS_API}?ref={TESTERS_BRANCH}"), Some(token), None).await?;
    match status {
        200 => {}
        404 => return Ok((Vec::new(), Survey::default(), None)),
        401 => return Err(AccountError::ReleaseTokenRefused),
        403 => return Err(AccountError::NeedsContents),
        _ => return Err(AccountError::Network),
    }
    let v: Value = serde_json::from_slice(&body).map_err(|_| AccountError::Network)?;
    let content: String = v["content"].as_str().unwrap_or("").chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = crate::recap::decode_base64(&content).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    Ok((parse_testers(&text), parse_survey(&text), v["sha"].as_str().map(str::to_string)))
}

fn owner_token(app: &AppHandle) -> Result<String, AccountError> {
    if !is_owner(app) {
        return Err(AccountError::NotOwner);
    }
    crate::secrets::get_internal(crate::updater::RELEASE_TOKEN).ok_or(AccountError::NoReleaseToken)
}

#[tauri::command]
pub async fn account_testers(app: AppHandle) -> Result<Vec<Person>, AccountError> {
    let token = owner_token(&app)?;
    let (list, survey, _) = read_list(&token).await?;
    *state(&app).testers.lock().unwrap() = Some(list.clone());
    *state(&app).survey.lock().unwrap() = survey;
    Ok(list)
}

async fn write_list(app: &AppHandle, edit: impl FnOnce(&mut Vec<Person>, &mut Survey)) -> Result<Vec<Person>, AccountError> {
    let token = owner_token(app)?;
    let (mut list, mut survey, sha) = read_list(&token).await?;
    edit(&mut list, &mut survey);
    let content = crate::claude::base64_for(testers_json(&list, &survey).as_bytes());
    let mut body = json!({
        "message": "Beta testers: updated from Coucou",
        "content": content,
        "branch": TESTERS_BRANCH,
    });
    if let Some(sha) = sha {
        body["sha"] = Value::String(sha);
    }
    let (status, _) = call(reqwest::Method::PUT, TESTERS_API, Some(&token), Some(body)).await?;
    match status {
        200 | 201 => {}
        401 => return Err(AccountError::ReleaseTokenRefused),
        403 | 404 => return Err(AccountError::NeedsContents),
        _ => return Err(AccountError::Network),
    }
    *state(app).testers.lock().unwrap() = Some(list.clone());
    *state(app).survey.lock().unwrap() = survey;
    changed(app);
    Ok(list)
}

#[tauri::command]
pub async fn account_tester_add(app: AppHandle, login: String, name: String) -> Result<Vec<Person>, AccountError> {
    let login = login.trim().to_string();
    if !valid_login(&login) {
        return Err(AccountError::BadLogin);
    }
    let name: String = name.trim().chars().take(80).collect();
    crate::log::line("account: a beta tester was added");
    write_list(&app, move |list, _| {
        if !is_tester(list, &login) {
            list.push(Person { login, name });
        }
    })
    .await
}

#[tauri::command]
pub async fn account_tester_remove(app: AppHandle, login: String) -> Result<Vec<Person>, AccountError> {
    crate::log::line("account: a beta tester was removed");
    write_list(&app, move |list, _| list.retain(|p| !p.login.eq_ignore_ascii_case(login.trim()))).await
}

/// When and whom Coucou asks for a review (owner only); kept in testers.json.
#[tauri::command]
pub async fn account_set_survey(app: AppHandle, days: u32, who: String) -> Result<AccountState, AccountError> {
    let days = days.clamp(1, 60);
    let who = if who == "everyone" { "everyone" } else { "testers" }.to_string();
    write_list(&app, move |_, survey| *survey = Survey { days, who }).await?;
    Ok(account_state_of(&app))
}

/// What reviews.rs needs: the survey, and whether this account is asked.
pub fn survey(app: &AppHandle) -> Survey {
    state(app).survey.lock().unwrap().clone()
}

/// The signed-in login ("" when signed out).
pub fn login(app: &AppHandle) -> String {
    state(app).me.lock().unwrap().as_ref().map(|m| m.login.clone()).unwrap_or_default()
}

pub fn signed_in(app: &AppHandle) -> bool {
    state(app).me.lock().unwrap().is_some()
}

/// The sign-in's token, for reviews.rs.
pub fn user_token() -> Option<String> {
    token().ok()
}

/// GitHub's people search, with their display names (owner only).
#[tauri::command]
pub async fn account_search(app: AppHandle, query: String) -> Result<Vec<Person>, AccountError> {
    if !is_owner(&app) {
        return Err(AccountError::NotOwner);
    }
    let query: String = query.trim().chars().take(60).collect();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let token = token().or_else(|_| owner_token(&app))?;
    let mut url = reqwest::Url::parse("https://api.github.com/search/users").map_err(|_| AccountError::Network)?;
    url.query_pairs_mut().append_pair("q", &format!("{query} type:user")).append_pair("per_page", "6");
    let (status, body) = call(reqwest::Method::GET, url.as_str(), Some(&token), None).await?;
    if status == 401 {
        drop_account(&app);
        return Err(AccountError::SignedOut);
    }
    if status != 200 {
        return Err(AccountError::Network);
    }
    let v: Value = serde_json::from_slice(&body).map_err(|_| AccountError::Network)?;
    let logins: Vec<String> = v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["login"].as_str())
        .filter(|l| valid_login(l))
        .map(str::to_string)
        .collect();
    // Their names, side by side (the search only gives logins).
    let mut tasks = Vec::new();
    for login in logins {
        let token = token.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            let name = match call(reqwest::Method::GET, &format!("https://api.github.com/users/{login}"), Some(&token), None).await {
                Ok((200, body)) => serde_json::from_slice::<Value>(&body)
                    .ok()
                    .and_then(|v| v["name"].as_str().map(str::to_string))
                    .unwrap_or_default(),
                _ => String::new(),
            };
            Person { login, name }
        }));
    }
    let mut out = Vec::new();
    for task in tasks {
        if let Ok(p) = task.await {
            out.push(p);
        }
    }
    Ok(out)
}

// ── Saves ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn account_set_sync(app: AppHandle, sync: bool, parts: Vec<String>) -> AccountState {
    let a = state(&app);
    if let Some(me) = a.me.lock().unwrap().as_mut() {
        me.sync = sync;
        me.parts = all_parts().into_iter().filter(|p| parts.contains(p)).collect();
        write_stored(Some(me));
    }
    account_state_of(&app)
}

/// settings.json was saved: the picked parts go up a little later.
pub fn settings_saved(app: &AppHandle, settings: &crate::settings::Settings) {
    let a = state(app);
    let on = a.me.lock().unwrap().as_ref().is_some_and(|m| m.sync);
    if !on {
        return;
    }
    let Ok(value) = serde_json::to_value(settings) else { return };
    *a.pending.lock().unwrap() = Some(value);
    let mine = a.changes.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(UPLOAD_AFTER).await;
        if state(&app).changes.load(Ordering::SeqCst) != mine {
            return;
        }
        let Some(value) = state(&app).pending.lock().unwrap().take() else { return };
        if upload(&app, &value).await.is_ok() {
            let _ = app.emit("account-changed", account_state_of(&app));
        }
    });
}

async fn find_gist(token: &str) -> Result<Option<String>, AccountError> {
    let (status, body) = call(reqwest::Method::GET, "https://api.github.com/gists?per_page=100", Some(token), None).await?;
    if status == 401 {
        return Err(AccountError::SignedOut);
    }
    if status != 200 {
        return Err(AccountError::Network);
    }
    let v: Value = serde_json::from_slice(&body).map_err(|_| AccountError::Network)?;
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .find(|g| g["files"].get(GIST_FILE).is_some())
        .and_then(|g| g["id"].as_str())
        .map(str::to_string))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

async fn upload(app: &AppHandle, settings: &Value) -> Result<u64, AccountError> {
    let token = token()?;
    let me = state(app).me.lock().unwrap().clone().ok_or(AccountError::NotSignedIn)?;
    let at = now_ms();
    let file = json!({ "v": 1, "savedAt": at, "parts": snapshot(settings, &me.parts) });
    let content = serde_json::to_string_pretty(&file).unwrap_or_default();
    let mut files = Map::new();
    files.insert(GIST_FILE.to_string(), json!({ "content": content }));
    let files = Value::Object(files);
    let mut gist = me.gist.clone();
    if gist.is_empty() {
        gist = find_gist(&token).await.map_err(|e| signed_out(app, e))?.unwrap_or_default();
    }
    let mut ok = false;
    if !gist.is_empty() {
        let (status, _) =
            call(reqwest::Method::PATCH, &format!("https://api.github.com/gists/{gist}"), Some(&token), Some(json!({ "files": files.clone() })))
                .await?;
        if status == 401 {
            return Err(signed_out(app, AccountError::SignedOut));
        }
        ok = status == 200;
    }
    if !ok {
        // None yet, or deleted on github.com: a new secret one.
        let body = json!({ "description": GIST_DESCRIPTION, "public": false, "files": files });
        let (status, created) = call(reqwest::Method::POST, "https://api.github.com/gists", Some(&token), Some(body)).await?;
        if status == 401 {
            return Err(signed_out(app, AccountError::SignedOut));
        }
        if status != 201 {
            return Err(AccountError::Network);
        }
        let v: Value = serde_json::from_slice(&created).map_err(|_| AccountError::Network)?;
        gist = v["id"].as_str().unwrap_or("").to_string();
    }
    if let Some(me) = state(app).me.lock().unwrap().as_mut() {
        me.gist = gist;
        me.last_sync = at;
        write_stored(Some(me));
    }
    Ok(at)
}

fn signed_out(app: &AppHandle, e: AccountError) -> AccountError {
    if e == AccountError::SignedOut {
        drop_account(app);
    }
    e
}

#[tauri::command]
pub async fn account_sync_now(app: AppHandle) -> Result<AccountState, AccountError> {
    let settings = app.state::<crate::Shared>().settings.lock().unwrap().clone();
    let value = serde_json::to_value(&settings).map_err(|_| AccountError::Network)?;
    upload(&app, &value).await?;
    Ok(account_state_of(&app))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub saved_at: u64,
    pub parts: Vec<String>,
}

async fn read_saved(app: &AppHandle) -> Result<Option<Value>, AccountError> {
    let token = token()?;
    let mut gist = state(app).me.lock().unwrap().as_ref().map(|m| m.gist.clone()).unwrap_or_default();
    if gist.is_empty() {
        match find_gist(&token).await.map_err(|e| signed_out(app, e))? {
            Some(g) => gist = g,
            None => return Ok(None),
        }
    }
    let (status, body) = call(reqwest::Method::GET, &format!("https://api.github.com/gists/{gist}"), Some(&token), None).await?;
    match status {
        200 => {}
        401 => return Err(signed_out(app, AccountError::SignedOut)),
        404 => return Ok(None),
        _ => return Err(AccountError::Network),
    }
    let v: Value = serde_json::from_slice(&body).map_err(|_| AccountError::Network)?;
    let text = v["files"][GIST_FILE]["content"].as_str().unwrap_or("");
    if let Some(me) = state(app).me.lock().unwrap().as_mut() {
        if me.gist != gist {
            me.gist = gist;
            write_stored(Some(me));
        }
    }
    Ok(serde_json::from_str::<Value>(text).ok())
}

/// What the saves on GitHub hold, if there are any.
#[tauri::command]
pub async fn account_saved(app: AppHandle) -> Result<Option<Saved>, AccountError> {
    Ok(read_saved(&app).await?.map(|v| Saved {
        saved_at: v["savedAt"].as_u64().unwrap_or(0),
        parts: saved_parts(&v["parts"]),
    }))
}

/// The settings with the picked saved parts put back. The page saves them,
/// so everything a change sets off happens as for any other change.
#[tauri::command]
pub async fn account_restore(app: AppHandle, parts: Vec<String>) -> Result<Value, AccountError> {
    let saved = read_saved(&app).await?.ok_or(AccountError::NoSaves)?;
    let settings = app.state::<crate::Shared>().settings.lock().unwrap().clone();
    let mut value = serde_json::to_value(&settings).map_err(|_| AccountError::Network)?;
    apply(&mut value, &saved["parts"], &parts);
    crate::log::line("account: saves restored from GitHub");
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_saved_setting_is_a_real_one_and_nothing_tied_to_this_computer_goes() {
        let defaults = serde_json::to_value(crate::settings::Settings::default()).unwrap();
        for (part, keys) in PARTS {
            for key in *keys {
                assert!(defaults.get(*key).is_some(), "{part}: {key} is not a setting");
            }
        }
        let stays = [
            "autostart", "hooksInstalled", "planRelayInstalled", "ollamaUrl", "lmstudioUrl", "customUrl",
            "claudeCodeDir", "chatsDir", "desktopMochi", "islandEdge", "phoneLink", "phoneChat", "phoneRelay",
            "phoneRelayUrl", "phoneChatModels",
        ];
        for key in stays {
            assert!(PARTS.iter().all(|(_, keys)| !keys.contains(&key)), "{key} must stay on this computer");
        }
    }

    #[test]
    fn only_the_parts_picked_go_and_come_back() {
        let settings = json!({ "pet": { "treats": 7 }, "mochiOutfit": "pirate", "language": "ru", "phoneLink": true });
        let snap = snapshot(&settings, &["mochi".into(), "general".into()]);
        assert_eq!(snap["mochi"]["pet"]["treats"], 7);
        assert_eq!(snap["general"]["language"], "ru");
        assert!(snap.get("outfits").is_none());
        assert!(!snap.to_string().contains("phoneLink"));
        assert_eq!(saved_parts(&snap), vec!["general".to_string(), "mochi".to_string()]);

        let mut other = json!({ "pet": { "treats": 1 }, "mochiOutfit": "none", "language": "fr", "phoneLink": false });
        apply(&mut other, &snap, &["mochi".into()]);
        assert_eq!(other["pet"]["treats"], 7);
        assert_eq!(other["language"], "fr", "general was not picked");
        assert_eq!(other["phoneLink"], false);
        // a part picked but not saved changes nothing; a stray key in the file neither
        let sneaky = json!({ "outfits": { "mochiOutfit": "x", "phoneLink": true } });
        apply(&mut other, &sneaky, &["outfits".into()]);
        assert_eq!(other["mochiOutfit"], "x");
        assert_eq!(other["phoneLink"], false);
    }

    #[test]
    fn the_testers_file_reads_both_ways_and_stays_clean() {
        let list = parse_testers(r#"{"testers":["anna-tests",{"login":"Sergey-Dev","name":" Сергей "},"bad login","sergey-dev","-x",""]}"#);
        assert_eq!(list, vec![
            Person { login: "anna-tests".into(), name: "".into() },
            Person { login: "Sergey-Dev".into(), name: "Сергей".into() },
        ]);
        assert!(is_tester(&list, "sergey-dev"));
        assert!(!is_tester(&list, "maxim"));
        assert_eq!(parse_testers(&testers_json(&list, &Survey::default())), list);
        let survey = Survey { days: 3, who: "everyone".into() };
        assert_eq!(parse_survey(&testers_json(&list, &survey)), survey);
        assert_eq!(parse_survey(r#"{"survey":{"days":0,"who":"all"}}"#), Survey::default());
        assert_eq!(parse_survey("{}"), Survey::default());
        assert!(parse_testers("not json").is_empty());
        assert!(parse_testers(r#"{"testers":[]}"#).is_empty());
    }

    #[test]
    fn logins_are_githubs() {
        assert!(valid_login("Disa153"));
        assert!(valid_login("a-b"));
        for bad in ["", "-a", "a-", "a--b", "a b", "a/b", "../x", &"x".repeat(40)] {
            assert!(!valid_login(bad), "{bad}");
        }
    }
}
