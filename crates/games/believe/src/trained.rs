use feint_turn::{PolicyError, TurnPolicy, turn_layout};

use crate::observation_len;
use crate::rules::{Moves, Params};

#[derive(Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub name: &'static str,
    pub id: u64,
    pub bytes: &'static [u8],
}

pub const CHECKPOINTS: &[Checkpoint] = &[Checkpoint {
    name: "believe-smoke-00146",
    id: 0xc78b_bff6_678f_5a22,
    bytes: include_bytes!(concat!(env!("OUT_DIR"), "/believe-smoke-00146.safetensors")),
}];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardCheckpointError {
    Unknown,
    Policy(PolicyError),
}

impl std::fmt::Display for CardCheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "no card checkpoint has this id"),
            Self::Policy(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CardCheckpointError {}

#[must_use]
pub fn lookup(id: u64) -> Option<&'static Checkpoint> {
    CHECKPOINTS.iter().find(|c| c.id == id)
}

#[must_use]
pub fn layout() -> String {
    let params = Params::default();
    turn_layout(
        "believe",
        observation_len(&params),
        Moves::new(&params).len(),
    )
}

pub fn policy(id: u64) -> Result<TurnPolicy, CardCheckpointError> {
    let checkpoint = lookup(id).ok_or(CardCheckpointError::Unknown)?;
    TurnPolicy::from_safetensors(checkpoint.bytes, &layout()).map_err(CardCheckpointError::Policy)
}
