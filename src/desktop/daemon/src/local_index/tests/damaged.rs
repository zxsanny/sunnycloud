use std::fs;
use std::path::Path;

use rusqlite::Connection;

use super::{entry, new_pair, open};
use crate::local_index::{INDEX_FILE_NAME, IndexError, LocalIndexStore, SCHEMA_VERSION};

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn assert_rebuild_gives_an_empty_clean_store(dir: &Path) {
    let rebuilt = LocalIndexStore::rebuild(dir).expect("rebuild");
    assert_eq!(rebuilt.get_pair(1).expect("pair"), None);
    assert_eq!(rebuilt.meta_get("server_url").expect("meta"), None);
    assert_eq!(rebuilt.cursor().expect("cursor"), None);
    drop(rebuilt);
    LocalIndexStore::open(dir).expect("opens cleanly after rebuild");
}

#[test]
fn a_file_of_garbage_is_index_corrupt_and_left_untouched_then_rebuild_gives_an_empty_store() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join(INDEX_FILE_NAME);
    fs::write(&file, vec![0xA5u8; 8192]).expect("write garbage");
    let before = fs::read(&file).expect("read");
    let before_listing = listing(dir.path());

    // Act
    let result = LocalIndexStore::open(dir.path());

    // Assert
    assert!(
        matches!(result, Err(IndexError::IndexCorrupt)),
        "{:?}",
        result.err()
    );
    assert_eq!(fs::read(&file).expect("read"), before);
    assert_eq!(listing(dir.path()), before_listing);
    assert_rebuild_gives_an_empty_clean_store(dir.path());
}

#[test]
fn a_real_index_with_overwritten_pages_is_index_corrupt() {
    // Arrange
    let fixture = open();
    let pair = fixture.store.add_pair(&new_pair(false)).expect("pair");
    let batch: Vec<_> = (0..500)
        .map(|n| entry(pair, &format!("dir/file-{n}.txt"), n))
        .collect();
    fixture.store.upsert_entries(&batch).expect("upsert");
    let dir = fixture.dir;
    drop(fixture.store);
    let file = dir.path().join(INDEX_FILE_NAME);
    let mut bytes = fs::read(&file).expect("read");
    assert!(bytes.len() > 3 * 4096, "the index has several pages");
    bytes[4096..].fill(0xFF);
    fs::write(&file, &bytes).expect("damage");

    // Act
    let result = LocalIndexStore::open(dir.path());

    // Assert
    assert!(
        matches!(result, Err(IndexError::IndexCorrupt)),
        "{:?}",
        result.err()
    );
    assert_eq!(fs::read(&file).expect("read"), bytes);
    assert_rebuild_gives_an_empty_clean_store(dir.path());
}

#[test]
fn an_index_with_a_higher_user_version_is_refused_untouched_then_rebuild_gives_an_empty_store() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join(INDEX_FILE_NAME);
    let newer = SCHEMA_VERSION + 1;
    {
        let conn = Connection::open(&file).expect("create");
        conn.execute_batch("CREATE TABLE from_the_future (x INTEGER)")
            .expect("table");
        conn.pragma_update(None, "user_version", newer)
            .expect("version");
    }
    let before = fs::read(&file).expect("read");
    let before_listing = listing(dir.path());

    // Act
    let result = LocalIndexStore::open(dir.path());

    // Assert
    assert!(
        matches!(result, Err(IndexError::IndexNewerThanDaemon { found, supported }) if found == newer && supported == SCHEMA_VERSION),
        "{:?}",
        result.err()
    );
    assert_eq!(fs::read(&file).expect("read"), before);
    assert_eq!(listing(dir.path()), before_listing);
    assert_rebuild_gives_an_empty_clean_store(dir.path());
}

#[test]
fn rebuild_of_a_populated_index_with_its_wal_files_leaves_an_empty_store() {
    // Arrange
    let fixture = open();
    fixture
        .store
        .meta_set("server_url", "https://sync.example")
        .expect("set");
    let dir = fixture.dir;
    drop(fixture.store);

    // Act and Assert
    assert_rebuild_gives_an_empty_clean_store(dir.path());
}

#[test]
fn rebuild_in_a_directory_without_an_index_creates_one() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");

    // Act
    let store = LocalIndexStore::rebuild(&dir.path().join("not-yet")).expect("rebuild");

    // Assert
    assert_eq!(store.get_pair(1).expect("pair"), None);
}
