// The Win32 side of window-shim.m: floating on top, keeping out of the taskbar, dragging the window
// by the card and remembering where it was dropped. Same functions as native.rs, so main.rs doesn't
// know which one it got.
//
// Win32 works in physical pixels from the top-left of the primary screen; GPUI in logical ones.
// GPUI keeps an invisible frame around the content, so the window is placed by its content's rect.

use std::cell::Cell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONULL,
    MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromRect, ReleaseDC, SRCCOPY, SelectObject,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetClientRect, GetCursorPos, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect,
    HWND_TOPMOST, SM_SWAPBUTTON, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SET_WINDOW_POS_FLAGS, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, WS_CAPTION, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};

// Everything here runs on the main thread, where GPUI made the window.
thread_local! {
    static WINDOW: Cell<HWND> = const { Cell::new(null_mut()) };
    /// Where show_window puts the window: chosen by saved_position, before GPUI opens it.
    static OPEN_AT: Cell<RECT> = const { Cell::new(RECT { left: 0, top: 0, right: 0, bottom: 0 }) };
    /// Where the pill sat before the card had to be pushed back onto the screen to open, so that
    /// minimising puts it back there. Forgotten once the card is dragged.
    static PILL: Cell<Option<POINT>> = const { Cell::new(None) };
    static DRAG: Cell<(POINT, POINT, bool)> = const { Cell::new((POINT { x: 0, y: 0 }, POINT { x: 0, y: 0 }, false)) };
}

unsafe extern "system" fn find_gpui_window(hwnd: HWND, found: LPARAM) -> BOOL {
    let mut class = [0u16; 32];
    let n = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    if String::from_utf16_lossy(&class[..n.max(0) as usize]) == "Zed::Window" {
        unsafe { *(found as *mut HWND) = hwnd };
        return 0;
    }
    1
}

fn window() -> Option<HWND> {
    let mut hwnd = WINDOW.get();
    if hwnd.is_null() {
        unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(find_gpui_window), &mut hwnd as *mut HWND as LPARAM) };
        WINDOW.set(hwnd);
    }
    (!hwnd.is_null()).then_some(hwnd)
}

fn scale(hwnd: HWND) -> f64 {
    unsafe { GetDpiForWindow(hwnd) as f64 / 96.0 }
}

/// The content's rect on screen: the window's without GPUI's invisible frame.
fn rect_of(hwnd: HWND) -> RECT {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let mut corner = POINT { x: 0, y: 0 };
    unsafe {
        GetClientRect(hwnd, &mut r);
        ClientToScreen(hwnd, &mut corner);
    }
    sized(corner.x, corner.y, r.right, r.bottom)
}

/// Moves the window so that its content fills `r`.
fn place(hwnd: HWND, r: RECT, after: HWND, flags: SET_WINDOW_POS_FLAGS) {
    let (mut outer, inner) = (RECT { left: 0, top: 0, right: 0, bottom: 0 }, rect_of(hwnd));
    unsafe { GetWindowRect(hwnd, &mut outer) };
    let (l, t) = (r.left - (inner.left - outer.left), r.top - (inner.top - outer.top));
    let (w, h) = (r.right - r.left + (outer.right - outer.left) - (inner.right - inner.left), r.bottom - r.top + (outer.bottom - outer.top) - (inner.bottom - inner.top));
    unsafe { SetWindowPos(hwnd, after, l, t, w, h, flags) };
}

fn sized(left: i32, top: i32, width: i32, height: i32) -> RECT {
    RECT { left, top, right: left + width, bottom: top + height }
}

fn monitor_info(monitor: HMONITOR) -> MONITORINFO {
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..unsafe { std::mem::zeroed() } };
    unsafe { GetMonitorInfoW(monitor, &mut info) };
    info
}

fn monitor_scale(monitor: HMONITOR) -> f64 {
    let (mut x, mut y) = (96, 96);
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) };
    x as f64 / 96.0
}

// The frame moved, not resized, so that it fits inside the work area of the screen `anchor` is
// mostly on. The anchor is the pill when the card grows from it: by the card's own overlap, a card
// hanging off the side of a screen would be pulled onto the screen beside it.
fn fit_on_screen(frame: RECT, anchor: RECT) -> RECT {
    let work = monitor_info(unsafe { MonitorFromRect(&anchor, MONITOR_DEFAULTTOPRIMARY) }).rcWork;
    let (w, h) = (frame.right - frame.left, frame.bottom - frame.top);
    let left = frame.left.min(work.right - w).max(work.left);
    let top = frame.top.min(work.bottom - h).max(work.top);
    sized(left, top, w, h)
}

unsafe extern "system" fn collect_monitor(monitor: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> BOOL {
    unsafe { (*(list as *mut Vec<HMONITOR>)).push(monitor) };
    1
}

/// GPUI numbers the displays in the order Windows lists them.
fn display_index(monitor: HMONITOR) -> Option<u32> {
    let mut list: Vec<HMONITOR> = Vec::new();
    unsafe { EnumDisplayMonitors(null_mut(), null(), Some(collect_monitor), &mut list as *mut _ as LPARAM) };
    list.iter().position(|&m| m == monitor).map(|i| i as u32)
}

// The spot goes to a file of its own, beside the settings.
fn position_file() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("Doodoro").join("window.json")
}

fn save_position(hwnd: HWND) {
    let r = rect_of(hwnd);
    let file = position_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, serde_json::json!({ "left": r.left, "top": r.top }).to_string() + "\n");
}

fn load_position() -> Option<POINT> {
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(position_file()).ok()?).ok()?;
    Some(POINT { x: json["left"].as_i64()? as i32, y: json["top"].as_i64()? as i32 })
}

pub fn float_window() {
    let Some(hwnd) = window() else { return };
    unsafe {
        // A tool window has no taskbar button, as the Mac version has no Dock icon. Set while the
        // window is still hidden, so the taskbar never sees it.
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ((style & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW) as isize);
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

/// GPUI draws no buttons here; Windows 11 would still round the window's corners and outline it.
/// The card draws its own edge and shadow.
pub fn hide_window_buttons() {
    let Some(hwnd) = window() else { return };
    unsafe {
        // Without a caption, DWM draws no frame line along the top of the content.
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        SetWindowLongPtrW(hwnd, GWL_STYLE, (style & !WS_CAPTION) as isize);
        let corners = DWMWCP_DONOTROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &corners as *const _ as *const c_void, 4);
        let none = DWMWA_COLOR_NONE;
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as u32, &none as *const _ as *const c_void, 4);
    }
}

/// Where a `width` × `height` window was last dropped: its top-left corner in logical pixels, as
/// GPUI places windows on Windows, and the index of its display. None on first run, or when the
/// screen it was on has gone. Also settles where show_window will put it.
pub fn saved_position(width: f64, height: f64) -> Option<(f64, f64, u32)> {
    let primary = unsafe { MonitorFromRect(&sized(0, 0, 1, 1), MONITOR_DEFAULTTOPRIMARY) };
    let centered = || {
        let s = monitor_scale(primary);
        let (w, h) = ((width * s).round() as i32, (height * s).round() as i32);
        let work = monitor_info(primary).rcWork;
        sized((work.left + work.right - w) / 2, (work.top + work.bottom - h) / 2, w, h)
    };
    OPEN_AT.set(centered());
    let top_left = load_position()?;
    let corner = sized(top_left.x, top_left.y, 1, 1);
    let corner_screen = unsafe { MonitorFromRect(&corner, MONITOR_DEFAULTTONULL) };
    let s = monitor_scale(if corner_screen.is_null() { primary } else { corner_screen });
    let frame = sized(top_left.x, top_left.y, (width * s).round() as i32, (height * s).round() as i32);
    // A spot whose screen has since gone is forgotten, rather than pulled onto another screen.
    if unsafe { MonitorFromRect(&frame, MONITOR_DEFAULTTONULL) }.is_null() {
        return None;
    }
    // Kept on the screen of the corner it was dropped by, which may have been the pill's.
    let frame = fit_on_screen(frame, if corner_screen.is_null() { frame } else { corner });
    let screen = unsafe { MonitorFromRect(&frame, MONITOR_DEFAULTTOPRIMARY) };
    let s = monitor_scale(screen);
    OPEN_AT.set(frame);
    Some((frame.left as f64 / s, frame.top as f64 / s, display_index(screen)?))
}

/// Shows the window once its chrome is set up, without taking focus from the app in front.
pub fn show_window() {
    let Some(hwnd) = window() else { return };
    place(hwnd, OPEN_AT.get(), HWND_TOPMOST, SWP_SHOWWINDOW | SWP_NOACTIVATE);
}

// Where the window goes for content of width × height: its top-left corner kept, unless the card
// would then hang off the screen. Then it is nudged back onto it, with the pill it grew from at the
// returned point.
fn plan_resize(hwnd: HWND, width: f32, height: f32) -> (RECT, Option<POINT>) {
    let now = rect_of(hwnd);
    let s = scale(hwnd);
    let (w, h) = ((width as f64 * s).round() as i32, (height as f64 * s).round() as i32);
    let growing = w as i64 * h as i64 > (now.right - now.left) as i64 * (now.bottom - now.top) as i64;
    let top_left = match PILL.get() {
        Some(pill) if !growing => pill,
        _ => POINT { x: now.left, y: now.top },
    };
    let frame = sized(top_left.x, top_left.y, w, h);
    if growing {
        let fitted = fit_on_screen(frame, now);
        if (fitted.left, fitted.top) != (frame.left, frame.top) {
            return (fitted, Some(top_left));
        }
    }
    (frame, None)
}

/// Resizes the content keeping the top-left corner, unless the card would hang off the screen.
pub fn resize_window(width: f32, height: f32) {
    let Some(hwnd) = window() else { return };
    let (r, pill) = plan_resize(hwnd, width, height);
    PILL.set(pill);
    place(hwnd, r, null_mut(), SWP_NOZORDER | SWP_NOACTIVATE);
}

fn offset(hwnd: HWND, pill: Option<POINT>, from: RECT) -> (f32, f32) {
    let s = scale(hwnd);
    pill.map_or((0., 0.), |p| (((p.x - from.left) as f64 / s) as f32, ((p.y - from.top) as f64 / s) as f32))
}

/// Where the pill sits, or will go back to, from the window's top-left; off it while the card is
/// pushed onto the screen.
pub fn pill_offset() -> (f32, f32) {
    window().map_or((0., 0.), |hwnd| offset(hwnd, PILL.get(), rect_of(hwnd)))
}

/// Where the pill will sit from the window's top-left once it is resized to `width` × `height`.
pub fn pill_offset_after_resize(width: f32, height: f32) -> (f32, f32) {
    let Some(hwnd) = window() else { return (0., 0.) };
    let (r, pill) = plan_resize(hwnd, width, height);
    offset(hwnd, pill, r)
}

/// Whether the last press moved the window, so a drag doesn't also count as a click.
pub fn window_was_dragged() -> bool {
    DRAG.get().2
}

fn cursor() -> POINT {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    p
}

/// Starts moving the window with the pointer; false if there is no window.
pub fn drag_begin() -> bool {
    let Some(hwnd) = window() else { return false };
    let r = rect_of(hwnd);
    DRAG.set((cursor(), POINT { x: r.left, y: r.top }, false));
    true
}

/// Moves the window with the pointer; false once the button is up.
pub fn drag_step() -> bool {
    let Some(hwnd) = window() else { return false };
    let (mouse, origin, mut moved) = DRAG.get();
    // The primary button, which is the right one when they are swapped.
    let button = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 { VK_RBUTTON } else { VK_LBUTTON };
    if unsafe { GetAsyncKeyState(button as i32) } as u16 & 0x8000 == 0 {
        if moved {
            save_position(hwnd);
        }
        return false;
    }
    let m = cursor();
    if (m.x - mouse.x).abs() + (m.y - mouse.y).abs() > 3 {
        moved = true;
        PILL.set(None);
    }
    DRAG.set((mouse, origin, moved));
    let r = rect_of(hwnd);
    let (x, y) = (origin.x + m.x - mouse.x, origin.y + m.y - mouse.y);
    place(hwnd, sized(x, y, r.right - r.left, r.bottom - r.top), null_mut(), SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    true
}

/// Writes the window, as it shows on screen, to a PNG at `path`. GPUI presents through
/// DirectComposition, which PrintWindow can't see, so this copies the screen under the window.
pub fn screenshot(path: &str) -> bool {
    let Some(hwnd) = window() else { return false };
    let r = rect_of(hwnd);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    unsafe {
        let screen = GetDC(null_mut());
        let dc = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(dc, bitmap);
        BitBlt(dc, 0, 0, w, h, screen, r.left, r.top, SRCCOPY);
        SelectObject(dc, old);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        GetDIBits(dc, bitmap, 0, h as u32, pixels.as_mut_ptr() as *mut c_void, &mut info, DIB_RGB_COLORS);
        DeleteObject(bitmap);
        DeleteDC(dc);
        ReleaseDC(null_mut(), screen);
    }
    for px in pixels.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    image::RgbaImage::from_raw(w as u32, h as u32, pixels).is_some_and(|img| img.save(path).is_ok())
}
