use believe::check::{Referee, Tally, check_against, false_lay, wilson};
use believe::{Move, Params, Style};
use feint_turn::{TurnPolicy, turn_layout};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 5e-5
}

#[test]
fn wilson_matches_the_closed_form() {
    let (low, high) = wilson(200.0, 400.0);
    assert!(close(low, 0.4512) && close(high, 0.5488), "{low} {high}");
    let (low, high) = wilson(0.0, 400.0);
    assert!(low.abs() < 1e-12, "{low}");
    assert!(close(high, 0.009_514), "{high}");
    let (low, high) = wilson(400.0, 400.0);
    assert!((high - 1.0).abs() < 1e-12, "{high}");
    assert!(close(low, 0.990_486), "{low}");
}

#[test]
fn a_draw_counts_half() {
    let tally = Tally {
        wins: 100,
        draws: 100,
        losses: 200,
        ..Tally::default()
    };
    assert_eq!(tally.matches(), 400);
    assert!((tally.share() - 0.375).abs() < 1e-12);
    let (low, _) = tally.interval();
    let (expected, _) = wilson(150.0, 400.0);
    assert!((low - expected).abs() < 1e-12);
}

fn lay(claim: u8, counts: [u8; 5]) -> Move {
    let mut c = [0u8; believe::MAX_RANKS];
    c[..5].copy_from_slice(&counts);
    Move::Lay { claim, counts: c }
}

#[test]
fn a_lay_is_false_when_a_card_is_not_of_the_claimed_rank() {
    assert!(!false_lay(&lay(2, [0, 0, 3, 0, 0])));
    assert!(false_lay(&lay(2, [0, 1, 2, 0, 0])));
    assert!(false_lay(&lay(0, [0, 0, 0, 0, 1])));
    assert!(!false_lay(&Move::Call));
}

#[test]
fn lies_calls_and_hits_are_counted_from_the_move_stream() {
    let bot = 1u8;
    let mut referee = Referee::new(bot);
    let stream = [
        (0, lay(1, [0, 2, 0, 0, 0])),
        (1, lay(1, [1, 0, 0, 0, 0])),
        (0, lay(1, [0, 1, 0, 0, 0])),
        (1, lay(1, [0, 0, 0, 2, 0])),
        (0, Move::Call),
        (1, lay(3, [0, 0, 0, 1, 0])),
        (0, lay(3, [2, 0, 0, 0, 0])),
        (1, Move::Call),
        (0, lay(4, [0, 0, 0, 0, 1])),
        (1, Move::Call),
        (0, lay(2, [0, 0, 1, 0, 0])),
        (1, lay(2, [0, 0, 0, 0, 3])),
    ];
    for (player, m) in stream {
        referee.record(player, &m);
    }
    referee.end_match([1, -1]);
    let t = referee.tally();
    assert_eq!(t.lays, 4);
    assert_eq!(t.false_lays, 3);
    assert_eq!(t.lies_called, 1);
    assert_eq!(t.lies_survived, 2);
    assert_eq!(t.calls, 2);
    assert_eq!(t.calls_hit, 1);
    assert_eq!((t.wins, t.draws, t.losses), (0, 0, 1));
}

#[test]
fn the_seat_decides_who_won() {
    for (me, score, want) in [
        (0u8, [1, -1], (1, 0, 0)),
        (1, [1, -1], (0, 0, 1)),
        (1, [-1, 1], (1, 0, 0)),
        (0, [0, 0], (0, 1, 0)),
    ] {
        let mut referee = Referee::new(me);
        referee.end_match(score);
        let t = referee.tally();
        assert_eq!((t.wins, t.draws, t.losses), want, "seat {me} {score:?}");
    }
}

fn zero_policy() -> TurnPolicy {
    let features = believe::observation_len(&Params::default());
    let actions = believe::Moves::new(&Params::default()).len();
    let layout = turn_layout("believe", features, actions);
    let hidden = 4;
    let mut header = String::from("{");
    let mut data: Vec<u8> = Vec::new();
    for (name, shape) in [
        ("layer0.weight", vec![hidden, features]),
        ("layer0.bias", vec![hidden]),
        ("layer1.weight", vec![hidden, hidden]),
        ("layer1.bias", vec![hidden]),
        ("layer2.weight", vec![actions, hidden]),
        ("layer2.bias", vec![actions]),
    ] {
        let start = data.len();
        data.resize(start + 4 * shape.iter().product::<usize>(), 0);
        let dims: Vec<String> = shape.iter().map(ToString::to_string).collect();
        header += &format!(
            "\"{name}\":{{\"dtype\":\"F32\",\"shape\":[{}],\"data_offsets\":[{start},{}]}},",
            dims.join(","),
            data.len()
        );
    }
    let scales = vec!["1"; features].join(",");
    header += &format!(
        "\"__metadata__\":{{\"format\":\"feint-mlp-v1\",\"layout\":\"{layout}\",\"features\":\"{features}\",\"outputs\":\"{actions}\",\"hidden\":\"{hidden},{hidden}\",\"activation\":\"relu\",\"inv_scales\":\"{scales}\"}}}}"
    );
    while (8 + header.len()) % 8 != 0 {
        header.push(' ');
    }
    let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(&data);
    TurnPolicy::from_safetensors(&bytes, &layout).unwrap()
}

#[test]
fn a_uniform_policy_loses_to_the_counter_player() {
    let tally = check_against(&zero_policy(), Style::Counter, 30_000..30_060).unwrap();
    assert_eq!(tally.matches(), 60);
    let (_, high) = tally.interval();
    assert!(high < 0.5, "uniform play should lose: {tally:?}");
    assert!(tally.lays > 0 && tally.false_lays > 0, "{tally:?}");
}
