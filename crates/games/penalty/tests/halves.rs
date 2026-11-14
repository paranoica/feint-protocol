use feint_ai::Difficulty;
use penalty::persons::{Advice, TEST_PERSONS, halves};

fn report(level: u8, advice: Advice, matches: u64) -> (f64, f64, f64, f64) {
    let d = Difficulty::preset(level).unwrap();
    let h = halves(d, advice, TEST_PERSONS..TEST_PERSONS + matches);
    let rate = |r: [u32; 2]| f64::from(r[0]) / f64::from(r[1].max(1));
    let (diff, se) = h.difference();
    let all = (h.share(0) * f64::from(h.shooter[0][1] + h.keeper[0][1])
        + h.share(1) * f64::from(h.shooter[1][1] + h.keeper[1][1]))
        / f64::from(h.shooter[0][1] + h.keeper[0][1] + h.shooter[1][1] + h.keeper[1][1]);
    println!(
        "level {level} {advice:?}: {} matches, share {all:.3}; halves {:.3} -> {:.3}, difference {diff:+.3} ± {:.3}; shooter {:.3} -> {:.3}, keeper {:.3} -> {:.3}",
        h.matches,
        h.share(0),
        h.share(1),
        1.96 * se,
        rate(h.shooter[0]),
        rate(h.shooter[1]),
        rate(h.keeper[0]),
        rate(h.keeper[1]),
    );
    (all, diff, se, h.share(1))
}

#[test]
#[ignore = "a measurement: bedrock cmd halves"]
fn halves_against_new_persons() {
    let matches: u64 = std::env::var("HALVES_MATCHES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    report(2, Advice::Count, matches);
    let blind = report(5, Advice::Blind, matches);
    report(5, Advice::Count, matches);
    let oracle = report(5, Advice::Oracle, matches);
    println!(
        "ceiling: oracle share {:.3} against blind {:.3}, gain {:+.3}",
        oracle.0,
        blind.0,
        oracle.0 - blind.0
    );
}
