mod sequence;
use sequence::Lcg;

use airhockey::{BadBody, Body, Mallet, Params, Region, Side, Vec2};

const EPS: f64 = 1e-9;

fn inside(region: &Region, p: Vec2) -> bool {
    p.x >= region.min.x - EPS
        && p.x <= region.max.x + EPS
        && p.y >= region.min.y - EPS
        && p.y <= region.max.y + EPS
}

#[test]
fn regions_are_own_half_inside_the_rails() {
    let params = Params::default();
    let low = Region::of(Side::Low, &params);
    assert_eq!(low.min, Vec2::new(-0.95, -0.45));
    assert_eq!(low.max, Vec2::new(-0.05, 0.45));
    let high = Region::of(Side::High, &params);
    assert_eq!(high.min, Vec2::new(0.05, -0.45));
    assert_eq!(high.max, Vec2::new(0.95, 0.45));
    assert_eq!(low.clamp(Vec2::new(0.3, 9.0)), Vec2::new(-0.05, 0.45));
    assert_eq!(low.clamp(Vec2::new(-0.5, 0.1)), Vec2::new(-0.5, 0.1));
}

#[test]
fn body_limits_must_be_positive_and_finite() {
    let good = Body::new(8.0, 60.0).expect("valid limits");
    assert_eq!((good.max_speed(), good.max_accel()), (8.0, 60.0));
    for (speed, accel) in [
        (0.0, 60.0),
        (8.0, 0.0),
        (-1.0, 60.0),
        (8.0, -1.0),
        (f64::NAN, 60.0),
        (8.0, f64::INFINITY),
    ] {
        assert_eq!(Body::new(speed, accel), Err(BadBody), "{speed}, {accel}");
    }
}

#[test]
fn mallet_respects_speed_and_accel_limits() {
    let params = Params::default();
    let dt = params.substep_dt();
    for (side, body) in [
        (Side::Low, Body::default()),
        (Side::High, Body::new(3.2, 25.0).unwrap()),
    ] {
        let region = Region::of(side, &params);
        let start = region.clamp(Vec2::new(0.0, 0.0));
        let mut mallet = Mallet::at_rest(start);
        let mut rng = Lcg(match side {
            Side::Low => 1,
            Side::High => 2,
        });
        let mut travelled = 0.0;
        for _ in 0..2_000 {
            // Targets anywhere, including the other half and beyond the rails.
            mallet.target = Vec2::new(rng.range(-1.5, 1.5), rng.range(-0.8, 0.8));
            let hold = 1 + (rng.unit() * 120.0) as u32;
            for _ in 0..hold {
                let before = mallet;
                mallet.step(&region, &body, dt);
                let dv = (mallet.vel - before.vel).length();
                assert!(mallet.vel.length() <= body.max_speed() * (1.0 + EPS));
                assert!(
                    dv <= body.max_accel() * dt * (1.0 + EPS),
                    "dv {dv} > {} at {:?}",
                    body.max_accel() * dt,
                    before
                );
                assert!(inside(&region, mallet.pos), "left its region: {mallet:?}");
                travelled += (mallet.pos - before.pos).length();
            }
        }
        assert!(travelled > 20.0, "the mallet barely moved: {travelled} m");
    }
}

#[test]
fn mallet_reaches_target_without_overshoot_in_near_minimal_time() {
    let params = Params::default();
    let dt = params.substep_dt();
    let region = Region::of(Side::Low, &params);
    let body = Body::default();
    let start = Vec2::new(-0.8, 0.0);
    let target = Vec2::new(-0.2, 0.3);
    let distance = (target - start).length();
    let dir = (target - start) * (1.0 / distance);
    let mut mallet = Mallet::at_rest(start);
    mallet.target = target;

    // From rest to rest under acceleration a, top speed never reached here:
    // the bang-bang time is 2·sqrt(d/a).
    let peak = (body.max_accel() * distance).sqrt();
    assert!(peak < body.max_speed());
    let minimal = 2.0 * (distance / body.max_accel()).sqrt();

    let mut progress = 0.0;
    let mut steps = 0u32;
    while mallet.pos != target || mallet.vel != Vec2::ZERO {
        mallet.step(&region, &body, dt);
        steps += 1;
        let along = (mallet.pos - start).dot(dir);
        assert!(along >= progress - EPS, "moved backwards at step {steps}");
        assert!(along <= distance + EPS, "overshot at step {steps}");
        progress = along;
        assert!(steps < 2_000, "never arrived: {mallet:?}");
    }
    let took = f64::from(steps) * dt;
    assert!(
        took <= minimal * 1.1 + 4.0 * dt,
        "took {took} s, minimal {minimal} s"
    );
}

#[test]
fn mallet_cannot_leave_own_half_or_table() {
    let params = Params::default();
    let dt = params.substep_dt();
    let body = Body::default();
    let cases = [
        (Side::Low, Vec2::new(0.5, 0.0), Vec2::new(-0.05, 0.0)),
        (Side::Low, Vec2::new(-0.05, 0.0), Vec2::new(-0.05, 0.0)),
        (Side::Low, Vec2::new(-2.0, 2.0), Vec2::new(-0.95, 0.45)),
        (Side::Low, Vec2::new(-0.95, -0.45), Vec2::new(-0.95, -0.45)),
        (Side::High, Vec2::new(-0.5, 0.0), Vec2::new(0.05, 0.0)),
        (Side::High, Vec2::new(3.0, -3.0), Vec2::new(0.95, -0.45)),
    ];
    for (side, aim, expected) in cases {
        let region = Region::of(side, &params);
        let mut mallet = Mallet::at_rest(Vec2::new(
            (region.min.x + region.max.x) / 2.0,
            (region.min.y + region.max.y) / 2.0,
        ));
        mallet.target = aim;
        for _ in 0..4_000 {
            mallet.step(&region, &body, dt);
            assert!(inside(&region, mallet.pos), "{side:?} {aim:?}: {mallet:?}");
        }
        assert_eq!(mallet.pos, expected, "{side:?} aiming at {aim:?}");
        assert_eq!(mallet.vel, Vec2::ZERO);
    }
}

#[test]
fn snap_to_target_respects_max_speed() {
    let params = Params::default();
    let dt = params.substep_dt();
    let region = Region::of(Side::Low, &params);
    let body = Body::new(0.05, 200.0).unwrap();
    let mut mallet = Mallet::at_rest(Vec2::new(-0.5, 0.0));
    mallet.target = Vec2::new(-0.5 + 2.0e-4, 0.0);
    mallet.step(&region, &body, dt);
    let speed = (mallet.pos.x + 0.5) / dt;
    assert!(
        speed <= body.max_speed() * (1.0 + EPS),
        "moved at {speed} m/s, limit {}",
        body.max_speed()
    );
}

#[test]
fn tiny_but_positive_accel_neither_panics_nor_moves() {
    let params = Params::default();
    let region = Region::of(Side::Low, &params);
    let body = Body::new(8.0, 1e-305).expect("positive and finite");
    let mut mallet = Mallet::at_rest(Vec2::new(-0.5, 0.0));
    mallet.target = Vec2::new(-0.3, 0.2);
    for _ in 0..40 {
        mallet.step(&region, &body, params.substep_dt());
    }
    assert!(mallet.pos.is_finite() && mallet.vel.is_finite());
    assert!((mallet.pos - Vec2::new(-0.5, 0.0)).length() < 1e-12);
}
