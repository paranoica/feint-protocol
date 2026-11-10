use std::process::ExitCode;

use duel::{Params, Phase, QuickDraw};
use feint_ai::decode_record;
use feint_protocol::GameAdapter;

#[derive(Default)]
struct Habits {
    bot_draws: u32,
    stepped: u32,
    step_after_draw: Vec<f64>,
    step_before_commit: u32,
    shots: u32,
    centre: u32,
    leads: Vec<f64>,
    waits: Vec<f64>,
    fakes: u32,
    steps_left: u32,
    steps_right: u32,
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() -> ExitCode {
    let mut h = Habits::default();
    for path in std::env::args().skip(1) {
        let Ok(Ok(record)) = std::fs::read(&path).map(|b| decode_record(&b)) else {
            eprintln!("{path}: unreadable");
            return ExitCode::FAILURE;
        };
        let mut game = QuickDraw::new(Params::default(), record.header.levels[1]);
        if game
            .reset(record.header.seed, &record.header.bodies)
            .is_err()
        {
            return ExitCode::FAILURE;
        }
        let hz = f64::from(game.live().params.tick_hz);
        let mut pending = record.actions.iter().peekable();
        let mut events = Vec::new();
        let mut bot_draw: Option<u64> = None;
        let mut answered = false;
        let mut ready_since: Option<u64> = None;
        let mut my_draw: Option<u64> = None;
        let mut committed_seen = false;
        for tick in 0..record.ticks {
            let before = game.live().clone();
            while let Some(a) = pending.next_if(|a| a.tick <= tick) {
                let _ = game.apply(a);
            }
            events.clear();
            game.advance(&mut events);
            let s = game.live();
            let (me, bot) = (&s.gunners[0], &s.gunners[1]);
            let (pme, pbot) = (&before.gunners[0], &before.gunners[1]);
            if s.round != before.round {
                bot_draw = None;
                ready_since = None;
                my_draw = None;
                continue;
            }
            if s.phase != Phase::Live {
                continue;
            }
            if bot.hand > 0.0 && pbot.hand == 0.0 && bot.loaded {
                bot_draw = Some(tick);
                answered = false;
                h.bot_draws += 1;
            }
            if me.rolls > pme.rolls {
                if me.last_roll < 0 {
                    h.steps_left += 1;
                } else {
                    h.steps_right += 1;
                }
                if let Some(t) = bot_draw.filter(|_| !answered) {
                    answered = true;
                    h.stepped += 1;
                    h.step_after_draw.push((tick - t) as f64 / hz);
                    if !bot.committed && s.bullets[1].is_none() {
                        h.step_before_commit += 1;
                    }
                }
            }
            if bot_draw.is_some() && s.bullets[1].is_none() && pbot.loaded && !bot.loaded {
                bot_draw = None;
            }
            if me.loaded && (!pme.loaded || ready_since.is_none()) {
                ready_since = Some(tick);
            }
            if me.hand > 0.0 && pme.hand == 0.0 && me.loaded {
                if let Some(t) = ready_since {
                    h.waits.push((tick - t) as f64 / hz);
                }
                my_draw = Some(tick);
                committed_seen = false;
            }
            if me.committed {
                committed_seen = true;
            }
            if my_draw.is_some() && me.hand == 0.0 && pme.hand > 0.0 && !committed_seen && me.loaded
            {
                h.fakes += 1;
                my_draw = None;
            }
            if s.bullets[0].is_some() && before.bullets[0].is_none() {
                let lead = s.bullets[0].map_or(0.0, |b| b.lead);
                h.shots += 1;
                if lead.abs() < 0.5 {
                    h.centre += 1;
                }
                h.leads.push(lead);
                my_draw = None;
                ready_since = None;
            }
        }
    }
    let rate = |a: u32, b: u32| {
        if b == 0 {
            f64::NAN
        } else {
            f64::from(a) / f64::from(b)
        }
    };
    let leads: Vec<f64> = h.leads.iter().copied().filter(|l| l.abs() >= 0.5).collect();
    println!(
        "bot draws {}, you stepped on {:.0}% of them",
        h.bot_draws,
        100.0 * rate(h.stepped, h.bot_draws)
    );
    println!(
        "step after the bot's draw began: median {:.2}s, before its commit {:.0}%",
        median(&mut h.step_after_draw.clone()),
        100.0 * rate(h.step_before_commit, h.stepped)
    );
    println!("your steps left {} right {}", h.steps_left, h.steps_right);
    println!(
        "your shots {}, aimed straight {:.0}%, lead sizes {:?}",
        h.shots,
        100.0 * rate(h.centre, h.shots),
        leads
            .iter()
            .map(|l| (l * 10.0).round() / 10.0)
            .collect::<Vec<_>>()
    );
    println!(
        "wait from loaded to draw: median {:.2}s over {}",
        median(&mut h.waits.clone()),
        h.waits.len()
    );
    println!("your fakes (draw released before the mark): {}", h.fakes);
    let mut gaps = h.step_after_draw.clone();
    gaps.sort_by(f64::total_cmp);
    println!(
        "step times: {:?}",
        gaps.iter()
            .map(|g| (g * 100.0).round() / 100.0)
            .collect::<Vec<_>>()
    );
    ExitCode::SUCCESS
}
