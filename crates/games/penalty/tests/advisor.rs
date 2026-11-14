use std::sync::Arc;

use feint_ai::{Advisor, Predictor};
use feint_context::{ContextModel, sigmoid, softmax};
use penalty::context::{ContextAdvisor, LAYOUT};
use penalty::persons::{Advice, Habits, match_tokens};

fn tiny() -> Arc<ContextModel> {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../feint-context/tests/fixtures/tiny.safetensors"
    ))
    .unwrap();
    Arc::new(ContextModel::from_safetensors(&bytes, LAYOUT).unwrap())
}

fn expected(model: &ContextModel, tokens: &[[f32; 20]]) -> ([f64; 3], [f64; 3], f64, f64) {
    let out = model.predict(tokens).map(f64::from);
    let corner = softmax(&out[0..3]);
    let tell = softmax(&out[3..6]);
    (
        [corner[0], corner[1], corner[2]],
        [tell[0], tell[1], tell[2]],
        sigmoid(out[6]),
        sigmoid(out[7]),
    )
}

#[test]
fn the_advisor_answers_from_the_model_on_the_kicks_it_saw() {
    let model = tiny();
    let advisor = ContextAdvisor::new(Arc::clone(&model));
    let (corner, tell, guess, follow) = expected(&model, &[]);
    let odds = advisor.shot_odds().unwrap();
    for (k, p) in odds {
        assert!((p - corner[usize::from(k)]).abs() < 1e-12);
    }
    let best = (0..3)
        .max_by(|&a, &b| corner[a].total_cmp(&corner[b]))
        .unwrap() as u8;
    assert_eq!(advisor.likely_shot(), Some(best));
    for shown in [0u8, 2] {
        let same = corner[usize::from(shown)] * tell[0];
        let other = corner[usize::from(2 - shown)] * tell[1];
        let theirs = advisor.their_odds(shown).unwrap();
        let p = |c: u8| {
            theirs
                .iter()
                .find(|(r, _)| *r == Some(c))
                .map_or(0.0, |x| x.1)
        };
        assert!((p(shown) - same / (same + other)).abs() < 1e-12);
        assert!((p(2 - shown) - other / (same + other)).abs() < 1e-12);
        let cover = (guess >= 0.5).then(|| if follow >= 0.5 { shown } else { 2 - shown });
        assert_eq!(advisor.likely_cover(shown), cover);
    }
    assert_eq!(advisor.their_odds(1), None);
    assert_eq!(advisor.likely_real(0), None);
    assert!(!advisor.knowing_beats_reading());
}

#[test]
fn the_advisor_updates_after_each_kick_and_forgets_a_match() {
    let model = tiny();
    let habits = Habits::person(5);
    let tokens = match_tokens(4, Advice::Blind, habits, 0, 5);
    let features: Vec<[f32; 20]> = tokens.iter().map(|t| t.features).collect();
    let mut advisor = ContextAdvisor::new(Arc::clone(&model));
    let mut tokenizer = penalty::context::Tokenizer::default();
    let states = penalty::persons::match_states(4, habits, 0, 5);
    let mut answered = Vec::new();
    for s in &states {
        advisor.see(s, 0);
        tokenizer.see(s, 0);
        let n = tokenizer.tokens().len();
        if answered.len() == n {
            answered.push(advisor.shot_odds().unwrap());
        }
    }
    assert_eq!(answered.len(), features.len() + 1);
    for (n, odds) in answered.iter().enumerate() {
        let (corner, ..) = expected(&model, &features[..n]);
        for (k, p) in odds {
            assert!(
                (p - corner[usize::from(*k)]).abs() < 1e-12,
                "after {n} kicks"
            );
        }
    }
    advisor.new_match();
    let (corner, ..) = expected(&model, &[]);
    assert!((advisor.shot_odds().unwrap()[0].1 - corner[0]).abs() < 1e-12);
}
