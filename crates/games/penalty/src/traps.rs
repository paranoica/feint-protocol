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
