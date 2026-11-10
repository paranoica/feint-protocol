use feint_ai::{Bot, Controller, Difficulty, Playbook, RunHeader, Runner};
use feint_protocol::{BodyParamsT, GameAdapter};

const MATCHES: u64 = 40;
const OUTCOME_TICKS: u64 = 480;

#[derive(Default, Debug)]
struct Side {
    attempts: u32,
    scored: u32,
    conceded: u32,
    ticks: u64,
    bit: u32,
    read: u32,
    scored_bit: u32,
    scored_unbit: u32,
}

#[derive(Default, Debug)]
struct Study {
    honest: Side,
    fake: Side,
    unread: u32,
}

struct Attempt<S> {
    start: u64,
    since: S,
    fake_shown: Option<u8>,
    covered: Option<u8>,
    score: [i64; 2],
}

struct Setup<A: GameAdapter> {
    header: RunHeader,
    seat: u8,
    cap: u64,
    score_of: fn(&A::State) -> [i64; 2],
}

fn study<A, P>(
    mut game: A,
    mut bot: Bot<A, P>,
    reader: &P,
    human: &mut dyn Controller<A>,
    setup: Setup<A>,
    out: &mut Study,
) where
    A: GameAdapter,
    P: Playbook<A>,
{
    let Setup {
        header,
        seat,
        cap,
        score_of,
    } = setup;
    let me = usize::from(seat);
    let them = 1 - seat;
    let mut runner = if seat == 0 {
        Runner::start(&mut game, [&mut bot, human], header)
    } else {
        Runner::start(&mut game, [human, &mut bot], header)
    }
    .unwrap_or_else(|e| panic!("start: {e:?}"));
    let mut attempt: Option<Attempt<A::State>> = None;
    let mut waiting: Vec<(Attempt<A::State>, u64, u64)> = Vec::new();
    let mut possession = None;
    let mut prev = game.live().clone();
    let close = |a: Attempt<A::State>, now: [i64; 2], tick: u64, out: &mut Study| {
        let scored = now[me] > a.score[me];
        let conceded = now[1 - me] > a.score[1 - me];
        let side = if a.fake_shown.is_some() {
            &mut out.fake
        } else {
            &mut out.honest
        };
        side.attempts += 1;
        side.scored += u32::from(scored);
        side.conceded += u32::from(conceded);
        side.ticks += tick - a.start;
        if let Some(shown) = a.fake_shown {
            match a.covered {
                Some(c) => {
                    side.read += 1;
                    if c == shown {
                        side.bit += 1;
                        side.scored_bit += u32::from(scored);
                    } else {
                        side.scored_unbit += u32::from(scored);
                    }
                }
                None => out.unread += 1,
            }
        }
    };
    while runner.ticks() < cap {
        let stepped = if seat == 0 {
            runner.step(&mut game, [&mut bot as &mut dyn Controller<A>, human])
        } else {
            runner.step(&mut game, [human, &mut bot as &mut dyn Controller<A>])
        };
        let state = game.live().clone();
        let score = score_of(&state);
        if let Some(a) = attempt.as_mut() {
            if let Some(c) = bot.current().filter(|c| c.deceptive) {
                a.fake_shown = a.fake_shown.or(c.shown).or(Some(u8::MAX));
            }
            if let Some(c) = reader.read(&a.since, &state, them).covered {
                a.covered = Some(c);
            }
        }
        let now_possession = reader.possession(&state, seat);
        if now_possession != possession {
            if let Some(a) = attempt.take() {
                let end = runner.ticks();
                waiting.push((a, end, end + OUTCOME_TICKS));
            }
            if now_possession.is_some() {
                attempt = Some(Attempt {
                    start: runner.ticks(),
                    since: state.clone(),
                    fake_shown: None,
                    covered: None,
                    score,
                });
            }
            possession = now_possession;
        }
        let now = runner.ticks();
        let (due, rest): (Vec<_>, Vec<_>) = waiting.into_iter().partition(|w| now >= w.2);
        waiting = rest;
        for (a, end, _) in due {
            close(a, score, end, out);
        }
        prev = state;
        if !stepped {
            break;
        }
    }
    let _ = prev;
    let last = score_of(game.live());
    for (a, end, _) in waiting {
        close(a, last, end, out);
    }
    if let Some(a) = attempt.take() {
        close(a, last, runner.ticks(), out);
    }
}

fn header(
    id: &str,
    version: u32,
    seed: u64,
    bot: BodyParamsT,
    human: BodyParamsT,
    seat: u8,
) -> RunHeader {
    RunHeader {
        game_id: id.into(),
        game_version: version,
        seed,
        bodies: if seat == 0 {
            vec![bot, human]
        } else {
            vec![human, bot]
        },
        levels: [0, 0],
    }
}

fn report(name: &str, s: &Study, hz: f64) {
    let line = |label: &str, x: &Side| {
        let rate = |a: u32, b: u32| {
            if b == 0 {
                0.0
            } else {
                100.0 * f64::from(a) / f64::from(b)
            }
        };
        println!(
            "  {label}: {} attempts, scored {:.0}%, conceded {:.0}%, {:.2}s each",
            x.attempts,
            rate(x.scored, x.attempts),
            rate(x.conceded, x.attempts),
            if x.attempts == 0 {
                0.0
            } else {
                x.ticks as f64 / f64::from(x.attempts) / hz
            }
        );
    };
    println!("{name}");
    line("honest", &s.honest);
    line("with a fake", &s.fake);
    let f = &s.fake;
    let pct = |a: u32, b: u32| {
        if b == 0 {
            0.0
        } else {
            100.0 * f64::from(a) / f64::from(b)
        }
    };
    println!(
        "  fakes the opponent answered: {} (unanswered {}), bit {:.0}%; scored after a bite {:.0}%, after no bite {:.0}%",
        f.read,
        s.unread,
        pct(f.bit, f.read),
        pct(f.scored_bit, f.bit),
        pct(f.scored_unbit, f.read - f.bit)
    );
}

fn main() {
    let five = Difficulty::preset(5).unwrap_or_else(|| panic!("level 5"));

    let mut s = Study::default();
    let params = airhockey::Params {
        goals_to_win: airhockey::human::HUMAN_GOALS,
        ..airhockey::Params::default()
    };
    for seed in 0..MATCHES {
        let seat = (seed % 2) as u8;
        let game = airhockey::AirHockey::new(params.clone(), 1);
        let spec = game.spec();
        let bot = Bot::new(five, airhockey::HockeyPlaybook::new(params.clone()), &spec)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let reader = airhockey::HockeyPlaybook::new(params.clone());
        let mut human = airhockey::human::Human::new(params.clone(), airhockey::human::HUMAN_NOISE);
        let h = header(
            "airhockey",
            airhockey::GAME_VERSION,
            seed,
            five.body(&spec),
            five.body(&spec),
            seat,
        );
        study(
            game,
            bot,
            &reader,
            &mut human,
            Setup {
                header: h,
                seat,
                cap: 240 * 600,
                score_of: |s: &airhockey::Match| s.score().map(i64::from),
            },
            &mut s,
        );
    }
    report("air hockey", &s, 240.0);

    let mut s = Study::default();
    for seed in 0..MATCHES {
        let seat = (seed % 2) as u8;
        let game = penalty::Penalty::new(penalty::Params::default(), 5);
        let spec = game.spec();
        let bot = Bot::new(
            five,
            penalty::PenaltyPlaybook::new(penalty::Params::default()),
            &spec,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let reader = penalty::PenaltyPlaybook::new(penalty::Params::default());
        let mut human = penalty::human::Human::new();
        let hb = penalty::human::human_body(&game);
        let h = header(
            "penalty",
            penalty::GAME_VERSION,
            seed,
            five.body(&spec),
            hb,
            seat,
        );
        study(
            game,
            bot,
            &reader,
            &mut human,
            Setup {
                header: h,
                seat,
                cap: 240 * 900,
                score_of: |s: &penalty::Shootout| s.score.map(i64::from),
            },
            &mut s,
        );
    }
    report("penalty", &s, 240.0);

    let mut s = Study::default();
    for seed in 0..MATCHES {
        let seat = (seed % 2) as u8;
        let game = duel::QuickDraw::new(duel::Params::default(), 5);
        let spec = game.spec();
        let bot = Bot::new(
            five,
            duel::DuelPlaybook::new(duel::Params::default()),
            &spec,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let reader = duel::DuelPlaybook::new(duel::Params::default());
        let mut human = duel::human::Human::new();
        let hb = duel::human::human_body(&game);
        let h = header("duel", duel::GAME_VERSION, seed, five.body(&spec), hb, seat);
        study(
            game,
            bot,
            &reader,
            &mut human,
            Setup {
                header: h,
                seat,
                cap: 240 * 900,
                score_of: |s: &duel::Duel| s.score.map(i64::from),
            },
            &mut s,
        );
    }
    report("duel", &s, 240.0);
}
