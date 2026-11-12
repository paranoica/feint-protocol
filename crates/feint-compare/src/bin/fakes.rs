use std::process::ExitCode;

use feint_ai::Difficulty;
use feint_compare::calibrate::calibrate;
use feint_compare::honest;
use feint_compare::trained::{TrainedReport, trained_report, wilson};

const SEEDS: std::ops::Range<u64> = 30_000..30_200;
const NOISE_SEEDS: std::ops::Range<u64> = 20_000..20_100;

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
    let per_shot = if r.shots == 0 {
        0.0
    } else {
        f64::from(r.fakes) / f64::from(r.shots)
    };
    let verdict = if per_shot <= r.noise {
        " -> no sign of fakes"
    } else {
        ""
    };
    println!(
        "  detected fakes {:.3}% of shots; the detector's false alarms on a bot that never fakes: {:.3}% of shots{verdict}",
        100.0 * per_shot,
        100.0 * r.noise
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
    let five = Difficulty::preset(5).unwrap_or_else(|| panic!("level 5"));
    let mut r = trained_report(5, SEEDS);
    let noise = calibrate(honest(five), NOISE_SEEDS)
        .per_shot()
        .unwrap_or(0.0);
    r.noise = (noise * 1e6).round() / 1e6;
    report(&r);
    if let Err(error) = std::fs::write(&path, r.to_json()) {
        eprintln!("{path}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
