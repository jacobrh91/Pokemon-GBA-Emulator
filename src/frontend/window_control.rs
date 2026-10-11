//! Window behaviour minifb does not expose: a locked content aspect ratio, a
//! minimum size and full-screen mode.
//!
//! These are implemented on macOS by messaging the underlying `NSWindow`
//! through the Objective-C runtime (which minifb already links). On other
//! platforms the calls are no-ops; the window is still resizable and the
//! picture is letterboxed by minifb's aspect-ratio scale mode.

use minifb::Window;

/// The mouse position in the coordinates of the view the picture is drawn
/// into (origin top-left, in points), together with that view's size. `None`
/// when the platform cannot say.
///
/// This asks the window system for the pointer relative to the picture view
/// itself. minifb's own mouse values are measured against a content rectangle
/// it derives from the window frame, which is wrong in full screen (off by the
/// title-bar height) and after leaving full screen (where its view ends up
/// smaller than that rectangle), so using them made the dock unreachable.
pub fn pointer_in_picture_view(window: &Window) -> Option<((f32, f32), (f32, f32))> {
    #[cfg(target_os = "macos")]
    return mac::pointer_in_picture_view(window);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
}

/// Whether the left mouse button is physically down, read from the window
/// system rather than from events delivered to the window.
///
/// minifb only learns about a click if the window receives the mouse-down
/// event. While the window is not key (during the full-screen transition and
/// until it is re-activated afterwards) the first click is consumed to
/// activate it, so a click on the dock right after toggling full screen was
/// lost. `None` when the platform cannot say.
pub fn left_button_down() -> Option<bool> {
    #[cfg(target_os = "macos")]
    return Some(mac::left_button_down());
    #[cfg(not(target_os = "macos"))]
    None
}

/// True if `position` lies inside a window of `size`.
pub fn contains(position: (f32, f32), size: (f32, f32)) -> bool {
    position.0 >= 0.0 && position.1 >= 0.0 && position.0 < size.0 && position.1 < size.1
}

/// Turns a raw window-space mouse position into buffer coordinates, or
/// `None` when the cursor is outside the window.
///
/// The inside-the-window test is done here against the *live* window size
/// instead of by minifb's `MouseMode::Discard`, which clips against a size it
/// only refreshes at the end of a live-resize drag. After a resize that stale
/// size threw away every cursor position in the newly exposed area, which
/// included the dock, so its buttons stopped responding until minifb caught up.
pub fn pointer_in_buffer(
    raw: Option<(f32, f32)>,
    window_size: (f32, f32),
    buffer_size: (usize, usize),
) -> Option<(f32, f32)> {
    raw.filter(|position| contains(*position, window_size))
        .map(|position| window_to_buffer(position, window_size, buffer_size))
}

/// Maps a window-space point to buffer pixel coordinates.
///
/// The buffer is scaled uniformly to fit the window and centred, so there may
/// be bars on two sides; points over a bar map outside `0..buffer`, which is
/// what callers want (the cursor is still "in the window").
pub fn window_to_buffer(
    position: (f32, f32),
    window_size: (f32, f32),
    buffer_size: (usize, usize),
) -> (f32, f32) {
    let (window_width, window_height) = window_size;
    let (buffer_width, buffer_height) = (buffer_size.0 as f32, buffer_size.1 as f32);
    if window_width <= 0.0 || window_height <= 0.0 {
        return position;
    }
    let scale = (window_width / buffer_width).min(window_height / buffer_height);
    let offset_x = (window_width - buffer_width * scale) / 2.0;
    let offset_y = (window_height - buffer_height * scale) / 2.0;
    (
        (position.0 - offset_x) / scale,
        (position.1 - offset_y) / scale,
    )
}

/// Keeps the window's content area at `width : height` while it is resized,
/// and stops it shrinking below `min_width`×`min_height`.
pub fn lock_aspect_ratio(
    window: &Window,
    width: usize,
    height: usize,
    min_width: usize,
    min_height: usize,
) {
    #[cfg(target_os = "macos")]
    mac::lock_aspect_ratio(window, width, height, min_width, min_height);
    #[cfg(not(target_os = "macos"))]
    let _ = (window, width, height, min_width, min_height);
}

/// Allows the window to enter native full-screen mode.
pub fn enable_fullscreen(window: &Window) {
    #[cfg(target_os = "macos")]
    mac::enable_fullscreen(window);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// Switches between windowed and full-screen mode.
pub fn toggle_fullscreen(window: &Window) {
    #[cfg(target_os = "macos")]
    mac::toggle_fullscreen(window);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// True while the window is in full-screen mode.
pub fn is_fullscreen(window: &Window) -> bool {
    #[cfg(target_os = "macos")]
    return mac::is_fullscreen(window);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        false
    }
}

#[cfg(target_os = "macos")]
mod mac;

#[cfg(test)]
mod tests;
