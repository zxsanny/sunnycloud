use super::open;
use crate::local_index::SCHEMA_VERSION;

#[test]
fn fresh_directory_gets_wal_normal_sync_foreign_keys_five_tables_four_indexes_and_the_schema_version()
 {
    // Arrange
    let fixture = open();

    // Act
    let (journal, synchronous, foreign_keys, version, tables, indexes) = fixture
        .store
        .write(|conn| {
            let text = |sql: &str| conn.query_row(sql, [], |row| row.get::<_, String>(0));
            let number = |sql: &str| conn.query_row(sql, [], |row| row.get::<_, i64>(0));
            let names = |sql: &str| -> rusqlite::Result<Vec<String>> {
                conn.prepare(sql)?
                    .query_map([], |row| row.get(0))?
                    .collect()
            };
            Ok((
                text("PRAGMA journal_mode")?,
                number("PRAGMA synchronous")?,
                number("PRAGMA foreign_keys")?,
                number("PRAGMA user_version")?,
                names("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?,
                names("SELECT name FROM pragma_index_list('entry') ORDER BY name")?,
            ))
        })
        .expect("inspect");

    // Assert
    assert_eq!(journal, "wal");
    assert_eq!(synchronous, 1, "NORMAL");
    assert_eq!(foreign_keys, 1);
    assert_eq!(version, i64::from(SCHEMA_VERSION));
    assert_eq!(
        tables,
        ["batch", "entry", "meta", "pair", "upload_checkpoint"]
    );
    assert_eq!(indexes.len(), 4, "{indexes:?}");
    for expected in ["entry_file_id", "entry_node_id", "entry_not_synced"] {
        assert!(indexes.iter().any(|name| name == expected), "{indexes:?}");
    }
}

#[test]
fn reopening_an_existing_index_keeps_its_data_and_version() {
    // Arrange
    let fixture = open();
    fixture
        .store
        .meta_set("server_url", "https://sync.example")
        .expect("set");
    let dir = fixture.dir;
    drop(fixture.store);

    // Act
    let reopened = super::LocalIndexStore::open(dir.path()).expect("reopen");

    // Assert
    assert_eq!(
        reopened.meta_get("server_url").expect("get").as_deref(),
        Some("https://sync.example")
    );
}

#[test]
fn the_reader_connection_cannot_write() {
    // Arrange
    let fixture = open();

    // Act
    let result = fixture
        .store
        .read(|conn| Ok(conn.execute("INSERT INTO meta (key, value) VALUES ('a', 'b')", [])?));

    // Assert
    assert!(result.is_err());
}
