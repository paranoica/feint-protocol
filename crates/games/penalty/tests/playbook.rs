use feint_ai::{Playbook, Stance};
use penalty::{Body, Params, PenaltyPlaybook, Phase, Shootout};

fn in_flight(aim: f64) -> Shootout {
    let p = Params::default();
    let body = Body {
        dive_speed: p.max_dive_speed,
        lean_rate: p.max_lean_rate,
    };
    let mut s = Shootout::new(p, [body; 2], 1);
    let mut events = Vec::new();
    while s.phase == Phase::RunUp {
        s.step(&mut events);
    }
    assert!(s.kick(aim));
    while s.phase != Phase::Flight {
        s.step(&mut events);
    }
    for _ in 0..10 {
        s.step(&mut events);
    }
    s
}

#[test]
fn an_unknown_shooter_is_any_corner_alike_and_an_unknown_keeper_stays_naive() {
    let book = PenaltyPlaybook::new(Params::default());
    let s = in_flight(2.5);
    let third = 1.0 / 3.0;
    assert_eq!(
        book.unknown(&s, s.keeper()),
        Some(vec![
            (Stance::Shoot(0), third),
            (Stance::Shoot(1), third),
            (Stance::Shoot(2), third)
        ])
    );
    assert_eq!(book.unknown(&s, s.shooter()), None);
}
