use airhockey::{Body, Input, InputError, Mallet, Match, MatchEvent, Params, Puck, Side, Vec2};

fn new_match(seed: u64) -> Match {
    Match::new(Params::default(), [Body::default(); 2], seed)
}

fn step(m: &mut Match) -> Vec<MatchEvent> {
    let mut events = Vec::new();
    m.step(&mut events);
    events
}

fn park(m: &mut Match) {
    m.world.mallets[0] = Mallet::at_rest(Vec2::new(-0.95, 0.45));
    m.world.mallets[1] = Mallet::at_rest(Vec2::new(0.95, 0.45));
}

fn score_against(m: &mut Match, conceding: Side) -> Vec<MatchEvent> {
    park(m);
    let (x, vx) = match conceding {
        Side::High => (0.8, 5.0),
        Side::Low => (-0.8, -5.0),
    };
    m.world.puck = Puck {
        pos: Vec2::new(x, 0.0),
        vel: Vec2::new(vx, 0.0),
    };
    for _ in 0..240 {
        let events = step(m);
        if events.iter().any(|e| matches!(e, MatchEvent::Goal { .. })) {
            return events;
        }
    }
    panic!("no goal");
}

#[test]
fn first_serve_side_follows_the_seed_lowest_bit() {
    for (seed, x) in [(0, -0.5), (1, 0.5), (2, -0.5), (u64::MAX, 0.5)] {
        let m = new_match(seed);
        assert_eq!(
            m.world.puck,
            Puck::at_rest(Vec2::new(x, 0.0)),
            "seed {seed}"
        );
    }
}

#[test]
fn goal_serves_puck_at_rest_to_conceding_side() {
    let mut m = new_match(0);
    let events = score_against(&mut m, Side::High);
    assert!(events.contains(&MatchEvent::Goal {
        tick: m.tick() - 1,
        scorer: Side::Low
    }));
    assert_eq!(m.score(), [1, 0]);
    assert_eq!(m.world.puck, Puck::at_rest(Vec2::new(0.5, 0.0)));

    score_against(&mut m, Side::Low);
    assert_eq!(m.score(), [1, 1]);
    assert_eq!(m.world.puck, Puck::at_rest(Vec2::new(-0.5, 0.0)));
}

#[test]
fn serve_onto_a_mallet_pushes_the_mallet_off_without_nan() {
    let mut m = new_match(1);
    score_against(&mut m, Side::Low);
    score_against(&mut m, Side::Low);
    m.world.mallets[0] = Mallet::at_rest(Vec2::new(-0.5, 0.0));
    m.world.puck = Puck {
        pos: Vec2::new(-0.85, 0.05),
        vel: Vec2::new(-5.0, 0.0),
    };
    m.world.mallets[1] = Mallet::at_rest(Vec2::new(0.95, 0.45));
    let mut scored = false;
    for _ in 0..240 {
        let events = step(&mut m);
        let mallet = m.world.mallets[0];
        assert!(
            mallet.pos.is_finite() && mallet.vel.is_finite(),
            "{mallet:?}"
        );
        if events.iter().any(|e| matches!(e, MatchEvent::Goal { .. })) {
            scored = true;
            break;
        }
    }
    assert!(scored, "set-up failed: no goal");
    assert_eq!(m.world.puck, Puck::at_rest(Vec2::new(-0.5, 0.0)));
    let mallet = m.world.mallets[0];
    assert!(
        (mallet.pos.x - -0.59).abs() < 1e-12 && mallet.pos.y == 0.0,
        "{mallet:?}"
    );
}

#[test]
fn match_ends_at_seven_goals_and_freezes() {
    let mut m = new_match(0);
    for goal in 1..=6 {
        score_against(&mut m, Side::High);
        assert_eq!(m.score(), [goal, 0]);
        assert!(!m.is_over());
    }
    let events = score_against(&mut m, Side::High);
    assert_eq!(m.score(), [7, 0]);
    assert!(m.is_over());
    assert!(events.contains(&MatchEvent::Over { tick: m.tick() - 1 }));

    let (tick, digest) = (m.tick(), m.digest());
    for _ in 0..100 {
        assert!(step(&mut m).is_empty());
    }
    assert_eq!(
        (m.tick(), m.digest()),
        (tick, digest),
        "a finished match moved"
    );
    let input = Input {
        tick,
        player: 0,
        target: [-0.5, 0.0],
    };
    assert_eq!(m.apply(input), Err(InputError::MatchOver));
}

#[test]
fn stall_of_seven_seconds_serves_to_the_other_player() {
    let mut m = new_match(0);
    let stall_ticks = 7 * 240;
    for t in 0..stall_ticks - 1 {
        assert!(step(&mut m).is_empty(), "event before 7 s, at tick {t}");
    }
    let events = step(&mut m);
    assert_eq!(
        events,
        vec![MatchEvent::RoundEnd {
            tick: stall_ticks - 1,
            to: Side::High
        }]
    );
    assert_eq!(m.world.puck, Puck::at_rest(Vec2::new(0.5, 0.0)));
    assert_eq!(m.score(), [0, 0]);
}

#[test]
fn input_for_another_tick_player_or_non_finite_target_is_refused() {
    let mut m = new_match(0);
    let before = m.digest();
    let input = |tick, player, target| Input {
        tick,
        player,
        target,
    };
    assert_eq!(
        m.apply(input(5, 0, [-0.5, 0.0])),
        Err(InputError::WrongTick {
            expected: 0,
            got: 5
        })
    );
    assert_eq!(
        m.apply(input(0, 2, [-0.5, 0.0])),
        Err(InputError::BadPlayer(2))
    );
    for bad in [
        [f32::NAN, 0.0],
        [0.0, f32::INFINITY],
        [f32::NEG_INFINITY, 0.0],
    ] {
        assert_eq!(m.apply(input(0, 0, bad)), Err(InputError::NotFinite));
    }
    assert_eq!(m.digest(), before, "a refused input changed the match");

    assert_eq!(m.apply(input(0, 1, [0.3, -0.2])), Ok(()));
    assert_eq!(
        m.world.mallets[1].target,
        Vec2::new(f64::from(0.3f32), f64::from(-0.2f32))
    );
    assert_ne!(m.digest(), before);
    step(&mut m);
    assert!(m.apply(input(0, 0, [-0.5, 0.0])).is_err(), "tick 0 is gone");
    assert_eq!(m.apply(input(1, 0, [-0.5, 0.0])), Ok(()));
}

#[test]
fn apply_order_within_a_tick_does_not_matter() {
    let mut a = new_match(9);
    let mut b = new_match(9);
    let start = a.digest();
    for t in 0..2_000u64 {
        let phase = t as f32 * 0.01;
        let p0 = Input {
            tick: t,
            player: 0,
            target: [-0.5 + 0.4 * (phase % 1.0), 0.3 - 0.6 * (phase % 1.0)],
        };
        let p1 = Input {
            tick: t,
            player: 1,
            target: [0.5 - 0.4 * (phase % 1.0), -0.3 + 0.6 * (phase % 1.0)],
        };
        a.apply(p0).unwrap();
        a.apply(p1).unwrap();
        b.apply(p1).unwrap();
        b.apply(p0).unwrap();
        step(&mut a);
        step(&mut b);
        assert_eq!(a.digest(), b.digest(), "diverged at tick {t}");
    }
    assert_ne!(a.digest(), start, "nothing moved");
}

#[test]
fn stall_clock_does_not_run_while_the_puck_is_on_the_centre_line() {
    let mut m = new_match(0);
    park(&mut m);
    m.world.puck = Puck::at_rest(Vec2::ZERO);
    for _ in 0..240 * 30 {
        assert!(step(&mut m).is_empty());
    }
    assert_eq!(m.stall_ticks(), 0);
}
