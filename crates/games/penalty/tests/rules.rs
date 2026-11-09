use penalty::{Body, KickEvent, Params, Phase, Shootout};

fn shootout() -> Shootout {
    let p = Params::default();
    let body = Body {
        dive_speed: p.max_dive_speed,
        lean_rate: p.max_lean_rate,
    };
    Shootout::new(p, [body; 2], 1)
}

fn run_to_ball(s: &mut Shootout) {
    let mut events = Vec::new();
    while s.phase == Phase::RunUp {
        s.step(&mut events);
    }
    assert!(events.is_empty());
}

fn finish_kick(s: &mut Shootout) -> Vec<KickEvent> {
    let mut events = Vec::new();
    while matches!(s.phase, Phase::Strike | Phase::Flight) {
        s.step(&mut events);
    }
    events
}

#[test]
fn a_still_keeper_concedes_a_corner_and_saves_the_middle() {
    let mut s = shootout();
    run_to_ball(&mut s);
    assert!(s.kick(2.8));
    assert_eq!(finish_kick(&mut s), vec![KickEvent::Goal { shooter: 0 }]);
    assert_eq!(s.score, [1, 0]);

    let mut s = shootout();
    run_to_ball(&mut s);
    assert!(s.kick(0.3));
    assert_eq!(finish_kick(&mut s), vec![KickEvent::Saved { shooter: 0 }]);
}

#[test]
fn a_dive_after_the_kick_with_human_reaction_misses_a_corner_but_an_early_dive_saves_it() {
    let p = Params::default();
    let reaction = (0.3 * f64::from(p.tick_hz)) as u32;
    let mut late = shootout();
    run_to_ball(&mut late);
    late.kick(2.8);
    let mut events = Vec::new();
    while late.phase == Phase::Strike {
        late.step(&mut events);
    }
    for _ in 0..reaction {
        late.step(&mut events);
    }
    assert!(late.dive_to(2.8));
    events.extend(finish_kick(&mut late));
    assert_eq!(
        events,
        vec![KickEvent::Goal { shooter: 0 }],
        "a reactive keeper cannot reach the corner"
    );

    let mut early = shootout();
    while early.dist > p.kick_range + 0.4 {
        early.step(&mut Vec::new());
    }
    assert!(early.dive_to(2.8));
    run_to_ball(&mut early);
    early.kick(2.8);
    assert_eq!(
        finish_kick(&mut early),
        vec![KickEvent::Saved { shooter: 0 }]
    );
}

#[test]
fn a_shot_wide_of_the_post_is_no_goal_and_a_keeper_dives_once() {
    let mut s = shootout();
    run_to_ball(&mut s);
    s.kick(4.2);
    assert_eq!(finish_kick(&mut s), vec![KickEvent::Saved { shooter: 0 }]);
    let mut s = shootout();
    assert!(s.dive_to(-2.0));
    assert!(!s.dive_to(2.0), "a dive commits");
    assert!(!s.kick(1.0), "no kick before the ball is reached");
}

#[test]
fn roles_alternate_and_the_shootout_ends_when_it_is_decided() {
    let mut s = shootout();
    let mut shooters = Vec::new();
    let mut guard = 0;
    while !s.over() {
        shooters.push(s.shooter());
        run_to_ball(&mut s);
        let aim = if s.shooter() == 0 { 2.8 } else { 0.0 };
        s.kick(aim);
        let mut events = finish_kick(&mut s);
        while s.phase == Phase::Settle {
            s.step(&mut events);
        }
        guard += 1;
        assert!(guard < 30);
    }
    assert_eq!(&shooters[..4], &[0, 1, 0, 1]);
    assert_eq!(
        s.score,
        [3, 0],
        "three goals to none cannot be caught with two kicks each left"
    );
    assert_eq!(s.kicks, 6);
}

#[test]
fn a_level_shootout_goes_to_sudden_death_and_stops_at_the_cap() {
    let mut s = shootout();
    let mut guard = 0;
    while !s.over() {
        run_to_ball(&mut s);
        s.kick(2.8);
        let mut events = finish_kick(&mut s);
        while s.phase == Phase::Settle {
            s.step(&mut events);
        }
        guard += 1;
        assert!(guard < 40);
    }
    assert_eq!(
        s.kicks, s.params.max_kicks,
        "every kick scores, so only the cap ends it"
    );
    assert_eq!(s.score[0], s.score[1]);
}

#[test]
fn the_aim_is_locked_from_the_plant_so_a_dive_during_the_wind_up_cannot_be_answered() {
    let mut s = shootout();
    run_to_ball(&mut s);
    assert!(s.kick(2.8));
    assert_eq!(s.phase, Phase::Strike);
    assert!(s.dive_to(2.8), "the keeper dives during the wind-up");
    assert!(
        !s.kick(-2.8),
        "the shooter cannot change the kick once planted"
    );
    assert_eq!(s.aim, Some(2.8));
    assert_eq!(finish_kick(&mut s), vec![KickEvent::Saved { shooter: 0 }]);
}
