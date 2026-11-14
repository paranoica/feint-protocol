use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::rules::{MAX_RANKS, Move, Params, Table, by_rank};

const STYLE_SALT: u64 = 0x4245_4c49_4556;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CallerStyle {
    Static {
        call: f64,
    },
    Trust {
        low: f64,
        high: f64,
        rate: f64,
        start: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiarStyle {
    Static { bluff: f64 },
    Wary { bluff: f64, rate: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Person {
    pub caller: CallerStyle,
    pub liar: LiarStyle,
    trust: f64,
    called: f64,
}

fn most_held(hand: &[u8; MAX_RANKS], ranks: usize, besides: Option<usize>) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (rank, &n) in hand.iter().enumerate().take(ranks) {
        if n == 0 || Some(rank) == besides {
            continue;
        }
        if best.is_none_or(|b| n > hand[b]) {
            best = Some(rank);
        }
    }
    best
}

#[must_use]
pub fn lay(claim: usize, rank: usize, n: u8) -> Move {
    let mut counts = [0u8; MAX_RANKS];
    counts[rank] = n;
    Move::Lay {
        claim: claim as u8,
        counts,
    }
}

#[must_use]
pub fn honest_batch(params: &Params, last: u64, claim: usize) -> bool {
    by_rank(params, last)
        .iter()
        .enumerate()
        .all(|(rank, &c)| c == 0 || rank == claim)
}

impl Person {
    #[must_use]
    pub fn new(caller: CallerStyle, liar: LiarStyle) -> Self {
        let trust = match caller {
            CallerStyle::Trust { start, .. } => start,
            CallerStyle::Static { .. } => 0.5,
        };
        Self {
            caller,
            liar,
            trust,
            called: 0.0,
        }
    }

    #[must_use]
    pub fn drawn(id: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(id ^ STYLE_SALT);
        let mut unit = || (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        let mut between = |lo: f64, hi: f64| lo + (hi - lo) * unit();
        let caller = if between(0.0, 1.0) < 0.4 {
            CallerStyle::Static {
                call: between(0.1, 0.5),
            }
        } else {
            CallerStyle::Trust {
                low: between(0.05, 0.2),
                high: between(0.5, 0.9),
                rate: between(0.2, 0.6),
                start: between(0.3, 0.7),
            }
        };
        let liar = if between(0.0, 1.0) < 0.5 {
            LiarStyle::Static {
                bluff: between(0.2, 0.8),
            }
        } else {
            LiarStyle::Wary {
                bluff: between(0.4, 0.9),
                rate: between(0.2, 0.6),
            }
        };
        Self::new(caller, liar)
    }

    #[must_use]
    pub fn call_rate(&self) -> f64 {
        match self.caller {
            CallerStyle::Static { call } => call,
            CallerStyle::Trust { low, high, .. } => low + (high - low) * (1.0 - self.trust),
        }
    }

    #[must_use]
    pub fn bluff_rate(&self) -> f64 {
        match self.liar {
            LiarStyle::Static { bluff } => bluff,
            LiarStyle::Wary { bluff, .. } => bluff * (1.0 - self.called),
        }
    }

    pub fn saw_call(&mut self, honest: bool) {
        if let CallerStyle::Trust { rate, .. } = self.caller {
            if honest {
                self.trust += rate * (1.0 - self.trust);
            } else {
                self.trust -= rate * self.trust;
            }
        }
    }

    pub fn saw_answer(&mut self, called: bool) {
        if let LiarStyle::Wary { rate, .. } = self.liar {
            let target = if called { 1.0 } else { 0.0 };
            self.called += rate * (target - self.called);
        }
    }

    #[must_use]
    pub fn moves(&self, params: &Params, table: &Table, me: u8) -> Vec<(Move, f64)> {
        let ranks = usize::from(params.ranks);
        let mine = usize::from(me);
        let hand = by_rank(params, table.hands[mine]);
        let Some(claim) = table.claim.map(usize::from).filter(|_| table.batches > 0) else {
            return most_held(&hand, ranks, None)
                .map(|r| vec![(lay(r, r, hand[r].min(params.max_batch)), 1.0)])
                .unwrap_or_default();
        };
        if table.hands[1 - mine] == 0 {
            return vec![(Move::Call, 1.0)];
        }
        let shown = table.last.count_ones();
        let mine_laid = by_rank(params, table.laid[mine])[claim];
        let accounted = u32::from(hand[claim]) + u32::from(mine_laid);
        if shown + accounted > u32::from(params.copies) {
            return vec![(Move::Call, 1.0)];
        }
        let q = self.call_rate();
        if hand[claim] > 0 {
            let honest = lay(claim, claim, hand[claim].min(params.max_batch));
            return vec![(Move::Call, q), (honest, 1.0 - q)];
        }
        match most_held(&hand, ranks, Some(claim)) {
            Some(rank) => {
                let b = self.bluff_rate();
                vec![
                    (Move::Call, q + (1.0 - q) * (1.0 - b)),
                    (lay(claim, rank, 1), (1.0 - q) * b),
                ]
            }
            None => vec![(Move::Call, 1.0)],
        }
    }
}
