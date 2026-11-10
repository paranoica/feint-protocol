use std::collections::VecDeque;
use std::marker::PhantomData;

use feint_protocol::{
    ActionT, ChaCha8Rng, GameAdapter, GameSpecT, ObservationT, PlayerId, TimeMode,
};
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::difficulty::Difficulty;
use crate::insight::{Insight, Seen};
use crate::long_game::Plan;
use crate::model::{OpponentModel, Profile, ProfileError};
use crate::playbook::{Choice, Controller, Playbook};
use crate::reaction::{Cadence, ReactionBuffer, delay_ticks};

mod plan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotError {
    BadCadence,
    BadReaction,
}

pub const SIMULATION_BUDGET: u32 = 4_800;

pub const MAX_DELAY_TICKS: usize = 1 << 16;

pub const REPLAN_HZ: u32 = 10;

pub const OPPONENT_REACTION_MS: u32 = 250;

#[derive(Clone, Copy, Debug)]
struct Committed {
    choice: Choice,
    since: u64,
    aim: [f64; 2],
    signal: Option<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InsightView {
    pub current: Option<Choice>,
    pub intent: Option<(u8, bool)>,
    pub memory: bool,
    pub likely_shot: Option<u8>,
    pub likely_cover: Vec<Option<u8>>,
}

pub struct Bot<A: GameAdapter, P: Playbook<A>> {
    difficulty: Difficulty,
    playbook: P,
    spec: GameSpecT,
    cadence: Option<Cadence>,
    delay: usize,
    stride: u32,
    reply_every: u32,
    replan_every: u64,
    buffer: ReactionBuffer,
    seen: ObservationT,
    rng: ChaCha8Rng,
    me: PlayerId,
    current: Option<Committed>,
    last_replan: Option<u64>,
    newest: Option<u64>,
    observed: Option<u64>,
    history: VecDeque<ActionT>,
    options: Vec<Choice>,
    insight: Insight<A::State>,
    plan: Plan,
    _game: PhantomData<fn() -> A>,
}

impl<A: GameAdapter, P: Playbook<A>> Bot<A, P> {
    pub fn new(difficulty: Difficulty, playbook: P, spec: &GameSpecT) -> Result<Self, BotError> {
        let realtime = spec.time_mode == TimeMode::Realtime;
        let (cadence, delay, stride, reply_every, replan_every) = if realtime {
            let cadence =
                Cadence::new(spec.tick_hz, spec.decision_hz).map_err(|_| BotError::BadCadence)?;
            let delay = delay_ticks(difficulty.reaction_ms, spec.tick_hz);
            if delay > MAX_DELAY_TICKS {
                return Err(BotError::BadReaction);
            }
            (
                Some(cadence),
                delay,
                (spec.tick_hz / spec.decision_hz).max(1),
                u32::try_from(delay_ticks(OPPONENT_REACTION_MS, spec.tick_hz))
                    .unwrap_or(u32::MAX)
                    .max(1),
                u64::from((spec.tick_hz / REPLAN_HZ).max(1)),
            )
        } else {
            (None, 0, 1, 1, 1)
        };
        let insight = Insight::new(&difficulty, playbook.signal_classes());
        Ok(Self {
            difficulty,
            playbook,
            spec: spec.clone(),
            cadence,
            delay,
            stride,
            reply_every,
            replan_every,
            buffer: ReactionBuffer::new(delay),
            seen: ObservationT::default(),
            rng: ChaCha8Rng::seed_from_u64(0),
            me: 0,
            current: None,
            last_replan: None,
            newest: None,
            observed: None,
            history: VecDeque::new(),
            options: Vec::new(),
            insight,
            plan: Plan::new(difficulty.long_game),
            _game: PhantomData,
        })
    }

    #[must_use]
    pub fn difficulty(&self) -> &Difficulty {
        &self.difficulty
    }

    #[must_use]
    pub fn playbook(&self) -> &P {
        &self.playbook
    }

    #[must_use]
    pub fn profile(&self, game_id: &str) -> Option<Profile> {
        self.insight.profile(game_id)
    }

    pub fn load_profile(&mut self, profile: &Profile, game_id: &str) -> Result<(), ProfileError> {
        self.insight.load_profile(profile, game_id)
    }

    #[must_use]
    pub fn insight(&self) -> InsightView {
        let model = self.insight.model();
        let classes = if model.is_some() {
            self.playbook.signal_classes()
        } else {
            0
        };
        InsightView {
            current: self.current(),
            intent: self.history.back().map(|a| (a.intent, a.deceptive)),
            memory: model.is_some(),
            likely_shot: model.and_then(OpponentModel::likely_shot),
            likely_cover: (0..classes)
                .map(|s| model.and_then(|m| m.likely_cover(s)))
                .collect(),
        }
    }

    #[must_use]
    pub fn memory_len(&self) -> usize {
        self.history.len()
    }

    #[must_use]
    pub fn current(&self) -> Option<Choice> {
        self.current.map(|c| c.choice)
    }

    fn opponent(&self) -> PlayerId {
        if self.me == 0 { 1 } else { 0 }
    }

    fn unit(&mut self) -> f64 {
        (self.rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn own_errors(&self, room: usize) -> Vec<([f64; 2], f64)> {
        let wide = self.difficulty.spread / 2.0_f64.sqrt();
        if wide > 0.0 && room >= 3 {
            vec![
                ([-wide, -wide], 1.0 / 6.0),
                ([0.0; 2], 2.0 / 3.0),
                ([wide, wide], 1.0 / 6.0),
            ]
        } else {
            vec![([0.0; 2], 1.0)]
        }
    }

    fn commit(&mut self, choice: Choice, now: u64, importance: f64, signal: Option<u8>) {
        self.insight.start_attempt(&choice);
        self.plan.committed(&choice, importance);
        let spread = self.difficulty.spread;
        let aim = [
            (self.unit() + self.unit() - 1.0) * spread,
            (self.unit() + self.unit() - 1.0) * spread,
        ];
        self.current = Some(Committed {
            choice,
            since: now,
            aim,
            signal,
        });
    }

    fn finish(&mut self, action: &mut ActionT, now: u64, choice: &Choice) {
        action.tick = now;
        action.player = self.me;
        action.intent = choice.intent;
        action.deceptive = choice.deceptive;
        let space = self.spec.action_space.as_deref();
        if let (Some(values), Some(dims)) = (
            action.values.as_mut(),
            space.and_then(|s| s.continuous.as_ref()),
        ) {
            for (value, dim) in values.iter_mut().zip(dims) {
                *value = value.max(dim.min).min(dim.max);
            }
        }
        if action.discrete >= 0
            && self.difficulty.spread > 0.0
            && self.unit() < self.difficulty.spread
        {
            let legal: Vec<i32> = self
                .seen
                .legal
                .iter()
                .flatten()
                .enumerate()
                .filter(|&(_, &ok)| ok == 1)
                .filter_map(|(i, _)| i32::try_from(i).ok())
                .collect();
            if !legal.is_empty() {
                let pick = (self.unit() * legal.len() as f64) as usize;
                action.discrete = legal[pick.min(legal.len() - 1)];
            }
        }
    }
}

impl<A: GameAdapter, P: Playbook<A>> Controller<A> for Bot<A, P> {
    fn new_match(&mut self, spec: &GameSpecT, me: PlayerId, seed: u64) {
        debug_assert!(
            spec.tick_hz == self.spec.tick_hz && spec.decision_hz == self.spec.decision_hz,
            "new_match with a spec of different timing"
        );
        self.me = me;
        let stream = (u64::from(me) + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ stream);
        self.buffer.clear();
        self.current = None;
        self.last_replan = None;
        self.newest = None;
        self.observed = None;
        self.history.clear();
        self.insight.new_match();
        self.plan.reset();
    }

    fn end_match(&mut self, adapter: &A, obs: &ObservationT) {
        let state = adapter.state_from_observation(obs, &mut self.rng);
        let them = self.opponent();
        let score = obs.score.as_deref().unwrap_or(&[]);
        self.insight
            .observe::<A, P>(&self.playbook, &state, score, self.me, them);
        self.insight.close();
    }

    fn on_tick(&mut self, adapter: &A, obs: &ObservationT) -> Option<ActionT> {
        let now = obs.tick;
        if self.newest.is_some_and(|newest| now < newest) {
            self.history.clear();
            self.buffer.clear();
            self.last_replan = None;
            self.current = None;
            self.observed = None;
            self.insight.forget_moment();
        }
        self.newest = Some(now);
        if let Some(cadence) = self.cadence {
            self.buffer.push(obs);
            if !cadence.is_decision(now) {
                return None;
            }
            let delayed = self.buffer.delayed(now)?;
            self.seen.clone_from(delayed);
        } else {
            self.seen.clone_from(obs);
        }
        let seen = std::mem::take(&mut self.seen);
        let mut state = adapter.state_from_observation(&seen, &mut self.rng);
        let them = self.opponent();
        let score = seen.score.as_deref().unwrap_or(&[]);
        let fresh = if self.observed == Some(seen.tick) {
            Seen::default()
        } else {
            self.observed = Some(seen.tick);
            self.insight
                .observe::<A, P>(&self.playbook, &state, score, self.me, them)
        };
        if fresh.rally {
            self.plan.rally();
        }
        if fresh.started {
            let open = self.unit() < self.difficulty.deception;
            self.insight.gate_open = open;
            self.insight.fake_pending = open && self.difficulty.depth < 2;
            self.current = None;
        }
        if self.cadence.is_none() && seen.to_act != self.me as i8 {
            self.seen = seen;
            return None;
        }
        if self.cadence.is_some() && self.difficulty.anticipate {
            state = self.roll_forward(adapter, state, seen.tick, now);
        }
        self.seen = seen;
        self.insight
            .watch_own::<A, P>(&self.playbook, &state, self.me);
        let due = self
            .last_replan
            .is_none_or(|last| now >= last.saturating_add(self.replan_every));
        if due {
            self.replan(adapter, &state, now);
            self.last_replan = Some(now);
        }
        let committed = self.current?;
        let elapsed = now.saturating_sub(committed.since);
        let mut action =
            self.playbook
                .act(&state, self.me, &committed.choice, elapsed, committed.aim);
        self.finish(&mut action, now, &committed.choice);
        let cutoff = now.saturating_sub(self.delay as u64);
        while self.history.len() > 1 && self.history[1].tick <= cutoff {
            self.history.pop_front();
        }
        if self.history.back().is_some_and(|last| last.tick == now) {
            self.history.pop_back();
        }
        self.history.push_back(action.clone());
        Some(action)
    }
}
