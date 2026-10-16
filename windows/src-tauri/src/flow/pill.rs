// The pill of Flow dictation: a small see-through capsule at the bottom of the
// screen (or on its left or right edge) while dictation is on. It opens into
// Mochi and nine dots while you speak, into a toolbar under the pointer, and
// shows a call being recorded and short messages. Port of FlowPill.swift.
//
// Its window is borderless, transparent, always on top, never takes the
// keyboard, and lets clicks through except where the pill and its bubble are:
// a cursor poll on Windows (like the island), the input region on Linux.
//
// On Windows it can be dragged anywhere: the page starts the drag, the poll
// carries the window with the cursor while the button is down, and the spot is
// kept (Flow/pill.json) for the edge it was dragged on. Another edge picked in
// the settings, or a display that is gone, puts it back in its place.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use super::store::Edge;
use crate::platform::{self, DesktopMode};

pub const LABEL: &str = "pill";

/// Window size (logical) along the bottom, and on the sides.
const BOTTOM: (f64, f64) = (520.0, 170.0);
const SIDE: (f64, f64) = (480.0, 320.0);
/// Off-screen spot on Windows while hidden: the webview keeps running, ready.
const PARKED: (i32, i32) = (-32000, -32000);

/// What the pill shows, sent whole to the page on every change.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PillState {
    /// "idle", "listening", "thinking", "locked", "copied", "failed", "pastedAgain".
    pub phase: String,
    pub edge: String,
    /// The dictation key, as the page writes it ("right_ctrl"…).
    pub key: String,
    /// When the call being recorded started (ms since 1970), or none.
    pub note_started_at: Option<u64>,
    pub announcement: Option<Announcement>,
    /// Dictation is on (the resting pill shows).
    pub enabled: bool,
    /// « OK Coucou » is talking with you.
    pub voice: Option<crate::voice::VoiceView>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Announcement {
    pub id: u64,
    /// "mic", "lock", "clipboard", "download", "check", "error".
    pub icon: String,
    pub tint: String,
    pub text: String,
    /// The button's label, if there is one.
    pub action: Option<String>,
    /// What the button does: "openSettings", "openNotes".
    pub action_id: Option<String>,
}

pub struct Pill {
    mode: DesktopMode,
    state: Mutex<PillState>,
    /// Where the page says the pill and its bubble are (window-logical rects).
    hot: Mutex<Vec<(f64, f64, f64, f64)>>,
    shown: AtomicBool,
    /// The pointer is on the pill or its bubble: a message waits for it to leave.
    pub hovered: AtomicBool,
    /// Parks the Windows cursor poll while the pill is hidden.
    poll: (Mutex<bool>, Condvar),
    announce_seq: std::sync::atomic::AtomicU64,
    /// Windows: being dragged, held this far (physical) from the window's corner.
    carry: Mutex<Option<(f64, f64)>>,
}

/// Where the pill was dragged: the window's top-left corner (physical) and the
/// edge it was on then.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spot {
    pub edge: String,
    pub x: i32,
    pub y: i32,
}

const SPOT_FILE: &str = "pill.json";

fn load_spot() -> Option<Spot> {
    let text = std::fs::read_to_string(super::store::Store::folder().join(SPOT_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_spot(spot: &Spot) {
    let folder = super::store::Store::folder();
    let _ = std::fs::create_dir_all(&folder);
    if let Ok(text) = serde_json::to_string(spot) {
        let _ = std::fs::write(folder.join(SPOT_FILE), text);
    }
}

/// A window of `size` at `top_left`, kept whole inside the work area its centre
/// is on (`areas`: x, y, width, height, physical). None when its centre is on
/// no display any more.
pub fn fit_spot(top_left: (i32, i32), size: (i32, i32), areas: &[(i32, i32, i32, i32)]) -> Option<(i32, i32)> {
    let c = (top_left.0 + size.0 / 2, top_left.1 + size.1 / 2);
    let a = areas.iter().find(|a| c.0 >= a.0 && c.0 < a.0 + a.2 && c.1 >= a.1 && c.1 < a.1 + a.3)?;
    let x = top_left.0.clamp(a.0, (a.0 + a.2 - size.0).max(a.0));
    let y = top_left.1.clamp(a.1, (a.1 + a.3 - size.1).max(a.1));
    Some((x, y))
}

fn work_areas(app: &AppHandle) -> Vec<(i32, i32, i32, i32, f64)> {
    app.available_monitors()
        .map(|list| {
            list.iter()
                .map(|m| {
                    let wa = m.work_area();
                    (wa.position.x, wa.position.y, wa.size.width as i32, wa.size.height as i32, m.scale_factor())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The saved spot for this edge, fitted to today's displays: top-left and size (physical).
fn saved_place(app: &AppHandle, edge: &str) -> Option<((i32, i32), (i32, i32))> {
    let spot = load_spot().filter(|s| s.edge == edge)?;
    let areas = work_areas(app);
    // The size at the scale of the display it was left on.
    let scale = areas
        .iter()
        .find(|a| spot.x >= a.0 && spot.x < a.0 + a.2 && spot.y >= a.1 && spot.y < a.1 + a.3)
        .map_or(1.0, |a| a.4);
    let (lw, lh) = size_for(edge);
    let size = ((lw * scale).round() as i32, (lh * scale).round() as i32);
    let rects: Vec<_> = areas.iter().map(|a| (a.0, a.1, a.2, a.3)).collect();
    Some((fit_spot((spot.x, spot.y), size, &rects)?, size))
}

/// The page saw a drag start on the pill. Windows only: false elsewhere.
pub fn drag_begin(app: &AppHandle) -> bool {
    let Some(pill) = get(app) else { return false };
    if pill.mode != DesktopMode::Poll {
        return false;
    }
    let Some(win) = window(app) else { return false };
    let (Ok(origin), Some((cx, cy))) = (win.outer_position(), platform::cursor_physical()) else { return false };
    *pill.carry.lock().unwrap() = Some((cx - origin.x as f64, cy - origin.y as f64));
    let _ = win.set_ignore_cursor_events(false);
    true
}

/// The button went up: the pill stays where it was let go, whole on its display, and is remembered there.
fn drop_here(app: &AppHandle, pill: &Pill, win: &WebviewWindow) {
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else { return };
    let rects: Vec<_> = work_areas(app).iter().map(|a| (a.0, a.1, a.2, a.3)).collect();
    let Some((x, y)) = fit_spot((pos.x, pos.y), (size.width as i32, size.height as i32), &rects) else { return };
    if (x, y) != (pos.x, pos.y) {
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
    let edge = pill.state.lock().unwrap().edge.clone();
    save_spot(&Spot { edge, x, y });
}

impl Pill {
    pub fn state(&self) -> PillState {
        self.state.lock().unwrap().clone()
    }
}

fn page_url(app: &AppHandle) -> WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path("/pill.html");
        return WebviewUrl::External(base);
    }
    let _ = app;
    WebviewUrl::App("pill.html".into())
}

/// Creates the pill's window, hidden. Before the island, for WebView2's sake
/// (see create_settings_window in lib.rs).
pub fn setup(app: &AppHandle) {
    let mut mode = platform::desktop_mode();
    if mode == DesktopMode::Off {
        crate::log::line("flow pill: no way to place it on this desktop");
    } else {
        let park = cfg!(windows);
        let mut builder = WebviewWindowBuilder::new(app, LABEL, page_url(app))
            .additional_browser_args(crate::BROWSER_ARGS)
            .title("Coucou")
            .inner_size(BOTTOM.0, BOTTOM.1)
            .resizable(cfg!(target_os = "linux"))
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible_on_all_workspaces(true)
            .focused(false)
            .maximizable(false)
            .minimizable(false)
            .closable(false)
            .disable_drag_drop_handler()
            .focusable(false)
            .visible(park);
        if park {
            builder = builder.position(PARKED.0 as f64, PARKED.1 as f64);
        }
        match builder.build() {
            Ok(win) => {
                if !platform::prepare_pill_window(&win, mode) {
                    let _ = win.destroy();
                    mode = DesktopMode::Off;
                }
            }
            Err(err) => {
                crate::log::line(format!("flow pill window failed: {err}"));
                mode = DesktopMode::Off;
            }
        }
    }
    let pill = Arc::new(Pill {
        mode,
        state: Mutex::new(PillState { phase: "idle".into(), edge: "bottom".into(), ..Default::default() }),
        hot: Mutex::new(Vec::new()),
        shown: AtomicBool::new(false),
        hovered: AtomicBool::new(false),
        poll: (Mutex::new(false), Condvar::new()),
        announce_seq: std::sync::atomic::AtomicU64::new(1),
        carry: Mutex::new(None),
    });
    app.manage(pill.clone());
    if mode == DesktopMode::Poll {
        spawn_poll(app.clone(), pill);
    }
}

/// A dictation is under way: the microphone is Flow's.
pub fn dictating(app: &AppHandle) -> bool {
    get(app).is_some_and(|p| matches!(p.state.lock().unwrap().phase.as_str(), "listening" | "locked" | "thinking"))
}

pub fn get(app: &AppHandle) -> Option<Arc<Pill>> {
    app.try_state::<Arc<Pill>>().map(|s| s.inner().clone())
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

fn size_for(edge: &str) -> (f64, f64) {
    if edge == "bottom" {
        BOTTOM
    } else {
        SIDE
    }
}

pub fn edge_name(edge: Edge) -> &'static str {
    match edge {
        Edge::Bottom => "bottom",
        Edge::Left => "left",
        Edge::Right => "right",
    }
}

/// Changes what the pill shows, and shows or hides it.
pub fn update(app: &AppHandle, change: impl FnOnce(&mut PillState)) {
    let Some(pill) = get(app) else { return };
    let (state, edge_changed) = {
        let mut s = pill.state.lock().unwrap();
        let edge_before = s.edge.clone();
        change(&mut s);
        (s.clone(), s.edge != edge_before)
    };
    let _ = app.emit_to(LABEL, "pill-state", &state);
    let visible = state.enabled || state.note_started_at.is_some() || state.announcement.is_some() || state.voice.is_some() || state.phase != "idle";
    let was = pill.shown.swap(visible, Ordering::Relaxed);
    if visible && (!was || edge_changed) {
        place(app, &pill, &state.edge, state.phase == "listening");
    }
    if visible != was {
        show(app, &pill, visible);
    }
}

fn show(app: &AppHandle, pill: &Pill, visible: bool) {
    let Some(win) = window(app) else { return };
    match pill.mode {
        DesktopMode::Poll => {
            if !visible {
                let _ = win.set_position(PhysicalPosition::new(PARKED.0, PARKED.1));
            }
            let (lock, cv) = &pill.poll;
            *lock.lock().unwrap() = visible;
            cv.notify_all();
        }
        DesktopMode::Window | DesktopMode::Layer => {
            let w = win.clone();
            let _ = app.run_on_main_thread(move || {
                if visible {
                    let _ = w.show();
                    let _ = w.set_always_on_top(true);
                } else {
                    let _ = w.hide();
                }
            });
        }
        DesktopMode::Off => {}
    }
    let _ = app.emit_to(LABEL, "pill-visible", visible);
}

/// The display to sit on: the one under the cursor when you speak, else the
/// primary one.
fn monitor(app: &AppHandle, on_cursor: bool) -> Option<tauri::Monitor> {
    if on_cursor {
        if let Some((cx, cy)) = platform::cursor_physical() {
            if let Ok(list) = app.available_monitors() {
                if let Some(m) = list.into_iter().find(|m| {
                    let (p, s) = (m.position(), m.size());
                    cx >= p.x as f64 && cx < (p.x + s.width as i32) as f64 && cy >= p.y as f64 && cy < (p.y + s.height as i32) as f64
                }) {
                    return Some(m);
                }
            }
        }
    }
    app.primary_monitor().ok().flatten()
}

fn place(app: &AppHandle, pill: &Pill, edge: &str, on_cursor: bool) {
    let Some(win) = window(app) else { return };
    let (lw, lh) = size_for(edge);
    match pill.mode {
        DesktopMode::Poll | DesktopMode::Window => {
            // Where it was dragged to, if it was, on this edge.
            if let Some(((x, y), (pw, ph))) = saved_place(app, edge) {
                #[cfg(target_os = "linux")]
                let _ = win.set_resizable(true);
                let _ = win.set_size(PhysicalSize::new(pw as u32, ph as u32));
                let _ = win.set_position(PhysicalPosition::new(x, y));
                return;
            }
            let Some(m) = monitor(app, on_cursor) else { return };
            let scale = m.scale_factor();
            let wa = m.work_area();
            let (pw, ph) = ((lw * scale).round() as i32, (lh * scale).round() as i32);
            let (ax, ay, aw, ah) = (wa.position.x, wa.position.y, wa.size.width as i32, wa.size.height as i32);
            let gap = (4.0 * scale).round() as i32;
            let (x, y) = match edge {
                "left" => (ax + gap, ay + (ah - ph) / 2),
                "right" => (ax + aw - pw - gap, ay + (ah - ph) / 2),
                _ => (ax + (aw - pw) / 2, ay + ah - ph - gap),
            };
            #[cfg(target_os = "linux")]
            let _ = win.set_resizable(true);
            let _ = win.set_size(PhysicalSize::new(pw as u32, ph as u32));
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
        DesktopMode::Layer => {
            let edge = edge.to_string();
            let w = win.clone();
            let _ = app.run_on_main_thread(move || {
                platform::set_pill_edge(&w, &edge);
                let _ = w.set_size(tauri::LogicalSize::new(lw, lh));
            });
        }
        DesktopMode::Off => {}
    }
}

/// The page tells where the pill and its bubble are, in window-logical pixels.
pub fn set_hot(app: &AppHandle, rects: Vec<(f64, f64, f64, f64)>) {
    let Some(pill) = get(app) else { return };
    *pill.hot.lock().unwrap() = rects.clone();
    if pill.mode != DesktopMode::Poll {
        if let Some(win) = window(app) {
            let _ = app.run_on_main_thread(move || platform::set_pill_input(&win, rects));
        }
    }
}

/// Windows: ~30 times a second while the pill shows, the window takes the
/// mouse only over the pill and its bubble.
fn spawn_poll(app: AppHandle, pill: Arc<Pill>) {
    std::thread::spawn(move || {
        let mut ignoring: Option<bool> = None;
        loop {
            {
                let (lock, cv) = &pill.poll;
                let mut on = lock.lock().unwrap();
                while !*on {
                    ignoring = None;
                    on = cv.wait(on).unwrap();
                }
            }
            // Being dragged: the window follows the cursor until the button goes up.
            let carry = *pill.carry.lock().unwrap();
            if let Some(grab) = carry {
                std::thread::sleep(Duration::from_millis(8));
                let Some(win) = window(&app) else { continue };
                let Some((cx, cy)) = platform::cursor_physical() else { continue };
                if platform::left_button_down() {
                    let to = PhysicalPosition::new((cx - grab.0).round() as i32, (cy - grab.1).round() as i32);
                    if win.outer_position().ok() != Some(to) {
                        let _ = win.set_position(to);
                    }
                } else {
                    *pill.carry.lock().unwrap() = None;
                    drop_here(&app, &pill, &win);
                    // Decide again who takes the mouse, from where it is now.
                    ignoring = None;
                }
                continue;
            }
            std::thread::sleep(Duration::from_millis(33));
            let Some(win) = window(&app) else { continue };
            let (Ok(origin), Some((cx, cy))) = (win.outer_position(), platform::cursor_physical()) else { continue };
            let scale = win.scale_factor().unwrap_or(1.0);
            let (x, y) = ((cx - origin.x as f64) / scale, (cy - origin.y as f64) / scale);
            let over = pill.hot.lock().unwrap().iter().any(|&(rx, ry, rw, rh)| x >= rx && x <= rx + rw && y >= ry && y <= ry + rh);
            if ignoring != Some(!over) {
                ignoring = Some(!over);
                let _ = win.set_ignore_cursor_events(!over);
                let _ = win.emit("pill-pointer", over);
            }
        }
    });
}

/// A message next to the pill, with a button to act; it goes away by itself
/// after a while unless the pointer is on it (the page keeps it then).
pub fn announce(app: &AppHandle, icon: &str, tint: &str, text: String, action: Option<(String, &str)>, seconds: u64) {
    let Some(pill) = get(app) else { return };
    let id = pill.announce_seq.fetch_add(1, Ordering::Relaxed);
    let (label, action_id) = match action {
        Some((label, id)) => (Some(label), Some(id.to_string())),
        None => (None, None),
    };
    let a = Announcement { id, icon: icon.into(), tint: tint.into(), text, action: label, action_id };
    update(app, |s| s.announcement = Some(a));
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(seconds));
        while get(&app).is_some_and(|p| p.hovered.load(Ordering::Relaxed)) {
            std::thread::sleep(Duration::from_secs(1));
        }
        dismiss(&app, id);
    });
}

/// Takes a message away (its ✕, its button, or its time ran out). The page
/// asks again later while the pointer rests on it.
pub fn dismiss(app: &AppHandle, id: u64) {
    update(app, |s| {
        if s.announcement.as_ref().is_some_and(|a| a.id == id) {
            s.announcement = None;
        }
    });
}

/// Back to rest after a short message.
pub fn rest_after(app: &AppHandle, phase: &str, ms: u64) {
    let app = app.clone();
    let phase = phase.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        update(&app, |s| {
            if s.phase == phase {
                s.phase = "idle".into();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::fit_spot;

    const TWO: [(i32, i32, i32, i32); 2] = [(0, 0, 1920, 1040), (1920, 0, 2560, 1400)];

    #[test]
    fn a_spot_on_a_display_stays_where_it_is() {
        assert_eq!(fit_spot((700, 600), (520, 170), &TWO), Some((700, 600)));
        assert_eq!(fit_spot((2500, 900), (520, 170), &TWO), Some((2500, 900)));
    }

    #[test]
    fn a_spot_half_off_its_display_is_brought_back_whole() {
        // past the taskbar at the bottom, and past the left side
        assert_eq!(fit_spot((-100, 950), (520, 170), &TWO), Some((0, 870)));
        // its centre on the second display: kept inside that one
        assert_eq!(fit_spot((4100, 100), (520, 170), &TWO), Some((3960, 100)));
    }

    #[test]
    fn a_spot_on_a_display_that_is_gone_is_dropped() {
        assert_eq!(fit_spot((5000, 300), (520, 170), &TWO), None);
        assert_eq!(fit_spot((100, 100), (520, 170), &[]), None);
    }
}
