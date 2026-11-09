#![allow(unsafe_code)]
#![allow(clippy::missing_safety_doc)]

use std::ffi::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Controller, Difficulty};
use feint_protocol::{BodyParamsT, GameAdapter, MessageT, ObservationT, decode, encode};

pub const FEINT_ERR_NULL: isize = -1;
pub const FEINT_ERR_INPUT: isize = -2;
pub const FEINT_ERR_SMALL: isize = -3;
pub const FEINT_ERR_BROKEN: isize = -4;
pub const FEINT_ERR_PANIC: isize = -5;
pub const FEINT_ACTION_MAX: usize = 256;
pub const FEINT_OBSERVATION_MAX: usize = 1024;

pub struct FeintGame {
    game: AirHockey,
    broken: bool,
}

pub struct FeintBot {
    bot: Bot<AirHockey, HockeyPlaybook>,
    last: Option<(u64, Vec<u8>)>,
    broken: bool,
}

static VERSION: &[u8] = b"0.1.0\0";

#[must_use]
pub fn body(level: u8, game: &AirHockey) -> Option<BodyParamsT> {
    if level == 0 {
        let p = Params::default();
        let mut b = BodyParamsT::default();
        b.values = Some(vec![p.max_body_speed as f32, p.max_body_accel as f32]);
        return Some(b);
    }
    Difficulty::preset(level).map(|d| d.body(&game.spec()))
}

fn input<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() || isize::try_from(len).is_err() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

fn output(bytes: &[u8], out: *mut u8, cap: usize) -> isize {
    if bytes.len() > cap {
        return FEINT_ERR_SMALL;
    }
    if bytes.is_empty() {
        return 0;
    }
    if out.is_null() {
        return FEINT_ERR_NULL;
    }
    unsafe { std::ptr::copy(bytes.as_ptr(), out, bytes.len()) };
    isize::try_from(bytes.len()).unwrap_or(FEINT_ERR_INPUT)
}

fn guarded(broken: &mut bool, f: impl FnOnce() -> isize) -> isize {
    if *broken {
        return FEINT_ERR_BROKEN;
    }
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(code) => code,
        Err(_) => {
            *broken = true;
            FEINT_ERR_PANIC
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn feint_version() -> *const c_char {
    VERSION.as_ptr().cast()
}

#[unsafe(no_mangle)]
pub extern "C" fn feint_game_new(seed: u64, level_a: u8, level_b: u8) -> *mut FeintGame {
    let made = catch_unwind(|| {
        let mut game = AirHockey::new(Params::default(), level_b);
        let bodies = vec![body(level_a, &game)?, body(level_b, &game)?];
        game.reset(seed, &bodies).ok()?;
        Some(Box::new(FeintGame {
            game,
            broken: false,
        }))
    });
    match made {
        Ok(Some(game)) => Box::into_raw(game),
        _ => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_free(game: *mut FeintGame) {
    if !game.is_null() {
        drop(unsafe { Box::from_raw(game) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_observe(
    game: *mut FeintGame,
    player: u8,
    out: *mut u8,
    cap: usize,
) -> isize {
    let Some(g) = (unsafe { game.as_mut() }) else {
        return FEINT_ERR_NULL;
    };
    let FeintGame { game, broken } = g;
    guarded(broken, || {
        if player > 1 {
            return FEINT_ERR_INPUT;
        }
        let mut obs = ObservationT::default();
        game.observe(game.live(), player, &mut obs);
        output(&encode(&MessageT::Observation(Box::new(obs))), out, cap)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_apply(
    game: *mut FeintGame,
    bytes: *const u8,
    len: usize,
) -> isize {
    let Some(g) = (unsafe { game.as_mut() }) else {
        return FEINT_ERR_NULL;
    };
    let Some(bytes) = input(bytes, len) else {
        return FEINT_ERR_NULL;
    };
    let FeintGame { game, broken } = g;
    guarded(broken, || match decode(bytes) {
        Ok(MessageT::Action(action)) => match game.apply(&action) {
            Ok(()) => 0,
            Err(_) => FEINT_ERR_INPUT,
        },
        _ => FEINT_ERR_INPUT,
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_advance(game: *mut FeintGame) -> isize {
    let Some(g) = (unsafe { game.as_mut() }) else {
        return FEINT_ERR_NULL;
    };
    let FeintGame { game, broken } = g;
    guarded(broken, || {
        if game.result().is_some() {
            return 0;
        }
        let mut events = Vec::new();
        game.advance(&mut events);
        isize::try_from(events.len()).unwrap_or(0)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_finished(game: *const FeintGame) -> isize {
    match unsafe { game.as_ref() } {
        Some(g) if g.broken => FEINT_ERR_BROKEN,
        Some(g) => isize::from(g.game.result().is_some()),
        None => FEINT_ERR_NULL,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_tick(game: *const FeintGame) -> u64 {
    unsafe { game.as_ref() }.map_or(0, |g| g.game.live().tick())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_score(game: *const FeintGame, out: *mut i64) -> isize {
    let Some(g) = (unsafe { game.as_ref() }) else {
        return FEINT_ERR_NULL;
    };
    if out.is_null() {
        return FEINT_ERR_NULL;
    }
    let score = g.game.live().score();
    unsafe {
        *out = i64::from(score[0]);
        *out.add(1) = i64::from(score[1]);
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_digest(game: *const FeintGame) -> u64 {
    unsafe { game.as_ref() }.map_or(0, |g| g.game.digest(g.game.live()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_bot_new(
    level: u8,
    seed: u64,
    player: u8,
    game: *const FeintGame,
) -> *mut FeintBot {
    let Some(g) = (unsafe { game.as_ref() }) else {
        return std::ptr::null_mut();
    };
    if player > 1 {
        return std::ptr::null_mut();
    }
    let made = catch_unwind(AssertUnwindSafe(|| {
        let spec = g.game.spec();
        let mut bot = Bot::new(
            Difficulty::preset(level)?,
            HockeyPlaybook::new(Params::default()),
            &spec,
        )
        .ok()?;
        bot.new_match(&spec, player, seed);
        Some(Box::new(FeintBot {
            bot,
            last: None,
            broken: false,
        }))
    }));
    match made {
        Ok(Some(bot)) => Box::into_raw(bot),
        _ => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_bot_free(bot: *mut FeintBot) {
    if !bot.is_null() {
        drop(unsafe { Box::from_raw(bot) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_bot_tick(
    bot: *mut FeintBot,
    game: *const FeintGame,
    obs: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> isize {
    let (Some(b), Some(g)) = (unsafe { bot.as_mut() }, unsafe { game.as_ref() }) else {
        return FEINT_ERR_NULL;
    };
    let Some(obs) = input(obs, len) else {
        return FEINT_ERR_NULL;
    };
    let obs = obs.to_vec();
    let FeintBot { bot, last, broken } = b;
    guarded(broken, || {
        let Ok(MessageT::Observation(obs)) = decode(&obs) else {
            return FEINT_ERR_INPUT;
        };
        if let Some((tick, bytes)) = last.as_ref().filter(|(tick, _)| *tick == obs.tick) {
            let _ = tick;
            return output(bytes, out, cap);
        }
        let bytes = bot
            .on_tick(&g.game, &obs)
            .map(|action| encode(&MessageT::Action(Box::new(action))))
            .unwrap_or_default();
        let written = output(&bytes, out, cap);
        *last = Some((obs.tick, bytes));
        written
    })
}
