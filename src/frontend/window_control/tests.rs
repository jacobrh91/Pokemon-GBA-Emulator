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
