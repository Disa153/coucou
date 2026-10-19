// The real chat behind the phone link (chat.rs holds the rules): the same provider code as the
// island's chat, with the phone's own conversation. The key is read inside `claude`, `openai_compat`
// or `local_chat` from the credential store; nothing in this file ever holds one.
//
// Failures are sorted into a few kinds here and only the kind goes on: a provider's message may
// contain part of a key, so it is neither forwarded nor logged.

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tokio::sync::mpsc::UnboundedSender;

use super::chat::{BoxFuture, ChatBackend, ChatConfig, ChatFail, ModelOption};
use crate::chat::Chat;
use crate::settings::Settings;
use crate::{chat, claude, log, openai_compat, secrets, Shared};

/// Most models the user can allow, and the longest model name taken.
pub const MAX_ALLOWED: usize = 50;
const MAX_MODEL_CHARS: usize = 200;

const PROVIDERS: [&str; 10] = ["anthropic", "claude-code", "gemini-cli", "google", "openai", "openrouter", "deepseek", "ollama", "lmstudio", "custom"];

pub struct AppChat {
    app: AppHandle,
    chat: Arc<Chat>,
}

impl AppChat {
    pub fn new(app: AppHandle) -> Self {
        Self { app, chat: Arc::new(Chat::for_phone()) }
    }

    fn settings(&self) -> Settings {
        self.app.state::<Shared>().settings.lock().unwrap().clone()
    }
}

impl ChatBackend for AppChat {
    fn config(&self) -> ChatConfig {
        let s = self.settings();
        ChatConfig { enabled: s.phone_chat, allowed: options(&s.phone_chat_models) }
    }

    fn send(&self, model_id: &str, text: String, deltas: UnboundedSender<String>) -> BoxFuture<Result<String, ChatFail>> {
        let settings = self.settings();
        let chat = self.chat.clone();
        let id = model_id.to_string();
        Box::pin(async move {
            let Some((provider, model)) = parse_model_id(&id) else { return Err(ChatFail::Internal) };
            if let Some(key) = key_name(provider) {
                if secrets::get(key).is_none() {
                    return Err(ChatFail::NoKey);
                }
            }
            let result = chat::send_for_phone(&settings, &chat, provider, model, text, move |so_far| {
                let _ = deltas.send(so_far);
            })
            .await;
            match result {
                Ok(reply) => Ok(reply.text),
                Err(err) => {
                    let kind = classify(is_local(provider), &err);
                    // The kind only: the text may echo a key.
                    log::line(format!("phone link: chat via {provider} failed ({kind:?})"));
                    Err(kind)
                }
            }
        })
    }

    fn reset(&self) {
        self.chat.reset();
    }

    /// Into `.coucou-phone` in the folder Claude Code and Gemini CLI work in, so they can read the
    /// photo without asking; the paths are relative to that folder. Photos older than a day go.
    fn save_images(&self, images: Vec<Vec<u8>>) -> Result<Vec<String>, ChatFail> {
        let dir = crate::claude_code::work_dir(&self.settings()).join(PHOTO_DIR);
        save_photos(&dir, &images, std::time::SystemTime::now()).map_err(|e| {
            log::line(format!("phone link: could not save a photo ({e})"));
            ChatFail::Internal
        })
    }
}

/// Where the phone's photos go, inside the CLI's working folder.
pub const PHOTO_DIR: &str = ".coucou-phone";
const PHOTO_KEEP: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Writes each photo as `photo-<time>-<n>.<ext>` in `dir` and returns `.coucou-phone/<name>` for each.
pub fn save_photos(dir: &std::path::Path, images: &[Vec<u8>], now: std::time::SystemTime) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(dir)?;
    // Yesterday's photos are not needed any more.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age > PHOTO_KEEP);
            if name.starts_with("photo-") && old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let stamp = now.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mut paths = Vec::new();
    for (n, bytes) in images.iter().enumerate() {
        let ext = super::chat::image_ext(bytes).ok_or_else(|| std::io::Error::other("not a JPEG or PNG"))?;
        let name = format!("photo-{stamp}-{n}.{ext}");
        std::fs::write(dir.join(&name), bytes)?;
        paths.push(format!("{PHOTO_DIR}/{name}"));
    }
    Ok(paths)
}

// ── Pure helpers (tested below) ───────────────────────────────────────────────

fn is_local(provider: &str) -> bool {
    matches!(provider, "ollama" | "lmstudio" | "custom")
}

/// The credential store entry a provider's key lives in; None for the model servers.
fn key_name(provider: &str) -> Option<&'static str> {
    if provider == chat::ANTHROPIC {
        return Some(claude::KEY);
    }
    openai_compat::provider(provider).map(|p| p.key)
}

pub fn provider_name(provider: &str) -> String {
    match provider {
        "anthropic" => "Anthropic".into(),
        "claude-code" => "Claude Code".into(),
        "gemini-cli" => "Gemini CLI".into(),
        "google" => "Google".into(),
        "openai" => "OpenAI".into(),
        "openrouter" => "OpenRouter".into(),
        "deepseek" => "DeepSeek".into(),
        "ollama" => "Ollama".into(),
        "lmstudio" => "LM Studio".into(),
        _ => "Custom server".into(),
    }
}

/// "provider/model" with a provider this build knows and a plain model name; the model may contain
/// slashes ("openrouter/auto" is the model of the "openrouter" provider written "openrouter/openrouter/auto").
pub fn parse_model_id(id: &str) -> Option<(&str, &str)> {
    let (provider, model) = id.split_once('/')?;
    let model = model.trim();
    if !PROVIDERS.contains(&provider) || model.is_empty() || model.chars().count() > MAX_MODEL_CHARS || model.chars().any(char::is_control) {
        return None;
    }
    Some((provider, model))
}

/// The user's list, made safe: valid ids only, no duplicates, at most MAX_ALLOWED.
pub fn clean_models(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if parse_model_id(id).is_some() && !out.contains(id) && out.len() < MAX_ALLOWED {
            out.push(id.clone());
        }
    }
    out
}

pub fn options(ids: &[String]) -> Vec<ModelOption> {
    clean_models(ids)
        .into_iter()
        .filter_map(|id| {
            let (provider, model) = parse_model_id(&id)?;
            let label = format!("{} · {}", provider_name(provider), model);
            Some(ModelOption { provider: provider.to_string(), label, id })
        })
        .collect()
}

/// Which kind of failure an error message from `chat::send_for_phone` stands for. The messages are
/// written for people (and translated), so this looks only at what does not change with the language:
/// an HTTP status in the text. A model server that answers with no status at all is one that did not answer.
pub fn classify(local: bool, err: &str) -> ChatFail {
    let has = |code: &str| err.split(|c: char| !c.is_ascii_digit()).any(|w| w == code);
    if has("401") || has("403") {
        return ChatFail::Auth;
    }
    let has_status = err.split(|c: char| !c.is_ascii_digit()).any(|w| w.len() == 3 && w.starts_with(['4', '5']));
    if local && !has_status {
        return ChatFail::Unreachable;
    }
    if err.starts_with("Network error") {
        return ChatFail::Unreachable;
    }
    ChatFail::Provider
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photos_are_saved_with_their_type_and_old_ones_go() {
        let dir = std::env::temp_dir().join(format!("coucou-photos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("photo-old.jpg"), b"x").unwrap();
        std::fs::write(dir.join("keep.txt"), b"x").unwrap();
        // two days later, the old photo is gone; other files stay
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2 * 24 * 3600);
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        let paths = save_photos(&dir, &[vec![0xFF, 0xD8, 0xFF, 1], png.clone()], later).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths[0].starts_with(".coucou-phone/photo-") && paths[0].ends_with("-0.jpg"), "{paths:?}");
        assert!(paths[1].ends_with("-1.png"));
        let name = paths[1].trim_start_matches(".coucou-phone/");
        assert_eq!(std::fs::read(dir.join(name)).unwrap(), png);
        assert!(!dir.join("photo-old.jpg").exists());
        assert!(dir.join("keep.txt").exists());
        assert!(save_photos(&dir, &[b"GIF89a".to_vec()], later).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn model_ids_need_a_known_provider_and_a_plain_model() {
        assert_eq!(parse_model_id("openai/gpt-4o"), Some(("openai", "gpt-4o")));
        assert_eq!(parse_model_id("openrouter/openrouter/auto"), Some(("openrouter", "openrouter/auto")));
        assert_eq!(parse_model_id("ollama/llama3.2:latest"), Some(("ollama", "llama3.2:latest")));
        for bad in ["", "openai", "openai/", "openai/ ", "nope/gpt", "OpenAI/gpt", "openai/a\nb", &format!("openai/{}", "x".repeat(201))] {
            assert_eq!(parse_model_id(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_allowed_list_is_cleaned_and_capped() {
        assert_eq!(clean_models(&ids(&["openai/gpt-4o", "bad", "openai/gpt-4o", "google/gemini"])), ids(&["openai/gpt-4o", "google/gemini"]));
        let many: Vec<String> = (0..80).map(|i| format!("openai/m{i}")).collect();
        assert_eq!(clean_models(&many).len(), MAX_ALLOWED);
        assert!(clean_models(&[]).is_empty());
    }

    #[test]
    fn what_the_phone_is_told_about_a_model_is_its_id_provider_and_label_only() {
        let o = options(&ids(&["anthropic/claude-x", "lmstudio/qwen"]));
        assert_eq!(o[0], ModelOption { id: "anthropic/claude-x".into(), provider: "anthropic".into(), label: "Anthropic · claude-x".into() });
        assert_eq!(o[1].label, "LM Studio · qwen");
    }

    #[test]
    fn failures_are_sorted_by_status_and_never_by_wording() {
        assert_eq!(classify(false, "Claude API 401 Unauthorized: invalid x-api-key"), ChatFail::Auth);
        assert_eq!(classify(false, "OpenAI rejected the API key (403). Check it in Settings."), ChatFail::Auth);
        assert_eq!(classify(false, "OpenAI rate limit reached (429): slow down"), ChatFail::Provider);
        assert_eq!(classify(false, "Claude API 500 Internal Server Error: boom"), ChatFail::Provider);
        assert_eq!(classify(false, "Network error: connection refused"), ChatFail::Unreachable);
        assert_eq!(classify(true, "Cannot reach http://127.0.0.1:11434. Is the server running?"), ChatFail::Unreachable);
        assert_eq!(classify(true, "Ollama 500: model crashed"), ChatFail::Provider);
        // 4010 or 14012 are not 401
        assert_eq!(classify(false, "something 14012 happened"), ChatFail::Provider);
    }

    #[test]
    fn keys_are_found_by_provider() {
        assert_eq!(key_name("anthropic"), Some("anthropic-api-key"));
        assert_eq!(key_name("openai"), Some("openai-api-key"));
        assert_eq!(key_name("ollama"), None);
        assert!(is_local("custom") && !is_local("openai"));
    }
}
