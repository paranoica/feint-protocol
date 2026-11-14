use feint_ai::Controller;
use feint_ai::search::Budget;
use feint_protocol::{ActionT, ChaCha8Rng, GameSpecT, ObservationT, PlayerId};
use feint_turn::TurnPilot;
use rand_chacha::rand_core::SeedableRng;

use crate::adapter::Believe;
use crate::search::{Cards, PolicyModel, PolicyPrior, choose};
use crate::trained::{CardCheckpointError, policy};

pub const POLICY_LEVEL: u8 = 5;
pub const LEVELS: [u8; 4] = [5, 6, 7, 8];
pub const TOP_K: usize = 8;
pub const EXPLORE: f64 = 1.5;
pub const ITERATIONS: u32 = 300;

const SEAT_MIX: u64 = 0x9E37_79B9_7F4A_7C15;
const SEARCH_SALT: u64 = 0x5345_4152_4348;

#[must_use]
pub fn depth(level: u8) -> Option<u32> {
    match level {
        POLICY_LEVEL => Some(0),
        6 => Some(1),
        7 => Some(2),
        8 => Some(3),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotError {
    Level,
    Checkpoint(CardCheckpointError),
}

pub struct SearchPilot {
    game: Cards,
    prior: PolicyPrior,
    model: PolicyModel,
    budget: Budget,
    me: PlayerId,
    rng: ChaCha8Rng,
}

impl SearchPilot {
    pub fn new(checkpoint: u64, depth: u32) -> Result<Self, CardCheckpointError> {
        Ok(Self {
            game: Cards::default(),
            prior: PolicyPrior::new(policy(checkpoint)?),
            model: PolicyModel::new(policy(checkpoint)?),
            budget: Budget {
                iterations: ITERATIONS,
                depth,
                top_k: TOP_K,
                explore: EXPLORE,
            },
            me: 0,
            rng: ChaCha8Rng::seed_from_u64(0),
        })
    }
}

impl Controller<Believe> for SearchPilot {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        let stream = seed ^ (u64::from(me) + 1).wrapping_mul(SEAT_MIX) ^ SEARCH_SALT;
        self.rng = ChaCha8Rng::seed_from_u64(stream);
    }

    fn on_tick(&mut self, _game: &Believe, obs: &ObservationT) -> Option<ActionT> {
        if PlayerId::try_from(obs.to_act).ok() != Some(self.me) {
            return None;
        }
        if !obs.legal.as_deref().unwrap_or_default().contains(&1) {
            return None;
        }
        let m = choose(
            &self.game,
            obs,
            &self.model,
            &mut self.prior,
            self.budget,
            &mut self.rng,
        )?;
        let mut action = ActionT::default();
        action.tick = obs.tick;
        action.player = self.me;
        action.discrete = self.game.moves().index(&m)?;
        Some(action)
    }
}

pub fn for_level(level: u8, checkpoint: u64) -> Result<Box<dyn Controller<Believe>>, BotError> {
    match depth(level) {
        Some(0) => Ok(Box::new(TurnPilot::new(
            policy(checkpoint).map_err(BotError::Checkpoint)?,
        ))),
        Some(d) => Ok(Box::new(
            SearchPilot::new(checkpoint, d).map_err(BotError::Checkpoint)?,
        )),
        None => Err(BotError::Level),
    }
}
