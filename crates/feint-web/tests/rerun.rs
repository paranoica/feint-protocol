use airhockey::HockeyPlaybook;
use feint_ai::{Bot, Difficulty, Opponent};
use feint_compare::play::play_against_human;
use feint_compare::trained::play_trained;
use feint_web::{Rerun, Session, SessionError};

fn web(kind: Rerun, level: u8, seed: u64, seat: u8) -> (u64, Vec<i64>, u64) {
    let mut p = Session::rerun_human(kind, level, seed, seat).unwrap();
    assert!(p.spectating(), "a rerun is watched, never uploaded");
    while !p.finished() {
        p.advance(4800);
    }
    let score = p.score().iter().map(|&g| i64::from(g)).collect();
    (p.digest(), score, p.tick())
}

#[test]
fn rerun_of_the_trained_bot_matches_the_native_harness_in_both_seats() {
    for (seed, seat) in [(30_030, 0), (30_081, 1)] {
        let native = play_trained(5, seed, seat);
        let r = native.report;
        assert_eq!(
            web(Rerun::Trained, 5, seed, seat),
            (r.digest, r.score, r.ticks),
            "seed {seed} seat {seat}"
        );
    }
}

#[test]
fn rerun_of_the_hand_made_bot_matches_the_native_harness_in_both_seats() {
    hand_made_parity(&[(1, 40, 0), (1, 41, 1)]);
}

#[test]
#[ignore = "release only: bedrock cmd wasm-check"]
fn a_rerun_stops_at_the_native_cap() {
    hand_made_parity(&[(2, 6, 0)]);
}

fn hand_made_parity(cases: &[(u8, u64, u8)]) {
    for &(level, seed, seat) in cases {
        let d = Difficulty::preset(level).unwrap();
        let native = play_against_human(
            |spec, params| {
                let bot = Bot::new(d, HockeyPlaybook::new(params.clone()), spec).unwrap();
                (bot, d.body(spec), Opponent::HandMade)
            },
            seed,
            seat,
        );
        let r = native.report;
        assert_eq!(r.finished, seed != 6, "seed 6 at level 2 runs into the cap");
        assert_eq!(
            web(Rerun::HandMade, level, seed, seat),
            (r.digest, r.score, r.ticks),
            "seed {seed} seat {seat}"
        );
    }
}

#[test]
fn rerun_refuses_a_bad_level_or_seat() {
    for (level, seat) in [(0, 0), (6, 0), (5, 2)] {
        assert!(
            matches!(
                Session::rerun_human(Rerun::Trained, level, 1, seat),
                Err(SessionError::Level)
            ),
            "level {level} seat {seat}"
        );
    }
}

#[test]
fn a_rerun_has_no_record() {
    let p = Session::rerun_human(Rerun::Trained, 5, 30_030, 0).unwrap();
    assert!(p.record().is_empty());
    assert!(p.profile().is_none());
}
