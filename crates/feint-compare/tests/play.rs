use feint_ai::Opponent;
use feint_compare::play::{Played, play_against_human};
use feint_ml::registry::for_level;
use feint_ml::trained::trained_pilot;
use std::num::NonZeroU64;

fn trained(seed: u64, seat: u8) -> Played<feint_ml::trained::TrainedPilot> {
    let checkpoint = for_level(5).unwrap().id;
    play_against_human(
        |spec, _| {
            let pilot = trained_pilot(5, checkpoint, spec).unwrap();
            let body = feint_ai::Difficulty::preset(5).unwrap().body(spec);
            (
                pilot,
                body,
                Opponent::Trained(NonZeroU64::new(checkpoint).unwrap()),
            )
        },
        seed,
        seat,
    )
}

#[test]
fn the_winning_goal_is_credited() {
    let p = trained(30_000, 0);
    assert!(
        p.report.finished,
        "level 5 trained beats the model inside the cap"
    );
    let last = p.frames.last().unwrap();
    let score: Vec<i64> = last.score.iter().map(|&g| i64::from(g)).collect();
    assert_eq!(
        score, p.report.score,
        "the last frame carries the final score"
    );
    assert_eq!(
        last.score[0], 7,
        "seat 0 is the trained bot and it reaches 7"
    );
    assert_eq!(last.tick, p.report.ticks);
}

#[test]
fn the_same_seed_and_seat_rerun_the_same_match() {
    let a = trained(30_001, 1);
    let b = trained(30_001, 1);
    assert_eq!(a.report.digest, b.report.digest);
    assert_eq!(a.frames, b.frames);
    let other = trained(30_001, 0);
    assert_ne!(
        a.report.digest, other.report.digest,
        "the seat changes the match"
    );
}

#[test]
fn frames_run_one_per_tick() {
    let p = trained(30_002, 1);
    assert_eq!(p.frames[0].tick, 0);
    assert!(p.frames.windows(2).all(|w| w[1].tick == w[0].tick + 1));
}
