use penalty::trained::{CHECKPOINTS, SHIPPED, advisor, lookup, model};

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn each_context_checkpoint_id_is_the_hash_of_its_bytes() {
    assert!(!CHECKPOINTS.is_empty());
    for c in CHECKPOINTS {
        assert_eq!(fnv(c.bytes), c.id, "{}", c.name);
    }
    assert_eq!(SHIPPED, 0xb3d3_9ebc_d442_71df);
    assert!(lookup(SHIPPED).is_some());
}

#[test]
fn the_shipped_model_loads_and_an_unknown_id_is_refused() {
    assert!(model(SHIPPED).is_ok());
    assert!(model(1).is_err());
    assert!(advisor(1).is_err());
    assert!(advisor(SHIPPED).is_ok());
}
