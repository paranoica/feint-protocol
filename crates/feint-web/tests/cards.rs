use believe::{Believe, Params, Table};
use feint_protocol::{GameAdapter, ObservationT};
use feint_web::cards::{CARD_CAP_TICKS, CardEvent, CardMatch, CardView};

const SHIPPED: u64 = believe::trained::CHECKPOINTS[0].id;

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn play_out(seed: u64) -> CardMatch {
    let mut m = CardMatch::new(seed, SHIPPED).unwrap();
    let mut rng = seed | 1;
    let mut ticks = 0;
    while !m.finished() && ticks < CARD_CAP_TICKS {
        if m.view().my_turn {
            let legal = m.legal();
            assert!(!legal.is_empty(), "my turn with no legal move");
            let pick = legal[(next(&mut rng) % legal.len() as u64) as usize];
            assert!(m.play(pick));
        }
        m.advance(1);
        ticks += 1;
    }
    m
}

#[test]
fn a_card_match_plays_to_the_end_by_legal_human_moves() {
    for seed in 3..23 {
        let m = play_out(seed);
        assert!(m.finished(), "seed {seed}");
        let view = m.view();
        assert!(!view.my_turn, "seed {seed}");
        let record = feint_ai::decode_record(&m.record()).unwrap();
        let expected = match record.score[0].cmp(&record.score[1]) {
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Less => -1,
        };
        assert_eq!(view.outcome, Some(expected), "seed {seed}");
    }
}

#[test]
fn an_idle_human_ends_the_match_at_the_tick_cap() {
    let mut m = CardMatch::new(8, SHIPPED).unwrap();
    assert!(m.view().my_turn);
    let legal = m.legal();
    m.advance(u32::try_from(CARD_CAP_TICKS).unwrap() + 10);
    assert!(m.finished());
    assert!(m.legal().is_empty());
    assert!(legal.iter().all(|&i| !m.play(i)));
    let record = feint_ai::decode_record(&m.record()).unwrap();
    assert_eq!(record.ticks, CARD_CAP_TICKS);
}

#[test]
fn play_queues_and_advance_applies() {
    let mut m = CardMatch::new(7, SHIPPED).unwrap();
    while !m.view().my_turn {
        m.advance(1);
    }
    let before = m.view();
    let pick = m.legal()[0];
    assert!(m.play(pick));
    assert_eq!(m.view(), before, "queued, not applied");
    assert!(!m.play(pick), "one move queued at a time");
    m.advance(1);
    let after = m.view();
    assert_ne!(after.events, before.events);
    assert!(
        after.events[before.events.len()..]
            .iter()
            .any(|e| e.seat() == 0),
        "my queued move is among the new events"
    );
}

#[test]
fn an_illegal_or_untimely_move_is_refused_and_changes_nothing() {
    let (seed, mut m) = (0..64)
        .map(|s| (s, CardMatch::new(s, SHIPPED).unwrap()))
        .find(|(_, m)| !m.view().my_turn)
        .expect("a deal where the bot leads");
    let opening = m.view();
    assert!(
        m.legal().is_empty(),
        "seed {seed}: no human moves on the bot's turn"
    );
    assert!((0..276).all(|i| !m.play(i)), "seed {seed}: not my turn");
    assert_eq!(m.view(), opening);
    m.advance(1);
    let now = m.view();
    assert!(now.my_turn && !m.finished(), "seed {seed}");
    let legal = m.legal();
    let illegal = (0..276).find(|i| !legal.contains(i)).unwrap();
    assert!(!m.play(illegal));
    assert!(!m.play(-1));
    assert!(!m.play(10_000));
    assert_eq!(m.view(), now);
    assert!(m.play(legal[0]));

    let mut done = play_out(seed);
    assert!(done.finished());
    let end = done.view();
    assert!(done.legal().is_empty());
    assert!(!done.play(0));
    done.advance(5);
    assert_eq!(done.view(), end);
}

#[test]
fn a_call_reveals_the_called_batch_and_who_takes_the_pile() {
    let mut seen = 0;
    for seed in 10..40 {
        let m = play_out(seed);
        let events = m.view().events;
        for (i, e) in events.iter().enumerate() {
            let CardEvent::Call {
                seat,
                honest,
                taker,
                revealed,
            } = e
            else {
                continue;
            };
            let Some(CardEvent::Lay {
                seat: layer,
                claim,
                count,
            }) = events[..i].last()
            else {
                panic!("a call follows a lay: {events:?}");
            };
            assert_eq!(
                revealed.iter().map(|&c| u32::from(c)).sum::<u32>(),
                u32::from(*count)
            );
            let truthful = revealed
                .iter()
                .enumerate()
                .all(|(rank, &c)| c == 0 || rank == usize::from(*claim));
            assert_eq!(*honest, truthful);
            if taker.is_none() {
                assert!(truthful, "only an honest last batch ends the match");
                assert_eq!(i + 2, events.len());
                assert_eq!(events[i + 1], CardEvent::Out { seat: *layer });
            } else {
                assert_eq!(*taker, Some(if truthful { *seat } else { *layer }));
            }
            seen += 1;
        }
    }
    assert!(seen > 5, "only {seen} calls seen");
}

fn swapped_bot_hand(seed: u64) -> (Table, Table) {
    let params = Params::default();
    let a = Table::deal(&params, seed);
    let mut b = a.clone();
    let rank = |id: u32| params.rank_of(id);
    let bot: Vec<u32> = (0..64).filter(|&i| a.hands[1] & (1 << i) != 0).collect();
    let aside: Vec<u32> = (0..64).filter(|&i| a.aside & (1 << i) != 0).collect();
    let (x, y) = bot
        .iter()
        .flat_map(|&x| aside.iter().map(move |&y| (x, y)))
        .find(|&(x, y)| rank(x) != rank(y))
        .expect("an aside card of another rank");
    b.hands[1] = (b.hands[1] & !(1 << x)) | (1 << y);
    b.aside = (b.aside & !(1 << y)) | (1 << x);
    (a, b)
}

#[test]
fn the_view_depends_on_the_seats_observation_only() {
    let params = Params::default();
    for seed in 0..20 {
        let (a, b) = swapped_bot_hand(seed);
        assert_ne!(a.hands[1], b.hands[1]);
        let views: Vec<CardView> = [a, b]
            .into_iter()
            .map(|table| {
                let game = Believe::from_table(params.clone(), table).unwrap();
                let mut obs = ObservationT::default();
                game.observe(game.live(), 0, &mut obs);
                CardView::from_observation(&obs, &params)
            })
            .collect();
        assert_eq!(views[0], views[1], "seed {seed}");
    }
}

#[test]
fn the_view_holds_the_seats_observation() {
    let params = Params::default();
    let table = Table::deal(&params, 5);
    let game = Believe::from_table(params.clone(), table.clone()).unwrap();
    let mut obs = ObservationT::default();
    game.observe(game.live(), 0, &mut obs);
    let view = CardView::from_observation(&obs, &params);
    let hand = believe::by_rank(&params, table.hands[0]);
    assert_eq!(view.hand, hand[..5].to_vec());
    assert_eq!(view.known, vec![0; 5]);
    assert_eq!(view.their_cards, table.hands[1].count_ones());
    assert_eq!(view.actions_left, params.cap_actions);
    assert_eq!(view.claim, None);
    assert_eq!(view.pile, 0);
}

#[test]
fn the_record_replays_to_the_same_end() {
    let m = play_out(21);
    let record = feint_ai::decode_record(&m.record()).unwrap();
    assert_eq!(record.header.game_id, "believe");
    assert_eq!(record.header.levels, [0, 5]);
    assert_eq!(
        record.header.opponent,
        feint_ai::Opponent::Trained(std::num::NonZeroU64::new(SHIPPED).unwrap())
    );
    let mut game = Believe::default();
    let digest = feint_ai::replay_record(&mut game, &record).unwrap();
    assert_eq!(digest, m.digest());
}

#[test]
fn an_unknown_checkpoint_is_refused() {
    assert!(CardMatch::new(1, 1).is_err());
}

#[test]
fn the_outcome_compares_seat_zeros_score_with_the_bots() {
    let params = Params::default();
    for (score, outcome) in [([3, 1], Some(1)), ([1, 1], Some(0)), ([0, 2], Some(-1))] {
        let mut obs = ObservationT::default();
        obs.to_act = -1;
        obs.score = Some(score.to_vec());
        assert_eq!(CardView::from_observation(&obs, &params).outcome, outcome);
        obs.to_act = 0;
        assert_eq!(CardView::from_observation(&obs, &params).outcome, None);
    }
}

#[test]
fn each_legal_move_describes_a_call_or_a_lay_from_the_hand() {
    let params = Params::default();
    let ranks = usize::from(params.ranks);
    let mut m = CardMatch::new(11, SHIPPED).unwrap();
    assert_eq!(m.ranks(), params.ranks);
    assert_eq!(m.describe(0), vec![-1]);
    assert!(m.describe(-1).is_empty());
    let total = i32::try_from(believe::Moves::new(&params).len()).unwrap();
    assert!(m.describe(total).is_empty());
    assert_eq!(m.describe(total - 1).len(), 1 + ranks);
    let mut turns = 0;
    while !m.finished() && turns < 6 {
        if m.view().my_turn {
            let view = m.view();
            let mut seen = std::collections::BTreeSet::new();
            for index in m.legal() {
                let d = m.describe(index);
                assert!(seen.insert(d.clone()), "two moves share {d:?}");
                if d == [-1] {
                    assert!(view.batches > 0, "a call needs a lay to call");
                    continue;
                }
                assert_eq!(d.len(), 1 + ranks);
                let claim = d[0];
                assert!((0..i32::from(params.ranks)).contains(&claim));
                if view.batches > 0 {
                    assert_eq!(Some(claim), view.claim.map(i32::from), "the claim stands");
                }
                let laid: i32 = d[1..].iter().sum();
                assert!((1..=i32::from(params.max_batch)).contains(&laid));
                for (rank, &c) in d[1..].iter().enumerate() {
                    assert!(
                        c >= 0 && c <= i32::from(view.hand[rank]),
                        "{d:?} {:?}",
                        view.hand
                    );
                }
            }
            let pick = m.legal()[0];
            assert!(m.play(pick));
            turns += 1;
        }
        m.advance(1);
    }
    assert_eq!(turns, 6);
}

#[test]
fn events_flatten_to_a_fixed_stride() {
    let ranks = 5;
    let lay = CardEvent::Lay {
        seat: 1,
        claim: 3,
        count: 2,
    };
    assert_eq!(lay.flat(ranks), vec![0, 1, 3, 2, 0, 0, 0, 0, 0]);
    let call = CardEvent::Call {
        seat: 0,
        honest: false,
        taker: Some(1),
        revealed: vec![0, 1, 0, 1, 0],
    };
    assert_eq!(call.flat(ranks), vec![1, 0, 0, 1, 0, 1, 0, 1, 0]);
    let honest = CardEvent::Call {
        seat: 1,
        honest: true,
        taker: Some(1),
        revealed: vec![2],
    };
    assert_eq!(honest.flat(ranks), vec![1, 1, 1, 1, 2, 0, 0, 0, 0]);
    let last = CardEvent::Call {
        seat: 0,
        honest: true,
        taker: None,
        revealed: vec![0, 0, 3],
    };
    assert_eq!(last.flat(ranks), vec![1, 0, 1, -1, 0, 0, 3, 0, 0]);
    let out = CardEvent::Out { seat: 1 };
    assert_eq!(out.flat(ranks), vec![2, 1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(feint_web::cards::event_stride(ranks), 9);
}

#[test]
fn a_match_won_by_an_empty_hand_ends_with_that_seat_out() {
    let (mut human, mut bot) = (0, 0);
    for seed in 40..100 {
        let m = play_out(seed);
        let view = m.view();
        let events = &view.events;
        let outs: Vec<usize> = events
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, CardEvent::Out { .. }))
            .map(|(i, _)| i)
            .collect();
        let emptied = if view.hand.iter().all(|&c| c == 0) {
            Some(0)
        } else if view.their_cards == 0 {
            Some(1)
        } else {
            None
        };
        assert!(
            outs.iter().all(|&i| i + 1 == events.len()),
            "seed {seed}: {events:?}"
        );
        if let Some(CardEvent::Out { seat }) = events.last() {
            assert_eq!(emptied, Some(*seat), "seed {seed}");
            assert_eq!(view.outcome, Some(if *seat == 0 { 1 } else { -1 }));
            if *seat == 0 { human += 1 } else { bot += 1 }
        } else if view.actions_left > 0 {
            assert_eq!(emptied, None, "seed {seed}: an empty hand ends with Out");
        }
    }
    assert!(bot > 5, "only {bot} matches won by the bot's empty hand");
    let _ = human;
}
