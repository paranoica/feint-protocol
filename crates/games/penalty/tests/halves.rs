use std::sync::Arc;

use penalty::context::{ContextAdvisor, LAYOUT};
use penalty::persons::{Advice, Halves, TEST_PERSONS, halves, halves_with};

fn report(level: u8, advice: Advice, matches: u64) -> Halves {
    let h = halves(level, advice, TEST_PERSONS..TEST_PERSONS + matches);
    let (diff, se) = h.difference();
    println!(
        "level {level} {advice:?}: {} matches; halves {:.3} -> {:.3}, difference {diff:+.3} ± {:.3}; shooter {:.3} -> {:.3}, keeper {:.3} -> {:.3}",
        h.matches.len(),
        h.share(0),
        h.share(1),
        1.96 * se,
        h.shooter(0),
        h.shooter(1),
        h.keeper(0),
        h.keeper(1),
    );
    h
}

#[test]
#[ignore = "a measurement: bedrock cmd halves"]
fn halves_against_new_persons() {
    let matches: u64 = std::env::var("HALVES_MATCHES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    let level: u8 = std::env::var("HALVES_LEVEL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let blind = report(level, Advice::Blind, matches);
    let count = report(level, Advice::Count, matches);
    let oracle = report(level, Advice::Oracle, matches);
    for (name, h) in [("count", &count), ("oracle", &oracle)] {
        let (d, se) = h.against(&blind);
        println!(
            "{name} against blind, paired by person: {d:+.3} ± {:.3}",
            1.96 * se
        );
    }
    if let Ok(path) = std::env::var("HALVES_CONTEXT") {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let model =
            Arc::new(feint_context::ContextModel::from_safetensors(&bytes, LAYOUT).unwrap());
        let h = halves_with(level, TEST_PERSONS..TEST_PERSONS + matches, &|_| {
            Some(Box::new(ContextAdvisor::new(Arc::clone(&model))))
        });
        let (diff, se) = h.difference();
        println!(
            "level {level} Context: halves {:.3} -> {:.3}, difference {diff:+.3} ± {:.3}; shooter {:.3} -> {:.3}, keeper {:.3} -> {:.3}",
            h.share(0),
            h.share(1),
            1.96 * se,
            h.shooter(0),
            h.shooter(1),
            h.keeper(0),
            h.keeper(1)
        );
        let (d, se) = h.against(&blind);
        println!(
            "context against blind, paired by person: {d:+.3} ± {:.3}",
            1.96 * se
        );
    }
    let best = oracle.share(1) - blind.share(0) - blind.difference().0;
    println!(
        "best case for a learner at level {level}: {best:+.3} (blind first half, oracle second half, less the blind difference)"
    );
}
