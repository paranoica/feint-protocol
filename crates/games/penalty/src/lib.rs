mod adapter;
pub mod context;
pub mod human;
pub mod persons;
mod playbook;
mod rules;
pub mod styles;
pub mod trained;

pub use adapter::{GAME_VERSION, OBSERVATION_LEN, Penalty, apply_to};
pub use playbook::PenaltyPlaybook;
pub use rules::{Body, Dive, KickEvent, Params, Phase, Shootout};
