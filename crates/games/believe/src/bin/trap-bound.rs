use std::process::ExitCode;

use believe::minds::{CallerStyle, LiarStyle, Person};
use believe::traps::{Played, play};

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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [first, count, samples, depths @ ..] = args.as_slice() else {
        eprintln!("usage: trap-bound <first person> <persons> <samples> <depth>...");
        return ExitCode::FAILURE;
    };
    let (Ok(first), Ok(count), Ok(samples)) = (
        first.parse::<u64>(),
        count.parse::<u64>(),
        samples.parse::<u32>(),
    ) else {
        eprintln!("numbers expected");
        return ExitCode::FAILURE;
    };
    let depths: Vec<u32> = depths.iter().filter_map(|d| d.parse().ok()).collect();
    if depths.len() < 2 {
        eprintln!("two depths or more; the first is the baseline");
        return ExitCode::FAILURE;
    }
    let threads = 8u64;
    let depths = &depths;
    let mut rows: Vec<(u64, Person, Vec<Played>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                scope.spawn(move || {
                    (first + t..first + count)
                        .step_by(threads as usize)
                        .map(|id| {
                            let p = Person::drawn(id);
                            let me = (id % 2) as u8;
                            let results = depths
                                .iter()
                                .map(|&d| play(&p, d, samples, me, id))
                                .collect();
                            (id, p, results)
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
        "{} persons from {first}, {samples} sampled worlds",
        rows.len()
    );
    for (i, d) in depths.iter().enumerate() {
        let r: Vec<f64> = rows.iter().map(|x| x.2[i].result).collect();
        let (m, se) = mean_se(&r);
        let lies: (u32, u32) = rows.iter().fold((0, 0), |a, x| {
            (a.0 + x.2[i].bot_lies[0], a.1 + x.2[i].bot_lies[1])
        });
        let calls: (u32, u32) = rows.iter().fold((0, 0), |a, x| {
            (a.0 + x.2[i].bot_calls[0], a.1 + x.2[i].bot_calls[1])
        });
        let actions: f64 =
            rows.iter().map(|x| f64::from(x.2[i].actions)).sum::<f64>() / rows.len().max(1) as f64;
        println!(
            "depth {d}: result {m:.4} ± {se:.4}  lies {:.3}  calls that hit {:.3}  actions {actions:.1}",
            f64::from(lies.0) / f64::from(lies.1.max(1)),
            f64::from(calls.0) / f64::from(calls.1.max(1))
        );
    }
    for (i, d) in depths.iter().enumerate().skip(1) {
        for (name, keep) in [
            ("all", None),
            ("adaptive", Some(true)),
            ("static", Some(false)),
        ] {
            let diffs: Vec<f64> = rows
                .iter()
                .filter(|x| keep.is_none_or(|k| adaptive(&x.1) == k))
                .map(|x| x.2[i].result - x.2[0].result)
                .collect();
            let (m, se) = mean_se(&diffs);
            println!(
                "depth {d} - depth {}: {name:9} n {:5} {m:+.4} ± {se:.4} (95% {:+.4} .. {:+.4})",
                depths[0],
                diffs.len(),
                m - 1.96 * se,
                m + 1.96 * se
            );
        }
    }
    ExitCode::SUCCESS
}
