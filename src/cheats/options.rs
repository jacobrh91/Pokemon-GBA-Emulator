//! The options of the cheat menu: built-in, game-specific cheats that are
//! switched on and off while playing, as opposed to the raw codes of a
//! `.cheats` file (see [`super::codebreaker`]).
//!
//! Which options act on the game today is recorded in [`CheatOptions::is_live`];
//! the rest are stored, shown and persisted so the menu is complete, but do not
//! change the game yet.

/// Highest level the game allows.
pub const MAX_LEVEL: u8 = 100;

/// Whose Pokémon the perfect-stats cheat applies to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PerfectTarget {
    /// Wild Pokémon.
    #[default]
    Wild,
    /// Opponents' (trainers') Pokémon.
    Opponents,
    /// Both of the above.
    Both,
}

impl PerfectTarget {
    /// The choices in the order the menu lists them.
    pub const ALL: [PerfectTarget; 3] = [Self::Wild, Self::Opponents, Self::Both];

    /// The menu's text for the choice.
    pub fn label(self) -> &'static str {
        match self {
            Self::Wild => "WILD POKEMON",
            Self::Opponents => "OPPONENTS POKEMON",
            Self::Both => "BOTH",
        }
    }

    pub fn covers_wild(self) -> bool {
        matches!(self, Self::Wild | Self::Both)
    }

    pub fn covers_opponents(self) -> bool {
        matches!(self, Self::Opponents | Self::Both)
    }

    /// The word used for the choice in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Self::Wild => "wild",
            Self::Opponents => "opponents",
            Self::Both => "both",
        }
    }

    pub fn from_key(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|target| target.key().eq_ignore_ascii_case(text.trim()))
    }
}

/// Every row of the menu has its own switch (a checkbox at its left). The two
/// rows that carry settings of their own, wild encounters and perfect stats,
/// keep the switch (`wild_enabled`, `perfect_enabled`) apart from the settings
/// so that a row can be turned off without losing what was chosen in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CheatOptions {
    /// Every Pokémon is shiny (FireRed).
    pub shiny: bool,
    /// Switch of the wild-encounter row. While off, `wild_species` and
    /// `wild_level` are remembered but do nothing.
    pub wild_enabled: bool,
    /// Every wild encounter is this species (national dex number), or `None`
    /// to leave species alone.
    pub wild_species: Option<u16>,
    /// Every wild encounter is at this level (1..=100), or `None` to keep the
    /// level the game chose.
    pub wild_level: Option<u8>,
    /// Switch of the perfect-stats row. While off, the target below is
    /// remembered but does nothing.
    pub perfect_enabled: bool,
    /// Whose Pokémon get maximum IVs (FireRed).
    pub perfect_target: PerfectTarget,
    /// The player never runs out of money (FireRed).
    pub infinite_money: bool,
    /// Every thrown ball catches (FireRed).
    pub catch_always: bool,
    /// All experience gained is boosted: every Pokémon is treated as a traded
    /// one (FireRed).
    pub exp_boost: bool,
    /// The raw codes of the `.cheats` file are applied. Never saved: those
    /// codes patch the game's own routines and can crash it if they are on
    /// before it has loaded the routine they target, so they start off every
    /// session.
    pub file_codes: bool,
}

/// One entry of the menu, for asking which options are live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheatOption {
    Shiny,
    WildSpecies,
    WildLevel,
    PerfectStats,
    InfiniteMoney,
    CatchAlways,
    ExpBoost,
}

impl CheatOptions {
    /// True when the option changes the game today. Options that are not live
    /// can be switched in the menu and are kept, but have no effect yet.
    pub fn is_live(option: CheatOption) -> bool {
        matches!(
            option,
            CheatOption::WildSpecies
                | CheatOption::WildLevel
                | CheatOption::CatchAlways
                | CheatOption::Shiny
                | CheatOption::PerfectStats
                | CheatOption::InfiniteMoney
                | CheatOption::ExpBoost
        )
    }

    /// The wild-encounter species the game should be given, if the row is on.
    pub fn active_wild_species(&self) -> Option<u16> {
        self.wild_enabled.then_some(self.wild_species).flatten()
    }

    /// Whether perfect stats should apply to wild Pokémon (row on, target
    /// covers them).
    pub fn perfect_for_wild(&self) -> bool {
        self.perfect_enabled && self.perfect_target.covers_wild()
    }

    /// Whether perfect stats should apply to opponents' Pokémon.
    pub fn perfect_for_opponents(&self) -> bool {
        self.perfect_enabled && self.perfect_target.covers_opponents()
    }

    /// The wild-encounter level the game should be given, if the row is on.
    pub fn active_wild_level(&self) -> Option<u8> {
        self.wild_enabled.then_some(self.wild_level).flatten()
    }

    /// Clamps a level to the range the game accepts.
    pub fn clamp_level(level: u8) -> u8 {
        level.clamp(1, MAX_LEVEL)
    }

    /// True when no option is set.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_set_by_default() {
        assert!(CheatOptions::default().is_default());
        let one = CheatOptions {
            infinite_money: true,
            ..CheatOptions::default()
        };
        assert!(!one.is_default());
    }

    #[test]
    fn every_option_is_live() {
        use CheatOption::*;
        let live: Vec<_> = [
            Shiny,
            WildSpecies,
            WildLevel,
            PerfectStats,
            InfiniteMoney,
            CatchAlways,
            ExpBoost,
        ]
        .into_iter()
        .filter(|option| CheatOptions::is_live(*option))
        .collect();
        assert_eq!(
            live,
            [
                Shiny,
                WildSpecies,
                WildLevel,
                PerfectStats,
                InfiniteMoney,
                CatchAlways,
                ExpBoost
            ]
        );
    }

    #[test]
    fn a_row_that_is_switched_off_keeps_its_settings_but_does_nothing() {
        let mut options = CheatOptions {
            wild_species: Some(147),
            wild_level: Some(50),
            ..CheatOptions::default()
        };
        assert_eq!(options.active_wild_species(), None, "off by default");
        assert_eq!(options.active_wild_level(), None);
        options.wild_enabled = true;
        assert_eq!(options.active_wild_species(), Some(147));
        assert_eq!(options.active_wild_level(), Some(50));
        options.wild_enabled = false;
        assert_eq!(options.wild_species, Some(147), "the choice is kept");
        assert_eq!(options.active_wild_species(), None);
    }

    #[test]
    fn the_perfect_target_chooses_wild_opponents_or_both() {
        use PerfectTarget::*;
        assert_eq!(PerfectTarget::default(), Wild);
        assert_eq!(PerfectTarget::ALL, [Wild, Opponents, Both]);
        assert_eq!(
            PerfectTarget::ALL.map(PerfectTarget::label),
            ["WILD POKEMON", "OPPONENTS POKEMON", "BOTH"]
        );
        assert!(Wild.covers_wild() && !Wild.covers_opponents());
        assert!(!Opponents.covers_wild() && Opponents.covers_opponents());
        assert!(Both.covers_wild() && Both.covers_opponents());
        for target in PerfectTarget::ALL {
            assert_eq!(PerfectTarget::from_key(target.key()), Some(target));
        }
        assert_eq!(PerfectTarget::from_key(" BOTH "), Some(Both));
        assert_eq!(PerfectTarget::from_key("mine"), None);
    }

    #[test]
    fn a_perfect_row_that_is_off_applies_to_nobody() {
        let mut options = CheatOptions {
            perfect_target: PerfectTarget::Both,
            ..CheatOptions::default()
        };
        assert!(!options.perfect_for_wild() && !options.perfect_for_opponents());
        options.perfect_enabled = true;
        assert!(options.perfect_for_wild() && options.perfect_for_opponents());
        options.perfect_target = PerfectTarget::Opponents;
        assert!(!options.perfect_for_wild() && options.perfect_for_opponents());
    }

    #[test]
    fn levels_are_kept_in_range() {
        assert_eq!(CheatOptions::clamp_level(0), 1);
        assert_eq!(CheatOptions::clamp_level(55), 55);
        assert_eq!(CheatOptions::clamp_level(200), 100);
    }
}
