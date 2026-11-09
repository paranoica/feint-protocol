use std::process::ExitCode;

use duel::{Params, QuickDraw, RoundEvent};
use feint_ai::decode_record;
use feint_protocol::GameAdapter;

fn who(seat: usize) -> &'static str {
    if seat == 0 { "you" } else { "bot" }
}

fn response(code: i8) -> &'static str {
    match code {
        -1 => "stepped left early",
        1 => "stepped right early",
        -2 => "stepped left late",
        2 => "stepped right late",
        _ => "stood",
    }
}

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: shots <record file>");
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
    if record.header.game_id != "duel" {
        eprintln!("{path}: a {} record", record.header.game_id);
        return ExitCode::FAILURE;
    }
    let level = record.header.levels[1];
    let mut game = QuickDraw::new(Params::default(), level);
    if let Err(error) = game.reset(record.header.seed, &record.header.bodies) {
        eprintln!("{path}: {error:?}");
        return ExitCode::FAILURE;
    }
    let hz = f64::from(game.live().params.tick_hz);
    let mut pending = record.actions.iter().peekable();
    let mut events = Vec::new();
    let mut fake = false;
    let mut fakes = 0;
    let mut draw_start = [None::<u64>; 2];
    for tick in 0..record.ticks {
        let before = game.live().clone();
        while let Some(action) = pending.next_if(|a| a.tick <= tick) {
            if action.player == 1 {
                if action.deceptive && !fake {
                    fakes += 1;
                }
                fake = action.deceptive;
            }
            if let Err(error) = game.apply(action) {
                eprintln!("tick {tick}: {error:?}");
                return ExitCode::FAILURE;
            }
        }
        events.clear();
        game.advance(&mut events);
        let after = game.live();
        for (seat, started) in draw_start.iter_mut().enumerate() {
            let (b, a) = (&before.gunners[seat], &after.gunners[seat]);
            if a.hand > 0.0 && b.hand == 0.0 {
                *started = Some(tick);
            }
            if a.shots > b.shots && after.round == before.round {
                let lead = before.bullets[seat].map_or(0.0, |x| x.lead);
                let drawn = started.map_or(0.0, |t| (tick - t) as f64 / hz);
                let hit = matches!(after.last, Some(RoundEvent::Hit { shooter }) if usize::from(shooter) == seat)
                    && after.phase != before.phase;
                println!(
                    "round {:<2} {} shoots: lead {:+.1}, {:.2}s after the draw began | {} {} | {}",
                    before.round + 1,
                    who(seat),
                    lead,
                    drawn,
                    who(1 - seat),
                    response(a.last_response),
                    if hit { "HIT" } else { "miss" }
                );
            }
        }
        if after.last != before.last && after.last == Some(RoundEvent::Void) {
            println!("round {:<2} timed out", before.round + 1);
        }
    }
    let s = game.live();
    println!(
        "level {level}, seed {}, score you {} : bot {}, bot fakes started {fakes}",
        record.header.seed, s.score[0], s.score[1]
    );
    ExitCode::SUCCESS
}
