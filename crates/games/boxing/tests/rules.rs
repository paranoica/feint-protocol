use boxing::{Body, Bout, Event, Height, Kind, Params, Phase, Stage};

fn body() -> Body {
    Body {
        speed: 1.5,
        hand: 1.0,
    }
}

fn bout() -> Bout {
    Bout::new(Params::default(), [body(); 2], 7)
}

fn close(b: &mut Bout, gap: f64) {
    b.fighters[0].x = -gap / 2.0;
    b.fighters[0].target = -gap / 2.0;
    b.fighters[1].x = gap / 2.0;
    b.fighters[1].target = gap / 2.0;
}

fn run(b: &mut Bout, ticks: u32) -> Vec<Event> {
    let mut events = Vec::new();
    for _ in 0..ticks {
        b.step(&mut events);
    }
    events
}

fn hold(b: &mut Bout, seat: u8, guard: Height) {
    let x = b.fighters[usize::from(seat)].x;
    b.order(seat, x, guard, None, false);
}

fn attack(b: &mut Bout, seat: u8, kind: Kind, height: Height) {
    let f = b.fighters[usize::from(seat)];
    b.order(seat, f.x, f.guard, Some((kind, height)), false);
}

#[test]
fn the_default_params_are_valid() {
    assert_eq!(Params::default().validate(), Ok(()));
}

#[test]
fn params_without_a_slower_hook_or_room_are_refused() {
    let d = Params::default;
    let bad = [
        Params {
            hook_windup: d().jab_windup,
            ..d()
        },
        Params {
            min_gap: d().start_gap,
            ..d()
        },
        Params {
            start_gap: 2.0 * d().ring,
            ..d()
        },
        Params {
            min_hand: d().max_hand + 0.1,
            ..d()
        },
        Params {
            cancel_recover: 0,
            ..d()
        },
        Params {
            rounds: 1000,
            ..d()
        },
        Params {
            jab_reach: f64::NAN,
            ..d()
        },
        Params {
            decision_hz: d().tick_hz + 1,
            ..d()
        },
        Params {
            guard_switch: 10 * d().tick_hz + 1,
            ..d()
        },
        Params {
            hook_windup: 10 * d().tick_hz + 1,
            ..d()
        },
        Params {
            jab_points: 0,
            ..d()
        },
        Params {
            hook_points: d().points_to_win + 1,
            ..d()
        },
    ];
    for p in bad {
        assert!(p.validate().is_err(), "{p:?}");
    }
}

#[test]
fn validation_answers_without_overflowing_on_extreme_rates() {
    let p = Params {
        tick_hz: u32::MAX,
        ..Params::default()
    };
    assert_eq!(p.validate(), Ok(()), "a fast clock alone is a valid setup");
}

#[test]
fn a_jab_lands_exactly_after_its_wind_up_and_the_fighters_part() {
    let mut b = bout();
    close(&mut b, 0.7);
    hold(&mut b, 1, Height::Low);
    attack(&mut b, 0, Kind::Jab, Height::High);
    let windup = Params::default().jab_windup;
    let before = run(&mut b, windup);
    assert!(
        before.is_empty(),
        "nothing lands one tick early: {before:?}"
    );
    assert_eq!(b.score, [0, 0]);
    let events = run(&mut b, 1);
    assert_eq!(events, vec![Event::Hit { by: 0, points: 1 }]);
    assert_eq!(b.score, [1, 0]);
    assert_eq!(b.landed, [1, 0]);
    assert_eq!(b.phase, Phase::Break);
    let half = Params::default().start_gap / 2.0;
    assert_eq!([b.fighters[0].x, b.fighters[1].x], [-half, half]);
    assert_eq!(b.fighters[0].stage, Stage::Idle);
    assert_eq!(b.fighters[0].strikes, 1);
    assert_eq!(b.fighters[0].last, Some(Height::High));
}

#[test]
fn a_slower_hand_winds_up_longer() {
    let mut b = Bout::new(
        Params::default(),
        [
            Body {
                speed: 1.5,
                hand: 0.5,
            },
            body(),
        ],
        0,
    );
    assert_eq!(b.windup(0, Kind::Jab), 2 * Params::default().jab_windup);
    assert_eq!(b.windup(1, Kind::Jab), Params::default().jab_windup);
    close(&mut b, 0.7);
    hold(&mut b, 1, Height::Low);
    attack(&mut b, 0, Kind::Jab, Height::High);
    assert!(run(&mut b, 2 * Params::default().jab_windup).is_empty());
    assert_eq!(run(&mut b, 1), vec![Event::Hit { by: 0, points: 1 }]);
}

#[test]
fn the_matching_guard_blocks_and_the_attacker_pays_the_blocked_recovery() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, 0.7);
    hold(&mut b, 1, Height::High);
    attack(&mut b, 0, Kind::Jab, Height::High);
    let events = run(&mut b, p.jab_windup + 1);
    assert_eq!(events, vec![Event::Blocked { by: 0 }]);
    assert_eq!(b.score, [0, 0]);
    assert_eq!(b.blocked, [1, 0]);
    assert_eq!(b.phase, Phase::Fight);
    assert!(matches!(
        b.fighters[0].stage,
        Stage::Strike { blocked: true, .. }
    ));
    hold(&mut b, 0, Height::High);
    run(&mut b, p.active);
    assert_eq!(
        b.fighters[0].stage,
        Stage::Recover {
            left: p.blocked_recover
        }
    );
}

#[test]
fn a_strike_out_of_reach_whiffs_and_recovers_by_its_kind() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, p.jab_reach + 0.05);
    hold(&mut b, 1, Height::High);
    attack(&mut b, 0, Kind::Jab, Height::High);
    let events = run(&mut b, p.jab_windup + 1);
    assert_eq!(events, vec![Event::Whiff { by: 0 }]);
    assert_eq!(b.score, [0, 0]);
    hold(&mut b, 0, Height::High);
    run(&mut b, p.active);
    assert_eq!(
        b.fighters[0].stage,
        Stage::Recover {
            left: p.jab_recover
        }
    );
}

#[test]
fn reach_is_inclusive_at_the_boundary() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, p.jab_reach);
    let gap = b.gap();
    assert!(gap <= p.jab_reach);
    hold(&mut b, 1, Height::Low);
    attack(&mut b, 0, Kind::Jab, Height::High);
    assert_eq!(
        run(&mut b, p.jab_windup + 1),
        vec![Event::Hit { by: 0, points: 1 }]
    );
}

#[test]
fn a_guard_blocks_at_the_reach_boundary_too() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, p.jab_reach);
    hold(&mut b, 1, Height::High);
    attack(&mut b, 0, Kind::Jab, Height::High);
    assert_eq!(
        run(&mut b, p.jab_windup + 1),
        vec![Event::Blocked { by: 0 }]
    );
}

#[test]
fn a_cancel_sent_with_the_attack_is_spent_before_the_wind_up() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, 0.7);
    hold(&mut b, 1, Height::Low);
    let f = b.fighters[0];
    b.order(0, f.x, f.guard, Some((Kind::Jab, Height::High)), true);
    let events = run(&mut b, p.jab_windup + 1);
    assert_eq!(events, vec![Event::Hit { by: 0, points: 1 }]);
}

#[test]
fn a_hook_scores_two_but_reaches_less_than_a_jab() {
    let p = Params::default();
    let mid = (p.hook_reach + p.jab_reach) / 2.0;
    let mut b = bout();
    close(&mut b, mid);
    hold(&mut b, 1, Height::Low);
    attack(&mut b, 0, Kind::Hook, Height::High);
    assert_eq!(run(&mut b, p.hook_windup + 1), vec![Event::Whiff { by: 0 }]);
    let mut b = bout();
    close(&mut b, p.hook_reach - 0.05);
    hold(&mut b, 1, Height::Low);
    attack(&mut b, 0, Kind::Hook, Height::High);
    assert_eq!(
        run(&mut b, p.hook_windup + 1),
        vec![Event::Hit { by: 0, points: 2 }]
    );
    assert_eq!(b.score, [2, 0]);
}

#[test]
fn the_guard_holds_during_a_wind_up_and_opens_in_recovery() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, 0.7);
    hold(&mut b, 0, Height::High);
    attack(&mut b, 1, Kind::Hook, Height::High);
    run(&mut b, 1);
    assert!(matches!(b.fighters[1].stage, Stage::WindUp { .. }));
    assert_eq!(b.fighters[1].covering(), Some(Height::High));
    let f = b.fighters[1];
    b.order(1, f.x, f.guard, None, true);
    let events = run(&mut b, 1);
    assert_eq!(events, vec![Event::Feint { by: 1 }]);
    assert_eq!(b.fighters[1].covering(), None);
    attack(&mut b, 0, Kind::Jab, Height::High);
    let f = b.fighters[1];
    b.order(1, f.x, f.guard, None, false);
    let events = run(&mut b, p.jab_windup + 1);
    assert!(
        events.contains(&Event::Blocked { by: 0 }),
        "the cancel recovery is over before a jab lands, so the guard is back: {events:?}"
    );
}

#[test]
fn a_cancel_turns_the_wind_up_into_a_recovery_without_a_strike() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, 0.7);
    hold(&mut b, 1, Height::Low);
    attack(&mut b, 0, Kind::Hook, Height::High);
    run(&mut b, 10);
    let f = b.fighters[0];
    b.order(0, f.x, f.guard, None, true);
    let events = run(&mut b, 1);
    assert_eq!(events, vec![Event::Feint { by: 0 }]);
    assert_eq!(
        b.fighters[0].stage,
        Stage::Recover {
            left: p.cancel_recover
        }
    );
    assert_eq!(b.fighters[0].feints, 1);
    assert_eq!(b.fighters[0].strikes, 0);
    let events = run(&mut b, p.hook_windup);
    assert!(
        events.is_empty(),
        "the cancelled hook never lands: {events:?}"
    );
    assert_eq!(b.score, [0, 0]);
    assert_eq!(b.fighters[0].stage, Stage::Idle);
}

#[test]
fn a_cancel_outside_a_wind_up_does_not_wait_for_the_next_one() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, 0.7);
    hold(&mut b, 1, Height::Low);
    let f = b.fighters[0];
    b.order(0, f.x, f.guard, None, true);
    run(&mut b, 1);
    attack(&mut b, 0, Kind::Jab, Height::High);
    let events = run(&mut b, p.jab_windup + 1);
    assert_eq!(events, vec![Event::Hit { by: 0, points: 1 }]);
    assert_eq!(b.fighters[0].feints, 0);
}

#[test]
fn a_guard_change_leaves_both_heights_open_while_it_switches() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, 0.7);
    attack(&mut b, 0, Kind::Jab, Height::Low);
    run(&mut b, p.jab_windup - p.guard_switch / 2);
    hold(&mut b, 1, Height::Low);
    hold(&mut b, 0, Height::High);
    assert_eq!(b.fighters[1].covering(), None);
    let events = run(&mut b, p.guard_switch);
    assert_eq!(
        events,
        vec![Event::Hit { by: 0, points: 1 }],
        "the new low guard is not up yet"
    );
}

#[test]
fn a_guard_change_completed_in_time_blocks() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, 0.7);
    attack(&mut b, 0, Kind::Jab, Height::Low);
    run(&mut b, 1);
    hold(&mut b, 1, Height::Low);
    hold(&mut b, 0, Height::High);
    let events = run(&mut b, p.jab_windup);
    assert_eq!(events, vec![Event::Blocked { by: 0 }]);
}

#[test]
fn simultaneous_strikes_resolve_for_both_seats() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, 0.7);
    b.order(
        0,
        b.fighters[0].x,
        Height::High,
        Some((Kind::Jab, Height::Low)),
        false,
    );
    b.order(
        1,
        b.fighters[1].x,
        Height::High,
        Some((Kind::Jab, Height::Low)),
        false,
    );
    run(&mut b, p.guard_switch);
    let events = run(&mut b, p.jab_windup + 1 - p.guard_switch);
    assert_eq!(
        events,
        vec![
            Event::Hit { by: 0, points: 1 },
            Event::Hit { by: 1, points: 1 }
        ]
    );
    assert_eq!(b.score, [1, 1]);
}

#[test]
fn the_fighters_keep_the_minimum_gap_and_stay_in_the_ring() {
    let p = Params::default();
    let mut b = bout();
    b.order(0, p.ring, Height::High, None, false);
    b.order(1, -p.ring, Height::High, None, false);
    run(&mut b, 2 * p.tick_hz);
    assert!((b.gap() - p.min_gap).abs() < 1e-9, "gap {}", b.gap());
    assert!((b.fighters[0].x + b.fighters[1].x).abs() < 1e-9);
    let mut b = bout();
    b.order(0, -p.ring, Height::High, None, false);
    b.order(1, -p.ring, Height::High, None, false);
    run(&mut b, 10 * p.tick_hz);
    assert_eq!(b.fighters[0].x, -p.ring);
    assert!((b.fighters[1].x - (-p.ring + p.min_gap)).abs() < 1e-9);
}

#[test]
fn a_fighter_walks_at_its_speed_and_stands_still_while_striking() {
    let p = Params::default();
    let mut b = bout();
    let start = b.fighters[0].x;
    b.order(0, start - 1.0, Height::High, None, false);
    run(&mut b, p.tick_hz / 2);
    let walked = start - b.fighters[0].x;
    assert!((walked - 0.75).abs() < 1e-9, "walked {walked}");
    let mut b = bout();
    close(&mut b, 0.7);
    hold(&mut b, 1, Height::High);
    attack(&mut b, 0, Kind::Jab, Height::High);
    run(&mut b, p.jab_windup + 1);
    let x = b.fighters[0].x;
    b.order(0, x - 1.0, Height::High, None, false);
    run(&mut b, p.active - 1);
    assert_eq!(b.fighters[0].x, x);
    run(&mut b, 2);
    assert!(b.fighters[0].x < x);
}

#[test]
fn a_break_freezes_the_fighters_then_the_fight_resumes() {
    let p = Params::default();
    let mut b = bout();
    close(&mut b, 0.7);
    hold(&mut b, 1, Height::Low);
    attack(&mut b, 0, Kind::Jab, Height::High);
    run(&mut b, p.jab_windup + 1);
    assert_eq!(b.phase, Phase::Break);
    b.order(0, 0.0, Height::Low, None, false);
    let x = b.fighters[0].x;
    run(&mut b, p.break_ticks - 1);
    assert_eq!(b.phase, Phase::Break);
    assert_eq!(b.fighters[0].x, x);
    assert_eq!(b.fighters[0].covering(), Some(Height::Low));
    run(&mut b, 1);
    assert_eq!(b.phase, Phase::Fight);
}

#[test]
fn rounds_end_with_a_break_and_the_match_ends_on_time() {
    let p = Params::default();
    let mut b = bout();
    let mut events = Vec::new();
    for _ in 0..p.round_ticks {
        b.step(&mut events);
    }
    assert_eq!(b.phase, Phase::Break);
    assert_eq!(events, vec![Event::RoundEnd]);
    events.clear();
    while !b.over() {
        b.step(&mut events);
    }
    assert_eq!(b.tick, p.match_ticks());
    assert_eq!(events, vec![Event::RoundEnd, Event::Over]);
    let tick = b.tick;
    b.step(&mut events);
    assert_eq!(b.tick, tick, "an ended match does not tick");
}

#[test]
fn a_round_that_ends_during_a_break_still_ends_and_restarts_the_break() {
    let p = Params::default();
    let mut b = bout();
    let mut events = Vec::new();
    while b.tick + u64::from(p.jab_windup) + 1 + 20 < u64::from(p.round_ticks) {
        b.step(&mut events);
    }
    events.clear();
    close(&mut b, 0.7);
    hold(&mut b, 1, Height::Low);
    attack(&mut b, 0, Kind::Jab, Height::High);
    run(&mut b, p.jab_windup + 1);
    assert_eq!(b.phase, Phase::Break);
    let rest = run(&mut b, 20);
    assert_eq!(b.tick, u64::from(p.round_ticks));
    assert_eq!(rest, vec![Event::RoundEnd]);
    assert_eq!(b.phase, Phase::Break);
    assert_eq!(b.phase_left, p.break_ticks);
}

#[test]
fn a_hit_on_the_last_tick_of_a_round_also_ends_the_round() {
    let p = Params::default();
    let mut b = bout();
    let mut events = Vec::new();
    while b.tick + u64::from(p.jab_windup) + 1 < u64::from(p.round_ticks) {
        b.step(&mut events);
    }
    close(&mut b, 0.7);
    hold(&mut b, 1, Height::Low);
    attack(&mut b, 0, Kind::Jab, Height::High);
    let events = run(&mut b, p.jab_windup + 1);
    assert_eq!(b.tick, u64::from(p.round_ticks));
    assert_eq!(
        events,
        vec![Event::Hit { by: 0, points: 1 }, Event::RoundEnd]
    );
}

#[test]
fn the_match_ends_at_the_points_to_win() {
    let p = Params {
        points_to_win: 2,
        ..Params::default()
    };
    let mut b = Bout::new(p.clone(), [body(); 2], 0);
    close(&mut b, 0.6);
    hold(&mut b, 1, Height::Low);
    attack(&mut b, 0, Kind::Hook, Height::High);
    let events = run(&mut b, p.hook_windup + 1);
    assert_eq!(events, vec![Event::Hit { by: 0, points: 2 }, Event::Over]);
    assert!(b.over());
}

#[test]
fn the_digest_tracks_the_state() {
    let a = bout();
    let mut b = bout();
    assert_eq!(a.digest(), b.digest());
    b.fighters[1].guard = Height::Low;
    assert_ne!(a.digest(), b.digest());
    let mut c = bout();
    c.fighters[0].queued = Some((Kind::Jab, Height::Low));
    assert_ne!(a.digest(), c.digest());
}
