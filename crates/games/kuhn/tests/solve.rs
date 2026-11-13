use kuhn::solve::{
    Infoset, Strategy, best_response, exploitability, family, family_check, reach, value,
};

const JACK: u8 = 0;
const QUEEN: u8 = 1;
const KING: u8 = 2;

fn equilibrium(alpha: f64) -> Strategy {
    let mut s = Strategy::constant(0.0);
    s.set(Infoset::opening(JACK), alpha);
    s.set(Infoset::opening(QUEEN), 0.0);
    s.set(Infoset::opening(KING), 3.0 * alpha);
    s.set(Infoset::facing_after_pass(JACK), 0.0);
    s.set(Infoset::facing_after_pass(QUEEN), alpha + 1.0 / 3.0);
    s.set(Infoset::facing_after_pass(KING), 1.0);
    s.set(Infoset::after_pass(JACK), 1.0 / 3.0);
    s.set(Infoset::after_pass(QUEEN), 0.0);
    s.set(Infoset::after_pass(KING), 1.0);
    s.set(Infoset::facing_bet(JACK), 0.0);
    s.set(Infoset::facing_bet(QUEEN), 1.0 / 3.0);
    s.set(Infoset::facing_bet(KING), 1.0);
    s
}

#[test]
fn the_closed_form_family_has_exploitability_zero() {
    for alpha in [0.0, 1.0 / 6.0, 1.0 / 3.0] {
        let s = equilibrium(alpha);
        assert!(
            (value(&s) + 1.0 / 18.0).abs() < 1e-12,
            "value at alpha {alpha}"
        );
        assert!(
            exploitability(&s).abs() < 1e-12,
            "alpha {alpha}: {}",
            exploitability(&s)
        );
        assert_eq!(family(alpha), s);
    }
}

#[test]
fn always_pass_is_exploitable_by_the_known_amount() {
    let s = Strategy::constant(0.0);
    assert!(value(&s).abs() < 1e-12);
    assert!((best_response(&s, 0) - 1.0).abs() < 1e-12);
    assert!((best_response(&s, 1) - 1.0).abs() < 1e-12);
    assert!((exploitability(&s) - 1.0).abs() < 1e-12);
}

#[test]
fn always_bet_is_exploitable_by_the_known_amount() {
    let s = Strategy::constant(1.0);
    assert!(value(&s).abs() < 1e-12);
    assert!((best_response(&s, 0) - 1.0 / 3.0).abs() < 1e-12);
    assert!((best_response(&s, 1) - 1.0 / 3.0).abs() < 1e-12);
    assert!((exploitability(&s) - 1.0 / 3.0).abs() < 1e-12);
}

#[test]
fn best_response_is_at_least_any_pure_strategy() {
    let base = equilibrium(0.1);
    let mut skewed = base.clone();
    skewed.set(Infoset::facing_bet(QUEEN), 0.9);
    skewed.set(Infoset::opening(JACK), 0.6);
    for s in [base, skewed] {
        for player in 0..2u8 {
            let sets = Infoset::all()
                .into_iter()
                .filter(|i| i.player() == player)
                .collect::<Vec<_>>();
            for bits in 0..(1u32 << sets.len()) {
                let mut pure = s.clone();
                for (k, set) in sets.iter().enumerate() {
                    pure.set(*set, f64::from((bits >> k) & 1));
                }
                let u = value(&pure);
                let mine = if player == 0 { u } else { -u };
                assert!(mine <= best_response(&s, player) + 1e-12);
            }
        }
    }
}

#[test]
fn reach_of_the_first_decisions_is_one_third_per_card() {
    let s = equilibrium(0.2);
    for card in [JACK, QUEEN, KING] {
        assert!((reach(&s, Infoset::opening(card)) - 1.0 / 3.0).abs() < 1e-12);
    }
    let pass_rate_of_others = |card: u8| {
        [JACK, QUEEN, KING]
            .into_iter()
            .filter(|&c| c != card)
            .map(|c| 1.0 - s.get(Infoset::opening(c)))
            .sum::<f64>()
            / 6.0
    };
    for card in [JACK, QUEEN, KING] {
        assert!((reach(&s, Infoset::after_pass(card)) - pass_rate_of_others(card)).abs() < 1e-12);
    }
    assert!(
        (reach(&s, Infoset::facing_after_pass(QUEEN)) - (1.0 / 3.0 + 1.0) / 6.0).abs() < 1e-12,
        "a queen always passes and meets a bet from a jack a third of the time and from a king always"
    );
    assert!((reach(&s, Infoset::facing_bet(QUEEN)) - (0.2 + 0.6) / 6.0).abs() < 1e-12);
}

#[test]
fn family_check_reads_alpha_and_the_jack_bluff() {
    let check = family_check(&equilibrium(0.25));
    assert!((check.alpha - 0.25).abs() < 1e-12);
    assert!((check.jack_bluff - 1.0 / 3.0).abs() < 1e-12);
    assert!(check.deviation < 1e-12);
    let mut off = equilibrium(0.25);
    off.set(Infoset::facing_bet(QUEEN), 0.5);
    let check = family_check(&off);
    assert!((check.deviation - (0.5 - 1.0 / 3.0)).abs() < 1e-12);
    let mut unreached = equilibrium(1.0 / 3.0);
    unreached.set(Infoset::facing_after_pass(KING), 0.0);
    assert!(
        family_check(&unreached).deviation < 1e-12,
        "a king never passes at alpha 1/3"
    );
}

fn policy_file() -> Vec<u8> {
    let tensors: [(&str, Vec<usize>, Vec<f32>); 6] = [
        (
            "layer0.weight",
            vec![2, 4],
            vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        ),
        ("layer0.bias", vec![2], vec![0.0, 1.0]),
        ("layer1.weight", vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]),
        ("layer1.bias", vec![2], vec![0.0, 0.0]),
        ("layer2.weight", vec![2, 2], vec![0.0, 0.0, 1.0, -1.0]),
        ("layer2.bias", vec![2], vec![0.0, 0.0]),
    ];
    let mut header = String::from("{");
    let mut data = Vec::new();
    for (name, shape, values) in &tensors {
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
    header += "\"__metadata__\":{\"format\":\"feint-mlp-v1\",\"layout\":\"turn-v1:kuhn:4:2\",\"features\":\"4\",\"outputs\":\"2\",\"hidden\":\"2,2\",\"activation\":\"relu\",\"inv_scales\":\"1,1,1,1\"}}";
    while (8 + header.len()) % 8 != 0 {
        header.push(' ');
    }
    let mut out = (header.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&data);
    out
}

#[test]
fn the_policy_reader_maps_observations_to_information_sets() {
    use feint_turn::{TurnPolicy, exp, turn_layout};
    let mut policy =
        TurnPolicy::from_safetensors(&policy_file(), &turn_layout("kuhn", 4, 2)).unwrap();
    let s = kuhn::solve::strategy_from_policy(&mut policy);
    let sigmoid = |x: f64| 1.0 / (1.0 + exp(-x));
    for card in [JACK, QUEEN, KING] {
        let c = f64::from(card);
        let cases = [
            (Infoset::opening(card), c),
            (Infoset::after_pass(card), c - 1.0),
            (Infoset::facing_bet(card), c - 2.0),
            (Infoset::facing_after_pass(card), c - 1.0),
        ];
        for (set, logit) in cases {
            assert!((s.get(set) - sigmoid(logit)).abs() < 1e-6, "{set:?}");
        }
    }
}

#[test]
fn a_strategy_is_a_probability_only_when_every_value_is_in_zero_to_one() {
    assert!(Strategy::constant(0.0).is_probability());
    assert!(Strategy::constant(1.0).is_probability());
    for bad in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        let mut s = Strategy::constant(0.5);
        s.set(Infoset::facing_after_pass(2), bad);
        assert!(!s.is_probability(), "{bad}");
    }
}
