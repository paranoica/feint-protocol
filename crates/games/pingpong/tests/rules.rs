use pingpong::{Body, Params, Phase, PointEvent, Swing, Table};

fn table(seed: u64) -> Table {
    let p = Params::default();
    let body = Body {
        speed: p.max_speed,
        accel: p.max_accel,
    };
    Table::new(p, [body; 2], seed)
}

fn run(t: &mut Table, ticks: u32) -> Vec<PointEvent> {
    let mut events = Vec::new();
    for _ in 0..ticks {
        t.step(&mut events);
    }
    events
}

fn serve_from(seat: u8, aim: [f64; 2], flight: f64, facing: f64) -> Table {
    let mut t = table(u64::from(seat));
    assert_eq!(t.server, seat);
    let pos = t.paddles[usize::from(seat)].pos;
    t.steer(seat, pos, Some(Swing { aim, flight }), facing);
    t
}

#[test]
fn a_serve_lands_where_it_was_aimed_after_its_flight() {
    let p = Params::default();
    let mut t = serve_from(0, [1.0, 0.4], 0.6, 0.8);
    let mut events = Vec::new();
    t.step(&mut events);
    assert_eq!(t.phase, Phase::Rally);
    assert_eq!(t.hitter, Some(0));
    let start = t.ball.pos;
    let ticks = (0.6 * f64::from(p.tick_hz)) as u32;
    for k in 0..ticks + 10 {
        t.step(&mut events);
        if t.bounces == 1 {
            assert!(
                k + 2 >= ticks && k <= ticks + 2,
                "bounced at {k}, flight {ticks}"
            );
            break;
        }
    }
    assert_eq!(t.bounces, 1);
    assert!(
        (t.ball.pos[0] - 1.0).abs() < 0.02,
        "{:?} from {start:?}",
        t.ball.pos
    );
    assert!((t.ball.pos[1] - 0.4).abs() < 0.02, "{:?}", t.ball.pos);
    assert!(events.is_empty());
}

#[test]
fn a_shot_lands_on_the_opponents_half_whatever_the_sign_of_its_depth() {
    for (seat, depth) in [(0u8, 1.0), (0, -1.0), (1, 1.0), (1, -1.0)] {
        let mut t = serve_from(seat, [depth, 0.0], 0.6, 0.0);
        let mut events = Vec::new();
        while t.bounces == 0 && events.is_empty() {
            t.step(&mut events);
        }
        let side = if seat == 0 { 1.0 } else { -1.0 };
        assert!(
            side * t.ball.pos[0] > 0.0,
            "seat {seat} depth {depth}: {:?}",
            t.ball.pos
        );
    }
}

#[test]
fn a_low_shot_into_the_net_gives_the_point_to_the_receiver() {
    let mut t = table(0);
    t.phase = Phase::Rally;
    t.hitter = Some(0);
    t.ball.pos = [-0.5, 0.0, 0.1];
    t.ball.vel = [5.0, 0.0, 0.0];
    let events = run(&mut t, 60);
    assert_eq!(events.first(), Some(&PointEvent::Point { winner: 1 }));
    assert_eq!(t.score, [0, 1]);
}

#[test]
fn a_high_shot_clears_the_net() {
    let mut t = table(0);
    t.phase = Phase::Rally;
    t.hitter = Some(0);
    t.ball.pos = [-0.5, 0.0, 0.3];
    t.ball.vel = [3.0, 0.0, 0.0];
    let mut events = Vec::new();
    while t.bounces == 0 && events.is_empty() {
        t.step(&mut events);
    }
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(t.bounces, 1);
    assert!(t.ball.pos[0] > 0.0);
}

#[test]
fn a_long_ball_gives_the_point_to_the_receiver_and_a_ball_left_alone_to_the_hitter() {
    let mut long = table(0);
    long.phase = Phase::Rally;
    long.hitter = Some(0);
    long.ball.pos = [-0.5, 0.0, 0.3];
    long.ball.vel = [9.0, 0.0, 0.5];
    let events = run(&mut long, 240);
    assert_eq!(
        events.first(),
        Some(&PointEvent::Point { winner: 1 }),
        "out"
    );
    let mut alone = table(0);
    alone.phase = Phase::Rally;
    alone.hitter = Some(0);
    alone.ball.pos = [0.1, 0.0, 0.3];
    alone.ball.vel = [2.5, 0.0, 0.0];
    let far = alone.paddles[1].pos;
    alone.steer(1, [far[0] + 5.0, far[1] + 5.0], None, 0.0);
    let events = run(&mut alone, 480);
    assert_eq!(
        events.first(),
        Some(&PointEvent::Point { winner: 0 }),
        "second bounce"
    );
}

#[test]
fn the_receiver_cannot_volley_and_returns_after_the_bounce() {
    let mut t = table(0);
    t.phase = Phase::Rally;
    t.hitter = Some(0);
    t.ball.pos = [1.5, 0.0, 0.3];
    t.ball.vel = [0.0, 0.0, 0.0];
    t.paddles[1].pos = [1.5, 0.0];
    t.steer(
        1,
        [1.5, 0.0],
        Some(Swing {
            aim: [1.0, 0.0],
            flight: 0.6,
        }),
        0.0,
    );
    assert_eq!(t.to_hit(), None, "no bounce yet");
    let mut events = Vec::new();
    t.step(&mut events);
    assert_eq!(t.hitter, Some(0), "a volley is not a hit");
    t.bounces = 1;
    t.ball.pos = [1.5, 0.0, 0.3];
    t.ball.vel = [0.0; 3];
    assert_eq!(t.to_hit(), Some(1));
    t.step(&mut events);
    assert_eq!(t.hitter, Some(1));
    assert_eq!(t.bounces, 0);
    assert!(t.ball.vel[0] < 0.0, "back toward seat 0");
}

#[test]
fn a_server_who_never_serves_loses_the_point() {
    let p = Params::default();
    let mut t = table(1);
    let events = run(&mut t, p.serve_ticks + 1);
    assert_eq!(events.first(), Some(&PointEvent::Point { winner: 0 }));
}

#[test]
fn the_serve_passes_every_two_points_and_the_game_needs_a_two_point_lead() {
    let p = Params::default();
    let mut t = table(0);
    let mut servers = Vec::new();
    for _ in 0..6 {
        servers.push(t.server);
        let s = t.server;
        let mut events = Vec::new();
        while events.is_empty() {
            t.step(&mut events);
        }
        assert_eq!(events[0], PointEvent::Point { winner: 1 - s });
        run(&mut t, p.settle_ticks + 1);
    }
    assert_eq!(servers, vec![0, 0, 1, 1, 0, 0]);
    let mut close = table(0);
    close.score = [10, 10];
    close.phase = Phase::Rally;
    close.hitter = Some(1);
    close.ball.pos = [0.5, 0.0, 0.1];
    close.ball.vel = [-5.0, 0.0, 0.0];
    let events = run(&mut close, 60);
    assert_eq!(close.score, [11, 10]);
    assert!(!events.contains(&PointEvent::Over), "11-10 is not a win");
    let mut won = table(0);
    won.score = [10, 8];
    won.phase = Phase::Rally;
    won.hitter = Some(1);
    won.ball.pos = [0.5, 0.0, 0.1];
    won.ball.vel = [-5.0, 0.0, 0.0];
    let events = run(&mut won, 60);
    assert_eq!(won.score, [11, 8]);
    assert!(events.contains(&PointEvent::Over));
    assert!(won.over());
    let mut capped = table(0);
    capped.score = [20, 19];
    capped.phase = Phase::Rally;
    capped.hitter = Some(1);
    capped.ball.pos = [0.5, 0.0, 0.1];
    capped.ball.vel = [-5.0, 0.0, 0.0];
    let events = run(&mut capped, 60);
    assert_eq!(capped.score, [21, 19]);
    assert!(
        events.contains(&PointEvent::Over),
        "{} points end the match",
        p.max_points
    );
}

#[test]
fn a_disguised_shot_is_slower_than_an_honest_one() {
    let p = Params::default();
    let flight_of = |facing: f64| {
        let mut t = serve_from(0, [1.0, 0.4], 0.0, facing);
        let mut events = Vec::new();
        t.step(&mut events);
        (1.0 - t.ball.pos[0]) / t.ball.vel[0]
    };
    let honest = flight_of(0.8);
    let disguised = flight_of(-0.8);
    assert!((honest - p.min_flight).abs() < 1e-3, "{honest}");
    assert!(
        (disguised - (p.min_flight + p.disguise_flight)).abs() < 1e-3,
        "{disguised}"
    );
}

#[test]
fn a_paddle_stays_on_its_own_side() {
    let p = Params::default();
    let mut t = table(0);
    t.steer(0, [5.0, 9.0], None, 0.0);
    assert_eq!(
        t.paddles[0].target,
        [-(p.half_length - p.over_table), p.half_width + p.side_room]
    );
    t.steer(1, [-5.0, -9.0], None, 3.0);
    assert_eq!(
        t.paddles[1].target,
        [p.half_length - p.over_table, -(p.half_width + p.side_room)]
    );
    assert_eq!(t.paddles[1].facing, 1.0);
}

#[test]
fn a_paddle_never_moves_faster_than_its_body() {
    let mut t = table(0);
    let slow = Body {
        speed: 1.5,
        accel: 6.0,
    };
    t.bodies = [slow, slow];
    t.steer(0, [-2.5, 1.5], None, 0.0);
    let mut top: f64 = 0.0;
    for _ in 0..480 {
        let mut e = Vec::new();
        t.step(&mut e);
        let v = t.paddles[0].vel;
        top = top.max((v[0] * v[0] + v[1] * v[1]).sqrt());
    }
    assert!(top <= 1.5 + 1e-9, "{top}");
    assert!(top > 1.4, "{top}");
}

#[test]
fn a_seed_repeats_the_digest() {
    let mut a = serve_from(0, [1.0, 0.3], 0.6, 0.8);
    let mut b = serve_from(0, [1.0, 0.3], 0.6, 0.8);
    run(&mut a, 500);
    run(&mut b, 500);
    assert_eq!(a.digest(), b.digest());
    let mut c = serve_from(0, [1.0, -0.3], 0.6, 0.8);
    run(&mut c, 500);
    assert_ne!(a.digest(), c.digest());
}
