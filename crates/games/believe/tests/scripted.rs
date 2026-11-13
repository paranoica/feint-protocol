use believe::{Believe, Move, Params, Scripted, Style, Table};
use feint_ai::{Controller, Opponent, RunHeader, run_match};
use feint_protocol::{BodyParamsT, GameAdapter, ObservationT};

fn cards(params: &Params, used: u64, ranks: &[(u8, u8)]) -> u64 {
    let mut mask = 0u64;
    for &(rank, n) in ranks {
        let mut taken = 0;
        for copy in 0..params.copies {
            let id = rank * params.copies + copy;
            if (mask | used) & (1 << id) == 0 && taken < n {
                mask |= 1 << id;
                taken += 1;
            }
        }
        assert_eq!(taken, n);
    }
    mask
}

fn header(seed: u64) -> RunHeader {
    RunHeader {
        game_id: "believe".into(),
        game_version: believe::GAME_VERSION,
        seed,
        bodies: vec![BodyParamsT::default(), BodyParamsT::default()],
        levels: [0, 0],
        opponent: Opponent::HandMade,
    }
}

fn styles() -> [Style; 4] {
    [
        Style::Honest,
        Style::Liar(300),
        Style::Caller(300),
        Style::Counter,
    ]
}

fn decide(game: &Believe, style: Style, me: u8, seed: u64) -> Move {
    let mut bot = Scripted::new(style);
    bot.new_match(&game.spec(), me, seed);
    let mut obs = ObservationT::default();
    game.observe(game.live(), me, &mut obs);
    let action = bot.on_tick(game, &obs).expect("it is this player's turn");
    assert_eq!(action.player, me);
    assert_eq!(action.tick, game.live().tick);
    game.moves().decode(action.discrete).unwrap()
}

fn after_a_lay(
    params: &Params,
    mine: &[(u8, u8)],
    theirs: &[(u8, u8)],
    laid: &[(u8, u8)],
    claim: u8,
) -> Believe {
    let a = cards(params, 0, mine);
    let b = cards(params, a, theirs);
    let mut t = Table::empty(params);
    t.hands = [a, b];
    t.seen = [a, b];
    t.aside = ((1u64 << params.deck()) - 1) & !(a | b);
    t.to_act = 1;
    let mut counts = [0u8; 8];
    for &(rank, n) in laid {
        counts[usize::from(rank)] += n;
    }
    t.play(params, 1, &Move::Lay { claim, counts }).unwrap();
    Believe::from_table(params.clone(), t).unwrap()
}

fn is_false(m: &Move) -> bool {
    match m {
        Move::Lay { claim, counts } => counts
            .iter()
            .enumerate()
            .any(|(rank, &c)| c > 0 && rank != usize::from(*claim)),
        Move::Call => false,
    }
}

#[test]
fn honest_never_lays_a_false_card() {
    let params = Params::default();
    for (seed, other) in (0..60).zip(styles().into_iter().cycle()) {
        let mut game = Believe::new(params.clone()).unwrap();
        let mut honest = Scripted::new(Style::Honest);
        let mut rival = Scripted::new(other);
        let report = run_match(&mut game, [&mut honest, &mut rival], header(seed), 10_000).unwrap();
        for a in report.actions.iter().filter(|a| a.player == 0) {
            let m = game.moves().decode(a.discrete).unwrap();
            assert!(!is_false(&m), "seed {seed}: {m:?}");
        }
    }
}

#[test]
fn the_liar_lies_and_the_others_do_not_lie_while_they_hold_the_rank() {
    let params = Params::default();
    let game = after_a_lay(&params, &[(0, 2), (1, 2)], &[(2, 2), (3, 2)], &[(2, 1)], 0);
    let lies = (0..400)
        .filter(|&seed| is_false(&decide(&game, Style::Liar(300), 0, seed)))
        .count();
    assert!((80..=160).contains(&lies), "{lies} of 400 at 300 per mille");
    for style in [Style::Honest, Style::Counter] {
        for seed in 0..50 {
            let m = decide(&game, style, 0, seed);
            assert_eq!(
                m,
                Move::Lay {
                    claim: 0,
                    counts: [2, 0, 0, 0, 0, 0, 0, 0]
                },
                "{style:?}"
            );
        }
    }
}

#[test]
fn the_counter_calls_an_impossible_claim() {
    let params = Params::default();
    let impossible = after_a_lay(&params, &[(0, 3), (1, 1)], &[(2, 2), (3, 2)], &[(2, 2)], 0);
    assert_eq!(decide(&impossible, Style::Counter, 0, 1), Move::Call);
    let possible = after_a_lay(&params, &[(0, 3), (1, 1)], &[(2, 2), (3, 2)], &[(2, 1)], 0);
    assert_ne!(decide(&possible, Style::Counter, 0, 1), Move::Call);
}

#[test]
fn the_counter_counts_its_own_cards_in_the_pile() {
    let params = Params::default();
    let a = cards(&params, 0, &[(0, 3), (1, 1)]);
    let b = cards(&params, a, &[(2, 2), (3, 2)]);
    let mut t = Table::empty(&params);
    t.hands = [a, b];
    t.seen = [a, b];
    t.aside = ((1u64 << params.deck()) - 1) & !(a | b);
    let mut one_nine = [0u8; 8];
    one_nine[0] = 1;
    let mut two_jacks = [0u8; 8];
    two_jacks[2] = 2;
    t.play(
        &params,
        0,
        &Move::Lay {
            claim: 0,
            counts: one_nine,
        },
    )
    .unwrap();
    t.play(
        &params,
        1,
        &Move::Lay {
            claim: 0,
            counts: two_jacks,
        },
    )
    .unwrap();
    let game = Believe::from_table(params, t).unwrap();
    assert_eq!(
        decide(&game, Style::Counter, 0, 1),
        Move::Call,
        "two nines in hand and one in the pile leave one for the opponent, who claimed two"
    );
}

#[test]
fn the_caller_calls_at_its_rate() {
    let params = Params::default();
    let game = after_a_lay(&params, &[(0, 2), (1, 2)], &[(2, 2), (3, 2)], &[(2, 1)], 0);
    let n = 2_000u32;
    let calls = (0..u64::from(n))
        .filter(|&seed| decide(&game, Style::Caller(300), 0, seed) == Move::Call)
        .count() as f64;
    let share = calls / f64::from(n);
    let se = (0.3_f64 * 0.7 / f64::from(n)).sqrt();
    assert!((share - 0.3).abs() < 3.0 * se, "share {share}");
}

#[test]
fn everyone_calls_when_the_opponent_has_laid_their_last_cards() {
    let params = Params::default();
    let game = after_a_lay(&params, &[(0, 2), (1, 2)], &[(2, 1)], &[(2, 1)], 0);
    assert_eq!(game.live().out, Some(1));
    for style in styles() {
        assert_eq!(decide(&game, style, 0, 3), Move::Call, "{style:?}");
    }
}

#[test]
fn a_player_without_the_rank_calls_unless_it_lies() {
    let params = Params::default();
    let game = after_a_lay(&params, &[(1, 2), (2, 1)], &[(3, 2), (4, 2)], &[(3, 1)], 0);
    for style in [Style::Honest, Style::Counter, Style::Caller(0)] {
        assert_eq!(decide(&game, style, 0, 5), Move::Call, "{style:?}");
    }
    assert!(is_false(&decide(&game, Style::Liar(0), 0, 5)));
}

#[test]
fn the_leader_claims_the_rank_it_holds_most() {
    let params = Params::default();
    let a = cards(&params, 0, &[(1, 1), (3, 3), (4, 2)]);
    let b = cards(&params, a, &[(0, 2)]);
    let mut t = Table::empty(&params);
    t.hands = [a, b];
    t.seen = [a, b];
    let game = Believe::from_table(params, t).unwrap();
    for style in styles() {
        assert_eq!(
            decide(&game, style, 0, 2),
            Move::Lay {
                claim: 3,
                counts: [0, 0, 0, 3, 0, 0, 0, 0]
            },
            "{style:?}"
        );
    }
}

#[test]
fn every_scripted_pair_finishes_matches() {
    let params = Params::default();
    for first in styles() {
        for second in styles() {
            for seed in 0..25 {
                let mut game = Believe::new(params.clone()).unwrap();
                let mut a = Scripted::new(first);
                let mut b = Scripted::new(second);
                let report = run_match(&mut game, [&mut a, &mut b], header(seed), 10_000).unwrap();
                assert!(report.finished, "{first:?} vs {second:?} seed {seed}");
                assert!(report.actions.len() <= 300);
            }
        }
    }
}
