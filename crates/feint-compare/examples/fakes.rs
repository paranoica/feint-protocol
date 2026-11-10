use feint_compare::GAMES;
use feint_compare::attacks::{Side, Study, attacks};

fn line(label: &str, x: &Side) {
    let rate = |a: u32, b: u32| {
        if b == 0 {
            0.0
        } else {
            100.0 * f64::from(a) / f64::from(b)
        }
    };
    println!(
        "  {label}: {} attempts, scored {:.0}%, conceded {:.0}%, {:.2}s each",
        x.attempts,
        rate(x.scored, x.attempts),
        rate(x.conceded, x.attempts),
        if x.attempts == 0 {
            0.0
        } else {
            x.ticks as f64 / f64::from(x.attempts) / 240.0
        }
    );
}

fn report(name: &str, s: &Study) {
    let pct = |a: u32, b: u32| {
        if b == 0 {
            0.0
        } else {
            100.0 * f64::from(a) / f64::from(b)
        }
    };
    println!("{name}");
    line("honest", &s.honest);
    line("with a fake", &s.fake);
    let f = &s.fake;
    println!(
        "  fakes the opponent answered: {} (unanswered {}), bit {:.0}%; scored after a bite {:.0}%, after no bite {:.0}%",
        f.read,
        s.unread,
        pct(f.bit, f.read),
        pct(f.scored_bit, f.bit),
        pct(f.scored_unbit, f.read - f.bit)
    );
}

fn main() {
    for id in GAMES {
        report(id, &attacks(id, 40));
    }
}
