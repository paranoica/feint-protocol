use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::minds::{Person, honest_batch, lay};
use crate::rules::{MAX_RANKS, Move, Outcome, Params, Table, by_rank};

const SAMPLE_TRIES: u32 = 64;

#[must_use]
pub fn bot_moves(params: &Params, table: &Table, me: u8) -> Vec<Move> {
    let ranks = usize::from(params.ranks);
    let hand = by_rank(params, table.hands[usize::from(me)]);
    let mut out = Vec::new();
    let held: Vec<usize> = (0..ranks).filter(|&r| hand[r] > 0).collect();
    match table.claim.map(usize::from).filter(|_| table.batches > 0) {
        None => {
            let Some(&top) = held
                .iter()
                .max_by_key(|&&r| (hand[r], std::cmp::Reverse(r)))
            else {
                return out;
            };
            for n in 1..=hand[top].min(params.max_batch) {
                out.push(lay(top, top, n));
            }
            if let Some(&low) = held
                .iter()
                .filter(|&&r| r != top)
                .min_by_key(|&&r| (hand[r], r))
            {
                out.push(lay(top, low, 1));
            }
        }
        Some(claim) => {
            out.push(Move::Call);
            for n in 1..=hand[claim].min(params.max_batch) {
                out.push(lay(claim, claim, n));
            }
            let mut others: Vec<usize> = held.iter().copied().filter(|&r| r != claim).collect();
            others.sort_by_key(|&r| (std::cmp::Reverse(hand[r]), r));
            let mut counts = [0u8; MAX_RANKS];
            let mut left = hand;
            for _ in 0..params.max_batch {
                let Some(&r) = others.iter().find(|&&r| left[r] > 0) else {
                    break;
                };
                left[r] -= 1;
                counts[r] += 1;
                out.push(Move::Lay {
                    claim: claim as u8,
                    counts,
                });
                others.sort_by_key(|&r| (std::cmp::Reverse(left[r]), r));
            }
        }
    }
    out.retain(|m| table.clone().play(params, me, m).is_ok());
    out
}

#[must_use]
pub fn leaf(table: &Table, me: u8) -> f64 {
    match table.outcome {
        Some(Outcome::Winner(w)) => f64::from(u8::from(w == me)),
        Some(Outcome::Draw) => 0.5,
        None => {
            let a = f64::from(table.hands[usize::from(me)].count_ones());
            let b = f64::from(table.hands[usize::from(1 - me)].count_ones());
            b / (a + b).max(1.0)
        }
    }
}

pub fn learn(person: &mut Person, params: &Params, before: &Table, player: u8, m: &Move, me: u8) {
    let running = before.batches > 0;
    if player == me {
        if running && before.last_by != me {
            person.saw_answer(matches!(m, Move::Call));
        }
    } else if matches!(m, Move::Call) && running {
        let claim = before.claim.map_or(0, usize::from);
        person.saw_call(honest_batch(params, before.last, claim));
    }
}

pub struct Search<'a> {
    pub params: &'a Params,
    pub me: u8,
}

impl Search<'_> {
    fn value(&self, table: &Table, person: &Person, depth: u32) -> f64 {
        if table.outcome.is_some() {
            return leaf(table, self.me);
        }
        let them = 1 - self.me;
        if table.to_act == self.me {
            if depth == 0 {
                return leaf(table, self.me);
            }
            self.values(table, person, depth)
                .into_iter()
                .map(|(_, v)| v)
                .fold(f64::NEG_INFINITY, f64::max)
        } else {
            let mut v = 0.0;
            for (m, p) in person.moves(self.params, table, them) {
                if p <= 0.0 {
                    continue;
                }
                let mut next = table.clone();
                if next.play(self.params, them, &m).is_err() {
                    continue;
                }
                let mut mind = *person;
                learn(&mut mind, self.params, table, them, &m, self.me);
                v += p * self.value(&next, &mind, depth);
            }
            v
        }
    }

    #[must_use]
    pub fn values(&self, table: &Table, person: &Person, depth: u32) -> Vec<(Move, f64)> {
        bot_moves(self.params, table, self.me)
            .into_iter()
            .map(|m| {
                let mut next = table.clone();
                let ok = next.play(self.params, self.me, &m).is_ok();
                let mut mind = *person;
                learn(&mut mind, self.params, table, self.me, &m, self.me);
                let v = if ok {
                    self.value(&next, &mind, depth - 1)
                } else {
                    f64::NEG_INFINITY
                };
                (m, v)
            })
            .collect()
    }
}

#[must_use]
pub fn determinize(
    params: &Params,
    table: &Table,
    me: u8,
    person: &Person,
    rng: &mut ChaCha8Rng,
) -> Table {
    let mine = usize::from(me);
    let theirs = 1 - mine;
    let deck = params.deck_mask();
    let pool = deck & !table.hands[mine] & !table.laid[mine];
    let hand_n = table.hands[theirs].count_ones();
    let laid_n = table.laid[theirs].count_ones();
    let theirs_last = table.batches > 0 && usize::from(table.last_by) == theirs;
    let last_n = if theirs_last {
        table.last.count_ones()
    } else {
        0
    };
    let claim = table.claim.map(usize::from);
    let pick = |from: u64, n: u32, rng: &mut ChaCha8Rng| -> u64 {
        let mut chosen = 0u64;
        let mut rest = from;
        for _ in 0..n {
            let k = rest.count_ones();
            if k == 0 {
                break;
            }
            let mut i = rng.next_u32() % k;
            let mut bits = rest;
            while i > 0 {
                bits &= bits - 1;
                i -= 1;
            }
            let bit = bits & bits.wrapping_neg();
            chosen |= bit;
            rest &= !bit;
        }
        chosen
    };
    let rank_mask = |rank: usize| -> u64 {
        let c = u32::from(params.copies);
        let base = ((1u64 << c) - 1) << (rank as u32 * c);
        base & deck
    };
    let mut fallback = table.clone();
    for attempt in 0..SAMPLE_TRIES {
        let mut set = 0u64;
        for rank in 0..usize::from(params.ranks) {
            let need = u32::from(table.known[mine][rank]);
            set |= pick(pool & rank_mask(rank) & !set, need, rng);
        }
        let more = (hand_n + laid_n).saturating_sub(set.count_ones());
        set |= pick(pool & !set, more, rng);
        let mut last = 0u64;
        if theirs_last {
            let c = claim.unwrap_or(0);
            let claim_cards = set & rank_mask(c);
            let held = claim_cards.count_ones();
            let full = last_n >= u32::from(params.max_batch);
            let consistent_honest = if full { held >= last_n } else { held == last_n };
            if consistent_honest {
                last = pick(claim_cards, last_n, rng);
            } else if last_n == 1 && held == 0 {
                let u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
                if u >= person.bluff_rate() && attempt + 1 < SAMPLE_TRIES {
                    continue;
                }
                last = pick(set & !rank_mask(c), 1, rng);
            } else if attempt + 1 < SAMPLE_TRIES {
                continue;
            } else {
                last = pick(set, last_n, rng);
            }
        }
        let laid = last | pick(set & !last, laid_n.saturating_sub(last_n), rng);
        let mut t = table.clone();
        t.hands[theirs] = set & !laid;
        t.laid[theirs] = laid;
        t.aside = pool & !set;
        if theirs_last {
            t.last = last;
        }
        if t.validate(params).is_ok() {
            return t;
        }
        fallback = t;
    }
    fallback
}

pub fn choose(
    params: &Params,
    table: &Table,
    me: u8,
    person: &Person,
    depth: u32,
    samples: u32,
    rng: &mut ChaCha8Rng,
) -> Option<Move> {
    let search = Search { params, me };
    let moves = bot_moves(params, table, me);
    let mut totals = vec![0.0; moves.len()];
    for _ in 0..samples.max(1) {
        let world = determinize(params, table, me, person, rng);
        for (total, (m, v)) in totals
            .iter_mut()
            .zip(search.values(&world, person, depth.max(1)))
        {
            debug_assert!(moves.contains(&m));
            *total += v;
        }
    }
    let mut best: Option<(usize, f64)> = None;
    for (i, &v) in totals.iter().enumerate() {
        if best.is_none_or(|(_, b)| v > b) {
            best = Some((i, v));
        }
    }
    best.map(|(i, _)| moves[i])
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Played {
    pub result: f64,
    pub actions: u32,
    pub bot_lies: [u32; 2],
    pub bot_calls: [u32; 2],
}

#[must_use]
pub fn play(start: &Person, depth: u32, samples: u32, me: u8, seed: u64) -> Played {
    let params = Params::default();
    let mut table = Table::deal(&params, seed);
    let mut person = *start;
    let mut out = Played::default();
    let them = 1 - me;
    while table.outcome.is_none() {
        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ (u64::from(table.actions) << 40));
        let player = table.to_act;
        let m = if player == me {
            match choose(&params, &table, me, &person, depth, samples, &mut rng) {
                Some(m) => m,
                None => break,
            }
        } else {
            let u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
            let options = person.moves(&params, &table, them);
            let mut acc = 0.0;
            let mut pick = None;
            for (m, p) in &options {
                acc += p;
                if u < acc {
                    pick = Some(*m);
                    break;
                }
            }
            match pick.or_else(|| options.iter().rev().find(|x| x.1 > 0.0).map(|x| x.0)) {
                Some(m) => m,
                None => break,
            }
        };
        if player == me {
            match m {
                Move::Call => {
                    out.bot_calls[1] += 1;
                    let claim = table.claim.map_or(0, usize::from);
                    if !honest_batch(&params, table.last, claim) {
                        out.bot_calls[0] += 1;
                    }
                }
                Move::Lay { claim, counts } => {
                    let lie = counts
                        .iter()
                        .enumerate()
                        .any(|(r, &c)| c > 0 && r != usize::from(claim));
                    out.bot_lies[1] += 1;
                    out.bot_lies[0] += u32::from(lie);
                }
            }
        }
        let before = table.clone();
        if table.play(&params, player, &m).is_err() {
            break;
        }
        learn(&mut person, &params, &before, player, &m, me);
    }
    out.result = leaf(&table, me);
    out.actions = table.actions;
    out
}
