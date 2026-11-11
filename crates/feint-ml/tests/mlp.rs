use airhockey::Params;
use feint_ml::Policy;
use feint_ml::batch::feature_scales;
use feint_ml::mlp::{Mlp, MlpError};

const PYTHON_FILE: &[u8] = include_bytes!("../checkpoints/airhockey-g999.safetensors");
const FEATURES_BIN: &[u8] = include_bytes!("fixtures/features.bin");
const OUTPUTS_BIN: &[u8] = include_bytes!("fixtures/outputs.bin");
const RECORDED: usize = 1000;
const LAYOUT: &str = "airhockey-v3:anticipated-observation16+reaction_s+physique";

fn matrix(bytes: &[u8]) -> (usize, usize, Vec<f32>) {
    let rows = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let width = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let values: Vec<f32> = bytes[8..]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    assert_eq!(values.len(), rows * width);
    (rows, width, values)
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

struct Layer {
    weight: Vec<f32>,
    bias: Vec<f32>,
}

fn safetensors(layers: &[(usize, usize, Layer)], meta: &[(&str, String)]) -> Vec<u8> {
    let mut header = String::from("{\"__metadata__\":{");
    let pairs: Vec<String> = meta
        .iter()
        .map(|(k, v)| format!("\"{k}\":\"{v}\""))
        .collect();
    header.push_str(&pairs.join(","));
    header.push('}');
    let mut data = Vec::new();
    for (i, (rows, cols, layer)) in layers.iter().enumerate() {
        let start = data.len();
        data.extend(floats(&layer.weight));
        header.push_str(&format!(
            ",\"layer{i}.weight\":{{\"dtype\":\"F32\",\"shape\":[{rows},{cols}],\"data_offsets\":[{start},{}]}}",
            data.len()
        ));
        let start = data.len();
        data.extend(floats(&layer.bias));
        header.push_str(&format!(
            ",\"layer{i}.bias\":{{\"dtype\":\"F32\",\"shape\":[{rows}],\"data_offsets\":[{start},{}]}}",
            data.len()
        ));
    }
    header.push('}');
    let mut out = (header.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(header.as_bytes());
    out.extend(data);
    out
}

fn ones(n: usize) -> String {
    vec!["1."; n].join(",")
}

fn meta(hidden: usize) -> Vec<(&'static str, String)> {
    vec![
        ("format", "feint-mlp-v1".into()),
        ("layout", LAYOUT.into()),
        ("features", "18".into()),
        ("outputs", "2".into()),
        ("hidden", format!("{hidden},{hidden}")),
        ("activation", "relu".into()),
        ("level", String::new()),
        ("inv_scales", ones(18)),
    ]
}

fn negative_zero_net() -> Vec<(usize, usize, Layer)> {
    vec![
        (
            1,
            18,
            Layer {
                weight: vec![0.0; 18],
                bias: vec![-0.0],
            },
        ),
        (
            1,
            1,
            Layer {
                weight: vec![1.0],
                bias: vec![-0.0],
            },
        ),
        (
            2,
            1,
            Layer {
                weight: vec![1.0, -1.0],
                bias: vec![-0.0, -0.0],
            },
        ),
    ]
}

fn with(key: &str, value: &str) -> Vec<(&'static str, String)> {
    meta(1)
        .into_iter()
        .map(|(k, v)| {
            if k == key {
                (k, value.to_string())
            } else {
                (k, v)
            }
        })
        .collect()
}

fn run(mlp: &mut Mlp, features: &[f32]) -> [f32; 2] {
    let mut out = [f32::NAN; 2];
    mlp.act(features, &mut out);
    out
}

#[test]
fn forward_matches_pytorch_on_recorded_features() {
    let mut mlp = Mlp::from_safetensors(PYTHON_FILE).unwrap();
    let (rows, width, inputs) = matrix(FEATURES_BIN);
    let (out_rows, out_width, expected) = matrix(OUTPUTS_BIN);
    assert_eq!((width, out_rows, out_width), (18, rows, 2));
    assert!(rows > RECORDED);
    let mut worst_recorded = 0.0f32;
    for row in 0..rows {
        let got = run(&mut mlp, &inputs[row * 18..(row + 1) * 18]);
        for k in 0..2 {
            let want = expected[row * 2 + k];
            let diff = (got[k] - want).abs();
            if row < RECORDED {
                worst_recorded = worst_recorded.max(diff);
                assert!(diff <= 1e-5, "row {row} out {k}: {} vs {want}", got[k]);
            } else {
                assert!(
                    diff <= 1e-5 * want.abs().max(1.0),
                    "extreme row {row} out {k}: {} vs {want}",
                    got[k]
                );
            }
        }
    }
    assert!(worst_recorded > 0.0 || rows == 0);
}

#[test]
fn inverse_scales_read_back_exactly() {
    let mlp = Mlp::from_safetensors(PYTHON_FILE).unwrap();
    let scales = feature_scales(&Params::default());
    let expected: Vec<u32> = scales.iter().map(|s| (1.0f32 / s).to_bits()).collect();
    let got: Vec<u32> = mlp.inverse_scales().iter().map(|v| v.to_bits()).collect();
    assert_eq!(got, expected);
}

#[test]
fn relu_never_passes_a_negative_zero_on() {
    let bytes = safetensors(&negative_zero_net(), &meta(1));
    let mut mlp = Mlp::from_safetensors(&bytes).unwrap();
    let out = run(&mut mlp, &[-1.0; 18]);
    assert_eq!(out[0].to_bits(), 0.0f32.to_bits());
    assert_eq!(out[1].to_bits(), (-0.0f32).to_bits());
}

#[test]
fn the_same_input_gives_the_same_bits_every_time() {
    let mut mlp = Mlp::from_safetensors(PYTHON_FILE).unwrap();
    let (_, _, inputs) = matrix(FEATURES_BIN);
    let first = run(&mut mlp, &inputs[..18]);
    for row in 1..50 {
        run(&mut mlp, &inputs[row * 18..(row + 1) * 18]);
    }
    let again = run(&mut mlp, &inputs[..18]);
    assert_eq!(first.map(f32::to_bits), again.map(f32::to_bits));
}

#[test]
fn metadata_that_does_not_match_the_file_is_refused() {
    let net = negative_zero_net();
    let cases = [
        ("format", "feint-mlp-v2"),
        ("layout", "airhockey-v2:observation16"),
        ("activation", "tanh"),
        ("features", "17"),
        ("outputs", "3"),
        ("hidden", "2,2"),
        ("hidden", "1"),
    ];
    for (key, value) in cases {
        let bytes = safetensors(&net, &with(key, value));
        assert!(
            matches!(Mlp::from_safetensors(&bytes), Err(MlpError::Metadata(_))),
            "{key}={value}"
        );
    }
    let missing: Vec<_> = meta(1)
        .into_iter()
        .filter(|(k, _)| *k != "layout")
        .collect();
    let bytes = safetensors(&net, &missing);
    assert!(matches!(
        Mlp::from_safetensors(&bytes),
        Err(MlpError::Metadata(_))
    ));
}

#[test]
fn hostile_inverse_scales_are_refused() {
    let net = negative_zero_net();
    let mut cases = vec![
        String::new(),
        ones(17),
        ones(19),
        format!("{},NaN", ones(17)),
        format!("{},inf", ones(17)),
        format!("{},-inf", ones(17)),
        format!("{},abc", ones(17)),
        format!("{},,1.", ones(16)),
        format!("{},0", ones(17)),
        format!("{},-1.", ones(17)),
    ];
    cases.push(format!("{},1e40", ones(17)));
    for value in cases {
        let bytes = safetensors(&net, &with("inv_scales", &value));
        assert!(
            matches!(Mlp::from_safetensors(&bytes), Err(MlpError::Scales)),
            "{value}"
        );
    }
}

#[test]
fn layers_with_the_wrong_shapes_are_refused() {
    let mut net = negative_zero_net();
    net[1] = (
        1,
        2,
        Layer {
            weight: vec![1.0, 1.0],
            bias: vec![0.0],
        },
    );
    let bytes = safetensors(&net, &meta(1));
    assert!(matches!(
        Mlp::from_safetensors(&bytes),
        Err(MlpError::Shape(_))
    ));
    let bytes = safetensors(&negative_zero_net()[..2], &meta(1));
    assert!(matches!(
        Mlp::from_safetensors(&bytes),
        Err(MlpError::Shape(_))
    ));
}

#[test]
fn a_broken_file_is_a_format_error() {
    assert!(matches!(
        Mlp::from_safetensors(&PYTHON_FILE[..100]),
        Err(MlpError::Format(_))
    ));
}

#[test]
fn a_hidden_layer_of_width_zero_is_refused() {
    let net = vec![
        (
            0,
            18,
            Layer {
                weight: Vec::new(),
                bias: Vec::new(),
            },
        ),
        (
            1,
            0,
            Layer {
                weight: Vec::new(),
                bias: vec![0.0],
            },
        ),
        (
            2,
            1,
            Layer {
                weight: vec![1.0, 1.0],
                bias: vec![0.0, 0.0],
            },
        ),
    ];
    let mut meta = meta(1);
    for (k, v) in &mut meta {
        if *k == "hidden" {
            *v = "0,1".into();
        }
    }
    assert!(matches!(
        Mlp::from_safetensors(&safetensors(&net, &meta)),
        Err(MlpError::Shape(_))
    ));
}
