use feint_ai::{Bot, Difficulty, Opponent, RunHeader, run_match};
use feint_protocol::{ActionT, AdapterError, BodyParamsT, ChaCha8Rng, GameAdapter, ObservationT};
use pingpong::human::{Human, human_body};
use pingpong::{OBSERVATION_LEN, Params, Phase, PingPong, PingPongPlaybook};
use rand_chacha::rand_core::SeedableRng;

fn bodies(game: &PingPong) -> Vec<BodyParamsT> {
    vec![human_body(game), human_body(game)]
}

fn mid_rally() -> PingPong {
    let mut game = PingPong::new(Params::default(), 3);
    let b = bodies(&game);
    game.reset(4, &b).unwrap();
    let mut events = Vec::new();
    let server = game.live().server;
    let pos = game.live().paddles[usize::from(server)].pos;
    let m = pingpong::mirror(server);
    let mut a = ActionT::default();
    a.player = server;
    a.values = Some(vec![
        (m * pos[0]) as f32,
        (m * pos[1]) as f32,
        1.0,
        1.0,
        0.3,
        0.6,
        0.8,
    ]);
    game.apply(&a).unwrap();
    for _ in 0..40 {
        game.advance(&mut events);
    }
    assert_eq!(game.live().phase, Phase::Rally);
    game
}

#[test]
fn each_seat_sees_itself_on_the_negative_side() {
    let game = mid_rally();
    let s = game.live();
    let mut a = ObservationT::default();
    let mut b = ObservationT::default();
    game.observe(s, 0, &mut a);
    game.observe(s, 1, &mut b);
    let (a, b) = (a.state.unwrap(), b.state.unwrap());
    assert_eq!(a.len(), OBSERVATION_LEN);
    assert_eq!(b.len(), OBSERVATION_LEN);
    assert_eq!(a[0], s.paddles[0].pos[0] as f32);
    assert_eq!(b[0], -s.paddles[1].pos[0] as f32);
    assert_eq!(b[1], -s.paddles[1].pos[1] as f32);
    assert_eq!(b[8], -s.ball.pos[0] as f32);
    assert_eq!(b[10], s.ball.pos[2] as f32, "height is not mirrored");
    assert_eq!(
        a[4..8],
        [b[0], b[1], b[2], b[3]].map(|v| -v),
        "their paddle is my opponent's own, mirrored"
    );
    assert!(a[0] < 0.0 && b[0] < 0.0);
}

#[test]
fn a_state_read_back_from_an_observation_keeps_the_ball_paddles_and_score() {
    let game = mid_rally();
    let s = game.live();
    for seat in [0u8, 1] {
        let mut obs = ObservationT::default();
        game.observe(s, seat, &mut obs);
        let back = game.state_from_observation(&obs, &mut ChaCha8Rng::seed_from_u64(0));
        let close = |x: f64, y: f64| (x - y).abs() < 1e-5;
        for k in 0..3 {
            assert!(
                close(back.ball.pos[k], s.ball.pos[k]) && close(back.ball.vel[k], s.ball.vel[k]),
                "{seat}"
            );
        }
        for p in 0..2 {
            for k in 0..2 {
                assert!(
                    close(back.paddles[p].pos[k], s.paddles[p].pos[k]),
                    "{seat} {p}"
                );
            }
        }
        assert_eq!(
            (back.phase, back.server, back.hitter, back.score),
            (s.phase, s.server, s.hitter, s.score)
        );
    }
}

#[test]
fn an_action_is_checked_and_mirrored_for_seat_one() {
    let mut game = mid_rally();
    let tick = game.live().tick;
    let mut a = ActionT::default();
    a.tick = tick;
    a.player = 1;
    a.values = Some(vec![-1.9, 0.4, 0.0, 1.0, 0.0, 0.6, 0.5]);
    game.apply(&a).unwrap();
    let pad = game.live().paddles[1];
    assert_eq!(pad.target, [f64::from(1.9f32), f64::from(-0.4f32)]);
    assert_eq!(pad.facing, -0.5);
    for (values, why) in [
        (vec![0.0; 6], "seven values"),
        (vec![f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.6, 0.0], "not finite"),
        (
            vec![0.0, 0.0, 2.0, 1.0, 0.0, 0.6, 0.0],
            "outside the action space",
        ),
        (
            vec![0.0, 0.0, 1.0, 1.0, 0.0, 0.6, 1.5],
            "outside the action space",
        ),
        (
            vec![9.0, 0.0, 1.0, 1.0, 0.0, 0.6, 0.0],
            "outside the action space",
        ),
    ] {
        a.values = Some(values);
        assert_eq!(game.apply(&a), Err(AdapterError::BadAction(why)));
    }
    a.values = Some(vec![-1.9, 0.0, 0.0, 1.0, 0.0, 0.6, 0.0]);
    a.tick = tick + 5;
    assert!(matches!(
        game.apply(&a),
        Err(AdapterError::WrongTick { .. })
    ));
    a.tick = tick;
    a.player = 2;
    assert_eq!(game.apply(&a), Err(AdapterError::BadPlayer(2)));
}

#[test]
fn a_body_outside_the_declared_maxima_is_refused() {
    let mut game = PingPong::new(Params::default(), 3);
    let p = Params::default();
    let mut fast = BodyParamsT::default();
    fast.values = Some(vec![(p.max_speed * 2.0) as f32, 1.0]);
    let fine = human_body(&game);
    assert!(game.reset(1, &[fast, fine.clone()]).is_err());
    assert!(game.reset(1, std::slice::from_ref(&fine)).is_err());
    assert!(game.reset(1, &[fine.clone(), fine]).is_ok());
}

#[test]
fn a_match_against_the_human_model_ends_and_repeats_for_a_seed() {
    let digest = |seed: u64| {
        let mut game = PingPong::new(Params::default(), 3);
        let spec = game.spec();
        let d = Difficulty::preset(3).unwrap();
        let mut bot = Bot::new(d, PingPongPlaybook::new(Params::default()), &spec).unwrap();
        let mut human = Human::new();
        let header = RunHeader {
            game_id: "pingpong".into(),
            game_version: pingpong::GAME_VERSION,
            seed,
            bodies: vec![d.body(&spec), human_body(&game)],
            levels: [3, 0],
            opponent: Opponent::HandMade,
        };
        let r = run_match(&mut game, [&mut bot, &mut human], header, 240 * 1800).unwrap();
        assert!(r.finished, "{seed}");
        (r.digest, r.score)
    };
    let a = digest(5);
    assert_eq!(a, digest(5));
    assert!(a.1[0] + a.1[1] >= 11);
}

#[test]
fn the_hit_counter_and_rally_survive_an_observation() {
    let game = mid_rally();
    let s = game.live();
    assert_eq!(s.hits, 1);
    for seat in [0u8, 1] {
        let mut obs = ObservationT::default();
        game.observe(s, seat, &mut obs);
        let back = game.state_from_observation(&obs, &mut ChaCha8Rng::seed_from_u64(0));
        assert_eq!((back.hits, back.rally), (s.hits, s.rally), "{seat}");
    }
}

#[test]
fn the_bot_reads_where_the_opponent_played_and_where_it_covered() {
    use feint_ai::Playbook;
    let book = PingPongPlaybook::new(Params::default());
    let read_back = |g: &PingPong, me: u8| {
        let mut obs = ObservationT::default();
        g.observe(g.live(), me, &mut obs);
        g.state_from_observation(&obs, &mut ChaCha8Rng::seed_from_u64(0))
    };
    let serve = |g: &mut PingPong| {
        let server = g.live().server;
        let pos = g.live().paddles[usize::from(server)].pos;
        let m = pingpong::mirror(server);
        let mut a = ActionT::default();
        a.player = server;
        a.values = Some(vec![
            (m * pos[0]) as f32,
            (m * pos[1]) as f32,
            1.0,
            1.0,
            0.5,
            0.6,
            0.8,
        ]);
        g.apply(&a).unwrap();
        let mut events = Vec::new();
        g.advance(&mut events);
    };
    let mut game = PingPong::new(Params::default(), 3);
    let b = bodies(&game);
    game.reset(4, &b).unwrap();
    let server = game.live().server;
    let receiver = 1 - server;
    let before = read_back(&game, receiver);
    serve(&mut game);
    let after = read_back(&game, receiver);
    assert_eq!(
        book.read(&before, &after, server).shot,
        Some(0),
        "their serve to their right lands on my left"
    );
    let mut game = PingPong::new(Params::default(), 3);
    game.reset(4, &b).unwrap();
    let before = read_back(&game, server);
    serve(&mut game);
    let after = read_back(&game, server);
    let theirs = pingpong::mirror(server) * game.live().paddles[usize::from(receiver)].pos[1];
    let want = if theirs < -0.2 {
        0
    } else if theirs > 0.2 {
        2
    } else {
        1
    };
    assert_eq!(
        book.read(&before, &after, receiver).covered,
        Some(want),
        "where the receiver stands when the bot hits"
    );
    assert_eq!(
        book.read(&after, &after, receiver).covered,
        None,
        "only at the hit"
    );
}
