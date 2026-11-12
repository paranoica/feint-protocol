use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Difficulty, RunHeader, run_match};
use feint_ml::Policy;
use feint_ml::mlp::Mlp;
use feint_ml::registry::{fnv1a64, for_level};
use feint_ml::trained::trained_pilot;
use feint_protocol::GameAdapter;

const FEATURES: &[u8] = include_bytes!("../../feint-ml/tests/fixtures/features.bin");
pub const MATCH_TICKS: u64 = 240 * 120;
pub const MATCH_SEED: u64 = 0x05EE_D0B3;

#[must_use]
pub fn outputs_digest() -> u64 {
    let mut mlp = Mlp::from_safetensors(level_five().bytes).expect("the embedded checkpoint");
    let width = u32::from_le_bytes([FEATURES[4], FEATURES[5], FEATURES[6], FEATURES[7]]) as usize;
    let values: Vec<f32> = FEATURES[8..]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let mut bytes = Vec::with_capacity(values.len() / width * 8);
    let mut out = [0.0f32; 2];
    for row in values.chunks_exact(width) {
        mlp.act(row, &mut out);
        for v in out {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    fnv1a64(&bytes)
}

#[must_use]
pub fn match_digest() -> u64 {
    let mut game = AirHockey::new(Params::default(), 5);
    let spec = game.spec();
    let difficulty = Difficulty::preset(5).expect("level 5");
    let body = difficulty.body(&spec);
    let mut pilot = trained_pilot(5, level_five().id, &spec).expect("the trained pilot");
    let mut bot = Bot::new(difficulty, HockeyPlaybook::new(Params::default()), &spec)
        .expect("the hand-made bot");
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed: MATCH_SEED,
        bodies: vec![body.clone(), body],
        levels: [5, 5],
        opponent: feint_ai::Opponent::HandMade,
    };
    let report =
        run_match(&mut game, [&mut bot, &mut pilot], header, MATCH_TICKS).expect("the match runs");
    let mut words = Vec::with_capacity(report.actions.len() * 16 + 8);
    words.extend_from_slice(&report.digest.to_le_bytes());
    for action in &report.actions {
        words.extend_from_slice(&action.tick.to_le_bytes());
        for v in action.values.as_deref().unwrap_or_default() {
            words.extend_from_slice(&v.to_le_bytes());
        }
    }
    fnv1a64(&words)
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn run_outputs_digest() -> u64 {
    outputs_digest()
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn run_match_digest() -> u64 {
    match_digest()
}

fn level_five() -> &'static feint_ml::registry::Checkpoint {
    for_level(5).expect("level 5 has a checkpoint")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_digests_repeat() {
        assert_eq!(outputs_digest(), outputs_digest());
        assert_eq!(match_digest(), match_digest());
    }
}
