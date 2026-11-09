use feint_arena::{Store, StoreError, clean_nick};

#[test]
fn nicks_are_trimmed_capped_and_clean() {
    assert_eq!(clean_nick("  ann  ").unwrap(), "ann");
    assert_eq!(clean_nick(&"ё".repeat(24)).unwrap(), "ё".repeat(24));
    assert_eq!(clean_nick(&"ё".repeat(25)), Err(StoreError::Nick));
    assert_eq!(clean_nick("   "), Err(StoreError::Nick));
    assert_eq!(clean_nick("a\u{0007}b"), Err(StoreError::Nick));
    assert_eq!(clean_nick("a\u{202e}b"), Err(StoreError::Nick));
    assert_eq!(clean_nick("a\u{2067}b"), Err(StoreError::Nick));
}

#[test]
fn the_database_is_versioned_and_survives_a_restart() {
    let root = std::env::temp_dir().join(format!("feint-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut store = Store::open(&root).unwrap();
    assert_eq!(store.schema_version().unwrap(), 1);
    store.issue_ticket(100, "t1", 42).unwrap();
    drop(store);
    let store = Store::open(&root).unwrap();
    assert_eq!(store.schema_version().unwrap(), 1);
    assert_eq!(
        store.ticket_seed(200, "t1").unwrap(),
        42,
        "a ticket outlives a restart"
    );
    assert_eq!(
        store.ticket_seed(100 + 45 * 60, "t1"),
        Err(StoreError::Ticket),
        "and expires"
    );
}
