#![allow(unsafe_code)]

use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Difficulty, RunHeader, run_match};
use feint_ffi::*;
use feint_protocol::{ActionT, GameAdapter, MessageT, decode};

const SEED: u64 = 77;
const CAP: u64 = 240 * 60 * 10;

type Outcome = (u64, [i64; 2], u64, Vec<ActionT>);

fn native() -> Outcome {
    let mut game = AirHockey::new(Params::default(), 5);
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed: SEED,
        bodies: vec![body(1, &game).unwrap(), body(5, &game).unwrap()],
        levels: [1, 5],
        opponent: feint_ai::Opponent::HandMade,
    };
    let spec = game.spec();
    let bot = |level| {
        Bot::new(
            Difficulty::preset(level).unwrap(),
            HockeyPlaybook::new(Params::default()),
            &spec,
        )
        .unwrap()
    };
    let (mut a, mut b) = (bot(1), bot(5));
    let report = run_match(&mut game, [&mut a, &mut b], header, CAP).unwrap();
    (
        report.digest,
        [report.score[0], report.score[1]],
        report.ticks,
        report.actions,
    )
}

#[derive(Clone, Copy)]
enum Mode {
    Plain,
    RetryAfterSmall,
    SharedBuffer,
}

unsafe fn tick_of(game: *const FeintGame) -> u64 {
    let mut t = 0u64;
    assert_eq!(unsafe { feint_game_tick(game, &mut t) }, 0);
    t
}

fn through_ffi(mode: Mode) -> Outcome {
    unsafe {
        let game = feint_game_new(SEED, 1, 5);
        assert!(!game.is_null());
        let bots = [
            feint_bot_new(1, SEED, 0, game),
            feint_bot_new(5, SEED, 1, game),
        ];
        assert!(bots.iter().all(|b| !b.is_null()));
        let mut obs = vec![0u8; FEINT_OBSERVATION_MAX];
        let mut out = vec![0u8; FEINT_ACTION_MAX];
        let mut applied = Vec::new();
        while feint_game_finished(game) == 0 && tick_of(game) < CAP {
            for (seat, &bot) in bots.iter().enumerate() {
                let n = feint_game_observe(game, seat as u8, obs.as_mut_ptr(), obs.len());
                assert!(n > 0, "observe returned {n}");
                let n = n as usize;
                let written = match mode {
                    Mode::Plain => {
                        feint_bot_tick(bot, game, obs.as_ptr(), n, out.as_mut_ptr(), out.len())
                    }
                    Mode::RetryAfterSmall => {
                        let first = feint_bot_tick(bot, game, obs.as_ptr(), n, out.as_mut_ptr(), 1);
                        assert!(first == 0 || first == FEINT_ERR_SMALL, "{first}");
                        feint_bot_tick(bot, game, obs.as_ptr(), n, out.as_mut_ptr(), out.len())
                    }
                    Mode::SharedBuffer => {
                        feint_bot_tick(bot, game, obs.as_ptr(), n, obs.as_mut_ptr(), obs.len())
                    }
                };
                assert!(written >= 0, "bot tick returned {written}");
                if written > 0 {
                    let src = if matches!(mode, Mode::SharedBuffer) {
                        &obs
                    } else {
                        &out
                    };
                    let bytes = &src[..written as usize];
                    if feint_game_apply(game, bytes.as_ptr(), bytes.len()) == 0 {
                        match decode(bytes).unwrap() {
                            MessageT::Action(a) => applied.push(*a),
                            _ => panic!("the bot answered with something other than an action"),
                        }
                    }
                }
            }
            assert!(feint_game_advance(game) >= 0);
        }
        let (mut a, mut b, mut digest) = (0i64, 0i64, 0u64);
        assert_eq!(feint_game_score(game, &mut a, &mut b), 0);
        assert_eq!(feint_game_digest(game, &mut digest), 0);
        let result = (digest, [a, b], tick_of(game), applied);
        for bot in bots {
            feint_bot_free(bot);
        }
        feint_game_free(game);
        result
    }
}

#[test]
fn a_match_through_the_c_interface_is_the_native_match() {
    let want = native();
    assert!(
        want.1.contains(&7),
        "the native match should finish: {:?}",
        want.1
    );
    assert_eq!(through_ffi(Mode::Plain), want);
    assert_eq!(
        through_ffi(Mode::RetryAfterSmall),
        want,
        "a retry after a small buffer must not re-decide"
    );
    assert_eq!(
        through_ffi(Mode::SharedBuffer),
        want,
        "one buffer for input and output"
    );
}

#[test]
fn bad_input_is_refused_without_crashing() {
    unsafe {
        assert!(feint_game_new(1, 6, 1).is_null());
        assert!(feint_game_new(1, 1, 9).is_null());
        let game = feint_game_new(1, 0, 3);
        assert!(!game.is_null(), "level 0 is a human at the maxima");
        assert!(feint_bot_new(3, 1, 2, game).is_null());
        assert!(feint_bot_new(0, 1, 1, game).is_null());
        assert!(feint_bot_new(6, 1, 1, game).is_null());
        assert!(feint_bot_new(3, 1, 1, std::ptr::null()).is_null());
        let bot = feint_bot_new(3, 1, 1, game);
        let mut buf = vec![0u8; FEINT_OBSERVATION_MAX];
        assert_eq!(
            feint_game_observe(std::ptr::null_mut(), 0, buf.as_mut_ptr(), buf.len()),
            FEINT_ERR_NULL
        );
        assert_eq!(
            feint_game_observe(game, 2, buf.as_mut_ptr(), buf.len()),
            FEINT_ERR_INPUT
        );
        assert_eq!(
            feint_game_observe(game, 0, buf.as_mut_ptr(), 4),
            FEINT_ERR_SMALL
        );
        assert_eq!(
            feint_game_observe(game, 0, std::ptr::null_mut(), buf.len()),
            FEINT_ERR_NULL
        );
        let n = feint_game_observe(game, 1, buf.as_mut_ptr(), buf.len()) as usize;
        assert_eq!(
            feint_game_apply(game, buf.as_ptr(), n),
            FEINT_ERR_INPUT,
            "an observation is not an action"
        );
        assert_eq!(
            feint_game_apply(game, [1u8, 2, 3].as_ptr(), 3),
            FEINT_ERR_INPUT
        );
        assert_eq!(
            feint_game_apply(game, std::ptr::null(), 0),
            FEINT_ERR_INPUT,
            "no bytes, no action"
        );
        assert_eq!(feint_game_apply(game, std::ptr::null(), 5), FEINT_ERR_NULL);
        assert_eq!(
            feint_game_apply(game, buf.as_ptr(), usize::MAX),
            FEINT_ERR_INPUT
        );

        let mut out = [0u8; FEINT_ACTION_MAX];
        assert_eq!(
            feint_bot_tick(
                bot,
                game,
                [9u8; 12].as_ptr(),
                12,
                out.as_mut_ptr(),
                out.len()
            ),
            FEINT_ERR_INPUT
        );
        assert_eq!(
            feint_bot_tick(
                std::ptr::null_mut(),
                game,
                buf.as_ptr(),
                n,
                out.as_mut_ptr(),
                out.len()
            ),
            FEINT_ERR_NULL
        );
        let first = feint_bot_tick(bot, game, buf.as_ptr(), n, out.as_mut_ptr(), out.len());
        let mut again = [0u8; FEINT_ACTION_MAX];
        let second = feint_bot_tick(bot, game, buf.as_ptr(), n, again.as_mut_ptr(), again.len());
        assert!(first >= 0 && first == second, "{first} vs {second}");
        assert_eq!(
            &again[..second as usize],
            &out[..first as usize],
            "a retry returns the same bytes"
        );
        let seat0 = feint_game_observe(game, 0, buf.as_mut_ptr(), buf.len()) as usize;
        assert_eq!(
            feint_bot_tick(bot, game, buf.as_ptr(), seat0, out.as_mut_ptr(), out.len()),
            FEINT_ERR_INPUT,
            "a bot of seat 1 refuses seat 0's observation"
        );

        let (mut one, mut digest, mut tick) = (-9i64, 0u64, 0u64);
        assert_eq!(
            feint_game_score(game, &mut one, std::ptr::null_mut()),
            FEINT_ERR_NULL
        );
        assert_eq!(one, -9, "no half-written score");
        assert_eq!(feint_game_finished(std::ptr::null()), FEINT_ERR_NULL);
        assert_eq!(
            feint_game_digest(std::ptr::null(), &mut digest),
            FEINT_ERR_NULL
        );
        assert_eq!(feint_game_tick(game, std::ptr::null_mut()), FEINT_ERR_NULL);
        assert_eq!(feint_game_tick(game, &mut tick), 0);
        feint_bot_free(std::ptr::null_mut());
        feint_game_free(std::ptr::null_mut());
        feint_bot_free(bot);
        feint_game_free(game);
        let version = std::ffi::CStr::from_ptr(feint_version());
        assert_eq!(version.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }
}

#[test]
fn a_panicked_game_refuses_every_later_call() {
    unsafe {
        let game = feint_game_new(3, 1, 2);
        let bot = feint_bot_new(2, 3, 1, game);
        let mut buf = vec![0u8; FEINT_OBSERVATION_MAX];
        let n = feint_game_observe(game, 1, buf.as_mut_ptr(), buf.len()) as usize;
        assert_eq!(feint_test_panic(game), FEINT_ERR_PANIC);
        let (mut a, mut b, mut t) = (0i64, 0i64, 0u64);
        assert_eq!(
            feint_game_observe(game, 0, buf.as_mut_ptr(), buf.len()),
            FEINT_ERR_BROKEN
        );
        assert_eq!(feint_game_advance(game), FEINT_ERR_BROKEN);
        assert_eq!(feint_game_finished(game), FEINT_ERR_BROKEN);
        assert_eq!(feint_game_score(game, &mut a, &mut b), FEINT_ERR_BROKEN);
        assert_eq!(feint_game_tick(game, &mut t), FEINT_ERR_BROKEN);
        assert_eq!(feint_game_digest(game, &mut t), FEINT_ERR_BROKEN);
        assert!(feint_bot_new(2, 3, 0, game).is_null());
        let mut out = [0u8; FEINT_ACTION_MAX];
        assert_eq!(
            feint_bot_tick(bot, game, buf.as_ptr(), n, out.as_mut_ptr(), out.len()),
            FEINT_ERR_BROKEN
        );
        feint_bot_free(bot);
        feint_game_free(game);
    }
}

#[test]
fn positions_and_layout_are_the_live_tables() {
    unsafe {
        let game = feint_game_new(3, 2, 4);
        let mut out = [0f32; FEINT_POSITIONS];
        assert_eq!(
            feint_game_positions(game, out.as_mut_ptr(), out.len()),
            FEINT_POSITIONS as isize
        );
        let mut native = AirHockey::new(Params::default(), 4);
        let bodies = vec![body(2, &native).unwrap(), body(4, &native).unwrap()];
        native.reset(3, &bodies).unwrap();
        let w = &native.live().world;
        let want = [
            w.puck.pos.x,
            w.puck.pos.y,
            w.puck.vel.x,
            w.puck.vel.y,
            w.mallets[0].pos.x,
            w.mallets[0].pos.y,
            w.mallets[0].vel.x,
            w.mallets[0].vel.y,
            w.mallets[1].pos.x,
            w.mallets[1].pos.y,
            w.mallets[1].vel.x,
            w.mallets[1].vel.y,
        ]
        .map(|v| v as f32);
        assert_eq!(out, want);
        let mut layout = [0f32; FEINT_LAYOUT];
        assert_eq!(
            feint_game_layout(game, layout.as_mut_ptr(), layout.len()),
            FEINT_LAYOUT as isize
        );
        let p = Params::default();
        assert_eq!(
            layout,
            [
                p.half_length,
                p.half_width,
                p.mouth_half_width,
                p.puck_radius,
                p.mallet_radius
            ]
            .map(|v| v as f32)
        );
        assert_eq!(
            feint_game_positions(game, out.as_mut_ptr(), 11),
            FEINT_ERR_SMALL
        );
        assert_eq!(
            feint_game_positions(game, std::ptr::null_mut(), 12),
            FEINT_ERR_NULL
        );
        assert_eq!(
            feint_game_positions(std::ptr::null(), out.as_mut_ptr(), 12),
            FEINT_ERR_NULL
        );
        assert_eq!(
            feint_game_layout(game, layout.as_mut_ptr(), 4),
            FEINT_ERR_SMALL
        );
        feint_game_free(game);
    }
}

#[test]
fn an_encoded_action_is_the_message_the_game_takes() {
    unsafe {
        let values = [0.25f32, -0.1];
        let mut out = vec![0u8; FEINT_ACTION_MAX];
        let n = feint_action_encode(
            42,
            1,
            values.as_ptr(),
            values.len(),
            out.as_mut_ptr(),
            out.len(),
        );
        assert!(n > 0);
        let Ok(MessageT::Action(a)) = decode(&out[..n as usize]) else {
            panic!("an action message");
        };
        assert_eq!(
            (a.tick, a.player, a.values.as_deref()),
            (42, 1, Some(&values[..]))
        );
        let game = feint_game_new(1, 0, 3);
        let mut now = 0u64;
        assert_eq!(feint_game_tick(game, &mut now), 0);
        let k = feint_action_encode(now, 1, values.as_ptr(), 2, out.as_mut_ptr(), out.len());
        assert_eq!(feint_game_apply(game, out.as_ptr(), k as usize), 0);
        let far = [9.0f32, 0.0];
        let m = feint_action_encode(now, 0, far.as_ptr(), 2, out.as_mut_ptr(), out.len());
        assert_eq!(
            feint_game_apply(game, out.as_ptr(), m as usize),
            FEINT_ERR_INPUT
        );
        assert_eq!(
            feint_action_encode(1, 2, values.as_ptr(), 2, out.as_mut_ptr(), out.len()),
            FEINT_ERR_INPUT
        );
        assert_eq!(
            feint_action_encode(1, 0, std::ptr::null(), 2, out.as_mut_ptr(), out.len()),
            FEINT_ERR_NULL
        );
        assert_eq!(
            feint_action_encode(1, 0, values.as_ptr(), 2, out.as_mut_ptr(), 3),
            FEINT_ERR_SMALL
        );
        assert_eq!(
            feint_action_encode(
                1,
                0,
                values.as_ptr(),
                FEINT_VALUES_MAX + 1,
                out.as_mut_ptr(),
                out.len()
            ),
            FEINT_ERR_INPUT
        );
        feint_game_free(game);
    }
}
