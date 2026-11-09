mod adapter;
mod playbook;
mod rules;

pub use adapter::{GAME_VERSION, OBSERVATION_LEN, Penalty, apply_to};
pub use playbook::PenaltyPlaybook;
pub use rules::{Body, Dive, KickEvent, Params, Phase, Shootout};
