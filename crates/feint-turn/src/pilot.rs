use feint_ai::Controller;
use feint_protocol::{ActionT, ChaCha8Rng, GameAdapter, GameSpecT, ObservationT, PlayerId};
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::policy::{TurnPolicy, sample_index};

const SEAT_MIX: u64 = 0x9E37_79B9_7F4A_7C15;

pub fn unit_draw(rng: &mut ChaCha8Rng) -> f32 {
    (rng.next_u32() >> 8) as f32 / 16_777_216.0
}

pub struct TurnPilot {
    policy: TurnPolicy,
    me: PlayerId,
    seed: u64,
    rng: ChaCha8Rng,
    probabilities: Vec<f32>,
}

impl TurnPilot {
    #[must_use]
    pub fn new(policy: TurnPolicy) -> Self {
        Self {
            policy,
            me: 0,
            seed: 0,
            rng: ChaCha8Rng::seed_from_u64(0),
            probabilities: Vec::new(),
        }
    }

    #[must_use]
    pub fn stream_seed(&self) -> u64 {
        self.seed
    }
}

impl<A: GameAdapter> Controller<A> for TurnPilot {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.seed = seed ^ (u64::from(me) + 1).wrapping_mul(SEAT_MIX);
        self.rng = ChaCha8Rng::seed_from_u64(self.seed);
    }

    fn on_tick(&mut self, _adapter: &A, obs: &ObservationT) -> Option<ActionT> {
        if PlayerId::try_from(obs.to_act).ok() != Some(self.me) {
            return None;
        }
        let legal = obs.legal.as_deref().unwrap_or_default();
        if !legal.contains(&1) {
            return None;
        }
        let features = obs.state.as_deref().unwrap_or_default();
        self.policy
            .probabilities(features, legal, &mut self.probabilities);
        let u = unit_draw(&mut self.rng);
        let index = sample_index(&self.probabilities, legal, u)?;
        let mut action = ActionT::default();
        action.tick = obs.tick;
        action.player = self.me;
        action.discrete = i32::try_from(index).ok()?;
        Some(action)
    }
}
