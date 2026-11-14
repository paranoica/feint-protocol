use feint_context::{ContextError, ContextModel, FEATURES, OUTPUTS, sigmoid, softmax};

const LAYOUT: &str = "context-v1:penalty:20:10:8";

type Case = (Vec<[f32; FEATURES]>, [f32; OUTPUTS]);

fn fixture() -> (Vec<u8>, Vec<Case>) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    let bytes = std::fs::read(format!("{dir}tiny.safetensors")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}tiny.txt")).unwrap();
    let mut cases = Vec::new();
    let mut tokens = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let numbers =
            |p: std::str::SplitWhitespace| -> Vec<f32> { p.map(|v| v.parse().unwrap()).collect() };
        match parts.next() {
            Some("case") => tokens.clear(),
            Some("token") => tokens.push(numbers(parts).try_into().unwrap()),
            Some("outputs") => cases.push((tokens.clone(), numbers(parts).try_into().unwrap())),
            _ => {}
        }
    }
    (bytes, cases)
}

#[test]
fn the_transformer_matches_torch_within_tolerance() {
    let (bytes, cases) = fixture();
    let model = ContextModel::from_safetensors(&bytes, LAYOUT).unwrap();
    assert_eq!(cases.len(), 4);
    for (tokens, want) in &cases {
        let got = model.predict(tokens);
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g - w).abs() < 2e-4,
                "{} kicks: {got:?} against {want:?}",
                tokens.len()
            );
        }
    }
}

#[test]
fn a_prediction_depends_only_on_the_kicks_given() {
    let (bytes, cases) = fixture();
    let model = ContextModel::from_safetensors(&bytes, LAYOUT).unwrap();
    let tokens = &cases[3].0;
    assert_eq!(model.predict(tokens), model.predict(tokens));
    assert_ne!(model.predict(&tokens[..4]), model.predict(tokens));
}

#[test]
fn more_kicks_than_the_context_slide_the_window() {
    let (bytes, cases) = fixture();
    let model = ContextModel::from_safetensors(&bytes, LAYOUT).unwrap();
    let nine = cases[3].0.clone();
    let mut twelve = vec![[0.5; FEATURES]; 3];
    twelve.extend(nine.iter().copied());
    assert_eq!(model.predict(&twelve), model.predict(&nine));
}

#[test]
fn a_file_of_another_layout_or_format_is_refused() {
    let (bytes, _) = fixture();
    assert!(matches!(
        ContextModel::from_safetensors(&bytes, "context-v1:duel:20:10:8"),
        Err(ContextError::Layout(_))
    ));
    assert!(ContextModel::from_safetensors(b"not a file", LAYOUT).is_err());
}

#[test]
fn softmax_and_sigmoid_are_probabilities() {
    let p = softmax(&[1.0, 2.0, -3.0]);
    let sum: f64 = p.iter().sum();
    assert!((sum - 1.0).abs() < 1e-12);
    let e = [
        std::f64::consts::E,
        7.389_056_098_930_65,
        0.049_787_068_367_863_944,
    ];
    let total: f64 = e.iter().sum();
    for (a, b) in p.iter().zip(e) {
        assert!((a - b / total).abs() < 1e-12);
    }
    assert!((sigmoid(0.0) - 0.5).abs() < 1e-15);
    assert!((sigmoid(2.0) - 0.880_797_077_977_882_3).abs() < 1e-12);
    assert_eq!(softmax(&[1000.0, 0.0])[1], 0.0);
}
