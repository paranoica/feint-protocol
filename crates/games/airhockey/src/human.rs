use std::collections::VecDeque;

use crate::analysis::Frame;
use crate::{AirHockey, HockeyPlaybook, Params, Vec2};
use feint_ai::{Bot, Controller, Difficulty, Playbook, RunHeader, Stance, Standing, run_match};
use feint_protocol::{ActionT, ChaCha8Rng, EventT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use rand_chacha::rand_core::{RngCore, SeedableRng};

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

pub const HUMAN_DELAY_TICKS: usize = 72;
pub const HUMAN_POINTER_TICKS: u64 = 4;
pub const HUMAN_NOISE_TICKS: u64 = 60;
const INTERCEPT_DEPTH: f64 = 0.15;
const WHACK_THROUGH: f64 = 0.15;
const WHACK_BACK: f64 = 0.08;

pub struct Human {
    noise: f64,
    params: Params,
    bounds: [f32; 2],
    reader: Reader,
    seen: VecDeque<ObservationT>,
    rng: ChaCha8Rng,
    me: PlayerId,
    aim: u8,
    attacking: bool,
    error: Vec2,
}

impl Human {
    pub fn new(params: Params, noise: f64) -> Self {
        Self {
            noise,
            bounds: [params.half_length as f32, params.half_width as f32],
            reader: Reader::new(params.clone(), false),
            params,
            seen: VecDeque::new(),
            rng: ChaCha8Rng::seed_from_u64(0),
            me: 0,
            aim: 1,
            attacking: false,
            error: Vec2::ZERO,
        }
    }

    fn intercept(&self, puck: Vec2, vel: Vec2) -> Option<Vec2> {
        let goal_x = if self.me == 0 {
            -self.params.half_length
        } else {
            self.params.half_length
        };
        if (goal_x - puck.x) * vel.x <= 0.0 || vel.x.abs() < 0.5 {
            return None;
        }
        let line_x = goal_x - goal_x.signum() * INTERCEPT_DEPTH;
        let t = (line_x - puck.x) / vel.x;
        let limit = self.params.half_width - self.params.puck_radius;
        let mut y = puck.y + vel.y * t;
        for _ in 0..8 {
            if y > limit {
                y = 2.0 * limit - y;
            } else if y < -limit {
                y = -2.0 * limit - y;
            } else {
                break;
            }
        }
        Some(Vec2::new(line_x, y.clamp(-limit, limit)))
    }

    fn whack(&self, puck: Vec2, mallet: Vec2) -> Vec2 {
        let goal_x = if self.me == 0 {
            self.params.half_length
        } else {
            -self.params.half_length
        };
        let corner = [-0.6, 0.0, 0.6][usize::from(self.aim.min(2))] * self.params.mouth_half_width;
        let aim = Vec2::new(goal_x, if self.me == 0 { corner } else { -corner });
        let line = aim - puck;
        let line = line * (1.0 / line.length());
        let reach = self.params.puck_radius + self.params.mallet_radius;
        if (mallet - puck).dot(line) < -0.5 * reach {
            puck + line * WHACK_THROUGH
        } else {
            puck - line * (reach + WHACK_BACK)
        }
    }

    fn unit(&mut self) -> f64 {
        f64::from(self.rng.next_u32()) / f64::from(u32::MAX)
    }
}

impl Controller<AirHockey> for Human {
    fn new_match(&mut self, spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.seen.clear();
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ 0x48_554d_414e);
        self.reader.new_match(spec, me, seed);
        self.attacking = false;
        self.error = Vec2::ZERO;
    }

    fn on_tick(&mut self, game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push_back(obs.clone());
        while self.seen.len() > HUMAN_DELAY_TICKS + 1 {
            self.seen.pop_front();
        }
        if !obs.tick.is_multiple_of(HUMAN_POINTER_TICKS) {
            return None;
        }
        if obs.tick.is_multiple_of(HUMAN_NOISE_TICKS) {
            let (a, b) = (self.unit(), self.unit());
            self.error = Vec2::new(a - 0.5, b - 0.5) * (2.0 * self.noise);
        }
        let state = game.state_from_observation(self.seen.front()?, &mut self.rng);
        let puck = state.world.puck.pos;
        let mine = if self.me == 0 {
            puck.x < 0.0
        } else {
            puck.x > 0.0
        };
        if mine && !self.attacking {
            self.aim = (self.unit() * 3.0).min(2.0) as u8;
        }
        self.attacking = mine;
        let attacker = usize::from(1 - self.me);
        let stance = if mine {
            Stance::Shoot(self.aim)
        } else {
            self.reader.stance(puck, state.world.mallets[attacker].pos)
        };
        let mut action = self.reader.playbook.opponent_move(&state, self.me, stance);
        if mine {
            let hit = self.whack(puck, state.world.mallets[usize::from(self.me)].pos);
            let seen = if self.me == 0 { hit } else { -hit };
            action.values = Some(vec![seen.x as f32, seen.y as f32]);
        }
        if let Some(block) = self.intercept(puck, state.world.puck.vel) {
            let seen = if self.me == 0 { block } else { -block };
            action.values = Some(vec![seen.x as f32, seen.y as f32]);
        }
        if let Some(v) = action.values.as_mut() {
            let seen = if self.me == 0 {
                self.error
            } else {
                -self.error
            };
            v[0] = (v[0] + seen.x as f32).clamp(-self.bounds[0], self.bounds[0]);
            v[1] = (v[1] + seen.y as f32).clamp(-self.bounds[1], self.bounds[1]);
        }
        action.tick = obs.tick;
        action.player = self.me;
        Some(action)
    }
}

pub const HUMAN_NOISE: f64 = 0.04;
pub const HUMAN_GOALS: u32 = 7;
const CAP_TICKS: u64 = 240 * 600;

#[must_use]
pub fn against_human(d: Difficulty, matches: u64) -> Standing {
    let params = Params {
        goals_to_win: HUMAN_GOALS,
        ..Params::default()
    };
    let mut standing = Standing::default();
    let mut memory: Option<feint_ai::Profile> = None;
    for seed in 0..matches {
        let me = (seed % 2) as usize;
        let mut game = AirHockey::new(params.clone(), 1);
        let spec = game.spec();
        let mut bot = Bot::new(d, HockeyPlaybook::new(params.clone()), &spec)
            .unwrap_or_else(|e| panic!("a preset difficulty builds a bot: {e:?}"));
        if let Some(profile) = memory.as_ref() {
            bot.load_profile(profile, "airhockey")
                .unwrap_or_else(|e| panic!("a profile of this game loads: {e:?}"));
        }
        let mut human = Human::new(params.clone(), HUMAN_NOISE);
        let human_body = Difficulty::preset(5)
            .unwrap_or_else(|| panic!("level 5 exists"))
            .body(&spec);
        let bodies = if me == 0 {
            vec![d.body(&spec), human_body]
        } else {
            vec![human_body, d.body(&spec)]
        };
        let header = RunHeader {
            game_id: "airhockey".into(),
            game_version: crate::GAME_VERSION,
            seed,
            bodies,
            levels: [0, 0],
        };
        let report = if me == 0 {
            run_match(&mut game, [&mut bot, &mut human], header, CAP_TICKS)
        } else {
            run_match(&mut game, [&mut human, &mut bot], header, CAP_TICKS)
        }
        .unwrap_or_else(|e| panic!("a match against the human model runs: {e:?}"));
        standing.add(&report, me);
        if d.memory == feint_ai::Memory::MatchAndProfile {
            memory = bot.profile("airhockey");
        }
    }
    standing
}
