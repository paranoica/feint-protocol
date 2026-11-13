use std::num::NonZeroU64;

use feint_ai::{Opponent, Record, decode_record, encode_record};
use feint_arena::{VerifyError, verify};
use feint_web::cards::{CARD_CAP_TICKS, CardMatch};

const SHIPPED: u64 = believe::trained::CHECKPOINTS[0].id;

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn played(seed: u64, moves: usize) -> CardMatch {
    let mut m = CardMatch::new(seed, SHIPPED).unwrap();
    let mut rng = seed | 1;
    let mut made = 0;
    let mut ticks = 0;
    while !m.finished() && made < moves && ticks < CARD_CAP_TICKS {
        if m.view().my_turn {
            let legal = m.legal();
            assert!(m.play(legal[(next(&mut rng) % legal.len() as u64) as usize]));
            made += 1;
        }
        m.advance(1);
        ticks += 1;
    }
    m
}

fn finished(seed: u64) -> CardMatch {
    let m = played(seed, usize::MAX);
    assert!(m.finished());
    m
}

fn forge(bytes: &[u8], edit: impl FnOnce(&mut Record)) -> Vec<u8> {
    let mut record = decode_record(bytes).unwrap();
    edit(&mut record);
    encode_record(&record)
}

fn hand_made_profile() -> feint_ai::Profile {
    let points = [(-0.6, 0.3), (-0.2, -0.4), (-0.8, 0.0), (-0.4, 0.45)];
    let mut s = feint_web::Session::new(5, 4).unwrap();
    for _ in 0..2_000 {
        let (x, y) = points[(s.tick() / 41) as usize % points.len()];
        s.set_pointer(x, y);
        s.advance(1);
    }
    s.profile().expect("level 5 keeps a profile")
}

#[test]
fn a_finished_card_match_verifies_and_reproduces_its_end() {
    for seed in [3, 9] {
        let m = finished(seed);
        let bytes = m.record();
        let v = verify(&bytes, None).unwrap();
        assert_eq!(v.game, "believe");
        assert!(v.columns.is_empty());
        assert!(v.frames.is_empty());
        assert!(v.finished);
        assert!(v.profile.is_none());
        assert_eq!(v.digest, m.digest());
        assert_eq!(v.score, decode_record(&bytes).unwrap().score);
        assert_eq!(
            v.record.header.opponent,
            Opponent::Trained(NonZeroU64::new(SHIPPED).unwrap())
        );
    }
}

#[test]
fn an_unfinished_card_match_verifies_as_unfinished() {
    let m = played(5, 3);
    assert!(!m.finished());
    let v = verify(&m.record(), None).unwrap();
    assert!(!v.finished);
    assert_eq!(v.digest, m.digest());
}

#[test]
fn a_card_record_with_a_profile_is_refused() {
    let m = finished(3);
    assert_eq!(
        verify(&m.record(), Some(&hand_made_profile())).err(),
        Some(VerifyError::Profile)
    );
}

#[test]
fn a_card_record_against_a_hand_made_bot_is_refused() {
    let bytes = finished(3).record();
    let forged = forge(&bytes, |r| r.header.opponent = Opponent::HandMade);
    assert_eq!(verify(&forged, None).err(), Some(VerifyError::Level));
}

#[test]
fn a_card_record_with_an_unknown_checkpoint_is_refused() {
    let bytes = finished(3).record();
    let forged = forge(&bytes, |r| {
        r.header.opponent = Opponent::Trained(NonZeroU64::new(SHIPPED ^ 1).unwrap());
    });
    assert_eq!(verify(&forged, None).err(), Some(VerifyError::Checkpoint));
}

#[test]
fn a_card_record_with_another_header_is_refused() {
    let bytes = finished(3).record();
    let cases: Vec<(Vec<u8>, VerifyError)> = vec![
        (
            forge(&bytes, |r| r.header.levels = [0, 4]),
            VerifyError::Level,
        ),
        (
            forge(&bytes, |r| r.header.levels = [0, 0]),
            VerifyError::Level,
        ),
        (
            forge(&bytes, |r| r.header.levels = [0, 6]),
            VerifyError::Level,
        ),
        (
            forge(&bytes, |r| r.header.game_version += 1),
            VerifyError::Game,
        ),
        (
            forge(&bytes, |r| r.header.bodies.truncate(1)),
            VerifyError::Bodies,
        ),
        (forge(&bytes, |r| r.ticks = 0), VerifyError::Ticks),
        (
            forge(&bytes, |r| r.ticks = CARD_CAP_TICKS + 1),
            VerifyError::Ticks,
        ),
    ];
    for (i, (forged, expected)) in cases.into_iter().enumerate() {
        assert_eq!(verify(&forged, None).err(), Some(expected), "case {i}");
    }
}

#[test]
fn a_tampered_card_record_is_refused() {
    let bytes = finished(3).record();
    let record = decode_record(&bytes).unwrap();
    let bot = record.actions.iter().position(|a| a.player == 1).unwrap();
    let human = record.actions.iter().position(|a| a.player == 0).unwrap();
    let cases = [
        forge(&bytes, |r| r.score[0] += 1),
        forge(&bytes, |r| r.actions[bot].discrete ^= 1),
        forge(&bytes, |r| r.actions[human].discrete ^= 1),
        forge(&bytes, |r| r.header.seed += 1),
        forge(&bytes, |r| {
            r.actions.pop();
        }),
    ];
    for (i, forged) in cases.iter().enumerate() {
        assert_eq!(
            verify(forged, None).err(),
            Some(VerifyError::Mismatch),
            "case {i}"
        );
    }
}

#[test]
fn a_card_record_with_a_stranger_seat_or_a_double_move_is_refused() {
    let bytes = finished(3).record();
    let human = decode_record(&bytes)
        .unwrap()
        .actions
        .iter()
        .position(|a| a.player == 0)
        .unwrap();
    let stranger = forge(&bytes, |r| r.actions[human].player = 2);
    assert_eq!(verify(&stranger, None).err(), Some(VerifyError::Seat));
    let twice = forge(&bytes, |r| {
        let copy = r.actions[human].clone();
        r.actions.insert(human, copy);
    });
    assert_eq!(verify(&twice, None).err(), Some(VerifyError::Twice));
}
