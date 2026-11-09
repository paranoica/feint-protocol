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
fn a_reader_bites_on_fakes_in_the_log() {
    let t = bot_tally(4, Side::Reader, 0..40);
    eprintln!("level 4 against a reader: {t:?}");
    assert!(t.fakes >= 10, "only {} fakes in 40 matches", t.fakes);
    assert!(
        2 * t.shifted >= t.fakes,
        "the reader shifted towards the shown side in {} of {} fakes",
        t.shifted,
        t.fakes
    );
    let rate = |goals: u32, tries: u32| f64::from(goals) / f64::from(tries.max(1));
    assert!(
        rate(t.fake_goals, t.fakes) > rate(t.honest_goals, t.honest),
        "fakes scored {}/{}, honest strikes {}/{}",
        t.fake_goals,
        t.fakes,
        t.honest_goals,
        t.honest
    );
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

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn each_level_beats_the_level_below() {
    for (below, level) in [(2u8, 3u8), (3, 4), (4, 5)] {
        let won = wins(level, Side::Level(below), 0..40);
        eprintln!("level {level} won {won} of 40 against level {below}");
        assert!(
            won >= 24,
            "level {level} won only {won} of 40 against level {below}"
        );
    }
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

#[test]
#[ignore = "probe"]
fn probe_levels_against_the_human() {
    for level in 1..=5u8 {
        let (mut w, mut gf, mut ga) = (0, 0, 0);
        for seed in 0..40u64 {
            let me = (seed % 2) as usize;
            let sides = if me == 0 {
                [Side::Level(level), Side::Human]
            } else {
                [Side::Human, Side::Level(level)]
            };
            let r = record(seed, sides).0;
            w += usize::from(won(&r, me));
            gf += r.score[me];
            ga += r.score[1 - me];
        }
        eprintln!("PROBE level {level}: won {w}/20, goals {gf}:{ga}");
    }
}

fn share_against_human(d: Difficulty, seeds: u64) -> (u32, u32) {
    let (mut gf, mut ga) = (0, 0);
    for seed in 0..seeds {
        let me = (seed % 2) as usize;
        let mut game = AirHockey::new(params(), 3);
        let spec = game.spec();
        let mut bot = Bot::new(d, HockeyPlaybook::new(params()), &spec).unwrap();
        let mut human = Human::new(params(), HUMAN_NOISE);
        let bot_body = d.body(&spec);
        let human_body = body(Side::Human, &game);
        let (bodies, levels) = if me == 0 {
            (vec![bot_body, human_body], [3, 0])
        } else {
            (vec![human_body, bot_body], [0, 3])
        };
        let header = RunHeader {
            game_id: "airhockey".into(),
            game_version: airhockey::GAME_VERSION,
            seed,
            bodies,
            levels,
        };
        let r = if me == 0 {
            run_match(&mut game, [&mut bot, &mut human], header, CAP_TICKS)
        } else {
            run_match(&mut game, [&mut human, &mut bot], header, CAP_TICKS)
        }
        .unwrap();
        gf += r.score[me] as u32;
        ga += r.score[1 - me] as u32;
    }
    (gf, ga)
}

#[test]
#[ignore = "probe"]
fn probe_what_makes_level_two() {
    let one = Difficulty::preset(1).unwrap();
    let two = Difficulty::preset(2).unwrap();
    let variants: [(&str, Difficulty); 6] = [
        ("level 1", one),
        ("level 2", two),
        (
            "2 with reaction 400",
            Difficulty {
                reaction_ms: 400,
                ..two
            },
        ),
        (
            "2 with physique 0.4",
            Difficulty {
                physique: 0.4,
                ..two
            },
        ),
        (
            "2 with spread 0.08",
            Difficulty {
                spread: 0.08,
                ..two
            },
        ),
        (
            "2 without strikes",
            Difficulty {
                repertoire: 0,
                anticipate: false,
                ..two
            },
        ),
    ];
    for (name, d) in variants {
        let (gf, ga) = share_against_human(d, 20);
        eprintln!("PROBE {name}: bot {gf} : {ga} human");
    }
}

#[test]
#[ignore = "probe"]
fn probe_spread() {
    for level in 2..=5u8 {
        for spread in [0.06, 0.1, 0.14] {
            let d = Difficulty {
                spread,
                ..Difficulty::preset(level).unwrap()
            };
            let (gf, ga) = share_against_human(d, 20);
            eprintln!("PROBE level {level} spread {spread}: bot {gf} : {ga} human");
        }
    }
}

#[test]
#[ignore = "probe"]
fn probe_fakes() {
    for level in [4u8, 5] {
        eprintln!(
            "PROBE level {level} vs human: {:?}",
            bot_tally(level, Side::Human, 0..40)
        );
        eprintln!(
            "PROBE level {level} vs reader: {:?}",
            bot_tally(level, Side::Reader, 0..40)
        );
    }
}

#[test]
#[ignore = "probe"]
fn probe_fake_anatomy() {
    let mut printed = 0;
    for seed in 0..20u64 {
        let me = (seed % 2) as u8;
        let sides = if me == 0 {
            [Side::Level(5), Side::Human]
        } else {
            [Side::Human, Side::Level(5)]
        };
        let (_, frames) = record(seed, sides);
        let report = record(seed, sides).0;
        let sign = if me == 0 { 1.0 } else { -1.0 };
        for a in attempts(&params(), &frames, &report.actions)
            .iter()
            .filter(|a| a.player == me && a.deceptive)
        {
            let Some(c) = a.contact else { continue };
            let at = |t: u64| frames.iter().find(|f| f.tick == t).copied();
            let (Some(f0), Some(f1)) = (at(c), at(c + 8)) else {
                continue;
            };
            let vx = sign * (f1.puck.x - f0.puck.x) * 30.0;
            let vy = sign * (f1.puck.y - f0.puck.y) * 30.0;
            let them = usize::from(1 - me);
            let crossing = frames
                .iter()
                .filter(|f| f.tick > c && f.tick < c + 480)
                .find(|f| sign * f.puck.x > 0.8 || sign * (f.puck.x - f0.puck.x) < -0.05);
            let (dt, gap) = crossing.map_or((-1.0, 0.0), |f| {
                (
                    (f.tick - c) as f64 / 240.0,
                    sign * (f.puck.y - f.mallets[them].y),
                )
            });
            eprintln!(
                "PROBE seed {seed} shown {:?} shifted {} scored {} | strike at x {:.2} y {:.2}, v ({vx:.1}, {vy:.1}), reaches 0.8 after {dt:.2}s, puck-defender gap {gap:.2}, show {} ticks",
                a.shown,
                a.shifted,
                a.scored,
                sign * f0.puck.x,
                sign * f0.puck.y,
                c - a.start
            );
            printed += 1;
            if printed >= 25 {
                return;
            }
        }
    }
}

#[test]
#[ignore = "probe"]
fn probe_goal_credit() {
    let p = params();
    let reach = p.puck_radius + p.mallet_radius + 0.01;
    let (mut total, mut defender_last, mut credited, mut no_touch) = (0, 0, 0, 0);
    for seed in 0..20u64 {
        let me = (seed % 2) as usize;
        let sides = if me == 0 {
            [Side::Level(5), Side::Human]
        } else {
            [Side::Human, Side::Level(5)]
        };
        let (report, frames) = record(seed, sides);
        let found = attempts(&p, &frames, &report.actions);
        credited += found
            .iter()
            .filter(|a| usize::from(a.player) == me && a.scored)
            .count();
        for k in 1..frames.len() {
            if frames[k].score[me] <= frames[k - 1].score[me] {
                continue;
            }
            total += 1;
            let start = k.saturating_sub(240);
            let last = (start..k).rev().find_map(|j| {
                let f = frames[j];
                let near = |m: usize| (f.puck - f.mallets[m]).length() < reach;
                if near(me) {
                    Some(me)
                } else if near(1 - me) {
                    Some(1 - me)
                } else {
                    None
                }
            });
            match last {
                None => no_touch += 1,
                Some(w) if w != me => defender_last += 1,
                _ => {}
            }
        }
    }
    eprintln!(
        "PROBE bot goals {total}: credited to attempts {credited}, defender touched last {defender_last}, no touch in 1 s {no_touch}"
    );
}

#[test]
#[ignore = "probe"]
fn probe_fake_lengths() {
    let mut lengths = Vec::new();
    for seed in 0..10u64 {
        let me = (seed % 2) as u8;
        let sides = if me == 0 {
            [Side::Level(5), Side::Human]
        } else {
            [Side::Human, Side::Level(5)]
        };
        let report = record(seed, sides).0;
        let mut start: Option<u64> = None;
        let mut last = 0u64;
        for a in report.actions.iter().filter(|a| a.player == me) {
            match (a.deceptive, start) {
                (true, None) => start = Some(a.tick),
                (false, Some(s)) => {
                    lengths.push(last - s);
                    start = None;
                }
                _ => {}
            }
            last = a.tick;
        }
    }
    lengths.sort_unstable();
    let full = lengths.iter().filter(|&&l| l >= 132).count();
    eprintln!(
        "PROBE {} fakes in 10 matches, {} lasted past wind-up and pump, median {} ticks",
        lengths.len(),
        full,
        lengths.get(lengths.len() / 2).copied().unwrap_or(0)
    );
}
