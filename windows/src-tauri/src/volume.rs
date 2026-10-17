// The Windows mixer in the island: the master volume and the level of each app
// that has sound (Core Audio), the same levels the mixer in the taskbar shows.
//
// Windows only. Elsewhere `volume_state` answers "not supported" and the island
// shows no button for it.

use serde::Serialize;

/// One app in the mixer. A browser has one audio session per process; they are
/// shown, and set, together as one app.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VolumeApp {
    /// What `volume_set` / `volume_mute` take as `app:<id>`: the program's file name, lower case.
    pub id: String,
    /// "Chrome", "Spotify"… empty for the system sounds (the island words that one itself).
    pub name: String,
    /// 0…100, the app's own level (what the mixer slider shows, mute aside).
    pub volume: i32,
    pub muted: bool,
    /// Sound is coming out of it right now.
    pub playing: bool,
    /// Windows' own sounds (notifications, errors).
    pub system: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VolumeState {
    pub supported: bool,
    /// The default output device's master volume, 0…100.
    pub master: i32,
    pub master_muted: bool,
    pub apps: Vec<VolumeApp>,
}

/// 0.0…1.0 from Core Audio to 0…100.
fn percent(level: f32) -> i32 {
    (level * 100.0).round().clamp(0.0, 100.0) as i32
}

/// 0…100 from the island to 0.0…1.0; anything else is brought into range.
fn fraction(value: f64) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    (value / 100.0).clamp(0.0, 1.0) as f32
}

/// "C:\Program Files\Google\Chrome\Application\chrome.exe" → "chrome".
fn exe_stem(path: &str) -> String {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let lower = file.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// A program's name as people know it; any other is its file name, capitalised.
fn friendly_name(stem: &str) -> String {
    let known = match stem {
        "chrome" => "Chrome",
        "msedge" => "Edge",
        "firefox" => "Firefox",
        "opera" => "Opera",
        "brave" => "Brave",
        "browser" => "Yandex Browser",
        "spotify" => "Spotify",
        "discord" => "Discord",
        "telegram" => "Telegram",
        "vlc" => "VLC",
        "steam" => "Steam",
        "zoom" => "Zoom",
        "ms-teams" | "teams" => "Teams",
        "coucou" => "Coucou",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Playing first, then the apps by name, Windows' own sounds last.
fn order(apps: &mut [VolumeApp]) {
    apps.sort_by(|a, b| {
        (!a.playing, a.system, a.name.to_lowercase()).cmp(&(!b.playing, b.system, b.name.to_lowercase()))
    });
}

#[cfg(windows)]
mod win {
    use std::collections::BTreeMap;

    use windows::core::Interface;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, AudioSessionStateActive, AudioSessionStateExpired, IAudioEndpointVolume,
        IAudioSessionControl2, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume,
        MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use super::{exe_stem, fraction, friendly_name, order, percent, VolumeApp, VolumeState};

    /// Core Audio needs COM on the calling thread; one that has it already keeps it.
    fn com() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    }

    fn exe_of(pid: u32) -> Option<String> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let named =
                QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
            let _ = CloseHandle(process);
            named.ok()?;
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    fn default_device() -> Option<IMMDevice> {
        unsafe {
            let devices = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            devices.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()
        }
    }

    fn master_volume(device: &IMMDevice) -> Option<IAudioEndpointVolume> {
        unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok() }
    }

    /// One audio session of one process.
    struct Session {
        id: String,
        name: String,
        system: bool,
        active: bool,
        volume: ISimpleAudioVolume,
    }

    /// The sessions on the default output that still exist.
    fn sessions(device: &IMMDevice) -> Vec<Session> {
        let mut out = Vec::new();
        unsafe {
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return out };
            let Ok(list) = manager.GetSessionEnumerator() else { return out };
            for i in 0..list.GetCount().unwrap_or(0) {
                let Ok(control) = list.GetSession(i) else { continue };
                let state = control.GetState().ok();
                if state == Some(AudioSessionStateExpired) {
                    continue;
                }
                let active = state == Some(AudioSessionStateActive);
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
                let Ok(volume) = control.cast::<ISimpleAudioVolume>() else { continue };
                // S_OK (0) says yes, S_FALSE (1) says no.
                let system = control2.IsSystemSoundsSession().0 == 0;
                let (id, name) = if system {
                    ("system".to_string(), String::new())
                } else {
                    let pid = control2.GetProcessId().unwrap_or(0);
                    if pid == 0 {
                        continue;
                    }
                    let Some(exe) = exe_of(pid) else { continue };
                    let stem = exe_stem(&exe);
                    if stem.is_empty() {
                        continue;
                    }
                    let name = friendly_name(&stem);
                    (stem, name)
                };
                out.push(Session { id, name, system, active, volume });
            }
        }
        out
    }

    pub fn state() -> VolumeState {
        com();
        let Some(device) = default_device() else { return VolumeState::default() };
        let (master, master_muted) = match master_volume(&device) {
            Some(endpoint) => unsafe {
                (
                    endpoint.GetMasterVolumeLevelScalar().map(percent).unwrap_or(0),
                    endpoint.GetMute().map(|m| m.as_bool()).unwrap_or(false),
                )
            },
            None => (0, false),
        };

        // A browser's processes are one app: the loudest level, muted only if all are.
        let mut apps: BTreeMap<String, VolumeApp> = BTreeMap::new();
        for s in sessions(&device) {
            let (level, muted) = unsafe {
                (
                    s.volume.GetMasterVolume().map(percent).unwrap_or(0),
                    s.volume.GetMute().map(|m| m.as_bool()).unwrap_or(false),
                )
            };
            let app = apps.entry(s.id.clone()).or_insert_with(|| VolumeApp {
                id: s.id.clone(),
                name: s.name.clone(),
                volume: 0,
                muted: true,
                playing: false,
                system: s.system,
            });
            app.volume = app.volume.max(level);
            app.muted = app.muted && muted;
            app.playing = app.playing || s.active;
        }
        let mut apps: Vec<VolumeApp> = apps.into_values().collect();
        order(&mut apps);
        VolumeState { supported: true, master, master_muted, apps }
    }

    /// `target` is "master" or "app:<id>". A level set by hand unmutes, as the mixer does.
    pub fn set(target: &str, value: f64) -> bool {
        com();
        let level = fraction(value);
        let Some(device) = default_device() else { return false };
        if target == "master" {
            let Some(endpoint) = master_volume(&device) else { return false };
            return unsafe {
                let _ = endpoint.SetMute(false, std::ptr::null());
                endpoint.SetMasterVolumeLevelScalar(level, std::ptr::null()).is_ok()
            };
        }
        let Some(id) = target.strip_prefix("app:") else { return false };
        let mut done = false;
        for s in sessions(&device).iter().filter(|s| s.id == id) {
            unsafe {
                let _ = s.volume.SetMute(false, std::ptr::null());
                done |= s.volume.SetMasterVolume(level, std::ptr::null()).is_ok();
            }
        }
        done
    }

    pub fn mute(target: &str, muted: bool) -> bool {
        com();
        let Some(device) = default_device() else { return false };
        if target == "master" {
            let Some(endpoint) = master_volume(&device) else { return false };
            return unsafe { endpoint.SetMute(muted, std::ptr::null()).is_ok() };
        }
        let Some(id) = target.strip_prefix("app:") else { return false };
        let mut done = false;
        for s in sessions(&device).iter().filter(|s| s.id == id) {
            done |= unsafe { s.volume.SetMute(muted, std::ptr::null()).is_ok() };
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, playing: bool, system: bool) -> VolumeApp {
        VolumeApp { id: name.to_lowercase(), name: name.into(), volume: 50, muted: false, playing, system }
    }

    #[test]
    fn levels_are_whole_percents_inside_the_range() {
        assert_eq!(percent(0.0), 0);
        assert_eq!(percent(0.455), 46);
        assert_eq!(percent(1.0), 100);
        assert_eq!(percent(1.7), 100);
        assert_eq!(percent(-0.3), 0);
    }

    #[test]
    fn a_level_from_the_island_is_brought_into_range() {
        assert_eq!(fraction(45.0), 0.45);
        assert_eq!(fraction(250.0), 1.0);
        assert_eq!(fraction(-5.0), 0.0);
        assert_eq!(fraction(f64::NAN), 0.0);
        assert_eq!(fraction(f64::INFINITY), 0.0);
    }

    #[test]
    fn a_program_is_named_by_its_file() {
        assert_eq!(exe_stem(r"C:\Program Files\Google\Chrome\Application\chrome.exe"), "chrome");
        assert_eq!(exe_stem("C:/Games/Game.EXE"), "game");
        assert_eq!(exe_stem("spotify"), "spotify");
        assert_eq!(exe_stem(""), "");
    }

    #[test]
    fn known_programs_get_their_names_and_others_a_capital() {
        assert_eq!(friendly_name("msedge"), "Edge");
        assert_eq!(friendly_name("browser"), "Yandex Browser");
        assert_eq!(friendly_name("somegame"), "Somegame");
        assert_eq!(friendly_name(""), "");
    }

    #[test]
    fn the_mixer_lists_what_plays_first_and_windows_sounds_last() {
        let mut apps = vec![app("Zoom", false, false), app("", true, true), app("Spotify", true, false), app("Chrome", true, false)];
        order(&mut apps);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Chrome", "Spotify", "", "Zoom"]);
    }

    #[test]
    fn the_state_goes_to_the_page_in_camel_case() {
        let json = serde_json::to_value(VolumeState { supported: true, master: 40, master_muted: true, apps: vec![app("Chrome", true, false)] })
            .unwrap();
        assert_eq!(json["masterMuted"], true);
        assert_eq!(json["apps"][0]["playing"], true);
        assert_eq!(json["apps"][0]["system"], false);
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// The master volume and the apps' levels; `supported: false` off Windows.
#[tauri::command]
pub async fn volume_state() -> VolumeState {
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(win::state).await.unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        VolumeState::default()
    }
}

/// Sets the level of `target` ("master" or "app:<id>"), 0…100. True when it took.
#[tauri::command]
pub async fn volume_set(target: String, value: f64) -> bool {
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(move || win::set(&target, value)).await.unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        let _ = (target, value);
        false
    }
}

/// Mutes or unmutes `target` ("master" or "app:<id>"). True when it took.
#[tauri::command]
pub async fn volume_mute(target: String, muted: bool) -> bool {
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(move || win::mute(&target, muted)).await.unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        let _ = (target, muted);
        false
    }
}
