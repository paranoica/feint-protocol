use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::human::Habits;

pub const REACT: usize = 3;

const STYLE_SALT: u64 = 0x5354_594c_4553;
const HELD_OUT_SALT: u64 = 0x4845_4c44_4f55;
const FRESH_WEIGHT: f64 = 1.0;
const LEARNER_START: f64 = 1.0 / 3.0;
const LEARNER_FLOOR: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeeperStyle {
    Static,
    Trust { rate: f64, guess: f64, start: f64 },
    Recency { decay: f64, guess: f64 },
    AntiRecency { decay: f64, guess: f64 },
    Fictitious { decay: f64, guess: f64 },
    Learner { rate: f64, guess: f64 },
    Counter { run: u32, guess: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShooterStyle {
    Static,
    WinStay { stay: f64 },
    WinShift { stay: f64 },
    AvoidDive { avoid: f64 },
    Markov,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KickSeen {
    pub person_shot: bool,
    pub lean: Option<u8>,
    pub shot: Option<u8>,
    pub dive: Option<u8>,
    pub goal: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mind {
    pub keeper: KeeperStyle,
    pub shooter: ShooterStyle,
    habits: Habits,
    table: [[f64; 3]; 9],
    trust: f64,
    weights: [f64; 3],
    values: [f64; 3],
    honest_run: u32,
    last: Option<(u8, bool)>,
    before_last: Option<u8>,
    last_dive: Option<u8>,
}

struct Draw(ChaCha8Rng);

impl Draw {
    fn new(seed: u64) -> Self {
        Self(ChaCha8Rng::seed_from_u64(seed))
    }

    fn unit(&mut self) -> f64 {
        (self.0.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn between(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

fn class(c: u8) -> usize {
    usize::from(c.min(2))
}

impl Mind {
    #[must_use]
    pub fn new(keeper: KeeperStyle, shooter: ShooterStyle, habits: Habits) -> Self {
        Self::with_table(keeper, shooter, habits, [[1.0 / 3.0; 3]; 9])
    }

    fn with_table(
        keeper: KeeperStyle,
        shooter: ShooterStyle,
        habits: Habits,
        table: [[f64; 3]; 9],
    ) -> Self {
        let trust = match keeper {
            KeeperStyle::Trust { start, .. } => start,
            _ => 0.5,
        };
        Self {
            keeper,
            shooter,
            habits,
            table,
            trust,
            weights: [FRESH_WEIGHT; 3],
            values: [LEARNER_START; 3],
            honest_run: 0,
            last: None,
            before_last: None,
            last_dive: None,
        }
    }

    #[must_use]
    pub fn person(id: u64) -> Self {
        let mut d = Draw::new(id ^ STYLE_SALT);
        let k = d.unit();
        let keeper = if k < 0.35 {
            KeeperStyle::Static
        } else if k < 0.60 {
            KeeperStyle::Trust {
                rate: d.between(0.2, 0.6),
                guess: d.between(0.6, 1.0),
                start: d.between(0.35, 0.65),
            }
        } else if k < 0.75 {
            KeeperStyle::Recency {
                decay: d.between(0.5, 0.85),
                guess: d.between(0.4, 0.9),
            }
        } else {
            KeeperStyle::AntiRecency {
                decay: d.between(0.5, 0.85),
                guess: d.between(0.4, 0.9),
            }
        };
        let s = d.unit();
        let shooter = if s < 0.35 {
            ShooterStyle::Static
        } else if s < 0.65 {
            ShooterStyle::WinStay {
                stay: d.between(0.6, 0.9),
            }
        } else if s < 0.80 {
            ShooterStyle::WinShift {
                stay: d.between(0.6, 0.9),
            }
        } else {
            ShooterStyle::AvoidDive {
                avoid: d.between(0.5, 0.9),
            }
        };
        Self::new(keeper, shooter, Habits::person(id))
    }

    #[must_use]
    pub fn held_out(id: u64) -> Self {
        let mut d = Draw::new(id ^ HELD_OUT_SALT);
        let k = d.unit();
        let keeper = if k < 1.0 / 3.0 {
            KeeperStyle::Fictitious {
                decay: d.between(0.5, 0.85),
                guess: d.between(0.4, 0.9),
            }
        } else if k < 2.0 / 3.0 {
            KeeperStyle::Learner {
                rate: d.between(0.2, 0.5),
                guess: d.between(0.5, 0.9),
            }
        } else {
            KeeperStyle::Counter {
                run: 2 + ((d.unit() * 3.0) as u32).min(2),
                guess: d.between(0.6, 1.0),
            }
        };
        let mut table = [[0.0; 3]; 9];
        for row in &mut table {
            let favourite = ((d.unit() * 3.0) as usize).min(2);
            for (c, w) in row.iter_mut().enumerate() {
                *w = d.between(0.2, 1.0) + if c == favourite { 1.5 } else { 0.0 };
            }
            let total: f64 = row.iter().sum();
            for w in row.iter_mut() {
                *w /= total;
            }
        }
        Self::with_table(keeper, ShooterStyle::Markov, Habits::person(id), table)
    }

    #[must_use]
    pub fn habits(&self) -> &Habits {
        &self.habits
    }

    #[must_use]
    pub fn keeper_odds(&self, lean: Option<u8>) -> [f64; 4] {
        let mut odds = [0.0; 4];
        let with_lean = |odds: &mut [f64; 4], with: f64, against: f64| {
            if let Some(l) = lean.filter(|&l| l != 1) {
                odds[class(l)] = with;
                odds[2 - class(l)] = against;
            }
        };
        match self.keeper {
            KeeperStyle::Static => {
                let g = f64::from(self.habits.guess);
                if self.habits.follows_lean {
                    with_lean(&mut odds, g, 0.0);
                } else {
                    with_lean(&mut odds, 0.0, g);
                }
            }
            KeeperStyle::Trust { guess, .. } => {
                let t = self.trust;
                with_lean(&mut odds, guess * t * t, guess * (1.0 - t) * (1.0 - t));
            }
            KeeperStyle::Recency { guess, .. } => {
                let total: f64 = self.weights.iter().sum();
                for (o, w) in odds.iter_mut().zip(self.weights) {
                    *o = guess * w / total;
                }
            }
            KeeperStyle::AntiRecency { guess, .. } => {
                let total: f64 = self.weights.iter().sum();
                for (o, w) in odds.iter_mut().zip(self.weights) {
                    *o = guess * (total - w) / (2.0 * total);
                }
            }
            KeeperStyle::Fictitious { guess, .. } => {
                let cube = self.weights.map(|w| w * w * w);
                let total: f64 = cube.iter().sum();
                for (o, w) in odds.iter_mut().zip(cube) {
                    *o = guess * w / total;
                }
            }
            KeeperStyle::Learner { guess, .. } => {
                let sq = self.values.map(|v| {
                    let x = LEARNER_FLOOR + v.max(0.0);
                    x * x
                });
                let total: f64 = sq.iter().sum();
                for (o, w) in odds.iter_mut().zip(sq) {
                    *o = guess * w / total;
                }
            }
            KeeperStyle::Counter { run, guess } => {
                if self.honest_run >= run {
                    with_lean(&mut odds, 0.0, guess);
                } else {
                    with_lean(&mut odds, guess / 2.0, 0.0);
                }
            }
        }
        odds[REACT] = (1.0 - odds[0] - odds[1] - odds[2]).max(0.0);
        odds
    }

    fn static_corners(&self) -> [f64; 3] {
        let c = self.habits.corner.map(f64::from);
        let total: f64 = c.iter().sum();
        c.map(|w| w / total)
    }

    #[must_use]
    pub fn shooter_odds(&self) -> [f64; 3] {
        let spread = |c: u8, p: f64| {
            let mut odds = [(1.0 - p) / 2.0; 3];
            odds[class(c)] = p;
            odds
        };
        match (self.shooter, self.last) {
            (ShooterStyle::WinStay { stay }, Some((c, goal))) => {
                spread(c, if goal { stay } else { 1.0 - stay })
            }
            (ShooterStyle::WinShift { stay }, Some((c, goal))) => {
                spread(c, if goal { 1.0 - stay } else { stay })
            }
            (ShooterStyle::AvoidDive { avoid }, _) => {
                let mut w = self.static_corners();
                if let Some(d) = self.last_dive {
                    w[class(d)] *= 1.0 - avoid;
                }
                let total: f64 = w.iter().sum();
                w.map(|x| x / total)
            }
            (ShooterStyle::Markov, Some((c, _))) => match self.before_last {
                Some(b) => self.table[3 * class(b) + class(c)],
                None => self.static_corners(),
            },
            _ => self.static_corners(),
        }
    }

    #[must_use]
    pub fn lean_odds(&self, corner: u8) -> [(Option<u8>, f64); 3] {
        let honest = f64::from(self.habits.honest);
        let fake = f64::from(self.habits.fake);
        if corner == 1 {
            return [(None, 1.0), (Some(0), 0.0), (Some(2), 0.0)];
        }
        let c = corner.min(2);
        [
            (None, (1.0 - honest - fake).max(0.0)),
            (Some(c), honest),
            (Some(2 - c), fake),
        ]
    }

    pub fn learn(&mut self, kick: KickSeen) {
        if kick.person_shot {
            if let Some(shot) = kick.shot {
                self.before_last = self.last.map(|(c, _)| c);
                self.last = Some((shot, kick.goal));
            }
            self.last_dive = kick.dive;
            return;
        }
        match self.keeper {
            KeeperStyle::Static => {}
            KeeperStyle::Trust { rate, .. } => {
                if let (Some(l), Some(s)) = (kick.lean.filter(|&l| l != 1), kick.shot) {
                    if s == l {
                        self.trust += rate * (1.0 - self.trust);
                    } else if s == 2 - l.min(2) {
                        self.trust -= rate * self.trust;
                    }
                }
            }
            KeeperStyle::Recency { decay, .. }
            | KeeperStyle::AntiRecency { decay, .. }
            | KeeperStyle::Fictitious { decay, .. } => {
                if let Some(s) = kick.shot {
                    for w in &mut self.weights {
                        *w *= decay;
                    }
                    self.weights[class(s)] += 1.0;
                }
            }
            KeeperStyle::Learner { rate, .. } => {
                if let (Some(d), Some(s)) = (kick.dive, kick.shot) {
                    let reward = if d == s { 1.0 } else { 0.0 };
                    let v = &mut self.values[class(d)];
                    *v += rate * (reward - *v);
                }
            }
            KeeperStyle::Counter { .. } => {
                if let (Some(l), Some(s)) = (kick.lean.filter(|&l| l != 1), kick.shot) {
                    if s == l {
                        self.honest_run += 1;
                    } else {
                        self.honest_run = 0;
                    }
                }
            }
        }
    }
}
