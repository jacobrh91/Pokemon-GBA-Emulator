//! Draws the dock at every stage of its slide animation and with every
//! control state, to shake out out-of-bounds drawing.
#[path = "../src/draw.rs"]
mod draw;
#[path = "../src/hud.rs"]
mod hud;
#[path = "../src/volume_dock.rs"]
mod volume_dock;

use volume_dock::{Pointer, VolumeDock};

fn main() {
    let (w, h) = (960usize, 640usize);
    let mut buffer = vec![0u32; w * h];
    let mut draws = 0u64;
    for volume in [0.0f32, 0.3, 0.7, 1.0] {
        let mut dock = VolumeDock::new(volume);
        // The mouse first appears mid-window, then moves around and rests.
        let path = [
            (480.0, 300.0),
            (500.0, 320.0),
            (480.0, 600.0),
            (10.0, 630.0),
            (950.0, 5.0),
        ];
        for (px, py) in path {
            for _ in 0..40 {
                dock.update(
                    Pointer {
                        position: Some((px, py)),
                        left_down: false,
                        scroll: 0.0,
                    },
                    w,
                    h,
                );
                dock.draw(&mut buffer, w, h);
                hud::draw_fast_forward(&mut buffer, w, h, 4.2);
                draws += 1;
            }
        }
    }
    println!(
        "drew {draws} dock frames without panicking; checksum {}",
        buffer.iter().fold(0u32, |a, b| a.wrapping_add(*b))
    );
}
