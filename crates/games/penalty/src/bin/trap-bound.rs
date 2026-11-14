use std::collections::BTreeMap;
use std::process::ExitCode;

use penalty::styles::{KeeperStyle, Mind, ShooterStyle};
use penalty::traps::{Abstract, Odds, Rules, Shootout, Table, play};

const BOT_SCORES: f64 = 0.85;
const PERSON_SCORES: f64 = 0.15;

fn keeper_name(m: &Mind) -> &'static str {
    match m.keeper {
        KeeperStyle::Static => "static",
        KeeperStyle::Trust { .. } => "trust",
        KeeperStyle::Recency { .. } => "recency",
        KeeperStyle::AntiRecency { .. } => "anti-recency",
        KeeperStyle::Fictitious { .. } => "fictitious",
        KeeperStyle::Learner { .. } => "learner",
        KeeperStyle::Counter { .. } => "counter",
    }
}

fn shooter_name(m: &Mind) -> &'static str {
    match m.shooter {
        ShooterStyle::Static => "static",
        ShooterStyle::WinStay { .. } => "win-stay",
        ShooterStyle::WinShift { .. } => "win-shift",
        ShooterStyle::AvoidDive { .. } => "avoid-dive",
        ShooterStyle::Markov => "markov",
    }
}

fn mean_se(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    if n < 2.0 {
        return (xs.iter().sum::<f64>() / n.max(1.0), f64::NAN);
    }
    let mean = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n - 1.0);
    (mean, (var / n).sqrt())
}

struct Row {
    keeper: &'static str,
    shooter: &'static str,
    results: Vec<Abstract>,
}

fn rate(won: u32, n: u32) -> f64 {
    f64::from(won) / f64::from(n.max(1))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [first, count, deception, react, family, mode] = args.as_slice() else {
        eprintln!(
            "usage: trap-bound <first person> <persons> <deception> <react save> <person|held-out> <share|wins>"
        );
        return ExitCode::FAILURE;
    };
    let (Ok(first), Ok(count), Ok(deception), Ok(react)) = (
        first.parse::<u64>(),
        count.parse::<u64>(),
        deception.parse::<f64>(),
        react.parse::<f64>(),
    ) else {
        eprintln!("first and persons are integers, deception and react save are numbers");
        return ExitCode::FAILURE;
    };
    let make: fn(u64) -> Mind = match family.as_str() {
        "person" => Mind::person,
        "held-out" => Mind::held_out,
        _ => {
            eprintln!("family is person or held-out");
            return ExitCode::FAILURE;
        }
    };
    let table = Table { react_save: react };
    let wins = match mode.as_str() {
        "share" => false,
        "wins" => true,
        _ => {
            eprintln!("mode is share or wins");
            return ExitCode::FAILURE;
        }
    };
    let odds = [
        Odds::new(Rules::SHOOTOUT, 0, BOT_SCORES, PERSON_SCORES),
        Odds::new(Rules::SHOOTOUT, 1, BOT_SCORES, PERSON_SCORES),
    ];
    let odds = &odds;
    let depths: &[u32] = if wins { &[1, 2] } else { &[1, 2, 3] };
    let last = depths.len() - 1;
    let threads = 8u64;
    let rows: Vec<Row> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                scope.spawn(move || {
                    (first + t..first + count)
                        .step_by(threads as usize)
                        .map(|id| {
                            let mind = make(id);
                            let bot_first = id % 2 == 0;
                            let results = depths
                                .iter()
                                .map(|&d| {
                                    if wins {
                                        let shootout = Shootout {
                                            odds: &odds[usize::from(!bot_first)],
                                            table,
                                            deception,
                                        };
                                        shootout.play(&mind, d, id)
                                    } else {
                                        play(&mind, d, deception, &table, bot_first, id)
                                    }
                                })
                                .collect();
                            (
                                id,
                                Row {
                                    keeper: keeper_name(&mind),
                                    shooter: shooter_name(&mind),
                                    results,
                                },
                            )
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut all: Vec<(u64, Row)> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_else(|_| panic!("a worker failed")))
            .collect();
        all.sort_by_key(|r| r.0);
        all.into_iter().map(|r| r.1).collect()
    });
    println!(
        "{} {family} persons from {first}, deception {deception}, react save {react}",
        rows.len()
    );
    for (i, d) in depths.iter().enumerate() {
        let shares: Vec<f64> = rows.iter().map(|r| r.results[i].share()).collect();
        let (m, se) = mean_se(&shares);
        let shoot = rows.iter().fold([0, 0], |a, r| {
            [
                a[0] + r.results[i].shooting[0],
                a[1] + r.results[i].shooting[1],
            ]
        });
        let keep = rows.iter().fold([0, 0], |a, r| {
            [
                a[0] + r.results[i].keeping[0],
                a[1] + r.results[i].keeping[1],
            ]
        });
        let results: Vec<f64> = rows.iter().map(|r| r.results[i].result).collect();
        let (w, wse) = mean_se(&results);
        println!(
            "depth {d}: share {m:.4} ± {se:.4}  shooting {:.4}  keeping {:.4}  result {w:.4} ± {wse:.4}",
            rate(shoot[0], shoot[1]),
            rate(keep[0], keep[1])
        );
    }
    for (i, d) in depths.iter().enumerate().skip(1) {
        let diffs: Vec<f64> = rows
            .iter()
            .map(|r| r.results[i].share() - r.results[0].share())
            .collect();
        let (m, se) = mean_se(&diffs);
        println!(
            "depth {d} - depth 1: share {m:+.4} ± {se:.4} (95% {:+.4} .. {:+.4})",
            m - 1.96 * se,
            m + 1.96 * se
        );
        let diffs: Vec<f64> = rows
            .iter()
            .map(|r| r.results[i].result - r.results[0].result)
            .collect();
        let (m, se) = mean_se(&diffs);
        println!(
            "depth {d} - depth 1: result {m:+.4} ± {se:.4} (95% {:+.4} .. {:+.4})",
            m - 1.96 * se,
            m + 1.96 * se
        );
    }
    let mut by: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    for r in &rows {
        let shoot = |i: usize| rate(r.results[i].shooting[0], r.results[i].shooting[1]);
        let keep = |i: usize| rate(r.results[i].keeping[0], r.results[i].keeping[1]);
        by.entry(format!("keeper {}", r.keeper))
            .or_default()
            .push((shoot(last) - shoot(0), shoot(0)));
        by.entry(format!("shooter {}", r.shooter))
            .or_default()
            .push((keep(last) - keep(0), keep(0)));
    }
    println!(
        "by style, the bot's role against it: depth {} - depth 1 (depth-1 rate)",
        depths[last]
    );
    for (name, v) in &by {
        let diffs: Vec<f64> = v.iter().map(|x| x.0).collect();
        let base: Vec<f64> = v.iter().map(|x| x.1).collect();
        let (m, se) = mean_se(&diffs);
        let (b, _) = mean_se(&base);
        println!("  {name:22} n {:5}  {m:+.4} ± {se:.4}  ({b:.4})", v.len());
    }
    ExitCode::SUCCESS
}
