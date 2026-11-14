use std::collections::VecDeque;

use feint_ai::{Bot, Controller, Difficulty, RunHeader, RunReport, Standing, run_match};
use feint_protocol::{
    ActionT, BodyParamsT, ChaCha8Rng, GameAdapter, GameSpecT, ObservationT, PlayerId,
};
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::flight::intercept;
use crate::{Params, PingPong, PingPongPlaybook};

pub const DELAY_TICKS: usize = 72;
const BITE: f32 = 0.85;
const READ_FACING: f32 = 0.3;
const WIND_UP: f32 = 0.6;
const READY_BACK: f64 = 0.5;
const MISS_BASE: f32 = 0.02;
const MISS_PER_SPEED: f32 = 0.03;
const MISJUDGE: f32 = 0.015;
const AIM_SPREAD: f32 = 0.15;

pub struct Human {
    me: PlayerId,
    seen: VecDeque<ObservationT>,
    rng: ChaCha8Rng,
    shot: [f32; 3],
    hits_seen: u32,
    serve_wait: f32,
    bites: bool,
    misses: bool,
    misjudge: f32,
    game: Option<PingPong>,
}

impl Human {
    #[must_use]
    pub fn new() -> Self {
        Self {
            me: 0,
            seen: VecDeque::new(),
            rng: ChaCha8Rng::seed_from_u64(0),
            shot: [1.0, 0.4, 0.6],
            hits_seen: u32::MAX,
            serve_wait: 0.0,
            bites: false,
            misses: false,
            misjudge: 0.0,
            game: None,
        }
    }

    fn unit(&mut self) -> f32 {
        (self.rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    fn plan_shot(&mut self, speed: f32) {
        let side = if self.unit() < 0.5 { -1.0 } else { 1.0 };
        let spread = |u: f32| (2.0 * u - 1.0) * AIM_SPREAD;
        let (a, b) = (self.unit(), self.unit());
        self.shot = [
            0.7 + 0.5 * self.unit() + spread(a),
            side * (0.2 + 0.45 * self.unit()) + spread(b),
            0.45 + 0.35 * self.unit(),
        ];
        self.serve_wait = 0.5 + self.unit();
        self.bites = self.unit() < BITE;
        let fast = (speed - 3.0).max(0.0);
        self.misses = self.unit() < MISS_BASE + MISS_PER_SPEED * fast;
        self.misjudge = (2.0 * self.unit() - 1.0) * MISJUDGE * speed;
    }
}

impl Default for Human {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller<PingPong> for Human {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.seen.clear();
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ 0x5049_4e47);
        self.hits_seen = u32::MAX;
        self.plan_shot(0.0);
        self.game = Some(PingPong::new(Params::default(), 0));
    }

    fn on_tick(&mut self, _adapter: &PingPong, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push_back(obs.clone());
        while self.seen.len() > DELAY_TICKS + 1 {
            self.seen.pop_front();
        }
        let view = self.seen.front()?.clone();
        let v = view.state.clone()?;
        let p = Params::default();
        let game = self.game.as_ref()?;
        let table = game.state_from_observation(&view, &mut ChaCha8Rng::seed_from_u64(0));
        let hits = table.hits;
        if hits != self.hits_seen {
            self.hits_seen = hits;
            let [vx, vy, _] = table.ball.vel;
            self.plan_shot((vx * vx + vy * vy).sqrt() as f32);
        }
        let (phase, phase_time, my_serve, ball_vx) = (v[16], v[17], v[18] >= 0.5, v[11]);
        let their_facing = v[15];
        let ready = [-(p.half_length + READY_BACK) as f32, 0.0];
        let me = self.me.min(1);
        let mine = intercept(&table, me);
        let mut target = ready;
        let mut swing = 0.0;
        if phase == 0.0 && my_serve {
            target = [v[0], v[1]];
            swing = f32::from(u8::from(phase_time >= self.serve_wait));
        } else if let Some(at) = mine {
            let m = crate::adapter::mirror(me);
            target = [(at.at[0] * m) as f32, (at.at[1] * m) as f32 + self.misjudge];
            swing = f32::from(u8::from(!self.misses));
        } else if phase == 1.0 && ball_vx > 0.0 {
            let ball_x = v[8];
            let near_them = ball_x > (p.half_length as f32) * WIND_UP;
            if near_them && self.bites && their_facing.abs() > READ_FACING {
                target = [ready[0], their_facing.signum() * 0.45];
            }
        }
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = self.me;
        let facing = self.shot[1].signum() * 0.8;
        a.values = Some(vec![
            target[0],
            target[1],
            swing,
            self.shot[0],
            self.shot[1],
            self.shot[2],
            facing,
        ]);
        Some(a)
    }
}

#[must_use]
pub fn human_body(game: &PingPong) -> BodyParamsT {
    let spec = game.spec();
    let dims = spec.body_params.unwrap_or_default();
    let mut b = BodyParamsT::default();
    b.values = Some(dims.iter().map(|d| d.max).collect());
    b
}

#[must_use]
pub fn against_human(d: Difficulty, matches: u64) -> Standing {
    let mut standing = Standing::default();
    let mut memory: Option<feint_ai::Profile> = None;
    for seed in 0..matches {
        let me = (seed % 2) as usize;
        let mut game = PingPong::new(Params::default(), 3);
        let spec = game.spec();
        let mut bot = Bot::new(d, PingPongPlaybook::new(Params::default()), &spec)
            .unwrap_or_else(|e| panic!("a preset difficulty builds a bot: {e:?}"));
        if let Some(profile) = memory.as_ref() {
            bot.load_profile(profile, "pingpong")
                .unwrap_or_else(|e| panic!("a profile of this game loads: {e:?}"));
        }
        let mut human = Human::new();
        let bodies = if me == 0 {
            vec![d.body(&spec), human_body(&game)]
        } else {
            vec![human_body(&game), d.body(&spec)]
        };
        let header = RunHeader {
            game_id: "pingpong".into(),
            game_version: crate::GAME_VERSION,
            seed,
            bodies,
            levels: if me == 0 { [3, 0] } else { [0, 3] },
            opponent: feint_ai::Opponent::HandMade,
        };
        let report: RunReport = if me == 0 {
            run_match(&mut game, [&mut bot, &mut human], header, 240 * 1800)
        } else {
            run_match(&mut game, [&mut human, &mut bot], header, 240 * 1800)
        }
        .unwrap_or_else(|e| panic!("a match against the human model runs: {e:?}"));
        standing.add(&report, me);
        if d.memory == feint_ai::Memory::MatchAndProfile {
            memory = bot.profile("pingpong");
        }
    }
    standing
}
