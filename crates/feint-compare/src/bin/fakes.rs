use std::process::ExitCode;

use feint_ai::Difficulty;
use feint_compare::calibrate::calibrate;
use feint_compare::honest;
use feint_compare::trained::{
    FakesRun, Source, TrainedReport, fakes_run, hand_made_leans, trained_report_from, wilson,
};
use feint_ml::mlp::Mlp;
use feint_ml::registry::fnv1a64;

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
    println!(
        "  the human model stood on the shown side at contact on {} of {} detected fakes",
        r.leaned, r.fakes
    );
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(run) = fakes_run(&args) else {
        eprintln!("usage: fakes <out.json> | fakes --of <checkpoint.safetensors>");
        return ExitCode::FAILURE;
    };
    let (source, out) = match run {
        FakesRun::Shipped(out) => (Source::Shipped, Some(out)),
        FakesRun::Of(file) => {
            let bytes = match std::fs::read(&file) {
                Ok(bytes) => bytes,
                Err(error) => {
                    eprintln!("{file}: {error}");
                    return ExitCode::FAILURE;
                }
            };
            match Mlp::from_safetensors(&bytes) {
                Ok(mlp) => (Source::File(Box::new(mlp), fnv1a64(&bytes)), None),
                Err(error) => {
                    eprintln!("{file}: {error:?}");
                    return ExitCode::FAILURE;
                }
            }
        }
    };
    let five = Difficulty::preset(5).unwrap_or_else(|| panic!("level 5"));
    if matches!(source, Source::File(..)) {
        let (fakes, leaned) = hand_made_leans(NOISE_SEEDS);
        println!(
            "hand-made level 5 against the human model: it stood on the shown side at contact on {leaned} of {fakes} detected fakes"
        );
    }
    let mut r = trained_report_from(5, &source, SEEDS);
    let noise = calibrate(honest(five), NOISE_SEEDS)
        .per_shot()
        .unwrap_or(0.0);
    r.noise = (noise * 1e6).round() / 1e6;
    report(&r);
    if let Some(path) = out
        && let Err(error) = std::fs::write(&path, r.to_json())
    {
        eprintln!("{path}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
