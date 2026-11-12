use std::process::ExitCode;

use feint_compare::trained::{TrainedReport, trained_report, wilson};

const SEEDS: std::ops::Range<u64> = 30_000..30_200;

fn rate(name: &str, count: u32, scored: u32) {
    let interval =
        wilson(scored, count).map_or("-".to_string(), |(l, h)| format!("[{l:.3}, {h:.3}]"));
    println!("  {name}: {count} attacks, scored {scored}, Wilson 95% {interval}");
}

fn report(r: &TrainedReport) {
    println!(
        "trained level {} ({:016x}) against the human model: {} matches, {} shots",
        r.level, r.checkpoint, r.matches, r.shots
    );
    rate("faked", r.fakes, r.fake_scored);
    rate("honest", r.honest, r.honest_scored);
    if r.links.is_empty() {
        println!("  no fakes found: stage criterion 3 stays open");
    }
    for l in &r.links {
        println!(
            "  /?replay=trained&level={}&seed={}&seat={}&from={}{}",
            r.level,
            l.seed,
            l.seat,
            l.contact,
            if l.scored { " (scored)" } else { "" }
        );
    }
}

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: fakes <out.json>");
        return ExitCode::FAILURE;
    };
    let r = trained_report(5, SEEDS);
    report(&r);
    if let Err(error) = std::fs::write(&path, r.to_json()) {
        eprintln!("{path}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
