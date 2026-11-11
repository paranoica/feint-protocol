use std::sync::Mutex;

use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Controller, Difficulty, Opponent, RunHeader, run_match};
use feint_ml::registry::CHECKPOINTS;
use feint_ml::trained::trained_pilot;
use feint_protocol::GameAdapter;

const LEVEL: u8 = 5;
const FIRST_SEED: u64 = 10_000;
const MATCHES: u64 = 200;
const CAP_TICKS: u64 = 240 * 600;
const BAR: u32 = 112;

#[derive(Clone, Copy, Default)]
struct Tally {
    won: u32,
    lost: u32,
    ahead: u32,
    behind: u32,
    level: u32,
    mine: i64,
    theirs: i64,
}

fn play(seed: u64, checkpoint: u64) -> (i64, i64, bool) {
    let mut game = AirHockey::new(Params::default(), LEVEL);
    let spec = game.spec();
    let difficulty = Difficulty::preset(LEVEL).expect("level 5");
    let body = difficulty.body(&spec);
    let mut pilot = trained_pilot(LEVEL, checkpoint, &spec).expect("the trained pilot");
    let mut bot = Bot::new(difficulty, HockeyPlaybook::new(Params::default()), &spec)
        .expect("the hand-made bot");
    let trained_first = (seed - FIRST_SEED).is_multiple_of(2);
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed,
        bodies: vec![body.clone(), body],
        levels: [LEVEL, LEVEL],
        opponent: Opponent::HandMade,
    };
    let seats: [&mut dyn Controller<AirHockey>; 2] = if trained_first {
        [&mut pilot, &mut bot]
    } else {
        [&mut bot, &mut pilot]
    };
    let report = run_match(&mut game, seats, header, CAP_TICKS).expect("the match runs");
    let me = usize::from(!trained_first);
    (report.score[me], report.score[1 - me], report.finished)
}

fn wilson(won: u32, n: u32) -> (f64, f64) {
    let z = 1.959_963_984_540_054;
    let p = f64::from(won) / f64::from(n);
    let n = f64::from(n);
    let centre = (p + z * z / (2.0 * n)) / (1.0 + z * z / n);
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / (1.0 + z * z / n);
    (centre - half, centre + half)
}

#[test]
fn wilson_matches_a_worked_example() {
    let (lo, hi) = wilson(112, 200);
    assert!((lo - 0.4907).abs() < 5e-4, "{lo}");
    assert!((hi - 0.6270).abs() < 5e-4, "{hi}");
}

#[test]
#[ignore]
fn bar() {
    let checkpoint = &CHECKPOINTS[0];
    let tally = Mutex::new(Tally::default());
    let next = Mutex::new(FIRST_SEED);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let seed = {
                        let mut next = next.lock().unwrap();
                        if *next >= FIRST_SEED + MATCHES {
                            return;
                        }
                        *next += 1;
                        *next - 1
                    };
                    let (mine, theirs, finished) = play(seed, checkpoint.id);
                    let mut t = tally.lock().unwrap();
                    t.mine += mine;
                    t.theirs += theirs;
                    match (finished, mine.cmp(&theirs)) {
                        (true, std::cmp::Ordering::Greater) => t.won += 1,
                        (true, _) => t.lost += 1,
                        (false, std::cmp::Ordering::Greater) => t.ahead += 1,
                        (false, std::cmp::Ordering::Less) => t.behind += 1,
                        (false, std::cmp::Ordering::Equal) => t.level += 1,
                    }
                }
            });
        }
    });
    let t = tally.into_inner().unwrap();
    let played = t.won + t.lost + t.ahead + t.behind + t.level;
    assert_eq!(u64::from(played), MATCHES);
    let (lo, hi) = wilson(t.won, played);
    eprintln!(
        "bar: {} (level 5, equal body, 150 ms, seeds {FIRST_SEED}..{}, seats alternating, empty profile, cap {} s)",
        checkpoint.name,
        FIRST_SEED + MATCHES - 1,
        CAP_TICKS / 240
    );
    eprintln!(
        "won {}/{played}, lost {}, capped {} ({} ahead, {} behind, {} level), goals {}:{}, Wilson 95% [{lo:.3}, {hi:.3}], bar {BAR}: {}",
        t.won,
        t.lost,
        t.ahead + t.behind + t.level,
        t.ahead,
        t.behind,
        t.level,
        t.mine,
        t.theirs,
        if t.won >= BAR { "met" } else { "not met" }
    );
}
