//! Cheats, in three kinds that work by different mechanisms:
//!
//! * [`codebreaker`]: raw CodeBreaker codes from a `.cheats` file beside the
//!   ROM. They know nothing about the game; they write constants into RAM
//!   (once per frame, or at a ROM address named by a master code).
//! * [`firered`]: built-in, game-specific cheats for Pokémon FireRed. They
//!   find the game's own code in the ROM by its bytes and change what the CPU
//!   sees there (a register at a function entry, a value in the middle of a
//!   routine), so they work in every build whose code matches.
//! * [`options`]: the state of the cheat menu (which built-in cheats are on,
//!   and their settings), which is what drives the built-in kind.
//!
//! [`species`] holds the Pokémon names shared by the menu, the settings file
//! and the cheats file.

pub mod codebreaker;
pub mod firered;
pub mod options;
pub mod species;

pub use codebreaker::Cheats;
