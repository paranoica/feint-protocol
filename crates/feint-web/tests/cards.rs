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
            assert_eq!(*taker, if truthful { *seat } else { *layer });
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
