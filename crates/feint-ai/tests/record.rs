mod line_game;

use feint_ai::{
    Bot, Difficulty, Record, RecordError, RunHeader, decode_record, encode_record, replay_record,
    run_match,
};
use feint_protocol::{BodyParamsT, GameAdapter, MessageT, encode};
use line_game::{Line, LinePlaybook};

fn header() -> RunHeader {
    let mut body = BodyParamsT::default();
    body.values = Some(vec![1.0]);
    RunHeader {
        game_id: "line".into(),
        game_version: 1,
        seed: 21,
        bodies: vec![body.clone(), body],
        levels: [0, 4],
    }
}

fn played() -> (Record, u64) {
    let mut game = Line::new(0.0, 0.3);
    let spec = game.spec();
    let d = |l| Difficulty::preset(l).unwrap();
    let mut a = Bot::new(d(2), LinePlaybook::new(&[-0.6, 0.6], 60), &spec).unwrap();
    let mut b = Bot::new(d(4), LinePlaybook::new(&[-0.6, 0.6], 60), &spec).unwrap();
    let report = run_match(&mut game, [&mut a, &mut b], header(), 700).unwrap();
    (report.record(), report.digest)
}

#[test]
fn a_record_round_trips_through_bytes_and_replays() {
    let (record, digest) = played();
    assert!(!record.actions.is_empty());
    let bytes = encode_record(&record);
    let back = decode_record(&bytes).unwrap();
    assert_eq!(back, record);
    assert_eq!(
        replay_record(&mut Line::new(0.0, 0.3), &back).unwrap(),
        digest
    );
}

#[test]
fn broken_records_are_refused() {
    let (record, _) = played();
    let bytes = encode_record(&record);
    assert_eq!(decode_record(&[]), Err(RecordError::Missing));
    assert_eq!(
        decode_record(&bytes[..bytes.len() - 3]),
        Err(RecordError::Frame)
    );
    let mut flipped = bytes.clone();
    flipped[10] ^= 0xFF;
    assert!(decode_record(&flipped).is_err());
    let mut huge = bytes.clone();
    huge[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(decode_record(&huge), Err(RecordError::Frame));

    let frame = |m: &MessageT| {
        let body = encode(m);
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend(body);
        out
    };
    let header_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize + 4;
    let without_header = &bytes[header_len..];
    assert_eq!(decode_record(without_header), Err(RecordError::Order));
    let mut twice = bytes[..header_len].to_vec();
    twice.extend_from_slice(&bytes);
    assert_eq!(
        decode_record(&twice),
        Err(RecordError::Order),
        "two headers"
    );
    let mut back_in_time = bytes[..header_len].to_vec();
    for tick in [5u64, 4] {
        let mut a = feint_protocol::ActionT::default();
        a.tick = tick;
        back_in_time.extend(frame(&MessageT::Action(Box::new(a))));
    }
    back_in_time.extend_from_slice(&bytes[bytes.len() - last_len(&bytes)..]);
    assert_eq!(
        decode_record(&back_in_time),
        Err(RecordError::Order),
        "actions out of tick order"
    );
    let oversized = vec![0u8; feint_ai::MAX_RECORD_BYTES + 1];
    assert_eq!(decode_record(&oversized), Err(RecordError::Frame));
    let mut action = feint_protocol::ActionT::default();
    action.tick = u64::MAX;
    let mut late = bytes.clone();
    late.extend(frame(&MessageT::Action(Box::new(action))));
    assert_eq!(
        decode_record(&late),
        Err(RecordError::Order),
        "an action after the result"
    );
    let without_result = &bytes[..bytes.len() - last_len(&bytes)];
    assert_eq!(decode_record(without_result), Err(RecordError::Missing));
}

fn last_len(bytes: &[u8]) -> usize {
    let mut at = 0;
    let mut last = 0;
    while at < bytes.len() {
        let n = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize + 4;
        last = n;
        at += n;
    }
    last
}

#[test]
fn a_replay_stops_where_the_match_ended_and_refuses_longer_claims() {
    let mut game = Line::new(0.0, 0.3);
    game.end_at = Some(120);
    let spec = game.spec();
    let d = |l| Difficulty::preset(l).unwrap();
    let mut a = Bot::new(d(2), LinePlaybook::new(&[-0.6, 0.6], 60), &spec).unwrap();
    let mut b = Bot::new(d(4), LinePlaybook::new(&[-0.6, 0.6], 60), &spec).unwrap();
    let report = run_match(&mut game, [&mut a, &mut b], header(), 1_000).unwrap();
    assert!(report.finished && report.ticks == 120);
    let fresh = || {
        let mut g = Line::new(0.0, 0.3);
        g.end_at = Some(120);
        g
    };
    let record = report.record();
    assert_eq!(replay_record(&mut fresh(), &record).unwrap(), report.digest);

    let mut longer = record.clone();
    longer.ticks = 10_000;
    assert!(
        replay_record(&mut fresh(), &longer).is_err(),
        "more ticks than the match had"
    );
    let mut endless = record.clone();
    endless.ticks = u64::MAX;
    assert!(
        replay_record(&mut fresh(), &endless).is_err(),
        "more ticks than the match had"
    );

    let mut leftover = record.clone();
    let mut late = leftover.actions.last().unwrap().clone();
    late.tick = 500;
    leftover.actions.push(late);
    assert!(
        replay_record(&mut fresh(), &leftover).is_err(),
        "actions after the end"
    );
}
