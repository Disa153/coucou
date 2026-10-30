//! The app panel's rules, with nothing of the OS in them (apps/mod.rs keeps them
//! on disk, apps/win.rs asks Windows): which programs are pinned and in what
//! order, which of them run, what the Start menu offers and what a new install
//! starts with. Plain Rust, so `rustc --test logic.rs` runs these tests alone.

/// A program: what the panel shows and what a click opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its name as the Start menu gives it ("Telegram", "Visual Studio Code").
    pub name: String,
    /// What a click opens: a Start-menu shortcut (.lnk) or a program (.exe).
    pub path: String,
    /// The program it runs, lower case, without folder or extension
    /// ("telegram", "code"): how a running window is matched to it.
    pub exe: String,
}

/// At most this many pinned programs (one row of the island holds about ten).
pub const MAX_PINNED: usize = 12;
/// At most this many running programs shown beside the pinned ones.
pub const MAX_OTHERS: usize = 6;

/// "C:\\Program Files\\Telegram\\Telegram.exe" → "telegram".
pub fn exe_key(path: &str) -> String {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let stem = match file.rfind('.') {
        Some(dot) if dot > 0 => &file[..dot],
        _ => file,
    };
    stem.trim().to_lowercase()
}

/// The program a shortcut really starts. Squirrel installs (Discord, Slack, Teams
/// classic…) point at Update.exe with `--processStart Discord.exe`.
pub fn exe_of_shortcut(target: &str, arguments: &str) -> String {
    let key = exe_key(target);
    if key == "update" {
        let mut words = arguments.split_whitespace();
        while let Some(word) = words.next() {
            if word.eq_ignore_ascii_case("--processStart") {
                if let Some(exe) = words.next() {
                    return exe_key(exe.trim_matches('"'));
                }
            }
        }
    }
    key
}

/// Pins `entry` at the end. False when it is already there (same shortcut or
/// same program) or the panel is full.
pub fn pin(list: &mut Vec<Entry>, entry: Entry) -> bool {
    let same = |e: &Entry| {
        e.path.eq_ignore_ascii_case(&entry.path) || (!entry.exe.is_empty() && e.exe == entry.exe)
    };
    if list.iter().any(same) || list.len() >= MAX_PINNED {
        return false;
    }
    list.push(entry);
    true
}

/// Unpins the program opened by `path`.
pub fn unpin(list: &mut Vec<Entry>, path: &str) -> bool {
    let before = list.len();
    list.retain(|e| !e.path.eq_ignore_ascii_case(path));
    list.len() != before
}

/// Moves the program opened by `path` to `index` (clamped to the list).
pub fn move_to(list: &mut Vec<Entry>, path: &str, index: usize) -> bool {
    let Some(from) = list.iter().position(|e| e.path.eq_ignore_ascii_case(path)) else {
        return false;
    };
    let entry = list.remove(from);
    let to = index.min(list.len());
    list.insert(to, entry);
    from != to
}

/// Whether a pinned program has a window open, `running` being the programs of
/// the open windows (exe keys).
pub fn is_running(entry: &Entry, running: &[String]) -> bool {
    !entry.exe.is_empty() && running.iter().any(|r| *r == entry.exe)
}

/// The running programs that are not pinned, once each, in the order given
/// (the front window first), at most MAX_OTHERS.
pub fn others(running: &[Entry], pinned: &[Entry]) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for e in running {
        if e.exe.is_empty() || pinned.iter().any(|p| p.exe == e.exe) || out.iter().any(|o| o.exe == e.exe) {
            continue;
        }
        out.push(e.clone());
        if out.len() == MAX_OTHERS {
            break;
        }
    }
    out
}

/// Start-menu entries that are not programs to open: uninstallers, read-mes,
/// help files, websites.
pub fn worth_listing(name: &str, path: &str) -> bool {
    let n = name.to_lowercase();
    let words = [
        "uninstall", "удалить", "удаление", "désinstaller", "deinstall", "readme", "read me", "help", "справка",
        "documentation", "manual", "license", "licence", "release notes", "website", "web site",
    ];
    let p = path.to_lowercase();
    !words.iter().any(|w| n.contains(w)) && (p.ends_with(".lnk") || p.ends_with(".exe"))
}

/// The Start menu as a list: sorted by name, one entry per name (the user's own
/// folder is read first and wins).
pub fn tidy_catalog(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for e in entries {
        if !worth_listing(&e.name, &e.path) || out.iter().any(|o| o.name.eq_ignore_ascii_case(&e.name)) {
            continue;
        }
        out.push(e);
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

/// What the Start menu shows for `query`: the names that start with it first,
/// then those that contain it. Empty query: everything.
pub fn search(catalog: &[Entry], query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return catalog.to_vec();
    }
    let mut starts: Vec<Entry> = Vec::new();
    let mut inside: Vec<Entry> = Vec::new();
    for e in catalog {
        let n = e.name.to_lowercase();
        if n.starts_with(&q) || n.split_whitespace().any(|w| w.starts_with(&q)) {
            starts.push(e.clone());
        } else if n.contains(&q) || e.exe.contains(&q) {
            inside.push(e.clone());
        }
    }
    starts.extend(inside);
    starts
}

/// The programs a new install starts with, in this order, when the Start menu
/// has them: a browser, Telegram, Discord, VS Code, Spotify, Steam. The file
/// explorer comes first, whatever the Start menu says.
pub fn seed(catalog: &[Entry], explorer: Entry) -> Vec<Entry> {
    let wanted: [&[&str]; 6] = [
        &["chrome", "msedge", "firefox", "opera", "brave", "yandex", "browser"],
        &["telegram"],
        &["discord"],
        &["code"],
        &["spotify"],
        &["steam"],
    ];
    let mut out = vec![explorer];
    for group in wanted {
        let found = group.iter().find_map(|exe| catalog.iter().find(|e| e.exe == *exe));
        if let Some(e) = found {
            pin(&mut out, e.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, path: &str, exe: &str) -> Entry {
        Entry { name: name.into(), path: path.into(), exe: exe.into() }
    }

    #[test]
    fn exe_keys() {
        assert_eq!(exe_key(r"C:\Program Files\Telegram Desktop\Telegram.exe"), "telegram");
        assert_eq!(exe_key("/usr/bin/code"), "code");
        assert_eq!(exe_key(r"C:\x\My.App.exe"), "my.app");
        assert_eq!(exe_key(".hidden"), ".hidden");
    }

    #[test]
    fn squirrel_shortcuts_name_the_real_program() {
        let target = r"C:\Users\u\AppData\Local\Discord\Update.exe";
        assert_eq!(exe_of_shortcut(target, "--processStart Discord.exe"), "discord");
        assert_eq!(exe_of_shortcut(target, r#"--processStart "Slack.exe" --x"#), "slack");
        assert_eq!(exe_of_shortcut(target, ""), "update");
        assert_eq!(exe_of_shortcut(r"C:\a\chrome.exe", "--profile 1"), "chrome");
    }

    #[test]
    fn pinning_keeps_one_of_each_and_a_limit() {
        let mut list = Vec::new();
        assert!(pin(&mut list, e("Telegram", r"C:\s\Telegram.lnk", "telegram")));
        assert!(!pin(&mut list, e("Telegram 2", r"c:\s\telegram.LNK", "")), "same shortcut");
        assert!(!pin(&mut list, e("TG", r"C:\t\Telegram.exe", "telegram")), "same program");
        assert!(pin(&mut list, e("Notes", r"C:\n.lnk", "")));
        assert!(pin(&mut list, e("Other", r"C:\o.lnk", "")), "an unknown program never matches another");
        for i in 0..20 {
            pin(&mut list, e("x", &format!(r"C:\{i}.lnk"), &format!("x{i}")));
        }
        assert_eq!(list.len(), MAX_PINNED);
    }

    #[test]
    fn unpin_and_move() {
        let mut list = vec![e("A", "a", "a"), e("B", "b", "b"), e("C", "c", "c")];
        assert!(move_to(&mut list, "C", 0));
        assert_eq!(list.iter().map(|x| x.name.as_str()).collect::<String>(), "CAB");
        assert!(move_to(&mut list, "c", 99));
        assert_eq!(list.iter().map(|x| x.name.as_str()).collect::<String>(), "ABC");
        assert!(!move_to(&mut list, "a", 0), "already there");
        assert!(!move_to(&mut list, "z", 0));
        assert!(unpin(&mut list, "B"));
        assert!(!unpin(&mut list, "B"));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn running_and_others() {
        let pinned = vec![e("Telegram", "t.lnk", "telegram"), e("Odd", "o.lnk", "")];
        let running = vec!["telegram".to_string(), "code".to_string()];
        assert!(is_running(&pinned[0], &running));
        assert!(!is_running(&pinned[1], &running), "unknown program: never shown running");
        let windows = vec![
            e("Code", r"C:\code.exe", "code"),
            e("Telegram", r"C:\Telegram.exe", "telegram"),
            e("Code", r"C:\code.exe", "code"),
            e("?", "", ""),
        ];
        let o = others(&windows, &pinned);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].exe, "code");
        let many: Vec<Entry> = (0..10).map(|i| e("p", &format!("{i}"), &format!("p{i}"))).collect();
        assert_eq!(others(&many, &[]).len(), MAX_OTHERS);
    }

    #[test]
    fn the_catalog_drops_uninstallers_and_doubles() {
        let list = tidy_catalog(vec![
            e("Telegram", "u.lnk", "telegram"),
            e("telegram", "m.lnk", "telegram"),
            e("Uninstall Telegram", "x.lnk", "unins000"),
            e("Удалить Steam", "y.lnk", "uninstall"),
            e("Steam Help", "h.lnk", "steam"),
            e("Steam", "s.lnk", "steam"),
            e("Notes", "n.url", ""),
            e("audacity", "a.lnk", "audacity"),
        ]);
        let names: Vec<&str> = list.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["audacity", "Steam", "Telegram"]);
        assert_eq!(list[2].path, "u.lnk", "the first one read wins");
    }

    #[test]
    fn search_puts_the_start_of_a_word_first() {
        let c = vec![e("Google Chrome", "c", "chrome"), e("Microsoft Edge", "m", "msedge"), e("Steam", "s", "steam")];
        let names = |q: &str| search(&c, q).into_iter().map(|x| x.name).collect::<Vec<_>>();
        assert_eq!(names(""), ["Google Chrome", "Microsoft Edge", "Steam"]);
        assert_eq!(names("ch"), ["Google Chrome"]);
        assert_eq!(names("e"), ["Microsoft Edge", "Google Chrome", "Steam"]);
        assert_eq!(names("msed"), ["Microsoft Edge"], "the program's name counts too");
        assert!(names("zzz").is_empty());
    }

    #[test]
    fn a_new_install_starts_with_what_the_start_menu_has() {
        let c = vec![
            e("Steam", "s", "steam"),
            e("Microsoft Edge", "m", "msedge"),
            e("Google Chrome", "c", "chrome"),
            e("Discord", "d", "discord"),
        ];
        let s = seed(&c, e("File Explorer", "explorer.exe", "explorer"));
        let names: Vec<&str> = s.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["File Explorer", "Google Chrome", "Discord", "Steam"]);
    }
}
