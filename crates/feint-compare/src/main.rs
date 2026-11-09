use std::process::ExitCode;

use feint_compare::{GAMES, measure, to_json};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: feint-compare <out.json> [matches per level]");
        return ExitCode::FAILURE;
    };
    let matches = args.next().and_then(|m| m.parse().ok()).unwrap_or(40);
    let reports = std::thread::scope(|scope| {
        let jobs: Vec<_> = GAMES
            .iter()
            .map(|&id| scope.spawn(move || measure(id, matches)))
            .collect();
        jobs.into_iter()
            .map(|job| job.join())
            .collect::<Result<Vec<_>, _>>()
    });
    let Ok(reports) = reports else {
        eprintln!("a measurement failed");
        return ExitCode::FAILURE;
    };
    for r in &reports {
        let shares: Vec<String> = r
            .ladder
            .iter()
            .map(|s| format!("{:.2}", s.share()))
            .collect();
        println!(
            "{}: ladder {}, honest level 5 {:.2}, deception adds {:+.2}",
            r.id,
            shares.join(" "),
            r.honest.share(),
            r.payoff()
        );
    }
    if let Err(error) = std::fs::write(&path, to_json(matches, &reports)) {
        eprintln!("{path}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
