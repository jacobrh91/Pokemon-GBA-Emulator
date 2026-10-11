//! macOS implementation: messages the `NSWindow` behind the minifb window
//! through the Objective-C runtime.

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
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    #[cfg(not(target_arch = "aarch64"))]
    fn objc_msgSend_stret();
}

fn selector(name: &CStr) -> *mut c_void {
    // Safety: `name` is a valid NUL-terminated string.
    unsafe { sel_registerName(name.as_ptr()) }
}

/// The window's content area (below the title bar) from `contentLayoutRect`.
pub fn content_size(window: &Window) -> Option<(f32, f32)> {
    let object = ns_window(window);
    if object.is_null() {
        return None;
    }
    // Safety: `contentLayoutRect` takes no arguments and returns an NSRect;
    // `object` is minifb's live NSWindow, used from the main thread.
    let rect = unsafe { send_rect(object, c"contentLayoutRect") };
    (rect.width > 0.0 && rect.height > 0.0).then_some((rect.width as f32, rect.height as f32))
}

pub fn left_button_down() -> bool {
    // Safety: `+[NSEvent pressedMouseButtons]` takes no arguments and
    // returns an NSUInteger bit mask (bit 0 is the left button).
    unsafe {
        let class = objc_getClass(c"NSEvent".as_ptr());
        if class.is_null() {
            return false;
        }
        let call: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        call(class, selector(c"pressedMouseButtons")) & 1 != 0
    }
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
