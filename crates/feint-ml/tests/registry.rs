use feint_ml::mlp::Mlp;
use feint_ml::registry::{CHECKPOINTS, fnv1a64, for_level, lookup};

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

#[test]
fn each_level_plays_the_checkpoint_picked_for_it() {
    let names: Vec<&str> = (1..=5).map(|l| for_level(l).unwrap().name).collect();
    assert_eq!(
        names,
        [
            "airhockey-early",
            "airhockey-pilot",
            "airhockey-pilot",
            "airhockey-pilot",
            "airhockey-focus"
        ]
    );
    assert_eq!(for_level(1).unwrap().id, 0x11e0_e1c1_a096_28b6);
    assert_eq!(for_level(2).unwrap().id, 0x3ac0_02e1_2274_846f);
    assert!(for_level(0).is_none());
    assert!(for_level(6).is_none());
}

#[test]
fn every_registered_checkpoint_serves_a_level() {
    for c in CHECKPOINTS {
        assert!(
            (1..=5).any(|l| for_level(l).map(|f| f.id) == Some(c.id)),
            "{}",
            c.name
        );
    }
    assert_eq!(CHECKPOINTS.len(), 3);
}

#[test]
fn level_five_is_the_checkpoint_that_met_the_bar() {
    let shipped = for_level(5).unwrap();
    assert_eq!(shipped.name, "airhockey-focus");
    assert_eq!(shipped.id, 0x1517_1d6c_57a0_caa7);
    let file = feint_ml::safetensors::SafeTensors::parse(shipped.bytes).unwrap();
    assert_eq!(file.metadata("selected_from"), Some("policy-00321"));
    assert_eq!(file.metadata("selected_wins"), Some("128"));
    assert_eq!(file.metadata("selected_matches"), Some("200"));
}

#[test]
fn the_parity_checkpoint_is_not_in_the_registry() {
    let parity = include_bytes!("../checkpoints/airhockey-g999.safetensors");
    assert_eq!(fnv1a64(parity), 0xdaf7_bada_17a0_1ab0);
    assert!(lookup(0xdaf7_bada_17a0_1ab0).is_none());
}

#[test]
fn every_shipped_checkpoint_stays_registered_so_its_records_verify() {
    for id in [
        0x11e0_e1c1_a096_28b6_u64,
        0x3ac0_02e1_2274_846f,
        0x1517_1d6c_57a0_caa7,
    ] {
        assert!(
            lookup(id).is_some(),
            "{id:016x} was shipped and must stay in the registry"
        );
    }
}
