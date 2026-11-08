//! One scripted match, the same on every target: native and wasm32 must agree on its
//! digest, or physics is not deterministic across targets (FNT-D4).

use airhockey::{Body, Input, Match, Params};

/// Ticks in the scripted match: long enough for many contacts and goals.
pub const TICKS: u64 = 50_000;

/// The digest after the scripted match.
#[must_use]
pub fn scripted_digest() -> u64 {
    scripted_match().digest()
}

/// The scripted match, played to its end or to [`TICKS`].
#[must_use]
pub fn scripted_match() -> Match {
    let mut game = Match::new(Params::default(), [Body::default(); 2], 0xFE1);
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut events = Vec::with_capacity(4);
    for tick in 0..TICKS {
        for player in 0..2u8 {
            let roll = next();
            if roll % 5 != 0 {
                continue;
            }
            let puck = game.world.puck.pos;
            let target = if roll % 3 == 0 {
                [puck.x as f32, puck.y as f32]
            } else {
                let x = (next() % 2000) as f32 / 1000.0 - 1.0;
                let y = (next() % 1000) as f32 / 1000.0 - 0.5;
                [x, y]
            };
            let _ = game.apply(Input {
                tick,
                player,
                target,
            });
        }
        events.clear();
        game.step(&mut events);
    }
    game
}

/// The entry point the wasm check calls.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn run_digest() -> u64 {
    scripted_digest()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_match_exercises_goals_and_contacts() {
        let game = scripted_match();
        let goals: u32 = game.score().iter().sum();
        assert!(
            goals >= 5,
            "only {goals} goals: the check would compare an idle table"
        );
    }
}
