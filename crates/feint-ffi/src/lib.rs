#![allow(unsafe_code)]
#![allow(clippy::missing_safety_doc)]

use std::ffi::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Controller, Difficulty};
use feint_protocol::{
    ActionT, BodyParamsT, GameAdapter, MessageT, ObservationT, PlayerId, decode, encode,
};

pub const FEINT_ERR_NULL: isize = -1;
pub const FEINT_ERR_INPUT: isize = -2;
pub const FEINT_ERR_SMALL: isize = -3;
pub const FEINT_ERR_BROKEN: isize = -4;
pub const FEINT_ERR_PANIC: isize = -5;
pub const FEINT_ACTION_MAX: usize = 256;
pub const FEINT_OBSERVATION_MAX: usize = 1024;
pub const FEINT_POSITIONS: usize = 12;
pub const FEINT_LAYOUT: usize = 5;
pub const FEINT_VALUES_MAX: usize = 16;

pub struct FeintGame {
    game: AirHockey,
    broken: bool,
}

pub struct FeintBot {
    bot: Bot<AirHockey, HockeyPlaybook>,
    player: PlayerId,
    last: Option<(u64, Vec<u8>)>,
    broken: bool,
}

static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

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

unsafe fn input<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], isize> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return Err(FEINT_ERR_NULL);
    }
    if isize::try_from(len).is_err() {
        return Err(FEINT_ERR_INPUT);
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
}

unsafe fn output(bytes: &[u8], out: *mut u8, cap: usize) -> isize {
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

unsafe fn write<T>(out: *mut T, value: T) -> isize {
    if out.is_null() {
        return FEINT_ERR_NULL;
    }
    unsafe { out.write(value) };
    0
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

fn read_only(f: impl FnOnce() -> isize) -> isize {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(FEINT_ERR_PANIC)
}

unsafe fn live<'a>(game: *const FeintGame) -> Result<&'a FeintGame, isize> {
    match unsafe { game.as_ref() } {
        None => Err(FEINT_ERR_NULL),
        Some(g) if g.broken => Err(FEINT_ERR_BROKEN),
        Some(g) => Ok(g),
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
        let bytes = encode(&MessageT::Observation(Box::new(obs)));
        unsafe { output(&bytes, out, cap) }
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
    let bytes = match unsafe { input(bytes, len) } {
        Ok(bytes) => bytes,
        Err(code) => return code,
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
    match unsafe { live(game) } {
        Ok(g) => read_only(|| isize::from(g.game.result().is_some())),
        Err(code) => code,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_tick(game: *const FeintGame, out: *mut u64) -> isize {
    match unsafe { live(game) } {
        Ok(g) => read_only(|| unsafe { write(out, g.game.live().tick()) }),
        Err(code) => code,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_score(
    game: *const FeintGame,
    first: *mut i64,
    second: *mut i64,
) -> isize {
    let g = match unsafe { live(game) } {
        Ok(g) => g,
        Err(code) => return code,
    };
    if first.is_null() || second.is_null() {
        return FEINT_ERR_NULL;
    }
    read_only(|| {
        let score = g.game.live().score();
        unsafe {
            write(first, i64::from(score[0]));
            write(second, i64::from(score[1]))
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_digest(game: *const FeintGame, out: *mut u64) -> isize {
    match unsafe { live(game) } {
        Ok(g) => read_only(|| unsafe { write(out, g.game.digest(g.game.live())) }),
        Err(code) => code,
    }
}

unsafe fn floats(values: &[f32], out: *mut f32, cap: usize) -> isize {
    if out.is_null() {
        return FEINT_ERR_NULL;
    }
    if cap < values.len() {
        return FEINT_ERR_SMALL;
    }
    unsafe { std::ptr::copy(values.as_ptr(), out, values.len()) };
    isize::try_from(values.len()).unwrap_or(FEINT_ERR_INPUT)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_positions(
    game: *const FeintGame,
    out: *mut f32,
    cap: usize,
) -> isize {
    match unsafe { live(game) } {
        Ok(g) => read_only(|| {
            let w = &g.game.live().world;
            let [a, b] = w.mallets;
            let values: [f32; FEINT_POSITIONS] = [
                w.puck.pos.x,
                w.puck.pos.y,
                w.puck.vel.x,
                w.puck.vel.y,
                a.pos.x,
                a.pos.y,
                a.vel.x,
                a.vel.y,
                b.pos.x,
                b.pos.y,
                b.vel.x,
                b.vel.y,
            ]
            .map(|v| v as f32);
            unsafe { floats(&values, out, cap) }
        }),
        Err(code) => code,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_game_layout(
    game: *const FeintGame,
    out: *mut f32,
    cap: usize,
) -> isize {
    match unsafe { live(game) } {
        Ok(g) => read_only(|| {
            let p = &g.game.live().params;
            let values: [f32; FEINT_LAYOUT] = [
                p.half_length,
                p.half_width,
                p.mouth_half_width,
                p.puck_radius,
                p.mallet_radius,
            ]
            .map(|v| v as f32);
            unsafe { floats(&values, out, cap) }
        }),
        Err(code) => code,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_action_encode(
    tick: u64,
    player: u8,
    values: *const f32,
    n: usize,
    out: *mut u8,
    cap: usize,
) -> isize {
    if player > 1 || n > FEINT_VALUES_MAX {
        return FEINT_ERR_INPUT;
    }
    if values.is_null() && n > 0 {
        return FEINT_ERR_NULL;
    }
    read_only(|| {
        let values = if n == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(values, n) }.to_vec()
        };
        let mut action = ActionT::default();
        action.tick = tick;
        action.player = player;
        action.values = Some(values);
        let bytes = encode(&MessageT::Action(Box::new(action)));
        unsafe { output(&bytes, out, cap) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_bot_new(
    level: u8,
    seed: u64,
    player: u8,
    game: *const FeintGame,
) -> *mut FeintBot {
    let Ok(g) = (unsafe { live(game) }) else {
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
            player,
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
    let Some(b) = (unsafe { bot.as_mut() }) else {
        return FEINT_ERR_NULL;
    };
    let g = match unsafe { live(game) } {
        Ok(g) => g,
        Err(code) => return code,
    };
    let obs = match unsafe { input(obs, len) } {
        Ok(obs) => obs.to_vec(),
        Err(code) => return code,
    };
    let FeintBot {
        bot,
        player,
        last,
        broken,
    } = b;
    guarded(broken, || {
        let Ok(MessageT::Observation(obs)) = decode(&obs) else {
            return FEINT_ERR_INPUT;
        };
        if obs.player != *player {
            return FEINT_ERR_INPUT;
        }
        if let Some((_, bytes)) = last.as_ref().filter(|(tick, _)| *tick == obs.tick) {
            return unsafe { output(bytes, out, cap) };
        }
        let bytes = bot
            .on_tick(&g.game, &obs)
            .map(|action| encode(&MessageT::Action(Box::new(action))))
            .unwrap_or_default();
        let written = unsafe { output(&bytes, out, cap) };
        *last = Some((obs.tick, bytes));
        written
    })
}

#[cfg(feature = "test-panic")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn feint_test_panic(game: *mut FeintGame) -> isize {
    let Some(g) = (unsafe { game.as_mut() }) else {
        return FEINT_ERR_NULL;
    };
    guarded(&mut g.broken, || panic!("a panic on purpose"))
}
