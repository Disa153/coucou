//! The island's sound mixer (views/mixer.ts): the speakers (which output, its
//! level, mute) and every program's own level, as the Windows volume mixer
//! shows them. Windows only: on Linux the commands answer `supported: false`.
//!
//! Levels are 0…100. A program is every audio session of the same executable
//! on the current output (a browser has one per tab process), keyed by the
//! executable's name; the system sounds are the key `system`.

// The helpers only Windows calls stay compiled (and tested) everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::Serialize;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub current: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Level {
    pub volume: i32,
    pub muted: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub key: String,
    pub name: String,
    pub volume: i32,
    pub muted: bool,
    /// Coucou itself (Mochi's sounds).
    pub own: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MixerState {
    pub supported: bool,
    pub devices: Vec<Device>,
    pub master: Option<Level>,
    pub apps: Vec<App>,
}

/// The key the mixer groups a program's sessions under: its executable's
/// name, lower case, without `.exe`.
pub fn app_key(exe: &str) -> String {
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_lowercase();
    file.strip_suffix(".exe").unwrap_or(&file).to_string()
}

/// The name a program wears in the mixer: the session's own when it has one
/// (not a resource reference like `@%SystemRoot%\…`), else a known program's,
/// else its executable's name with a capital.
pub fn app_name(key: &str, display: &str) -> String {
    let display = display.trim();
    if !display.is_empty() && !display.starts_with('@') {
        return display.to_string();
    }
    const KNOWN: [(&str, &str); 22] = [
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("firefox", "Firefox"),
        ("opera", "Opera"),
        ("browser", "Яндекс Браузер"),
        ("brave", "Brave"),
        ("discord", "Discord"),
        ("telegram", "Telegram"),
        ("spotify", "Spotify"),
        ("yandexmusic", "Яндекс Музыка"),
        ("steam", "Steam"),
        ("steamwebhelper", "Steam"),
        ("cs2", "Counter-Strike 2"),
        ("dota2", "Dota 2"),
        ("vlc", "VLC"),
        ("obs64", "OBS Studio"),
        ("zoom", "Zoom"),
        ("teams", "Teams"),
        ("ms-teams", "Teams"),
        ("whatsapp", "WhatsApp"),
        ("microsoft.media.player", "Media Player"),
        ("coucou", "Coucou"),
    ];
    if let Some((_, name)) = KNOWN.iter().find(|(k, _)| *k == key) {
        return name.to_string();
    }
    let mut chars = key.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => key.to_string(),
    }
}

/// Programs in the order the mixer lists them: Coucou last, the rest by name.
pub fn sort_apps(apps: &mut [App]) {
    apps.sort_by(|a, b| a.own.cmp(&b.own).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

fn percent(level: f32) -> i32 {
    (level * 100.0).round().clamp(0.0, 100.0) as i32
}

fn scalar(percent: f64) -> f32 {
    (percent / 100.0).clamp(0.0, 1.0) as f32
}

// ── Commands ─────────────────────────────────────────────────────────────────

/// Core Audio runs on a blocking thread: it can take a moment while a device
/// wakes up, and must never hold the island's thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static, fallback: T) -> T {
    tauri::async_runtime::spawn_blocking(f).await.unwrap_or(fallback)
}

#[tauri::command]
pub async fn mixer_state() -> MixerState {
    blocking(imp::state, MixerState::default()).await
}

#[tauri::command]
pub async fn mixer_set_master(volume: Option<f64>, muted: Option<bool>) -> bool {
    blocking(move || imp::set_master(volume, muted), false).await
}

#[tauri::command]
pub async fn mixer_set_app(key: String, volume: Option<f64>, muted: Option<bool>) -> bool {
    blocking(move || imp::set_app(&key, volume, muted), false).await
}

#[tauri::command]
pub async fn mixer_set_device(id: String) -> bool {
    blocking(move || imp::set_device(&id), false).await
}

#[tauri::command]
pub fn mixer_open_system() -> bool {
    imp::open_system()
}

// ── Windows: Core Audio ──────────────────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use super::{app_key, app_name, percent, scalar, sort_apps, App, Device, Level, MixerState};
    use std::collections::HashMap;
    use std::ffi::c_void;

    use windows::core::{Interface, GUID, HRESULT, IUnknown, PCWSTR};
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Foundation::{CloseHandle, S_OK};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCommunications, eConsole, eMultimedia, eRender, AudioSessionStateExpired, IAudioSessionControl2,
        IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
        DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    fn com() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    }

    fn enumerator() -> Option<IMMDeviceEnumerator> {
        com();
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
    }

    /// The output Windows plays to: the one the sound settings call default.
    fn default_device(devices: &IMMDeviceEnumerator) -> Option<IMMDevice> {
        unsafe { devices.GetDefaultAudioEndpoint(eRender, eConsole).ok() }
    }

    fn device_id(device: &IMMDevice) -> Option<String> {
        unsafe {
            let raw = device.GetId().ok()?;
            let id = raw.to_string().ok();
            CoTaskMemFree(Some(raw.0 as *const c_void));
            id
        }
    }

    fn device_name(device: &IMMDevice) -> Option<String> {
        unsafe {
            let store = device.OpenPropertyStore(STGM_READ).ok()?;
            let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
            let name = value.to_string();
            (!name.is_empty()).then_some(name)
        }
    }

    fn exe_of(pid: u32) -> Option<String> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let named = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(process);
            named.ok()?;
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// One audio session on the current output, with the program it belongs to.
    struct Session {
        key: String,
        display: String,
        own: bool,
        volume: ISimpleAudioVolume,
    }

    fn sessions(device: &IMMDevice) -> Vec<Session> {
        let mut out = Vec::new();
        let own_pid = unsafe { GetCurrentProcessId() };
        unsafe {
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return out };
            let Ok(list) = manager.GetSessionEnumerator() else { return out };
            for i in 0..list.GetCount().unwrap_or(0) {
                let Ok(control) = list.GetSession(i) else { continue };
                let Ok(control) = control.cast::<IAudioSessionControl2>() else { continue };
                if control.GetState().is_ok_and(|s| s == AudioSessionStateExpired) {
                    continue;
                }
                let Ok(volume) = control.cast::<ISimpleAudioVolume>() else { continue };
                let display = control
                    .GetDisplayName()
                    .ok()
                    .map(|p| {
                        let s = p.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(p.0 as *const c_void));
                        s
                    })
                    .unwrap_or_default();
                if control.IsSystemSoundsSession() == S_OK {
                    out.push(Session { key: "system".into(), display: String::new(), own: false, volume });
                    continue;
                }
                let pid = control.GetProcessId().unwrap_or(0);
                if pid == 0 {
                    continue;
                }
                // A program that has just quit leaves its session for a moment.
                let Some(exe) = exe_of(pid) else { continue };
                // Coucou's sounds play in its webview, a child process: it
                // still wears Coucou's name, as it does in Windows' mixer.
                let key = app_key(&exe);
                let own = pid == own_pid || key == "coucou";
                let key = if own { "coucou".to_string() } else { key };
                out.push(Session { key, display, own, volume });
            }
        }
        out
    }

    fn level_of(volume: &ISimpleAudioVolume) -> Option<(f32, bool)> {
        unsafe {
            let level = volume.GetMasterVolume().ok()?;
            let muted = volume.GetMute().map(|m| m.as_bool()).unwrap_or(false);
            Some((level, muted))
        }
    }

    pub fn state() -> MixerState {
        let Some(devices) = enumerator() else {
            return MixerState { supported: true, ..Default::default() };
        };
        let current = default_device(&devices);
        let current_id = current.as_ref().and_then(device_id);

        let mut list = Vec::new();
        unsafe {
            if let Ok(all) = devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
                for i in 0..all.GetCount().unwrap_or(0) {
                    let Ok(device) = all.Item(i) else { continue };
                    let Some(id) = device_id(&device) else { continue };
                    let name = device_name(&device).unwrap_or_else(|| id.clone());
                    let is_current = current_id.as_deref() == Some(id.as_str());
                    list.push(Device { id, name, current: is_current });
                }
            }
        }

        let master = current.as_ref().and_then(|device| unsafe {
            let endpoint = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()?;
            let level = endpoint.GetMasterVolumeLevelScalar().ok()?;
            let muted = endpoint.GetMute().map(|m| m.as_bool()).unwrap_or(false);
            Some(Level { volume: percent(level), muted })
        });

        // One row per program: its loudest session, muted only when all are.
        let mut apps: HashMap<String, App> = HashMap::new();
        for s in current.as_ref().map(sessions).unwrap_or_default() {
            let Some((level, muted)) = level_of(&s.volume) else { continue };
            let name = if s.key == "system" { String::new() } else { app_name(&s.key, &s.display) };
            let app = apps.entry(s.key.clone()).or_insert_with(|| App {
                key: s.key.clone(),
                name,
                volume: 0,
                muted: true,
                own: s.own,
            });
            app.volume = app.volume.max(percent(level));
            app.muted &= muted;
        }
        let mut apps: Vec<App> = apps.into_values().collect();
        sort_apps(&mut apps);
        MixerState { supported: true, devices: list, master, apps }
    }

    pub fn set_master(volume: Option<f64>, muted: Option<bool>) -> bool {
        let Some(devices) = enumerator() else { return false };
        let Some(device) = default_device(&devices) else { return false };
        unsafe {
            let Ok(endpoint) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else { return false };
            let mut ok = true;
            if let Some(v) = volume {
                // A level set by hand unmutes, as the volume flyout does.
                let _ = endpoint.SetMute(false, std::ptr::null());
                ok &= endpoint.SetMasterVolumeLevelScalar(scalar(v), std::ptr::null()).is_ok();
            }
            if let Some(m) = muted {
                ok &= endpoint.SetMute(m, std::ptr::null()).is_ok();
            }
            ok
        }
    }

    pub fn set_app(key: &str, volume: Option<f64>, muted: Option<bool>) -> bool {
        let Some(devices) = enumerator() else { return false };
        let Some(device) = default_device(&devices) else { return false };
        let mut done = false;
        for s in sessions(&device).into_iter().filter(|s| s.key == key) {
            unsafe {
                if let Some(v) = volume {
                    let _ = s.volume.SetMute(false, std::ptr::null());
                    done |= s.volume.SetMasterVolume(scalar(v), std::ptr::null()).is_ok();
                }
                if let Some(m) = muted {
                    done |= s.volume.SetMute(m, std::ptr::null()).is_ok();
                }
            }
        }
        done
    }

    // Windows has no public call to change the default output. The sound
    // settings, and every output switcher, use the undocumented IPolicyConfig
    // that has kept this layout since Windows 7: IUnknown's three methods,
    // then ten more, then SetDefaultEndpoint(id, role).
    const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
    const SET_DEFAULT_ENDPOINT: usize = 13;
    type SetDefaultEndpoint = unsafe extern "system" fn(*mut c_void, PCWSTR, i32) -> HRESULT;

    pub fn set_device(id: &str) -> bool {
        // Only an output Windows lists right now.
        let Some(devices) = enumerator() else { return false };
        let known = unsafe {
            devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).is_ok_and(|all| {
                (0..all.GetCount().unwrap_or(0))
                    .filter_map(|i| all.Item(i).ok())
                    .any(|d| device_id(&d).as_deref() == Some(id))
            })
        };
        if !known {
            return false;
        }
        let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let Ok(unknown) = CoCreateInstance::<_, IUnknown>(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) else {
                crate::log::line("mixer: the output switcher is not available");
                return false;
            };
            let mut raw: *mut c_void = std::ptr::null_mut();
            if unknown.query(&IID_POLICY_CONFIG, &mut raw).is_err() || raw.is_null() {
                crate::log::line("mixer: the output switcher is not available");
                return false;
            }
            let policy = IUnknown::from_raw(raw);
            let vtable = *(policy.as_raw() as *const *const usize);
            let set: SetDefaultEndpoint = std::mem::transmute(*vtable.add(SET_DEFAULT_ENDPOINT));
            let mut ok = true;
            for role in [eConsole, eMultimedia, eCommunications] {
                ok &= set(policy.as_raw(), PCWSTR(wide.as_ptr()), role.0).is_ok();
            }
            if !ok {
                crate::log::line("mixer: Windows refused to change the output");
            }
            ok
        }
    }

    pub fn open_system() -> bool {
        std::process::Command::new("explorer.exe").arg("ms-settings:sound").spawn().is_ok()
    }
}

#[cfg(not(windows))]
mod imp {
    use super::MixerState;

    pub fn state() -> MixerState {
        MixerState::default()
    }
    pub fn set_master(_: Option<f64>, _: Option<bool>) -> bool {
        false
    }
    pub fn set_app(_: &str, _: Option<f64>, _: Option<bool>) -> bool {
        false
    }
    pub fn set_device(_: &str) -> bool {
        false
    }
    pub fn open_system() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_the_executable_name() {
        assert_eq!(app_key(r"C:\Program Files\Google\Chrome\Application\chrome.exe"), "chrome");
        assert_eq!(app_key(r"C:\Users\me\AppData\Roaming\Telegram Desktop\Telegram.exe"), "telegram");
        assert_eq!(app_key("/usr/bin/vlc"), "vlc");
        assert_eq!(app_key("Discord.EXE"), "discord");
    }

    #[test]
    fn names_prefer_the_session_then_known_programs() {
        assert_eq!(app_name("cs2", ""), "Counter-Strike 2");
        assert_eq!(app_name("browser", ""), "Яндекс Браузер");
        assert_eq!(app_name("cs2", "  "), "Counter-Strike 2");
        assert_eq!(app_name("game", "My Game"), "My Game");
        assert_eq!(app_name("audiodg", "@%SystemRoot%\\System32\\AudioSrv.Dll,-202"), "Audiodg");
        assert_eq!(app_name("", ""), "");
    }

    #[test]
    fn coucou_comes_last() {
        let app = |key: &str, name: &str, own: bool| App { key: key.into(), name: name.into(), volume: 50, muted: false, own };
        let mut apps = vec![app("coucou", "Coucou", true), app("telegram", "Telegram", false), app("chrome", "Chrome", false)];
        sort_apps(&mut apps);
        let names: Vec<_> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Chrome", "Telegram", "Coucou"]);
    }

    #[test]
    fn levels_round_and_clamp() {
        assert_eq!(percent(0.625), 63);
        assert_eq!(percent(1.4), 100);
        assert_eq!(scalar(150.0), 1.0);
        assert_eq!(scalar(-3.0), 0.0);
    }

    #[test]
    fn state_is_camel_case() {
        let s = MixerState { supported: true, ..Default::default() };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["supported"], true);
        assert!(json["master"].is_null());
    }
}
