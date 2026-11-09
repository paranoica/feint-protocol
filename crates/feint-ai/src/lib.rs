//! The Feint core: one for every game. It knows games only through `GameAdapter` and a
//! game's `Playbook` (FNT-D7).

pub mod difficulty;
pub mod reaction;

pub use difficulty::{Difficulty, LongGame, Memory};
pub use reaction::{BadCadence, Cadence, ReactionBuffer, delay_ticks};
