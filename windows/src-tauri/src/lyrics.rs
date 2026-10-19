//! The words of the song that plays, for the music card's Lyrics view
//! (src/views/lyrics.ts). Off until the user turns it on (Settings →
//! Integrations → Lyrics): the only network call is to LRCLIB (lrclib.net), an
//! open database of lyrics, with the song's title, artist, album and length.
//! What it answers is kept on this computer, so a song is asked for once.

use std::path::PathBuf;
use std::time::Duration;

use ring::digest;
use serde::{Deserialize, Serialize};

const API: &str = "https://lrclib.net/api";
const USER_AGENT: &str = "Coucou (https://github.com/Disa153/coucou)";
const LIMIT: usize = 512 * 1024;

/// What LRCLIB has for a song: timed lines (LRC), plain words, or neither.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
    pub instrumental: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

impl Record {
    fn lyrics(self) -> Lyrics {
        let keep = |s: Option<String>| s.filter(|s| !s.trim().is_empty());
        Lyrics { synced: keep(self.synced_lyrics), plain: keep(self.plain_lyrics), instrumental: self.instrumental }
    }
    fn has_words(&self) -> bool {
        self.synced_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty())
            || self.plain_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty())
    }
}

/// Of the search results, the one closest in length (within 4 s), timed lines first.
fn best(records: Vec<Record>, duration: f64) -> Option<Record> {
    let mut near: Vec<Record> = records
        .into_iter()
        .filter(|r| r.has_words() && (duration <= 0.0 || (r.duration - duration).abs() <= 4.0))
        .collect();
    near.sort_by(|a, b| {
        let timed = |r: &Record| r.synced_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty());
        timed(b).cmp(&timed(a)).then_with(|| {
            (a.duration - duration).abs().partial_cmp(&(b.duration - duration).abs()).unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    near.into_iter().next()
}

fn cache_path(title: &str, artist: &str, duration: f64) -> PathBuf {
    let key = format!("{}\n{}\n{}", artist.trim().to_lowercase(), title.trim().to_lowercase(), duration.round() as i64);
    let hash = digest::digest(&digest::SHA256, key.as_bytes());
    let name: String = hash.as_ref()[..12].iter().map(|b| format!("{b:02x}")).collect();
    crate::settings::local_dir().join("lyrics").join(format!("{name}.json"))
}

async fn get(url: reqwest::Url) -> Result<Option<Vec<u8>>, String> {
    let client = crate::updater::client(Duration::from_secs(15))?;
    let response = client.get(url).header("User-Agent", USER_AGENT).send().await.map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("LRCLIB answered {}", response.status()));
    }
    crate::net::read_capped(response, LIMIT).await.map(Some)
}

async fn lookup(title: &str, artist: &str, album: &str, duration: f64) -> Result<Lyrics, String> {
    let mut exact = reqwest::Url::parse(&format!("{API}/get")).map_err(|e| e.to_string())?;
    {
        let mut q = exact.query_pairs_mut();
        q.append_pair("track_name", title).append_pair("artist_name", artist);
        if !album.is_empty() {
            q.append_pair("album_name", album);
        }
        if duration > 0.0 {
            q.append_pair("duration", &format!("{}", duration.round() as i64));
        }
    }
    if let Some(body) = get(exact).await? {
        if let Ok(record) = serde_json::from_slice::<Record>(&body) {
            if record.has_words() || record.instrumental {
                return Ok(record.lyrics());
            }
        }
    }
    // Not under that exact album or length: the closest of a search.
    let mut search = reqwest::Url::parse(&format!("{API}/search")).map_err(|e| e.to_string())?;
    search.query_pairs_mut().append_pair("track_name", title).append_pair("artist_name", artist);
    let Some(body) = get(search).await? else { return Ok(Lyrics::default()) };
    let records: Vec<Record> = serde_json::from_slice(&body).unwrap_or_default();
    Ok(best(records, duration).map(Record::lyrics).unwrap_or_default())
}

/// The song's lyrics, from this computer if asked before, else from LRCLIB.
/// Err("off") while the user hasn't turned lyrics on.
#[tauri::command]
pub async fn lyrics_get(title: String, artist: String, album: String, duration: f64) -> Result<Lyrics, String> {
    if !crate::settings::load().lyrics {
        return Err("off".into());
    }
    let title: String = title.trim().chars().take(200).collect();
    let artist: String = artist.trim().chars().take(200).collect();
    let album: String = album.trim().chars().take(200).collect();
    if title.is_empty() {
        return Ok(Lyrics::default());
    }
    let path = cache_path(&title, &artist, duration);
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(kept) = serde_json::from_slice::<Lyrics>(&bytes) {
            return Ok(kept);
        }
    }
    let found = lookup(&title, &artist, &album, duration).await.map_err(|e| {
        crate::log::line(format!("lyrics: not reached ({e})"));
        e
    })?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec(&found) {
        let _ = std::fs::write(&path, json);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(duration: f64, synced: Option<&str>, plain: Option<&str>) -> Record {
        Record { duration, instrumental: false, plain_lyrics: plain.map(str::to_string), synced_lyrics: synced.map(str::to_string) }
    }

    #[test]
    fn the_closest_search_result_with_timed_lines_first() {
        let picked = best(vec![rec(200.0, None, Some("a")), rec(203.0, Some("[00:01.00] b"), None), rec(260.0, Some("[00:01.00] c"), None)], 201.0).unwrap();
        assert_eq!(picked.synced_lyrics.as_deref(), Some("[00:01.00] b"));
        assert!(best(vec![rec(260.0, Some("x"), None)], 201.0).is_none());
        assert!(best(vec![rec(201.0, Some("  "), Some(""))], 201.0).is_none());
    }

    #[test]
    fn a_song_is_kept_under_one_name() {
        assert_eq!(cache_path("Song", "Artist", 200.4), cache_path(" song ", "ARTIST", 199.6));
        assert_ne!(cache_path("Song", "Artist", 200.0), cache_path("Song", "Artist", 230.0));
    }
}
