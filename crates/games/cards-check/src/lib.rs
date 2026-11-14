use believe::{Believe, Params, Scripted, Style};
use feint_ai::{Opponent, RunHeader, run_match};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, ObservationT};
use kuhn::Kuhn;

pub const KUHN_DEALS: u64 = 2_000;
pub const BELIEVE_MATCHES: u64 = 64;

fn fold(h: u64, word: u64) -> u64 {
    word.to_le_bytes()
        .iter()
        .fold(h, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

fn bodies() -> [BodyParamsT; 2] {
    [BodyParamsT::default(), BodyParamsT::default()]
}

#[must_use]
pub fn kuhn_digest() -> u64 {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    let mut obs = ObservationT::default();
    let mut events = Vec::new();
    for seed in 0..KUHN_DEALS {
        let mut game = Kuhn::new();
        if game.reset(seed, &bodies()).is_err() {
            return 0;
        }
        while game.result().is_none() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            game.observe(game.live(), 0, &mut obs);
            let mut action = ActionT::default();
            action.tick = game.live().tick;
            action.player = obs.to_act as u8;
            action.discrete = (state % 2) as i32;
            if state.is_multiple_of(3) {
                game.advance(&mut events);
                continue;
            }
            if game.apply(&action).is_err() {
                return 0;
            }
            game.advance(&mut events);
        }
        h = fold(h, game.digest(game.live()));
    }
    h
}

#[must_use]
pub fn believe_digest() -> u64 {
    let styles = [
        Style::Honest,
        Style::Liar(300),
        Style::Caller(300),
        Style::Counter,
    ];
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for seed in 0..BELIEVE_MATCHES {
        let Ok(mut game) = Believe::new(Params::default()) else {
            return 0;
        };
        let mut first = Scripted::new(styles[(seed % 4) as usize]);
        let mut second = Scripted::new(styles[(seed / 4 % 4) as usize]);
        let header = RunHeader {
            game_id: "believe".into(),
            game_version: believe::GAME_VERSION,
            seed,
            bodies: bodies().to_vec(),
            levels: [0, 0],
            opponent: Opponent::HandMade,
        };
        let Ok(report) = run_match(&mut game, [&mut first, &mut second], header, 10_000) else {
            return 0;
        };
        h = fold(fold(h, report.digest), report.actions.len() as u64);
    }
    h
}

pub const SEARCH_LEVELS: [u8; 2] = [6, 8];

fn searched(level: u8, seed: u64) -> Option<(u64, u64)> {
    let checkpoint = believe::trained::CHECKPOINTS.first()?.id;
    let mut game = Believe::new(Params::default()).ok()?;
    let mut bot = believe::bot::for_level(level, checkpoint).ok()?;
    let mut rival = Scripted::new(Style::Counter);
    let header = RunHeader {
        game_id: "believe".into(),
        game_version: believe::GAME_VERSION,
        seed,
        bodies: bodies().to_vec(),
        levels: [level, 0],
        opponent: Opponent::HandMade,
    };
    let report = run_match(&mut game, [bot.as_mut(), &mut rival], header, 10_000).ok()?;
    let mine = report.actions.iter().filter(|a| a.player == 0).count() as u64;
    Some((report.digest, mine))
}

#[must_use]
pub fn search_digest() -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for level in SEARCH_LEVELS {
        match searched(level, u64::from(level)) {
            Some((digest, mine)) => h = fold(fold(h, digest), mine),
            None => return 0,
        }
    }
    h
}

#[must_use]
pub fn search_decisions(level: u8) -> u64 {
    searched(level, 7).map_or(0, |(_, mine)| mine)
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn run_search_digest() -> u64 {
    search_digest()
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn run_search_decisions(level: u32) -> u64 {
    u8::try_from(level).map_or(0, search_decisions)
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn run_kuhn_digest() -> u64 {
    kuhn_digest()
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn run_believe_digest() -> u64 {
    believe_digest()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_digests_are_set_and_stable() {
        assert_ne!(kuhn_digest(), 0);
        assert_ne!(believe_digest(), 0);
        assert_eq!(kuhn_digest(), kuhn_digest());
        assert_eq!(believe_digest(), believe_digest());
    }

    #[test]
    fn a_searched_level_plays_and_an_unknown_level_does_not() {
        assert!(search_decisions(6) > 0);
        assert_eq!(search_decisions(4), 0);
    }
}
