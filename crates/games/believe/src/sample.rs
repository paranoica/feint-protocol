use feint_protocol::{ChaCha8Rng, ObservationT};
use rand_chacha::rand_core::RngCore;

use crate::rules::{MAX_CARDS, Outcome, Params, Table};

fn take_lowest(params: &Params, from: &mut u64, rank: usize, n: u8) -> u64 {
    let mut mask = 0u64;
    let mut left = n;
    for copy in 0..u32::from(params.copies) {
        if left == 0 {
            break;
        }
        let bit = 1u64 << (rank as u32 * u32::from(params.copies) + copy);
        if *from & bit != 0 {
            *from &= !bit;
            mask |= bit;
            left -= 1;
        }
    }
    mask
}

fn take_counts(params: &Params, from: &mut u64, counts: &[f32]) -> u64 {
    let mut mask = 0u64;
    for (rank, &n) in counts.iter().enumerate() {
        mask |= take_lowest(params, from, rank, n as u8);
    }
    mask
}

fn take_random(from: &mut u64, n: u32, rng: &mut ChaCha8Rng) -> u64 {
    let mut mask = 0u64;
    for _ in 0..n {
        let size = from.count_ones();
        if size == 0 {
            break;
        }
        let mut pick = rng.next_u32() % size;
        let mut rest = *from;
        while pick > 0 {
            rest &= rest - 1;
            pick -= 1;
        }
        let bit = 1u64 << rest.trailing_zeros();
        *from &= !bit;
        mask |= bit;
    }
    mask
}

pub(crate) fn table_from_view(p: &Params, obs: &ObservationT, rng: &mut ChaCha8Rng) -> Table {
    let ranks = usize::from(p.ranks);
    let seen = obs.state.as_deref().unwrap_or_default();
    let field = |group: usize| seen.get(group * ranks..(group + 1) * ranks).unwrap_or(&[]);
    let scalar = |i: usize| seen.get(5 * ranks + i).copied().unwrap_or(0.0);
    let me = usize::from(obs.player.min(1));
    let other = 1 - me;
    let me8 = me as u8;
    let mut free = p.deck_mask();
    let mut table = Table::empty(p);
    table.hands[me] = take_counts(p, &mut free, field(0));
    let known = take_counts(p, &mut free, field(1));
    table.laid[me] = take_counts(p, &mut free, field(2));
    table.claim = field(3).iter().position(|&f| f > 0.5).map(|r| r as u8);
    for (rank, &n) in field(1).iter().enumerate() {
        table.known[me][rank] = (n as u8).min(p.copies);
    }
    let pile = scalar(0) as u32;
    let last_count = scalar(1) as u32;
    let their_hand = scalar(2) as u32;
    let leads = scalar(3) > 0.5;
    table.batches = (scalar(4) as u16).min(MAX_CARDS as u16);
    table.actions = p.cap_actions.saturating_sub(scalar(5) as u32);
    let their_laid = pile.saturating_sub(table.laid[me].count_ones());
    let possession = their_hand.saturating_add(their_laid);
    let mut theirs = known
        | take_random(
            &mut free,
            possession.saturating_sub(known.count_ones()),
            rng,
        );
    table.laid[other] = take_random(&mut theirs, their_laid, rng);
    table.hands[other] = theirs;
    table.aside = free;
    let leader = if leads { me8 } else { 1 - me8 };
    let follower_to_act = table.batches % 2 == 1;
    table.to_act = match obs.to_act {
        0 => 0,
        1 => 1,
        _ if follower_to_act => 1 - leader,
        _ => leader,
    };
    if table.batches > 0 {
        table.last_by = if follower_to_act { leader } else { 1 - leader };
        let mut from = table.laid[usize::from(table.last_by)];
        table.last = if table.last_by == me8 {
            take_counts(p, &mut from, field(4))
        } else {
            take_random(&mut from, last_count, rng)
        };
    }
    if table.batches > 0 && table.hands[usize::from(table.last_by)] == 0 {
        table.out = Some(table.last_by);
    }
    let score = obs.score.as_deref().unwrap_or_default();
    if obs.to_act < 0 {
        table.outcome = Some(match score {
            [1, 0] => Outcome::Winner(0),
            [0, 1] => Outcome::Winner(1),
            _ => Outcome::Draw,
        });
    }
    table.tick = obs.tick;
    table
}
