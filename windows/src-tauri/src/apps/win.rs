//! The app panel on Windows: the Start menu's shortcuts, the programs with a
//! window open, their icons, and opening or bringing one forward. Every Shell
//! call runs on a thread of its own with COM set up there (`with_com`).

use std::path::{Path, PathBuf};

use ::windows::core::{w, Interface, PCWSTR};
use ::windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use ::windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use ::windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use ::windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use ::windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    COINIT_DISABLE_OLE1DDE, STGM_READ,
};
use ::windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use ::windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
use ::windows::Win32::UI::Shell::{
    IShellLinkW, SHGetFileInfoW, SHGetImageList, ShellExecuteExW, ShellLink, SEE_MASK_FLAG_NO_UI, SHELLEXECUTEINFOW,
    SHFILEINFOW, SHGFI_SYSICONINDEX, SHIL_EXTRALARGE,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, EnumWindows, GetClassNameW, GetIconInfo, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER,
    HICON, ICONINFO, SW_RESTORE, SW_SHOWNORMAL, WS_EX_TOOLWINDOW,
};

use super::logic::{exe_key, exe_of_shortcut, Entry};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Runs `f` on a new thread with an apartment of its own: the Shell's objects
/// want one, and a Tauri thread may already be in another.
fn with_com<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::spawn(move || unsafe {
        let ok = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).is_ok();
        let out = f();
        if ok {
            CoUninitialize();
        }
        out
    })
    .join()
    .ok()
}

// ── The Start menu ────────────────────────────────────────────────────────────

fn start_menu_folders() -> Vec<PathBuf> {
    let mut out = Vec::new();
    // The user's own first: an entry there wins over the one for all users.
    if let Ok(app_data) = std::env::var("APPDATA") {
        out.push(Path::new(&app_data).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(program_data) = std::env::var("ProgramData") {
        out.push(Path::new(&program_data).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    out
}

fn shortcuts_in(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for item in read.flatten() {
        let path = item.path();
        if path.is_dir() {
            if depth < 3 {
                shortcuts_in(&path, depth + 1, out);
            }
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
            out.push(path);
        }
    }
}

/// What a shortcut starts: (target, arguments). Empty for a Store app's or a
/// folder's shortcut.
unsafe fn shortcut_target(link: &IShellLinkW, path: &Path) -> (String, String) {
    let Ok(file) = link.cast::<IPersistFile>() else { return Default::default() };
    let p = wide(&path.to_string_lossy());
    if file.Load(PCWSTR(p.as_ptr()), STGM_READ).is_err() {
        return Default::default();
    }
    let mut target = [0u16; 1024];
    let _ = link.GetPath(&mut target, std::ptr::null_mut(), 0);
    let mut args = [0u16; 1024];
    let _ = link.GetArguments(&mut args);
    (from_wide(&target), from_wide(&args))
}

/// Every program in the Start menu (logic::tidy_catalog sorts and cleans it).
pub fn catalog() -> Vec<Entry> {
    with_com(|| unsafe {
        let mut files = Vec::new();
        for dir in start_menu_folders() {
            shortcuts_in(&dir, 0, &mut files);
        }
        let link: Option<IShellLinkW> = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok();
        files
            .into_iter()
            .map(|path| {
                let (target, args) = link.as_ref().map(|l| shortcut_target(l, &path)).unwrap_or_default();
                let exe = if target.to_lowercase().ends_with(".exe") { exe_of_shortcut(&target, &args) } else { String::new() };
                Entry {
                    name: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                    path: path.to_string_lossy().to_string(),
                    exe,
                }
            })
            .collect()
    })
    .unwrap_or_default()
}

/// The file explorer, pinned first on a new install.
pub fn explorer() -> Entry {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    Entry { name: "Explorer".into(), path: format!(r"{windir}\explorer.exe"), exe: "explorer".into() }
}

// ── The open windows ──────────────────────────────────────────────────────────

/// A program's window: its handle and the program's full path.
struct AppWindow {
    hwnd: isize,
    exe_path: String,
}

fn exe_path_of(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, ::windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The windows a user would call open programs, front first: visible, titled,
/// nobody's dialog, not a tool window, not a hidden ("cloaked") Store window.
/// The desktop and taskbar are left out; File Explorer's own windows count.
fn app_windows() -> Vec<AppWindow> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> ::windows::core::BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<(isize, u32)>) };
        unsafe {
            let visible = IsWindowVisible(hwnd).as_bool() && GetWindowTextLengthW(hwnd) > 0;
            let owned = GetWindow(hwnd, GW_OWNER).is_ok();
            let tool = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize != 0;
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
            if visible && !owned && !tool && cloaked == 0 {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
                list.push((hwnd.0 as isize, pid));
            }
        }
        true.into()
    }
    let mut raw: Vec<(isize, u32)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut raw as *mut _ as isize));
    }
    let me = std::process::id();
    raw.into_iter()
        .filter(|(_, pid)| *pid != me && *pid != 0)
        .filter_map(|(hwnd, pid)| {
            let exe_path = exe_path_of(pid)?;
            let key = exe_key(&exe_path);
            let explorer_window = key == "explorer" && class_of(HWND(hwnd as *mut _)) == "CabinetWClass";
            if crate::flow::is_shell(&key) && !explorer_window {
                return None;
            }
            Some(AppWindow { hwnd, exe_path })
        })
        .collect()
}

/// The programs with a window open, front first, named from the Start menu
/// when it has them.
pub fn running(catalog: &[Entry]) -> Vec<Entry> {
    app_windows()
        .into_iter()
        .map(|w| {
            let exe = exe_key(&w.exe_path);
            let name = catalog
                .iter()
                .find(|e| e.exe == exe)
                .map(|e| e.name.clone())
                .unwrap_or_else(|| crate::flow::display_name(&exe));
            Entry { name, path: w.exe_path, exe }
        })
        .collect()
}

// ── Opening ───────────────────────────────────────────────────────────────────

fn bring_forward(raw: isize) -> bool {
    let hwnd = HWND(raw as *mut _);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // Allowed: the click on the island that asked for this was the last input.
        SetForegroundWindow(hwnd).as_bool()
    }
}

/// A click on a program: its front window comes forward if it has one, else
/// it starts (the shortcut or the program itself).
pub fn open(path: &str, exe: &str) -> bool {
    if !exe.is_empty() {
        if let Some(w) = app_windows().into_iter().find(|w| exe_key(&w.exe_path) == exe) {
            if bring_forward(w.hwnd) {
                return true;
            }
        }
    }
    let path = path.to_string();
    with_com(move || unsafe {
        let file = wide(&path);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_FLAG_NO_UI,
            lpVerb: w!("open"),
            lpFile: PCWSTR(file.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut info).is_ok()
    })
    .unwrap_or(false)
}

// ── Icons ─────────────────────────────────────────────────────────────────────

/// A bitmap's pixels, top row first, as BGRA.
unsafe fn pixels(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    let mut bm = BITMAP::default();
    let got = GetObjectW(HGDIOBJ(bitmap.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut _));
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 || bm.bmWidth > 256 || bm.bmHeight > 256 {
        return None;
    }
    let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32), // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(None);
    let lines = GetDIBits(dc, bitmap, 0, h, Some(buf.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    (lines == h as i32).then_some((w, h, buf))
}

/// An icon as RGBA. Old icons have no alpha: their mask says what is transparent.
unsafe fn icon_rgba(icon: HICON) -> Option<(u32, u32, Vec<u8>)> {
    let mut ii = ICONINFO::default();
    GetIconInfo(icon, &mut ii).ok()?;
    let color = pixels(ii.hbmColor);
    let mask = pixels(ii.hbmMask);
    let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
    let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
    let (w, h, mut px) = color?;
    let has_alpha = px.chunks(4).any(|p| p[3] != 0);
    for (i, p) in px.chunks_mut(4).enumerate() {
        p.swap(0, 2); // BGRA → RGBA
        if !has_alpha {
            let opaque = mask.as_ref().is_none_or(|(_, _, m)| m.get(i * 4).copied().unwrap_or(0) == 0);
            p[3] = if opaque { 255 } else { 0 };
        }
    }
    Some((w, h, px))
}

/// The icon Explorer shows for `path` (48×48), as PNG.
pub fn icon_png(path: &str) -> Option<Vec<u8>> {
    let path = path.to_string();
    with_com(move || unsafe {
        let p = wide(&path);
        let mut info = SHFILEINFOW::default();
        let found = SHGetFileInfoW(
            PCWSTR(p.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info as *mut SHFILEINFOW),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX,
        );
        if found == 0 {
            return None;
        }
        let list: IImageList = SHGetImageList(SHIL_EXTRALARGE as i32).ok()?;
        let icon = list.GetIcon(info.iIcon, ILD_TRANSPARENT.0).ok()?;
        let rgba = icon_rgba(icon);
        let _ = DestroyIcon(icon);
        let (w, h, px) = rgba?;
        super::png::encode_rgba(w, h, &px)
    })
    .flatten()
}
