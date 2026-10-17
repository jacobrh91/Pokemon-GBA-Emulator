use super::*;
use crate::frontend::draw::Canvas;
use gba_emulator::cheats::options::{CheatOptions, ShinyOdds};
use std::time::{Duration, Instant};

const WIDTH: usize = 960;
const HEIGHT: usize = 640;

fn dock() -> Rect {
    Rect {
        x: 80,
        y: 562,
        width: 802,
        height: 60,
    }
}

fn layout() -> MenuLayout {
    MenuLayout::new(WIDTH, dock(), false)
}

/// The layout when the cheats file has codes (one extra row on top).
fn layout_with_file_row() -> MenuLayout {
    MenuLayout::new(WIDTH, dock(), true)
}

fn centre(rect: Rect) -> (f32, f32) {
    (
        rect.x as f32 + rect.width as f32 / 2.0,
        rect.y as f32 + rect.height as f32 / 2.0,
    )
}

fn at(point: (f32, f32), down: bool) -> Pointer {
    Pointer {
        position: Some(point),
        left_down: down,
        scroll: 0.0,
    }
}

fn open_menu() -> CheatMenu {
    let mut menu = CheatMenu::new();
    menu.open();
    menu
}

/// One full click: the frame the button goes down, then its release.
fn click(menu: &mut CheatMenu, layout: &MenuLayout, point: (f32, f32)) -> bool {
    let used = menu.update(at(point, true), true, layout);
    menu.update(at(point, false), false, layout);
    used
}

#[test]
fn the_panel_fits_the_window_and_holds_every_control() {
    let layout = layout();
    let panel = layout.panel;
    assert!(panel.x >= 0 && panel.y >= 0);
    assert!(panel.x + panel.width <= WIDTH as i32);
    assert!(
        panel.y + panel.height <= dock().y,
        "the panel sits above the dock"
    );
    for rect in layout.always_on.into_iter().chain([
        layout.no_encounters,
        layout.fast_battles,
        layout.shiny,
        layout.wild_enable,
        layout.perfect_enable,
        layout.dropdown,
        layout.level_minus,
        layout.level_value,
        layout.level_plus,
        layout.level_clear,
        layout.perfect_dropdown,
        layout.money,
        layout.catch,
        layout.exp_boost,
    ]) {
        assert!(
            rect.x >= panel.x && rect.x + rect.width <= panel.x + panel.width,
            "{rect:?}"
        );
        assert!(
            rect.y >= panel.y && rect.y + rect.height <= panel.y + panel.height,
            "{rect:?}"
        );
    }
    // The species list opens inside the panel too.
    assert!(layout.list.y + layout.list.height <= panel.y + panel.height);
}

#[test]
fn the_panel_stays_on_screen_for_a_dock_pushed_to_the_edge() {
    let far_right = Rect { x: 900, ..dock() };
    let layout = MenuLayout::new(WIDTH, far_right, true);
    assert!(layout.panel.x + layout.panel.width <= WIDTH as i32);
    assert!(layout.panel.x >= 0);
}

#[test]
fn every_rows_own_checkbox_toggles_just_that_cheat_and_reports_one_change() {
    let layout = layout();
    let mut menu = open_menu();
    assert!(!menu.take_changed());

    type Pick = fn(&CheatOptions) -> bool;
    let cases: [(Rect, Pick); 8] = [
        (layout.wild_enable, |o| o.wild_enabled),
        (layout.perfect_enable, |o| o.perfect_enabled),
        (layout.money, |o| o.infinite_money),
        (layout.catch, |o| o.catch_always),
        (layout.exp_boost, |o| o.exp_boost),
        (layout.shiny, |o| o.shiny),
        (layout.no_encounters, |o| o.no_encounters),
        (layout.fast_battles, |o| o.fast_battles),
    ];
    for (index, (rect, pick)) in cases.iter().enumerate() {
        assert!(
            click(&mut menu, &layout, centre(*rect)),
            "the click is used"
        );
        assert!(pick(&menu.options()), "case {index} turned on");
        assert!(menu.take_changed());
        assert!(!menu.take_changed(), "reported once");
        // Exactly the cheats clicked so far are on.
        let on = cases.iter().filter(|(_, p)| p(&menu.options())).count();
        assert_eq!(on, index + 1);
    }
    // Each turns off again, on its own.
    for (rect, pick) in &cases {
        click(&mut menu, &layout, centre(*rect));
        assert!(!pick(&menu.options()));
    }
    assert!(menu.options().is_default());
}

#[test]
fn every_row_has_its_checkbox_in_one_column_at_the_left() {
    let layout = layout_with_file_row();
    let column: Vec<Rect> = [
        layout.file_codes.unwrap(),
        layout.wild_enable,
        layout.perfect_enable,
        layout.money,
        layout.catch,
        layout.exp_boost,
        layout.shiny,
        layout.no_encounters,
        layout.fast_battles,
    ]
    .into_iter()
    .chain(layout.always_on)
    .collect();
    for rect in &column {
        assert_eq!(rect.x, column[0].x, "one column");
    }
    // The checkboxes are left of everything else in their rows.
    assert!(layout.wild_enable.x + layout.wild_enable.width < layout.wild_label.0);
    assert!(layout.perfect_enable.x + layout.perfect_enable.width < layout.perfect_label.0);
    assert!(layout.wild_enable.x + layout.wild_enable.width < layout.dropdown.x);
    assert!(layout.perfect_enable.x + layout.perfect_enable.width < layout.perfect_dropdown.x);
    // Rows run top to bottom in the documented order.
    let ys: Vec<i32> = column.iter().map(|r| r.y).collect();
    assert!(ys.windows(2).all(|pair| pair[0] < pair[1]), "{ys:?}");
}

#[test]
fn choosing_in_a_row_switches_it_on_and_unticking_keeps_the_choices() {
    let layout = layout();
    let mut menu = open_menu();

    // A species chosen from the list switches the wild row on.
    click(&mut menu, &layout, centre(layout.dropdown));
    menu.list_scroll = 147 - 3;
    click(&mut menu, &layout, centre(layout.list_row(3)));
    assert_eq!(menu.options().wild_species, Some(147));
    assert!(menu.options().wild_enabled, "choosing switched the row on");

    // Unticking the row keeps the species...
    click(&mut menu, &layout, centre(layout.wild_enable));
    assert!(!menu.options().wild_enabled);
    assert_eq!(menu.options().wild_species, Some(147));
    assert_eq!(
        menu.options().active_wild_species(),
        None,
        "...but it is off"
    );
    // ...and ticking brings it back without choosing again.
    click(&mut menu, &layout, centre(layout.wild_enable));
    assert_eq!(menu.options().active_wild_species(), Some(147));

    // A level does the same.
    click(&mut menu, &layout, centre(layout.wild_enable));
    assert!(!menu.options().wild_enabled);
    click(&mut menu, &layout, centre(layout.level_plus));
    assert_eq!(menu.options().wild_level, Some(1));
    assert!(menu.options().wild_enabled);
    // Clearing a choice does not switch anything.
    click(&mut menu, &layout, centre(layout.wild_enable));
    click(&mut menu, &layout, centre(layout.level_clear));
    assert!(!menu.options().wild_enabled);

    // Choosing a perfect-stats target switches that row on.
    click(&mut menu, &layout, centre(layout.perfect_dropdown));
    assert!(menu.perfect_list_open());
    click(&mut menu, &layout, centre(layout.perfect_row(1)));
    assert_eq!(menu.options().perfect_target, PerfectTarget::Opponents);
    assert!(menu.options().perfect_enabled);
    assert!(!menu.perfect_list_open(), "choosing closes the list");
    // Unticking the row keeps the target.
    click(&mut menu, &layout, centre(layout.perfect_enable));
    assert!(!menu.options().perfect_enabled);
    assert_eq!(menu.options().perfect_target, PerfectTarget::Opponents);
    // Choosing the same target again changes nothing, so it does not tick
    // the row back on.
    click(&mut menu, &layout, centre(layout.perfect_dropdown));
    click(&mut menu, &layout, centre(layout.perfect_row(1)));
    assert!(!menu.options().perfect_enabled);
    // A different choice does.
    click(&mut menu, &layout, centre(layout.perfect_dropdown));
    click(&mut menu, &layout, centre(layout.perfect_row(2)));
    assert_eq!(menu.options().perfect_target, PerfectTarget::Both);
    assert!(menu.options().perfect_enabled);
}

#[test]
fn the_perfect_stats_dropdown_lists_wild_opponents_and_both() {
    let layout = layout();
    let mut menu = open_menu();
    assert_eq!(
        menu.options().perfect_target,
        PerfectTarget::Wild,
        "the first"
    );
    click(&mut menu, &layout, centre(layout.perfect_dropdown));
    // The list opens just below the dropdown, three rows, inside the panel.
    assert!(layout.perfect_list.y >= layout.perfect_dropdown.y + layout.perfect_dropdown.height);
    assert_eq!(layout.perfect_list.height, 3 * LIST_ROW_HEIGHT);
    assert!(
        layout.perfect_list.y + layout.perfect_list.height <= layout.panel.y + layout.panel.height
    );
    for (index, expected) in [
        PerfectTarget::Wild,
        PerfectTarget::Opponents,
        PerfectTarget::Both,
    ]
    .into_iter()
    .enumerate()
    {
        let mut menu = open_menu();
        click(&mut menu, &layout, centre(layout.perfect_dropdown));
        click(&mut menu, &layout, centre(layout.perfect_row(index)));
        assert_eq!(menu.options().perfect_target, expected);
    }
    // A click outside the list only dismisses it.
    let used = click(
        &mut menu,
        &layout,
        (layout.panel.x as f32 + 10.0, layout.panel.y as f32 + 10.0),
    );
    assert!(used);
    assert!(!menu.perfect_list_open());
    assert!(menu.is_open());
    assert_eq!(menu.options().perfect_target, PerfectTarget::Wild);
}

#[test]
fn only_one_of_the_two_lists_is_open_at_a_time() {
    let layout = layout();
    let mut menu = open_menu();
    click(&mut menu, &layout, centre(layout.dropdown));
    assert!(menu.dropdown_open() && !menu.perfect_list_open());
    // A click outside the species list dismisses it and nothing else: the
    // checkbox it landed on is not toggled.
    click(&mut menu, &layout, centre(layout.money));
    assert!(!menu.dropdown_open() && !menu.perfect_list_open());
    assert!(!menu.options().infinite_money);

    click(&mut menu, &layout, centre(layout.perfect_dropdown));
    assert!(menu.perfect_list_open() && !menu.dropdown_open());
    // Likewise the other way round: the click only closes the open list.
    click(&mut menu, &layout, centre(layout.dropdown));
    assert!(!menu.perfect_list_open() && !menu.dropdown_open());
    // And a second click then opens the species list.
    click(&mut menu, &layout, centre(layout.dropdown));
    assert!(menu.dropdown_open());
}

#[test]
fn the_open_species_list_sits_over_the_perfect_stats_dropdown() {
    // The list opens downward over the row below, so a click there chooses a
    // species rather than reaching the dropdown underneath.
    let layout = layout();
    // A point of the dropdown that lies under the list's left part (its right
    // edge is the scrollbar), low enough to be over a species, not over ANY.
    let covered = (
        layout.perfect_dropdown.x as f32 + 10.0,
        (layout.perfect_dropdown.y + layout.perfect_dropdown.height - 3) as f32,
    );
    assert!(layout.perfect_dropdown.contains(covered.0, covered.1));
    assert!(layout.list.contains(covered.0, covered.1));
    let mut menu = open_menu();
    click(&mut menu, &layout, centre(layout.dropdown));
    click(&mut menu, &layout, covered);
    assert!(
        menu.options().wild_species.is_some(),
        "a species was chosen"
    );
    assert!(!menu.perfect_list_open());
    assert_eq!(menu.options().perfect_target, PerfectTarget::Wild);
}

#[test]
fn the_cheats_file_row_exists_only_when_the_file_has_codes() {
    assert!(layout().file_codes.is_none());
    let with = layout_with_file_row();
    assert!(with.file_codes.is_some());
    // The panel grows upward from the dock by one row, so the rows that were
    // already there keep their place on screen; within the panel they sit one
    // row further down.
    let without = layout();
    assert_eq!(with.wild_enable.y, without.wild_enable.y);
    assert_eq!(with.panel.height - without.panel.height, 44);
    assert_eq!(without.panel.y - with.panel.y, 44);
    assert_eq!(
        with.wild_enable.y - with.panel.y,
        without.wild_enable.y - without.panel.y + 44
    );
    // The file's row is the new first row, above the wild row.
    assert!(with.file_codes.unwrap().y < with.wild_enable.y);
    assert!(with.panel.y >= 0);
    assert!(with.panel.y + with.panel.height <= dock().y);

    let mut menu = open_menu();
    menu.set_file_codes_available(3);
    assert!(menu.has_file_row());
    click(&mut menu, &with, centre(with.file_codes.unwrap()));
    assert!(menu.options().file_codes);
    click(&mut menu, &with, centre(with.file_codes.unwrap()));
    assert!(!menu.options().file_codes);
    // The file's switch is a session setting and says nothing about the rows.
    assert!(!menu.options().wild_enabled && !menu.options().perfect_enabled);
}

#[test]
fn a_closed_menu_ignores_the_pointer() {
    let layout = layout();
    let mut menu = CheatMenu::new();
    assert!(!click(&mut menu, &layout, centre(layout.shiny)));
    assert!(menu.options().is_default());
}

#[test]
fn the_level_steps_up_from_empty_down_to_empty_and_clamps() {
    let layout = layout();
    let mut menu = open_menu();
    assert_eq!(menu.options().wild_level, None, "empty to begin with");

    // `-` on empty does nothing.
    click(&mut menu, &layout, centre(layout.level_minus));
    assert_eq!(menu.options().wild_level, None);

    click(&mut menu, &layout, centre(layout.level_plus));
    assert_eq!(menu.options().wild_level, Some(1), "empty steps up to 1");
    click(&mut menu, &layout, centre(layout.level_plus));
    assert_eq!(menu.options().wild_level, Some(2));
    click(&mut menu, &layout, centre(layout.level_minus));
    click(&mut menu, &layout, centre(layout.level_minus));
    assert_eq!(
        menu.options().wild_level,
        None,
        "stepping below 1 empties it"
    );

    // Clear empties from any value.
    menu.set_options(CheatOptions {
        wild_level: Some(37),
        ..CheatOptions::default()
    });
    click(&mut menu, &layout, centre(layout.level_clear));
    assert_eq!(menu.options().wild_level, None);

    // The top of the range holds.
    menu.set_options(CheatOptions {
        wild_level: Some(100),
        ..CheatOptions::default()
    });
    click(&mut menu, &layout, centre(layout.level_plus));
    assert_eq!(menu.options().wild_level, Some(100));
}

#[test]
fn holding_a_stepper_button_repeats_after_a_delay() {
    let layout = layout();
    let mut menu = open_menu();
    let plus = centre(layout.level_plus);
    let start = Instant::now();
    let ms = |n: u64| start + Duration::from_millis(n);
    // Press: one step.
    menu.update_at(at(plus, true), true, &layout, ms(0));
    assert_eq!(menu.options().wild_level, Some(1));
    // Held for less than the delay (at 60 frames a second): nothing more.
    let mut t = 16;
    while t < 400 {
        menu.update_at(at(plus, true), false, &layout, ms(t));
        t += 16;
    }
    assert_eq!(menu.options().wild_level, Some(1));
    // Past the delay it repeats, whatever the frame rate: one second of
    // holding goes past ten steps at 60 frames a second and at 20.
    for frame_ms in [16, 50] {
        let mut menu = open_menu();
        menu.update_at(at(plus, true), true, &layout, ms(0));
        let mut t = frame_ms;
        while t <= 1400 {
            menu.update_at(at(plus, true), false, &layout, ms(t));
            t += frame_ms;
        }
        let level = menu.options().wild_level.unwrap();
        assert!(level >= 10, "{frame_ms} ms frames: reached {level}");
    }
    // Releasing stops it.
    menu.update_at(at(plus, false), false, &layout, ms(1500));
    let after = menu.options().wild_level;
    for step in 1..30 {
        menu.update_at(at(plus, false), false, &layout, ms(1500 + step * 16));
    }
    assert_eq!(menu.options().wild_level, after);
}

#[test]
fn the_minus_button_repeats_down_to_empty() {
    let layout = layout();
    let mut menu = open_menu();
    menu.set_options(CheatOptions {
        wild_level: Some(5),
        ..Default::default()
    });
    let minus = centre(layout.level_minus);
    let start = Instant::now();
    menu.update_at(at(minus, true), true, &layout, start);
    assert_eq!(menu.options().wild_level, Some(4));
    for step in 1..=120 {
        menu.update_at(
            at(minus, true),
            false,
            &layout,
            start + Duration::from_millis(step * 16),
        );
    }
    assert_eq!(menu.options().wild_level, None);
}

#[test]
fn the_wheel_over_the_level_steps_it() {
    let layout = layout();
    let mut menu = open_menu();
    let over = centre(layout.level_value);
    let wheel = |delta: f32| Pointer {
        position: Some(over),
        left_down: false,
        scroll: delta,
    };
    menu.update(wheel(1.0), false, &layout);
    menu.update(wheel(1.0), false, &layout);
    assert_eq!(menu.options().wild_level, Some(2));
    menu.update(wheel(-1.0), false, &layout);
    assert_eq!(menu.options().wild_level, Some(1));
}

#[test]
fn choosing_a_species_from_the_list() {
    let layout = layout();
    let mut menu = open_menu();
    assert!(!menu.dropdown_open());
    click(&mut menu, &layout, centre(layout.dropdown));
    assert!(menu.dropdown_open());

    // Scroll so Dratini (147) is visible, then click its row.
    menu.list_scroll = 147 - 3;
    click(&mut menu, &layout, centre(layout.list_row(3)));
    assert_eq!(menu.options().wild_species, Some(147));
    assert!(!menu.dropdown_open(), "choosing closes the list");
    assert!(menu.is_open(), "...but not the menu");
    assert!(menu.take_changed());

    // "ANY" (row 0) clears the species again.
    click(&mut menu, &layout, centre(layout.dropdown));
    assert_eq!(
        menu.list_scroll(),
        147 - LIST_VISIBLE_ROWS / 2,
        "opens at the selection"
    );
    menu.list_scroll = 0;
    click(&mut menu, &layout, centre(layout.list_row(0)));
    assert_eq!(menu.options().wild_species, None);
}

#[test]
fn the_list_scrolls_with_the_wheel_and_stops_at_both_ends() {
    let layout = layout();
    let mut menu = open_menu();
    click(&mut menu, &layout, centre(layout.dropdown));
    let wheel = |delta: f32| Pointer {
        position: Some(centre(layout.list_row(2))),
        left_down: false,
        scroll: delta,
    };
    for _ in 0..5 {
        menu.update(wheel(-1.0), false, &layout);
    }
    assert_eq!(
        menu.list_scroll(),
        5 * WHEEL_ROWS,
        "wheel down moves further into the list"
    );
    for _ in 0..100 {
        menu.update(wheel(1.0), false, &layout);
    }
    assert_eq!(menu.list_scroll(), 0, "stops at the top");
    for _ in 0..500 {
        menu.update(wheel(-1.0), false, &layout);
    }
    assert_eq!(
        menu.list_scroll(),
        LIST_ENTRIES - LIST_VISIBLE_ROWS,
        "stops at the end"
    );
    // The last species is reachable.
    click(
        &mut menu,
        &layout,
        centre(layout.list_row(LIST_VISIBLE_ROWS - 1)),
    );
    assert_eq!(menu.options().wild_species, Some(251));
}

#[test]
fn pressing_the_scrollbar_strip_jumps_without_choosing_or_closing() {
    let layout = layout();
    let mut menu = open_menu();
    click(&mut menu, &layout, centre(layout.dropdown));
    let strip_x = (layout.list.x + layout.list.width - SCROLLBAR_WIDTH / 2) as f32;
    // Bottom of the strip: the end of the list.
    let bottom = (layout.list.y + layout.list.height - 1) as f32;
    menu.update(at((strip_x, bottom), true), true, &layout);
    assert_eq!(menu.list_scroll(), LIST_ENTRIES - LIST_VISIBLE_ROWS);
    // Dragging to the top returns to the start.
    menu.update(at((strip_x, layout.list.y as f32), true), false, &layout);
    assert_eq!(menu.list_scroll(), 0);
    menu.update(at((strip_x, layout.list.y as f32), false), false, &layout);
    assert!(menu.dropdown_open(), "the list stays open");
    assert_eq!(menu.options().wild_species, None, "nothing was chosen");
    // Roughly the middle of the strip lands roughly mid-list.
    let middle = (layout.list.y + layout.list.height / 2) as f32;
    menu.update(at((strip_x, middle), true), true, &layout);
    let scroll = menu.list_scroll();
    assert!((100..=150).contains(&scroll), "{scroll}");
}

#[test]
fn a_click_outside_the_list_only_dismisses_the_list() {
    let layout = layout();
    let mut menu = open_menu();
    click(&mut menu, &layout, centre(layout.dropdown));
    // Somewhere on the panel, away from the list.
    let used = click(
        &mut menu,
        &layout,
        (layout.panel.x as f32 + 10.0, layout.panel.y as f32 + 10.0),
    );
    assert!(used);
    assert!(!menu.dropdown_open());
    assert!(menu.is_open());
    assert_eq!(menu.options().wild_species, None, "nothing was chosen");
}

#[test]
fn clicks_on_the_panel_are_used_and_clicks_outside_are_not() {
    let layout = layout();
    let mut menu = open_menu();
    let blank_panel = (layout.panel.x as f32 + 400.0, layout.panel.y as f32 + 25.0);
    assert!(click(&mut menu, &layout, blank_panel));
    assert!(!click(&mut menu, &layout, (5.0, 5.0)));
}

#[test]
fn list_rows_are_labelled_any_then_capitalised_names() {
    use super::draw::row_label;
    assert_eq!(row_label(0), "ANY");
    assert_eq!(row_label(1), "BULBASAUR");
    assert_eq!(row_label(147), "DRATINI");
    assert_eq!(row_label(122), "MR. MIME");
    assert_eq!(row_label(251), "CELEBI");
}

#[test]
fn every_label_the_menu_draws_has_a_glyph() {
    // The font would silently skip an unknown character.
    let mut labels: Vec<String> = [
        "CHEATS",
        "ALL EXPERIENCE IS BOOSTED",
        "SHINY CHANCE",
        "1/1000",
        "WILD POKEMON IS",
        "AT LEVEL",
        "PERFECT STATS FOR",
        "WILD POKEMON",
        "OPPONENTS POKEMON",
        "INFINITE MONEY",
        "100% CATCH RATE",
        "--",
        "-",
        "+",
        "X",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    labels.extend((0..LIST_ENTRIES).map(super::draw::row_label));
    for label in labels {
        for character in label.chars().filter(|c| *c != ' ') {
            let mut buffer = vec![0u32; 40 * 40];
            Canvas::new(&mut buffer, 40, 40, 0).text(&character.to_string(), 4, 4, 1);
            assert!(
                buffer.contains(&1),
                "no glyph for {character:?} in {label:?}"
            );
        }
    }
}

#[test]
fn drawing_every_state_stays_inside_the_window() {
    let layout = layout();
    let states = [
        (CheatOptions::default(), false),
        (
            CheatOptions {
                file_codes: true,
                wild_enabled: true,
                perfect_enabled: true,
                shiny: true,
                shiny_odds: ShinyOdds::from_one_in(1000).unwrap(),
                wild_species: Some(147),
                wild_level: Some(100),
                perfect_target: PerfectTarget::Opponents,
                infinite_money: true,
                catch_always: true,
                exp_boost: true,
                no_encounters: true,
                fast_battles: true,
            },
            false,
        ),
        (CheatOptions::default(), true),
        (
            CheatOptions {
                wild_species: Some(251),
                ..CheatOptions::default()
            },
            true,
        ),
    ];
    for (options, list_open) in states {
        let mut menu = open_menu();
        menu.set_options(options);
        if list_open {
            menu.open_dropdown();
        }
        let mut buffer = vec![0u32; WIDTH * HEIGHT];
        let mut canvas = Canvas::new(&mut buffer, WIDTH, HEIGHT, 0);
        menu.draw(&mut canvas, &layout);
        let painted: Vec<usize> = (0..buffer.len()).filter(|i| buffer[*i] != 0).collect();
        assert!(!painted.is_empty());
        // Everything drawn is within the panel (the list included).
        let panel = layout.panel;
        for index in painted {
            let (x, y) = ((index % WIDTH) as i32, (index / WIDTH) as i32);
            assert!(
                x >= panel.x - 4
                    && x < panel.x + panel.width + 4
                    && y >= panel.y - 4
                    && y < panel.y + panel.height + 4,
                "pixel ({x}, {y}) outside the panel"
            );
        }
    }
}

#[test]
fn the_rows_run_wild_perfect_stats_money_catch_experience_shiny_no_encounters_fast_battles_then_the_always_on_rows()
 {
    let layout = layout();
    let tops = [
        layout.dropdown.y,
        layout.perfect_dropdown.y,
        layout.money.y,
        layout.catch.y,
        layout.exp_boost.y,
        layout.shiny.y,
        layout.no_encounters.y,
        layout.fast_battles.y,
    ];
    let tops: Vec<i32> = tops
        .into_iter()
        .chain(layout.always_on.map(|rect| rect.y))
        .collect();
    assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
    // The always-on row is last: nothing in the panel is below it.
    for rect in [
        layout.dropdown,
        layout.perfect_dropdown,
        layout.money,
        layout.catch,
        layout.exp_boost,
        layout.shiny,
        layout.no_encounters,
        layout.fast_battles,
        layout.level_minus,
    ] {
        assert!(rect.y < layout.always_on[0].y);
    }
    // The perfect-stats dropdown shares its row with the row's label and
    // checkbox, to the right of both.
    assert_eq!(
        layout.perfect_dropdown.y + layout.perfect_dropdown.height / 2,
        layout.perfect_enable.y + layout.perfect_enable.height / 2
    );
    assert!(layout.perfect_label.0 < layout.perfect_dropdown.x);
}

#[test]
fn the_panel_lines_up_with_the_right_end_of_the_dock() {
    let layout = layout();
    let dock = dock();
    assert_eq!(
        layout.panel.x + layout.panel.width,
        dock.x + dock.width,
        "right edges match, over the CHEATS button"
    );
}

#[test]
fn the_species_list_fits_inside_the_panel_with_eight_rows() {
    assert_eq!(LIST_VISIBLE_ROWS, 8);
    let layout = layout();
    assert!(layout.list.y + layout.list.height <= layout.panel.y + layout.panel.height);
}

#[test]
fn the_always_on_rows_are_shown_but_are_not_controls() {
    let layout = layout();
    let mut menu = open_menu();
    for row in layout.always_on {
        // Clicking a box or its text changes nothing and reports nothing.
        for point in [centre(row), ((row.x + 200) as f32, centre(row).1)] {
            click(&mut menu, &layout, point);
            assert!(!menu.take_changed());
            assert!(menu.options().is_default());
        }
    }
    // Each is drawn with its tag, in the tag colour, at the right of its row.
    let mut drawn = vec![0u32; WIDTH * HEIGHT];
    menu.draw(&mut Canvas::new(&mut drawn, WIDTH, HEIGHT, 0), &layout);
    let mut single = vec![0u32; 400 * 40];
    Canvas::new(&mut single, 400, 40, 0).text("ALWAYS ON", 4, 4, 0x007F_B89A);
    let tag_pixels = single.iter().filter(|p| **p == 0x007F_B89A).count();
    for row in layout.always_on {
        let in_row = (row.y..row.y + row.height)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y as usize)))
            .filter(|&(x, y)| drawn[y * WIDTH + x] == 0x007F_B89A)
            .count();
        assert_eq!(in_row, tag_pixels, "the ALWAYS ON tag, once");
    }
}

#[test]
fn the_keyboard_moves_between_rows_and_wraps() {
    let mut menu = open_menu();
    assert_eq!(
        menu.focused_row(),
        None,
        "nothing is drawn until it is used"
    );
    // The first key only shows the focus, on the top row.
    menu.navigate(MenuNav::Down);
    assert_eq!(menu.focused_row(), Some(FocusRow::Wild));
    for expected in [
        FocusRow::Perfect,
        FocusRow::Money,
        FocusRow::Catch,
        FocusRow::Experience,
        FocusRow::Shiny,
        FocusRow::NoEncounters,
        FocusRow::FastBattles,
        FocusRow::Wild,
    ] {
        menu.navigate(MenuNav::Down);
        assert_eq!(menu.focused_row(), Some(expected));
    }
    menu.navigate(MenuNav::Up);
    assert_eq!(menu.focused_row(), Some(FocusRow::FastBattles), "wraps");
    // Closing the menu forgets that the keyboard was used.
    menu.close();
    menu.open();
    assert_eq!(menu.focused_row(), None);
}

#[test]
fn the_cheats_file_row_is_the_first_one_the_keyboard_reaches() {
    let mut menu = open_menu();
    menu.set_file_codes_available(3);
    menu.navigate(MenuNav::Down);
    assert_eq!(menu.focused_row(), Some(FocusRow::FileCodes), "the top row");
    menu.navigate(MenuNav::Up);
    assert_eq!(menu.focused_row(), Some(FocusRow::FastBattles), "wraps up");
    menu.navigate(MenuNav::Down);
    assert_eq!(menu.focused_row(), Some(FocusRow::FileCodes));
    menu.navigate(MenuNav::Toggle);
    assert!(menu.options().file_codes);
}

#[test]
fn a_switches_the_focused_row_only() {
    type Pick = fn(&CheatOptions) -> bool;
    let rows: [(Pick, usize); 7] = [
        (|o| o.wild_enabled, 0),
        (|o| o.perfect_enabled, 1),
        (|o| o.infinite_money, 2),
        (|o| o.catch_always, 3),
        (|o| o.exp_boost, 4),
        (|o| o.shiny, 5),
        (|o| o.no_encounters, 6),
    ];
    for (pick, down) in rows {
        let mut menu = open_menu();
        // The first press only shows the focus, on the top row.
        menu.navigate(MenuNav::Down);
        for _ in 0..down {
            menu.navigate(MenuNav::Down);
        }
        menu.navigate(MenuNav::Toggle);
        assert!(pick(&menu.options()), "row {down} on");
        assert!(menu.take_changed());
        let on = rows.iter().filter(|(p, _)| p(&menu.options())).count();
        assert_eq!(on, 1, "only that row");
        menu.navigate(MenuNav::Toggle);
        assert!(menu.options().is_default(), "and off again");
    }
}

/// A menu whose keyboard focus is on the wild row.
fn menu_on_the_wild_row() -> CheatMenu {
    let mut menu = open_menu();
    menu.navigate(MenuNav::Down);
    assert_eq!(menu.focused_row(), Some(FocusRow::Wild));
    menu
}

#[test]
fn left_and_right_move_between_the_parts_of_the_wild_row() {
    let mut menu = menu_on_the_wild_row();
    assert_eq!(menu.wild_part(), WildPart::Enable);
    for expected in [
        WildPart::Species,
        WildPart::LevelDown,
        WildPart::LevelUp,
        WildPart::LevelClear,
    ] {
        menu.navigate(MenuNav::Right);
        assert_eq!(menu.wild_part(), expected);
    }
    menu.navigate(MenuNav::Right);
    assert_eq!(menu.wild_part(), WildPart::LevelClear, "stops at the end");
    for expected in [
        WildPart::LevelUp,
        WildPart::LevelDown,
        WildPart::Species,
        WildPart::Enable,
    ] {
        menu.navigate(MenuNav::Left);
        assert_eq!(menu.wild_part(), expected);
    }
    menu.navigate(MenuNav::Left);
    assert_eq!(menu.wild_part(), WildPart::Enable, "stops at the start");
    // Nothing was changed by moving about.
    assert!(menu.options().is_default());
    assert!(!menu.take_changed());
}

#[test]
fn a_on_the_wild_rows_checkbox_and_level_buttons() {
    let mut menu = menu_on_the_wild_row();
    menu.navigate(MenuNav::Toggle);
    assert!(menu.options().wild_enabled);
    menu.navigate(MenuNav::Toggle);
    assert!(!menu.options().wild_enabled);
    // The level buttons: up from empty is 1, and it switches the row on.
    menu.navigate(MenuNav::Right);
    menu.navigate(MenuNav::Right);
    assert_eq!(menu.wild_part(), WildPart::LevelDown);
    menu.navigate(MenuNav::Toggle);
    assert_eq!(
        menu.options().wild_level,
        None,
        "down from empty stays empty"
    );
    menu.navigate(MenuNav::Right);
    assert_eq!(menu.wild_part(), WildPart::LevelUp);
    menu.navigate(MenuNav::Toggle);
    menu.navigate(MenuNav::Toggle);
    menu.navigate(MenuNav::Toggle);
    assert_eq!(menu.options().wild_level, Some(3));
    assert!(
        menu.options().wild_enabled,
        "choosing a level switches it on"
    );
    menu.navigate(MenuNav::Left);
    menu.navigate(MenuNav::Toggle);
    assert_eq!(menu.options().wild_level, Some(2));
}

#[test]
fn a_on_the_species_field_opens_the_list_and_up_down_a_choose() {
    let mut menu = menu_on_the_wild_row();
    menu.navigate(MenuNav::Right);
    assert_eq!(menu.wild_part(), WildPart::Species);
    menu.navigate(MenuNav::Toggle);
    assert!(menu.dropdown_open(), "A opens the list");
    assert_eq!(menu.list_cursor(), Some(0), "on ANY, the current choice");
    // Down moves the cursor; nothing is chosen until A.
    menu.navigate(MenuNav::Down);
    menu.navigate(MenuNav::Down);
    menu.navigate(MenuNav::Down);
    assert_eq!(menu.list_cursor(), Some(3));
    assert_eq!(menu.options().wild_species, None);
    menu.navigate(MenuNav::Toggle);
    assert!(!menu.dropdown_open(), "A closes it");
    assert_eq!(menu.options().wild_species, Some(3));
    assert!(menu.options().wild_enabled, "choosing switches the row on");
    assert!(menu.take_changed());
    // Reopening starts on the species in use.
    menu.navigate(MenuNav::Toggle);
    assert_eq!(menu.list_cursor(), Some(3));
}

#[test]
fn b_leaves_the_species_list_without_choosing_and_then_closes_the_menu() {
    let mut menu = menu_on_the_wild_row();
    menu.navigate(MenuNav::Right);
    menu.navigate(MenuNav::Toggle);
    menu.navigate(MenuNav::Down);
    menu.navigate(MenuNav::Down);
    menu.navigate(MenuNav::Back);
    assert!(!menu.dropdown_open());
    assert!(menu.is_open(), "the menu stays open");
    assert!(menu.options().is_default(), "nothing was chosen");
    assert!(!menu.take_changed());
    menu.navigate(MenuNav::Back);
    assert!(!menu.is_open(), "B again closes the menu");
}

#[test]
fn the_species_cursor_wraps_and_the_list_scrolls_to_keep_it_on_screen() {
    let mut menu = menu_on_the_wild_row();
    menu.navigate(MenuNav::Right);
    menu.navigate(MenuNav::Toggle);
    menu.navigate(MenuNav::Up);
    assert_eq!(
        menu.list_cursor(),
        Some(251),
        "up from ANY wraps to the last"
    );
    assert!(
        (menu.list_scroll()..menu.list_scroll() + LIST_VISIBLE_ROWS).contains(&251),
        "scrolled to show it"
    );
    menu.navigate(MenuNav::Down);
    assert_eq!(menu.list_cursor(), Some(0));
    assert_eq!(menu.list_scroll(), 0);
    for _ in 0..LIST_VISIBLE_ROWS {
        menu.navigate(MenuNav::Down);
    }
    let cursor = menu.list_cursor().unwrap();
    assert!((menu.list_scroll()..menu.list_scroll() + LIST_VISIBLE_ROWS).contains(&cursor));
    // Left and right do nothing while the list is open.
    let before = (menu.list_cursor(), menu.wild_part());
    menu.navigate(MenuNav::Left);
    menu.navigate(MenuNav::Right);
    assert_eq!((menu.list_cursor(), menu.wild_part()), before);
}

#[test]
fn left_and_right_cycle_the_perfect_stats_target() {
    let mut menu = open_menu();
    menu.navigate(MenuNav::Down);
    menu.navigate(MenuNav::Down);
    assert_eq!(menu.focused_row(), Some(FocusRow::Perfect));
    menu.navigate(MenuNav::Right);
    assert_eq!(menu.options().perfect_target, PerfectTarget::Opponents);
    assert!(menu.options().perfect_enabled, "choosing switches it on");
    menu.navigate(MenuNav::Right);
    menu.navigate(MenuNav::Right);
    assert_eq!(menu.options().perfect_target, PerfectTarget::Wild, "wraps");
    menu.navigate(MenuNav::Left);
    assert_eq!(menu.options().perfect_target, PerfectTarget::Both);
    // Other rows ignore left and right.
    menu.navigate(MenuNav::Down);
    let before = menu.options();
    menu.navigate(MenuNav::Left);
    menu.navigate(MenuNav::Right);
    assert_eq!(menu.options(), before);
}

#[test]
fn a_closed_menu_ignores_the_keys_and_a_mouse_list_is_dismissed_by_them() {
    let mut menu = open_menu();
    menu.open_perfect_list_for_smoke_test();
    assert!(menu.perfect_list_open());
    menu.navigate(MenuNav::Down);
    assert!(!menu.perfect_list_open());
    assert!(menu.options().is_default());

    let mut closed = CheatMenu::new();
    closed.navigate(MenuNav::Toggle);
    closed.navigate(MenuNav::Down);
    closed.navigate(MenuNav::Back);
    assert!(closed.options().is_default());
    assert!(!closed.take_changed());
    assert_eq!(closed.focused_row(), None);
}

#[test]
fn the_focused_row_is_drawn_as_a_band_and_only_then() {
    let layout = layout();
    let mut menu = open_menu();
    let pixels = |menu: &CheatMenu| {
        let mut drawn = vec![0u32; WIDTH * HEIGHT];
        menu.draw(&mut Canvas::new(&mut drawn, WIDTH, HEIGHT, 0), &layout);
        drawn
    };
    let before = pixels(&menu);
    menu.navigate(MenuNav::Down);
    let after = pixels(&menu);
    let band = layout.checkbox_of(FocusRow::Wild);
    let x = (layout.panel.x + 14) as usize;
    let y = (band.y + band.height / 2) as usize;
    assert_ne!(
        before[y * WIDTH + x],
        after[y * WIDTH + x],
        "the band tints the left edge of the focused row"
    );
    // Every pixel outside the panel is untouched.
    for row in 0..HEIGHT {
        for column in 0..WIDTH {
            let outside = (column as i32) < layout.panel.x
                || column as i32 >= layout.panel.x + layout.panel.width
                || (row as i32) < layout.panel.y
                || row as i32 >= layout.panel.y + layout.panel.height;
            if outside {
                assert_eq!(before[row * WIDTH + column], after[row * WIDTH + column]);
            }
        }
    }
}

#[test]
fn a_on_the_x_empties_the_level() {
    let mut menu = menu_on_the_wild_row();
    for _ in 0..4 {
        menu.navigate(MenuNav::Right);
    }
    assert_eq!(menu.wild_part(), WildPart::LevelClear);
    // Set a level with the level buttons first, then clear it.
    menu.navigate(MenuNav::Left);
    menu.navigate(MenuNav::Toggle);
    menu.navigate(MenuNav::Toggle);
    assert_eq!(menu.options().wild_level, Some(2));
    menu.navigate(MenuNav::Right);
    menu.navigate(MenuNav::Toggle);
    assert_eq!(menu.options().wild_level, None);
}

#[test]
fn holding_a_on_a_level_button_repeats_it() {
    let mut menu = menu_on_the_wild_row();
    menu.navigate(MenuNav::Right);
    menu.navigate(MenuNav::Right);
    menu.navigate(MenuNav::Right);
    assert_eq!(menu.wild_part(), WildPart::LevelUp);
    let start = Instant::now();
    // The press is one step; holding past the delay keeps stepping.
    menu.navigate(MenuNav::Toggle);
    menu.hold_toggle(true, start);
    assert_eq!(menu.options().wild_level, Some(1));
    for frame in 1..=60u64 {
        menu.hold_toggle(true, start + Duration::from_millis(frame * 16));
    }
    let level = menu.options().wild_level.unwrap();
    assert!(level >= 6, "one second of holding reached {level}");
    // Releasing stops it, and a later hold waits out the delay again.
    menu.hold_toggle(false, start + Duration::from_millis(1100));
    let after = menu.options().wild_level;
    menu.hold_toggle(true, start + Duration::from_millis(1200));
    menu.hold_toggle(true, start + Duration::from_millis(1300));
    assert_eq!(menu.options().wild_level, after);
}

#[test]
fn holding_a_does_nothing_off_the_level_buttons() {
    let mut menu = menu_on_the_wild_row();
    let start = Instant::now();
    for frame in 0..100u64 {
        menu.hold_toggle(true, start + Duration::from_millis(frame * 16));
    }
    assert!(menu.options().is_default());
}

#[test]
fn the_shiny_chance_steps_with_the_mouse_and_switches_the_row_on() {
    let layout = layout();
    let mut menu = open_menu();
    assert_eq!(menu.options().shiny_odds.one_in(), 1);
    // + at the most often does nothing; - steps to the next, 1/2.
    click(&mut menu, &layout, centre(layout.shiny_plus));
    assert_eq!(menu.options().shiny_odds.one_in(), 1);
    assert!(!menu.options().shiny, "no change, so no switch");
    click(&mut menu, &layout, centre(layout.shiny_minus));
    assert_eq!(menu.options().shiny_odds.one_in(), 2);
    assert!(menu.options().shiny, "choosing a chance switches it on");
    click(&mut menu, &layout, centre(layout.shiny_minus));
    click(&mut menu, &layout, centre(layout.shiny_minus));
    assert_eq!(menu.options().shiny_odds.one_in(), 10);
    click(&mut menu, &layout, centre(layout.shiny_plus));
    assert_eq!(menu.options().shiny_odds.one_in(), 5);
    // Unticking keeps the chance.
    click(&mut menu, &layout, centre(layout.shiny));
    assert!(!menu.options().shiny);
    assert_eq!(menu.options().shiny_odds.one_in(), 5);
}

#[test]
fn left_and_right_step_the_shiny_chance_on_its_row() {
    let mut menu = open_menu();
    // The first key shows the focus; then walk down to the shiny row.
    menu.navigate(MenuNav::Down);
    while menu.focused_row() != Some(FocusRow::Shiny) {
        menu.navigate(MenuNav::Down);
    }
    menu.navigate(MenuNav::Left);
    menu.navigate(MenuNav::Left);
    menu.navigate(MenuNav::Left);
    assert_eq!(menu.options().shiny_odds.one_in(), 10, "left is rarer");
    menu.navigate(MenuNav::Right);
    assert_eq!(menu.options().shiny_odds.one_in(), 5, "right is more often");
    assert!(menu.options().shiny);
}
