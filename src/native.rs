// The AppKit helpers in window-shim.m: floating on top, hiding the traffic lights, dragging the
// window by the card and remembering where it was dropped.

use std::ffi::{CString, c_char, c_int, c_long, c_uint};

unsafe extern "C" {
    fn pomodoro_float() -> c_long;
    fn pomodoro_hide_buttons() -> c_long;
    fn pomodoro_saved_position(width: f64, height: f64, x: *mut f64, y: *mut f64, display: *mut c_uint) -> c_int;
    fn pomodoro_show() -> c_long;
    fn pomodoro_resize(width: c_long, height: c_long) -> c_long;
    fn pomodoro_pill_x() -> c_long;
    fn pomodoro_pill_y() -> c_long;
    fn pomodoro_drag_begin() -> c_long;
    fn pomodoro_drag_step() -> c_long;
    fn pomodoro_drag_moved() -> c_long;
    fn pomodoro_screenshot(path: *const c_char) -> c_long;
}

/// Float above other apps' windows and follow you across Spaces.
pub fn float_window() {
    unsafe { pomodoro_float() };
}

pub fn hide_window_buttons() {
    unsafe { pomodoro_hide_buttons() };
}

/// Where a `width` × `height` window was last dropped: its top-left corner from the top-left of
/// its screen, and that screen's display id. None on first run.
pub fn saved_position(width: f64, height: f64) -> Option<(f64, f64, u32)> {
    let (mut x, mut y, mut display) = (0.0, 0.0, 0);
    (unsafe { pomodoro_saved_position(width, height, &mut x, &mut y, &mut display) } != 0).then_some((x, y, display))
}

pub fn show_window() {
    unsafe { pomodoro_show() };
}

/// Resizes the content keeping the top-left corner, unless the card would hang off the screen.
pub fn resize_window(width: f32, height: f32) {
    unsafe { pomodoro_resize(width as c_long, height as c_long) };
}

/// Where the pill sits, or will go back to, from the window's top-left; off it while the card is
/// pushed onto the screen.
pub fn pill_offset() -> (f32, f32) {
    unsafe { (pomodoro_pill_x() as f32, pomodoro_pill_y() as f32) }
}

/// Whether the last press moved the window, so a drag doesn't also count as a click.
pub fn window_was_dragged() -> bool {
    unsafe { pomodoro_drag_moved() != 0 }
}

/// Starts moving the window with the pointer; false if there is no window.
pub fn drag_begin() -> bool {
    unsafe { pomodoro_drag_begin() != 0 }
}

/// Moves the window with the pointer; false once the button is up.
pub fn drag_step() -> bool {
    unsafe { pomodoro_drag_step() != 0 }
}

/// Writes the window, as drawn, to a PNG at `path`.
pub fn screenshot(path: &str) -> bool {
    let Ok(path) = CString::new(path) else { return false };
    unsafe { pomodoro_screenshot(path.as_ptr()) != 0 }
}
