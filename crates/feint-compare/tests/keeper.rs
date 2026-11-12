use airhockey::HockeyPlaybook;
use airhockey::analysis::{Attempt, attempts};
use feint_ai::{Bot, Difficulty, Opponent};
use feint_compare::honest;
use feint_compare::play::{human_params, play_against};
use feint_compare::trained::wilson;
use feint_ml::keeper::{BITE_TICKS, Keeper, KeeperCounts};

const SEEDS: std::ops::Range<u64> = 60_000..60_100;
const BITE_PERMILLE: u16 = 700;

#[derive(Default)]
struct Tally {
    fakes: u32,
    fakes_scored: u32,
    honest: u32,
    honest_scored: u32,
    keeper: KeeperCounts,
    by_distance: [[u32; 2]; 3],
    rail_goals: u32,
    deflected_goals: u32,
    free_window_goals: u32,
    bite_to_contact: Vec<u64>,
    fakes_without_bite: u32,
}

fn bucket(x: f64) -> usize {
    if x < -0.5 {
        0
    } else if x < 0.0 {
        1
    } else {
        2
    }
}

fn measure(d: Difficulty, seed: u64, out: &mut Tally) {
    let seat = (seed % 2) as u8;
    let me = usize::from(seat);
    let mut keeper = Keeper::new(human_params(), BITE_PERMILLE);
    let played = play_against(
        |spec, params| {
            let bot = Bot::new(d, HockeyPlaybook::new(params.clone()), spec).unwrap();
            (bot, d.body(spec), Opponent::HandMade)
        },
        &mut keeper,
        seed,
        seat,
    );
    let params = human_params();
    let frames = &played.frames;
    let at = |tick: u64| {
        frames
            .partition_point(|f| f.tick < tick)
            .min(frames.len() - 1)
    };
    let mine: Vec<Attempt> = attempts(&params, frames, &played.report.actions)
        .into_iter()
        .filter(|a| a.player == seat && a.contact.is_some())
        .collect();
    let bites = keeper.bite_ticks().to_vec();
    out.keeper += keeper.counts();
    let reach = params.puck_radius + params.mallet_radius + 0.01;
    let rail = params.half_width - params.puck_radius - 0.005;
    for a in &mine {
        let contact = a.contact.unwrap_or(a.end);
        let x = frames[at(contact)].puck.x * if seat == 0 { 1.0 } else { -1.0 };
        let cell = &mut out.by_distance[bucket(x)];
        cell[0] += 1;
        if a.deceptive {
            out.fakes += 1;
            match bites.iter().rev().find(|&&b| b <= contact) {
                Some(&b) => out.bite_to_contact.push(contact - b),
                None => out.fakes_without_bite += 1,
            }
        } else {
            out.honest += 1;
        }
        if !a.scored {
            continue;
        }
        cell[1] += 1;
        if a.deceptive {
            out.fakes_scored += 1;
        } else {
            out.honest_scored += 1;
            if bites
                .iter()
                .any(|&b| b <= contact && contact < b + BITE_TICKS)
            {
                out.free_window_goals += 1;
            }
        }
        let flight = &frames[at(contact)..];
        let goal = flight
            .windows(2)
            .position(|w| w[1].score[me] > w[0].score[me])
            .unwrap_or(flight.len() - 1);
        let path = &flight[..=goal];
        if path.iter().any(|f| f.puck.y.abs() >= rail) {
            out.rail_goals += 1;
        }
        if path
            .iter()
            .any(|f| (f.puck - f.mallets[1 - me]).length() <= reach)
        {
            out.deflected_goals += 1;
        }
    }
}

fn rate(k: u32, n: u32) -> f64 {
    f64::from(k) / f64::from(n.max(1))
}

fn report(name: &str, t: &Tally) {
    let fw = wilson(t.fakes_scored, t.fakes).unwrap_or((0.0, 0.0));
    let hw = wilson(t.honest_scored, t.honest).unwrap_or((0.0, 0.0));
    eprintln!("{name}:");
    eprintln!(
        "  faked attacks {} scored {} rate {:.3} Wilson [{:.3}, {:.3}]",
        t.fakes,
        t.fakes_scored,
        rate(t.fakes_scored, t.fakes),
        fw.0,
        fw.1
    );
    eprintln!(
        "  honest attacks {} scored {} rate {:.3} Wilson [{:.3}, {:.3}]",
        t.honest,
        t.honest_scored,
        rate(t.honest_scored, t.honest),
        hw.0,
        hw.1
    );
    eprintln!(
        "  keeper: reads {} bites {} goals {} goals while bitten {}",
        t.keeper.reads, t.keeper.bites, t.keeper.goals, t.keeper.bite_goals
    );
    for (label, cell) in ["x < -0.5", "-0.5 <= x < 0", "x >= 0"]
        .iter()
        .zip(&t.by_distance)
    {
        eprintln!(
            "  contact {label}: attacks {} scored {} rate {:.3}",
            cell[0],
            cell[1],
            rate(cell[1], cell[0])
        );
    }
    eprintln!(
        "  scored after a rail {}, scored past a keeper touch {}, honest goals inside a bite window {}",
        t.rail_goals, t.deflected_goals, t.free_window_goals
    );
    let mut d = t.bite_to_contact.clone();
    d.sort_unstable();
    let q = |p: usize| {
        d.get(d.len().saturating_sub(1) * p / 100)
            .copied()
            .unwrap_or(0)
    };
    let inside = d.iter().filter(|&&x| x < BITE_TICKS).count();
    eprintln!(
        "  fakes: bite to contact median {} p90 {} ticks, {} of {} inside the {}-tick commit, {} with no bite before",
        q(50),
        q(90),
        inside,
        d.len(),
        BITE_TICKS,
        t.fakes_without_bite
    );
}

#[test]
#[ignore = "release only: bedrock cmd keeper-gate"]
fn the_keeper_gate_prints_where_hand_made_goals_come_from() {
    let level5 = Difficulty::preset(5).unwrap();
    let mut deceptive = Tally::default();
    let mut plain = Tally::default();
    for seed in SEEDS {
        measure(level5, seed, &mut deceptive);
        measure(honest(level5), seed, &mut plain);
    }
    report("level 5, deception 0.7", &deceptive);
    report("level 5, honest", &plain);
    assert!(
        deceptive.fakes >= 100,
        "only {} fakes to measure",
        deceptive.fakes
    );
    assert_eq!(plain.fakes, 0);
}
