// « OK Coucou » on Windows and Linux: say it, then ask anything. Port of the
// Mac's Voice/ (VoiceEngine.swift and its friends).
//
// While it is on, the microphone is open and a cheap energy detector cuts what
// you say into stretches; only a stretch of speech goes to Parakeet, on this
// computer, and only a stretch that starts with « OK Coucou » goes further:
// what you said after it goes to Claude with your own key (brain.rs), never
// the audio. Mochi shows it all in the pill at the bottom of the screen —
// listening, thinking, then the answer, said out loud by the system's voice
// (the pill's speechSynthesis) or ElevenLabs with your key.
//
// It stops listening while you dictate, while a call is recorded and while it
// speaks itself.

pub mod brain;
pub mod wake;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::flow::pill;
use crate::speech::{self, audio};

/// What the pill shows of a conversation.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct VoiceView {
    /// "listening", "thinking", "speaking".
    pub phase: String,
    /// What you said (after « OK Coucou »).
    pub heard: String,
    /// Coucou's answer.
    pub answer: String,
}

struct Running {
    stop: Arc<AtomicBool>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

/// Rust waits on these for the island's answers and the end of speech.
fn pending() -> &'static Mutex<HashMap<u64, mpsc::Sender<String>>> {
    static P: OnceLock<Mutex<HashMap<u64, mpsc::Sender<String>>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Whether the assistant can run: the voice model, and a Claude key unless
/// what is said goes to the island's chat (Claude Code needs none).
pub fn requirements_met(app: &AppHandle) -> bool {
    let to_chat = crate::flow::get(app).is_none_or(|f| f.store.lock().unwrap().prefs.voice_to_chat);
    speech::is_ready() && (to_chat || crate::secrets::present(crate::claude::KEY))
}

/// The answer's first words, as they are said: no Markdown, no code, at most
/// two sentences (the rest stays in the chat).
pub fn speakable(answer: &str) -> String {
    let mut text = String::new();
    let mut in_code = false;
    for line in answer.lines() {
        let l = line.trim();
        if l.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || l.is_empty() || l.starts_with('|') {
            continue;
        }
        let l = l.trim_start_matches(['#', '>', '-', '*', ' ']);
        let clean: String = l.chars().filter(|c| !matches!(c, '*' | '`' | '_')).collect();
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(clean.trim());
        if text.len() > 600 {
            break;
        }
    }
    // Links: keep their words.
    while let (Some(a), Some(b)) = (text.find("]("), text.find(')')) {
        if b <= a {
            break;
        }
        let open = text[..a].rfind('[').unwrap_or(a);
        let words = text[open + 1..a].to_string();
        text.replace_range(open..=b, &words);
    }
    // A sentence ends at . ! ? … followed by a space or the end, not after a
    // short abbreviation ("см.", "т.е.", "e.g.").
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut sentences = 0;
    for (i, &c) in chars.iter().enumerate() {
        out.push(c);
        let ends = matches!(c, '.' | '!' | '?' | '…') && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
        let word: String = out.trim_end_matches(['.', '!', '?', '…']).chars().rev().take_while(|c| c.is_alphanumeric()).collect();
        let abbreviation = c == '.' && word.chars().count() <= 2 && word.chars().all(|c| c.is_lowercase());
        if ends && !abbreviation {
            sentences += 1;
            if sentences == 2 {
                break;
            }
        }
    }
    out.trim().to_string()
}

/// The user's wake phrase (Settings → Voice), « Привет, Диса » by default.
fn phrase(app: &AppHandle) -> String {
    let p = crate::flow::get(app).map(|f| f.store.lock().unwrap().prefs.wake_phrase.clone()).unwrap_or_default();
    if p.trim().is_empty() { crate::flow::store::DEFAULT_WAKE_PHRASE.to_string() } else { p }
}

/// Starts or stops listening to follow the preference.
pub fn sync(app: &AppHandle) {
    let wanted = crate::flow::get(app).is_some_and(|f| f.store.lock().unwrap().prefs.voice);
    let mut running = RUNNING.lock().unwrap();
    match (wanted && requirements_met(app), running.is_some()) {
        (true, false) => {
            let stop = Arc::new(AtomicBool::new(false));
            let app2 = app.clone();
            let stop2 = stop.clone();
            std::thread::Builder::new().name("coucou-voice".into()).spawn(move || run(app2, stop2)).ok();
            *running = Some(Running { stop });
            speech::preload();
            crate::log::line("voice: « OK Coucou » listening");
        }
        (false, true) => {
            if let Some(r) = running.take() {
                r.stop.store(true, Ordering::Relaxed);
            }
            let dictation = crate::flow::get(app).is_some_and(|f| f.store.lock().unwrap().prefs.enabled);
            if !dictation && !crate::notes::is_recording(app) {
                speech::unload();
            }
            crate::log::line("voice: « OK Coucou » off");
        }
        _ => {}
    }
}

/// « OK Coucou » is listening (the voice model stays loaded).
pub fn is_listening() -> bool {
    RUNNING.lock().unwrap().is_some()
}

/// Starts again on another microphone.
pub fn restart(app: &AppHandle) {
    if let Some(r) = RUNNING.lock().unwrap().take() {
        r.stop.store(true, Ordering::Relaxed);
    }
    sync(app);
}

fn show(app: &AppHandle, view: Option<VoiceView>) {
    pill::update(app, |s| s.voice = view);
}

enum Mode {
    /// Waiting for « OK Coucou ».
    Wake,
    /// Heard « OK Coucou » alone: what comes next is the question.
    Command { text: String, until: Instant },
    /// Coucou asked something back: an answer comes without « OK Coucou ».
    FollowUp { until: Instant },
}

fn run(app: AppHandle, stop: Arc<AtomicBool>) {
    let mic = crate::flow::get(&app).map(|f| f.store.lock().unwrap().prefs.mic.clone()).unwrap_or_default();
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let capture = audio::Capture::start(audio::Source::Mic(if mic.is_empty() { None } else { Some(mic) }), move |s| {
        let _ = tx.send(s.to_vec());
    });
    let mut capture = match capture {
        Ok(c) => c,
        Err(err) => {
            crate::log::line(format!("voice: microphone: {err}"));
            *RUNNING.lock().unwrap() = None;
            return;
        }
    };
    let mut vad = wake::Vad::default();
    let mut mode = Mode::Wake;
    let mut brain = brain::Brain::default();
    let mut hide_at: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        let chunk = match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let now = Instant::now();
        // The pill's last answer goes away after a while.
        if hide_at.is_some_and(|t| now > t) {
            hide_at = None;
            show(&app, None);
        }
        match &mode {
            Mode::Command { until, .. } | Mode::FollowUp { until } if now > *until => {
                mode = Mode::Wake;
                vad.hangover = 25;
                show(&app, None);
            }
            _ => {}
        }
        // Dictation and calls have the microphone: Coucou doesn't listen meanwhile.
        if pill::dictating(&app) || crate::notes::is_recording(&app) {
            vad.reset();
            continue;
        }
        let Some(chunk) = chunk else { continue };
        for segment in vad.feed(&chunk) {
            if segment.len() < speech::SAMPLE_RATE as usize * 35 / 100 {
                continue;
            }
            let Some(heard) = speech::transcribe(&segment).map(|t| t.text.trim().to_string()) else { continue };
            if heard.is_empty() {
                continue;
            }
            let question = match &mut mode {
                Mode::Wake => match wake::after_any_wake(&heard, &phrase(&app)) {
                    Some(rest) => {
                        crate::log::line("voice: wake phrase heard");
                        hide_at = None;
                        if crate::flow::text::word_count(&rest) >= 2 {
                            Some(rest)
                        } else {
                            show(&app, Some(VoiceView { phase: "listening".into(), heard: rest.clone(), answer: String::new() }));
                            mode = Mode::Command { text: rest, until: now + Duration::from_secs(7) };
                            // A question may come with pauses in it.
                            vad.hangover = 40;
                            None
                        }
                    }
                    None => {
                        // A short stretch that was not the phrase: noted (on this computer only),
                        // so a phrase Parakeet writes oddly can be seen in the log.
                        if crate::flow::text::word_count(&heard) <= 4 {
                            crate::log::line(format!("voice: heard « {} », not the wake phrase", heard.chars().take(40).collect::<String>()));
                        }
                        None
                    }
                },
                Mode::Command { text, .. } => {
                    let full = format!("{text} {heard}").trim().to_string();
                    Some(full)
                }
                Mode::FollowUp { .. } => Some(heard),
            };
            let Some(question) = question else { continue };
            mode = Mode::Wake;
            vad.hangover = 25;
            show(&app, Some(VoiceView { phase: "thinking".into(), heard: question.clone(), answer: String::new() }));
            let to_chat = crate::flow::get(&app).is_none_or(|f| f.store.lock().unwrap().prefs.voice_to_chat);
            if to_chat {
                // Into the island's chat with Claude, like typed there; its answer is said.
                let full = ask_island_waiting(&app, "chat", json!({ "text": question }), Duration::from_secs(20 * 60)).unwrap_or_default();
                let said = speakable(&full);
                if said.is_empty() {
                    show(&app, Some(VoiceView { phase: "speaking".into(), heard: question, answer: crate::i18n::t("The answer is in the chat.") }));
                    hide_at = Some(Instant::now() + Duration::from_secs(5));
                    continue;
                }
                show(&app, Some(VoiceView { phase: "speaking".into(), heard: question, answer: said.clone() }));
                speak(&app, &said);
                vad.reset();
                while rx.try_recv().is_ok() {}
                hide_at = Some(Instant::now() + Duration::from_secs(6));
                continue;
            }
            let reply = brain.respond(&app, &question);
            let answer = reply.as_ref().map(|r| r.text.clone()).unwrap_or_default();
            if answer.is_empty() {
                let done = reply.is_some_and(|r| r.acted);
                show(&app, Some(VoiceView {
                    phase: "speaking".into(),
                    heard: question,
                    answer: if done { "✓".into() } else { crate::i18n::t("Coucou couldn't answer: check your Claude key in Notch → Chat.") },
                }));
                hide_at = Some(Instant::now() + Duration::from_secs(5));
                continue;
            }
            show(&app, Some(VoiceView { phase: "speaking".into(), heard: question.clone(), answer: answer.clone() }));
            speak(&app, &answer);
            vad.reset();
            // Drop what the microphone heard of Coucou's own voice.
            while rx.try_recv().is_ok() {}
            if answer.trim_end().ends_with('?') {
                mode = Mode::FollowUp { until: Instant::now() + Duration::from_secs(8) };
                vad.hangover = 40;
                show(&app, Some(VoiceView { phase: "listening".into(), heard: String::new(), answer }));
            } else {
                hide_at = Some(Instant::now() + Duration::from_secs(6));
            }
        }
    }
    capture.stop();
    show(&app, None);
}

// ── Speaking ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Say {
    id: u64,
    text: String,
    lang: Option<&'static str>,
    /// ElevenLabs' MP3, base64.
    audio: Option<String>,
    gender: String,
}

/// Says `text` and returns once it is said (or after a sensible time).
fn speak(app: &AppHandle, text: &str) {
    let (output, gender, chosen) = crate::flow::get(app)
        .map(|f| {
            let p = &f.store.lock().unwrap().prefs;
            (p.voice_output.clone(), p.voice_gender.clone(), p.eleven_voice.clone())
        })
        .unwrap_or_else(|| ("system".into(), "female".into(), String::new()));
    if output == "off" {
        std::thread::sleep(Duration::from_millis(1500));
        return;
    }
    let audio = if output == "elevenlabs" {
        match elevenlabs(text, &gender, &chosen) {
            Ok(mp3) => Some(base64(&mp3)),
            Err(why) => {
                *LAST_REFUSAL.lock().unwrap() = Some(why);
                None
            }
        }
    } else {
        None
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    pending().lock().unwrap().insert(id, tx);
    let _ = app.emit_to(pill::LABEL, "voice-say", Say { id, text: text.into(), lang: wake::speech_lang(text), audio, gender });
    let words = crate::flow::text::word_count(text) as u64;
    let _ = rx.recv_timeout(Duration::from_millis(4000 + words * 600));
    pending().lock().unwrap().remove(&id);
}

/// The pill says it has finished speaking (`id`), or the island answers a question.
#[tauri::command]
pub fn voice_done(id: u64, text: Option<String>) {
    if let Some(tx) = pending().lock().unwrap().remove(&id) {
        let _ = tx.send(text.unwrap_or_default());
    }
}

/// Where the webview has no voice (WebKitGTK often): speech-dispatcher or
/// espeak-ng, when installed.
#[tauri::command]
pub async fn voice_say_native(text: String, lang: Option<String>) -> bool {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "linux")]
        {
            let lang = lang.unwrap_or_default();
            let short = lang.split('-').next().unwrap_or("").to_string();
            if let Some(spd) = crate::platform::find_on_path("spd-say") {
                let mut cmd = std::process::Command::new(spd);
                cmd.arg("-w");
                if !short.is_empty() {
                    cmd.args(["-l", &short]);
                }
                return cmd.arg(&text).status().is_ok_and(|s| s.success());
            }
            if let Some(espeak) = crate::platform::find_on_path("espeak-ng") {
                let mut cmd = std::process::Command::new(espeak);
                if !short.is_empty() {
                    cmd.args(["-v", &short]);
                }
                return cmd.arg(&text).status().is_ok_and(|s| s.success());
            }
            false
        }
        #[cfg(windows)]
        {
            let _ = (text, lang);
            false
        }
    })
    .await
    .unwrap_or(false)
}

/// What ElevenLabs said last time it refused, for Settings → Voice → Listen.
static LAST_REFUSAL: Mutex<Option<String>> = Mutex::new(None);

/// ElevenLabs' answer in words: the HTTP status and its own message.
fn refusal(status: reqwest::StatusCode, body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let detail = v["detail"]["message"].as_str().or_else(|| v["detail"].as_str()).unwrap_or("").trim().to_string();
    let code = v["detail"]["status"].as_str().unwrap_or("");
    let mut out = format!("HTTP {}", status.as_u16());
    if !code.is_empty() {
        out.push_str(&format!(" · {code}"));
    }
    if !detail.is_empty() {
        out.push_str(&format!(" · {}", detail.chars().take(240).collect::<String>()));
    }
    out
}

/// One text-to-speech call with `voice`.
async fn eleven_say(client: &reqwest::Client, key: &str, voice: &str, text: &str) -> Result<Vec<u8>, String> {
    // Multilingual v2 reads Russian most naturally; calm and soft, with little acting.
    let response = client
        .post(format!("https://api.elevenlabs.io/v1/text-to-speech/{voice}?output_format=mp3_44100_128"))
        .header("xi-api-key", key)
        .header("accept", "audio/mpeg")
        .json(&json!({
            "text": text,
            "model_id": "eleven_multilingual_v2",
            "voice_settings": { "stability": 0.55, "similarity_boost": 0.75, "style": 0.1, "use_speaker_boost": true, "speed": 0.95 }
        }))
        .send()
        .await
        .map_err(|e| format!("no answer from ElevenLabs ({e})"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(refusal(status, &body));
    }
    response.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}

/// ElevenLabs' MP3 for `text`, with the user's key; Err (why) and the system voice speaks.
/// `chosen`: a voice ID from the Voice Library; when it is refused (a free plan
/// cannot use library voices through the API) one of the user's own voices, by gender.
fn elevenlabs(text: &str, gender: &str, chosen: &str) -> Result<Vec<u8>, String> {
    let key = crate::secrets::get("elevenlabs-api-key").ok_or_else(|| "no ElevenLabs key saved".to_string())?;
    let chosen = valid_voice_id(chosen).map(String::from);
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(25)).build().map_err(|e| e.to_string())?;
        let mut first_refusal = None;
        if let Some(v) = &chosen {
            match eleven_say(&client, &key, v, text).await {
                Ok(mp3) => return Ok(mp3),
                Err(why) => {
                    crate::log::line(format!("voice: ElevenLabs refused the chosen voice: {why}"));
                    first_refusal = Some(why);
                }
            }
        }
        let voices: Value = client
            .get("https://api.elevenlabs.io/v1/voices")
            .header("xi-api-key", &key)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let fallback = voices["voices"].as_array().and_then(|list| pick_voice(list, gender));
        let Some(voice) = fallback else {
            return Err(first_refusal.unwrap_or_else(|| refusal(reqwest::StatusCode::UNAUTHORIZED, &voices.to_string())));
        };
        match eleven_say(&client, &key, &voice, text).await {
            Ok(mp3) => {
                if let Some(why) = first_refusal {
                    *LAST_REFUSAL.lock().unwrap() = Some(format!("chosen voice refused ({why}); one of your voices spoke instead"));
                }
                Ok(mp3)
            }
            Err(why) => {
                crate::log::line(format!("voice: ElevenLabs refused: {why}"));
                Err(first_refusal.unwrap_or(why))
            }
        }
    })
}

/// A Voice Library ID ("rQOBu7YxCDxGiFdTm28w"), or a link to its page: the ID, else None.
pub fn valid_voice_id(text: &str) -> Option<&str> {
    let id = text.trim().trim_end_matches('/').rsplit('/').next()?.split(['?', '#']).next()?;
    (id.len() >= 16 && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric())).then_some(id)
}

/// Settings → Voice → Listen: says `text` with the chosen voice. "" when
/// ElevenLabs spoke as asked, else why it did not (the system voice spoke).
#[tauri::command]
pub async fn voice_preview(app: AppHandle, text: String) -> String {
    *LAST_REFUSAL.lock().unwrap() = None;
    let _ = tauri::async_runtime::spawn_blocking(move || speak(&app, &text)).await;
    LAST_REFUSAL.lock().unwrap().take().unwrap_or_default()
}

/// First English voice of the gender (a young one first for a man); else any
/// voice of the gender; else the first one. Same choice as the Mac.
pub fn pick_voice(voices: &[Value], gender: &str) -> Option<String> {
    let label = |v: &Value, k: &str| v["labels"][k].as_str().unwrap_or("").to_lowercase();
    let english = |v: &Value| {
        let lang = label(v, "language");
        if !lang.is_empty() {
            return lang.starts_with("en");
        }
        let accent = label(v, "accent");
        accent.is_empty() || ["american", "british", "australian", "irish", "english", "us", "uk"].iter().any(|a| accent.contains(a))
    };
    let of_gender: Vec<&Value> = voices.iter().filter(|v| label(v, "gender") == gender).collect();
    let english_ones: Vec<&&Value> = of_gender.iter().filter(|v| english(v)).collect();
    let young = if gender == "male" { english_ones.iter().find(|v| label(v, "age").contains("young")).copied() } else { None };
    let v = young.or_else(|| english_ones.first().copied()).copied().or_else(|| of_gender.first().copied()).or_else(|| voices.first())?;
    v["voice_id"].as_str().map(String::from)
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

// ── The tools ─────────────────────────────────────────────────────────────────

/// Asks the island (it knows the pills and the services' data) and waits for its answer.
pub fn ask_island(app: &AppHandle, kind: &str, input: Value) -> Option<String> {
    ask_island_waiting(app, kind, input, Duration::from_secs(5))
}

/// The same, waiting up to `wait` (the chat: as long as Claude works).
fn ask_island_waiting(app: &AppHandle, kind: &str, input: Value, wait: Duration) -> Option<String> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    pending().lock().unwrap().insert(id, tx);
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "voice-ask", json!({ "id": id, "kind": kind, "input": input }));
    let answer = rx.recv_timeout(wait).ok();
    pending().lock().unwrap().remove(&id);
    answer
}

/// Runs one of Claude's tools: what goes back to Claude, and whether something changed.
pub fn run_tool(app: &AppHandle, name: &str, input: &Value) -> (String, bool) {
    match name {
        "pills" => {
            let answer = ask_island(app, "pills", input.clone()).unwrap_or_else(|| "the island did not answer".into());
            let acted = input["action"].as_str() != Some("list") && !answer.contains("unknown");
            (answer, acted)
        }
        "service_info" => (ask_island(app, "info", json!({})).unwrap_or_else(|| "no data".into()), false),
        "music" => music(app, input),
        "open_app" => open_app(input["name"].as_str().unwrap_or("")),
        "prepare_email" => {
            let to = input["recipient"].as_str().unwrap_or("").trim().to_string();
            if !to.contains('@') {
                return ("No email address: ask the user to spell it.".into(), false);
            }
            let enc = |s: &str| -> String {
                s.bytes()
                    .map(|b| match b {
                        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                        _ => format!("%{b:02X}"),
                    })
                    .collect()
            };
            let url = format!(
                "mailto:{}?subject={}&body={}",
                enc(&to).replace("%40", "@"),
                enc(input["subject"].as_str().unwrap_or("")),
                enc(input["body"].as_str().unwrap_or(""))
            );
            crate::platform::open_url(&url);
            (format!("The mail app opened with the email to {to}. Nothing is sent until the user sends it."), true)
        }
        _ => (format!("unknown tool {name}"), false),
    }
}

fn music(app: &AppHandle, input: &Value) -> (String, bool) {
    let action = input["action"].as_str().unwrap_or("");
    let (control, value) = match action {
        "play_pause" => ("playPause", None),
        "next" => ("next", None),
        "previous" => ("previous", None),
        "set_volume" => ("volume", Some(input["volume"].as_f64().unwrap_or(50.0).clamp(0.0, 100.0))),
        "search" => {
            let q = input["query"].as_str().unwrap_or("").trim();
            if q.is_empty() {
                return ("missing query".into(), false);
            }
            let encoded: String = q.chars().map(|c| if c.is_alphanumeric() { c.to_string() } else { "%20".into() }).collect();
            crate::platform::open_url(&format!("spotify:search:{encoded}"));
            return (format!("Spotify opened on a search for {q}: the user picks what to play."), true);
        }
        _ => return ("unknown action".into(), false),
    };
    let app2 = app.clone();
    let done = tauri::async_runtime::block_on(crate::spotify::spotify_control(app2, control.into(), value));
    if done {
        ("done".into(), true)
    } else {
        ("Spotify is not running or not playing: tell the user to open it.".into(), false)
    }
}

/// Opens an app whose name matches: a Start menu shortcut on Windows, a
/// .desktop entry on Linux. Nothing else is ever run.
fn open_app(name: &str) -> (String, bool) {
    let wanted = wake::normalize(name);
    if wanted.is_empty() {
        return ("no name".into(), false);
    }
    match find_app(&wanted) {
        Some((label, launch)) => {
            launch();
            (format!("{label} opened"), true)
        }
        None => (format!("No app named {name} on this computer."), false),
    }
}

#[cfg(windows)]
fn find_app(wanted: &str) -> Option<(String, Box<dyn FnOnce()>)> {
    let mut roots = Vec::new();
    if let Some(p) = std::env::var_os("APPDATA") {
        roots.push(std::path::PathBuf::from(p).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Some(p) = std::env::var_os("ProgramData") {
        roots.push(std::path::PathBuf::from(p).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    let mut best: Option<(usize, std::path::PathBuf)> = None;
    let mut stack = roots;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                let stem = wake::normalize(&path.file_stem().unwrap_or_default().to_string_lossy());
                let score = if stem == wanted { 0 } else if stem.starts_with(wanted) { 1 } else if stem.contains(wanted) { 2 } else { continue };
                if best.as_ref().is_none_or(|(s, p)| score < *s || (score == *s && path.as_os_str().len() < p.as_os_str().len())) {
                    best = Some((score, path));
                }
            }
        }
    }
    let (_, path) = best?;
    let label = path.file_stem()?.to_string_lossy().to_string();
    Some((label, Box::new(move || crate::platform::reveal_folder(&path.to_string_lossy()))))
}

#[cfg(target_os = "linux")]
fn find_app(wanted: &str) -> Option<(String, Box<dyn FnOnce()>)> {
    let mut dirs = vec![std::path::PathBuf::from("/usr/share/applications"), std::path::PathBuf::from("/var/lib/flatpak/exports/share/applications")];
    dirs.push(crate::platform::home_dir().join(".local/share/applications"));
    dirs.push(crate::platform::home_dir().join(".local/share/flatpak/exports/share/applications"));
    let mut best: Option<(usize, String, String)> = None;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if text.lines().any(|l| l.trim() == "NoDisplay=true") {
                continue;
            }
            let Some(app_name) = text.lines().find_map(|l| l.strip_prefix("Name=")).map(str::trim) else { continue };
            let n = wake::normalize(app_name);
            let score = if n == wanted { 0 } else if n.starts_with(wanted) { 1 } else if n.contains(wanted) { 2 } else { continue };
            let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            if best.as_ref().is_none_or(|(s, _, _)| score < *s) {
                best = Some((score, app_name.to_string(), id));
            }
        }
    }
    let (_, label, id) = best?;
    Some((label, Box::new(move || {
        let _ = std::process::Command::new("gtk-launch").arg(&id).spawn();
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b""), "");
    }

    #[test]
    fn only_the_beginning_of_an_answer_is_said() {
        assert_eq!(speakable("Готово. Тесты прошли. Изменено 4 файла."), "Готово. Тесты прошли.");
        assert_eq!(speakable("## Итог\n\n**Готово**: см. `lib.rs`.\n```rust\nfn main(){}\n```\nЕщё."), "Итог Готово: см. lib.rs. Ещё.");
        assert_eq!(speakable("Смотри [документацию](https://x.y/z) здесь."), "Смотри документацию здесь.");
        assert_eq!(speakable("```\ncode only\n```"), "");
    }

    #[test]
    fn elevenlabs_refusals_read_as_words() {
        let body = r#"{"detail":{"status":"payment_required","message":"Free users cannot use library voices via the API."}}"#;
        assert_eq!(
            refusal(reqwest::StatusCode::PAYMENT_REQUIRED, body),
            "HTTP 402 · payment_required · Free users cannot use library voices via the API."
        );
        assert_eq!(refusal(reqwest::StatusCode::UNAUTHORIZED, "nope"), "HTTP 401");
    }

    #[test]
    fn a_voice_id_or_its_link() {
        assert_eq!(valid_voice_id("rQOBu7YxCDxGiFdTm28w"), Some("rQOBu7YxCDxGiFdTm28w"));
        assert_eq!(valid_voice_id("https://elevenlabs.io/voices/rQOBu7YxCDxGiFdTm28w/"), Some("rQOBu7YxCDxGiFdTm28w"));
        assert_eq!(valid_voice_id("https://elevenlabs.io/voices/rQOBu7YxCDxGiFdTm28w?x=1"), Some("rQOBu7YxCDxGiFdTm28w"));
        assert_eq!(valid_voice_id(""), None);
        assert_eq!(valid_voice_id("not a voice"), None);
    }

    #[test]
    fn the_voice_is_picked_like_on_the_mac() {
        let voices: Vec<Value> = serde_json::from_str(r#"[
            {"voice_id":"a","labels":{"gender":"male","accent":"american","age":"middle aged"}},
            {"voice_id":"b","labels":{"gender":"male","accent":"british","age":"young"}},
            {"voice_id":"c","labels":{"gender":"female","accent":"american"}},
            {"voice_id":"d","labels":{"gender":"female","language":"fr"}}
        ]"#).unwrap();
        assert_eq!(pick_voice(&voices, "male").as_deref(), Some("b"));
        assert_eq!(pick_voice(&voices, "female").as_deref(), Some("c"));
        assert_eq!(pick_voice(&[], "female"), None);
    }
}

/// With the models downloaded: `COUCOU_TEST_WAKE_WAV=… cargo test -- --ignored wake_in`.
#[cfg(test)]
mod with_models {
    #[test]
    #[ignore]
    fn hears_the_wake_phrase_in_a_recording() {
        let path = std::env::var("COUCOU_TEST_WAKE_WAV").expect("COUCOU_TEST_WAKE_WAV");
        let bytes = std::fs::read(&path).unwrap();
        let rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let pcm: Vec<f32> = bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        let mut r = crate::speech::audio::Resampler::new(rate);
        let mut samples = Vec::new();
        r.process(&pcm, &mut samples);
        let mut vad = super::wake::Vad::default();
        let mut heard = Vec::new();
        for chunk in samples.chunks(1600) {
            for segment in vad.feed(chunk) {
                let text = crate::speech::transcribe(&segment).expect("models").text;
                println!("segment {:.2} s: {text}", segment.len() as f32 / 16_000.0);
                heard.push(super::wake::after_wake(&text));
            }
        }
        println!("{heard:?}");
        assert!(heard.iter().any(|h| h.as_deref().is_some_and(|q| !q.is_empty())));
    }
}
