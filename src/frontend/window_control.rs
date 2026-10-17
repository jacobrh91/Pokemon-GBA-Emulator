//! Window behaviour minifb does not expose: a locked content aspect ratio, a
//! minimum size and full-screen mode.
//!
//! On macOS these message the underlying `NSWindow` through the Objective-C
//! runtime. Elsewhere the calls are no-ops and minifb letterboxes the picture.

use minifb::Window;

/// The mouse position in points relative to the picture view (origin
/// top-left), with that view's size; `None` if the platform cannot say.
///
/// minifb's own mouse values are wrong in full screen and after leaving it.
pub fn pointer_in_picture_view(window: &Window) -> Option<((f32, f32), (f32, f32))> {
    #[cfg(target_os = "macos")]
    return mac::pointer_in_picture_view(window);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
}

/// The window's content size in points, excluding the title bar (falling back
/// to minifb's size off macOS).
pub fn content_size(window: &Window) -> (f32, f32) {
    #[cfg(target_os = "macos")]
    if let Some(size) = mac::content_size(window) {
        return size;
    }
    let (width, height) = window.get_size();
    (width as f32, height as f32)
}

/// Whether the left mouse button is down, read from the window system rather
/// than from events delivered to the window (a click right after toggling
/// full screen is otherwise consumed to activate the window). `None` if the
/// platform cannot say.
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

/// Turns a window-space mouse position into buffer coordinates, or `None`
/// outside the window. The inside test uses the live window size, not
/// minifb's `MouseMode::Discard`, which is stale after a resize.
pub fn pointer_in_buffer(
    raw: Option<(f32, f32)>,
    window_size: (f32, f32),
    buffer_size: (usize, usize),
) -> Option<(f32, f32)> {
    raw.filter(|position| contains(*position, window_size))
        .map(|position| window_to_buffer(position, window_size, buffer_size))
}

/// Maps a window-space point to buffer pixel coordinates. The buffer is
/// scaled to fit and centred; points over the bars map outside `0..buffer`.
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
