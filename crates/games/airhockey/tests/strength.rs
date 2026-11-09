mod opponents;

use airhockey::analysis::{Frame, Tally, attempts, tally};
use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Controller, Difficulty, RunHeader, RunReport, run_match};
use feint_protocol::{ActionT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use opponents::{Human, Reader, Recorder};

const GOALS: u32 = 3;
const CAP_TICKS: u64 = 240 * 600;
const PANEL_BODY_LEVEL: u8 = 3;
const HUMAN_NOISE: f64 = 0.04;

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

struct Aside {
    me: PlayerId,
}

impl Controller<AirHockey> for Aside {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
    }

    fn on_tick(&mut self, _game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = self.me;
        a.values = Some(vec![-0.9, 0.4]);
        Some(a)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Idle,
    Level(u8),
    Reader,
    Anticipator,
    Human,
}

fn controller(side: Side, game: &AirHockey) -> Box<dyn Controller<AirHockey>> {
    match side {
        Side::Idle => Box::new(Idle),
        Side::Level(l) => Box::new(bot(l, game)),
        Side::Reader => Box::new(Reader::new(params(), false)),
        Side::Anticipator => Box::new(Reader::new(params(), true)),
        Side::Human => Box::new(Human::new(params(), HUMAN_NOISE)),
    }
}

fn body(side: Side, game: &AirHockey) -> feint_protocol::BodyParamsT {
    let level = match side {
        Side::Level(l) => l,
        Side::Reader | Side::Anticipator => PANEL_BODY_LEVEL,
        Side::Idle | Side::Human => 5,
    };
    Difficulty::preset(level).unwrap().body(&game.spec())
}

fn record(seed: u64, sides: [Side; 2]) -> (RunReport, Vec<Frame>) {
    let level = |s: Side| match s {
        Side::Level(l) => l,
        _ => 0,
    };
    let levels = sides.map(level);
    let mut game = AirHockey::new(params(), levels[0].max(1));
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed,
        bodies: sides.iter().map(|&s| body(s, &game)).collect(),
        levels,
    };
    let mut first = controller(sides[0], &game);
    let mut second = controller(sides[1], &game);
    let mut recorder = Recorder {
        inner: first.as_mut(),
        frames: Vec::new(),
    };
    let report = run_match(
        &mut game,
        [&mut recorder, second.as_mut()],
        header,
        CAP_TICKS,
    )
    .unwrap();
    (report, recorder.frames)
}

fn play(seed: u64, levels: [u8; 2]) -> RunReport {
    let side = |l: u8| if l == 0 { Side::Idle } else { Side::Level(l) };
    record(seed, levels.map(side)).0
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

fn bot_tally(level: u8, opponent: Side, seeds: std::ops::Range<u64>) -> Tally {
    let mut sum = Tally::default();
    for seed in seeds {
        let me = (seed % 2) as u8;
        let sides = if me == 0 {
            [Side::Level(level), opponent]
        } else {
            [opponent, Side::Level(level)]
        };
        let (report, frames) = record(seed, sides);
        let t = tally(&attempts(&params(), &frames, &report.actions), me);
        sum.fakes += t.fakes;
        sum.shifted += t.shifted;
        sum.fake_goals += t.fake_goals;
        sum.honest += t.honest;
        sum.honest_goals += t.honest_goals;
    }
    sum
}

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn fakes_are_frequent_and_a_human_bites_on_them() {
    for level in [4u8, 5] {
        let t = bot_tally(level, Side::Human, 0..40);
        eprintln!("level {level} against the human model: {t:?}");
        assert!(
            t.fakes >= 80,
            "level {level} played only {} fakes in 40 matches",
            t.fakes
        );
        assert!(
            5 * t.shifted >= 2 * t.fakes,
            "the human model bit on {} of {} fakes",
            t.shifted,
            t.fakes
        );
    }
}

fn wins(level: u8, opponent: Side, seeds: std::ops::Range<u64>) -> usize {
    seeds
        .map(|seed| {
            let me = (seed % 2) as usize;
            let sides = if me == 0 {
                [Side::Level(level), opponent]
            } else {
                [opponent, Side::Level(level)]
            };
            usize::from(won(&record(seed, sides).0, me))
        })
        .sum()
}

fn against_the_human(level: u8) -> (usize, u32, u32) {
    let (mut won_matches, mut goals, mut against) = (0, 0, 0);
    for seed in 0..40u64 {
        let me = (seed % 2) as usize;
        let sides = if me == 0 {
            [Side::Level(level), Side::Human]
        } else {
            [Side::Human, Side::Level(level)]
        };
        let report = record(seed, sides).0;
        won_matches += usize::from(won(&report, me));
        goals += report.score[me] as u32;
        against += report.score[1 - me] as u32;
    }
    (won_matches, goals, against)
}

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn the_levels_climb_against_a_human_model() {
    let share = |(_, goals, against): (usize, u32, u32)| {
        f64::from(goals) / f64::from((goals + against).max(1))
    };
    let ladder: Vec<(usize, u32, u32)> = (1..=5).map(against_the_human).collect();
    for (level, row) in (1..=5).zip(&ladder) {
        eprintln!(
            "level {level}: won {} of 40, goals {}:{}, share {:.2}",
            row.0,
            row.1,
            row.2,
            share(*row)
        );
    }
    assert!(ladder[0].0 <= 10, "the human model must beat level 1");
    assert!(ladder[1].0 >= 24, "level 2 must win most matches");
    assert!(
        share(ladder[1]) <= 0.85,
        "the human model must still score on level 2"
    );
    for pair in ladder.windows(2) {
        assert!(
            share(pair[1]) > share(pair[0]),
            "a level is not stronger than the one below: {pair:?}"
        );
    }
    assert!(
        share(ladder[4]) >= share(ladder[1]) + 0.1,
        "level 5 is not clearly above level 2"
    );
}

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn thinking_levels_hold_the_readers_at_least_as_well_as_level_two() {
    let against_readers =
        |level: u8| wins(level, Side::Reader, 0..20) + wins(level, Side::Anticipator, 0..20);
    const CEILING: usize = 38;
    let floor = against_readers(2).min(CEILING);
    for level in 3..=5 {
        let won = against_readers(level);
        eprintln!("level {level} won {won} of 40 against the readers, level 2 won {floor}");
        assert!(
            won >= floor,
            "level {level} won {won} of 40 against the readers, level 2 {floor}"
        );
    }
}

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn level_five_beats_level_two() {
    let won = wins(5, Side::Level(2), 0..60);
    eprintln!("level 5 won {won} of 60 against level 2");
    assert!(won >= 36, "level 5 won only {won} of 60 against level 2");
}

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn fakes_that_score_are_recorded_as_won() {
    let mut d = Difficulty::preset(5).unwrap();
    d.deception = 1.0;
    d.adaptation = 0.0;
    d.depth = 1;
    let long = Params {
        goals_to_win: 7,
        ..params()
    };
    let mut game = AirHockey::new(long.clone(), 5);
    let mut faker = Bot::new(d, HockeyPlaybook::new(long), &game.spec()).unwrap();
    let mut idle = Aside { me: 1 };
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed: 7,
        bodies: vec![body(Side::Level(5), &game), body(Side::Idle, &game)],
        levels: [5, 0],
    };
    let report = run_match(&mut game, [&mut faker, &mut idle], header, 240 * 240).unwrap();
    let profile = faker.profile("airhockey").expect("level 5 keeps a model");
    eprintln!(
        "score {:?}, fake tries {:.2}, wins {:.2}",
        report.score, profile.fake_tries, profile.fake_wins
    );
    assert!(
        report.score[0] >= 2,
        "the faker should score against an opponent standing aside"
    );
    assert!(profile.fake_wins > 0.0, "no fake was ever recorded as won");
}
