use sync_core_proto::Cursor;
use uuid::Uuid;

use super::{entry, new_pair, open};
use crate::local_index::{EntryState, PairState};
use crate::platform_fs::FileId;

#[test]
fn case_insensitive_pair_finds_and_updates_the_same_entry_in_any_case() {
    // Arrange
    let fixture = open();
    let pair = fixture.store.add_pair(&new_pair(true)).expect("pair");
    fixture
        .store
        .upsert_entries(&[entry(pair, "Docs/Report.txt", 1)])
        .expect("insert");

    // Act
    let found = fixture
        .store
        .get_entry(pair, "docs/report.TXT")
        .expect("lookup");
    let mut renamed = entry(pair, "docs/report.txt", 1);
    renamed.size = 999;
    fixture.store.upsert_entries(&[renamed]).expect("upsert");

    // Assert
    assert_eq!(found.expect("found").rel_path, "Docs/Report.txt");
    let all = fixture
        .store
        .entries_in_state(pair, EntryState::Synced)
        .expect("all");
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].rel_path, "docs/report.txt");
    assert_eq!(all[0].size, 999);
}

#[test]
fn case_sensitive_pair_keeps_paths_that_differ_in_case_as_separate_entries() {
    // Arrange
    let fixture = open();
    let pair = fixture.store.add_pair(&new_pair(false)).expect("pair");

    // Act
    fixture
        .store
        .upsert_entries(&[entry(pair, "Report.txt", 1), entry(pair, "report.txt", 2)])
        .expect("upsert");

    // Assert
    let all = fixture
        .store
        .entries_in_state(pair, EntryState::Synced)
        .expect("all");
    assert_eq!(all.len(), 2);
    assert_eq!(
        fixture
            .store
            .get_entry(pair, "Report.txt")
            .expect("get")
            .expect("found")
            .size,
        10
    );
    assert_eq!(
        fixture
            .store
            .get_entry(pair, "report.txt")
            .expect("get")
            .expect("found")
            .size,
        20
    );
}

#[test]
fn case_folding_is_per_pair() {
    // Arrange
    let fixture = open();
    let insensitive = fixture.store.add_pair(&new_pair(true)).expect("pair");
    let sensitive = fixture.store.add_pair(&new_pair(false)).expect("pair");

    // Act
    fixture
        .store
        .upsert_entries(&[entry(insensitive, "A.txt", 1), entry(sensitive, "A.txt", 2)])
        .expect("upsert");

    // Assert
    assert!(
        fixture
            .store
            .get_entry(insensitive, "a.txt")
            .expect("get")
            .is_some()
    );
    assert!(
        fixture
            .store
            .get_entry(sensitive, "a.txt")
            .expect("get")
            .is_none()
    );
}

#[test]
fn an_entry_round_trips_every_field() {
    // Arrange
    let fixture = open();
    let pair = fixture.store.add_pair(&new_pair(false)).expect("pair");
    let mut stored = entry(pair, "a/b.txt", 7);
    stored.state = EntryState::NameEscaped;
    stored.settle_until = Some(1_700_000_000);
    stored.escaped_from = Some("a/b?.txt".to_owned());
    let bare = {
        let mut e = entry(pair, "c", 8);
        e.node_id = None;
        e.revision_id = None;
        e.content_hash = None;
        e
    };

    // Act
    fixture
        .store
        .upsert_entries(&[stored.clone(), bare.clone()])
        .expect("upsert");

    // Assert
    assert_eq!(
        fixture.store.get_entry(pair, "a/b.txt").expect("get"),
        Some(stored)
    );
    assert_eq!(fixture.store.get_entry(pair, "c").expect("get"), Some(bare));
    assert_eq!(fixture.store.get_entry(pair, "missing").expect("get"), None);
}

#[test]
fn lookups_by_file_id_node_and_state_return_the_matching_entries() {
    // Arrange
    let fixture = open();
    let pair = fixture.store.add_pair(&new_pair(false)).expect("pair");
    let other = fixture.store.add_pair(&new_pair(false)).expect("pair");
    let mut dirty = entry(pair, "dirty.txt", 2);
    dirty.state = EntryState::Dirty;
    let mut held = entry(pair, "held.txt", 3);
    held.state = EntryState::DeleteHeld;
    fixture
        .store
        .upsert_entries(&[
            entry(pair, "synced.txt", 1),
            dirty,
            held,
            entry(other, "synced.txt", 1),
        ])
        .expect("upsert");

    // Act
    let by_file = fixture
        .store
        .find_by_file_id(
            pair,
            FileId {
                volume: 1,
                index: 2,
            },
        )
        .expect("file id");
    let by_node = fixture
        .store
        .find_by_node(pair, Uuid::from_u128(4))
        .expect("node");
    let dirty_only = fixture
        .store
        .entries_in_state(pair, EntryState::Dirty)
        .expect("dirty");
    let synced = fixture
        .store
        .entries_in_state(pair, EntryState::Synced)
        .expect("synced");

    // Assert
    assert_eq!(
        by_file
            .iter()
            .map(|e| e.rel_path.as_str())
            .collect::<Vec<_>>(),
        ["dirty.txt"]
    );
    assert_eq!(
        by_node
            .iter()
            .map(|e| e.rel_path.as_str())
            .collect::<Vec<_>>(),
        ["held.txt"]
    );
    assert_eq!(dirty_only.len(), 1);
    assert_eq!(synced.len(), 1);
    assert_eq!(synced[0].rel_path, "synced.txt");
}

#[test]
fn upsert_into_an_unknown_pair_fails_and_stores_nothing_of_the_batch() {
    // Arrange
    let fixture = open();
    let pair = fixture.store.add_pair(&new_pair(false)).expect("pair");

    // Act
    let result = fixture
        .store
        .upsert_entries(&[entry(pair, "ok.txt", 1), entry(pair + 100, "orphan.txt", 2)]);

    // Assert
    assert!(result.is_err());
    assert_eq!(fixture.store.get_entry(pair, "ok.txt").expect("get"), None);
}

#[test]
fn case_insensitive_pair_rejects_a_path_with_an_illegal_component() {
    // Arrange
    let fixture = open();
    let pair = fixture.store.add_pair(&new_pair(true)).expect("pair");

    // Act
    let result = fixture.store.upsert_entries(&[entry(pair, "a//b", 1)]);

    // Assert
    assert!(matches!(
        result,
        Err(crate::local_index::IndexError::InvalidPath(_))
    ));
}

#[test]
fn repeated_set_cursor_for_two_pairs_leaves_one_cursor_row() {
    // Arrange
    let fixture = open();
    fixture.store.add_pair(&new_pair(false)).expect("pair");
    fixture.store.add_pair(&new_pair(false)).expect("pair");

    // Act
    for seq in 1..=5 {
        fixture
            .store
            .set_cursor(Cursor::new(3, seq))
            .expect("cursor");
    }

    // Assert
    assert_eq!(
        fixture.store.cursor().expect("cursor"),
        Some(Cursor::new(3, 5))
    );
    let rows = fixture
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM meta WHERE key LIKE '%cursor%'",
                [],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .expect("count");
    assert_eq!(rows, 1);
}

#[test]
fn pair_state_and_hold_detail_survive_a_reopen() {
    // Arrange
    let fixture = open();
    let pair = fixture.store.add_pair(&new_pair(true)).expect("pair");

    // Act
    let changed = fixture
        .store
        .set_pair_state(pair, PairState::HoldMassDelete, Some("250 deletes pending"))
        .expect("set");
    let missing = fixture
        .store
        .set_pair_state(pair + 1, PairState::Paused, None)
        .expect("set");
    let dir = fixture.dir;
    drop(fixture.store);
    let reopened = crate::local_index::LocalIndexStore::open(dir.path()).expect("reopen");

    // Assert
    assert!(changed);
    assert!(!missing);
    let stored = reopened.get_pair(pair).expect("get").expect("pair");
    assert_eq!(stored.state, PairState::HoldMassDelete);
    assert_eq!(stored.hold_detail.as_deref(), Some("250 deletes pending"));
    assert!(stored.case_insensitive);
}

#[test]
fn no_column_of_the_schema_can_hold_a_token_and_the_files_hold_none_of_the_stored_secrets() {
    // Arrange
    let fixture = open();
    let pair = fixture.store.add_pair(&new_pair(false)).expect("pair");
    fixture
        .store
        .meta_set("server_url", "https://sync.example")
        .expect("meta");
    fixture.store.set_cursor(Cursor::new(1, 2)).expect("cursor");
    fixture
        .store
        .upsert_entries(&[entry(pair, "file.txt", 1)])
        .expect("upsert");

    // Act
    let columns: Vec<String> = fixture
        .store
        .read(|conn| {
            Ok(conn
                .prepare("SELECT m.name || '.' || p.name FROM sqlite_master m, pragma_table_info(m.name) p WHERE m.type = 'table'")?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?)
        })
        .expect("columns");

    // Assert
    assert!(columns.len() > 20, "{columns:?}");
    for column in &columns {
        for word in ["token", "secret", "password", "credential", "refresh"] {
            assert!(!column.contains(word), "{column}");
        }
    }
}
