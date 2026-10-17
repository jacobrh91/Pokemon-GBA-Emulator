//! Cheats, in three kinds that work by different mechanisms:
//!
//! * [`codebreaker`]: raw CodeBreaker codes from a `.cheats` file. They know
//!   nothing about the game; they write constants into RAM.
//! * [`gen3`]: built-in cheats for Pokémon FireRed, LeafGreen and Emerald. They find the
//!   game's own code by its bytes and change what the CPU sees there.
//! * [`options`]: the cheat menu's state, which drives the built-in kind.
//!
//! [`scan`] remembers where the built-in cheats hook in a ROM, and
//! [`species`] holds the Pokémon names.

pub mod codebreaker;
pub mod gen3;
pub mod options;
pub mod scan;
pub mod species;

pub use codebreaker::Cheats;
