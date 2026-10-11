//! Logs per-frame timing and view geometry right after toggling full screen.
#[path = "../src/window_control.rs"]
mod window_control;

use minifb::{Scale, ScaleMode, Window, WindowOptions};
use std::time::Instant;

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
    for _ in 0..30 {
        window.update_with_buffer(&buffer, w, h).unwrap();
    }
    let start = Instant::now();
    window_control::toggle_fullscreen(&window);
    let mut last = Instant::now();
    let mut previous = String::new();
    while start.elapsed().as_secs_f32() < 3.5 {
        let t = Instant::now();
        window.update_with_buffer(&buffer, w, h).unwrap();
        let frame_ms = t.elapsed().as_secs_f32() * 1000.0;
        let state = format!(
            "fs={} view={:?} active={}",
            window_control::is_fullscreen(&window),
            window_control::content_size(&window),
            window.is_active()
        );
        if state != previous || frame_ms > 40.0 || last.elapsed().as_millis() > 100 {
            println!(
                "t={:5.0}ms frame={:6.1}ms {}",
                start.elapsed().as_secs_f32() * 1000.0,
                frame_ms,
                state
            );
            previous = state;
        }
        last = Instant::now();
    }
}
