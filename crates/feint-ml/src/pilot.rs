use std::marker::PhantomData;

use feint_ai::{
    Anticipation, BotError, Cadence, Controller, MAX_DELAY_TICKS, Playbook, ReactionBuffer,
    clamp_to_space, delay_ticks,
};
use feint_protocol::{
    ActionT, ChaCha8Rng, DimT, GameAdapter, GameSpecT, ObservationT, PlayerId, TimeMode,
};
use rand_chacha::rand_core::SeedableRng;

pub const EXTRA_FEATURES: usize = 2;

pub trait Policy {
    fn act(&mut self, features: &[f32], out: &mut [f32]);
}

pub struct Pilot<A: GameAdapter, P: Playbook<A>, Q: Policy> {
    playbook: P,
    policy: Q,
    spec: GameSpecT,
    dims: Vec<DimT>,
    cadence: Cadence,
    buffer: ReactionBuffer,
    own: Anticipation,
    reaction: f32,
    physique: f32,
    me: PlayerId,
    rng: ChaCha8Rng,
    scratch: ObservationT,
    features: Vec<f32>,
    out: Vec<f32>,
    sensed: Option<u64>,
    newest: Option<u64>,
    _game: PhantomData<fn() -> A>,
}

impl<A: GameAdapter, P: Playbook<A>, Q: Policy> Pilot<A, P, Q> {
    pub fn new(
        playbook: P,
        policy: Q,
        spec: &GameSpecT,
        reaction_ms: u32,
        physique: f64,
    ) -> Result<Self, BotError> {
        if spec.time_mode != TimeMode::Realtime {
            return Err(BotError::BadCadence);
        }
        let cadence =
            Cadence::new(spec.tick_hz, spec.decision_hz).map_err(|_| BotError::BadCadence)?;
        let delay = delay_ticks(reaction_ms, spec.tick_hz);
        if delay > MAX_DELAY_TICKS {
            return Err(BotError::BadReaction);
        }
        let dims = spec
            .action_space
            .as_deref()
            .and_then(|s| s.continuous.clone())
            .unwrap_or_default();
        Ok(Self {
            playbook,
            policy,
            spec: spec.clone(),
            out: vec![0.0; dims.len()],
            dims,
            cadence,
            buffer: ReactionBuffer::new(delay),
            own: Anticipation::new(delay as u64),
            reaction: reaction_ms as f32 / 1000.0,
            physique: physique as f32,
            me: 0,
            rng: ChaCha8Rng::seed_from_u64(0),
            scratch: ObservationT::default(),
            features: Vec::new(),
            sensed: None,
            newest: None,
            _game: PhantomData,
        })
    }

    #[must_use]
    pub fn policy(&self) -> &Q {
        &self.policy
    }

    pub fn policy_mut(&mut self) -> &mut Q {
        &mut self.policy
    }

    #[must_use]
    pub fn features(&self) -> &[f32] {
        &self.features
    }

    fn forget(&mut self) {
        self.buffer.clear();
        self.own.clear();
        self.features.clear();
        self.sensed = None;
        self.newest = None;
    }

    pub fn sense(&mut self, adapter: &A, obs: &ObservationT) -> Option<&[f32]> {
        let now = obs.tick;
        if self.sensed == Some(now) {
            return Some(&self.features);
        }
        if self.newest.is_some_and(|newest| now < newest) {
            self.forget();
        }
        self.newest = Some(now);
        self.buffer.push(obs);
        if !self.cadence.is_decision(now) {
            return None;
        }
        let delayed = self.buffer.delayed(now)?;
        let from = delayed.tick;
        let state = adapter.state_from_observation(delayed, &mut self.rng);
        let them = if self.me == 0 { 1 } else { 0 };
        let state = self
            .own
            .roll_forward(adapter, &self.playbook, them, state, from, now);
        adapter.observe(&state, self.me, &mut self.scratch);
        self.features.clear();
        self.features
            .extend(self.scratch.state.as_deref().unwrap_or_default());
        self.features.extend([self.reaction, self.physique]);
        self.sensed = Some(now);
        Some(&self.features)
    }

    fn values(&self) -> Vec<f32> {
        let mut values: Vec<f32> = self
            .out
            .iter()
            .zip(&self.dims)
            .map(|(&out, dim)| {
                let unit = if out.is_nan() {
                    0.0
                } else {
                    out.clamp(-1.0, 1.0)
                };
                let half = (dim.max - dim.min) / 2.0;
                let mid = (dim.max + dim.min) / 2.0;
                mid + half * unit
            })
            .collect();
        clamp_to_space(&mut values, &self.spec);
        values
    }
}

impl<A: GameAdapter, P: Playbook<A>, Q: Policy> Controller<A> for Pilot<A, P, Q> {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        let stream = (u64::from(me) + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ stream);
        self.forget();
    }

    fn on_tick(&mut self, adapter: &A, obs: &ObservationT) -> Option<ActionT> {
        let now = obs.tick;
        self.sense(adapter, obs)?;
        self.policy.act(&self.features, &mut self.out);
        let mut action = ActionT::default();
        action.tick = now;
        action.player = self.me;
        action.values = Some(self.values());
        self.own.remember(&action, now);
        Some(action)
    }
}
