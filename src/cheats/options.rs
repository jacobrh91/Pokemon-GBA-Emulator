//! The options of the cheat menu: built-in cheats switched on and off while
//! playing, as opposed to the raw codes of a `.cheats` file (see
//! [`super::codebreaker`]).

/// Highest level the game allows.
pub const MAX_LEVEL: u8 = 100;

/// How often the shiny cheat applies: one in N, from a ladder of 1 to 1000.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShinyOdds(u8);

impl ShinyOdds {
    /// The values of N, from the most to the least often.
    pub const ONE_IN: [u32; 9] = [1, 2, 5, 10, 25, 50, 100, 500, 1000];

    /// N, as in "one in N".
    pub fn one_in(self) -> u32 {
        Self::ONE_IN[usize::from(self.0).min(Self::ONE_IN.len() - 1)]
    }

    /// The menu's text: `1/10`.
    pub fn label(self) -> String {
        format!("1/{}", self.one_in())
    }

    /// The step `delta` along the ladder, stopping at its ends. A positive
    /// `delta` makes shiny Pokémon more common.
    pub fn stepped(self, delta: i32) -> Self {
        let last = Self::ONE_IN.len() as i32 - 1;
        Self((i32::from(self.0) - delta).clamp(0, last) as u8)
    }

    /// The setting for exactly one in `one_in`, if it is on the ladder.
    pub fn from_one_in(one_in: u32) -> Option<Self> {
        Self::ONE_IN
            .iter()
            .position(|n| *n == one_in)
            .map(|index| Self(index as u8))
    }
}

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

/// Every row has its own switch. Rows with settings of their own (wild
/// encounters, perfect stats, shiny) keep the switch apart from the settings,
/// so a row can be turned off without losing its choices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CheatOptions {
    /// Pokémon are made shiny (FireRed / LeafGreen / Emerald), at the chance `shiny_odds`.
    pub shiny: bool,
    /// How often. Remembered while the row is off.
    pub shiny_odds: ShinyOdds,
    /// Switch of the wild-encounter row; the choices below are kept while off.
    pub wild_enabled: bool,
    /// Species of every wild encounter (national dex number), or `None`.
    pub wild_species: Option<u16>,
    /// Level of every wild encounter (1..=100), or `None` for the game's.
    pub wild_level: Option<u8>,
    /// Switch of the perfect-stats row; the target is kept while off.
    pub perfect_enabled: bool,
    /// Whose Pokémon get maximum IVs (FireRed / LeafGreen / Emerald).
    pub perfect_target: PerfectTarget,
    /// The player never runs out of money (FireRed / LeafGreen / Emerald).
    pub infinite_money: bool,
    /// Walking never starts a wild encounter (FireRed / LeafGreen / Emerald).
    pub no_encounters: bool,
    /// Every thrown ball catches (FireRed / LeafGreen / Emerald).
    pub catch_always: bool,
    /// All experience gained is boosted: every Pokémon is treated as a traded
    /// one (FireRed / LeafGreen / Emerald).
    pub exp_boost: bool,
    /// The emulator runs flat out while a battle is on (Emerald; FireRed and
    /// LeafGreen in English).
    pub fast_battles: bool,
    /// The `.cheats` file's codes are applied. Never saved: they can crash
    /// the game if on before it has loaded what they patch.
    pub file_codes: bool,
}

impl CheatOptions {
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
    fn shiny_odds_step_along_the_ladder_and_stop_at_its_ends() {
        let all = ShinyOdds::default();
        assert_eq!(all.one_in(), 1, "every Pokémon by default");
        assert_eq!(all.label(), "1/1");
        assert_eq!(all.stepped(1), all, "already the most often");
        let mut odds = all;
        let mut seen = vec![odds.one_in()];
        for _ in 0..20 {
            odds = odds.stepped(-1);
            if seen.last() != Some(&odds.one_in()) {
                seen.push(odds.one_in());
            }
        }
        assert_eq!(seen, ShinyOdds::ONE_IN);
        assert_eq!(odds.label(), "1/1000", "stops at the rarest");
        assert_eq!(odds.stepped(1).one_in(), 500, "up is more often");
        assert_eq!(
            ShinyOdds::from_one_in(100).map(ShinyOdds::one_in),
            Some(100)
        );
        assert_eq!(ShinyOdds::from_one_in(7), None);
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
