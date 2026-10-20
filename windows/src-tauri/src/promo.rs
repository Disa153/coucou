// A card after an update: once per new build, the island shows a friend's
// project (Custom UI, a macOS-style shell for Windows 11 by sergey1029) with
// "More" for the details. Nothing is fetched: the words are in the app, and
// the link opens only on the user's click. "Don't show again" ends it for good.
// The first launch after installing shows nothing: only an update does.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Seen {
    /// The build that ran last time Coucou started (0: never noted).
    pub last_build: u64,
    /// A new build arrived and the card was not shown for it yet.
    pub due: bool,
    pub never: bool,
}

/// What a start of `build` does to what was kept.
pub fn on_start(mut s: Seen, build: u64) -> Seen {
    if build == 0 {
        return s;
    }
    if s.last_build != 0 && build != s.last_build {
        s.due = true;
    }
    s.last_build = build;
    s
}

fn path() -> PathBuf {
    crate::settings::local_dir().join("promo.json")
}

fn load() -> Seen {
    std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(s: &Seen) {
    if let Ok(text) = serde_json::to_string(s) {
        let _ = std::fs::write(path(), text);
    }
}

pub fn setup() {
    save(&on_start(load(), crate::updater::build()));
}

/// The card is to be shown now (the island asks after its greeting). Windows only: Custom UI is for Windows 11.
#[tauri::command]
pub fn promo_due() -> bool {
    let s = load();
    cfg!(windows) && s.due && !s.never
}

/// Shown (or put away); `never`: not after any update again.
#[tauri::command]
pub fn promo_done(never: bool) {
    let mut s = load();
    s.due = false;
    s.never |= never;
    save(&s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_update_brings_the_card() {
        // the first start after installing: noted, nothing shown
        let s = on_start(Seen::default(), 68);
        assert_eq!(s, Seen { last_build: 68, due: false, never: false });
        // the same build again: nothing
        assert!(!on_start(s.clone(), 68).due);
        // an update
        let s = on_start(s, 69);
        assert!(s.due);
        // a local build changes nothing
        assert_eq!(on_start(s.clone(), 0), s);
    }
}
