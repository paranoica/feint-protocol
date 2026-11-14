use std::collections::VecDeque;

use crate::styles::{KickSeen, Mind, REACT};
use crate::{Params, Penalty, PenaltyPlaybook};
use feint_ai::{Bot, Controller, Difficulty, RunHeader, RunReport, Standing, run_match};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

pub const DELAY_TICKS: usize = 72;
const HONEST_LEAN: f32 = 0.35;
const FAKE_LEAN: f32 = 0.45;
const STRAIGHTEN: f32 = 0.25;
const STRAIGHTEN_AT: f32 = 1.0;
const EARLY_GUESS: f32 = 0.5;
const MIND_SALT: u64 = 0x4d49_4e44;
const FLIGHT_SECONDS: f32 = 106.0 / 240.0;
const GUESS_Y: f32 = 2.8;
const READ_LEAN: f32 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Habits {
    pub corner: [f32; 3],
    pub honest: f32,
    pub fake: f32,
    pub straighten: f32,
    pub guess: f32,
    pub follows_lean: bool,
    pub noise: f32,
}

impl Default for Habits {
    fn default() -> Self {
        Self {
            corner: [1.0, 1.0, 1.0],
            honest: HONEST_LEAN,
            fake: FAKE_LEAN,
            straighten: STRAIGHTEN,
            guess: EARLY_GUESS,
            follows_lean: false,
            noise: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kick {
    pub corner: f32,
    pub tell: f32,
    pub straightens: bool,
    pub guesses: bool,
    pub noise: f32,
}

impl Habits {
    pub fn draw(&self, unit: &mut dyn FnMut() -> f32) -> Kick {
        let [a, b, c] = self.corner;
        let x = unit() * (a + b + c);
        let pick = if x < a {
            0.0
        } else if x < a + b {
            1.0
        } else {
            2.0
        };
        let t = unit();
        let tell = if t < self.honest {
            1.0
        } else if t < self.honest + self.fake {
            -1.0
        } else {
            0.0
        };
        let straightens = unit() < self.straighten;
        let guesses = unit() < self.guess;
        let noise = (unit() - 0.5) * self.noise;
        Kick {
            corner: (pick - 1.0) * 2.7,
            tell,
            straightens,
            guesses,
            noise,
        }
    }
}

pub struct Human {
    me: PlayerId,
    habits: Habits,
    seen: VecDeque<ObservationT>,
    rng: ChaCha8Rng,
    corner: f32,
    tell: f32,
    straightens: bool,
    guesses: bool,
    noise: f32,
    kick_seen: f32,
    adaptive: Option<Adaptive>,
}

struct Adaptive {
    start: Mind,
    mind: Mind,
    rng: ChaCha8Rng,
    watch: Option<Watch>,
    pick: Option<(f32, usize)>,
    seen: Vec<KickSeen>,
}

#[derive(Clone, Copy, Debug)]
struct Watch {
    kick: f32,
    keeper: bool,
    score: [i64; 2],
    struck: bool,
    lean: Option<u8>,
    shot: Option<u8>,
    dive: Option<u8>,
}

impl Adaptive {
    fn unit(&mut self) -> f64 {
        (self.rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn sample(&mut self, odds: &[f64]) -> usize {
        let u = self.unit();
        let mut acc = 0.0;
        for (i, p) in odds.iter().enumerate() {
            acc += p;
            if u < acc {
                return i;
            }
        }
        odds.iter().rposition(|&p| p > 0.0).unwrap_or(0)
    }

    fn track(&mut self, v: &[f32], score: &[i64]) {
        let score = [
            score.first().copied().unwrap_or(0),
            score.get(1).copied().unwrap_or(0),
        ];
        let kick = v[13];
        if self.watch.is_none_or(|w| w.kick != kick) {
            if let Some(w) = self.watch {
                let shooter = usize::from(w.kick as u32 % 2 == 1);
                let seen = KickSeen {
                    person_shot: !w.keeper,
                    lean: w.lean,
                    shot: w.shot,
                    dive: w.dive,
                    goal: score[shooter] > w.score[shooter],
                };
                self.mind.learn(seen);
                self.seen.push(seen);
            }
            self.watch = Some(Watch {
                kick,
                keeper: v[0] >= 0.5,
                score,
                struck: false,
                lean: None,
                shot: None,
                dive: None,
            });
        }
        let Some(w) = self.watch.as_mut() else {
            return;
        };
        let (phase, t, lean) = (v[1], v[2], v[4]);
        if phase == 5.0 && !w.struck {
            w.struck = true;
            w.lean = PenaltyPlaybook::lean_class(f64::from(lean));
        }
        if phase == 2.0 && t > 0.0 {
            let aim = v[6] * FLIGHT_SECONDS / t.min(FLIGHT_SECONDS);
            w.shot = Some(PenaltyPlaybook::class_of(f64::from(aim)));
        }
        if v[12] >= 0.5 && w.dive.is_none() {
            w.dive = Some(PenaltyPlaybook::class_of(f64::from(v[10])));
        }
    }
}

impl Human {
    #[must_use]
    pub fn new() -> Self {
        Self::with_habits(Habits::default())
    }

    #[must_use]
    pub fn with_habits(habits: Habits) -> Self {
        Self {
            me: 0,
            habits,
            seen: VecDeque::new(),
            rng: ChaCha8Rng::seed_from_u64(0),
            corner: 0.0,
            tell: 1.0,
            straightens: false,
            guesses: false,
            noise: 0.0,
            kick_seen: -1.0,
            adaptive: None,
        }
    }

    #[must_use]
    pub fn with_mind(mind: Mind) -> Self {
        let mut human = Self::with_habits(*mind.habits());
        human.adaptive = Some(Adaptive {
            start: mind.clone(),
            mind,
            rng: ChaCha8Rng::seed_from_u64(0),
            watch: None,
            pick: None,
            seen: Vec::new(),
        });
        human
    }

    #[must_use]
    pub fn seen(&self) -> &[KickSeen] {
        self.adaptive.as_ref().map_or(&[], |a| a.seen.as_slice())
    }

    #[must_use]
    pub fn mind(&self) -> Option<&Mind> {
        self.adaptive.as_ref().map(|a| &a.mind)
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
        if let Some(a) = self.adaptive.as_mut() {
            a.mind = a.start.clone();
            a.rng = ChaCha8Rng::seed_from_u64(seed ^ MIND_SALT);
            a.watch = None;
            a.pick = None;
            a.seen.clear();
        }
    }

    fn on_tick(&mut self, _game: &Penalty, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push_back(obs.clone());
        while self.seen.len() > DELAY_TICKS + 1 {
            self.seen.pop_front();
        }
        let v = self.seen.front()?.state.clone()?;
        let (keeper, phase, t, dist, lean, kicks) = (v[0] >= 0.5, v[1], v[2], v[3], v[4], v[13]);
        if let Some(a) = self.adaptive.as_mut() {
            let score = self
                .seen
                .front()
                .and_then(|o| o.score.clone())
                .unwrap_or_default();
            a.track(&v, &score);
        }
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
            let habits = self.habits;
            let kick = habits.draw(&mut || self.unit());
            self.corner = kick.corner;
            self.tell = kick.tell;
            self.straightens = kick.straightens;
            self.guesses = kick.guesses;
            self.noise = kick.noise;
            if let Some(a) = self.adaptive.as_mut()
                && !keeper
            {
                let odds = a.mind.shooter_odds();
                let class = a.sample(&odds);
                self.corner = (class as f32 - 1.0) * 2.7;
            }
        }
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = self.me;
        let values = if keeper {
            let planted = phase == 5.0;
            let flying = phase == 2.0 && t > 0.0;
            let pick = self.adaptive.as_mut().and_then(|a| {
                if planted && a.pick.is_none_or(|(k, _)| k != kicks) {
                    let lean = a.watch.and_then(|w| w.lean);
                    let odds = a.mind.keeper_odds(lean);
                    let pick = a.sample(&odds);
                    a.pick = Some((kicks, pick));
                }
                a.pick.filter(|&(k, _)| k == kicks).map(|(_, p)| p)
            });
            if let Some(p) = pick.filter(|&p| p != REACT && (planted || flying)) {
                [0.0, 0.0, 1.0, (p as f32 - 1.0) * GUESS_Y + self.noise]
            } else if self.adaptive.is_some() && !flying {
                [0.0, 0.0, 0.0, 0.0]
            } else if self.guesses && planted && lean.abs() > READ_LEAN {
                let side = if self.habits.follows_lean { 2.8 } else { -2.8 };
                [0.0, 0.0, 1.0, side * lean.signum() + self.noise]
            } else if flying {
                let f = 106.0 / 240.0;
                let aim = v[6] * f / t.min(f);
                [0.0, 0.0, 1.0, aim + self.noise]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            }
        } else {
            let late = self.straightens && dist < STRAIGHTEN_AT;
            let lean = if late || self.corner == 0.0 {
                0.0
            } else {
                self.tell * self.corner.signum() * 0.8
            };
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
            opponent: feint_ai::Opponent::HandMade,
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
