use super::draw::{FILL_COLOR, POWER_COLOR, POWER_SYMBOL_COLOR};
use super::layout::*;
use super::*;
use crate::frontend::draw::text_width;

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
    let text_end = layout.text_x + text_width("100%");
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
    let text_end = layout.text_x + text_width("100%");
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

fn click_at(dock: &mut Dock, point: (f32, f32)) {
    let down = Pointer {
        position: Some(point),
        left_down: true,
        ..Pointer::default()
    };
    dock.update(down, W, H);
    dock.update(hover(point.0, point.1), W, H);
}

fn centre_of(rect: crate::frontend::draw::Rect) -> (f32, f32) {
    (
        rect.x as f32 + rect.width as f32 / 2.0,
        rect.y as f32 + rect.height as f32 / 2.0,
    )
}

#[test]
fn the_cheats_button_is_the_rightmost_control_of_the_dock() {
    let layout = Layout::new(W, H);
    for other in layout.speed_buttons {
        assert!(other.x + other.width < layout.cheats_divider_x);
    }
    for other in [layout.power, layout.restart, layout.fullscreen, layout.icon] {
        assert!(other.x + other.width < layout.cheats.x);
    }
    assert!(layout.cheats_divider_x < layout.cheats.x);
    // It ends where the dock's padding begins, and the dock still fits.
    assert_eq!(
        layout.cheats.x + layout.cheats.width,
        layout.dock.x + layout.dock.width - DOCK_PADDING
    );
    assert!(layout.dock.x >= 0 && layout.dock.x + layout.dock.width <= W as i32);
    assert!(layout.cheats.width >= text_width("CHEATS"));
    // The power button is back at the left end.
    assert_eq!(layout.power.x, layout.dock.x + DOCK_PADDING);
}

#[test]
fn the_cheats_button_opens_and_closes_the_menu() {
    let layout = Layout::new(W, H);
    let button = centre_of(layout.cheats);
    let mut dock = Dock::new(0.5);
    settle(&mut dock, hover(button.0, button.1));
    assert!(!dock.menu_open());

    click_at(&mut dock, button);
    assert!(dock.menu_open());
    click_at(&mut dock, button);
    assert!(!dock.menu_open());
}

#[test]
fn an_open_menu_keeps_the_dock_up_and_a_click_elsewhere_dismisses_it() {
    let layout = Layout::new(W, H);
    let button = centre_of(layout.cheats);
    let mut dock = Dock::new(0.5);
    settle(&mut dock, hover(button.0, button.1));
    click_at(&mut dock, button);
    assert!(dock.menu_open());

    // Even with the mouse resting somewhere harmless for a long time.
    let still = hover(20.0, 20.0);
    for _ in 0..(HIDE_AFTER_IDLE_FRAMES * 2) {
        dock.update(still, W, H);
    }
    assert!(
        dock.reveal() > 0.9,
        "the dock stays up while the menu is open"
    );
    assert!(dock.menu_open());

    // A click in the game area closes it.
    click_at(&mut dock, (20.0, 20.0));
    assert!(!dock.menu_open());
}

#[test]
fn clicking_another_dock_button_closes_the_menu_and_still_works() {
    let layout = Layout::new(W, H);
    let mut dock = Dock::new(0.5);
    settle(
        &mut dock,
        hover(centre_of(layout.cheats).0, centre_of(layout.cheats).1),
    );
    click_at(&mut dock, centre_of(layout.cheats));
    assert!(dock.menu_open());
    click_at(&mut dock, centre_of(layout.fullscreen));
    assert!(!dock.menu_open());
    assert!(
        dock.take_fullscreen_request(),
        "the click was not swallowed"
    );
}

#[test]
fn menu_choices_are_reported_once_and_do_not_press_dock_buttons() {
    use cheat_menu::MenuLayout;
    let layout = Layout::new(W, H);
    let menu = MenuLayout::new(W, layout.dock, false);
    let mut dock = Dock::new(0.5);
    settle(
        &mut dock,
        hover(centre_of(layout.cheats).0, centre_of(layout.cheats).1),
    );
    click_at(&mut dock, centre_of(layout.cheats));
    assert_eq!(dock.take_cheat_options_change(), None);

    click_at(&mut dock, centre_of(menu.money));
    let changed = dock
        .take_cheat_options_change()
        .expect("a change is reported");
    assert!(changed.infinite_money);
    assert_eq!(dock.take_cheat_options_change(), None, "only once");
    assert!(dock.cheat_options().infinite_money);
    assert!(dock.menu_open(), "choosing an option leaves the menu open");

    // Nothing in the dock fired.
    assert!(!dock.take_quit_request());
    assert!(!dock.take_restart_request());
    assert!(!dock.take_fullscreen_request());
}

#[test]
fn restored_options_are_not_reported_as_a_change() {
    let mut dock = Dock::new(0.5);
    let options = gba_emulator::cheat_options::CheatOptions {
        wild_species: Some(147),
        wild_level: Some(20),
        ..Default::default()
    };
    dock.set_cheat_options(options);
    assert_eq!(dock.cheat_options(), options);
    assert_eq!(dock.take_cheat_options_change(), None);
}

#[test]
fn escape_closes_the_menu_through_close_menu() {
    let layout = Layout::new(W, H);
    let mut dock = Dock::new(0.5);
    settle(
        &mut dock,
        hover(centre_of(layout.cheats).0, centre_of(layout.cheats).1),
    );
    click_at(&mut dock, centre_of(layout.cheats));
    assert!(dock.menu_open());
    dock.close_menu();
    assert!(!dock.menu_open());
}

#[test]
fn the_dock_with_the_menu_and_list_open_draws_inside_the_window() {
    let mut dock = Dock::new(0.8);
    dock.open_everything_for_smoke_test();
    let mut buffer = vec![0u32; W * H];
    dock.draw(&mut buffer, W, H);
    assert!(buffer.iter().any(|p| *p != 0));
    // The menu's panel is above the dock and below the top of the window.
    let layout = Layout::new(W, H);
    let menu = cheat_menu::MenuLayout::new(W, layout.dock, false);
    let probe = (menu.panel.y + 4) as usize * W + (menu.panel.x + 40) as usize;
    assert_ne!(buffer[probe], 0, "the panel is painted");
}

#[test]
fn tab_opens_and_closes_the_menu_the_same_way_the_button_does() {
    // The frontend calls `toggle_menu` for Tab.
    let mut dock = Dock::new(0.5);
    assert!(!dock.menu_open());
    dock.toggle_menu();
    assert!(dock.menu_open());
    dock.toggle_menu();
    assert!(!dock.menu_open());
    // And it mixes freely with the button and Escape.
    let layout = Layout::new(W, H);
    dock.toggle_menu();
    settle(
        &mut dock,
        hover(centre_of(layout.cheats).0, centre_of(layout.cheats).1),
    );
    click_at(&mut dock, centre_of(layout.cheats));
    assert!(!dock.menu_open(), "the button closes a menu Tab opened");
    dock.toggle_menu();
    dock.close_menu();
    assert!(!dock.menu_open());
}

#[test]
fn the_menu_has_a_cheats_file_row_only_when_the_file_has_codes() {
    use cheat_menu::MenuLayout;
    let layout = Layout::new(W, H);
    let mut dock = Dock::new(0.5);
    dock.toggle_menu();
    settle(
        &mut dock,
        hover(centre_of(layout.cheats).0, centre_of(layout.cheats).1),
    );

    // No codes: the row is absent, so the topmost row is the wild one.
    let bare = MenuLayout::new(W, layout.dock, false);
    assert!(bare.file_codes.is_none());

    // With codes the row appears and its checkbox switches them on.
    dock.set_file_codes_available(4);
    let with = MenuLayout::new(W, layout.dock, true);
    click_at(&mut dock, centre_of(with.file_codes.unwrap()));
    assert!(dock.cheat_options().file_codes);
    let change = dock.take_cheat_options_change().expect("reported");
    assert!(change.file_codes);
    click_at(&mut dock, centre_of(with.file_codes.unwrap()));
    assert!(!dock.cheat_options().file_codes);
}
