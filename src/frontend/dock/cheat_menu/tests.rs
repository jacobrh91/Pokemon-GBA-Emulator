use super::*;
use crate::frontend::draw::Canvas;
use gba_emulator::cheat_options::CheatOptions;

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
    for rect in [
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
    ] {
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
    let cases: [(Rect, Pick); 6] = [
        (layout.wild_enable, |o| o.wild_enabled),
        (layout.perfect_enable, |o| o.perfect_enabled),
        (layout.money, |o| o.infinite_money),
        (layout.catch, |o| o.catch_always),
        (layout.exp_boost, |o| o.exp_boost),
        (layout.shiny, |o| o.shiny),
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
    let column = [
        layout.file_codes.unwrap(),
        layout.wild_enable,
        layout.perfect_enable,
        layout.money,
        layout.catch,
        layout.exp_boost,
        layout.shiny,
    ];
    for rect in column {
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
    // edge is the scrollbar).
    let covered = (
        layout.perfect_dropdown.x as f32 + 10.0,
        centre(layout.perfect_dropdown).1,
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
    // Press: one step.
    menu.update(at(plus, true), true, &layout);
    assert_eq!(menu.options().wild_level, Some(1));
    // Held for less than the delay: nothing more.
    for _ in 0..(REPEAT_DELAY_FRAMES - 1) {
        menu.update(at(plus, true), false, &layout);
    }
    assert_eq!(menu.options().wild_level, Some(1));
    // Past the delay it repeats at the interval.
    for _ in 0..(REPEAT_INTERVAL_FRAMES * 10) {
        menu.update(at(plus, true), false, &layout);
    }
    let level = menu.options().wild_level.unwrap();
    assert!(
        (8..=13).contains(&level),
        "repeated roughly ten times: {level}"
    );
    // Releasing stops it.
    menu.update(at(plus, false), false, &layout);
    let after = menu.options().wild_level;
    for _ in 0..30 {
        menu.update(at(plus, false), false, &layout);
    }
    assert_eq!(menu.options().wild_level, after);
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
        "ALL POKEMON ARE SHINY",
        "WILD POKEMON IS",
        "AT LEVEL",
        "PERFECT STATS FOR",
        "WILD POKEMON",
        "OPPONENTS POKEMON",
        "INFINITE MONEY",
        "100% CATCH RATE",
        "SOON",
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
                wild_species: Some(147),
                wild_level: Some(100),
                perfect_target: PerfectTarget::Opponents,
                infinite_money: true,
                catch_always: true,
                exp_boost: true,
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
fn only_options_that_are_not_live_are_tagged_soon() {
    // SOON is painted for exactly the options `is_live` says do nothing yet.
    let layout = layout();
    let menu = open_menu();
    let mut with_tags = vec![0u32; WIDTH * HEIGHT];
    menu.draw(&mut Canvas::new(&mut with_tags, WIDTH, HEIGHT, 0), &layout);
    let soon_colour = 0x008A_9299u32;
    let count = with_tags.iter().filter(|p| **p == soon_colour).count();
    assert!(count > 0, "SOON tags are drawn");
    // Five rows carry a tag: perfect stats, money, catch rate, experience
    // and shiny.
    let tag_pixels_per_tag = count / 5;
    assert!(tag_pixels_per_tag > 20);
    assert_eq!(count % 5, 0, "every tag is drawn the same");
}

#[test]
fn the_rows_run_wild_perfect_stats_money_catch_experience_then_shiny() {
    let layout = layout();
    let tops = [
        layout.dropdown.y,
        layout.perfect_dropdown.y,
        layout.money.y,
        layout.catch.y,
        layout.exp_boost.y,
        layout.shiny.y,
    ];
    assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
    // Shiny is last: nothing in the panel is below it.
    for rect in [
        layout.dropdown,
        layout.perfect_dropdown,
        layout.money,
        layout.catch,
        layout.exp_boost,
        layout.level_minus,
    ] {
        assert!(rect.y < layout.shiny.y);
    }
    // Experience is second to last: only shiny is below it.
    assert!(layout.exp_boost.y < layout.shiny.y);
    assert!(layout.catch.y < layout.exp_boost.y);
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
