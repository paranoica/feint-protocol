use believe::ledger::{Fact, Ledger};
use believe::{Counts, MAX_RANKS, Move, Moves, Outcome, Params, Table, by_rank};

fn counts(v: &[u8]) -> Counts {
    let mut c = [0; MAX_RANKS];
    c[..v.len()].copy_from_slice(v);
    c
}

fn lay(claim: u8, v: &[u8]) -> Move {
    Move::Lay {
        claim,
        counts: counts(v),
    }
}

#[test]
fn an_honest_batch_called_goes_to_the_doubter() {
    let params = Params::default();
    let mut l = Ledger::new(&params);
    assert_eq!(l.cards(), [8, 8]);
    assert_eq!(
        l.record(0, &lay(2, &[0, 0, 2])),
        vec![Fact::Lay {
            seat: 0,
            claim: 2,
            counts: counts(&[0, 0, 2])
        }]
    );
    assert_eq!((l.cards(), l.pile()), ([6, 8], 2));
    l.record(1, &lay(2, &[1]));
    assert_eq!((l.cards(), l.pile()), ([6, 7], 3));
    assert_eq!(
        l.record(0, &Move::Call),
        vec![Fact::Call {
            seat: 0,
            layer: 1,
            claim: 2,
            honest: false,
            taker: Some(1),
            revealed: counts(&[1]),
        }]
    );
    assert_eq!((l.cards(), l.pile()), ([6, 10], 0));
    l.record(1, &lay(4, &[0, 0, 0, 0, 3]));
    assert_eq!(
        l.record(0, &Move::Call),
        vec![Fact::Call {
            seat: 0,
            layer: 1,
            claim: 4,
            honest: true,
            taker: Some(0),
            revealed: counts(&[0, 0, 0, 0, 3]),
        }]
    );
    assert_eq!((l.cards(), l.pile()), ([9, 7], 0));
}

#[test]
fn a_call_with_nothing_laid_records_nothing() {
    let mut l = Ledger::new(&Params::default());
    assert!(l.record(0, &Move::Call).is_empty());
    assert_eq!((l.cards(), l.pile()), ([8, 8], 0));
}

fn emptied() -> Ledger {
    let params = Params {
        hand: 3,
        ..Params::default()
    };
    let mut l = Ledger::new(&params);
    l.record(0, &lay(1, &[0, 2, 1]));
    assert_eq!(l.cards(), [0, 3]);
    l
}

#[test]
fn laying_on_an_emptied_hand_lays_nothing_and_the_empty_hand_wins() {
    let mut l = emptied();
    assert_eq!(l.record(1, &lay(1, &[0, 1])), vec![Fact::Out { seat: 0 }]);
    assert_eq!((l.cards(), l.pile()), ([0, 3], 3));
}

#[test]
fn an_honest_last_batch_called_wins_and_nobody_takes_the_pile() {
    let params = Params {
        hand: 2,
        ..Params::default()
    };
    let mut l = Ledger::new(&params);
    l.record(0, &lay(1, &[0, 2]));
    assert_eq!(
        l.record(1, &Move::Call),
        vec![
            Fact::Call {
                seat: 1,
                layer: 0,
                claim: 1,
                honest: true,
                taker: None,
                revealed: counts(&[0, 2]),
            },
            Fact::Out { seat: 0 },
        ]
    );
    assert_eq!((l.cards(), l.pile()), ([0, 2], 2));
}

#[test]
fn a_false_last_batch_called_goes_back_to_the_layer() {
    let mut l = emptied();
    assert_eq!(
        l.record(1, &Move::Call),
        vec![Fact::Call {
            seat: 1,
            layer: 0,
            claim: 1,
            honest: false,
            taker: Some(0),
            revealed: counts(&[0, 2, 1]),
        }]
    );
    assert_eq!((l.cards(), l.pile()), ([3, 3], 0));
}

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn the_ledger_agrees_with_the_rules_over_random_matches() {
    let mut outs = 0;
    for hand in [8, 3, 2] {
        let params = Params {
            hand,
            ..Params::default()
        };
        let moves = Moves::new(&params);
        for seed in 0..300 {
            let mut table = Table::deal(&params, seed);
            let mut ledger = Ledger::new(&params);
            let mut rng = seed * 2 + 1;
            while table.outcome.is_none() {
                let p = table.to_act;
                let legal: Vec<i32> = table
                    .legal(&params, &moves, p)
                    .iter()
                    .enumerate()
                    .filter(|&(_, &l)| l == 1)
                    .map(|(i, _)| i as i32)
                    .collect();
                let index = if rng % 3 == 0 && legal[0] == 0 {
                    0
                } else {
                    legal[(next(&mut rng) % legal.len() as u64) as usize]
                };
                let m = moves.decode(index).unwrap();
                let before = table.clone();
                table.play(&params, p, &m).unwrap();
                let facts = ledger.record(p, &m);
                let held = [table.hands[0].count_ones(), table.hands[1].count_ones()];
                assert_eq!(ledger.cards(), held, "hand {hand} seed {seed}");
                let out = match &m {
                    Move::Lay { .. } => before.out.filter(|&o| o != p),
                    Move::Call => {
                        let claim = before.claim.map_or(0, usize::from);
                        let honest = by_rank(&params, before.last)
                            .iter()
                            .enumerate()
                            .all(|(r, &c)| c == 0 || r == claim);
                        before.out.filter(|&o| honest && o == before.last_by)
                    }
                };
                if let Some(w) = out {
                    assert_eq!(table.outcome, Some(Outcome::Winner(w)));
                }
                assert_eq!(
                    facts.last().and_then(|f| match f {
                        Fact::Out { seat } => Some(*seat),
                        _ => None,
                    }),
                    out,
                    "hand {hand} seed {seed}: {facts:?}"
                );
                outs += usize::from(out.is_some());
                for f in &facts {
                    if let Fact::Call {
                        taker,
                        revealed,
                        honest,
                        claim,
                        ..
                    } = f
                    {
                        assert_eq!(*revealed, by_rank(&params, before.last));
                        assert_eq!(Some(*claim), before.claim);
                        let truthful = revealed
                            .iter()
                            .enumerate()
                            .all(|(r, &c)| c == 0 || r == usize::from(*claim));
                        assert_eq!(*honest, truthful);
                        if let Some(t) = taker {
                            let pile = (before.laid[0] | before.laid[1]).count_ones();
                            let t = usize::from(*t);
                            assert_eq!(held[t], before.hands[t].count_ones() + pile);
                        }
                    }
                }
            }
        }
    }
    assert!(outs > 50, "only {outs} matches ended by an empty hand");
}
