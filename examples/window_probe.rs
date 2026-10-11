//! Compares the native pointer lookup with minifb's across a full-screen round
//! trip. Opens a window briefly; move the mouse over it for meaningful numbers.
#[path = "../src/window_control.rs"]
mod window_control;

use minifb::{MouseMode, Scale, ScaleMode, Window, WindowOptions};
use std::time::{Duration, Instant};

fn main() {
    let (w, h) = (960usize, 640usize);
    let mut window = Window::new(
        "window_probe",
        w,
        h,
        WindowOptions {
            resize: true,
            scale: Scale::X1,
            scale_mode: ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )
    .unwrap();
    window.set_target_fps(60);
    let buffer = vec![0x0020_4060u32; w * h];
    window_control::lock_aspect_ratio(&window, w, h, 480, 320);
    window_control::enable_fullscreen(&window);
    let pump = |window: &mut Window, ms: u64| {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until && window.is_open() {
            window.update_with_buffer(&buffer, w, h).unwrap();
        }
    };
    let report = |window: &Window, label: &str| {
        println!(
            "{label:26} view={:?} native_pointer={:?} minifb_pass={:?}",
            window_control::content_size(window),
            window_control::pointer_in_picture_view(window).map(|(p, _)| p),
            window.get_mouse_pos(MouseMode::Pass),
        );
    };
    pump(&mut window, 400);
    report(&window, "windowed");
    window_control::toggle_fullscreen(&window);
    pump(&mut window, 2200);
    report(&window, "fullscreen");
    window_control::toggle_fullscreen(&window);
    pump(&mut window, 2200);
    report(&window, "windowed after fullscreen");
}
