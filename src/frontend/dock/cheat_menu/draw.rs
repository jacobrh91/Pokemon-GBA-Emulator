//! Painting the cheat menu.

use super::{
    CheatMenu, FocusRow, LIST_ENTRIES, LIST_ROW_HEIGHT, LIST_VISIBLE_ROWS, MenuLayout, WildPart,
};
use crate::frontend::dock::draw::{DOCK_COLOR, FILL_COLOR, ICON_COLOR, TRACK_COLOR};
use crate::frontend::draw::{Canvas, GLYPH_HEIGHT, Rect, TEXT_SCALE, text_width};
use gba_emulator::cheats::options::{CheatOption, CheatOptions, PerfectTarget};
use gba_emulator::cheats::species;

/// Colour of the SOON tag on options that do not change the game yet.
const SOON_COLOR: u32 = 0x008A_9299;
/// Text of a row that is switched off (its choices stay visible).
const OFF_COLOR: u32 = 0x00B4_BBC2;
/// Colour of the ALWAYS ON tag (distinct from the SOON tag, which the menu's
/// tests count).
const ALWAYS_ON_COLOR: u32 = 0x007F_B89A;
/// The tag of a cheat that is always on (it has no switch).
pub(super) const ALWAYS_ON_TAG: &str = "ALWAYS ON";
/// The check mark: down-right, then up-right, in 3x3 blocks.
const CHECK_MARK: [(i32, i32); 8] = [
    (5, 11),
    (7, 13),
    (9, 15),
    (11, 17),
    (13, 15),
    (15, 13),
    (17, 11),
    (19, 9),
];
const PADDING: i32 = 24;
const TEXT_HEIGHT: i32 = GLYPH_HEIGHT * TEXT_SCALE;

/// The species list's text for row `index`: ANY, then the names in capitals.
pub(super) fn row_label(index: usize) -> String {
    if index == 0 {
        "ANY".to_owned()
    } else {
        species::name(index as u16)
            .unwrap_or("?")
            .to_ascii_uppercase()
    }
}

impl CheatMenu {
    /// Paints the menu (and the open species list) over the frame.
    pub(in crate::frontend::dock) fn draw(&self, canvas: &mut Canvas, layout: &MenuLayout) {
        canvas.rounded_rect(layout.panel, 14, DOCK_COLOR, 0.95);
        canvas.text(
            "CHEATS",
            layout.panel.x + PADDING,
            layout.title_y,
            FILL_COLOR,
        );
        canvas.rect(
            layout.panel.x + PADDING,
            layout.title_y + TEXT_HEIGHT + 10,
            layout.panel.width - 2 * PADDING,
            2,
            TRACK_COLOR,
        );
        // The row the keyboard acts on, once the keyboard has been used.
        if let Some(row) = self.focused_row() {
            let box_ = layout.checkbox_of(row);
            canvas.rounded_rect(
                Rect {
                    x: layout.panel.x + 10,
                    y: box_.y - 8,
                    width: layout.panel.width - 20,
                    height: box_.height + 16,
                },
                8,
                FILL_COLOR,
                0.2,
            );
        }
        // On the wild row, the part of it the keyboard will use.
        if self.focused_row() == Some(FocusRow::Wild) {
            let part = match self.wild_part() {
                WildPart::Enable => layout.wild_enable,
                WildPart::Species => layout.dropdown,
                WildPart::LevelDown => layout.level_minus,
                WildPart::LevelUp => layout.level_plus,
                WildPart::LevelClear => layout.level_clear,
            };
            canvas.rounded_rect(
                Rect {
                    x: part.x - 4,
                    y: part.y - 4,
                    width: part.width + 8,
                    height: part.height + 8,
                },
                10,
                FILL_COLOR,
                0.7,
            );
        }
        let right = layout.panel.x + layout.panel.width - PADDING;
        let soon = |canvas: &mut Canvas, option: CheatOption, centre_y: i32| {
            if !CheatOptions::is_live(option) {
                canvas.text(
                    "SOON",
                    right - text_width("SOON"),
                    centre_y - TEXT_HEIGHT / 2,
                    SOON_COLOR,
                );
            }
        };

        let options = self.options();
        // A row that is switched off keeps its choices on show, dimmed.
        let tone = |on: bool| if on { ICON_COLOR } else { OFF_COLOR };

        // The cheats file's codes, when it has some.
        if let Some(rect) = layout.file_codes {
            checkbox(canvas, rect, options.file_codes);
            checkbox_label(
                canvas,
                rect,
                &format!("{} CODES FROM THE CHEATS FILE", self.file_codes_available()),
            );
        }

        // Wild Pokémon is <species> at level <n>.
        let wild_tone = tone(options.wild_enabled);
        checkbox(canvas, layout.wild_enable, options.wild_enabled);
        canvas.text(
            "WILD POKEMON IS",
            layout.wild_label.0,
            layout.wild_label.1,
            wild_tone,
        );
        let species_text = row_label(options.wild_species.map_or(0, usize::from));
        field(canvas, layout.dropdown, self.dropdown_open());
        canvas.text(
            &species_text,
            layout.dropdown.x + 12,
            centre(layout.dropdown) - TEXT_HEIGHT / 2,
            wild_tone,
        );
        caret(
            canvas,
            layout.dropdown.x + layout.dropdown.width - 18,
            centre(layout.dropdown),
            wild_tone,
        );
        canvas.text(
            "AT LEVEL",
            layout.level_label.0,
            layout.level_label.1,
            wild_tone,
        );
        button(canvas, layout.level_minus, "-", wild_tone);
        button(canvas, layout.level_plus, "+", wild_tone);
        button(canvas, layout.level_clear, "X", wild_tone);
        field(canvas, layout.level_value, false);
        let level_text = options
            .wild_level
            .map_or_else(|| "--".to_owned(), |level| level.to_string());
        canvas.text(
            &level_text,
            layout.level_value.x + (layout.level_value.width - text_width(&level_text)) / 2,
            centre(layout.level_value) - TEXT_HEIGHT / 2,
            wild_tone,
        );

        // Perfect stats: its own switch, then whose Pokémon.
        let perfect_tone = tone(options.perfect_enabled);
        checkbox(canvas, layout.perfect_enable, options.perfect_enabled);
        canvas.text(
            "PERFECT STATS FOR",
            layout.perfect_label.0,
            layout.perfect_label.1,
            perfect_tone,
        );
        field(canvas, layout.perfect_dropdown, self.perfect_list_open());
        canvas.text(
            options.perfect_target.label(),
            layout.perfect_dropdown.x + 12,
            centre(layout.perfect_dropdown) - TEXT_HEIGHT / 2,
            perfect_tone,
        );
        caret(
            canvas,
            layout.perfect_dropdown.x + layout.perfect_dropdown.width - 18,
            centre(layout.perfect_dropdown),
            perfect_tone,
        );
        soon(
            canvas,
            CheatOption::PerfectStats,
            layout.perfect_label.1 + TEXT_HEIGHT / 2,
        );

        // Money, catch rate, experience, shiny, then no encounters at the bottom.
        checkbox(canvas, layout.money, options.infinite_money);
        checkbox_label(canvas, layout.money, "INFINITE MONEY");
        soon(canvas, CheatOption::InfiniteMoney, centre(layout.money));
        checkbox(canvas, layout.catch, options.catch_always);
        checkbox_label(canvas, layout.catch, "100% CATCH RATE");
        soon(canvas, CheatOption::CatchAlways, centre(layout.catch));
        checkbox(canvas, layout.exp_boost, options.exp_boost);
        checkbox_label(canvas, layout.exp_boost, "ALL EXPERIENCE IS BOOSTED");
        soon(canvas, CheatOption::ExpBoost, centre(layout.exp_boost));
        checkbox(canvas, layout.shiny, options.shiny);
        checkbox_label(canvas, layout.shiny, "ALL POKEMON ARE SHINY");
        soon(canvas, CheatOption::Shiny, centre(layout.shiny));
        checkbox(canvas, layout.no_encounters, options.no_encounters);
        checkbox_label(canvas, layout.no_encounters, "NO RANDOM ENCOUNTERS");
        soon(
            canvas,
            CheatOption::NoEncounters,
            centre(layout.no_encounters),
        );
        // Always on: ticked, dimmed, tagged, and not a control.
        canvas.rounded_rect(layout.always_on, 6, TRACK_COLOR, 0.8);
        for (dx, dy) in CHECK_MARK {
            canvas.rect(
                layout.always_on.x + dx - 1,
                layout.always_on.y + dy - 4,
                3,
                3,
                OFF_COLOR,
            );
        }
        canvas.text(
            "TMS ARE INFINITELY USABLE",
            layout.always_on.x + layout.always_on.width + 12,
            centre(layout.always_on) - TEXT_HEIGHT / 2,
            OFF_COLOR,
        );
        canvas.text(
            ALWAYS_ON_TAG,
            right - text_width(ALWAYS_ON_TAG),
            centre(layout.always_on) - TEXT_HEIGHT / 2,
            ALWAYS_ON_COLOR,
        );

        if self.dropdown_open() {
            self.draw_list(canvas, layout);
        }
        if self.perfect_list_open() {
            self.draw_perfect_list(canvas, layout);
        }
    }

    /// The three-row list of perfect-stats targets under its dropdown.
    fn draw_perfect_list(&self, canvas: &mut Canvas, layout: &MenuLayout) {
        canvas.rounded_rect(
            Rect {
                x: layout.perfect_list.x - 3,
                y: layout.perfect_list.y - 3,
                width: layout.perfect_list.width + 6,
                height: layout.perfect_list.height + 6,
            },
            9,
            TRACK_COLOR,
            1.0,
        );
        canvas.rounded_rect(layout.perfect_list, 8, DOCK_COLOR, 1.0);
        for (index, target) in PerfectTarget::ALL.into_iter().enumerate() {
            let rect = layout.perfect_row(index);
            let selected = target == self.options().perfect_target;
            if selected {
                canvas.rounded_rect(
                    Rect {
                        x: rect.x + 3,
                        y: rect.y + 2,
                        width: rect.width - 6,
                        height: rect.height - 4,
                    },
                    6,
                    FILL_COLOR,
                    1.0,
                );
            }
            canvas.text(
                target.label(),
                rect.x + 12,
                rect.y + (LIST_ROW_HEIGHT - TEXT_HEIGHT) / 2,
                if selected { DOCK_COLOR } else { ICON_COLOR },
            );
        }
    }

    fn draw_list(&self, canvas: &mut Canvas, layout: &MenuLayout) {
        canvas.rounded_rect(
            Rect {
                x: layout.list.x - 3,
                y: layout.list.y - 3,
                width: layout.list.width + 6,
                height: layout.list.height + 6,
            },
            9,
            TRACK_COLOR,
            1.0,
        );
        canvas.rounded_rect(layout.list, 8, DOCK_COLOR, 1.0);
        // The keyboard's cursor, or else the species in use.
        let selected = self
            .list_cursor()
            .unwrap_or_else(|| self.options().wild_species.map_or(0, usize::from));
        for row in 0..LIST_VISIBLE_ROWS {
            let index = self.list_scroll() + row;
            if index >= LIST_ENTRIES {
                break;
            }
            let rect = layout.list_row(row);
            let (text_color, highlighted) = if index == selected {
                (DOCK_COLOR, true)
            } else {
                (ICON_COLOR, false)
            };
            if highlighted {
                canvas.rounded_rect(
                    Rect {
                        x: rect.x + 3,
                        y: rect.y + 2,
                        width: rect.width - 6,
                        height: rect.height - 4,
                    },
                    6,
                    FILL_COLOR,
                    1.0,
                );
            }
            canvas.text(
                &row_label(index),
                rect.x + 12,
                rect.y + (LIST_ROW_HEIGHT - TEXT_HEIGHT) / 2,
                text_color,
            );
        }
        // Scroll position: a thin thumb down the right edge.
        let track_height = layout.list.height - 8;
        let thumb_height = (track_height * LIST_VISIBLE_ROWS as i32 / LIST_ENTRIES as i32).max(14);
        let travel = track_height - thumb_height;
        let thumb_y = layout.list.y
            + 4
            + travel * self.list_scroll() as i32 / (LIST_ENTRIES - LIST_VISIBLE_ROWS) as i32;
        canvas.rounded_rect(
            Rect {
                x: layout.list.x + layout.list.width - 8,
                y: thumb_y,
                width: 4,
                height: thumb_height,
            },
            2,
            TRACK_COLOR,
            1.0,
        );
    }
}

fn centre(rect: Rect) -> i32 {
    rect.y + rect.height / 2
}

/// A rounded box that holds a value (the dropdown, the level).
fn field(canvas: &mut Canvas, rect: Rect, active: bool) {
    canvas.rounded_rect(rect, 8, if active { FILL_COLOR } else { TRACK_COLOR }, 0.55);
}

fn button(canvas: &mut Canvas, rect: Rect, symbol: &str, colour: u32) {
    canvas.rounded_rect(rect, 8, TRACK_COLOR, 0.8);
    canvas.text(
        symbol,
        rect.x + (rect.width - text_width(symbol)) / 2,
        centre(rect) - TEXT_HEIGHT / 2,
        colour,
    );
}

/// A downward-pointing triangle marking a dropdown.
fn caret(canvas: &mut Canvas, x: i32, y: i32, colour: u32) {
    canvas.triangle(
        ((x - 7) as f32, (y - 4) as f32),
        ((x + 7) as f32, (y - 4) as f32),
        (x as f32, (y + 5) as f32),
        colour,
    );
}

fn checkbox(canvas: &mut Canvas, rect: Rect, checked: bool) {
    if checked {
        canvas.rounded_rect(rect, 6, FILL_COLOR, 1.0);
        for (dx, dy) in CHECK_MARK {
            canvas.rect(rect.x + dx - 1, rect.y + dy - 4, 3, 3, DOCK_COLOR);
        }
    } else {
        canvas.rounded_rect(rect, 6, TRACK_COLOR, 0.8);
    }
}

fn checkbox_label(canvas: &mut Canvas, checkbox: Rect, label: &str) {
    canvas.text(
        label,
        checkbox.x + checkbox.width + 12,
        centre(checkbox) - TEXT_HEIGHT / 2,
        ICON_COLOR,
    );
}
