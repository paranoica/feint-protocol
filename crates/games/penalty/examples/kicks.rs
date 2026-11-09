use std::process::ExitCode;

use feint_ai::decode_record;
use feint_protocol::{EventKind, GameAdapter};
use penalty::{Params, Penalty, Phase};

const LOOKBACK: usize = 120;

struct Kick {
    shooter: u8,
    plant: Option<u64>,
    lean_before: f64,
    lean_at_plant: f64,
    aim: f64,
    dive: Option<(u64, f64)>,
    bot_mind: Option<(u8, bool)>,
    result: &'static str,
}

fn who(seat: u8) -> &'static str {
    if seat == 0 { "you" } else { "bot" }
}

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: kicks <record file>");
        return ExitCode::FAILURE;
    };
    let record = match std::fs::read(&path).map(|b| decode_record(&b)) {
        Ok(Ok(record)) => record,
        Ok(Err(error)) => {
            eprintln!("{path}: {error:?}");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    if record.header.game_id != "penalty" {
        eprintln!("{path}: a {} record", record.header.game_id);
        return ExitCode::FAILURE;
    }
    let level = record.header.levels[1];
    let mut game = Penalty::new(Params::default(), level);
    if let Err(error) = game.reset(record.header.seed, &record.header.bodies) {
        eprintln!("{path}: {error:?}");
        return ExitCode::FAILURE;
    }
    let hz = f64::from(game.live().params.tick_hz);
    let mut pending = record.actions.iter().peekable();
    let mut events = Vec::new();
    let mut leans = std::collections::VecDeque::new();
    let mut mind = None;
    let mut kicks: Vec<Kick> = Vec::new();
    let fresh = |shooter: u8| Kick {
        shooter,
        plant: None,
        lean_before: 0.0,
        lean_at_plant: 0.0,
        aim: 0.0,
        dive: None,
        bot_mind: None,
        result: "",
    };
    kicks.push(fresh(game.live().shooter()));
    for tick in 0..record.ticks {
        let before = game.live().clone();
        while let Some(action) = pending.next_if(|a| a.tick <= tick) {
            if action.player == 1 {
                mind = Some((action.intent, action.deceptive));
            }
            if let Err(error) = game.apply(action) {
                eprintln!("tick {tick}: {error:?}");
                return ExitCode::FAILURE;
            }
        }
        events.clear();
        game.advance(&mut events);
        let after = game.live();
        let Some(kick) = kicks.last_mut() else {
            break;
        };
        leans.push_back(after.lean);
        if leans.len() > LOOKBACK {
            leans.pop_front();
        }
        if after.phase == Phase::Strike && before.phase != Phase::Strike {
            kick.plant = Some(tick);
            kick.lean_before = leans.front().copied().unwrap_or(0.0);
            kick.lean_at_plant = after.lean;
            kick.aim = after.aim.unwrap_or(f64::NAN);
            if kick.shooter == 1 {
                kick.bot_mind = mind;
            }
        }
        if after.dived && !before.dived {
            kick.dive = Some((tick, after.dive.map_or(f64::NAN, |d| d.target)));
            if kick.shooter == 0 {
                kick.bot_mind = mind;
            }
        }
        for e in &events {
            kick.result = match e.kind {
                EventKind::Score => "GOAL",
                _ => "saved",
            };
        }
        if after.kicks != before.kicks {
            leans.clear();
            mind = None;
            kicks.push(fresh(after.shooter()));
        }
    }
    let score = game.live().score;
    println!(
        "level {level}, seed {}, score you {} : bot {}",
        record.header.seed, score[0], score[1]
    );
    for (n, k) in kicks.iter().enumerate().filter(|(_, k)| k.plant.is_some()) {
        let plant = k.plant.unwrap_or(0);
        let dive = k.dive.map_or_else(
            || "no dive".to_owned(),
            |(t, y)| {
                let at = (t as f64 - plant as f64) / hz;
                format!("dive {at:+.2}s to {y:+.1}")
            },
        );
        let mind = k.bot_mind.map_or_else(
            || "-".to_owned(),
            |(intent, fake)| format!("intent {intent}{}", if fake { " FAKE" } else { "" }),
        );
        println!(
            "#{:<2} {} shoots | lean {:+.2} -> {:+.2} at plant, aim {:+.2} | {} keeps: {} | bot {} | {}",
            n + 1,
            who(k.shooter),
            k.lean_before,
            k.lean_at_plant,
            k.aim,
            who(1 - k.shooter),
            dive,
            mind,
            k.result
        );
    }
    ExitCode::SUCCESS
}
