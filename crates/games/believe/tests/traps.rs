use believe::minds::{CallerStyle, LiarStyle, Person, lay};
use believe::traps::{Search, bot_moves, determinize, leaf, play};
use believe::{Move, Outcome, Params, Table, by_rank};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;

const EPS: f64 = 1e-12;

fn trusting() -> Person {
    Person::new(
        CallerStyle::Trust {
            low: 0.1,
            high: 0.9,
            rate: 0.5,
            start: 0.5,
        },
        LiarStyle::Wary {
            bluff: 0.8,
            rate: 0.5,
        },
    )
}

#[test]
fn a_trusting_caller_calls_less_after_honest_reveals_and_more_after_lies() {
    let mut honest = trusting();
    let mut lied = trusting();
    for _ in 0..2 {
        honest.saw_call(true);
        lied.saw_call(false);
    }
    let up = 1.0 - 0.5 * 0.5 * 0.5;
    let down = 0.5 * 0.5 * 0.5;
    assert!((honest.call_rate() - (0.1 + 0.8 * (1.0 - up))).abs() < EPS);
    assert!((lied.call_rate() - (0.1 + 0.8 * (1.0 - down))).abs() < EPS);
}

#[test]
fn a_wary_liar_bluffs_less_after_being_called() {
    let mut p = trusting();
    p.saw_answer(true);
    assert!((p.bluff_rate() - 0.8 * 0.5).abs() < EPS);
    p.saw_answer(false);
    assert!((p.bluff_rate() - 0.8 * (1.0 - 0.25)).abs() < EPS);
    let mut s = Person::new(
        CallerStyle::Static { call: 0.3 },
        LiarStyle::Static { bluff: 0.6 },
    );
    s.saw_answer(true);
    s.saw_call(false);
    assert!((s.bluff_rate() - 0.6).abs() < EPS && (s.call_rate() - 0.3).abs() < EPS);
}

fn cards(params: &Params, ranks: &[(u8, u8)], used: u64) -> u64 {
    let mut mask = 0u64;
    for &(rank, n) in ranks {
        let mut left = n;
        for copy in 0..params.copies {
            let bit = 1u64 << (u32::from(rank) * u32::from(params.copies) + u32::from(copy));
            if left > 0 && (used | mask) & bit == 0 {
                mask |= bit;
                left -= 1;
            }
        }
    }
    mask
}

fn round(
    params: &Params,
    bot: &[(u8, u8)],
    person: &[(u8, u8)],
    last: &[(u8, u8)],
    claim: u8,
) -> Table {
    let mut t = Table::empty(params);
    t.hands[0] = cards(params, bot, 0);
    t.hands[1] = cards(params, person, t.hands[0]);
    let laid = cards(params, last, t.hands[0] | t.hands[1]);
    t.laid[0] = laid;
    t.last = laid;
    t.last_by = 0;
    t.claim = Some(claim);
    t.batches = 1;
    t.to_act = 1;
    t.aside = params.deck_mask() & !(t.hands[0] | t.hands[1] | laid);
    t
}

#[test]
fn the_person_answers_a_lay_by_its_rates_and_calls_an_impossible_claim() {
    let params = Params::default();
    let p = trusting();
    let t = round(&params, &[(0, 2), (1, 2)], &[(2, 2), (3, 1)], &[(1, 1)], 0);
    let moves = p.moves(&params, &t, 1);
    let q = p.call_rate();
    let b = p.bluff_rate();
    assert_eq!(moves.len(), 2);
    assert!((moves[0].1 - (q + (1.0 - q) * (1.0 - b))).abs() < EPS);
    assert_eq!(moves[1].0, lay(0, 2, 1));
    assert!((moves.iter().map(|m| m.1).sum::<f64>() - 1.0).abs() < EPS);
    let holds = round(&params, &[(1, 2)], &[(0, 2), (3, 1)], &[(1, 1)], 0);
    let moves = p.moves(&params, &holds, 1);
    assert_eq!(moves[1].0, lay(0, 0, 2));
    assert!((moves[1].1 - (1.0 - q)).abs() < EPS);
    let impossible = round(&params, &[(1, 2)], &[(0, 3), (3, 1)], &[(1, 2)], 0);
    assert_eq!(p.moves(&params, &impossible, 1), vec![(Move::Call, 1.0)]);
}

#[test]
fn the_bot_offers_a_call_honest_lays_and_lies_and_nothing_illegal() {
    let params = Params::default();
    let mut t = round(&params, &[(0, 2), (2, 3)], &[(1, 3)], &[(4, 1)], 0);
    t.last_by = 1;
    t.laid = [0, t.laid[0]];
    t.to_act = 0;
    let moves = bot_moves(&params, &t, 0);
    assert!(moves.contains(&Move::Call));
    assert!(moves.contains(&lay(0, 0, 1)) && moves.contains(&lay(0, 0, 2)));
    assert!(moves.contains(&lay(0, 2, 1)));
    for m in &moves {
        assert!(t.clone().play(&params, 0, m).is_ok(), "{m:?}");
    }
}

#[test]
fn laying_while_the_person_is_out_loses_and_the_search_sees_it() {
    let params = Params::default();
    let mut t = round(&params, &[(0, 2), (2, 2)], &[], &[(1, 1)], 1);
    t.laid = [0, t.laid[0]];
    t.last_by = 1;
    t.out = Some(1);
    t.to_act = 0;
    let s = Search {
        params: &params,
        me: 0,
    };
    let values = s.values(&t, &trusting(), 1);
    for (m, v) in &values {
        if matches!(m, Move::Lay { .. }) {
            assert!(v.abs() < EPS, "{values:?}");
        }
    }
    let mut won = t.clone();
    won.outcome = Some(Outcome::Winner(0));
    assert!((leaf(&won, 0) - 1.0).abs() < EPS);
}

#[test]
fn a_sampled_world_keeps_what_the_bot_sees() {
    let params = Params::default();
    for seed in 0..40 {
        let t0 = Table::deal(&params, seed);
        let mut t = t0.clone();
        let p = trusting();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut steps = 0;
        while t.outcome.is_none() && steps < 30 {
            let player = t.to_act;
            let moves = if player == 0 {
                bot_moves(&params, &t, 0)
            } else {
                p.moves(&params, &t, 1).into_iter().map(|m| m.0).collect()
            };
            let Some(m) = moves.get(steps % moves.len().max(1)).copied() else {
                break;
            };
            t.play(&params, player, &m)
                .unwrap_or_else(|e| panic!("{e}"));
            steps += 1;
            if t.to_act != 0 || t.outcome.is_some() {
                continue;
            }
            let w = determinize(&params, &t, 0, &p, &mut rng);
            assert!(w.validate(&params).is_ok(), "{seed} {steps}");
            assert_eq!((w.hands[0], w.laid[0]), (t.hands[0], t.laid[0]));
            assert_eq!(w.hands[1].count_ones(), t.hands[1].count_ones());
            assert_eq!(w.laid[1].count_ones(), t.laid[1].count_ones());
            assert_eq!(w.last.count_ones(), t.last.count_ones());
            assert_eq!(
                (w.claim, w.batches, w.to_act, w.known),
                (t.claim, t.batches, t.to_act, t.known)
            );
            let held = by_rank(&params, w.hands[1] | w.laid[1]);
            for (k, h) in t.known[0].iter().zip(held) {
                assert!(*k <= h, "{seed} {steps}");
            }
        }
    }
}

#[test]
fn a_match_ends_with_a_result_and_repeats_for_a_seed() {
    for seed in 0..6 {
        let p = Person::drawn(seed);
        let a = play(&p, 1, 2, (seed % 2) as u8, seed);
        let b = play(&p, 1, 2, (seed % 2) as u8, seed);
        assert_eq!(a, b);
        assert!([0.0, 0.5, 1.0].contains(&a.result), "{a:?}");
        assert!(a.actions > 0);
    }
}

#[test]
fn the_person_learns_how_the_bot_answered_its_lay_and_what_a_call_showed() {
    use believe::traps::learn;
    let params = Params::default();
    let mut t = round(&params, &[(0, 2), (2, 3)], &[(1, 3)], &[(4, 1)], 0);
    t.last_by = 1;
    t.laid = [0, t.laid[0]];
    t.to_act = 0;
    let mut p = trusting();
    p.saw_answer(true);
    learn(&mut p, &params, &t, 0, &lay(0, 0, 1), 0);
    assert!((p.bluff_rate() - 0.8 * (1.0 - 0.25)).abs() < EPS);
    learn(&mut p, &params, &t, 0, &Move::Call, 0);
    assert!((p.bluff_rate() - 0.8 * (1.0 - 0.625)).abs() < EPS);
    let lie = round(&params, &[(0, 2)], &[(1, 3)], &[(4, 1)], 0);
    let before = p.call_rate();
    learn(&mut p, &params, &lie, 1, &Move::Call, 0);
    assert!(p.call_rate() > before);
    let truth = round(&params, &[(2, 2)], &[(1, 3)], &[(0, 1)], 0);
    let before = p.call_rate();
    learn(&mut p, &params, &truth, 1, &Move::Call, 0);
    assert!(p.call_rate() < before);
}
