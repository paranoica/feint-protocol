use std::process::ExitCode;
use std::time::Instant;

use believe::minds::{CallerStyle, LiarStyle, Person};
use believe::search::{Cards, Oracle, PolicyModel, PolicyPrior, choose};
use believe::trained::{CHECKPOINTS, policy};
use believe::traps::{leaf, learn};
use believe::{Move, Table};
use feint_ai::search::Budget;
use feint_protocol::{ChaCha8Rng, ObservationT};
use feint_turn::{TurnPolicy, sample_index, unit_draw};
use rand_chacha::rand_core::{RngCore, SeedableRng};

const TOP_K: usize = 8;
const EXPLORE: f64 = 1.5;
const BOT_SALT: u64 = 0x5345_4152_4348;

struct Bot {
    depth: u32,
    iterations: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Against {
    Players,
    Policy,
}

struct Outcome {
    result: f64,
    decisions: u32,
    seconds: f64,
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

fn adaptive(p: &Person) -> bool {
    matches!(p.caller, CallerStyle::Trust { .. }) || matches!(p.liar, LiarStyle::Wary { .. })
}

struct Brains<'a> {
    raw: &'a mut TurnPolicy,
    rival: &'a mut TurnPolicy,
    prior: &'a mut PolicyPrior,
    model: &'a PolicyModel,
    against: Against,
}

fn play(
    game: &Cards,
    brains: &mut Brains<'_>,
    start: &Person,
    bot: &Bot,
    me: u8,
    seed: u64,
) -> Outcome {
    let params = game.params().clone();
    let mut table = Table::deal(&params, seed);
    let mut person = *start;
    let mut obs = ObservationT::default();
    let mut probabilities = Vec::new();
    let (mut decisions, mut seconds) = (0u32, 0.0f64);
    while table.outcome.is_none() {
        let player = table.to_act;
        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ (u64::from(table.actions) << 40));
        let m = if player == me {
            game.observe(&table, me, &mut obs);
            let mut bot_rng = ChaCha8Rng::seed_from_u64(rng.next_u64() ^ BOT_SALT);
            let started = Instant::now();
            let m = if bot.depth == 0 {
                let features = obs.state.as_deref().unwrap_or_default();
                let legal = obs.legal.as_deref().unwrap_or_default();
                brains
                    .raw
                    .probabilities(features, legal, &mut probabilities);
                sample_index(&probabilities, legal, unit_draw(&mut bot_rng))
                    .and_then(|i| i32::try_from(i).ok())
                    .and_then(|i| game.moves().decode(i))
            } else {
                let budget = Budget {
                    iterations: bot.iterations,
                    depth: bot.depth,
                    top_k: TOP_K,
                    explore: EXPLORE,
                };
                match brains.against {
                    Against::Players => {
                        let model = Oracle { person, me };
                        choose(game, &obs, &model, brains.prior, budget, &mut bot_rng)
                    }
                    Against::Policy => {
                        choose(game, &obs, brains.model, brains.prior, budget, &mut bot_rng)
                    }
                }
            };
            seconds += started.elapsed().as_secs_f64();
            decisions += 1;
            match m {
                Some(m) => m,
                None => break,
            }
        } else if brains.against == Against::Policy {
            game.observe(&table, player, &mut obs);
            let features = obs.state.as_deref().unwrap_or_default();
            let legal = obs.legal.as_deref().unwrap_or_default();
            brains
                .rival
                .probabilities(features, legal, &mut probabilities);
            match sample_index(&probabilities, legal, unit_draw(&mut rng))
                .and_then(|i| i32::try_from(i).ok())
                .and_then(|i| game.moves().decode(i))
            {
                Some(m) => m,
                None => break,
            }
        } else {
            let u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
            let options = person.moves(&params, &table, player);
            let mut acc = 0.0;
            let mut pick: Option<Move> = None;
            for (m, p) in &options {
                acc += p;
                if u < acc {
                    pick = Some(*m);
                    break;
                }
            }
            match pick.or_else(|| options.iter().rev().find(|x| x.1 > 0.0).map(|x| x.0)) {
                Some(m) => m,
                None => break,
            }
        };
        let before = table.clone();
        if table.play(&params, player, &m).is_err() {
            break;
        }
        learn(&mut person, &params, &before, player, &m, me);
    }
    Outcome {
        result: leaf(&table, me),
        decisions,
        seconds,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [first, count, iterations, against, depths @ ..] = args.as_slice() else {
        eprintln!(
            "usage: search-gate <first player> <players> <iterations> <players|policy> <depth>... (depth 0 = the policy alone)"
        );
        return ExitCode::FAILURE;
    };
    let (Ok(first), Ok(count), Ok(iterations)) = (
        first.parse::<u64>(),
        count.parse::<u64>(),
        iterations.parse::<u32>(),
    ) else {
        eprintln!("numbers expected");
        return ExitCode::FAILURE;
    };
    let against = match against.as_str() {
        "players" => Against::Players,
        "policy" => Against::Policy,
        _ => {
            eprintln!("against players or policy");
            return ExitCode::FAILURE;
        }
    };
    let depths: Vec<u32> = depths.iter().filter_map(|d| d.parse().ok()).collect();
    if depths.len() < 2 {
        eprintln!("two depths or more");
        return ExitCode::FAILURE;
    }
    let bots: Vec<Bot> = depths
        .iter()
        .map(|&depth| Bot { depth, iterations })
        .collect();
    let id = CHECKPOINTS[0].id;
    let threads = 8u64;
    let bots = &bots;
    let mut rows: Vec<(u64, Person, Vec<Outcome>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                scope.spawn(move || {
                    let game = Cards::default();
                    let load = || policy(id).unwrap_or_else(|e| panic!("{e}"));
                    let mut raw = load();
                    let mut rival = load();
                    let mut prior = PolicyPrior::new(load());
                    let model = PolicyModel::new(load());
                    (first + t..first + count)
                        .step_by(threads as usize)
                        .map(|pid| {
                            let p = Person::drawn(pid);
                            let me = (pid % 2) as u8;
                            let out = bots
                                .iter()
                                .map(|b| {
                                    let mut brains = Brains {
                                        raw: &mut raw,
                                        rival: &mut rival,
                                        prior: &mut prior,
                                        model: &model,
                                        against,
                                    };
                                    play(&game, &mut brains, &p, b, me, pid)
                                })
                                .collect();
                            (pid, p, out)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_else(|_| panic!("a worker failed")))
            .collect()
    });
    rows.sort_by_key(|r| r.0);
    println!(
        "{} players from {first}, {iterations} iterations, top {TOP_K}, explore {EXPLORE}",
        rows.len()
    );
    for (i, b) in bots.iter().enumerate() {
        let r: Vec<f64> = rows.iter().map(|x| x.2[i].result).collect();
        let (m, se) = mean_se(&r);
        let decisions: u32 = rows.iter().map(|x| x.2[i].decisions).sum();
        let seconds: f64 = rows.iter().map(|x| x.2[i].seconds).sum();
        println!(
            "depth {}: result {m:.4} ± {se:.4}  {:.2} ms per decision",
            b.depth,
            1000.0 * seconds / f64::from(decisions.max(1))
        );
    }
    for base in 0..bots.len() {
        for (i, b) in bots.iter().enumerate().skip(base + 1) {
            for (name, keep) in [
                ("all", None),
                ("adaptive", Some(true)),
                ("static", Some(false)),
            ] {
                let diffs: Vec<f64> = rows
                    .iter()
                    .filter(|x| keep.is_none_or(|k| adaptive(&x.1) == k))
                    .map(|x| x.2[i].result - x.2[base].result)
                    .collect();
                let (m, se) = mean_se(&diffs);
                println!(
                    "depth {} - depth {}: {name:9} n {:5} {m:+.4} ± {se:.4} (95% {:+.4} .. {:+.4})",
                    b.depth,
                    bots[base].depth,
                    diffs.len(),
                    m - 1.96 * se,
                    m + 1.96 * se
                );
            }
        }
    }
    ExitCode::SUCCESS
}
