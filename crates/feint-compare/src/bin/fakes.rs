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
    let per_match = if r.matches == 0 {
        0.0
    } else {
        f64::from(r.fakes) / r.matches as f64
    };
    println!(
        "  detected fakes {per_match:.3} a match; the detector's false alarms on a bot that never fakes: {:.3} a match{}",
        r.noise,
        if per_match <= r.noise {
            " -> no sign of fakes"
        } else {
            ""
        }
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
    r.noise = calibrate(honest(five), NOISE_SEEDS).detected_rate();
    report(&r);
    if let Err(error) = std::fs::write(&path, r.to_json()) {
        eprintln!("{path}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
