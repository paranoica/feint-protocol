use feint_ai::Difficulty;
use feint_compare::calibrate::{Calibration, calibrate};
use feint_compare::honest;

fn line(name: &str, c: &Calibration) {
    let show = |v: Option<f64>| v.map_or("-".to_string(), |v| format!("{v:.2}"));
    println!(
        "{name}: side agreement {} of {}, {} matches, labelled {} ({:.2}/match), detected {} ({:.2}/match), precision {}, recall {}, botched {}, held fakes with no shot {}",
        c.agreed,
        c.reached,
        c.matches,
        c.labelled,
        c.labelled_rate(),
        c.detected,
        c.detected_rate(),
        show(c.precision()),
        show(c.recall()),
        c.botched,
        c.unshot
    );
}

fn main() {
    let mut args = std::env::args()
        .skip(1)
        .map(|a| a.parse::<u64>().unwrap_or(0));
    let from = args.next().unwrap_or(0);
    let to = args.next().unwrap_or(40);
    let five = Difficulty::preset(5).unwrap_or_else(|| panic!("level 5"));
    line("level 5", &calibrate(five, from..to));
    line("honest level 5", &calibrate(honest(five), from..to));
}
