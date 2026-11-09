#[macro_use]
mod builders;

use feint_protocol::{
    ActionSpaceT, ActionT, BodyParamsT, DecodeError, DimT, EnvelopeT, EventKind, EventT, GameSpecT,
    MAX_MESSAGE_BYTES, MatchHeaderT, MatchResultT, MessageT, ObservationT, PROTOCOL_VERSION,
    TimeMode, decode, encode, finish_envelope_buffer, root_as_envelope,
};

fn dim(name: &str, min: f32, max: f32, unit: &str) -> DimT {
    make!(DimT {
        name: Some(name.into()),
        min: min,
        max: max,
        unit: Some(unit.into()),
    })
}

fn realtime_spec() -> GameSpecT {
    make!(GameSpecT {
        game_id: Some("test-realtime".into()),
        game_version: 3,
        num_players: 2,
        time_mode: TimeMode::Realtime,
        tick_hz: 240,
        decision_hz: 60,
        action_space: Some(Box::new(make!(ActionSpaceT {
            continuous: Some(vec![dim("x", -1.0, 1.0, "m"), dim("y", -0.5, 0.5, "m")]),
            discrete: 0,
        }))),
        observation: Some(vec![]),
        body_params: Some(vec![dim("reaction", 0.0, 1.0, "s")]),
        signals: None,
        intent_labels: Some(vec!["none".into(), "fake".into()]),
    })
}

fn kuhn_spec() -> GameSpecT {
    make!(GameSpecT {
        game_id: Some("kuhn-poker".into()),
        game_version: 1,
        num_players: 2,
        time_mode: TimeMode::TurnBased,
        tick_hz: 0,
        decision_hz: 0,
        action_space: Some(Box::new(make!(ActionSpaceT {
            continuous: None,
            discrete: 2,
        }))),
        observation: Some(vec![
            dim("own_card", 0.0, 2.0, ""),
            dim("history", -1.0, 1.0, "")
        ]),
        body_params: Some(vec![dim("time_per_move", 0.0, 60.0, "s")]),
        signals: Some(vec![dim("bet", 0.0, 1.0, "chips")]),
        intent_labels: Some(vec!["none".into(), "bluff".into(), "value".into()]),
    })
}

fn all_messages() -> Vec<MessageT> {
    vec![
        MessageT::GameSpec(Box::new(realtime_spec())),
        MessageT::GameSpec(Box::default()),
        MessageT::Observation(Box::new(make!(ObservationT {
            tick: u64::MAX,
            player: 1,
            to_act: -1,
            state: Some(vec![0.5, -0.25, f32::MIN_POSITIVE, f32::MAX]),
            opponent_signals: Some(vec![]),
            legal: None,
            score: Some(vec![3, -2]),
        }))),
        MessageT::Action(Box::new(make!(ActionT {
            tick: 7,
            player: 0,
            values: Some(vec![0.1, -0.2]),
            discrete: -1,
            intent: 1,
            deceptive: true,
        }))),
        MessageT::Event(Box::new(make!(EventT {
            tick: 9,
            kind: EventKind::Score,
            player: 1,
        }))),
        MessageT::Event(Box::new(make!(EventT {
            tick: 10,
            kind: EventKind::Pause,
            player: -1,
        }))),
        MessageT::MatchHeader(Box::new(make!(MatchHeaderT {
            game_id: Some("test-realtime".into()),
            game_version: 3,
            seed: 0xDEAD_BEEF,
            level: 5,
            bodies: Some(vec![
                make!(BodyParamsT {
                    values: Some(vec![0.15])
                }),
                make!(BodyParamsT {
                    values: Some(vec![0.4])
                }),
            ]),
        }))),
        MessageT::MatchResult(Box::new(make!(MatchResultT {
            score: Some(vec![7, 5]),
            duration_ticks: 43_200,
            level: 1,
            seed: 42,
        }))),
    ]
}

fn raw_envelope(version: Option<&str>, message: MessageT, identifier: bool) -> Vec<u8> {
    let envelope = make!(EnvelopeT {
        protocol_version: version.map(Into::into),
        message: message,
    });
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let root = envelope.pack(&mut fbb);
    if identifier {
        finish_envelope_buffer(&mut fbb, root);
    } else {
        fbb.finish(root, None);
    }
    fbb.finished_data().to_vec()
}

#[test]
fn each_message_roundtrips_through_envelope() {
    for message in all_messages() {
        assert_eq!(decode(&encode(&message)), Ok(message));
    }
}

#[test]
fn encoded_envelope_carries_protocol_version() {
    let bytes = encode(&all_messages()[0]);
    let envelope = root_as_envelope(&bytes).expect("a valid envelope");
    assert_eq!(envelope.protocol_version(), Some(PROTOCOL_VERSION));
    assert_eq!(PROTOCOL_VERSION, "0.1");
}

#[test]
fn kuhn_poker_fits_schema_v0_1() {
    let seen_by = |player: u8, card: f32, score: Vec<i64>| {
        make!(ObservationT {
            tick: 2,
            player: player,
            to_act: 0,
            state: Some(vec![card, 1.0, -1.0]),
            opponent_signals: Some(vec![1.0]),
            legal: Some(vec![1, 1]),
            score: Some(score),
        })
    };
    let messages = [
        MessageT::GameSpec(Box::new(kuhn_spec())),
        MessageT::Observation(Box::new(seen_by(0, 2.0, vec![0, 0]))),
        MessageT::Observation(Box::new(seen_by(1, 0.0, vec![0, 0]))),
        MessageT::Action(Box::new(make!(ActionT {
            tick: 2,
            player: 0,
            values: None,
            discrete: 1,
            intent: 1,
            deceptive: true,
        }))),
        MessageT::MatchResult(Box::new(make!(MatchResultT {
            score: Some(vec![-2, 2]),
            duration_ticks: 3,
            level: 0,
            seed: 1,
        }))),
    ];
    for message in messages {
        assert_eq!(decode(&encode(&message)), Ok(message));
    }
}

#[test]
fn envelope_without_message_is_rejected() {
    let bytes = raw_envelope(Some(PROTOCOL_VERSION), MessageT::NONE, true);
    assert_eq!(decode(&bytes), Err(DecodeError::Empty));
}

#[test]
fn other_or_missing_protocol_version_is_rejected() {
    let message = all_messages().remove(3);
    let bytes = raw_envelope(Some("0.2"), message.clone(), true);
    assert_eq!(decode(&bytes), Err(DecodeError::Version("0.2".into())));
    let bytes = raw_envelope(None, message.clone(), true);
    assert_eq!(decode(&bytes), Err(DecodeError::Version(String::new())));
    let bytes = raw_envelope(Some(PROTOCOL_VERSION), message.clone(), true);
    assert_eq!(decode(&bytes), Ok(message));
}

#[test]
fn buffer_without_feint_identifier_is_rejected() {
    let bytes = raw_envelope(Some(PROTOCOL_VERSION), all_messages().remove(3), false);
    assert_eq!(decode(&bytes), Err(DecodeError::NotFeint));
}

#[test]
fn oversized_buffer_is_rejected_before_parsing() {
    let mut bytes = encode(&all_messages().remove(3));
    assert!(decode(&bytes).is_ok());
    bytes.resize(MAX_MESSAGE_BYTES + 1, 0);
    assert_eq!(
        decode(&bytes),
        Err(DecodeError::TooLarge(MAX_MESSAGE_BYTES + 1))
    );
}

#[test]
fn hostile_buffers_never_panic_or_decode_to_another_message() {
    assert!(decode(&[]).is_err());

    let mut s = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next_byte = || {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (s >> 56) as u8
    };
    for len in 0..512 {
        let mut buf: Vec<u8> = (0..len).map(|_| next_byte()).collect();
        if len >= 8 && len % 2 == 0 {
            buf[4..8].copy_from_slice(b"FNT0");
        }
        assert!(
            decode(&buf).is_err(),
            "random bytes of length {len} decoded"
        );
    }

    for message in all_messages() {
        let bytes = encode(&message);
        assert!(!bytes.is_empty());
        for n in 0..bytes.len() {
            if let Ok(got) = decode(&bytes[..n]) {
                assert_eq!(
                    got, message,
                    "prefix of {n} bytes decoded to another message"
                );
            }
        }
        for i in 0..bytes.len() {
            for flip in [0x01, 0x80, 0xFF] {
                let mut corrupt = bytes.clone();
                corrupt[i] ^= flip;
                let _ = decode(&corrupt);
            }
        }
    }
}

#[test]
fn version_error_keeps_only_a_short_prefix_of_the_client_string() {
    let long = "9".repeat(10_000);
    let bytes = raw_envelope(Some(&long), all_messages().remove(3), true);
    match decode(&bytes) {
        Err(DecodeError::Version(seen)) => assert_eq!(seen, "9".repeat(16)),
        other => panic!("expected a version error, got {other:?}"),
    }
    let bytes = raw_envelope(Some("0.2"), all_messages().remove(3), true);
    assert_eq!(decode(&bytes), Err(DecodeError::Version("0.2".into())));
}
