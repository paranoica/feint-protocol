use feint_turn::{PolicyError, TurnPolicy, exp, masked_softmax, turn_layout};

const REFERENCE: [(f64, f64); 8] = [
    (0.0, 1.0),
    (-0.5, 0.606_530_659_712_633_4),
    (-1.0, 0.367_879_441_171_442_33),
    (-std::f64::consts::LN_10, 0.1),
    (-10.0, 4.539_992_976_248_485_4e-5),
    (-80.0, 1.804_851_387_845_415_3e-35),
    (1.0, std::f64::consts::E),
    (-700.0, 9.859_676_543_759_77e-305),
];

#[test]
fn exp_from_arithmetic_matches_reference_values() {
    for (x, want) in REFERENCE {
        let got = exp(x);
        assert!(
            ((got - want) / want).abs() < 1e-14,
            "exp({x}) = {got}, want {want}"
        );
    }
    assert_eq!(exp(0.0), 1.0);
    let tiny = exp(-740.0);
    assert!(
        tiny > 0.0 && (tiny / 4.2e-322 - 1.0).abs() < 0.02,
        "exp(-740) = {tiny}"
    );
    assert_eq!(exp(-800.0), 0.0);
    assert_eq!(exp(f64::NEG_INFINITY), 0.0);
    assert_eq!(exp(800.0), f64::INFINITY);
    assert!(exp(f64::NAN).is_nan());
}

#[test]
fn exp_turns_sums_into_products_and_keeps_order() {
    let mut previous = 0.0;
    for i in 0..=1600 {
        let x = -80.0 + f64::from(i) * 0.05;
        let y = exp(x);
        assert!(y > previous, "exp is not increasing at {x}");
        previous = y;
        let half = exp(x / 2.0);
        assert!(
            ((half * half - y) / y).abs() < 1e-14,
            "exp({x}) is not exp({x}/2) squared"
        );
    }
}

#[test]
fn masked_softmax_gives_illegal_actions_zero_and_sums_to_one() {
    let mut out = Vec::new();
    masked_softmax(&[1.0, 2.0, 3.0, 50.0], &[1, 1, 1, 0], &mut out);
    assert_eq!(out[3], 0.0);
    let total: f32 = out.iter().sum();
    assert!((total - 1.0).abs() < 1e-6);
    let e = [exp(-2.0), exp(-1.0), 1.0];
    let sum: f64 = e.iter().sum();
    for (got, want) in out.iter().zip(e.iter().map(|v| v / sum)) {
        assert!((f64::from(*got) - want).abs() < 1e-7, "{got} vs {want}");
    }
    masked_softmax(&[1e4, -1e4, 1e4], &[1, 1, 1], &mut out);
    assert_eq!(out, vec![0.5, 0.0, 0.5]);
    masked_softmax(&[3.0, 4.0], &[0, 0], &mut out);
    assert_eq!(out, vec![0.0, 0.0], "no legal action, no probability");
    masked_softmax(&[3.0, 4.0, 5.0], &[0, 1], &mut out);
    assert_eq!(
        out,
        vec![0.0, 1.0, 0.0],
        "a short mask treats the rest as illegal"
    );
}

fn safetensors(tensors: &[(&str, Vec<usize>, Vec<f32>)], metadata: &[(&str, String)]) -> Vec<u8> {
    let mut header = String::from("{");
    let mut data = Vec::new();
    for (name, shape, values) in tensors {
        let start = data.len();
        for v in values {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let dims: Vec<String> = shape.iter().map(ToString::to_string).collect();
        header += &format!(
            "\"{name}\":{{\"dtype\":\"F32\",\"shape\":[{}],\"data_offsets\":[{start},{}]}},",
            dims.join(","),
            data.len()
        );
    }
    let meta: Vec<String> = metadata
        .iter()
        .map(|(k, v)| format!("\"{k}\":\"{v}\""))
        .collect();
    header += &format!("\"__metadata__\":{{{}}}}}", meta.join(","));
    while (8 + header.len()) % 8 != 0 {
        header.push(' ');
    }
    let mut out = (header.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&data);
    out
}

fn tiny(layout: &str) -> Vec<u8> {
    safetensors(
        &[
            ("layer0.weight", vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]),
            ("layer0.bias", vec![2], vec![0.0, 0.0]),
            ("layer1.weight", vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]),
            ("layer1.bias", vec![2], vec![0.0, -1.0]),
            (
                "layer2.weight",
                vec![3, 2],
                vec![1.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            ),
            ("layer2.bias", vec![3], vec![0.0, 0.0, -10.0]),
        ],
        &[
            ("format", "feint-mlp-v1".into()),
            ("layout", layout.into()),
            ("features", "2".into()),
            ("outputs", "3".into()),
            ("hidden", "2,2".into()),
            ("activation", "relu".into()),
            ("inv_scales", "0.5,1".into()),
        ],
    )
}

#[test]
fn turn_policy_computes_logits_and_masked_probabilities() {
    let layout = turn_layout("toy", 2, 3);
    assert_eq!(layout, "turn-v1:toy:2:3");
    let mut policy = TurnPolicy::from_safetensors(&tiny(&layout), &layout).unwrap();
    assert_eq!(policy.features(), 2);
    assert_eq!(policy.actions(), 3);
    assert_eq!(policy.logits(&[4.0, 3.0]), &[2.0, 2.0, -6.0]);
    assert_eq!(policy.logits(&[-4.0, 0.5]), &[0.0, 0.0, -10.0]);
    let mut out = Vec::new();
    policy.probabilities(&[4.0, 3.0], &[1, 0, 1], &mut out);
    let low = exp(-8.0) / (1.0 + exp(-8.0));
    assert!((f64::from(out[2]) - low).abs() < 1e-9);
    assert_eq!(out[1], 0.0);
}

#[test]
fn turn_policy_refuses_a_wrong_layout_or_shape() {
    let layout = turn_layout("toy", 2, 3);
    assert_eq!(
        TurnPolicy::from_safetensors(&tiny(&layout), &turn_layout("other", 2, 3)).err(),
        Some(PolicyError::Metadata("layout"))
    );
    assert_eq!(
        TurnPolicy::from_safetensors(
            &tiny("airhockey-v3:anticipated-observation16+reaction_s+physique"),
            &layout
        )
        .err(),
        Some(PolicyError::Metadata("layout"))
    );
    let mut bad = tiny(&layout);
    bad.truncate(bad.len() - 4);
    assert!(matches!(
        TurnPolicy::from_safetensors(&bad, &layout),
        Err(PolicyError::Format(_))
    ));
    let wrong_scales = safetensors(
        &[
            ("layer0.weight", vec![2, 2], vec![1.0; 4]),
            ("layer0.bias", vec![2], vec![0.0; 2]),
            ("layer1.weight", vec![2, 2], vec![1.0; 4]),
            ("layer1.bias", vec![2], vec![0.0; 2]),
            ("layer2.weight", vec![3, 2], vec![1.0; 6]),
            ("layer2.bias", vec![3], vec![0.0; 3]),
        ],
        &[
            ("format", "feint-mlp-v1".into()),
            ("layout", layout.clone()),
            ("features", "2".into()),
            ("outputs", "3".into()),
            ("hidden", "2,2".into()),
            ("activation", "relu".into()),
            ("inv_scales", "0.5,-1".into()),
        ],
    );
    assert_eq!(
        TurnPolicy::from_safetensors(&wrong_scales, &layout).err(),
        Some(PolicyError::Scales)
    );
}
