#![allow(dead_code)]

use std::collections::VecDeque;

use airhockey::analysis::Frame;
use airhockey::{AirHockey, HockeyPlaybook, Params, Vec2};
use feint_ai::{Controller, Playbook, Stance};
use feint_protocol::{ActionT, ChaCha8Rng, EventT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use rand_chacha::rand_core::SeedableRng;

pub const READ_DELAY_TICKS: usize = 60;
const SIDE: f64 = 0.03;

pub struct Reader {
    anticipates: bool,
    playbook: HockeyPlaybook,
    seen: VecDeque<ObservationT>,
    rng: ChaCha8Rng,
    me: PlayerId,
}

impl Reader {
    pub fn new(params: Params, anticipates: bool) -> Self {
        Self {
            anticipates,
            playbook: HockeyPlaybook::new(params),
            seen: VecDeque::new(),
            rng: ChaCha8Rng::seed_from_u64(0),
            me: 0,
        }
    }

    fn stance(&self, puck: Vec2, attacker_mallet: Vec2) -> Stance {
        let attacker = 1 - self.me;
        let mine = if self.me == 0 {
            puck.x < 0.0
        } else {
            puck.x > 0.0
        };
        if mine {
            return Stance::Shoot(1);
        }
        let rel = attacker_mallet - puck;
        let rel = if attacker == 0 { rel } else { -rel };
        let side = if rel.x >= 0.0 {
            1
        } else if rel.y > SIDE {
            0
        } else if rel.y < -SIDE {
            2
        } else {
            1
        };
        Stance::Cover(if self.anticipates { 2 - side } else { side })
    }
}

impl Controller<AirHockey> for Reader {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.seen.clear();
        self.rng = ChaCha8Rng::seed_from_u64(seed);
    }

    fn on_tick(&mut self, game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push_back(obs.clone());
        while self.seen.len() > READ_DELAY_TICKS + 1 {
            self.seen.pop_front();
        }
        let state = game.state_from_observation(self.seen.front()?, &mut self.rng);
        let attacker = usize::from(1 - self.me);
        let stance = self.stance(state.world.puck.pos, state.world.mallets[attacker].pos);
        let mut action = self.playbook.opponent_move(&state, self.me, stance);
        action.tick = obs.tick;
        action.player = self.me;
        Some(action)
    }
}

pub struct Recorder<'a> {
    pub inner: &'a mut dyn Controller<AirHockey>,
    pub frames: Vec<Frame>,
}

impl Controller<AirHockey> for Recorder<'_> {
    fn new_match(&mut self, spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.frames.clear();
        self.inner.new_match(spec, me, seed);
    }

    fn on_tick(&mut self, game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        let live = game.live();
        self.frames.push(Frame {
            tick: live.tick(),
            puck: live.world.puck.pos,
            mallets: live.world.mallets.map(|m| m.pos),
            score: live.score(),
        });
        self.inner.on_tick(game, obs)
    }

    fn on_event(&mut self, event: &EventT) {
        self.inner.on_event(event);
    }

    fn end_match(&mut self, game: &AirHockey, obs: &ObservationT) {
        self.inner.end_match(game, obs);
    }
}
