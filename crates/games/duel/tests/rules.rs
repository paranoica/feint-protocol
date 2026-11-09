use duel::{Body, Duel, Params, Phase, RoundEvent};

fn duel() -> Duel {
    let p = Params::default();
    let body = Body {
        draw_rate: p.max_draw_rate,
        step_rate: p.max_step_rate,
    };
    let mut d = Duel::new(p, [body; 2], 1);
    let mut events = Vec::new();
    while d.phase == Phase::Ready {
        d.step(&mut events);
    }
    assert!(events.is_empty());
    d
}

fn ticks(seconds: f64) -> u32 {
    (seconds * f64::from(Params::default().tick_hz)).round() as u32
}

fn shoot_with_dodge(dodge_at: Option<u32>, dir: i8) -> Vec<RoundEvent> {
    let mut d = duel();
    d.steer(0, 0.0, true);
    let mut events = Vec::new();
    let mut t = 0;
    while d.phase == Phase::Live && (d.gunners[0].loaded || d.bullets[0].is_some()) {
        if Some(t) == dodge_at {
            assert!(d.roll(1, dir));
        }
        d.step(&mut events);
        t += 1;
        assert!(t < 600, "the shot never resolved");
    }
    events
}

fn commit_tick() -> u32 {
    let p = Params::default();
    let per_tick = p.max_draw_rate / f64::from(p.tick_hz);
    (p.commit_at / per_tick).ceil() as u32
}

#[test]
fn a_centred_shot_hits_a_still_target_and_a_human_cannot_answer_the_commit_but_level_five_can() {
    assert_eq!(
        shoot_with_dodge(None, 1),
        vec![RoundEvent::Hit { shooter: 0 }]
    );
    let commit = commit_tick();
    assert_eq!(
        shoot_with_dodge(Some(commit + ticks(0.3)), 1),
        vec![RoundEvent::Hit { shooter: 0 }],
        "a human seeing the commit rolls too late"
    );
    assert_eq!(
        shoot_with_dodge(Some(commit + ticks(0.15)), -1),
        vec![],
        "a 150 ms reaction to the commit gets out of the line"
    );
    assert_eq!(
        shoot_with_dodge(Some(ticks(0.3)), 1),
        vec![],
        "a human answering the start of the draw escapes a centred shot"
    );
}

#[test]
fn releasing_before_the_commit_is_a_fake_that_keeps_the_bullet() {
    let mut d = duel();
    let mut events = Vec::new();
    d.steer(0, 0.0, true);
    for _ in 0..commit_tick() - 2 {
        d.step(&mut events);
    }
    assert!(!d.gunners[0].committed && d.gunners[0].hand > 0.5);
    d.steer(0, 0.0, false);
    for _ in 0..ticks(1.0) {
        d.step(&mut events);
    }
    assert_eq!(d.gunners[0].hand, 0.0);
    assert!(d.gunners[0].loaded, "the fake fired nothing");
    assert!(d.bullets[0].is_none() && events.is_empty());
}

#[test]
fn the_aim_is_locked_at_the_commit_and_a_lead_lands_where_the_roll_ends() {
    let p = Params::default();
    let mut d = duel();
    let mut events = Vec::new();
    d.steer(0, p.roll_dist, true);
    for _ in 0..commit_tick() {
        d.step(&mut events);
    }
    assert!(d.gunners[0].committed);
    assert_eq!(d.gunners[0].aim, Some(p.roll_dist));
    d.steer(0, -2.0, true);
    assert!(d.roll(1, 1), "the target rolls right after the commit");
    while d.phase == Phase::Live {
        d.step(&mut events);
    }
    assert_eq!(
        events,
        vec![RoundEvent::Hit { shooter: 0 }],
        "the shot led into the roll"
    );
}

#[test]
fn a_miss_reloads_the_gun_the_round_goes_on_and_a_roll_needs_recovery() {
    let p = Params::default();
    let mut d = duel();
    let mut events = Vec::new();
    d.steer(0, 2.0, true);
    while d.bullets[0].is_none() {
        d.step(&mut events);
    }
    assert!(!d.gunners[0].loaded);
    let mut ticks_to_reload = 0;
    while !d.gunners[0].loaded {
        d.step(&mut events);
        ticks_to_reload += 1;
    }
    assert_eq!(ticks_to_reload, p.reload_ticks);
    assert!(events.is_empty(), "a miss ends nothing: {events:?}");
    assert_eq!((d.phase, d.score), (Phase::Live, [0, 0]));
    assert_eq!(d.gunners[0].shots, 1);
    assert_eq!(d.gunners[0].last_response, 0, "the target stood still");

    let mut d = duel();
    d.steer(0, 0.0, true);
    assert!(d.roll(1, -1));
    assert!(!d.roll(1, 1), "no second roll while rolling");
    d.step(&mut events);
    assert!((d.gunners[1].x + p.roll_dist).abs() < 1e-9);
    assert!(!d.roll(1, 1), "no roll during recovery");
    while d.bullets[0].is_some() || d.gunners[0].loaded {
        d.step(&mut events);
    }
    assert_eq!(
        d.gunners[0].last_response, -1,
        "the shooter saw the roll to the left"
    );
    while d.gunners[1].recovery > 0 {
        d.step(&mut events);
    }
    assert!(d.roll(1, 1));
    let mut edge = duel();
    edge.gunners[1].x = p.half_width - 0.2;
    edge.roll(1, 1);
    edge.step(&mut events);
    assert_eq!(
        edge.gunners[1].x, p.half_width,
        "a roll stops at the lane end"
    );
}

#[test]
fn hits_on_the_same_tick_void_the_round_and_a_round_times_out() {
    let mut d = duel();
    let mut events = Vec::new();
    d.steer(0, 0.0, true);
    d.steer(1, 0.0, true);
    while d.phase == Phase::Live {
        d.step(&mut events);
    }
    assert_eq!(events, vec![RoundEvent::Void]);
    assert_eq!(d.score, [0, 0]);

    let mut d = duel();
    let mut events = Vec::new();
    let mut t = 0;
    while d.phase == Phase::Live {
        d.step(&mut events);
        t += 1;
    }
    assert_eq!(t, d.params.round_ticks);
    assert_eq!(events, vec![RoundEvent::Void]);
}

#[test]
fn the_first_to_the_wins_ends_the_match_and_the_cap_ends_a_stalemate() {
    let mut d = duel();
    let mut events = Vec::new();
    let mut guard = 0;
    while !d.over() {
        d.steer(0, d.gunners[1].x, true);
        d.steer(1, 2.4, d.phase == Phase::Live && d.t > 30);
        d.step(&mut events);
        guard += 1;
        assert!(guard < 240 * 600);
    }
    assert_eq!(d.score, [d.params.wins, 0]);
    assert_eq!(d.round, d.params.wins);
    assert_eq!(events.last(), Some(&RoundEvent::Over));

    let short = Params {
        round_ticks: 10,
        ..Params::default()
    };
    let body = Body {
        draw_rate: short.max_draw_rate,
        step_rate: short.max_step_rate,
    };
    let mut d = Duel::new(short, [body; 2], 2);
    let mut events = Vec::new();
    while !d.over() {
        d.step(&mut events);
    }
    assert_eq!(d.round, d.params.max_rounds);
    assert_eq!(d.score, [0, 0]);
    let bad = Params {
        commit_at: 1.0,
        ..Params::default()
    };
    assert!(bad.validate().is_err());
    let narrow = Params {
        half_width: 0.4,
        ..Params::default()
    };
    assert!(narrow.validate().is_err(), "a roll must fit the lane");
}
