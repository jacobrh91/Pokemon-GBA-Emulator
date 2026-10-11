//! Window behaviour minifb does not expose: a locked content aspect ratio, a
//! minimum size and full-screen mode.
//!
//! These are implemented on macOS by messaging the underlying `NSWindow`
//! through the Objective-C runtime (which minifb already links). On other
//! platforms the calls are no-ops; the window is still resizable and the
//! picture is letterboxed by minifb's aspect-ratio scale mode.

use minifb::Window;

/// The window's current content size in points, read from the window system
/// each call.
///
/// minifb's `Window::get_size` only refreshes when a live-resize drag ends
/// (and not at all for other resizes), so after a resize it can be badly
/// out of date. `None` when the platform offers no live value.
#[cfg_attr(not(test), allow(dead_code))]
pub fn content_size(window: &Window) -> Option<(f32, f32)> {
    #[cfg(target_os = "macos")]
    return mac::content_size(window);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
}

/// Resizes the window's content area. For verification and tests.
#[cfg_attr(not(test), allow(dead_code))]
pub fn set_content_size(window: &Window, width: usize, height: usize) {
    #[cfg(target_os = "macos")]
    mac::set_content_size(window, width, height);
    #[cfg(not(target_os = "macos"))]
    let _ = (window, width, height);
}

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

/// The locked content aspect ratio `(width, height)`, if the platform
/// reports one. Used by `examples/window_probe` to verify the lock.
#[cfg_attr(not(test), allow(dead_code))]
pub fn content_aspect_ratio(window: &Window) -> Option<(f64, f64)> {
    #[cfg(target_os = "macos")]
    return mac::content_aspect_ratio(window);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
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
mod mac {
    use super::Window;
    use std::ffi::{CStr, c_char, c_void};

    /// `NSSize`: two `CGFloat`s, passed in floating-point registers.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NsSize {
        width: f64,
        height: f64,
    }

    /// `NSWindowCollectionBehaviorFullScreenPrimary`.
    const FULL_SCREEN_PRIMARY: usize = 1 << 7;
    /// `NSWindowStyleMaskFullScreen`.
    const STYLE_MASK_FULL_SCREEN: usize = 1 << 14;

    #[link(name = "objc")]
    unsafe extern "C" {
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
        #[cfg(not(target_arch = "aarch64"))]
        fn objc_msgSend_stret();
    }

    fn selector(name: &CStr) -> *mut c_void {
        // Safety: `name` is a valid NUL-terminated string.
        unsafe { sel_registerName(name.as_ptr()) }
    }

    /// The `NSWindow` behind a minifb window (valid for the window's life).
    fn ns_window(window: &Window) -> *mut c_void {
        window.get_window_handle()
    }

    /// Sends a message taking one `NSSize` argument.
    ///
    /// # Safety
    /// `object` must be a live Objective-C object that responds to `name`
    /// with signature `(NSSize) -> void`, and the call must be on the main
    /// thread for AppKit objects.
    unsafe fn send_size(object: *mut c_void, name: &CStr, size: NsSize) {
        // Safety: objc_msgSend is a trampoline that must be called through a
        // pointer of the callee's real signature.
        unsafe {
            let call: unsafe extern "C" fn(*mut c_void, *mut c_void, NsSize) =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            call(object, selector(name), size);
        }
    }

    pub fn lock_aspect_ratio(
        window: &Window,
        width: usize,
        height: usize,
        min_width: usize,
        min_height: usize,
    ) {
        let object = ns_window(window);
        if object.is_null() {
            return;
        }
        // Safety: `object` is minifb's live NSWindow and we are on the main
        // thread, which is where the window was created.
        unsafe {
            send_size(
                object,
                c"setContentAspectRatio:",
                NsSize {
                    width: width as f64,
                    height: height as f64,
                },
            );
            send_size(
                object,
                c"setContentMinSize:",
                NsSize {
                    width: min_width as f64,
                    height: min_height as f64,
                },
            );
        }
    }

    pub fn enable_fullscreen(window: &Window) {
        let object = ns_window(window);
        if object.is_null() {
            return;
        }
        // Safety: as above; `setCollectionBehavior:` takes an NSUInteger.
        unsafe {
            let call: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            call(
                object,
                selector(c"setCollectionBehavior:"),
                FULL_SCREEN_PRIMARY,
            );
        }
    }

    pub fn toggle_fullscreen(window: &Window) {
        let object = ns_window(window);
        if object.is_null() {
            return;
        }
        // Safety: `toggleFullScreen:` takes one object argument (the sender).
        unsafe {
            let call: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            call(object, selector(c"toggleFullScreen:"), std::ptr::null_mut());
        }
    }

    #[allow(dead_code)]
    pub fn set_content_size(window: &Window, width: usize, height: usize) {
        let object = ns_window(window);
        if object.is_null() {
            return;
        }
        // Safety: `setContentSize:` takes an NSSize; main thread as above.
        unsafe {
            send_size(
                object,
                c"setContentSize:",
                NsSize {
                    width: width as f64,
                    height: height as f64,
                },
            );
        }
    }

    /// `NSRect`: origin then size, four `CGFloat`s.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct NsRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    /// Sends a message that returns an `NSRect`.
    ///
    /// # Safety
    /// `object` must be a live Objective-C object responding to `name` with
    /// signature `() -> NSRect`.
    unsafe fn send_rect(object: *mut c_void, name: &CStr) -> NsRect {
        // A 32-byte struct comes back in registers (d0-d3) on arm64 but in
        // memory, through `objc_msgSend_stret`, on x86_64.
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let call: unsafe extern "C" fn(*mut c_void, *mut c_void) -> NsRect =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            call(object, selector(name))
        }
        #[cfg(not(target_arch = "aarch64"))]
        unsafe {
            let mut result = NsRect::default();
            let call: unsafe extern "C" fn(*mut NsRect, *mut c_void, *mut c_void) =
                std::mem::transmute(objc_msgSend_stret as unsafe extern "C" fn());
            call(&mut result, object, selector(name));
            result
        }
    }

    /// The view minifb draws the picture into: the last subview of the
    /// window's content view (an `MTKView`), falling back to the content
    /// view itself. The content view reaches under the title bar, so it is not
    /// the visible picture area.
    ///
    /// # Safety
    /// `object` must be a live `NSWindow`, used from the main thread.
    unsafe fn picture_view(object: *mut c_void) -> *mut c_void {
        // Safety: each message takes no arguments and returns an object.
        unsafe {
            let object_message = |target: *mut c_void, name: &CStr| -> *mut c_void {
                let call: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                call(target, selector(name))
            };
            let content_view = object_message(object, c"contentView");
            if content_view.is_null() {
                return std::ptr::null_mut();
            }
            let subviews = object_message(content_view, c"subviews");
            let last = if subviews.is_null() {
                std::ptr::null_mut()
            } else {
                object_message(subviews, c"lastObject")
            };
            if last.is_null() { content_view } else { last }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn content_size(window: &Window) -> Option<(f32, f32)> {
        let object = ns_window(window);
        if object.is_null() {
            return None;
        }
        // Safety: `object` is minifb's live NSWindow; main thread.
        let bounds = unsafe {
            let view = picture_view(object);
            if view.is_null() {
                return None;
            }
            send_rect(view, c"bounds")
        };
        (bounds.width > 0.0 && bounds.height > 0.0)
            .then_some((bounds.width as f32, bounds.height as f32))
    }

    /// `NSPoint`: two `CGFloat`s, returned in floating-point registers.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NsPoint {
        x: f64,
        y: f64,
    }

    pub fn pointer_in_picture_view(window: &Window) -> Option<((f32, f32), (f32, f32))> {
        let object = ns_window(window);
        if object.is_null() {
            return None;
        }
        // Safety: `mouseLocationOutsideOfEventStream` takes no arguments and
        // returns an NSPoint in window coordinates; `convertPoint:fromView:`
        // takes (NSPoint, view) and returns an NSPoint, with a nil view
        // meaning window coordinates. Main thread as above.
        unsafe {
            let view = picture_view(object);
            if view.is_null() {
                return None;
            }
            let in_window: NsPoint = {
                let call: unsafe extern "C" fn(*mut c_void, *mut c_void) -> NsPoint =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                call(object, selector(c"mouseLocationOutsideOfEventStream"))
            };
            let in_view: NsPoint = {
                let call: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    NsPoint,
                    *mut c_void,
                ) -> NsPoint = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                call(
                    view,
                    selector(c"convertPoint:fromView:"),
                    in_window,
                    std::ptr::null_mut(),
                )
            };
            let bounds = send_rect(view, c"bounds");
            if bounds.width <= 0.0 || bounds.height <= 0.0 {
                return None;
            }
            // AppKit views are bottom-left origin unless flipped; the picture
            // view is not flipped, so measure Y from the top.
            let flipped: bool = {
                let call: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                call(view, selector(c"isFlipped"))
            };
            let y = if flipped {
                in_view.y
            } else {
                bounds.height - in_view.y
            };
            Some((
                (in_view.x as f32, y as f32),
                (bounds.width as f32, bounds.height as f32),
            ))
        }
    }

    #[allow(dead_code)]
    pub fn content_aspect_ratio(window: &Window) -> Option<(f64, f64)> {
        let object = ns_window(window);
        if object.is_null() {
            return None;
        }
        // Safety: `contentAspectRatio` takes no arguments and returns an
        // NSSize (two doubles, returned in floating-point registers).
        let size = unsafe {
            let call: unsafe extern "C" fn(*mut c_void, *mut c_void) -> NsSize =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            call(object, selector(c"contentAspectRatio"))
        };
        Some((size.width, size.height))
    }

    pub fn is_fullscreen(window: &Window) -> bool {
        let object = ns_window(window);
        if object.is_null() {
            return false;
        }
        // Safety: `styleMask` takes no arguments and returns an NSUInteger.
        let mask = unsafe {
            let call: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            call(object, selector(c"styleMask"))
        };
        mask & STYLE_MASK_FULL_SCREEN != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUFFER: (usize, usize) = (960, 640);

    fn close(a: (f32, f32), b: (f32, f32)) {
        assert!(
            (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01,
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn identical_sizes_map_one_to_one() {
        close(
            window_to_buffer((123.0, 45.0), (960.0, 640.0), BUFFER),
            (123.0, 45.0),
        );
    }

    #[test]
    fn a_larger_window_of_the_same_ratio_scales_uniformly() {
        // 2x: window point (1000, 500) is buffer (500, 250).
        close(
            window_to_buffer((1000.0, 500.0), (1920.0, 1280.0), BUFFER),
            (500.0, 250.0),
        );
    }

    #[test]
    fn a_wider_window_adds_side_bars_that_shift_the_x_axis() {
        // 1920x640 window: scale 1.0, 480 px bars on each side.
        close(
            window_to_buffer((480.0, 0.0), (1920.0, 640.0), BUFFER),
            (0.0, 0.0),
        );
        close(
            window_to_buffer((1440.0, 640.0), (1920.0, 640.0), BUFFER),
            (960.0, 640.0),
        );
        // A point over the left bar is left of the picture.
        let over_bar = window_to_buffer((100.0, 100.0), (1920.0, 640.0), BUFFER);
        assert!(over_bar.0 < 0.0);
    }

    #[test]
    fn a_taller_window_adds_top_and_bottom_bars() {
        // 960x1000: scale 1.0, 180 px bars above and below.
        close(
            window_to_buffer((10.0, 180.0), (960.0, 1000.0), BUFFER),
            (10.0, 0.0),
        );
        let over_bar = window_to_buffer((10.0, 20.0), (960.0, 1000.0), BUFFER);
        assert!(over_bar.1 < 0.0);
        let below = window_to_buffer((10.0, 980.0), (960.0, 1000.0), BUFFER);
        assert!(below.1 > 640.0);
    }

    #[test]
    fn the_centre_of_the_window_is_the_centre_of_the_picture() {
        for size in [
            (1280.0, 720.0),
            (700.0, 1200.0),
            (3456.0, 2234.0),
            (480.0, 320.0),
        ] {
            let centre = (size.0 / 2.0, size.1 / 2.0);
            close(window_to_buffer(centre, size, BUFFER), (480.0, 320.0));
        }
    }

    #[test]
    fn a_degenerate_window_size_leaves_the_position_alone() {
        close(window_to_buffer((5.0, 6.0), (0.0, 0.0), BUFFER), (5.0, 6.0));
    }

    #[test]
    fn contains_is_a_half_open_rectangle_test() {
        assert!(contains((0.0, 0.0), (100.0, 50.0)));
        assert!(contains((99.9, 49.9), (100.0, 50.0)));
        assert!(!contains((100.0, 10.0), (100.0, 50.0)));
        assert!(!contains((10.0, 50.0), (100.0, 50.0)));
        assert!(!contains((-0.1, 10.0), (100.0, 50.0)));
    }

    #[test]
    fn pointer_is_dropped_outside_the_live_window_and_mapped_inside() {
        let live = (1200.0, 800.0);
        // Inside the enlarged window, in the area an older 960x640 size
        // would have clipped away: it must be kept and mapped.
        let kept = pointer_in_buffer(Some((1100.0, 700.0)), live, BUFFER).expect("inside");
        close(kept, (880.0, 560.0));
        // Outside the live window is dropped, and so is no pointer at all.
        assert_eq!(pointer_in_buffer(Some((1200.0, 100.0)), live, BUFFER), None);
        assert_eq!(pointer_in_buffer(Some((-1.0, 100.0)), live, BUFFER), None);
        assert_eq!(pointer_in_buffer(None, live, BUFFER), None);
    }
}
