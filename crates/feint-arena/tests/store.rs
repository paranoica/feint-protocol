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
    assert_eq!(store.schema_version().unwrap(), 3);
    store.issue_ticket(100, "t1", 42).unwrap();
    drop(store);
    let store = Store::open(&root).unwrap();
    assert_eq!(store.schema_version().unwrap(), 3);
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

#[test]
fn a_version_one_database_is_migrated_in_place() {
    let root = std::env::temp_dir().join(format!("feint-store-v1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("matches")).unwrap();
    let conn = rusqlite::Connection::open(root.join("feint.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE players (id INTEGER PRIMARY KEY, nick TEXT NOT NULL UNIQUE);
         CREATE TABLE tickets (token TEXT PRIMARY KEY, seed INTEGER NOT NULL, expires INTEGER NOT NULL, used INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE matches (id INTEGER PRIMARY KEY, player_id INTEGER NOT NULL, level INTEGER NOT NULL, seed INTEGER NOT NULL, score_human INTEGER NOT NULL, score_bot INTEGER NOT NULL, ticks INTEGER NOT NULL, finished INTEGER NOT NULL, digest TEXT NOT NULL, profile_used INTEGER, record_path TEXT, parquet_path TEXT, created INTEGER NOT NULL);
         CREATE TABLE profiles (player_id INTEGER NOT NULL, version INTEGER NOT NULL, parent INTEGER, blob BLOB NOT NULL, PRIMARY KEY (player_id, version));
         INSERT INTO tickets (token, seed, expires) VALUES ('old', 5, 9999);
         PRAGMA user_version = 1;",
    )
    .unwrap();
    drop(conn);
    let mut store = Store::open(&root).unwrap();
    assert_eq!(store.schema_version().unwrap(), 3);
    for _ in 0..3 {
        assert_eq!(store.try_ticket(10, "old").unwrap(), 5);
    }
    assert_eq!(
        store.try_ticket(10, "old"),
        Err(StoreError::Ticket),
        "three tries per ticket"
    );
}

#[test]
fn a_version_two_database_keeps_its_rows_as_hockey() {
    let root = std::env::temp_dir().join(format!("feint-store-v2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("matches")).unwrap();
    let conn = rusqlite::Connection::open(root.join("feint.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE players (id INTEGER PRIMARY KEY, nick TEXT NOT NULL UNIQUE);
         CREATE TABLE tickets (token TEXT PRIMARY KEY, seed INTEGER NOT NULL, expires INTEGER NOT NULL, used INTEGER NOT NULL DEFAULT 0, attempts INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE matches (id INTEGER PRIMARY KEY, player_id INTEGER NOT NULL, level INTEGER NOT NULL, seed INTEGER NOT NULL, score_human INTEGER NOT NULL, score_bot INTEGER NOT NULL, ticks INTEGER NOT NULL, finished INTEGER NOT NULL, digest TEXT NOT NULL, profile_used INTEGER, record_path TEXT, parquet_path TEXT, created INTEGER NOT NULL);
         CREATE TABLE profiles (player_id INTEGER NOT NULL, version INTEGER NOT NULL, parent INTEGER, blob BLOB NOT NULL, PRIMARY KEY (player_id, version));
         INSERT INTO players (id, nick) VALUES (1, 'ann');
         INSERT INTO matches (player_id, level, seed, score_human, score_bot, ticks, finished, digest, created) VALUES (1, 5, 3, 2, 7, 900, 1, 'ab', 10);
         INSERT INTO profiles (player_id, version, parent, blob) VALUES (1, 1, NULL, x'0102');
         PRAGMA user_version = 2;",
    )
    .unwrap();
    drop(conn);
    let store = Store::open(&root).unwrap();
    assert_eq!(store.schema_version().unwrap(), 3);
    let game: String = store
        .connection()
        .query_row("SELECT game FROM matches", [], |r| r.get(0))
        .unwrap();
    assert_eq!(game, "airhockey");
    assert_eq!(
        store.latest_profile("ann", "airhockey").unwrap(),
        Some((1, vec![1, 2]))
    );
    assert_eq!(store.latest_profile("ann", "penalty").unwrap(), None);
    drop(store);
    assert_eq!(
        Store::open(&root).unwrap().schema_version().unwrap(),
        3,
        "a second open migrates nothing"
    );
}

#[test]
fn results_count_a_draw_as_neither_and_skip_unfinished_matches() {
    let root = std::env::temp_dir().join(format!("feint-store-results-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = Store::open(&root).unwrap();
    store
        .connection()
        .execute_batch(
            "INSERT INTO players (id, nick) VALUES (1, 'ann'), (2, 'bob');
             INSERT INTO matches (player_id, game, level, seed, score_human, score_bot, ticks, finished, digest, created) VALUES
               (1, 'duel', 3, 1, 5, 2, 10, 1, 'a', 1),
               (1, 'duel', 3, 2, 0, 0, 10, 1, 'b', 1),
               (1, 'duel', 3, 3, 1, 5, 10, 1, 'c', 1),
               (1, 'duel', 3, 4, 4, 0, 10, 0, 'd', 1),
               (2, 'duel', 3, 5, 5, 0, 10, 1, 'e', 1);",
        )
        .unwrap();
    let rows = store.results("ann").unwrap();
    assert_eq!(
        rows,
        vec![feint_arena::ResultRow {
            game: "duel".into(),
            level: 3,
            matches: 3,
            won: 1,
            lost: 1,
            mine: 6,
            theirs: 7,
        }]
    );
    assert!(store.results("carl").unwrap().is_empty());
}
