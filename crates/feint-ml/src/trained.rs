use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{BotError, Difficulty};
use feint_protocol::GameSpecT;

use crate::Pilot;
use crate::mlp::{Mlp, MlpError};
use crate::registry;

pub type TrainedPilot = Pilot<AirHockey, HockeyPlaybook, Mlp>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainedError {
    Level,
    Checkpoint,
    Model(MlpError),
    Bot(BotError),
}

pub fn trained_pilot(
    level: u8,
    checkpoint: u64,
    spec: &GameSpecT,
) -> Result<TrainedPilot, TrainedError> {
    let difficulty = Difficulty::preset(level).ok_or(TrainedError::Level)?;
    let file = registry::lookup(checkpoint).ok_or(TrainedError::Checkpoint)?;
    let mlp = Mlp::from_safetensors(file.bytes).map_err(TrainedError::Model)?;
    Pilot::new(
        HockeyPlaybook::new(Params::default()),
        mlp,
        spec,
        difficulty.reaction_ms,
        difficulty.physique,
    )
    .map_err(TrainedError::Bot)
}
