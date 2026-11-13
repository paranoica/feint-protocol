use believe::{Move, Moves, Outcome, Params, Table, by_rank};

fn p() -> Params {
    Params::default()
}

fn cards(params: &Params, ranks: &[(u8, u8)]) -> u64 {
    let mut mask = 0u64;
    for &(rank, n) in ranks {
        let mut taken = 0;
        for copy in 0..params.copies {
            let id = rank * params.copies + copy;
            if mask & (1 << id) == 0 && taken < n {
                mask |= 1 << id;
                taken += 1;
            }
        }
        assert_eq!(taken, n, "not enough copies of rank {rank}");
    }
    mask
}

fn counts(pairs: &[(u8, u8)]) -> [u8; 8] {
    let mut c = [0u8; 8];
    for &(rank, n) in pairs {
        c[usize::from(rank)] += n;
    }
    c
}

fn lay(claim: u8, pairs: &[(u8, u8)]) -> Move {
    Move::Lay {
        claim,
        counts: counts(pairs),
    }
}

fn table(params: &Params, first: u64, second: u64, to_act: u8) -> Table {
    assert_eq!(first & second, 0, "the hands share a card");
    let mut t = Table::empty(params);
    t.hands = [first, second];
    t.seen = [first, second];
    t.to_act = to_act;
    t
}

#[test]
fn the_action_table_has_276_entries_and_round_trips() {
    let params = p();
    let moves = Moves::new(&params);
    assert_eq!(moves.per_claim(), 5 + 15 + 35);
    assert_eq!(moves.len(), 1 + 5 * 55);
    assert_eq!(moves.index(&Move::Call), Some(0));
    assert_eq!(moves.decode(0), Some(Move::Call));
    for index in 1..moves.len() as i32 {
        let m = moves.decode(index).unwrap();
        assert_eq!(moves.index(&m), Some(index));
        let Move::Lay { claim, counts } = m else {
            panic!("index {index} is not a lay");
        };
        let total: u8 = counts.iter().sum();
        assert!((1..=3).contains(&total));
        assert!(claim < 5);
        assert!(counts[5..].iter().all(|&c| c == 0));
    }
    assert_eq!(moves.decode(moves.len() as i32), None);
    assert_eq!(moves.decode(-1), None);
    assert_eq!(moves.index(&lay(5, &[(0, 1)])), None);
    assert_eq!(moves.index(&lay(0, &[(0, 4)])), None);
}

#[test]
fn a_true_batch_sends_the_pile_to_the_doubter() {
    let params = p();
    let a = cards(&params, &[(0, 2), (1, 1)]);
    let b = cards(&params, &[(2, 2), (3, 1)]);
    let mut t = table(&params, a, b, 0);
    t.play(&params, 0, &lay(0, &[(0, 2)])).unwrap();
    t.play(&params, 1, &Move::Call).unwrap();
    assert_eq!(
        by_rank(&params, t.hands[1]),
        counts(&[(0, 2), (2, 2), (3, 1)])
    );
    assert_eq!(by_rank(&params, t.hands[0]), counts(&[(1, 1)]));
    assert_eq!(t.to_act, 0, "the one who was right leads");
    assert_eq!(t.claim, None);
    assert_eq!(t.batches, 0);
    assert_eq!(t.laid, [0, 0]);
}

#[test]
fn a_false_card_sends_the_pile_to_the_liar() {
    let params = p();
    let a = cards(&params, &[(0, 1), (1, 2)]);
    let b = cards(&params, &[(2, 2), (3, 1)]);
    let mut t = table(&params, a, b, 0);
    t.play(&params, 0, &lay(0, &[(0, 1)])).unwrap();
    t.play(&params, 1, &lay(0, &[(2, 1)])).unwrap();
    t.play(&params, 0, &Move::Call).unwrap();
    assert_eq!(
        by_rank(&params, t.hands[1]),
        counts(&[(0, 1), (2, 2), (3, 1)])
    );
    assert_eq!(by_rank(&params, t.hands[0]), counts(&[(1, 2)]));
    assert_eq!(t.to_act, 0, "the doubter was right and leads");
    assert_eq!(t.outcome, None);
}

#[test]
fn laying_after_the_last_batch_loses_at_once() {
    let params = p();
    let a = cards(&params, &[(0, 2)]);
    let b = cards(&params, &[(2, 2), (3, 1)]);
    let mut t = table(&params, a, b, 0);
    t.play(&params, 0, &lay(0, &[(0, 2)])).unwrap();
    assert_eq!(t.out, Some(0));
    let before = t.hands[1];
    t.play(&params, 1, &lay(0, &[(2, 1)])).unwrap();
    assert_eq!(t.outcome, Some(Outcome::Winner(0)));
    assert_eq!(t.hands[1], before, "the losing batch is not laid");
}

#[test]
fn a_true_last_batch_called_wins() {
    let params = p();
    let a = cards(&params, &[(0, 2)]);
    let b = cards(&params, &[(2, 2)]);
    let mut t = table(&params, a, b, 0);
    t.play(&params, 0, &lay(0, &[(0, 2)])).unwrap();
    t.play(&params, 1, &Move::Call).unwrap();
    assert_eq!(t.outcome, Some(Outcome::Winner(0)));
}

#[test]
fn a_false_last_batch_called_plays_on() {
    let params = p();
    let a = cards(&params, &[(1, 2)]);
    let b = cards(&params, &[(2, 2)]);
    let mut t = table(&params, a, b, 0);
    t.play(&params, 0, &lay(0, &[(1, 2)])).unwrap();
    t.play(&params, 1, &Move::Call).unwrap();
    assert_eq!(t.outcome, None);
    assert_eq!(t.out, None);
    assert_eq!(by_rank(&params, t.hands[0]), counts(&[(1, 2)]));
    assert_eq!(t.to_act, 1);
}

#[test]
fn the_cap_gives_the_match_to_fewer_cards() {
    let mut params = p();
    params.cap_actions = 2;
    let a = cards(&params, &[(0, 3)]);
    let b = cards(&params, &[(2, 2), (3, 1)]);
    let mut t = table(&params, a, b, 0);
    t.play(&params, 0, &lay(0, &[(0, 2)])).unwrap();
    assert_eq!(t.outcome, None);
    t.play(&params, 1, &lay(0, &[(2, 1)])).unwrap();
    assert_eq!(t.outcome, Some(Outcome::Winner(0)), "1 card against 2");
}

#[test]
fn an_equal_cap_is_a_draw() {
    let mut params = p();
    params.cap_actions = 2;
    let a = cards(&params, &[(0, 3)]);
    let b = cards(&params, &[(2, 3)]);
    let mut t = table(&params, a, b, 0);
    t.play(&params, 0, &lay(0, &[(0, 1)])).unwrap();
    t.play(&params, 1, &lay(0, &[(2, 1)])).unwrap();
    assert_eq!(t.outcome, Some(Outcome::Draw));
    assert_eq!(t.score(), [0, 0]);
}

#[test]
fn mid_round_only_the_claimed_rank_is_legal() {
    let params = p();
    let moves = Moves::new(&params);
    let a = cards(&params, &[(0, 2), (1, 1)]);
    let b = cards(&params, &[(2, 2), (3, 1)]);
    let mut t = table(&params, a, b, 0);
    t.play(&params, 0, &lay(1, &[(0, 1)])).unwrap();
    let legal = t.legal(&params, &moves, 1);
    assert_eq!(legal.len(), moves.len());
    assert_eq!(legal[0], 1, "call is legal mid-round");
    for (index, &allowed) in legal.iter().enumerate().skip(1) {
        let Some(Move::Lay { claim, .. }) = moves.decode(index as i32) else {
            panic!("index {index} is not a lay");
        };
        if claim != 1 {
            assert_eq!(allowed, 0, "claim {claim} at index {index}");
        }
    }
    assert_eq!(legal[moves.index(&lay(1, &[(2, 2)])).unwrap() as usize], 1);
    assert_eq!(
        t.play(&params, 1, &lay(2, &[(2, 1)])),
        Err("the claim is fixed in a round")
    );
    assert!(t.legal(&params, &moves, 0).iter().all(|&l| l == 0));
}

#[test]
fn call_is_illegal_at_the_start_of_a_round() {
    let params = p();
    let moves = Moves::new(&params);
    let t = table(
        &params,
        cards(&params, &[(0, 2)]),
        cards(&params, &[(1, 2)]),
        0,
    );
    assert_eq!(t.legal(&params, &moves, 0)[0], 0);
    let mut copy = t.clone();
    assert_eq!(copy.play(&params, 0, &Move::Call), Err("nothing to call"));
    let any_claim = (0..5u8).all(|claim| {
        t.legal(&params, &moves, 0)[moves.index(&lay(claim, &[(0, 1)])).unwrap() as usize] == 1
    });
    assert!(any_claim, "the leader may claim any rank");
}

#[test]
fn cards_not_in_hand_are_illegal() {
    let params = p();
    let moves = Moves::new(&params);
    let mut t = table(
        &params,
        cards(&params, &[(0, 2)]),
        cards(&params, &[(1, 2)]),
        0,
    );
    let legal = t.legal(&params, &moves, 0);
    assert_eq!(legal[moves.index(&lay(0, &[(0, 3)])).unwrap() as usize], 0);
    assert_eq!(legal[moves.index(&lay(0, &[(1, 1)])).unwrap() as usize], 0);
    assert_eq!(legal[moves.index(&lay(4, &[(0, 2)])).unwrap() as usize], 1);
    assert_eq!(
        t.play(&params, 0, &lay(0, &[(1, 1)])),
        Err("cards not in hand")
    );
}

#[test]
fn refused_moves_leave_the_table_unchanged() {
    let params = p();
    let mut t = table(
        &params,
        cards(&params, &[(0, 2)]),
        cards(&params, &[(1, 2)]),
        0,
    );
    let before = t.clone();
    assert_eq!(
        t.play(&params, 1, &lay(0, &[(1, 1)])),
        Err("not this player's turn")
    );
    assert_eq!(
        t.play(&params, 0, &lay(0, &[(1, 1)])),
        Err("cards not in hand")
    );
    assert_eq!(t.play(&params, 0, &lay(9, &[(0, 1)])), Err("no such rank"));
    assert_eq!(
        t.play(&params, 0, &lay(0, &[])),
        Err("batch size out of range")
    );
    assert_eq!(
        t.play(&params, 0, &lay(0, &[(0, 1), (1, 3)])),
        Err("batch size out of range")
    );
    assert_eq!(t, before);
    t.outcome = Some(Outcome::Draw);
    let ended = t.clone();
    assert_eq!(
        t.play(&params, 0, &lay(0, &[(0, 1)])),
        Err("the match is over")
    );
    assert_eq!(t, ended);
}

#[test]
fn known_cards_follow_a_pile_the_opponent_took() {
    let params = p();
    let a = cards(&params, &[(0, 2), (1, 2)]);
    let b = cards(&params, &[(2, 2), (3, 2)]);
    let mut t = table(&params, a, b, 0);
    t.play(&params, 0, &lay(0, &[(0, 2)])).unwrap();
    t.play(&params, 1, &lay(0, &[(3, 1)])).unwrap();
    assert_eq!(t.known(&params, 0), [0; 8], "nothing seen of player 1 yet");
    t.play(&params, 0, &Move::Call).unwrap();
    assert_eq!(t.known(&params, 0), counts(&[(0, 2), (3, 1)]));
    assert_eq!(
        t.known(&params, 1),
        [0; 8],
        "player 0 holds nothing player 1 has seen"
    );
    assert_eq!(t.to_act, 0);
    t.play(&params, 0, &lay(1, &[(1, 1)])).unwrap();
    t.play(&params, 1, &lay(1, &[(0, 1)])).unwrap();
    assert_eq!(
        t.known(&params, 0),
        counts(&[(0, 2), (3, 1)]),
        "a known card laid face down is still in player 1's possession"
    );
    t.play(&params, 0, &Move::Call).unwrap();
    assert_eq!(t.known(&params, 0), counts(&[(0, 2), (1, 1), (3, 1)]));
}

#[test]
fn the_seed_decides_who_leads_first() {
    let params = p();
    let mut leaders = [0u32; 2];
    for seed in 0..40 {
        let t = Table::deal(&params, seed);
        leaders[usize::from(t.to_act)] += 1;
        assert_eq!(t.hands[0].count_ones(), 8);
        assert_eq!(t.hands[1].count_ones(), 8);
        assert_eq!(t.aside.count_ones(), 4);
        assert_eq!(t.hands[0] & t.hands[1], 0);
        assert_eq!((t.hands[0] | t.hands[1]) & t.aside, 0);
        assert_eq!(t.seen, t.hands);
        assert_eq!(t, Table::deal(&params, seed));
    }
    assert!(leaders[0] > 0 && leaders[1] > 0, "{leaders:?}");
}

#[test]
fn params_refuse_impossible_decks() {
    assert_eq!(Params::default().validate(), Ok(()));
    let bad = [
        Params { ranks: 1, ..p() },
        Params { ranks: 9, ..p() },
        Params { copies: 0, ..p() },
        Params { hand: 0, ..p() },
        Params { hand: 11, ..p() },
        Params {
            max_batch: 0,
            ..p()
        },
        Params {
            max_batch: 5,
            ..p()
        },
        Params {
            cap_actions: 1,
            ..p()
        },
        Params { tick_hz: 0, ..p() },
        Params {
            ranks: 8,
            copies: 9,
            ..p()
        },
    ];
    for params in bad {
        assert!(params.validate().is_err(), "{params:?} accepted");
    }
}
