use penalty::persons::Habits;
use penalty::styles::{KeeperStyle, KickSeen, Mind, ShooterStyle};
use penalty::traps::{Keep, Shot, Table, horizon, keep_values, play, shoot_values};

const EPS: f64 = 1e-12;

fn trust(rate: f64, start: f64) -> Mind {
    Mind::new(
        KeeperStyle::Trust {
            rate,
            guess: 1.0,
            start,
        },
        ShooterStyle::Static,
        Habits::default(),
    )
}

fn value<T: PartialEq + std::fmt::Debug>(values: &[(T, f64)], a: T) -> f64 {
    values
        .iter()
        .find(|(b, _)| *b == a)
        .map(|&(_, v)| v)
        .unwrap_or_else(|| panic!("{a:?} not offered in {values:?}"))
}

fn honest_win(t: f64) -> f64 {
    1.0 - t * t
}

fn fake_win(t: f64) -> f64 {
    1.0 - (1.0 - t) * (1.0 - t)
}

#[test]
fn one_kick_values_are_the_expected_wins() {
    let values = shoot_values(&trust(0.4, 0.8), 1, true, 1.0);
    assert!((value(&values, Shot::Honest(0)) - honest_win(0.8)).abs() < EPS);
    assert!((value(&values, Shot::Honest(2)) - honest_win(0.8)).abs() < EPS);
    assert!(value(&values, Shot::Honest(1)).abs() < EPS);
    assert!((value(&values, Shot::Fake(0)) - fake_win(0.8)).abs() < EPS);
    assert_eq!(shoot_values(&trust(0.4, 0.8), 1, false, 1.0).len(), 3);
}

#[test]
fn two_kicks_add_the_best_second_kick_after_the_trust_each_first_kick_leaves() {
    let (r, t) = (0.9, 0.45);
    let up = t + r * (1.0 - t);
    let down = t - r * t;
    let both = |t2: f64, fakes: bool| {
        if fakes {
            honest_win(t2).max(fake_win(t2))
        } else {
            honest_win(t2)
        }
    };
    for deception in [1.0, 0.0, 0.4] {
        let later = |t2: f64| deception * both(t2, true) + (1.0 - deception) * both(t2, false);
        let values = shoot_values(&trust(r, t), 2, true, deception);
        let honest = value(&values, Shot::Honest(0));
        let fake = value(&values, Shot::Fake(2));
        assert!(
            (honest - (honest_win(t) + later(up))).abs() < EPS,
            "{deception}"
        );
        assert!(
            (fake - (fake_win(t) + later(down))).abs() < EPS,
            "{deception}"
        );
    }
}

#[test]
fn keeping_one_kick_saves_what_the_corner_odds_say() {
    let mut m = Mind::new(
        KeeperStyle::Static,
        ShooterStyle::WinStay { stay: 0.8 },
        Habits {
            honest: 0.5,
            fake: 0.3,
            ..Habits::default()
        },
    );
    m.learn(KickSeen {
        person_shot: true,
        lean: None,
        shot: Some(2),
        dive: None,
        goal: true,
    });
    let q = 0.6;
    let values = keep_values(&m, 1, &Table { react_save: q });
    assert!((value(&values, Keep::Guess(2)) - 0.8).abs() < EPS);
    assert!((value(&values, Keep::Guess(1)) - 0.1).abs() < EPS);
    assert!((value(&values, Keep::React) - (0.9 * q + 0.1)).abs() < EPS);
    let (honest, fake) = (f64::from(0.5f32), f64::from(0.3f32));
    let none = 1.0 - honest - fake;
    let read = 0.9 * honest + 0.9 * none * q + 0.1;
    assert!((value(&values, Keep::Read) - read).abs() < EPS);
}

#[test]
fn a_static_person_gives_every_depth_the_same_share() {
    let table = Table { react_save: 0.7 };
    for id in 0..40 {
        let m = Mind::new(
            KeeperStyle::Static,
            ShooterStyle::Static,
            Habits::person(id),
        );
        let one = play(&m, 1, 0.4, &table, id % 2 == 0, id);
        let three = play(&m, 3, 0.4, &table, id % 2 == 0, id);
        assert_eq!(one, three, "{id}");
        let n = 2;
        let v1 = keep_values(&m, 1, &table);
        let v2 = keep_values(&m, n, &table);
        let best1 = v1.iter().map(|x| x.1).fold(f64::NEG_INFINITY, f64::max);
        for ((a, x), (b, y)) in v1.iter().zip(&v2) {
            assert_eq!(a, b);
            assert!((y - (x + best1)).abs() < 1e-9, "{id}");
        }
    }
}

#[test]
fn a_match_plays_ten_kicks_five_in_each_role() {
    let table = Table { react_save: 0.7 };
    for id in 0..20 {
        let r = play(&Mind::person(id), 2, 0.7, &table, id % 2 == 1, id);
        assert_eq!(r.shooting[1], 5);
        assert_eq!(r.keeping[1], 5);
    }
}

#[test]
fn the_horizon_stops_at_the_last_kick_in_the_role() {
    assert_eq!(horizon(3, 0), 3);
    assert_eq!(horizon(3, 1), 3);
    assert_eq!(horizon(3, 6), 2);
    assert_eq!(horizon(3, 7), 2);
    assert_eq!(horizon(3, 8), 1);
    assert_eq!(horizon(2, 9), 1);
    assert_eq!(horizon(1, 0), 1);
}

#[test]
fn the_log_holds_every_kick_with_the_outcome_that_was_counted() {
    let table = Table { react_save: 0.7 };
    for id in 0..30 {
        let r = play(&Mind::person(id), 2, 0.7, &table, id % 2 == 0, id);
        assert_eq!(r.log.len(), 10);
        let goals_on_bot = r.log.iter().filter(|k| k.person_shot && k.goal).count() as u32;
        let goals_by_bot = r.log.iter().filter(|k| !k.person_shot && k.goal).count() as u32;
        assert_eq!(goals_on_bot, r.keeping[1] - r.keeping[0], "{id}");
        assert_eq!(goals_by_bot, r.shooting[0], "{id}");
        let first_shooter = r.log[0].person_shot;
        assert_eq!(first_shooter, id % 2 == 1);
    }
}
