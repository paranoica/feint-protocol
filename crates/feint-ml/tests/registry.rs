use feint_ml::mlp::Mlp;
use feint_ml::registry::{CHECKPOINTS, fnv1a64, lookup};

#[test]
fn fnv1a64_matches_the_published_vectors() {
    assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
}

#[test]
fn registry_ids_are_the_hash_of_their_bytes() {
    assert!(!CHECKPOINTS.is_empty());
    for c in CHECKPOINTS {
        assert_eq!(fnv1a64(c.bytes), c.id, "{}", c.name);
        assert_ne!(c.id, 0, "{}", c.name);
    }
    let mut ids: Vec<u64> = CHECKPOINTS.iter().map(|c| c.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), CHECKPOINTS.len());
}

#[test]
fn every_registered_checkpoint_loads_as_an_mlp() {
    for c in CHECKPOINTS {
        assert!(Mlp::from_safetensors(c.bytes).is_ok(), "{}", c.name);
    }
}

#[test]
fn an_unknown_id_is_not_in_the_registry() {
    assert!(lookup(0).is_none());
    for c in CHECKPOINTS {
        assert_eq!(lookup(c.id).map(|f| f.name), Some(c.name));
        assert!(lookup(c.id ^ 1).is_none());
    }
}
