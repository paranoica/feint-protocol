use airhockey::{BadBody, Params};

type Breaker = fn(&mut Params);

#[test]
fn default_params_are_valid_and_declare_human_body_limits() {
    let params = Params::default();
    assert_eq!(params.validate(), Ok(()));
    assert_eq!((params.max_body_speed, params.max_body_accel), (7.0, 120.0));
}

#[test]
fn the_default_pace_leaves_a_human_time_to_see_a_shot() {
    let params = Params::default();
    let median_human_reaction = 0.27;
    let fastest_crossing = 2.0 * params.half_length / params.puck_speed_cap;
    assert!(
        fastest_crossing >= median_human_reaction,
        "{fastest_crossing} s across the table"
    );
    assert!(params.serve_pause_seconds >= 1.0);
}

#[test]
fn broken_params_are_reported() {
    let cases: [(&str, Breaker); 12] = [
        ("negative serve pause", |p| p.serve_pause_seconds = -1.0),
        ("NaN serve pause", |p| p.serve_pause_seconds = f64::NAN),
        ("endless serve pause", |p| p.serve_pause_seconds = 1e12),
        ("mallet wider than the table", |p| p.half_width = 0.04),
        ("table shorter than two mallets", |p| p.half_length = 0.09),
        ("no substeps", |p| p.substeps = 0),
        ("no ticks", |p| p.tick_hz = 0),
        ("tick rate overflow", |p| p.substeps = 1 << 25),
        ("restitution above 1", |p| p.rail_restitution = 1.2),
        ("damping that reverses the puck", |p| p.damping = 1e4),
        ("no goals to win", |p| p.goals_to_win = 0),
        ("too coarse for the speed cap", |p| {
            p.substeps = 1;
            p.puck_speed_cap = 20.0;
        }),
    ];
    for (what, breaks) in cases {
        let mut params = Params::default();
        breaks(&mut params);
        assert!(params.validate().is_err(), "{what} passed validation");
    }
}

#[test]
fn bodies_are_checked_against_the_declared_limits() {
    let params = Params::default();
    let fastest = params
        .body(7.0, 120.0)
        .expect("the limits themselves are legal");
    assert_eq!((fastest.max_speed(), fastest.max_accel()), (7.0, 120.0));
    for (speed, accel) in [
        (7.001, 40.0),
        (5.0, 120.5),
        (1000.0, 1e7),
        (0.0, 60.0),
        (f64::NAN, 60.0),
    ] {
        assert_eq!(params.body(speed, accel), Err(BadBody), "{speed}, {accel}");
    }
}

#[test]
#[should_panic(expected = "invalid Params")]
fn a_match_refuses_broken_params() {
    let params = Params {
        substeps: 0,
        ..Params::default()
    };
    let _ = airhockey::Match::new(params, [airhockey::Body::default(); 2], 0);
}

#[test]
#[should_panic(expected = "body beyond the declared limits")]
fn a_match_refuses_a_body_beyond_the_declared_limits() {
    let too_fast = airhockey::Body::new(100.0, 60.0).expect("positive and finite");
    let _ = airhockey::Match::new(Params::default(), [too_fast, airhockey::Body::default()], 0);
}

#[test]
fn a_match_accepts_bodies_at_the_declared_limits() {
    let params = Params::default();
    let fastest = params.body(7.0, 120.0).unwrap();
    let m = airhockey::Match::new(params, [fastest; 2], 0);
    assert_eq!(m.tick(), 0);
}
