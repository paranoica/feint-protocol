use std::collections::VecDeque;

use feint_ai::{Bot, Controller, Difficulty, RunHeader, RunReport, Standing, run_match};
use feint_protocol::{
    ActionT, BodyParamsT, ChaCha8Rng, GameAdapter, GameSpecT, ObservationT, PlayerId,
};
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::playbook::order;
use crate::rules::{Bout, Height, Kind, Params, Phase, Stage};
use crate::{Boxing, BoxingPlaybook};

pub const DELAY_TICKS: usize = 72;
const BITE: f32 = 0.9;
const COUNTER: f32 = 0.25;
const SLACK: f64 = 0.08;
const HIGH: f32 = 0.65;
const HOOKS: f32 = 0.3;
const PAUSE: [f32; 2] = [0.4, 1.4];
const GUARD_EVERY: [f32; 2] = [0.5, 1.5];
const LEARN: f32 = 0.15;
const RANGE_NOISE: f64 = 0.06;
const READ: f32 = 0.4;
const REFRACTORY: u64 = 36;
const FOOLED_FOR: u64 = 120;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bites {
    pub windups: u32,
    pub bit: u32,
    pub feints: u32,
    pub feint_bit: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcomes {
    pub honest: [u32; 2],
    pub feint: [u32; 2],
}

pub struct Human {
    me: PlayerId,
    seen: VecDeque<ObservationT>,
    rng: ChaCha8Rng,
    habit: f32,
    guard: Height,
    next_guard: u64,
    next_attack: u64,
    plan: Option<(Kind, Height)>,
    their_strikes: u32,
    watching: Option<u32>,
    biting: bool,
    bite_at: u64,
    seen_feints: u32,
    fooled_until: u64,
    range_err: f64,
    bites: Bites,
    outcomes: Outcomes,
    counted: [u32; 2],
    bot_points: i64,
    after_feint: bool,
    game: Option<Boxing>,
}

impl Human {
    #[must_use]
    pub fn new() -> Self {
        Self {
            me: 0,
            seen: VecDeque::new(),
            rng: ChaCha8Rng::seed_from_u64(0),
            habit: 0.6,
            guard: Height::High,
            next_guard: 0,
            next_attack: 0,
            plan: None,
            their_strikes: 0,
            watching: None,
            biting: false,
            bite_at: 0,
            seen_feints: 0,
            fooled_until: 0,
            range_err: 0.0,
            bites: Bites::default(),
            outcomes: Outcomes::default(),
            counted: [0, 0],
            bot_points: 0,
            after_feint: false,
            game: None,
        }
    }

    #[must_use]
    pub fn bites(&self) -> Bites {
        self.bites
    }

    #[must_use]
    pub fn outcomes(&self) -> Outcomes {
        self.outcomes
    }

    fn count(&mut self, bot: &crate::Fighter, points: i64) {
        if bot.feints != self.counted[1] {
            self.counted[1] = bot.feints;
            self.after_feint = true;
        }
        if bot.strikes != self.counted[0] {
            self.counted[0] = bot.strikes;
            let row = if std::mem::take(&mut self.after_feint) {
                &mut self.outcomes.feint
            } else {
                &mut self.outcomes.honest
            };
            row[0] += 1;
            row[1] += u32::from(points > self.bot_points);
        }
        self.bot_points = points;
    }

    fn unit(&mut self) -> f32 {
        (self.rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    fn after(&mut self, now: u64, range: [f32; 2], hz: u32) -> u64 {
        let u = self.unit();
        now + (f64::from(range[0] + (range[1] - range[0]) * u) * f64::from(hz)) as u64
    }

    fn pick(&mut self, kind: Kind) -> (Kind, Height) {
        let height = if self.unit() < HIGH {
            Height::High
        } else {
            Height::Low
        };
        self.range_err = f64::from(2.0 * self.unit() - 1.0) * RANGE_NOISE;
        (kind, height)
    }

    fn react(&mut self, bot: &Stage, id: u32, now: u64, hz: u32) {
        let Stage::WindUp { kind, height, .. } = *bot else {
            self.biting = false;
            return;
        };
        if self.watching != Some(id) {
            self.watching = Some(id);
            self.bite_at = now
                + if now < self.fooled_until {
                    REFRACTORY
                } else {
                    0
                };
            self.bites.windups += 1;
            self.biting = self.unit() < BITE;
            if self.biting {
                self.bites.bit += 1;
            }
            if kind == Kind::Hook && self.plan.is_none() && self.unit() < COUNTER {
                self.plan = Some(self.pick(Kind::Jab));
                self.next_attack = now;
            }
        }
        if self.biting && now >= self.bite_at {
            self.guard = height;
            self.next_guard = self.after(now, GUARD_EVERY, hz);
        }
    }
}

impl Default for Human {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller<Boxing> for Human {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        *self = Self::new();
        self.me = me;
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ 0x424f_5849);
        self.game = Some(Boxing::new(Params::default(), 0));
    }

    fn on_tick(&mut self, _adapter: &Boxing, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push_back(obs.clone());
        while self.seen.len() > DELAY_TICKS + 1 {
            self.seen.pop_front();
        }
        let delayed = self.seen.front()?.clone();
        let game = self.game.as_ref()?;
        let p = Params::default();
        let hz = p.tick_hz;
        let rng = &mut ChaCha8Rng::seed_from_u64(0);
        let now_state: Bout = game.state_from_observation(obs, rng);
        let then: Bout = game.state_from_observation(&delayed, rng);
        let me = usize::from(self.me.min(1));
        let them = 1 - me;
        let now = obs.tick;
        let mine = now_state.fighters[me];
        let bot = then.fighters[them];
        let v = obs.state.as_deref()?;
        let d = delayed.state.as_deref()?;
        let (my_x, their_x) = (f64::from(v[0]), f64::from(d[1]));
        let points = obs
            .score
            .as_deref()
            .and_then(|s| s.get(them).copied())
            .unwrap_or(0);
        self.count(&now_state.fighters[them], points);
        if bot.strikes != self.their_strikes {
            self.their_strikes = bot.strikes;
            let high = f32::from(u8::from(bot.last == Some(Height::High)));
            self.habit += LEARN * (high - self.habit);
        }
        if now_state.phase != Phase::Fight {
            self.plan = None;
            self.watching = None;
            self.biting = false;
            return Some(self.stamp(order(self.me, my_x, self.guard, None, false), now));
        }
        if now >= self.next_guard {
            self.guard = if self.unit() < self.habit {
                Height::High
            } else {
                Height::Low
            };
            self.next_guard = self.after(now, GUARD_EVERY, hz);
        }
        if bot.feints != self.seen_feints {
            self.seen_feints = bot.feints;
            self.bites.feints += 1;
            if self.biting {
                self.bites.feint_bit += 1;
                self.fooled_until = now + FOOLED_FOR;
            }
        }
        self.react(&bot.stage, bot.strikes + bot.feints, now, hz);
        let gap = their_x - my_x;
        if matches!(mine.stage, Stage::WindUp { .. }) && self.plan.is_some() {
            self.plan = None;
            self.next_attack =
                self.after(now, PAUSE, hz) + u64::from(p.hook_windup + p.hook_recover);
        }
        if self.plan.is_none() && mine.stage == Stage::Idle && now >= self.next_attack {
            let kind = if self.unit() < HOOKS {
                Kind::Hook
            } else {
                Kind::Jab
            };
            self.plan = Some(self.pick(kind));
        }
        let action = match self.plan {
            Some((kind, height)) => {
                let range = p.reach(kind) - SLACK + self.range_err;
                let launch = mine.stage == Stage::Idle && gap <= range + 0.02;
                let height = if launch && self.unit() < READ {
                    bot.guard.other()
                } else {
                    height
                };
                order(
                    self.me,
                    their_x - range,
                    self.guard,
                    launch.then_some((kind, height)),
                    false,
                )
            }
            None => order(self.me, my_x, self.guard, None, false),
        };
        Some(self.stamp(action, now))
    }
}

impl Human {
    fn stamp(&self, mut a: ActionT, now: u64) -> ActionT {
        a.tick = now;
        a.player = self.me;
        a
    }
}

#[must_use]
pub fn human_body(game: &Boxing) -> BodyParamsT {
    let spec = game.spec();
    let dims = spec.body_params.unwrap_or_default();
    let mut b = BodyParamsT::default();
    b.values = Some(dims.iter().map(|d| d.max).collect());
    b
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    pub standing: Standing,
    pub feints: u32,
    pub bites: Bites,
    pub outcomes: Outcomes,
    pub landed: [u32; 2],
    pub blocked: [u32; 2],
    pub strikes: [u32; 2],
}

#[must_use]
pub fn against_human(d: Difficulty, matches: u64) -> Standing {
    tally(d, matches).standing
}

#[must_use]
pub fn tally(d: Difficulty, matches: u64) -> Tally {
    let mut out = Tally::default();
    let mut memory: Option<feint_ai::Profile> = None;
    for seed in 0..matches {
        let me = (seed % 2) as usize;
        let mut game = Boxing::new(Params::default(), 3);
        let spec = game.spec();
        let mut bot = Bot::new(d, BoxingPlaybook::new(Params::default()), &spec)
            .unwrap_or_else(|e| panic!("a preset difficulty builds a bot: {e:?}"));
        if let Some(profile) = memory.as_ref() {
            bot.load_profile(profile, "boxing")
                .unwrap_or_else(|e| panic!("a profile of this game loads: {e:?}"));
        }
        let mut human = Human::new();
        let bodies = if me == 0 {
            vec![d.body(&spec), human_body(&game)]
        } else {
            vec![human_body(&game), d.body(&spec)]
        };
        let header = RunHeader {
            game_id: "boxing".into(),
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
        out.standing.add(&report, me);
        let live = game.live();
        out.feints += live.fighters[me].feints;
        for (k, seat) in [me, 1 - me].into_iter().enumerate() {
            out.landed[k] += live.landed[seat];
            out.blocked[k] += live.blocked[seat];
            out.strikes[k] += live.fighters[seat].strikes;
        }
        let o = human.outcomes();
        for k in 0..2 {
            out.outcomes.honest[k] += o.honest[k];
            out.outcomes.feint[k] += o.feint[k];
        }
        let b = human.bites();
        out.bites.windups += b.windups;
        out.bites.bit += b.bit;
        out.bites.feints += b.feints;
        out.bites.feint_bit += b.feint_bit;
        if d.memory == feint_ai::Memory::MatchAndProfile {
            memory = bot.profile("boxing");
        }
    }
    out
}
