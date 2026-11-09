use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Controller, Difficulty, RunHeader, RunReport, run_match};
use feint_protocol::{ActionT, GameAdapter, GameSpecT, ObservationT, PlayerId};

const GOALS: u32 = 3;
const CAP_TICKS: u64 = 240 * 180;

fn params() -> Params {
    Params {
        goals_to_win: GOALS,
        ..Params::default()
    }
}

fn bot(level: u8, game: &AirHockey) -> Bot<AirHockey, HockeyPlaybook> {
    let d = Difficulty::preset(level).unwrap();
    Bot::new(d, HockeyPlaybook::new(params()), &game.spec()).unwrap()
}

struct Idle;

impl Controller<AirHockey> for Idle {
    fn new_match(&mut self, _spec: &GameSpecT, _me: PlayerId, _seed: u64) {}

    fn on_tick(&mut self, _game: &AirHockey, _obs: &ObservationT) -> Option<ActionT> {
        None
    }
}

fn play(seed: u64, levels: [u8; 2]) -> RunReport {
    let mut game = AirHockey::new(params(), levels[0].max(1));
    let peak = game.live().bodies[0];
    let bodies = levels
        .iter()
        .map(|&l| match Difficulty::preset(l) {
            Some(d) => d.body(&game.spec()),
            None => {
                let mut b = feint_protocol::BodyParamsT::default();
                b.values = Some(vec![peak.max_speed() as f32, peak.max_accel() as f32]);
                b
            }
        })
        .collect();
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed,
        bodies,
        levels,
    };
    let mut sides: Vec<Box<dyn Controller<AirHockey>>> = levels
        .iter()
        .map(|&l| -> Box<dyn Controller<AirHockey>> {
            if l == 0 {
                Box::new(Idle)
            } else {
                Box::new(bot(l, &game))
            }
        })
        .collect();
    let (a, b) = sides.split_at_mut(1);
    run_match(&mut game, [a[0].as_mut(), b[0].as_mut()], header, CAP_TICKS).unwrap()
}

fn won(report: &RunReport, me: usize) -> bool {
    report.finished && report.score[me] > report.score[1 - me]
}

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn level_two_beats_level_one() {
    let mut wins = 0;
    for seed in 0..30 {
        wins += usize::from(won(&play(seed, [2, 1]), 0));
        wins += usize::from(won(&play(seed, [1, 2]), 1));
    }
    eprintln!("level 2 won {wins} of 60 against level 1");
    assert!(wins >= 36, "level 2 won only {wins} of 60 against level 1");
}

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn level_one_does_not_beat_level_two_easily() {
    let mut wins = 0;
    for seed in 0..30 {
        wins += usize::from(won(&play(seed, [1, 2]), 0));
        wins += usize::from(won(&play(seed, [2, 1]), 1));
    }
    eprintln!("level 1 won {wins} of 60 against level 2");
    assert!(
        wins <= 24,
        "level 1 won {wins} of 60: the levels are not ordered"
    );
}

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn level_two_scores_against_an_idle_opponent() {
    let scored = (0..30)
        .filter(|&seed| play(seed, [2, 0]).score[0] > 0)
        .count();
    eprintln!("level 2 scored in {scored} of 30 matches against an idle opponent");
    assert!(scored >= 24, "level 2 scored in only {scored} of 30");
}
