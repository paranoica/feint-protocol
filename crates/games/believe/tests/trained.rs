use believe::Style;
use believe::check::check_against;
use believe::trained::{CHECKPOINTS, CardCheckpointError, lookup, policy};

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

const SHIPPED: [u64; 1] = [0x53c5_104d_2354_9530];

#[test]
fn each_checkpoint_id_is_the_hash_of_its_bytes() {
    assert!(!CHECKPOINTS.is_empty());
    for c in CHECKPOINTS {
        assert_eq!(fnv(c.bytes), c.id, "{}", c.name);
    }
}

#[test]
fn every_shipped_card_checkpoint_stays_registered() {
    for id in SHIPPED {
        assert!(lookup(id).is_some(), "{id:016x}");
    }
}

#[test]
fn an_unknown_checkpoint_is_refused() {
    assert_eq!(policy(1).err(), Some(CardCheckpointError::Unknown));
}

#[test]
fn the_shipped_policy_plays_legal_matches() {
    let p = policy(SHIPPED[0]).unwrap();
    let t = check_against(&p, Style::Honest, 40_000..40_004).unwrap();
    assert_eq!(t.matches(), 4);
    assert!(t.lays > 0);
}
