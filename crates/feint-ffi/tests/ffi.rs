#![allow(unsafe_code)]

use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Difficulty, RunHeader, run_match};
use feint_ffi::*;
use feint_protocol::GameAdapter;

const SEED: u64 = 77;
const CAP: u64 = 240 * 60 * 10;

fn native() -> (u64, [i64; 2], u64) {
    let mut game = AirHockey::new(Params::default(), 5);
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed: SEED,
        bodies: vec![body(1, &game).unwrap(), body(5, &game).unwrap()],
        levels: [1, 5],
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
    )
}

#[derive(Clone, Copy)]
enum Mode {
    Plain,
    RetryAfterSmall,
    SharedBuffer,
}

fn through_ffi(mode: Mode) -> (u64, [i64; 2], u64) {
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
        while feint_game_finished(game) == 0 && feint_game_tick(game) < CAP {
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
                    let _ = feint_game_apply(game, src.as_ptr(), written as usize);
                }
            }
            assert!(feint_game_advance(game) >= 0);
        }
        let mut score = [0i64; 2];
        assert_eq!(feint_game_score(game, score.as_mut_ptr()), 0);
        let result = (feint_game_digest(game), score, feint_game_tick(game));
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
        "the native match should finish: {want:?}"
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
            FEINT_ERR_NULL
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
        assert_eq!(feint_game_score(game, std::ptr::null_mut()), FEINT_ERR_NULL);
        assert_eq!(feint_game_finished(std::ptr::null()), FEINT_ERR_NULL);
        assert_eq!(feint_game_digest(std::ptr::null()), 0);
        feint_bot_free(std::ptr::null_mut());
        feint_game_free(std::ptr::null_mut());
        feint_bot_free(bot);
        feint_game_free(game);
        let version = std::ffi::CStr::from_ptr(feint_version());
        assert_eq!(version.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }
}
