// Updates from inside Coucou (this fork): two channels, published as GitHub
// release assets of Disa153/coucou.
//
//   * beta: every build of the all-in-one branch, for the owner, behind a
//     password (its SHA-256 is compiled in from the COUCOU_BETA_PASSWORD secret;
//     no secret, no beta switch). The password only hides the switch: the beta
//     installers are public files like any release.
//   * stable: only a beta build the owner promoted by hand (the
//     "Promote beta to stable" workflow). Everybody else is on it.
//
// Each channel has an `update.json` (release `channel-beta` / `channel-stable`)
// that names the build, the installer and its SHA-256, signed with the
// owner's Ed25519 key (secret COUCOU_UPDATE_KEY). Coucou checks the signature
// with the public key below and the installer's hash before running it, so a
// file swapped on the way, or a manifest written by anyone else, is refused.
//
// Nothing is downloaded or installed without a click; the check itself is one
// small GET at launch and every few hours.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use ring::digest;
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// The owner's public key (raw Ed25519, base64). Its private half is the
/// COUCOU_UPDATE_KEY secret of the repository and never ships.
const PUBLIC_KEY: &str = "sjtOpogg4BrmBSPWPorn4tsn/VN+cEyDxjT6Qsg4gLU=";
/// Where the channels and the installers live; any other address is refused.
const RELEASES: &str = "https://github.com/Disa153/coucou/releases/download/";
/// The page with the "Promote beta to stable" workflow, for the beta tester.
pub const PROMOTE_PAGE: &str = "https://github.com/Disa153/coucou/actions/workflows/promote-stable.yml";
const MANIFEST_LIMIT: usize = 64 * 1024;
const INSTALLER_LIMIT: usize = 400 * 1024 * 1024;
const BETA_SALT: &str = "coucou-beta:";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const FIRST_CHECK: Duration = Duration::from_secs(45);

/// This build's number (the CI run that made it); 0 for a local build, which never updates itself.
pub fn build() -> u64 {
    option_env!("COUCOU_BUILD").and_then(|b| b.trim().parse().ok()).unwrap_or(0)
}

fn beta_hash() -> Option<&'static str> {
    option_env!("COUCOU_BETA_HASH").map(str::trim).filter(|h| h.len() == 64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

impl Channel {
    pub fn name(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }
}

/// What a channel offers, as `update.json` says it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub channel: String,
    pub build: u64,
    pub version: String,
    #[serde(default)]
    pub notes: String,
    pub url: String,
    pub sha256: String,
    pub signature: String,
}

/// The text the signature covers: everything that decides what gets installed.
pub fn signed_text(channel: &str, build: u64, version: &str, url: &str, sha256: &str) -> String {
    format!("coucou-update\n{channel}\n{build}\n{version}\n{url}\n{sha256}")
}

/// Why an update was not offered or not installed; the page words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Refusal {
    /// The manifest is not signed by the owner's key, or names another file.
    BadSignature,
    /// The downloaded installer is not the one the manifest names.
    BadHash,
    Network,
    /// Only the Windows installer updates itself.
    Unsupported,
    /// A local build has no number to compare.
    LocalBuild,
}

fn decode_b64(s: &str) -> Option<Vec<u8>> {
    crate::recap::decode_base64(s.trim())
}

/// The manifest is the owner's, for this channel, and names a file of this repository.
pub fn verify(m: &Manifest, channel: Channel, public_key: &[u8]) -> Result<(), Refusal> {
    let ok_fields = m.channel == channel.name()
        && m.url.starts_with(RELEASES)
        && !m.url.contains("..")
        && m.sha256.len() == 64
        && m.sha256.bytes().all(|b| b.is_ascii_hexdigit());
    if !ok_fields {
        return Err(Refusal::BadSignature);
    }
    let sig = decode_b64(&m.signature).ok_or(Refusal::BadSignature)?;
    let text = signed_text(&m.channel, m.build, &m.version, &m.url, &m.sha256.to_ascii_lowercase());
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(text.as_bytes(), &sig)
        .map_err(|_| Refusal::BadSignature)
}

/// Whether `password` opens the beta channel of this build.
pub fn password_ok(password: &str, hash: Option<&str>) -> bool {
    let Some(hash) = hash else { return false };
    let got = hex(digest::digest(&digest::SHA256, format!("{BETA_SALT}{password}").as_bytes()).as_ref());
    // Not a secret worth a constant-time compare: it only hides a switch.
    got.eq_ignore_ascii_case(hash)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn public_key() -> Option<Vec<u8>> {
    decode_b64(PUBLIC_KEY).filter(|k| k.len() == 32)
}

// ── The chosen channel, kept beside the settings ──────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Stored {
    channel: Channel,
}

fn stored_path() -> PathBuf {
    crate::settings::local_dir().join("update.json")
}

fn load_channel() -> Channel {
    let channel = std::fs::read_to_string(stored_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Stored>(&t).ok())
        .map(|s| s.channel)
        .unwrap_or_default();
    // A build without the beta password cannot stay on beta.
    if channel == Channel::Beta && beta_hash().is_none() {
        Channel::Stable
    } else {
        channel
    }
}

fn save_channel(channel: Channel) {
    let path = stored_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(&Stored { channel }) {
        let _ = std::fs::write(path, text);
    }
}

// ── State and commands ────────────────────────────────────────────────────────

#[derive(Default)]
pub struct Updater {
    channel: Mutex<Channel>,
    /// The newer build found by the last check.
    found: Mutex<Option<Manifest>>,
    installing: std::sync::atomic::AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub build: u64,
    pub channel: Channel,
    /// This build has a beta password: the switch can be offered.
    pub beta_available: bool,
    /// Updates install themselves here (the Windows installer).
    pub can_install: bool,
    pub found: Option<Manifest>,
    pub promote_page: Option<String>,
}

pub fn setup(app: &AppHandle) {
    let updater = Updater { channel: Mutex::new(load_channel()), ..Default::default() };
    app.manage(updater);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            let _ = check(&app).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

fn state(app: &AppHandle) -> &Updater {
    app.state::<Updater>().inner()
}

fn info(app: &AppHandle) -> UpdateInfo {
    let u = state(app);
    let channel = *u.channel.lock().unwrap();
    UpdateInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build: build(),
        channel,
        beta_available: beta_hash().is_some(),
        can_install: cfg!(windows),
        found: u.found.lock().unwrap().clone(),
        promote_page: (channel == Channel::Beta).then(|| PROMOTE_PAGE.to_string()),
    }
}

fn manifest_url(channel: Channel) -> String {
    format!("{RELEASES}channel-{}/update.json", channel.name())
}

/// Ok(None): the file is not there (a channel nothing was published to yet).
async fn fetch(url: &str, limit: usize, timeout: Duration) -> Result<Option<Vec<u8>>, Refusal> {
    let parsed = reqwest::Url::parse(url).map_err(|_| Refusal::Network)?;
    let client = crate::net::client(&parsed, timeout).map_err(|_| Refusal::Network)?;
    let response = client
        .get(parsed)
        .header("User-Agent", "Coucou")
        .send()
        .await
        .map_err(|_| Refusal::Network)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(Refusal::Network);
    }
    crate::net::read_capped(response, limit).await.map(Some).map_err(|_| Refusal::Network)
}

/// Asks the chosen channel for a newer build. Ok(None): up to date (or nothing published yet).
async fn check(app: &AppHandle) -> Result<Option<Manifest>, Refusal> {
    let current = build();
    if current == 0 {
        return Err(Refusal::LocalBuild);
    }
    let channel = *state(app).channel.lock().unwrap();
    let body = match fetch(&manifest_url(channel), MANIFEST_LIMIT, Duration::from_secs(20)).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            *state(app).found.lock().unwrap() = None;
            return Ok(None);
        }
        Err(e) => {
            crate::log::line("update: the channel could not be read");
            return Err(e);
        }
    };
    let manifest: Manifest = serde_json::from_slice(&body).map_err(|_| Refusal::BadSignature)?;
    let key = public_key().ok_or(Refusal::BadSignature)?;
    if let Err(e) = verify(&manifest, channel, &key) {
        crate::log::line(format!("update: build {} on {} refused (signature)", manifest.build, channel.name()));
        return Err(e);
    }
    // On stable, a lower number than ours means we came from beta: nothing to do.
    let newer = manifest.build > current;
    let found = newer.then(|| manifest.clone());
    *state(app).found.lock().unwrap() = found.clone();
    if let Some(m) = &found {
        crate::log::line(format!("update: build {} available on {}", m.build, channel.name()));
        let _ = app.emit("update-available", m);
    }
    // The tray says so until it is installed.
    if let Some(tray) = app.tray_by_id("coucou") {
        let tip = match &found {
            Some(m) => format!("Coucou — {}", crate::i18n::tf("Build {0} is ready to install.", &[("0", &m.build.to_string())])),
            None => "Coucou".to_string(),
        };
        let _ = tray.set_tooltip(Some(tip));
    }
    Ok(found)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub info: UpdateInfo,
    pub refusal: Option<Refusal>,
}

#[tauri::command]
pub fn update_info(app: AppHandle) -> UpdateInfo {
    info(&app)
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> CheckResult {
    let refusal = check(&app).await.err();
    CheckResult { info: info(&app), refusal }
}

/// Beta needs the password; stable never does. False when the password is wrong.
#[tauri::command]
pub async fn update_set_channel(app: AppHandle, channel: Channel, password: Option<String>) -> bool {
    if channel == Channel::Beta && !password_ok(password.as_deref().unwrap_or(""), beta_hash()) {
        return false;
    }
    *state(&app).channel.lock().unwrap() = channel;
    *state(&app).found.lock().unwrap() = None;
    save_channel(channel);
    let _ = check(&app).await;
    true
}

/// Downloads the build found, checks it is the signed one, and runs its installer;
/// Coucou quits and starts again once it is installed.
#[tauri::command]
pub async fn update_install(app: AppHandle, build: u64) -> Result<(), Refusal> {
    if !cfg!(windows) {
        return Err(Refusal::Unsupported);
    }
    let manifest = state(&app).found.lock().unwrap().clone().filter(|m| m.build == build).ok_or(Refusal::Network)?;
    let u = state(&app);
    if u.installing.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }
    let result = download_and_run(&app, &manifest).await;
    if result.is_err() {
        u.installing.store(false, std::sync::atomic::Ordering::SeqCst);
    }
    result
}

async fn download_and_run(app: &AppHandle, m: &Manifest) -> Result<(), Refusal> {
    // The manifest is checked again: the one kept may be hours old, the rules are not.
    let channel = *state(app).channel.lock().unwrap();
    verify(m, channel, &public_key().ok_or(Refusal::BadSignature)?)?;
    let bytes = fetch(&m.url, INSTALLER_LIMIT, Duration::from_secs(15 * 60)).await?.ok_or(Refusal::Network)?;
    let got = hex(digest::digest(&digest::SHA256, &bytes).as_ref());
    if !got.eq_ignore_ascii_case(&m.sha256) {
        crate::log::line(format!("update: build {} refused (the file is not the signed one)", m.build));
        return Err(Refusal::BadHash);
    }
    let path = std::env::temp_dir().join(format!("Coucou-update-{}.exe", m.build));
    std::fs::write(&path, &bytes).map_err(|_| Refusal::Network)?;
    crate::log::line(format!("update: installing build {}", m.build));
    run_installer(&path)?;
    // Let the installer replace this exe; it starts Coucou again when done.
    app.exit(0);
    Ok(())
}

#[cfg(windows)]
fn run_installer(installer: &std::path::Path) -> Result<(), Refusal> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let exe = std::env::current_exe().map_err(|_| Refusal::Unsupported)?;
    // Wait for this Coucou to quit, install silently (per user, no admin
    // prompt), then start the installed Coucou.
    let line = format!(
        "/C ping -n 3 127.0.0.1 >nul & \"{}\" /S & start \"\" \"{}\"",
        installer.display(),
        exe.display()
    );
    std::process::Command::new("cmd.exe")
        .raw_arg(line)
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map(|_| ())
        .map_err(|_| Refusal::Unsupported)
}

#[cfg(not(windows))]
fn run_installer(_installer: &std::path::Path) -> Result<(), Refusal> {
    Err(Refusal::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn keys() -> Ed25519KeyPair {
        let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap()
    }

    fn b64(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in bytes.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..4 {
                out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
            }
        }
        out
    }

    fn signed(k: &Ed25519KeyPair, channel: &str, build: u64, url: &str) -> Manifest {
        let sha = "ab".repeat(32);
        let sig = k.sign(signed_text(channel, build, "0.5.0", url, &sha).as_bytes());
        Manifest {
            channel: channel.into(),
            build,
            version: "0.5.0".into(),
            notes: String::new(),
            url: url.into(),
            sha256: sha,
            signature: b64(sig.as_ref()),
        }
    }

    const URL: &str = "https://github.com/Disa153/coucou/releases/download/channel-stable/Coucou-Windows-setup.exe";

    #[test]
    fn a_manifest_signed_by_the_owner_is_taken() {
        let k = keys();
        let m = signed(&k, "stable", 42, URL);
        assert_eq!(verify(&m, Channel::Stable, k.public_key().as_ref()), Ok(()));
    }

    #[test]
    fn any_change_or_another_key_is_refused() {
        let k = keys();
        let pk = k.public_key().as_ref().to_vec();
        let m = signed(&k, "stable", 42, URL);
        let mut later = m.clone();
        later.build = 43;
        assert_eq!(verify(&later, Channel::Stable, &pk), Err(Refusal::BadSignature));
        let mut other_file = m.clone();
        other_file.sha256 = "cd".repeat(32);
        assert_eq!(verify(&other_file, Channel::Stable, &pk), Err(Refusal::BadSignature));
        // a beta manifest is not a stable one, even signed
        let beta = signed(&k, "beta", 42, URL);
        assert_eq!(verify(&beta, Channel::Stable, &pk), Err(Refusal::BadSignature));
        // someone else's key
        assert_eq!(verify(&m, Channel::Stable, keys().public_key().as_ref()), Err(Refusal::BadSignature));
    }

    #[test]
    fn a_file_outside_the_repository_is_refused_even_signed() {
        let k = keys();
        let m = signed(&k, "stable", 42, "https://example.com/Coucou-Windows-setup.exe");
        assert_eq!(verify(&m, Channel::Stable, k.public_key().as_ref()), Err(Refusal::BadSignature));
        let up = signed(&k, "stable", 42, "https://github.com/Disa153/coucou/releases/download/../../evil.exe");
        assert_eq!(verify(&up, Channel::Stable, k.public_key().as_ref()), Err(Refusal::BadSignature));
    }

    #[test]
    fn the_beta_password_is_checked_against_its_hash() {
        let hash = hex(digest::digest(&digest::SHA256, b"coucou-beta:s3cret").as_ref());
        assert!(password_ok("s3cret", Some(&hash)));
        assert!(!password_ok("S3cret", Some(&hash)));
        assert!(!password_ok("", Some(&hash)));
        // a build without the secret has no beta at all
        assert!(!password_ok("s3cret", None));
    }

    #[test]
    fn a_manifest_from_the_workflow_verifies_with_the_shipped_key() {
        // Written by .github/scripts/sign-update.mjs with the owner's key.
        let m = Manifest {
            channel: "beta".into(),
            build: 57,
            version: "0.5.0".into(),
            notes: String::new(),
            url: "https://github.com/Disa153/coucou/releases/download/beta-57/Coucou-Windows-setup.exe".into(),
            sha256: "bd610bb2b275a8bdb682c4538e6eba6c5b4ddeb4e44e1e994ab40d2a027515b3".into(),
            signature: "eEEELOabHepHstuJuWsm/rmtvUF8YVFrbbgFyruJ3CtJZ3aqQY7SVwoFj7naYsm0vBs732O9REjkYnX9VGMDCQ==".into(),
        };
        let key = public_key().expect("the shipped key decodes to 32 bytes");
        assert_eq!(verify(&m, Channel::Beta, &key), Ok(()));
        assert_eq!(verify(&m, Channel::Stable, &key), Err(Refusal::BadSignature));
    }

    #[test]
    fn the_signed_text_is_what_the_workflow_signs() {
        assert_eq!(signed_text("beta", 7, "0.5.0", "u", "h"), "coucou-update\nbeta\n7\n0.5.0\nu\nh");
    }
}
