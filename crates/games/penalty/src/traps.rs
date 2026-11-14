use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::styles::{KickSeen, Mind, REACT};

pub const KICKS: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shot {
    Honest(u8),
    Fake(u8),
}

impl Shot {
    pub const ALL: [Self; 5] = [
        Self::Honest(0),
        Self::Honest(1),
        Self::Honest(2),
        Self::Fake(0),
        Self::Fake(2),
    ];

    #[must_use]
    pub fn lean(self) -> Option<u8> {
        match self {
            Self::Honest(1) => None,
            Self::Honest(c) | Self::Fake(c) => Some(c),
        }
    }

    #[must_use]
    pub fn real(self) -> u8 {
        match self {
            Self::Honest(c) => c,
            Self::Fake(shown) => 2 - shown.min(2),
        }
    }

    #[must_use]
    pub fn deceptive(self) -> bool {
        matches!(self, Self::Fake(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Guess(u8),
    Read,
    React,
}

impl Keep {
    pub const ALL: [Self; 5] = [
        Self::Guess(0),
        Self::Guess(1),
        Self::Guess(2),
        Self::Read,
        Self::React,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Table {
    pub react_save: f64,
}

#[must_use]
pub fn person_saves(response: usize, real: u8) -> bool {
    response == usize::from(real) || (response == REACT && real == 1)
}

fn react(table: &Table, corner: u8) -> (f64, Option<u8>) {
    if corner == 1 {
        (1.0, None)
    } else {
        (table.react_save, Some(corner))
    }
}

fn keep_outcomes(table: &Table, keep: Keep, corner: u8, lean: Option<u8>) -> (f64, Option<u8>) {
    match keep {
        Keep::Guess(g) => (f64::from(u8::from(g == corner)), Some(g)),
        Keep::Read => match lean {
            Some(l) => (f64::from(u8::from(l == corner)), Some(l)),
            None => react(table, corner),
        },
        Keep::React => react(table, corner),
    }
}

#[must_use]
pub fn shoot_values(mind: &Mind, n: u32, fakes: bool, deception: f64) -> Vec<(Shot, f64)> {
    Shot::ALL
        .iter()
        .filter(|s| fakes || !s.deceptive())
        .map(|&s| (s, shoot_q(mind, s, n, deception)))
        .collect()
}

fn shoot_q(mind: &Mind, shot: Shot, n: u32, deception: f64) -> f64 {
    let (lean, real) = (shot.lean(), shot.real());
    let odds = mind.keeper_odds(lean);
    let mut value = 0.0;
    for (r, &p) in odds.iter().enumerate() {
        if p <= 0.0 {
            continue;
        }
        let win = !person_saves(r, real);
        let mut later = 0.0;
        if n > 1 {
            let mut child = mind.clone();
            child.learn(KickSeen {
                person_shot: false,
                lean,
                shot: Some(real),
                dive: (r != REACT).then_some(r as u8),
                goal: win,
            });
            later = deception * shoot_best(&child, n - 1, true, deception)
                + (1.0 - deception) * shoot_best(&child, n - 1, false, deception);
        }
        value += p * (f64::from(u8::from(win)) + later);
    }
    value
}

fn shoot_best(mind: &Mind, n: u32, fakes: bool, deception: f64) -> f64 {
    shoot_values(mind, n, fakes, deception)
        .into_iter()
        .map(|(_, v)| v)
        .fold(f64::NEG_INFINITY, f64::max)
}

#[must_use]
pub fn keep_values(mind: &Mind, n: u32, table: &Table) -> Vec<(Keep, f64)> {
    Keep::ALL
        .iter()
        .map(|&k| (k, keep_q(mind, k, n, table)))
        .collect()
}

fn keep_q(mind: &Mind, keep: Keep, n: u32, table: &Table) -> f64 {
    let corners = mind.shooter_odds();
    let mut value = 0.0;
    for (c, &pc) in corners.iter().enumerate() {
        if pc <= 0.0 {
            continue;
        }
        let corner = c as u8;
        for (lean, pl) in mind.lean_odds(corner) {
            if pl <= 0.0 {
                continue;
            }
            let (save, dive) = keep_outcomes(table, keep, corner, lean);
            for (saved, ps) in [(true, save), (false, 1.0 - save)] {
                if ps <= 0.0 {
                    continue;
                }
                let mut later = 0.0;
                if n > 1 {
                    let mut child = mind.clone();
                    child.learn(KickSeen {
                        person_shot: true,
                        lean,
                        shot: Some(corner),
                        dive,
                        goal: !saved,
                    });
                    later = keep_best(&child, n - 1, table);
                }
                value += pc * pl * ps * (f64::from(u8::from(saved)) + later);
            }
        }
    }
    value
}

fn keep_best(mind: &Mind, n: u32, table: &Table) -> f64 {
    keep_values(mind, n, table)
        .into_iter()
        .map(|(_, v)| v)
        .fold(f64::NEG_INFINITY, f64::max)
}

fn best<T: Copy>(values: &[(T, f64)]) -> Option<T> {
    let mut pick: Option<(T, f64)> = None;
    for &(a, v) in values {
        if pick.is_none_or(|(_, b)| v > b) {
            pick = Some((a, v));
        }
    }
    pick.map(|(a, _)| a)
}

fn sample(odds: &[f64], u: f64) -> usize {
    let mut acc = 0.0;
    for (i, p) in odds.iter().enumerate() {
        acc += p;
        if u < acc {
            return i;
        }
    }
    odds.iter().rposition(|&p| p > 0.0).unwrap_or(0)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Abstract {
    pub shooting: [u32; 2],
    pub keeping: [u32; 2],
    pub log: Vec<KickSeen>,
    pub result: f64,
}

#[must_use]
pub fn horizon(depth: u32, kick: u32) -> u32 {
    let left = (kick..KICKS).filter(|j| j % 2 == kick % 2).count() as u32;
    depth.min(left).max(1)
}

impl Abstract {
    #[must_use]
    pub fn share(&self) -> f64 {
        let won = self.shooting[0] + self.keeping[0];
        let n = self.shooting[1] + self.keeping[1];
        f64::from(won) / f64::from(n.max(1))
    }
}

#[must_use]
pub fn play(
    start: &Mind,
    depth: u32,
    deception: f64,
    table: &Table,
    bot_first: bool,
    seed: u64,
) -> Abstract {
    let mut mind = start.clone();
    let mut out = Abstract::default();
    for k in 0..KICKS {
        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ (u64::from(k) << 48));
        let mut unit = || (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        let (u_gate, u_resp, u_lean, u_out) = (unit(), unit(), unit(), unit());
        let bot_shoots = (k % 2 == 0) == bot_first;
        let n = horizon(depth, k);
        if bot_shoots {
            let fakes = u_gate < deception;
            let values = shoot_values(&mind, n, fakes, deception);
            let Some(shot) = best(&values) else {
                continue;
            };
            let (lean, real) = (shot.lean(), shot.real());
            let r = sample(&mind.keeper_odds(lean), u_resp);
            let win = !person_saves(r, real);
            let seen = KickSeen {
                person_shot: false,
                lean,
                shot: Some(real),
                dive: (r != REACT).then_some(r as u8),
                goal: win,
            };
            mind.learn(seen);
            out.log.push(seen);
            out.shooting[0] += u32::from(win);
            out.shooting[1] += 1;
        } else {
            let values = keep_values(&mind, n, table);
            let Some(keep) = best(&values) else {
                continue;
            };
            let corner = sample(&mind.shooter_odds(), u_resp) as u8;
            let leans = mind.lean_odds(corner);
            let lean = leans[sample(&leans.map(|(_, p)| p), u_lean)].0;
            let (save, dive) = keep_outcomes(table, keep, corner, lean);
            let saved = u_out < save;
            let seen = KickSeen {
                person_shot: true,
                lean,
                shot: Some(corner),
                dive,
                goal: !saved,
            };
            mind.learn(seen);
            out.log.push(seen);
            out.keeping[0] += u32::from(saved);
            out.keeping[1] += 1;
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    pub each: u32,
    pub max: u32,
}

impl Rules {
    pub const SHOOTOUT: Self = Self { each: 5, max: 20 };

    #[must_use]
    pub fn decided(&self, kicks: u32, score: [u32; 2]) -> bool {
        let taken = [kicks.div_ceil(2), kicks / 2];
        let [a, b] = score;
        if kicks <= 2 * self.each {
            let left = [
                self.each - taken[0].min(self.each),
                self.each - taken[1].min(self.each),
            ];
            if a > b + left[1] || b > a + left[0] {
                return true;
            }
        }
        if kicks >= 2 * self.each && kicks.is_multiple_of(2) && a != b {
            return true;
        }
        kicks >= self.max
    }
}

pub struct Odds {
    rules: Rules,
    bot: usize,
    table: Vec<f64>,
}

impl Odds {
    #[must_use]
    pub fn new(rules: Rules, bot: usize, bot_scores: f64, person_scores: f64) -> Self {
        let m = rules.max as usize + 1;
        let mut table = vec![0.0; m * m * m];
        for kicks in (0..=rules.max).rev() {
            for a in 0..=rules.max {
                for b in 0..=rules.max {
                    let score = [a, b];
                    let v = if rules.decided(kicks, score) {
                        result(score, bot)
                    } else {
                        let shooter = (kicks % 2) as usize;
                        let p = if shooter == bot {
                            bot_scores
                        } else {
                            person_scores
                        };
                        let mut scored = score;
                        scored[shooter] = (scored[shooter] + 1).min(rules.max);
                        let at = |s: [u32; 2]| {
                            (kicks as usize + 1) * m * m + s[0] as usize * m + s[1] as usize
                        };
                        p * table[at(scored)] + (1.0 - p) * table[at(score)]
                    };
                    table[kicks as usize * m * m + a as usize * m + b as usize] = v;
                }
            }
        }
        Self { rules, bot, table }
    }

    #[must_use]
    pub fn at(&self, kicks: u32, score: [u32; 2]) -> f64 {
        let m = self.rules.max as usize + 1;
        if kicks > self.rules.max || score[0] > self.rules.max || score[1] > self.rules.max {
            return result(score, self.bot);
        }
        self.table[kicks as usize * m * m + score[0] as usize * m + score[1] as usize]
    }
}

fn result(score: [u32; 2], bot: usize) -> f64 {
    let (mine, theirs) = (score[bot], score[1 - bot]);
    if mine > theirs {
        1.0
    } else if mine == theirs {
        0.5
    } else {
        0.0
    }
}

pub struct Shootout<'a> {
    pub odds: &'a Odds,
    pub table: Table,
    pub deception: f64,
}

impl Shootout<'_> {
    fn value(&self, mind: &Mind, kicks: u32, score: [u32; 2], left: u32, fakes: bool) -> f64 {
        let rules = self.odds.rules;
        if rules.decided(kicks, score) {
            return result(score, self.odds.bot);
        }
        if left == 0 {
            return self.odds.at(kicks, score);
        }
        let values = self.values(mind, kicks, score, left);
        values
            .iter()
            .filter(|(a, _)| fakes || !matches!(a, Act::Shoot(s) if s.deceptive()))
            .map(|&(_, v)| v)
            .fold(f64::NEG_INFINITY, f64::max)
    }

    fn later(&self, mind: &Mind, kicks: u32, score: [u32; 2], left: u32) -> f64 {
        let shooter = (kicks % 2) as usize;
        if shooter == self.odds.bot && left > 0 && !self.odds.rules.decided(kicks, score) {
            let values = self.values(mind, kicks, score, left);
            let all = values.iter().map(|x| x.1).fold(f64::NEG_INFINITY, f64::max);
            let honest = values
                .iter()
                .filter(|(a, _)| !matches!(a, Act::Shoot(s) if s.deceptive()))
                .map(|x| x.1)
                .fold(f64::NEG_INFINITY, f64::max);
            self.deception * all + (1.0 - self.deception) * honest
        } else {
            self.value(mind, kicks, score, left, true)
        }
    }

    #[must_use]
    pub fn values(&self, mind: &Mind, kicks: u32, score: [u32; 2], left: u32) -> Vec<(Act, f64)> {
        let shooter = (kicks % 2) as usize;
        let mut out = Vec::new();
        if shooter == self.odds.bot {
            for shot in Shot::ALL {
                let (lean, real) = (shot.lean(), shot.real());
                let odds = mind.keeper_odds(lean);
                let mut v = 0.0;
                for (r, &p) in odds.iter().enumerate() {
                    if p <= 0.0 {
                        continue;
                    }
                    let win = !person_saves(r, real);
                    let mut child = mind.clone();
                    child.learn(KickSeen {
                        person_shot: false,
                        lean,
                        shot: Some(real),
                        dive: (r != REACT).then_some(r as u8),
                        goal: win,
                    });
                    let mut s = score;
                    s[shooter] += u32::from(win);
                    v += p * self.later(&child, kicks + 1, s, left - 1);
                }
                out.push((Act::Shoot(shot), v));
            }
        } else {
            let corners = mind.shooter_odds();
            for keep in Keep::ALL {
                let mut v = 0.0;
                for (c, &pc) in corners.iter().enumerate() {
                    if pc <= 0.0 {
                        continue;
                    }
                    let corner = c as u8;
                    let leans = if keep == Keep::Read {
                        mind.lean_odds(corner).to_vec()
                    } else {
                        vec![(None, 1.0)]
                    };
                    for (lean, pl) in leans {
                        if pl <= 0.0 {
                            continue;
                        }
                        let (save, dive) = keep_outcomes(&self.table, keep, corner, lean);
                        for (saved, ps) in [(true, save), (false, 1.0 - save)] {
                            if ps <= 0.0 {
                                continue;
                            }
                            let mut child = mind.clone();
                            child.learn(KickSeen {
                                person_shot: true,
                                lean,
                                shot: Some(corner),
                                dive,
                                goal: !saved,
                            });
                            let mut s = score;
                            s[shooter] += u32::from(!saved);
                            v += pc * pl * ps * self.later(&child, kicks + 1, s, left - 1);
                        }
                    }
                }
                out.push((Act::Keep(keep), v));
            }
        }
        out
    }

    #[must_use]
    pub fn play(&self, start: &Mind, rounds: u32, seed: u64) -> Abstract {
        let mut mind = start.clone();
        let mut out = Abstract::default();
        let mut score = [0u32; 2];
        let mut kicks = 0u32;
        while !self.odds.rules.decided(kicks, score) {
            let mut rng = ChaCha8Rng::seed_from_u64(seed ^ (u64::from(kicks) << 48));
            let mut unit = || (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
            let (u_gate, u_resp, u_lean, u_out) = (unit(), unit(), unit(), unit());
            let shooter = (kicks % 2) as usize;
            let fakes = u_gate < self.deception;
            let values: Vec<(Act, f64)> = self
                .values(&mind, kicks, score, 2 * rounds)
                .into_iter()
                .filter(|(a, _)| fakes || !matches!(a, Act::Shoot(s) if s.deceptive()))
                .collect();
            let Some(act) = best(&values) else {
                break;
            };
            let seen = match act {
                Act::Shoot(shot) => {
                    let (lean, real) = (shot.lean(), shot.real());
                    let r = sample(&mind.keeper_odds(lean), u_resp);
                    let win = !person_saves(r, real);
                    out.shooting[0] += u32::from(win);
                    out.shooting[1] += 1;
                    KickSeen {
                        person_shot: false,
                        lean,
                        shot: Some(real),
                        dive: (r != REACT).then_some(r as u8),
                        goal: win,
                    }
                }
                Act::Keep(keep) => {
                    let corner = sample(&mind.shooter_odds(), u_resp) as u8;
                    let leans = mind.lean_odds(corner);
                    let lean = leans[sample(&leans.map(|(_, p)| p), u_lean)].0;
                    let (save, dive) = keep_outcomes(&self.table, keep, corner, lean);
                    let saved = u_out < save;
                    out.keeping[0] += u32::from(saved);
                    out.keeping[1] += 1;
                    KickSeen {
                        person_shot: true,
                        lean,
                        shot: Some(corner),
                        dive,
                        goal: !saved,
                    }
                }
            };
            score[shooter] += u32::from(seen.goal);
            mind.learn(seen);
            out.log.push(seen);
            kicks += 1;
        }
        out.result = result(score, self.odds.bot);
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Shoot(Shot),
    Keep(Keep),
}
