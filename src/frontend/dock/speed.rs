//! The selectable fast-forward speeds.

/// How fast the fast-forward key runs the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpeedSetting {
    X2,
    X4,
    X8,
    /// As fast as the machine allows.
    #[default]
    Max,
}

impl SpeedSetting {
    pub const ALL: [SpeedSetting; 4] = [Self::X2, Self::X4, Self::X8, Self::Max];

    pub fn label(self) -> &'static str {
        match self {
            Self::X2 => "2X",
            Self::X4 => "4X",
            Self::X8 => "8X",
            Self::Max => "MAX",
        }
    }

    /// The setting for a saved target multiple (`None` is unthrottled);
    /// anything unrecognised means [`SpeedSetting::Max`].
    pub fn from_multiple(multiple: Option<u32>) -> Self {
        match multiple {
            Some(2) => Self::X2,
            Some(4) => Self::X4,
            Some(8) => Self::X8,
            _ => Self::Max,
        }
    }

    /// The target multiple of real time, or `None` for unthrottled.
    pub fn multiple(self) -> Option<u32> {
        match self {
            Self::X2 => Some(2),
            Self::X4 => Some(4),
            Self::X8 => Some(8),
            Self::Max => None,
        }
    }
}
