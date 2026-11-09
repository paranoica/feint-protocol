use std::collections::VecDeque;
use std::marker::PhantomData;

use feint_protocol::{
    ActionT, ChaCha8Rng, EventT, GameAdapter, GameSpecT, ObservationT, PlayerId, TimeMode,
};
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::difficulty::Difficulty;
use crate::reaction::{Cadence, ReactionBuffer, delay_ticks};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choice {
    pub id: u16,
    pub intent: u8,
    pub deceptive: bool,
    pub horizon_ticks: u32,
    pub param: [f32; 2],
}

pub trait Playbook<A: GameAdapter> {
    fn options(&self, state: &A::State, me: PlayerId, repertoire: u8, out: &mut Vec<Choice>);

    fn act(
        &self,
        state: &A::State,
        me: PlayerId,
        choice: &Choice,
        elapsed: u64,
        aim: [f64; 2],
    ) -> ActionT;

    fn reply(&self, state: &A::State, them: PlayerId) -> ActionT;

    fn evaluate(&self, state: &A::State, me: PlayerId) -> f64;

    fn margin(&self) -> f64;
}

pub trait Controller<A: GameAdapter> {
    fn new_match(&mut self, spec: &GameSpecT, me: PlayerId, seed: u64);

    fn on_tick(&mut self, adapter: &A, obs: &ObservationT) -> Option<ActionT>;

    fn on_event(&mut self, event: &EventT) {
        let _ = event;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotError {
    BadCadence,
    BadReaction,
}

pub const SIMULATION_BUDGET: u32 = 1_200;

pub const MAX_DELAY_TICKS: usize = 1 << 16;

pub const REPLAN_HZ: u32 = 10;

pub const OPPONENT_REACTION_MS: u32 = 250;

#[derive(Clone, Copy, Debug)]
struct Committed {
    choice: Choice,
    since: u64,
    aim: [f64; 2],
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
    history: VecDeque<ActionT>,
    options: Vec<Choice>,
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
            history: VecDeque::new(),
            options: Vec::new(),
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

    fn step(&self, adapter: &A, state: &A::State, own: Option<&ActionT>, ticks: u64) -> A::State {
        let reply = self.playbook.reply(state, self.opponent());
        let ticks = u32::try_from(ticks).unwrap_or(u32::MAX);
        match own {
            Some(own) => adapter.simulate(state, &[own.clone(), reply], ticks),
            None => adapter.simulate(state, std::slice::from_ref(&reply), ticks),
        }
    }

    fn roll_forward(&self, adapter: &A, state: A::State, from: u64, to: u64) -> A::State {
        let to = to.min(from.saturating_add(self.delay as u64));
        let mut state = state;
        let mut at = from;
        let mut own = self.history.iter().rev().find(|a| a.tick <= from);
        for action in self
            .history
            .iter()
            .filter(|a| a.tick > from && a.tick <= to)
        {
            if action.tick > at {
                state = self.step(adapter, &state, own, action.tick - at);
                at = action.tick;
            }
            own = Some(action);
        }
        if to > at {
            state = self.step(adapter, &state, own, to - at);
        }
        state
    }

    fn rollout(
        &self,
        adapter: &A,
        state: &A::State,
        choice: &Choice,
        cap: u32,
        elapsed: u64,
    ) -> f64 {
        let horizon = choice.horizon_ticks.clamp(1, cap);
        let mut state = state.clone();
        let mut theirs = self.playbook.reply(&state, self.opponent());
        let mut next_reply = self.reply_every;
        let mut done = 0;
        while done < horizon {
            if done >= next_reply {
                theirs = self.playbook.reply(&state, self.opponent());
                next_reply = next_reply.saturating_add(self.reply_every);
            }
            let ticks = self.stride.min(horizon - done).min(next_reply - done);
            let mine = self.playbook.act(
                &state,
                self.me,
                choice,
                elapsed.saturating_add(u64::from(done)),
                [0.0; 2],
            );
            state = adapter.simulate(&state, &[mine, theirs.clone()], ticks);
            done += ticks;
        }
        self.playbook.evaluate(&state, self.me)
    }

    fn replan(&mut self, adapter: &A, state: &A::State, now: u64) {
        let mut options = std::mem::take(&mut self.options);
        self.playbook
            .options(state, self.me, self.difficulty.repertoire, &mut options);
        let current = self.current;
        let offered = |id: u16| options.iter().any(|c| c.id == id);
        if let Some(cur) = current
            && offered(cur.choice.id)
            && now.saturating_sub(cur.since) < u64::from(cur.choice.horizon_ticks / 2)
        {
            self.options = options;
            return;
        }
        if options.is_empty() {
            self.current = None;
            self.options = options;
            return;
        }
        let cap = (SIMULATION_BUDGET / u32::try_from(options.len()).unwrap_or(u32::MAX)).max(1);
        let mut best: Option<(Choice, f64)> = None;
        let mut current_score = None;
        for choice in &options {
            let elapsed = current
                .filter(|c| c.choice.id == choice.id)
                .map_or(0, |c| now.saturating_sub(c.since));
            let score = self.rollout(adapter, state, choice, cap, elapsed);
            if current.is_some_and(|c| c.choice.id == choice.id) {
                current_score = Some((*choice, score));
            }
            let better = best.is_none_or(|(b, bs)| score > bs || (score == bs && choice.id < b.id));
            if better {
                best = Some((*choice, score));
            }
        }
        self.options = options;
        let Some((choice, score)) = best else { return };
        let keep = match (current, current_score) {
            (Some(cur), Some((_, cur_score))) => {
                cur.choice.id == choice.id || score <= cur_score + self.playbook.margin()
            }
            _ => false,
        };
        match (keep, current, current_score) {
            (true, Some(cur), Some((offer, _))) => {
                let attempt = u64::from(cur.choice.horizon_ticks.max(1));
                if now.saturating_sub(cur.since) >= attempt {
                    self.commit(offer, now);
                }
            }
            _ => self.commit(choice, now),
        }
    }

    fn commit(&mut self, choice: Choice, now: u64) {
        let spread = self.difficulty.spread;
        let aim = [
            (self.unit() + self.unit() - 1.0) * spread,
            (self.unit() + self.unit() - 1.0) * spread,
        ];
        self.current = Some(Committed {
            choice,
            since: now,
            aim,
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
        self.history.clear();
    }

    fn on_tick(&mut self, adapter: &A, obs: &ObservationT) -> Option<ActionT> {
        let now = obs.tick;
        if self.newest.is_some_and(|newest| now < newest) {
            self.history.clear();
            self.buffer.clear();
            self.last_replan = None;
            self.current = None;
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
            if obs.to_act != self.me as i8 {
                return None;
            }
            self.seen.clone_from(obs);
        }
        let seen = std::mem::take(&mut self.seen);
        let mut state = adapter.state_from_observation(&seen, &mut self.rng);
        if self.cadence.is_some() && self.difficulty.anticipate {
            state = self.roll_forward(adapter, state, seen.tick, now);
        }
        self.seen = seen;
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
        self.history.push_back(action.clone());
        Some(action)
    }
}
