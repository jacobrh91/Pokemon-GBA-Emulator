//! The cheat menu: a panel that opens above the dock from its CHEATS button.
//!
//! | Control | Option |
//! |---|---|
//! Every row has its own checkbox at the left, which switches that cheat on
//! and off:
//!
//! | Row | Controls after the checkbox |
//! |---|---|
//! | cheats file (only when it has codes) | |
//! | wild Pokémon is *species* at level *n* | dropdown + level stepper (either may be left empty) |
//! | perfect stats for | dropdown: wild Pokémon / opponents' Pokémon / both |
//! | infinite money | |
//! | 100% catch rate | |
//! | all experience is boosted | |
//! | all Pokémon are shiny | |
//!
//! Choosing a species or level, or a perfect-stats target, also
//! switches its row on, so a choice never silently does nothing; unticking
//! the row's checkbox keeps the choices for later.
//!
//! Interaction ([`CheatMenu::update`]) is separate from drawing (`draw.rs`) so
//! it can be tested without a window. Options that do not change the game yet
//! (see [`CheatOptions::is_live`]) are tagged SOON in the panel; at present
//! every row is live.

mod draw;

use super::Pointer;
use crate::frontend::draw::{GLYPH_HEIGHT, Rect, TEXT_SCALE, text_width};
use gba_emulator::cheats::options::{CheatOptions, MAX_LEVEL, PerfectTarget};
use gba_emulator::cheats::species::LAST_SPECIES;

/// Species-list rows: "ANY" followed by the 251 species.
pub(super) const LIST_ENTRIES: usize = LAST_SPECIES as usize + 1;
const LIST_VISIBLE_ROWS: usize = 8;
/// Rows the wheel moves the species list per notch.
const WHEEL_ROWS: usize = 3;
/// Width of the strip at the list's right edge that works as a scrollbar.
const SCROLLBAR_WIDTH: i32 = 22;
const LIST_ROW_HEIGHT: i32 = 28;
/// Frames a stepper button must be held before it starts repeating, and the
/// interval between repeats afterwards.
const REPEAT_DELAY_FRAMES: u32 = 24;
const REPEAT_INTERVAL_FRAMES: u32 = 3;

pub(super) const PANEL_WIDTH: i32 = 820;
/// Space the row checkbox takes before a row's own controls.
const LEAD: i32 = 36;
const PADDING: i32 = 24;
const CHECKBOX: i32 = 24;
const CONTROL_HEIGHT: i32 = 32;
const DROPDOWN_WIDTH: i32 = 190;
const LEVEL_VALUE_WIDTH: i32 = 60;
/// Wide enough for "OPPONENTS POKEMON" and the caret.
const PERFECT_DROPDOWN_WIDTH: i32 = 280;

/// Where every control sits for a given window and dock.
#[derive(Clone, Copy, Debug)]
pub(super) struct MenuLayout {
    pub(super) panel: Rect,
    pub(super) title_y: i32,
    /// The cheats file's row; present only when the file has codes.
    pub(super) file_codes: Option<Rect>,
    /// Row switches: the checkbox at the left of the wild row, and of the
    /// perfect-stats row.
    pub(super) wild_enable: Rect,
    pub(super) perfect_enable: Rect,
    pub(super) wild_label: (i32, i32),
    pub(super) dropdown: Rect,
    pub(super) level_label: (i32, i32),
    pub(super) level_minus: Rect,
    pub(super) level_value: Rect,
    pub(super) level_plus: Rect,
    pub(super) level_clear: Rect,
    pub(super) perfect_label: (i32, i32),
    /// The perfect-stats target dropdown, and its three-row list below it.
    pub(super) perfect_dropdown: Rect,
    pub(super) perfect_list: Rect,
    pub(super) money: Rect,
    pub(super) catch: Rect,
    pub(super) exp_boost: Rect,
    /// Last in the list.
    pub(super) shiny: Rect,
    /// The open species list, below the dropdown.
    pub(super) list: Rect,
}

impl MenuLayout {
    /// `dock` is the dock's rectangle; the panel sits just above it, aligned
    /// to its right edge (above the CHEATS button) and kept inside the window.
    pub(super) fn new(width: usize, dock: Rect, file_row: bool) -> Self {
        // The optional file row pushes every other row down by one.
        let extra = if file_row { 44 } else { 0 };
        let panel_height = 344 + extra;
        let x = (dock.x + dock.width - PANEL_WIDTH)
            .min(width as i32 - PANEL_WIDTH - 12)
            .max(12);
        let y = (dock.y - 12 - panel_height).max(8);
        let panel = Rect {
            x,
            y,
            width: PANEL_WIDTH,
            height: panel_height,
        };
        let left = x + PADDING;
        let text_h = GLYPH_HEIGHT * TEXT_SCALE;
        let checkbox_at = |left: i32, centre_y: i32| Rect {
            x: left,
            y: centre_y - CHECKBOX / 2,
            width: CHECKBOX,
            height: CHECKBOX,
        };

        // Rows, top to bottom: (centre y) of each.
        let row_file = y + 78;
        let row_wild = y + 78 + extra;
        let row_perfect = y + 126 + extra;
        let row_money = y + 170 + extra;
        let row_catch = y + 214 + extra;
        let row_exp = y + 258 + extra;
        let row_shiny = y + 302 + extra;

        // Rows with controls of their own start after their checkbox.
        let wild_label = (left + LEAD, row_wild - text_h / 2);
        let dropdown = Rect {
            x: left + LEAD + text_width("WILD POKEMON IS") + 14,
            y: row_wild - CONTROL_HEIGHT / 2,
            width: DROPDOWN_WIDTH,
            height: CONTROL_HEIGHT,
        };
        let level_label = (dropdown.x + dropdown.width + 16, row_wild - text_h / 2);
        let level_minus = Rect {
            x: level_label.0 + text_width("AT LEVEL") + 14,
            y: row_wild - CONTROL_HEIGHT / 2,
            width: CONTROL_HEIGHT,
            height: CONTROL_HEIGHT,
        };
        let level_value = Rect {
            x: level_minus.x + level_minus.width + 4,
            y: level_minus.y,
            width: LEVEL_VALUE_WIDTH,
            height: CONTROL_HEIGHT,
        };
        let level_plus = Rect {
            x: level_value.x + level_value.width + 4,
            y: level_minus.y,
            width: CONTROL_HEIGHT,
            height: CONTROL_HEIGHT,
        };
        let level_clear = Rect {
            x: level_plus.x + level_plus.width + 10,
            y: level_minus.y,
            width: CONTROL_HEIGHT,
            height: CONTROL_HEIGHT,
        };

        // Perfect stats: its label, then a dropdown of whose Pokémon.
        let perfect_dropdown = Rect {
            x: left + LEAD + text_width("PERFECT STATS FOR") + 14,
            y: row_perfect - CONTROL_HEIGHT / 2,
            width: PERFECT_DROPDOWN_WIDTH,
            height: CONTROL_HEIGHT,
        };

        Self {
            panel,
            title_y: y + 20,
            file_codes: file_row.then(|| checkbox_at(left, row_file)),
            wild_enable: checkbox_at(left, row_wild),
            perfect_enable: checkbox_at(left, row_perfect),
            wild_label,
            dropdown,
            level_label,
            level_minus,
            level_value,
            level_plus,
            level_clear,
            perfect_label: (left + LEAD, row_perfect - text_h / 2),
            perfect_dropdown,
            perfect_list: Rect {
                x: perfect_dropdown.x,
                y: perfect_dropdown.y + perfect_dropdown.height + 4,
                width: perfect_dropdown.width,
                height: PerfectTarget::ALL.len() as i32 * LIST_ROW_HEIGHT,
            },
            money: checkbox_at(left, row_money),
            catch: checkbox_at(left, row_catch),
            exp_boost: checkbox_at(left, row_exp),
            shiny: checkbox_at(left, row_shiny),
            list: Rect {
                x: dropdown.x,
                y: dropdown.y + dropdown.height + 4,
                width: dropdown.width,
                height: LIST_VISIBLE_ROWS as i32 * LIST_ROW_HEIGHT,
            },
        }
    }

    /// The rectangle of row `index` of the perfect-stats list.
    pub(super) fn perfect_row(&self, index: usize) -> Rect {
        Rect {
            x: self.perfect_list.x,
            y: self.perfect_list.y + index as i32 * LIST_ROW_HEIGHT,
            width: self.perfect_list.width,
            height: LIST_ROW_HEIGHT,
        }
    }

    /// The rectangle of visible list row `row` (0-based from the top).
    pub(super) fn list_row(&self, row: usize) -> Rect {
        Rect {
            x: self.list.x,
            y: self.list.y + row as i32 * LIST_ROW_HEIGHT,
            width: self.list.width,
            height: LIST_ROW_HEIGHT,
        }
    }
}

pub(super) struct CheatMenu {
    open: bool,
    options: CheatOptions,
    dropdown_open: bool,
    /// The perfect-stats target list is open.
    perfect_list_open: bool,
    /// Index of the first visible species-list row (0 is "ANY").
    list_scroll: usize,
    /// Frames a stepper button has been held, for auto-repeat.
    held_frames: u32,
    changed: bool,
    /// Number of code lines in the cheats file; the file row shows when
    /// this is not zero.
    file_codes_available: usize,
}

impl CheatMenu {
    pub(super) fn new() -> Self {
        Self {
            open: false,
            options: CheatOptions::default(),
            dropdown_open: false,
            perfect_list_open: false,
            list_scroll: 0,
            held_frames: 0,
            changed: false,
            file_codes_available: 0,
        }
    }

    /// Tells the menu how many codes the cheats file holds; with none, the
    /// file's row is not shown.
    pub(super) fn set_file_codes_available(&mut self, count: usize) {
        self.file_codes_available = count;
    }

    pub(super) fn file_codes_available(&self) -> usize {
        self.file_codes_available
    }

    /// Whether the cheats file's row is shown.
    pub(super) fn has_file_row(&self) -> bool {
        self.file_codes_available > 0
    }

    pub(super) fn options(&self) -> CheatOptions {
        self.options
    }

    /// Replaces the options (from saved settings) without reporting a change.
    pub(super) fn set_options(&mut self, options: CheatOptions) {
        self.options = options;
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn dropdown_open(&self) -> bool {
        self.dropdown_open
    }

    pub(super) fn perfect_list_open(&self) -> bool {
        self.perfect_list_open
    }

    pub(super) fn list_scroll(&self) -> usize {
        self.list_scroll
    }

    #[cfg(test)]
    pub(super) fn open(&mut self) {
        self.open = true;
    }

    pub(super) fn toggle(&mut self) {
        self.open = !self.open;
        self.dropdown_open = false;
        self.perfect_list_open = false;
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.dropdown_open = false;
        self.perfect_list_open = false;
    }

    /// Opens the species list (for the launch smoke test, which has no mouse).
    pub(super) fn open_dropdown(&mut self) {
        self.open = true;
        self.dropdown_open = true;
        self.scroll_to_selection();
    }

    /// Opens the perfect-stats list too. Normally only one list is open; the
    /// launch smoke test wants both drawn so the real binary runs all of the
    /// drawing code.
    pub(super) fn open_perfect_list_for_smoke_test(&mut self) {
        self.open = true;
        self.perfect_list_open = true;
    }

    /// True once after the options changed.
    pub(super) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// The row of the species list that shows the current species.
    fn selected_row(&self) -> usize {
        self.options.wild_species.map_or(0, usize::from)
    }

    fn scroll_to_selection(&mut self) {
        let selected = self.selected_row();
        let max = LIST_ENTRIES - LIST_VISIBLE_ROWS;
        self.list_scroll = selected.saturating_sub(LIST_VISIBLE_ROWS / 2).min(max);
    }

    fn set(&mut self, mut options: CheatOptions) {
        let old = self.options;
        // A choice made in a row switches the row on, so it is never left
        // doing nothing; unticking the row keeps the choice.
        if (options.wild_species != old.wild_species && options.wild_species.is_some())
            || (options.wild_level != old.wild_level && options.wild_level.is_some())
        {
            options.wild_enabled = true;
        }
        if options.perfect_target != old.perfect_target {
            options.perfect_enabled = true;
        }
        if options != old {
            self.options = options;
            self.changed = true;
        }
    }

    /// Level after a step of `delta`: an empty level steps up to 1 and down
    /// from 1 to empty.
    fn stepped_level(level: Option<u8>, delta: i32) -> Option<u8> {
        match level {
            None if delta > 0 => Some(1),
            None => None,
            Some(level) => {
                let next = i32::from(level) + delta;
                if next < 1 {
                    None
                } else {
                    Some(next.min(i32::from(MAX_LEVEL)) as u8)
                }
            }
        }
    }

    /// Handles one frame of pointer input. `pressed` is true on the frame the
    /// left button went down. Returns true when the pointer is over the menu
    /// (so the dock beneath should not also react).
    pub(super) fn update(&mut self, pointer: Pointer, pressed: bool, layout: &MenuLayout) -> bool {
        if !self.open {
            self.held_frames = 0;
            return false;
        }
        let Some((x, y)) = pointer.position else {
            self.held_frames = 0;
            return false;
        };
        let over_panel = layout.panel.contains(x, y);
        let over_list = self.dropdown_open && layout.list.contains(x, y);

        if self.dropdown_open {
            return self.update_list(pointer, pressed, layout, (x, y), over_list || over_panel);
        }
        if self.perfect_list_open {
            return self.update_perfect_list(pressed, layout, (x, y), over_panel);
        }

        let mut options = self.options;
        if pressed && over_panel {
            if layout.shiny.inflated(4).contains(x, y) {
                options.shiny = !options.shiny;
            } else if layout
                .file_codes
                .is_some_and(|r| r.inflated(4).contains(x, y))
            {
                options.file_codes = !options.file_codes;
            } else if layout.wild_enable.inflated(4).contains(x, y) {
                options.wild_enabled = !options.wild_enabled;
            } else if layout.perfect_enable.inflated(4).contains(x, y) {
                options.perfect_enabled = !options.perfect_enabled;
            } else if layout.dropdown.contains(x, y) {
                self.dropdown_open = true;
                self.scroll_to_selection();
            } else if layout.level_minus.contains(x, y) {
                options.wild_level = Self::stepped_level(options.wild_level, -1);
                self.held_frames = 0;
            } else if layout.level_plus.contains(x, y) {
                options.wild_level = Self::stepped_level(options.wild_level, 1);
                self.held_frames = 0;
            } else if layout.level_clear.contains(x, y) {
                options.wild_level = None;
            } else if layout.perfect_dropdown.contains(x, y) {
                self.perfect_list_open = true;
            } else if layout.money.inflated(4).contains(x, y) {
                options.infinite_money = !options.infinite_money;
            } else if layout.catch.inflated(4).contains(x, y) {
                options.catch_always = !options.catch_always;
            } else if layout.exp_boost.inflated(4).contains(x, y) {
                options.exp_boost = !options.exp_boost;
            }
        }

        // Holding a stepper button repeats it after a short delay.
        let held_delta = if pointer.left_down {
            if layout.level_minus.contains(x, y) {
                Some(-1)
            } else if layout.level_plus.contains(x, y) {
                Some(1)
            } else {
                None
            }
        } else {
            None
        };
        match held_delta {
            Some(delta) if !pressed => {
                self.held_frames += 1;
                if self.held_frames >= REPEAT_DELAY_FRAMES
                    && (self.held_frames - REPEAT_DELAY_FRAMES)
                        .is_multiple_of(REPEAT_INTERVAL_FRAMES)
                {
                    options.wild_level = Self::stepped_level(options.wild_level, delta);
                }
            }
            Some(_) => {}
            None => self.held_frames = 0,
        }

        // The wheel over the level controls steps by one.
        if pointer.scroll != 0.0
            && (layout.level_value.contains(x, y)
                || layout.level_minus.contains(x, y)
                || layout.level_plus.contains(x, y))
        {
            let delta = if pointer.scroll > 0.0 { 1 } else { -1 };
            options.wild_level = Self::stepped_level(options.wild_level, delta);
        }

        self.set(options);
        over_panel
    }

    /// The open perfect-stats list: a click on a row chooses it; any click
    /// closes the list and is used up.
    fn update_perfect_list(
        &mut self,
        pressed: bool,
        layout: &MenuLayout,
        (x, y): (f32, f32),
        over_panel: bool,
    ) -> bool {
        if pressed {
            for (index, target) in PerfectTarget::ALL.into_iter().enumerate() {
                if layout.perfect_row(index).contains(x, y) {
                    let mut options = self.options;
                    options.perfect_target = target;
                    self.set(options);
                    break;
                }
            }
            self.perfect_list_open = false;
            return true;
        }
        over_panel
    }

    fn update_list(
        &mut self,
        pointer: Pointer,
        pressed: bool,
        layout: &MenuLayout,
        (x, y): (f32, f32),
        consumed: bool,
    ) -> bool {
        let max_scroll = LIST_ENTRIES - LIST_VISIBLE_ROWS;
        if pointer.scroll != 0.0 && (layout.list.contains(x, y) || layout.dropdown.contains(x, y)) {
            if pointer.scroll > 0.0 {
                self.list_scroll = self.list_scroll.saturating_sub(WHEEL_ROWS);
            } else {
                self.list_scroll = (self.list_scroll + WHEEL_ROWS).min(max_scroll);
            }
        }
        // The right edge of the list is a scrollbar: pressing or dragging
        // there jumps to the matching position without choosing anything.
        let scrollbar = Rect {
            x: layout.list.x + layout.list.width - SCROLLBAR_WIDTH,
            width: SCROLLBAR_WIDTH,
            ..layout.list
        };
        if pointer.left_down && scrollbar.contains(x, y) {
            // The last pixel row maps to the end of the list.
            let span = (layout.list.height - 1).max(1) as f32;
            let fraction = ((y - layout.list.y as f32) / span).clamp(0.0, 1.0);
            self.list_scroll = (fraction * max_scroll as f32).round() as usize;
            return true;
        }
        if pressed {
            if layout.list.contains(x, y) {
                let row = ((y as i32 - layout.list.y) / LIST_ROW_HEIGHT) as usize;
                let index = self.list_scroll + row.min(LIST_VISIBLE_ROWS - 1);
                if index < LIST_ENTRIES {
                    let mut options = self.options;
                    options.wild_species = if index == 0 { None } else { Some(index as u16) };
                    self.set(options);
                }
            }
            // Any click closes the list: on a row it chose, elsewhere it
            // simply dismisses. Either way the click is used up, so it does
            // not also close the menu or press a button underneath.
            self.dropdown_open = false;
            return true;
        }
        consumed
    }
}

#[cfg(test)]
mod tests;
