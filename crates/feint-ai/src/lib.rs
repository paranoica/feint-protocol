//! The Feint core: one for every game. It knows games only through `GameAdapter` and a
//! game's `Playbook` (FNT-D7).

pub mod bot;
pub mod difficulty;
pub mod reaction;
pub mod runner;

pub use bot::{
    Bot, BotError, Choice, Controller, OPPONENT_REACTION_MS, Playbook, REPLAN_HZ, SIMULATION_BUDGET,
};
pub use difficulty::{Difficulty, LongGame, Memory};
pub use reaction::{BadCadence, Cadence, ReactionBuffer, delay_ticks};
pub use runner::{RunHeader, RunReport, replay, run_match};
