use super::draw::{HINT, RESET, WAITING, notice_text};
use super::*;
use crate::frontend::draw::{Canvas, text_width};

const WIDTH: usize = 960;
const HEIGHT: usize = 640;

fn dock() -> Rect {
    Rect {
        x: 80,
        y: 562,
        width: 850,
        height: 60,
    }
}

fn layout() -> ControlsLayout {
    ControlsLayout::new(WIDTH, dock())
}

fn centre(rect: Rect) -> (f32, f32) {
    (
        rect.x as f32 + rect.width as f32 / 2.0,
        rect.y as f32 + rect.height as f32 / 2.0,
    )
}

fn at(point: (f32, f32)) -> Pointer {
    Pointer {
        position: Some(point),
        left_down: true,
        scroll: 0.0,
    }
}

fn open_menu() -> ControlsMenu {
    let mut menu = ControlsMenu::new();
    menu.open();
    menu
}

/// One click: the press frame (the release does nothing).
fn click(menu: &mut ControlsMenu, layout: &ControlsLayout, point: (f32, f32)) -> bool {
    menu.update(at(point), true, layout)
}

#[test]
fn the_panel_fits_above_the_dock_and_holds_every_control() {
    let layout = layout();
    let panel = layout.panel;
    assert!(panel.x >= 0 && panel.x + panel.width <= WIDTH as i32);
    assert!(panel.y >= 0 && panel.y + panel.height <= dock().y);
    let mut inside: Vec<Rect> = layout.keys.to_vec();
    inside.push(layout.reset);
    for rect in inside {
        assert!(rect.x >= panel.x && rect.x + rect.width <= panel.x + panel.width);
        assert!(rect.y >= panel.y && rect.y + rect.height <= panel.y + panel.height);
    }
    assert!(layout.notice.1 + 14 <= panel.y + panel.height);
    // The longest label ends before the key buttons begin.
    let longest = Control::ALL
        .iter()
        .map(|control| text_width(control.label()))
        .max()
        .unwrap();
    assert!(panel.x + super::PADDING + longest + 12 <= layout.keys[0].x);
    // One row per control, top to bottom, none overlapping.
    for pair in layout.keys.windows(2) {
        assert!(pair[0].y + pair[0].height <= pair[1].y);
    }
    let last = layout.keys[ROWS - 1];
    assert!(last.y + last.height <= layout.reset.y);
    // A dock pushed to the edge keeps the panel on screen.
    let far_right = ControlsLayout::new(WIDTH, Rect { x: 900, ..dock() });
    assert!(far_right.panel.x + far_right.panel.width <= WIDTH as i32);
}

#[test]
fn clicking_a_key_waits_for_a_key_and_the_next_press_binds_it() {
    let layout = layout();
    let mut menu = open_menu();
    assert_eq!(menu.waiting(), None);
    assert!(click(&mut menu, &layout, centre(layout.keys[4])), "A's row");
    assert_eq!(menu.waiting(), Some(Control::A));
    assert!(!menu.take_changed(), "waiting changes nothing yet");

    menu.key_pressed(Key::Z);
    assert_eq!(menu.waiting(), None);
    assert_eq!(menu.bindings().key(Control::A), Key::Z);
    assert!(menu.take_changed());
    assert!(!menu.take_changed(), "reported once");
    assert_eq!(menu.notice(), Notice::None);
    // Keys pressed with nothing waiting are ignored.
    menu.key_pressed(Key::Y);
    assert_eq!(menu.bindings().key(Control::A), Key::Z);
    assert!(!menu.take_changed());
}

#[test]
fn a_key_that_belongs_to_another_button_swaps_and_says_so() {
    let layout = layout();
    let mut menu = open_menu();
    click(&mut menu, &layout, centre(layout.keys[4]));
    // W is Up's.
    menu.key_pressed(Key::W);
    assert_eq!(menu.bindings().key(Control::A), Key::W);
    assert_eq!(menu.bindings().key(Control::Up), Key::Space);
    assert_eq!(menu.notice(), Notice::Swapped(Control::Up));
    assert!(menu.take_changed());
    let (text, warning) = notice_text(menu.notice(), false);
    assert_eq!(text, "SWAPPED WITH UP");
    assert!(!warning);
}

#[test]
fn a_key_that_cannot_be_bound_keeps_waiting() {
    let layout = layout();
    let mut menu = open_menu();
    click(&mut menu, &layout, centre(layout.keys[0]));
    menu.key_pressed(Key::Escape);
    assert_eq!(menu.waiting(), Some(Control::Up), "still waiting");
    assert_eq!(menu.notice(), Notice::Refused);
    assert!(!menu.take_changed());
    assert!(notice_text(menu.notice(), true).1, "shown as a warning");
    menu.key_pressed(Key::T);
    assert_eq!(menu.bindings().key(Control::Up), Key::T);
    assert_eq!(menu.waiting(), None);
}

#[test]
fn clicking_another_key_moves_the_wait_and_b_cancels_then_closes() {
    let layout = layout();
    let mut menu = open_menu();
    click(&mut menu, &layout, centre(layout.keys[0]));
    click(&mut menu, &layout, centre(layout.keys[5]));
    assert_eq!(menu.waiting(), Some(Control::B));
    menu.back();
    assert_eq!(menu.waiting(), None);
    assert!(menu.is_open(), "the first B only stops waiting");
    menu.back();
    assert!(!menu.is_open());
    // Closing also forgets a wait.
    menu.open();
    click(&mut menu, &layout, centre(layout.keys[1]));
    menu.close();
    menu.open();
    assert_eq!(menu.waiting(), None);
}

#[test]
fn reset_restores_the_default_keys() {
    let layout = layout();
    let mut menu = open_menu();
    menu.set_bindings({
        let mut bindings = Bindings::default();
        bindings.bind(Control::A, Key::Z);
        bindings
    });
    assert!(!menu.take_changed(), "setting them is not a change");
    assert!(click(&mut menu, &layout, centre(layout.reset)));
    assert!(menu.bindings().is_default());
    assert!(menu.take_changed());
}

#[test]
fn clicks_on_the_panel_are_used_and_clicks_elsewhere_are_not() {
    let layout = layout();
    let mut menu = open_menu();
    assert!(click(
        &mut menu,
        &layout,
        (layout.panel.x as f32 + 5.0, layout.panel.y as f32 + 5.0)
    ));
    assert!(!click(&mut menu, &layout, (5.0, 5.0)));
    let mut closed = ControlsMenu::new();
    assert!(
        !click(&mut closed, &layout, centre(layout.keys[0])),
        "closed"
    );
    assert_eq!(closed.waiting(), None);
    // A click on empty panel space stops a wait.
    click(&mut menu, &layout, centre(layout.keys[2]));
    click(
        &mut menu,
        &layout,
        (layout.panel.x as f32 + 5.0, layout.panel.y as f32 + 5.0),
    );
    assert_eq!(menu.waiting(), None);
}

#[test]
fn every_label_the_menu_draws_has_a_glyph() {
    let mut labels = vec![
        "CONTROLS".to_owned(),
        HINT.to_owned(),
        WAITING.to_owned(),
        RESET.to_owned(),
        notice_text(Notice::Refused, false).0,
        notice_text(Notice::None, true).0,
    ];
    labels.extend(Control::ALL.map(|control| control.label().to_owned()));
    labels.extend(Control::ALL.map(|control| notice_text(Notice::Swapped(control), false).0));
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
    let mut states = Vec::new();
    for waiting in [None, Some(Control::Select)] {
        let mut menu = open_menu();
        if let Some(control) = waiting {
            click(&mut menu, &layout, centre(layout.keys[control as usize]));
        }
        states.push(menu);
    }
    let mut refused = open_menu();
    click(&mut refused, &layout, centre(layout.keys[0]));
    refused.key_pressed(Key::Escape);
    states.push(refused);
    for menu in states {
        let mut buffer = vec![0u32; WIDTH * HEIGHT];
        let mut canvas = Canvas::new(&mut buffer, WIDTH, HEIGHT, 0);
        menu.draw(&mut canvas, &layout);
        let painted = buffer.iter().filter(|pixel| **pixel != 0).count();
        assert!(painted > 1000, "the panel is drawn");
    }
}

#[test]
fn the_cheat_menu_and_fast_forward_can_be_remapped_like_the_buttons() {
    let layout = layout();
    let mut menu = open_menu();
    click(&mut menu, &layout, centre(layout.keys[10]));
    assert_eq!(menu.waiting(), Some(Control::CheatMenu));
    // Tab is the cheat menu's own key and can be bound again; Escape cannot.
    menu.key_pressed(Key::Escape);
    assert_eq!(menu.notice(), Notice::Refused);
    menu.key_pressed(Key::M);
    assert_eq!(menu.bindings().key(Control::CheatMenu), Key::M);

    click(&mut menu, &layout, centre(layout.keys[11]));
    assert_eq!(menu.waiting(), Some(Control::FastForward));
    menu.key_pressed(Key::Tab);
    assert_eq!(menu.bindings().key(Control::FastForward), Key::Tab);
    assert!(menu.take_changed());
}
