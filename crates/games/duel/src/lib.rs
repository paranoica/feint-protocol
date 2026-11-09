mod adapter;
mod playbook;
mod rules;

pub use adapter::{GAME_VERSION, OBSERVATION_LEN, QuickDraw, apply_to};
pub use playbook::DuelPlaybook;
pub use rules::{Body, Bullet, Duel, Gunner, Params, Phase, Roll, RoundEvent};
