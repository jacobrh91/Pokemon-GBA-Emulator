//! Draws the volume dock over a PPM screenshot and writes the result as PPM.
//! usage: dock_preview IN_240x160.ppm OUT.ppm VOLUME [muted]
#[path = "../src/volume_dock.rs"]
mod volume_dock;

use volume_dock::{Pointer, VolumeDock};

fn main() {
    let mut args = std::env::args().skip(1);
    let input = std::fs::read(args.next().unwrap()).unwrap();
    let output = args.next().unwrap();
    let volume: f32 = args.next().unwrap().parse().unwrap();
    let muted = args.next().is_some();

    let pixels = input.splitn(4, |b| *b == b'\n').nth(3).unwrap();
    const SCALE: usize = 4;
    let (w, h) = (240 * SCALE, 160 * SCALE);
    let mut buffer = vec![0u32; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = ((y / SCALE) * 240 + x / SCALE) * 3;
            buffer[y * w + x] = (u32::from(pixels[i]) << 16)
                | (u32::from(pixels[i + 1]) << 8)
                | u32::from(pixels[i + 2]);
        }
    }

    let mut dock = VolumeDock::new(volume);
    let hover = Pointer {
        position: Some((w as f32 / 2.0, h as f32 - 20.0)),
        left_down: false,
        scroll: 0.0,
    };
    for _ in 0..30 {
        dock.update(hover, w, h);
    }
    if muted {
        // Click the speaker icon once.
        let click = Pointer {
            position: Some((w as f32 / 2.0 - 200.0 + 6.0, h as f32 - 18.0 - 30.0)),
            left_down: true,
            scroll: 0.0,
        };
        dock.update(click, w, h);
    }
    dock.draw(&mut buffer, w, h);

    let mut data = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in buffer {
        data.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    std::fs::write(output, data).unwrap();
}
