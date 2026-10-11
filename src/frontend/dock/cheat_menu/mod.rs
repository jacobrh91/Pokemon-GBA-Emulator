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
//! | no random encounters | |
//! | TMs are infinitely usable | none: always on, shown ticked |
//!
//! Choosing a species or level, or a perfect-stats target, also
//! switches its row on, so a choice never silently does nothing; unticking
//! the row's checkbox keeps the choices for later.
//!
//! From the keyboard, while the menu is open (the game gets no buttons then):
//! up and down move between the rows; on the wild row left and right move
//! between its checkbox, species field and level buttons, and A uses the one in
//! focus (on the species field it opens the list: up and down move through the
//! Pokémon, A chooses, B leaves without choosing); on the perfect-stats row
//! left and right cycle the target; A switches the other rows; B closes the
//! menu.
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
use std::time::{Duration, Instant};

/// A keyboard action on the menu (the movement keys, A and B).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuNav {
    /// Move to the row above / below (wrapping); in an open species list,
    /// move the cursor.
    Up,
    Down,
    /// On the wild row, move between its checkbox, species field and level
    /// buttons; on the perfect-stats row, cycle the target.
    Left,
    Right,
    /// The A button: use the focused part (switch a row, open the species
    /// list, press a level button), or choose the species under the cursor.
    Toggle,
    /// The B button: leave an open list without choosing, otherwise close the
    /// menu.
    Back,
}

/// The parts of the wild row the keyboard moves between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WildPart {
    Enable,
    Species,
    LevelDown,
    LevelUp,
    /// The X that empties the level.
    LevelClear,
}

impl WildPart {
    const ALL: [WildPart; 5] = [
        Self::Enable,
        Self::Species,
        Self::LevelDown,
        Self::LevelUp,
        Self::LevelClear,
    ];

    fn stepped(self, delta: i32) -> Self {
        let at = Self::ALL.iter().position(|part| *part == self).unwrap_or(0) as i32;
        Self::ALL[(at + delta).clamp(0, Self::ALL.len() as i32 - 1) as usize]
    }
}

/// The rows the keyboard can reach, top to bottom (the always-on row is only
/// shown).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FocusRow {
    FileCodes,
    Wild,
    Perfect,
    Money,
    Catch,
    Experience,
    Shiny,
    NoEncounters,
}

/// Species-list rows: "ANY" followed by the 251 species.
pub(super) const LIST_ENTRIES: usize = LAST_SPECIES as usize + 1;
const LIST_VISIBLE_ROWS: usize = 8;
/// Rows the wheel moves the species list per notch.
const WHEEL_ROWS: usize = 3;
/// Width of the strip at the list's right edge that works as a scrollbar.
const SCROLLBAR_WIDTH: i32 = 22;
const LIST_ROW_HEIGHT: i32 = 28;
/// How long a stepper button must be held before it starts repeating, the
/// interval between repeats, and the faster interval after
/// `FAST_AFTER_REPEATS` repeats. These are wall-clock times, so holding feels
/// the same whatever rate the window is drawn at.
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_INTERVAL: Duration = Duration::from_millis(70);
const FAST_REPEAT_INTERVAL: Duration = Duration::from_millis(30);
const FAST_AFTER_REPEATS: u32 = 10;

/// A stepper button being held.
#[derive(Clone, Copy, Debug)]
struct Hold {
    /// The step direction of the button (-1 or +1).
    delta: i32,
    since: Instant,
    last_step: Instant,
    repeats: u32,
}

impl Hold {
    fn new(delta: i32, now: Instant) -> Self {
        Self {
            delta,
            since: now,
            last_step: now,
            repeats: 0,
        }
    }

    /// Whether another step is due at `now`: after the delay, then at the
    /// repeat interval, which shortens the longer the button stays down.
    fn step_due(&mut self, now: Instant) -> bool {
        let interval = if self.repeats >= FAST_AFTER_REPEATS {
            FAST_REPEAT_INTERVAL
        } else {
            REPEAT_INTERVAL
        };
        let due = now.duration_since(self.since) >= REPEAT_DELAY
            && now.duration_since(self.last_step) >= interval;
        if due {
            self.last_step = now;
            self.repeats += 1;
        }
        due
    }
}

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
    pub(super) shiny: Rect,
    pub(super) no_encounters: Rect,
    /// Last in the list: a cheat that is always on, so its box is only shown.
    pub(super) always_on: Rect,
    /// The open species list, below the dropdown.
    pub(super) list: Rect,
}

impl MenuLayout {
    /// `dock` is the dock's rectangle; the panel sits just above it, aligned
    /// to its right edge (above the CHEATS button) and kept inside the window.
    pub(super) fn new(width: usize, dock: Rect, file_row: bool) -> Self {
        // The optional file row pushes every other row down by one.
        let extra = if file_row { 44 } else { 0 };
        let panel_height = 432 + extra;
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
        let row_no_encounters = y + 346 + extra;
        let row_always_on = y + 390 + extra;

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
            no_encounters: checkbox_at(left, row_no_encounters),
            always_on: checkbox_at(left, row_always_on),
            list: Rect {
                x: dropdown.x,
                y: dropdown.y + dropdown.height + 4,
                width: dropdown.width,
                height: LIST_VISIBLE_ROWS as i32 * LIST_ROW_HEIGHT,
            },
        }
    }

    /// The checkbox of a keyboard-reachable row.
    pub(super) fn checkbox_of(&self, row: FocusRow) -> Rect {
        match row {
            FocusRow::FileCodes => self.file_codes.unwrap_or(self.wild_enable),
            FocusRow::Wild => self.wild_enable,
            FocusRow::Perfect => self.perfect_enable,
            FocusRow::Money => self.money,
            FocusRow::Catch => self.catch,
            FocusRow::Experience => self.exp_boost,
            FocusRow::Shiny => self.shiny,
            FocusRow::NoEncounters => self.no_encounters,
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
    /// The stepper button being held with the mouse, for auto-repeat.
    hold: Option<Hold>,
    /// The same for the A key held on a stepper button.
    key_hold: Option<Hold>,
    changed: bool,
    /// Number of code lines in the cheats file; the file row shows when
    /// this is not zero.
    file_codes_available: usize,
    /// The row the keyboard acts on, and whether it is drawn (it is once the
    /// keyboard has been used since the menu opened).
    focus: usize,
    focus_visible: bool,
    /// The part of the wild row that has the focus.
    wild_part: WildPart,
    /// The species-list row under the keyboard's cursor (0 is "ANY") while the
    /// list is driven from the keyboard.
    list_cursor: Option<usize>,
}

impl CheatMenu {
    pub(super) fn new() -> Self {
        Self {
            open: false,
            options: CheatOptions::default(),
            dropdown_open: false,
            perfect_list_open: false,
            list_scroll: 0,
            hold: None,
            key_hold: None,
            changed: false,
            file_codes_available: 0,
            focus: 0,
            focus_visible: false,
            wild_part: WildPart::Enable,
            list_cursor: None,
        }
    }

    /// The rows the keyboard reaches, top to bottom.
    fn focus_rows(&self) -> Vec<FocusRow> {
        let mut rows = Vec::with_capacity(8);
        if self.has_file_row() {
            rows.push(FocusRow::FileCodes);
        }
        rows.extend([
            FocusRow::Wild,
            FocusRow::Perfect,
            FocusRow::Money,
            FocusRow::Catch,
            FocusRow::Experience,
            FocusRow::Shiny,
            FocusRow::NoEncounters,
        ]);
        rows
    }

    /// The row that has the keyboard focus, if it is to be drawn.
    pub(super) fn focused_row(&self) -> Option<FocusRow> {
        let rows = self.focus_rows();
        self.focus_visible
            .then(|| rows[self.focus.min(rows.len() - 1)])
    }

    /// The part of the wild row that has the focus.
    pub(super) fn wild_part(&self) -> WildPart {
        self.wild_part
    }

    /// The list row under the keyboard's cursor, while it drives the list.
    pub(super) fn list_cursor(&self) -> Option<usize> {
        self.list_cursor.filter(|_| self.dropdown_open)
    }

    /// Moves the keyboard's list cursor by `delta` rows, wrapping, and keeps
    /// it on screen.
    fn move_list_cursor(&mut self, delta: i32) {
        let at = self.list_cursor.unwrap_or_else(|| self.selected_row()) as i32;
        let next = (at + delta).rem_euclid(LIST_ENTRIES as i32) as usize;
        self.list_cursor = Some(next);
        if next < self.list_scroll {
            self.list_scroll = next;
        } else if next >= self.list_scroll + LIST_VISIBLE_ROWS {
            self.list_scroll = next + 1 - LIST_VISIBLE_ROWS;
        }
    }

    /// Acts on a keyboard press.
    pub(super) fn navigate(&mut self, nav: MenuNav) {
        if !self.open {
            return;
        }
        // B leaves a list without choosing; with none open it closes the menu.
        if nav == MenuNav::Back {
            if self.dropdown_open || self.perfect_list_open {
                self.dropdown_open = false;
                self.perfect_list_open = false;
                self.list_cursor = None;
            } else {
                self.close();
            }
            return;
        }
        // The species list: up and down move the cursor, A chooses.
        if self.dropdown_open {
            match nav {
                MenuNav::Up => self.move_list_cursor(-1),
                MenuNav::Down => self.move_list_cursor(1),
                MenuNav::Toggle => {
                    let index = self.list_cursor.unwrap_or_else(|| self.selected_row());
                    let mut options = self.options;
                    options.wild_species = (index != 0).then_some(index as u16);
                    self.set(options);
                    self.dropdown_open = false;
                    self.list_cursor = None;
                }
                _ => {}
            }
            return;
        }
        // The target list was opened with the mouse; the keys dismiss it.
        self.perfect_list_open = false;
        // The first key only shows where the focus is (the top row), so a
        // press is never spent on a row that was not on show.
        if !self.focus_visible {
            self.focus_visible = true;
            self.focus = 0;
            return;
        }
        let rows = self.focus_rows();
        self.focus = self.focus.min(rows.len() - 1);
        let row = rows[self.focus];
        let mut options = self.options;
        match nav {
            MenuNav::Up => self.focus = (self.focus + rows.len() - 1) % rows.len(),
            MenuNav::Down => self.focus = (self.focus + 1) % rows.len(),
            MenuNav::Left | MenuNav::Right => {
                let delta = if nav == MenuNav::Left { -1 } else { 1 };
                match row {
                    FocusRow::Wild => self.wild_part = self.wild_part.stepped(delta),
                    FocusRow::Perfect => {
                        let all = PerfectTarget::ALL;
                        let at = all
                            .iter()
                            .position(|target| *target == options.perfect_target)
                            .unwrap_or(0);
                        let next = (at as i32 + delta).rem_euclid(all.len() as i32);
                        options.perfect_target = all[next as usize];
                    }
                    _ => {}
                }
            }
            MenuNav::Toggle => match row {
                FocusRow::FileCodes => options.file_codes = !options.file_codes,
                FocusRow::Wild => match self.wild_part {
                    WildPart::Enable => options.wild_enabled = !options.wild_enabled,
                    WildPart::Species => {
                        self.dropdown_open = true;
                        self.list_cursor = Some(self.selected_row());
                        self.scroll_to_selection();
                    }
                    WildPart::LevelDown => {
                        options.wild_level = Self::stepped_level(options.wild_level, -1);
                    }
                    WildPart::LevelUp => {
                        options.wild_level = Self::stepped_level(options.wild_level, 1);
                    }
                    WildPart::LevelClear => options.wild_level = None,
                },
                FocusRow::Perfect => options.perfect_enabled = !options.perfect_enabled,
                FocusRow::Money => options.infinite_money = !options.infinite_money,
                FocusRow::Catch => options.catch_always = !options.catch_always,
                FocusRow::Experience => options.exp_boost = !options.exp_boost,
                FocusRow::Shiny => options.shiny = !options.shiny,
                FocusRow::NoEncounters => options.no_encounters = !options.no_encounters,
            },
            MenuNav::Back => {}
        }
        self.set(options);
    }

    /// Called every frame with whether the A key (Space or Enter) is held:
    /// while it stays down on a level button, the step repeats like a held
    /// mouse button. The press itself is a [`MenuNav::Toggle`].
    pub(super) fn hold_toggle(&mut self, held: bool, now: Instant) {
        let delta = match (self.open, self.focused_row(), self.wild_part) {
            (true, Some(FocusRow::Wild), WildPart::LevelDown)
                if held && !self.dropdown_open && !self.perfect_list_open =>
            {
                -1
            }
            (true, Some(FocusRow::Wild), WildPart::LevelUp)
                if held && !self.dropdown_open && !self.perfect_list_open =>
            {
                1
            }
            _ => {
                self.key_hold = None;
                return;
            }
        };
        let mut hold = self
            .key_hold
            .filter(|hold| hold.delta == delta)
            .unwrap_or_else(|| Hold::new(delta, now));
        if hold.step_due(now) {
            let mut options = self.options;
            options.wild_level = Self::stepped_level(options.wild_level, delta);
            self.set(options);
        }
        self.key_hold = Some(hold);
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
        self.focus_visible = false;
        self.list_cursor = None;
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.dropdown_open = false;
        self.perfect_list_open = false;
        self.focus_visible = false;
        self.list_cursor = None;
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
        self.update_at(pointer, pressed, layout, Instant::now())
    }

    /// [`Self::update`] at a given time, which the auto-repeat of the stepper
    /// buttons depends on.
    pub(super) fn update_at(
        &mut self,
        pointer: Pointer,
        pressed: bool,
        layout: &MenuLayout,
        now: Instant,
    ) -> bool {
        if !self.open {
            self.hold = None;
            return false;
        }
        let Some((x, y)) = pointer.position else {
            self.hold = None;
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
            } else if layout.no_encounters.inflated(4).contains(x, y) {
                options.no_encounters = !options.no_encounters;
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
                self.hold = None;
            } else if layout.level_plus.contains(x, y) {
                options.wild_level = Self::stepped_level(options.wild_level, 1);
                self.hold = None;
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
            Some(delta) => {
                let mut hold = self
                    .hold
                    .filter(|hold| hold.delta == delta && !pressed)
                    .unwrap_or_else(|| Hold::new(delta, now));
                if hold.step_due(now) {
                    options.wild_level = Self::stepped_level(options.wild_level, delta);
                }
                self.hold = Some(hold);
            }
            None => self.hold = None,
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
