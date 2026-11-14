use feint_protocol::{ActionT, GameAdapter, MessageT, encode};

use crate::{
    FEINT_ERR_INPUT, FEINT_ERR_NULL, FEINT_ERR_SMALL, FEINT_LAYOUT, FEINT_POSITIONS,
    FEINT_VALUES_MAX, FeintGame, live, output, read_only,
};

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
