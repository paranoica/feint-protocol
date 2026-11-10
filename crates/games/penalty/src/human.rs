use std::collections::VecDeque;

use crate::{Params, Penalty, PenaltyPlaybook};
use feint_ai::{Bot, Controller, Difficulty, RunHeader, RunReport, Standing, run_match};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

const DELAY_TICKS: usize = 72;
const HONEST_LEAN: f32 = 0.45;
const FAKE_LEAN: f32 = 0.45;
const EARLY_GUESS: f32 = 0.5;

pub struct Human {
    me: PlayerId,
    seen: VecDeque<ObservationT>,
    rng: ChaCha8Rng,
    corner: f32,
    tell: f32,
    guesses: bool,
    noise: f32,
    kick_seen: f32,
}

impl Human {
    #[must_use]
    pub fn new() -> Self {
        Self {
            me: 0,
            seen: VecDeque::new(),
            rng: ChaCha8Rng::seed_from_u64(0),
            corner: 0.0,
            tell: 1.0,
            guesses: false,
            noise: 0.0,
            kick_seen: -1.0,
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

impl Controller<Penalty> for Human {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.seen.clear();
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ 0x48_554d_414e);
        self.kick_seen = -1.0;
    }

    fn on_tick(&mut self, _game: &Penalty, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push_back(obs.clone());
        while self.seen.len() > DELAY_TICKS + 1 {
            self.seen.pop_front();
        }
        let v = self.seen.front()?.state.clone()?;
        let (keeper, phase, t, dist, lean, kicks) = (v[0] >= 0.5, v[1], v[2], v[3], v[4], v[13]);
        let now_kicks = obs.state.as_ref().and_then(|s| s.get(13)).copied();
        if now_kicks != Some(kicks) {
            let mut idle = ActionT::default();
            idle.tick = obs.tick;
            idle.player = self.me;
            idle.values = Some(vec![0.0, 1.0, 0.0, 0.0]);
            return Some(idle);
        }
        if kicks != self.kick_seen {
            self.kick_seen = kicks;
            let pick = (self.unit() * 3.0).min(2.0).floor();
            self.corner = (pick - 1.0) * 2.7;
            let tell = self.unit();
            self.tell = if tell < HONEST_LEAN {
                1.0
            } else if tell < HONEST_LEAN + FAKE_LEAN {
                -1.0
            } else {
                0.0
            };
            self.guesses = self.unit() < EARLY_GUESS;
            self.noise = (self.unit() - 0.5) * 1.0;
        }
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = self.me;
        let values = if keeper {
            let planted = phase == 5.0;
            let flying = phase == 2.0 && t > 0.0;
            if self.guesses && planted && lean.abs() > 0.3 {
                [0.0, 0.0, 1.0, -2.8 * lean.signum() + self.noise]
            } else if flying {
                let f = 106.0 / 240.0;
                let aim = v[6] * f / t.min(f);
                [0.0, 0.0, 1.0, aim + self.noise]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            }
        } else {
            let lean = self.tell * self.corner.signum() * 0.8;
            let at_ball = phase == 1.0 || dist < 0.2;
            [
                lean,
                1.0,
                f32::from(u8::from(at_ball)),
                self.corner + self.noise,
            ]
        };
        a.values = Some(values.to_vec());
        Some(a)
    }
}

#[must_use]
pub fn human_body(game: &Penalty) -> BodyParamsT {
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
        let mut game = Penalty::new(Params::default(), 3);
        let spec = game.spec();
        let mut bot = Bot::new(d, PenaltyPlaybook::new(Params::default()), &spec)
            .unwrap_or_else(|e| panic!("a preset difficulty builds a bot: {e:?}"));
        if let Some(profile) = memory.as_ref() {
            bot.load_profile(profile, "penalty")
                .unwrap_or_else(|e| panic!("a profile of this game loads: {e:?}"));
        }
        let mut human = Human::new();
        let bodies = if me == 0 {
            vec![d.body(&spec), human_body(&game)]
        } else {
            vec![human_body(&game), d.body(&spec)]
        };
        let header = RunHeader {
            game_id: "penalty".into(),
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
        if d.memory == feint_ai::Memory::MatchAndProfile {
            memory = bot.profile("penalty");
        }
    }
    standing
}
