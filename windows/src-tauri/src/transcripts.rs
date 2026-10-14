// Chat sessions saved as Markdown, one file per conversation, so they can be read
// again later: by the user, or by Claude in another chat once the folder is shared
// with it. On by default, in Documents\Coucou\sessions (Settings → Saved chats).
//
// A file is opened on a conversation's first answered turn and every later turn is
// appended to it; "New chat" starts the next file. Only what the user asked and what
// came back is written: never a key, never a file's contents. The island's chat and
// the phone's each have their own conversation, so their own files.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use crate::platform::{self, LocalTime};
use crate::settings::Settings;

/// Where conversations go, or None when saving is off. Set at launch and on every settings save.
static FOLDER: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

/// The longest file name part taken from the first question.
const SLUG_CHARS: usize = 40;

/// Documents\Coucou\sessions (or ~/Documents/Coucou/sessions).
pub fn default_dir() -> PathBuf {
    platform::home_dir().join("Documents").join("Coucou").join("sessions")
}

/// The folder chosen in the settings, or None when saving is off.
pub fn folder_for(settings: &Settings) -> Option<PathBuf> {
    if !settings.save_chats {
        return None;
    }
    let set = settings.chats_dir.trim();
    Some(if set.is_empty() { default_dir() } else { PathBuf::from(set) })
}

pub fn configure(settings: &Settings) {
    *FOLDER.lock().unwrap_or_else(|e| e.into_inner()) = folder_for(settings);
}

fn folder() -> Option<PathBuf> {
    FOLDER.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// A few words of the first question, safe in a file name on every system.
pub fn slug(question: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in question.chars() {
        if out.chars().count() >= SLUG_CHARS {
            break;
        }
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "chat".to_string() } else { out }
}

fn stamp(t: &LocalTime) -> String {
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

fn clock(t: &LocalTime) -> String {
    format!("{:02}:{:02}", t.hour, t.minute)
}

/// Who answered, as the file says it.
pub fn provider_label(provider: &str) -> &str {
    match provider {
        "" | "anthropic" => "Anthropic",
        "claude-code" => "Claude Code",
        "openai" => "OpenAI",
        "google" => "Google",
        "openrouter" => "OpenRouter",
        "ollama" => "Ollama",
        "lmstudio" => "LM Studio",
        "custom" => "Custom server",
        other => other,
    }
}

/// The text of one turn.
pub fn turn_text(t: &LocalTime, provider: &str, session: Option<&str>, question: &str, answer: &str) -> String {
    let mut s = format!("## You · {}\n\n{}\n\n## Mochi · {}", clock(t), question.trim(), provider_label(provider));
    if let Some(id) = session {
        s.push_str(&format!(" · session `{id}`"));
    }
    s.push_str(&format!("\n\n{}\n\n", answer.trim()));
    s
}

/// The file's first lines.
pub fn header(t: &LocalTime, origin: &str) -> String {
    let from = if origin == "phone" { "phone" } else { "computer" };
    format!(
        "# Coucou chat — {:04}-{:02}-{:02} {:02}:{:02}\n\nFrom the {from}. Each answer names who gave it; a Claude Code \
answer also names its session, which `claude --resume <id>` opens again.\n\n",
        t.year, t.month, t.day, t.hour, t.minute
    )
}

fn unique_path(dir: &Path, base: &str) -> PathBuf {
    let first = dir.join(format!("{base}.md"));
    if !first.exists() {
        return first;
    }
    (2..1000).map(|n| dir.join(format!("{base}-{n}.md"))).find(|p| !p.exists()).unwrap_or(first)
}

/// Appends a finished turn to the conversation's file, opening one first if it has none.
/// `file` is the conversation's own (kept in chat.rs, cleared by "New chat").
pub fn record(
    file: &mut Option<PathBuf>,
    origin: &str,
    provider: &str,
    session: Option<&str>,
    question: &str,
    answer: &str,
) {
    let Some(dir) = folder() else { return };
    let t = platform::local_time();
    let mut text = String::new();
    // A file from another folder (the setting changed mid-conversation) is left as it is.
    if file.as_ref().map(|p| p.parent() != Some(dir.as_path())).unwrap_or(true) {
        if let Err(err) = std::fs::create_dir_all(&dir) {
            crate::log::line(format!("saved chats: cannot create {}: {err}", dir.display()));
            return;
        }
        let tag = if origin == "phone" { "_phone" } else { "" };
        *file = Some(unique_path(&dir, &format!("{}{tag}_{}", stamp(&t), slug(question))));
        text.push_str(&header(&t, origin));
    }
    text.push_str(&turn_text(&t, provider, session, question, answer));
    let Some(path) = file.as_ref() else { return };
    let written = OpenOptions::new().create(true).append(true).open(path).and_then(|mut f| f.write_all(text.as_bytes()));
    if let Err(err) = written {
        crate::log::line(format!("saved chats: cannot write {}: {err}", path.display()));
    }
}

/// Settings → Saved chats → Open folder.
#[tauri::command]
pub fn chats_reveal(dir: String) {
    let dir = dir.trim();
    let path = if dir.is_empty() { default_dir() } else { PathBuf::from(dir) };
    let _ = std::fs::create_dir_all(&path);
    platform::reveal_folder(&path.to_string_lossy());
}

/// The folder used when the setting is empty, for the settings field's placeholder.
#[tauri::command]
pub fn chats_default_dir() -> String {
    default_dir().to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> LocalTime {
        LocalTime { year: 2026, month: 10, day: 14, hour: 9, minute: 5, second: 7 }
    }

    #[test]
    fn a_file_name_takes_a_few_safe_words_of_the_question() {
        assert_eq!(slug("Как подключить iPhone к Coucou?"), "как-подключить-iphone-к-coucou");
        assert_eq!(slug("  ../../etc/passwd  "), "etc-passwd");
        assert_eq!(slug("???"), "chat");
        assert!(slug(&"word ".repeat(40)).chars().count() <= SLUG_CHARS);
        assert_eq!(stamp(&at()), "2026-10-14_09-05-07");
    }

    #[test]
    fn a_turn_names_who_answered_and_the_claude_code_session() {
        let id = "0b1c2d3e-0000-4000-8000-000000000000";
        let s = turn_text(&at(), "claude-code", Some(id), " hi ", " hello ");
        assert_eq!(s, format!("## You · 09:05\n\nhi\n\n## Mochi · Claude Code · session `{id}`\n\nhello\n\n"));
        assert!(turn_text(&at(), "openai", None, "q", "a").contains("## Mochi · OpenAI\n\na"));
        assert!(header(&at(), "phone").contains("From the phone"));
    }

    #[test]
    fn saving_follows_the_settings() {
        let mut s = Settings::default();
        assert_eq!(folder_for(&s), Some(default_dir()));
        s.chats_dir = "  /tmp/x ".into();
        assert_eq!(folder_for(&s), Some(PathBuf::from("/tmp/x")));
        s.save_chats = false;
        assert_eq!(folder_for(&s), None);
    }

    #[test]
    fn turns_go_into_one_file_per_conversation() {
        let dir = std::env::temp_dir().join(format!("coucou-transcripts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        *FOLDER.lock().unwrap() = Some(dir.clone());
        let mut file = None;
        record(&mut file, "island", "claude-code", None, "First question", "First answer");
        record(&mut file, "island", "claude-code", None, "Second", "Again");
        let path = file.clone().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# Coucou chat"));
        assert_eq!(text.matches("## You").count(), 2);
        assert!(path.file_name().unwrap().to_string_lossy().ends_with("_first-question.md"));
        // "New chat": the next turn opens another file.
        let mut next = None;
        record(&mut next, "phone", "anthropic", None, "First question", "x");
        assert_ne!(next, file);
        *FOLDER.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
