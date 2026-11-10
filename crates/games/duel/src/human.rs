use std::collections::VecDeque;

use crate::{DuelPlaybook, Params, QuickDraw};
use feint_ai::{Bot, Controller, Difficulty, RunHeader, RunReport, Standing, run_match};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

const DELAY_TICKS: usize = 72;
const ANTICIPATE: f32 = 0.6;
const ANTICIPATE_SPREAD: f32 = 0.15;
const LEAD_SPREAD: f32 = 0.8;

pub struct Human {
    me: PlayerId,
    seen: VecDeque<ObservationT>,
    rng: ChaCha8Rng,
    cycle: f32,
    bites: bool,
    left: bool,
    fake: bool,
    wait: f32,
    noise: f32,
    loaded_for: u32,
    their_loaded_seen: bool,
    phase_seen: f32,
    step_at: Option<u64>,
}

impl Human {
    #[must_use]
    pub fn new() -> Self {
        Self {
            me: 0,
            seen: VecDeque::new(),
            rng: ChaCha8Rng::seed_from_u64(0),
            cycle: -1.0,
            bites: false,
            left: false,
            fake: false,
            wait: 0.0,
            noise: 0.0,
            loaded_for: 0,
            their_loaded_seen: true,
            phase_seen: 0.0,
            step_at: None,
        }
    }

    fn plan_step(&mut self, now: u64, after: u32) {
        if self.unit() < ANTICIPATE {
            let jitter = (self.unit() * ANTICIPATE_SPREAD * 240.0) as u64;
            let delay = u64::from(after).saturating_sub(DELAY_TICKS as u64);
            self.step_at = Some(now + delay + jitter);
        }
    }

    fn unit(&mut self) -> f32 {
        (self.rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
}

impl Default for Human {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller<QuickDraw> for Human {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.seen.clear();
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ 0x48_554d_414e);
        self.cycle = -1.0;
        self.loaded_for = 0;
        self.their_loaded_seen = true;
        self.phase_seen = 0.0;
        self.step_at = None;
    }

    fn on_tick(&mut self, _game: &QuickDraw, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push_back(obs.clone());
        while self.seen.len() > DELAY_TICKS + 1 {
            self.seen.pop_front();
        }
        let v = self.seen.front()?.state.clone()?;
        let (my_x, my_hand, my_loaded, my_recovery) = (v[0], v[1], v[3] >= 0.5, v[4]);
        let (their_x, their_hand, their_loaded) = (v[9], v[10], v[12] >= 0.5);
        let their_shots = v[16];
        let live = v[26] == 1.0;
        let cycle = v[28] * 1000.0 + their_shots;
        if cycle != self.cycle {
            self.cycle = cycle;
            self.bites = self.unit() < 0.6;
            self.left = self.unit() < 0.5;
            self.fake = self.unit() < 0.03;
            self.wait = 0.0;
            let guess = self.unit();
            let lead = if guess < 0.7 {
                0.0
            } else if guess < 0.85 {
                -1.0
            } else {
                1.0
            };
            let size = 1.0 + (self.unit() - 0.5) * LEAD_SPREAD;
            self.noise = lead * size + (self.unit() - 0.5) * 0.3;
        }
        let params = Params::default();
        if self.their_loaded_seen && !their_loaded {
            self.plan_step(obs.tick, params.reload_ticks);
        }
        if v[26] == 0.0 && self.phase_seen != 0.0 {
            self.plan_step(obs.tick, params.ready_ticks);
        }
        self.their_loaded_seen = their_loaded;
        self.phase_seen = v[26];
        let threatened = their_loaded && their_hand > 0.3;
        let guessed = self.step_at.is_some_and(|t| obs.tick >= t);
        if guessed {
            self.step_at = None;
        }
        let roll = if live && my_recovery == 0.0 && (guessed || self.bites && threatened) {
            let room_left = my_x > -1.4;
            let room_right = my_x < 1.4;
            if (self.left && room_left) || !room_right {
                -1.0
            } else {
                1.0
            }
        } else {
            0.0
        };
        if my_loaded && live {
            self.loaded_for += 1;
        } else {
            self.loaded_for = 0;
        }
        let waited = self.loaded_for as f32 / 240.0 >= self.wait || !my_loaded;
        let hold = if !waited {
            false
        } else if self.fake && my_loaded {
            if my_hand >= 0.4 {
                self.fake = false;
                self.wait += 0.6;
                false
            } else {
                true
            }
        } else {
            true
        };
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = self.me;
        a.values = Some(vec![their_x + self.noise, f32::from(u8::from(hold)), roll]);
        Some(a)
    }
}

#[must_use]
pub fn human_body(game: &QuickDraw) -> BodyParamsT {
    let spec = game.spec();
    let dims = spec.body_params.unwrap_or_default();
    let mut b = BodyParamsT::default();
    b.values = Some(dims.iter().map(|d| d.max).collect());
    b
}

#[must_use]
pub fn against_human(d: Difficulty, matches: u64) -> Standing {
    let mut standing = Standing::default();
    for seed in 0..matches {
        let me = (seed % 2) as usize;
        let mut game = QuickDraw::new(Params::default(), 3);
        let spec = game.spec();
        let mut bot = Bot::new(d, DuelPlaybook::new(Params::default()), &spec)
            .unwrap_or_else(|e| panic!("a preset difficulty builds a bot: {e:?}"));
        let mut human = Human::new();
        let bodies = if me == 0 {
            vec![d.body(&spec), human_body(&game)]
        } else {
            vec![human_body(&game), d.body(&spec)]
        };
        let header = RunHeader {
            game_id: "duel".into(),
            game_version: crate::GAME_VERSION,
            seed,
            bodies,
            levels: if me == 0 { [3, 0] } else { [0, 3] },
        };
        let report: RunReport = if me == 0 {
            run_match(&mut game, [&mut bot, &mut human], header, 240 * 900)
        } else {
            run_match(&mut game, [&mut human, &mut bot], header, 240 * 900)
        }
        .unwrap_or_else(|e| panic!("a match against the human model runs: {e:?}"));
        standing.add(&report, me);
    }
    standing
}
