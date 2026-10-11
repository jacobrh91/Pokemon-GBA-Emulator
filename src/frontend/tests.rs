use super::*;

#[test]
fn upscale_replicates_each_pixel_into_a_square_block() {
    let mut frame = vec![0u32; SCREEN_WIDTH * SCREEN_HEIGHT];
    frame[SCREEN_WIDTH + 2] = 0x0012_3456;
    let mut output = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];
    upscale(&frame, &mut output);
    for dy in 0..DISPLAY_SCALE {
        for dx in 0..DISPLAY_SCALE {
            let index = (DISPLAY_SCALE + dy) * WINDOW_WIDTH + 2 * DISPLAY_SCALE + dx;
            assert_eq!(output[index], 0x0012_3456);
        }
    }
    assert_eq!(
        output[(DISPLAY_SCALE) * WINDOW_WIDTH + 2 * DISPLAY_SCALE - 1],
        0
    );
}
