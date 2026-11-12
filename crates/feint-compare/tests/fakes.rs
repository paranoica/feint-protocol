use feint_ai::Difficulty;
use feint_compare::calibrate::{Calibration, calibrate};
use feint_compare::honest;

const GATE: std::ops::Range<u64> = 20_000..20_100;

fn line(name: &str, c: &Calibration) {
    eprintln!(
        "{name}: side agreement {}/{}, {} matches, labelled {} ({:.2}/match), detected {} ({:.2}/match), precision {:?}, recall {:?}, botched {}, held fakes with no shot {}",
        c.agreed,
        c.reached,
        c.matches,
        c.labelled,
        c.labelled_rate(),
        c.detected,
        c.detected_rate(),
        c.precision(),
        c.recall(),
        c.botched,
        c.unshot
    );
}

#[test]
#[ignore = "release only: bedrock cmd fakes"]
fn detector_recovers_the_hand_made_fake_rate() {
    let five = Difficulty::preset(5).unwrap();
    let faking = calibrate(five, GATE);
    let plain = calibrate(honest(five), GATE);
    line("level 5, deception 0.7", &faking);
    line("level 5, deception 0", &plain);
    let labelled = faking.labelled_rate();
    assert!(
        faking.labelled >= 100,
        "too few labelled fakes to judge: {faking:?}"
    );
    assert!(
        (faking.detected_rate() - labelled).abs() <= 0.2 * labelled,
        "detected {:.2} a match against {labelled:.2} labelled",
        faking.detected_rate()
    );
    assert!(faking.precision().unwrap() >= 0.7, "{faking:?}");
    assert!(faking.recall().unwrap() >= 0.7, "{faking:?}");
    assert!(
        faking.agreement().unwrap() >= 0.9,
        "the shot's side read ahead disagrees with where the puck went: {faking:?}"
    );
    assert_eq!(plain.labelled, 0, "deception 0 labels no fakes: {plain:?}");
    assert!(
        plain.detected_rate() <= 0.2 * labelled,
        "{:.2} false fakes a match at deception 0",
        plain.detected_rate()
    );
}
