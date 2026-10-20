// The beta chat for the phone (cap `betachat`), through this computer: the
// phone has no GitHub sign-in of its own, so it reads and writes the chat as
// the account signed in here (betachat.rs). Offered only while that account
// may see the chat.
//
// Phone → computer
//   betaChatGet    {full}                          → betaChatPage parts
//   betaChatUp     {fid, part, parts, data}        a part of a file for the next send
//   betaChatSend   {key, text, files:[{fid,name,mime}]} → betaChatSent {key, message | error}
//   betaChatDelete {id}                            → betaChatDeleted {id, error?}
//   betaChatFile   {url}                           → betaChatDown {url, part, parts, data} … | {url, error}
// Computer → phone, unasked
//   betaChatNews   {unread, login, text}           someone else wrote (the phone's notification)
//   betaChatChanged                                 read again
//
// Every line stays under the link's 64 KiB: files go in parts of PART_CHARS,
// the page in parts that fit.

use std::collections::HashMap;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::hub::Out;
use crate::betachat::{ChatNews, ChatPage, OutFile, FILES_MAX, FILE_MAX};

/// Base64 characters in one part of a file, either way.
pub const PART_CHARS: usize = 40_000;
/// A page part is cut before it reaches this many bytes.
pub const PAGE_PART_BYTES: usize = 48_000;
const PARTS_MAX: usize = FILE_MAX.div_ceil(PART_CHARS * 3 / 4) + 2;

/// The files being sent, by the phone's id (at most FILES_MAX at a time).
#[derive(Default)]
pub struct Uploads {
    files: HashMap<String, Vec<Option<String>>>,
}

impl Uploads {
    /// One part in; false when it breaks the limits (the upload is dropped).
    pub fn put(&mut self, fid: &str, part: usize, parts: usize, data: &str) -> bool {
        let ok_id = !fid.is_empty() && fid.len() <= 64 && fid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        let ok_data = data.len() <= PART_CHARS + 4 && data.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
        if !ok_id || !ok_data || parts == 0 || parts > PARTS_MAX || part >= parts {
            self.files.remove(fid);
            return false;
        }
        if !self.files.contains_key(fid) && self.files.len() >= FILES_MAX {
            return false;
        }
        let slots = self.files.entry(fid.to_string()).or_insert_with(|| vec![None; parts]);
        if slots.len() != parts {
            self.files.remove(fid);
            return false;
        }
        slots[part] = Some(data.to_string());
        true
    }

    /// The whole file, once every part came; it leaves the buffer.
    pub fn take(&mut self, fid: &str) -> Option<String> {
        let slots = self.files.get(fid)?;
        if slots.iter().any(Option::is_none) {
            return None;
        }
        let data: String = self.files.remove(fid)?.into_iter().flatten().collect();
        (crate::betachat::b64_len(&data) <= FILE_MAX).then_some(data)
    }
}

/// The page as lines that each fit the link: messages are kept whole, in order.
pub fn page_lines(page: &ChatPage) -> Vec<String> {
    let mut chunks: Vec<Vec<Value>> = vec![Vec::new()];
    let mut size = 0;
    for m in &page.messages {
        let v = serde_json::to_value(m).unwrap_or(Value::Null);
        let n = v.to_string().len();
        if size + n > PAGE_PART_BYTES && !chunks.last().unwrap().is_empty() {
            chunks.push(Vec::new());
            size = 0;
        }
        size += n;
        chunks.last_mut().unwrap().push(v);
    }
    let parts = chunks.len();
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, messages)| {
            json!({
                "type": "betaChatPage", "part": i, "parts": parts, "owner": page.owner, "page": page.page,
                "messages": messages,
            })
            .to_string()
        })
        .collect()
}

/// What the phone's notification says.
pub fn news_line(n: &ChatNews) -> String {
    let (login, text) = n.last.as_ref().map(|m| (m.login.clone(), m.text.chars().take(200).collect::<String>())).unwrap_or_default();
    json!({ "type": "betaChatNews", "unread": n.unread, "login": login, "text": text }).to_string()
}

/// A file as lines of at most PART_CHARS characters.
pub fn file_lines(url: &str, data: &str) -> Vec<String> {
    let parts = data.len().div_ceil(PART_CHARS).max(1);
    (0..parts)
        .map(|i| {
            let piece = &data[i * PART_CHARS..((i + 1) * PART_CHARS).min(data.len())];
            json!({ "type": "betaChatDown", "url": url, "part": i, "parts": parts, "data": piece }).to_string()
        })
        .collect()
}

fn code(e: crate::reviews::ReviewError) -> Value {
    serde_json::to_value(e).unwrap_or(Value::String("network".into()))
}

async fn say(tx: &mpsc::Sender<Out>, line: String) -> bool {
    tx.send(Out::Line(line.into())).await.is_ok()
}

/// One message from the phone. The slow part (GitHub) runs on its own task.
pub fn handle(tx: &mpsc::Sender<Out>, msg: &Value, uploads: &mut Uploads) {
    let tx = tx.clone();
    match msg["type"].as_str() {
        Some("betaChatGet") => {
            let full = msg["full"].as_bool().unwrap_or(false);
            tokio::spawn(async move {
                match crate::betachat::phone_page(full).await {
                    Ok(page) => {
                        for line in page_lines(&page) {
                            if !say(&tx, line).await {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = say(&tx, json!({ "type": "betaChatPage", "part": 0, "parts": 1, "error": code(e), "messages": [] }).to_string()).await;
                    }
                }
            });
        }
        Some("betaChatUp") => {
            let fid = msg["fid"].as_str().unwrap_or("");
            let part = msg["part"].as_u64().unwrap_or(u64::MAX) as usize;
            let parts = msg["parts"].as_u64().unwrap_or(0) as usize;
            uploads.put(fid, part, parts, msg["data"].as_str().unwrap_or(""));
        }
        Some("betaChatSend") => {
            let key: String = msg["key"].as_str().unwrap_or("").chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(64).collect();
            let text = msg["text"].as_str().unwrap_or("").to_string();
            let mut files = Vec::new();
            let mut missing = false;
            for f in msg["files"].as_array().into_iter().flatten().take(FILES_MAX) {
                let fid = f["fid"].as_str().unwrap_or("");
                match uploads.take(fid) {
                    Some(data) => files.push(OutFile {
                        name: f["name"].as_str().unwrap_or("file").to_string(),
                        mime: f["mime"].as_str().unwrap_or("application/octet-stream").to_string(),
                        data,
                    }),
                    None => missing = true,
                }
            }
            tokio::spawn(async move {
                let line = if missing {
                    json!({ "type": "betaChatSent", "key": key, "error": "tooBig" })
                } else {
                    match crate::betachat::phone_send(&text, &files).await {
                        Ok(m) => json!({ "type": "betaChatSent", "key": key, "message": m }),
                        Err(e) => json!({ "type": "betaChatSent", "key": key, "error": code(e) }),
                    }
                };
                let _ = say(&tx, line.to_string()).await;
            });
        }
        Some("betaChatDelete") => {
            let id = msg["id"].as_u64().unwrap_or(0);
            tokio::spawn(async move {
                let line = match crate::betachat::phone_delete(id).await {
                    Ok(()) => json!({ "type": "betaChatDeleted", "id": id }),
                    Err(e) => json!({ "type": "betaChatDeleted", "id": id, "error": code(e) }),
                };
                let _ = say(&tx, line.to_string()).await;
            });
        }
        Some("betaChatFile") => {
            let url = msg["url"].as_str().unwrap_or("").to_string();
            tokio::spawn(async move {
                match crate::betachat::phone_file(&url).await {
                    Ok(data) => {
                        for line in file_lines(&url, &data) {
                            if !say(&tx, line).await {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = say(&tx, json!({ "type": "betaChatDown", "url": url, "error": code(e) }).to_string()).await;
                    }
                }
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::betachat::Message;

    fn msg(id: u64, text: &str) -> Message {
        Message { id, login: "a".into(), text: text.into(), build: None, files: vec![], at: 0, owner: false, mine: false, updated: String::new() }
    }

    #[test]
    fn a_page_fits_the_link_and_keeps_every_message_in_order() {
        let page = ChatPage { messages: (0..100).map(|i| msg(i, &"я".repeat(1900))).collect(), page: "p".into(), owner: true, build: 1 };
        let lines = page_lines(&page);
        assert!(lines.len() > 1);
        let mut ids = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            assert!(l.len() < super::super::server::MAX_LINE, "part {i} is {} bytes", l.len());
            let v: Value = serde_json::from_str(l).unwrap();
            assert_eq!(v["part"], i);
            assert_eq!(v["parts"], lines.len());
            ids.extend(v["messages"].as_array().unwrap().iter().map(|m| m["id"].as_u64().unwrap()));
        }
        assert_eq!(ids, (0..100).collect::<Vec<_>>());
        // an empty chat is one empty part
        let empty = page_lines(&ChatPage { messages: vec![], page: String::new(), owner: false, build: 0 });
        assert_eq!(empty.len(), 1);
    }

    #[test]
    fn a_file_comes_back_whole_from_its_parts() {
        let data = "QUJD".repeat(25_000); // 100 000 base64 characters
        let lines = file_lines("u", &data);
        assert_eq!(lines.len(), 3);
        let mut up = Uploads::default();
        for l in &lines {
            assert!(l.len() < super::super::server::MAX_LINE);
            let v: Value = serde_json::from_str(l).unwrap();
            assert!(up.put("f1", v["part"].as_u64().unwrap() as usize, v["parts"].as_u64().unwrap() as usize, v["data"].as_str().unwrap()));
        }
        assert_eq!(up.take("f1").as_deref(), Some(data.as_str()));
        assert_eq!(up.take("f1"), None, "taken once");
    }

    #[test]
    fn uploads_keep_to_their_limits() {
        let mut up = Uploads::default();
        assert!(!up.put("../x", 0, 1, "QQ=="));
        assert!(!up.put("a", 0, 1, "not base64!"));
        assert!(!up.put("a", 2, 2, "QQ=="));
        assert!(!up.put("a", 0, PARTS_MAX + 1, "QQ=="));
        assert!(up.put("a", 0, 2, "QQ=="));
        assert_eq!(up.take("a"), None, "a part is missing");
        for f in ["b", "c", "d"] {
            assert!(up.put(f, 0, 1, "QQ=="));
        }
        assert!(!up.put("e", 0, 1, "QQ=="), "at most FILES_MAX files at once");
    }
}
