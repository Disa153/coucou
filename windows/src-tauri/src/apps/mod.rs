//! The island's app panel (src/views/apps.ts): the programs the user pinned,
//! the ones with a window open, and their icons, like a dock in the island.
//! The pinned list lives in apps.json beside the settings; nothing leaves the
//! computer. Windows only for now: elsewhere the panel answers `supported: false`.
//!
//! logic.rs holds the rules (tested on their own), png.rs turns an icon into a
//! PNG, win.rs asks Windows.

mod logic;
mod png;
#[cfg(windows)]
mod win;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub use logic::Entry;

/// The Start menu is read again at most this often (it changes on installs).
const CATALOG_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
struct SavedApp {
    name: String,
    path: String,
    exe: String,
}

impl Default for SavedApp {
    fn default() -> Self {
        SavedApp { name: String::new(), path: String::new(), exe: String::new() }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    pinned: Vec<SavedApp>,
    /// The panel is what the island opens on when no agent is at work.
    #[serde(default = "yes")]
    when_idle: bool,
    /// The first programs were pinned (once, on the first start).
    #[serde(default)]
    seeded: bool,
}

impl Default for Store {
    fn default() -> Self {
        Store { pinned: Vec::new(), when_idle: true, seeded: false }
    }
}

impl Store {
    fn entries(&self) -> Vec<Entry> {
        self.pinned.iter().map(|a| Entry { name: a.name.clone(), path: a.path.clone(), exe: a.exe.clone() }).collect()
    }
    fn set_entries(&mut self, list: Vec<Entry>) {
        self.pinned = list.into_iter().map(|e| SavedApp { name: e.name, path: e.path, exe: e.exe }).collect();
    }
}

fn path() -> PathBuf {
    crate::settings::local_dir().join("apps.json")
}

fn load() -> Store {
    std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(s: &Store) {
    if let Ok(text) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(path(), text);
    }
}

static CATALOG: LazyLock<Mutex<Option<(Instant, Vec<Entry>)>>> = LazyLock::new(|| Mutex::new(None));
static ICONS: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The Start menu, read at most once a minute.
fn catalog() -> Vec<Entry> {
    if let Some((at, list)) = CATALOG.lock().unwrap().as_ref() {
        if at.elapsed() < CATALOG_TTL {
            return list.clone();
        }
    }
    #[cfg(windows)]
    let list = logic::tidy_catalog(win::catalog());
    #[cfg(not(windows))]
    let list: Vec<Entry> = Vec::new();
    *CATALOG.lock().unwrap() = Some((Instant::now(), list.clone()));
    list
}

/// The store, with the first programs pinned on the very first start.
fn store() -> Store {
    let mut s = load();
    if !s.seeded && cfg!(windows) {
        #[cfg(windows)]
        s.set_entries(logic::seed(&catalog(), win::explorer()));
        s.seeded = true;
        save(&s);
    }
    s
}

fn running() -> Vec<Entry> {
    #[cfg(windows)]
    return win::running(&catalog());
    #[cfg(not(windows))]
    Vec::new()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub name: String,
    pub path: String,
    pub exe: String,
    pub running: bool,
    pub pinned: bool,
}

fn item(e: Entry, running: bool, pinned: bool) -> Item {
    Item { name: e.name, path: e.path, exe: e.exe, running, pinned }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Panel {
    pub supported: bool,
    pub pinned: Vec<Item>,
    /// Running programs that are not pinned.
    pub others: Vec<Item>,
    pub when_idle: bool,
}

/// What the panel shows now.
#[tauri::command]
pub async fn apps_panel() -> Panel {
    tokio::task::spawn_blocking(|| {
        let s = store();
        let pinned = s.entries();
        let windows = running();
        let keys: Vec<String> = windows.iter().map(|e| e.exe.clone()).collect();
        Panel {
            supported: cfg!(windows),
            pinned: pinned.iter().cloned().map(|e| { let r = logic::is_running(&e, &keys); item(e, r, true) }).collect(),
            others: logic::others(&windows, &pinned).into_iter().map(|e| item(e, true, false)).collect(),
            when_idle: s.when_idle,
        }
    })
    .await
    .unwrap_or(Panel { supported: false, pinned: Vec::new(), others: Vec::new(), when_idle: false })
}

/// The Start menu's programs matching `query`, for "Add a program".
#[tauri::command]
pub async fn apps_catalog(query: String) -> Vec<Item> {
    tokio::task::spawn_blocking(move || {
        let pinned = store().entries();
        logic::search(&catalog(), &query)
            .into_iter()
            .take(60)
            .map(|e| {
                let p = pinned.iter().any(|x| x.path.eq_ignore_ascii_case(&e.path) || (!e.exe.is_empty() && x.exe == e.exe));
                item(e, false, p)
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// A program's icon as a `data:image/png` URL, read once and kept.
#[tauri::command]
pub async fn apps_icon(path: String) -> Option<String> {
    if let Some(hit) = ICONS.lock().unwrap().get(&path) {
        return hit.clone();
    }
    let key = path.clone();
    let url = tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        return win::icon_png(&path).map(|png| format!("data:image/png;base64,{}", crate::claude::base64_for(&png)));
        #[cfg(not(windows))]
        {
            let _ = path;
            None
        }
    })
    .await
    .ok()
    .flatten();
    ICONS.lock().unwrap().insert(key, url.clone());
    url
}

#[tauri::command]
pub fn apps_pin(name: String, path: String, exe: String) -> bool {
    let mut s = store();
    let mut list = s.entries();
    let ok = logic::pin(&mut list, Entry { name, path, exe });
    if ok {
        s.set_entries(list);
        save(&s);
    }
    ok
}

#[tauri::command]
pub fn apps_unpin(path: String) -> bool {
    let mut s = store();
    let mut list = s.entries();
    let ok = logic::unpin(&mut list, &path);
    if ok {
        s.set_entries(list);
        save(&s);
    }
    ok
}

#[tauri::command]
pub fn apps_move(path: String, index: usize) -> bool {
    let mut s = store();
    let mut list = s.entries();
    let ok = logic::move_to(&mut list, &path, index);
    if ok {
        s.set_entries(list);
        save(&s);
    }
    ok
}

/// Opens a program, or brings its window forward when it has one.
#[tauri::command]
pub async fn apps_open(path: String, exe: String) -> bool {
    tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        return win::open(&path, &exe);
        #[cfg(not(windows))]
        {
            let _ = (path, exe);
            false
        }
    })
    .await
    .unwrap_or(false)
}

#[tauri::command]
pub fn apps_set_when_idle(on: bool) {
    let mut s = store();
    s.when_idle = on;
    save(&s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_store_opens_the_panel_when_idle_and_is_not_seeded() {
        let s: Store = serde_json::from_str("{}").unwrap();
        assert!(s.when_idle);
        assert!(!s.seeded);
        assert!(s.pinned.is_empty());
        let s: Store = serde_json::from_str(r#"{"whenIdle":false,"seeded":true,"pinned":[{"name":"A","path":"a.lnk"}]}"#).unwrap();
        assert!(!s.when_idle);
        assert_eq!(s.entries()[0].exe, "");
    }
}
