use super::draw::{FILL_COLOR, POWER_COLOR, POWER_SYMBOL_COLOR};
use super::layout::*;
use super::*;
use crate::frontend::draw::{GLYPH_WIDTH, TEXT_SCALE};

const W: usize = 960;
const H: usize = 640;

fn hover(x: f32, y: f32) -> Pointer {
    Pointer {
        position: Some((x, y)),
        ..Pointer::default()
    }
}

fn settle(dock: &mut Dock, pointer: Pointer) {
    for _ in 0..20 {
        dock.update(pointer, W, H);
    }
}

#[test]
fn dock_appears_when_the_mouse_moves_anywhere_and_hides_when_it_rests() {
    let mut dock = Dock::new(0.5);
    // Never seen the mouse: hidden.
    settle(&mut dock, Pointer::default());
    assert_eq!(dock.reveal(), 0.0);

    // Moving over the middle of the screen shows it.
    for step in 0..20 {
        dock.update(hover(300.0 + step as f32, 200.0), W, H);
    }
    assert_eq!(dock.reveal(), 1.0);

    // A mouse that has stopped moving hides it after the idle delay.
    for _ in 0..(HIDE_AFTER_IDLE_FRAMES + 30) {
        dock.update(hover(320.0, 200.0), W, H);
    }
    assert_eq!(dock.reveal(), 0.0);

    // Moving again brings it straight back.
    dock.update(hover(330.0, 210.0), W, H);
    for _ in 0..10 {
        dock.update(hover(330.0, 210.0), W, H);
    }
    assert!(dock.reveal() > 0.9);
}

#[test]
fn a_continuously_moving_mouse_keeps_the_dock_open() {
    let mut dock = Dock::new(0.5);
    for frame in 0..(HIDE_AFTER_IDLE_FRAMES * 3) {
        dock.update(hover(100.0 + (frame % 200) as f32, 150.0), W, H);
    }
    assert_eq!(dock.reveal(), 1.0);
}

#[test]
fn resting_near_the_bottom_or_dragging_keeps_the_dock_open_and_leaving_hides_it() {
    let mut dock = Dock::new(0.5);
    // Stationary inside the bottom strip: stays.
    for _ in 0..(HIDE_AFTER_IDLE_FRAMES * 2) {
        dock.update(hover(480.0, H as f32 - 20.0), W, H);
    }
    assert_eq!(dock.reveal(), 1.0);

    // Leaving the window hides it immediately (animation aside).
    settle(&mut dock, Pointer::default());
    assert_eq!(dock.reveal(), 0.0);
}

#[test]
fn restart_button_requests_a_restart_once_per_click() {
    let mut dock = Dock::new(0.5);
    settle(&mut dock, hover(480.0, H as f32 - 30.0));
    let layout = Layout::new(W, H);
    let (x, y) = (
        layout.restart.x as f32 + 10.0,
        layout.restart.y as f32 + 10.0,
    );
    assert!(!dock.take_restart_request());

    let press = Pointer {
        position: Some((x, y)),
        left_down: true,
        scroll: 0.0,
    };
    dock.update(press, W, H);
    assert!(dock.take_restart_request());
    // Holding the button does not fire again; neither does the release.
    dock.update(press, W, H);
    assert!(!dock.take_restart_request());
    dock.update(hover(x, y), W, H);
    assert!(!dock.take_restart_request());
    // A second click does.
    dock.update(press, W, H);
    assert!(dock.take_restart_request());
}

#[test]
fn controls_are_ordered_power_restart_volume_speed_inside_the_dock() {
    let layout = Layout::new(W, H);
    let dock_left = layout.dock.x;
    let dock_right = layout.dock.x + layout.dock.width;
    assert!(layout.power.x >= dock_left);
    assert!(layout.restart.x > layout.power.x + layout.power.width);
    assert!(layout.fullscreen.x > layout.restart.x + layout.restart.width);
    assert!(layout.restart_divider_x > layout.fullscreen.x + layout.fullscreen.width);
    assert!(layout.icon.x > layout.restart_divider_x);
    // Volume text, then the speed divider, then the four speed buttons.
    let text_end = layout.text_x + 4 * (GLYPH_WIDTH + 1) * TEXT_SCALE;
    assert!(text_end < layout.divider_x);
    let first = layout.speed_buttons[0];
    let last = layout.speed_buttons[3];
    assert!(first.x > layout.divider_x);
    assert!(last.x + last.width < dock_right);
    assert!(dock_left >= 0 && dock_right <= W as i32);
}

#[test]
fn power_button_requests_quit_once_per_click_and_is_red() {
    let mut dock = Dock::new(0.5);
    settle(&mut dock, hover(480.0, H as f32 - 30.0));
    let layout = Layout::new(W, H);
    let (x, y) = (layout.power.x as f32 + 10.0, layout.power.y as f32 + 10.0);
    assert!(!dock.take_quit_request());

    let press = Pointer {
        position: Some((x, y)),
        left_down: true,
        scroll: 0.0,
    };
    dock.update(press, W, H);
    assert!(dock.take_quit_request());
    dock.update(press, W, H);
    assert!(!dock.take_quit_request(), "holding does not repeat");
    // The power click is not a restart click.
    assert!(!dock.take_restart_request());

    let mut buffer = vec![0u32; W * H];
    dock.draw(&mut buffer, W, H);
    let probe = (layout.power.y as usize + 6) * W + layout.power.x as usize + 6;
    assert_eq!(buffer[probe], POWER_COLOR, "the button face is red");
    // The white symbol is drawn on it.
    let has_symbol = (0..layout.power.height as usize).any(|row| {
        (0..layout.power.width as usize).any(|column| {
            buffer[(layout.power.y as usize + row) * W + layout.power.x as usize + column]
                == POWER_SYMBOL_COLOR
        })
    });
    assert!(has_symbol);
}

#[test]
fn reveal_animates_over_several_frames() {
    let mut dock = Dock::new(0.5);
    dock.update(hover(480.0, H as f32 - 5.0), W, H);
    assert!(dock.reveal() > 0.0 && dock.reveal() < 1.0);
}

#[test]
fn clicking_and_dragging_the_track_sets_the_volume() {
    let mut dock = Dock::new(0.5);
    settle(&mut dock, hover(480.0, H as f32 - 30.0));
    let layout = Layout::new(W, H);
    let y = layout.track.y as f32 + 4.0;
    let at = |fraction: f32| layout.track.x as f32 + layout.track.width as f32 * fraction;

    let press = |x: f32| Pointer {
        position: Some((x, y)),
        left_down: true,
        scroll: 0.0,
    };
    dock.update(press(at(0.25)), W, H);
    assert!((dock.volume() - 0.25).abs() < 0.01);
    dock.update(press(at(0.8)), W, H);
    assert!((dock.volume() - 0.8).abs() < 0.01);

    // Dragging past the end, and well above the dock, clamps and keeps going.
    dock.update(
        Pointer {
            position: Some((at(1.5), 100.0)),
            left_down: true,
            scroll: 0.0,
        },
        W,
        H,
    );
    assert_eq!(dock.volume(), 1.0);
    assert_eq!(dock.reveal(), 1.0, "dock stays open while dragging");

    // Releasing ends the drag.
    dock.update(hover(at(0.1), y), W, H);
    assert_eq!(dock.volume(), 1.0);
}

#[test]
fn clicking_the_icon_toggles_mute_and_gain() {
    let mut dock = Dock::new(0.5);
    settle(&mut dock, hover(480.0, H as f32 - 30.0));
    let icon = Layout::new(W, H).icon;
    let (x, y) = (icon.x as f32 + 5.0, icon.y as f32 + 5.0);
    let press = Pointer {
        position: Some((x, y)),
        left_down: true,
        scroll: 0.0,
    };

    dock.update(press, W, H);
    assert!(dock.is_muted());
    assert_eq!(dock.gain(), 0.0);
    // Holding the button does not toggle again.
    dock.update(press, W, H);
    assert!(dock.is_muted());
    dock.update(hover(x, y), W, H);
    dock.update(press, W, H);
    assert!(!dock.is_muted());
    assert!((dock.gain() - 0.25).abs() < 1e-6);
}

#[test]
fn wheel_over_the_dock_adjusts_volume_and_unmutes() {
    let mut dock = Dock::new(0.5);
    settle(&mut dock, hover(480.0, H as f32 - 30.0));
    let layout = Layout::new(W, H);
    let over = (layout.dock.x as f32 + 200.0, layout.dock.y as f32 + 30.0);
    dock.update(
        Pointer {
            position: Some(over),
            left_down: false,
            scroll: 1.0,
        },
        W,
        H,
    );
    assert!((dock.volume() - 0.55).abs() < 1e-6);
}

#[test]
fn input_is_ignored_while_the_dock_is_mostly_hidden() {
    let mut dock = Dock::new(0.5);
    let layout = Layout::new(W, H);
    let icon = (layout.icon.x as f32 + 5.0, layout.icon.y as f32 + 5.0);
    // First frame of hover: reveal is ~0.18, below the interaction threshold.
    dock.update(
        Pointer {
            position: Some(icon),
            left_down: true,
            scroll: 0.0,
        },
        W,
        H,
    );
    assert!(!dock.is_muted());
}

#[test]
fn draw_leaves_the_frame_untouched_when_hidden_and_paints_when_shown() {
    let mut dock = Dock::new(0.5);
    let mut buffer = vec![0x0010_2030u32; W * H];
    dock.draw(&mut buffer, W, H);
    assert!(buffer.iter().all(|&p| p == 0x0010_2030));

    settle(&mut dock, hover(480.0, H as f32 - 30.0));
    dock.draw(&mut buffer, W, H);
    assert!(buffer.iter().any(|&p| p != 0x0010_2030));
    // Pixels well above the dock are unaffected.
    assert_eq!(buffer[50 * W + 50], 0x0010_2030);
}

#[test]
fn speed_defaults_to_max_and_clicking_a_button_selects_it() {
    let mut dock = Dock::new(0.5);
    assert_eq!(dock.speed(), SpeedSetting::Max);
    assert_eq!(dock.speed().multiple(), None);
    settle(&mut dock, hover(480.0, H as f32 - 30.0));

    let layout = Layout::new(W, H);
    for (index, setting) in SpeedSetting::ALL.into_iter().enumerate() {
        let button = layout.speed_buttons[index];
        let click = Pointer {
            position: Some((button.x as f32 + 10.0, button.y as f32 + 10.0)),
            left_down: true,
            scroll: 0.0,
        };
        dock.update(click, W, H);
        assert_eq!(dock.speed(), setting);
        // Release before the next click.
        dock.update(hover(button.x as f32 + 10.0, button.y as f32 + 10.0), W, H);
    }
    assert_eq!(SpeedSetting::X2.multiple(), Some(2));
    assert_eq!(SpeedSetting::X4.multiple(), Some(4));
    assert_eq!(SpeedSetting::X8.multiple(), Some(8));
}

#[test]
fn speed_buttons_fit_inside_the_dock_without_overlapping_the_volume_controls() {
    let layout = Layout::new(W, H);
    let dock_right = layout.dock.x + layout.dock.width;
    let mut previous_right = layout.divider_x;
    for button in layout.speed_buttons {
        assert!(
            button.x > previous_right,
            "buttons are ordered and separated"
        );
        previous_right = button.x + button.width;
    }
    assert!(previous_right < dock_right);
    // The volume percentage text ends before the divider.
    let text_end = layout.text_x + 4 * (GLYPH_WIDTH + 1) * TEXT_SCALE;
    assert!(text_end < layout.divider_x);
    assert!(layout.dock.x >= 0 && dock_right <= W as i32);
}

#[test]
fn selected_speed_is_highlighted_and_changing_volume_leaves_it_alone() {
    let mut dock = Dock::new(0.5);
    settle(&mut dock, hover(480.0, H as f32 - 30.0));
    let layout = Layout::new(W, H);
    let button = layout.speed_buttons[1]; // 4X
    dock.update(
        Pointer {
            position: Some((button.x as f32 + 5.0, button.y as f32 + 5.0)),
            left_down: true,
            scroll: 0.0,
        },
        W,
        H,
    );
    assert_eq!(dock.speed(), SpeedSetting::X4);

    let mut buffer = vec![0u32; W * H];
    dock.draw(&mut buffer, W, H);
    let corner = |index: usize| {
        let b = layout.speed_buttons[index];
        buffer[(b.y as usize + 2) * W + b.x as usize + b.width as usize / 2]
    };
    assert_eq!(corner(1), FILL_COLOR, "selected button uses the accent");
    assert_ne!(corner(0), FILL_COLOR);
    assert_ne!(corner(3), FILL_COLOR);

    // Scrolling the wheel changes volume only.
    dock.update(
        Pointer {
            position: Some((layout.dock.x as f32 + 100.0, layout.dock.y as f32 + 30.0)),
            left_down: false,
            scroll: 1.0,
        },
        W,
        H,
    );
    assert_eq!(dock.speed(), SpeedSetting::X4);
}

#[test]
fn fullscreen_button_requests_a_toggle_once_per_click() {
    let mut dock = Dock::new(0.5);
    settle(&mut dock, hover(480.0, H as f32 - 30.0));
    let layout = Layout::new(W, H);
    let (x, y) = (
        layout.fullscreen.x as f32 + 10.0,
        layout.fullscreen.y as f32 + 10.0,
    );
    assert!(!dock.take_fullscreen_request());

    let press = Pointer {
        position: Some((x, y)),
        left_down: true,
        scroll: 0.0,
    };
    dock.update(press, W, H);
    assert!(dock.take_fullscreen_request());
    dock.update(press, W, H);
    assert!(!dock.take_fullscreen_request(), "holding does not repeat");
    // It is neither a restart nor a quit.
    assert!(!dock.take_restart_request());
    assert!(!dock.take_quit_request());
}
