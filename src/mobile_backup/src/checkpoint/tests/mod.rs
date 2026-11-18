use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use sync_core::{
    BatchRecord, CheckpointStore, MemoryCheckpointStore, UploadCheckpoint, UploadState,
};
use sync_core_proto::{BatchEntry, FileHash, Hash32};
use tempfile::TempDir;
use uuid::Uuid;

use super::{CheckpointDb, DbError, SCHEMA_VERSION, SqliteCheckpointStore};

const TWO_VERSIONS: &[&str] = &[
    "CREATE TABLE cursor (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    "ALTER TABLE cursor ADD COLUMN note TEXT",
];
const FIRST_VERSION: &[&str] = &[TWO_VERSIONS[0]];

/// One statement without parameters, answering the transaction's own error type.
fn exec(t: &rusqlite::Transaction<'_>, sql: &str) -> Result<(), DbError> {
    t.execute(sql, [])?;
    Ok(())
}

fn db_in(dir: &TempDir) -> CheckpointDb {
    CheckpointDb::new(dir.path().join("checkpoint.db"))
}

fn user_version(db: &CheckpointDb) -> u32 {
    db.read(|c| Ok(c.query_row("PRAGMA user_version", [], |row| row.get(0))?))
        .expect("version")
}

fn table_names(db: &CheckpointDb) -> Vec<String> {
    db.read(|c| {
        let mut statement =
            c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(names)
    })
    .expect("tables")
}

fn integrity(db: &CheckpointDb) -> String {
    db.read(|c| Ok(c.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?))
        .expect("integrity check")
}

#[test]
fn a_new_database_has_the_four_tables_at_the_current_version_and_a_second_open_applies_nothing() {
    // Arrange
    let dir = TempDir::new().expect("temp dir");
    let first = db_in(&dir);

    // Act
    let version = user_version(&first);
    let tables = table_names(&first);
    drop(first);
    let second = db_in(&dir);
    let second_version = user_version(&second);

    // Assert
    assert_eq!((version, second_version), (SCHEMA_VERSION, SCHEMA_VERSION));
    assert_eq!(tables, ["asset", "batch", "cursor", "session"]);
}

#[test]
fn the_database_runs_in_wal_mode_with_full_synchronous_writes() {
    // Arrange
    let dir = TempDir::new().expect("temp dir");
    let db = db_in(&dir);

    // Act
    let (journal, synchronous) = db
        .read(|c| {
            Ok((
                c.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?,
                c.query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))?,
            ))
        })
        .expect("pragmas");

    // Assert
    assert_eq!((journal.as_str(), synchronous), ("wal", 2));
}

#[test]
fn a_reopen_does_not_run_the_applied_migrations_again() {
    // Arrange: migration 1 would fail on a second run (the table exists).
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("checkpoint.db");
    CheckpointDb::with_migrations(&path, FIRST_VERSION)
        .write(|t| exec(t, "INSERT INTO cursor (key, value) VALUES ('k', 'v')"))
        .expect("first open");

    // Act
    let reopened = CheckpointDb::with_migrations(&path, FIRST_VERSION);
    let rows: i64 = reopened
        .read(|c| Ok(c.query_row("SELECT count(*) FROM cursor", [], |row| row.get(0))?))
        .expect("reopen");

    // Assert
    assert_eq!(rows, 1);
}

#[test]
fn an_older_database_is_migrated_forward_with_its_rows_intact() {
    // Arrange
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("checkpoint.db");
    CheckpointDb::with_migrations(&path, FIRST_VERSION)
        .write(|t| exec(t, "INSERT INTO cursor (key, value) VALUES ('k', 'v')"))
        .expect("old build");

    // Act
    let migrated = CheckpointDb::with_migrations(&path, TWO_VERSIONS);
    let (version, row) = (
        user_version(&migrated),
        migrated
            .read(|c| {
                Ok(
                    c.query_row("SELECT key, value, note FROM cursor", [], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    })?,
                )
            })
            .expect("row"),
    );

    // Assert
    assert_eq!(version, 2);
    assert_eq!(row, ("k".to_owned(), "v".to_owned(), None));
}

#[test]
fn a_database_of_a_newer_version_is_refused_and_left_unchanged() {
    // Arrange: a file written by a build with one more migration than this one knows.
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("checkpoint.db");
    CheckpointDb::with_migrations(&path, TWO_VERSIONS)
        .write(|t| exec(t, "INSERT INTO cursor (key, value) VALUES ('k', 'v')"))
        .expect("newer build");
    let before = fs::read(&path).expect("file before");
    let wal_before = fs::read(path.with_extension("db-wal")).ok();

    // Act
    let refusal = CheckpointDb::with_migrations(&path, FIRST_VERSION)
        .read(|_| Ok(()))
        .expect_err("a newer schema is refused");

    // Assert
    assert!(
        matches!(
            refusal,
            DbError::NewerSchema {
                found: 2,
                supported: 1
            }
        ),
        "{refusal:?}"
    );
    assert_eq!(fs::read(&path).expect("file after"), before);
    assert_eq!(fs::read(path.with_extension("db-wal")).ok(), wal_before);
}

#[test]
fn constructing_the_database_and_its_store_creates_no_file() {
    // Arrange
    let dir = TempDir::new().expect("temp dir");

    // Act
    let db = Arc::new(db_in(&dir));
    let _store = SqliteCheckpointStore::new(Arc::clone(&db));

    // Assert
    assert_eq!(fs::read_dir(dir.path()).expect("dir").count(), 0);
}

fn sample_checkpoint(seed: u8, state: UploadState) -> UploadCheckpoint {
    UploadCheckpoint {
        session_id: (state != UploadState::Hashing).then(|| Uuid::from_bytes([seed; 16])),
        block_hashes: (0..3)
            .map(|i| Hash32::from([seed.wrapping_add(i); 32]))
            .collect(),
        file_hash: (state != UploadState::Hashing).then(|| FileHash::from([seed; 32])),
        commit_id: Uuid::from_bytes([seed.wrapping_add(100); 16]),
        state,
    }
}

fn sample_batch(seed: u8, members: u8) -> BatchRecord {
    BatchRecord {
        commit_id: Uuid::from_bytes([seed; 16]),
        entries: (0..members)
            .map(|i| BatchEntry {
                staged_key: Uuid::from_bytes([seed.wrapping_add(i + 1); 16]),
                parent: Uuid::from_bytes([9; 16]),
                name: format!("IMG_{seed}_{i}.jpg"),
                base_revision: (i % 2 == 0).then(|| Uuid::from_bytes([7; 16])),
                file_hash: FileHash::from([seed; 32]),
                client_meta: None,
                import_hint: None,
            })
            .collect(),
    }
}

/// The host-traits contract cases (13 host_traits.md IT-13-7): the answers of any `CheckpointStore`.
fn contract_answers(store: &dyn CheckpointStore) -> Vec<String> {
    let mut answers = Vec::new();
    let mut note = |label: &str, value: String| answers.push(format!("{label}: {value}"));
    note("load absent", format!("{:?}", store.load("a")));
    for (key, seed, state) in [
        ("a", 1, UploadState::Hashing),
        ("b", 2, UploadState::Uploading),
        ("c", 3, UploadState::Committing),
    ] {
        store
            .save(key, sample_checkpoint(seed, state))
            .expect("save");
    }
    for key in ["a", "b", "c", "z"] {
        note(
            &format!("load {key}"),
            format!("{:?}", store.load(key).expect("load")),
        );
    }
    store
        .save("a", sample_checkpoint(9, UploadState::Uploading))
        .expect("overwrite");
    note(
        "load a after overwrite",
        format!("{:?}", store.load("a").expect("load")),
    );
    store.clear("b").expect("clear");
    store.clear("never-saved").expect("clear of an absent key");
    note(
        "load b after clear",
        format!("{:?}", store.load("b").expect("load")),
    );
    note(
        "batches empty",
        format!("{:?}", store.load_batches().expect("load batches")),
    );
    for (seed, members) in [(30, 3), (10, 1), (20, 5)] {
        store
            .save_batch(sample_batch(seed, members))
            .expect("save batch");
    }
    note(
        "batches",
        format!("{:?}", store.load_batches().expect("load batches")),
    );
    store
        .clear_batch(Uuid::from_bytes([20; 16]))
        .expect("clear batch");
    store
        .clear_batch(Uuid::from_bytes([99; 16]))
        .expect("clear of an absent batch");
    note(
        "batches after clear",
        format!("{:?}", store.load_batches().expect("load batches")),
    );
    answers
}

#[test]
fn the_sqlite_store_answers_the_contract_cases_as_the_memory_store_does() {
    // Arrange
    let dir = TempDir::new().expect("temp dir");
    let sqlite = SqliteCheckpointStore::new(Arc::new(db_in(&dir)));
    let memory = MemoryCheckpointStore::new();

    // Act
    let from_sqlite = contract_answers(&sqlite);
    let from_memory = contract_answers(&memory);

    // Assert
    assert_eq!(from_sqlite, from_memory);
}

#[test]
fn every_record_survives_a_restart_of_the_store() {
    // Arrange
    let dir = TempDir::new().expect("temp dir");
    let first = SqliteCheckpointStore::new(Arc::new(db_in(&dir)));
    first
        .save("a", sample_checkpoint(1, UploadState::Uploading))
        .expect("save");
    first.save_batch(sample_batch(10, 4)).expect("save batch");
    drop(first);

    // Act
    let second = SqliteCheckpointStore::new(Arc::new(db_in(&dir)));

    // Assert
    assert_eq!(
        second.load("a").expect("load"),
        Some(sample_checkpoint(1, UploadState::Uploading))
    );
    assert_eq!(
        second.load_batches().expect("batches"),
        vec![sample_batch(10, 4)]
    );
}

#[test]
fn a_malformed_stored_record_is_an_error_and_not_a_wrong_checkpoint() {
    // Arrange: a session row whose state text no build writes.
    let dir = TempDir::new().expect("temp dir");
    let db = Arc::new(db_in(&dir));
    let corrupt = db.write(|t| {
        t.execute("PRAGMA ignore_check_constraints = ON", [])?;
        t.execute(
            "INSERT INTO session (local_key, state, commit_id, block_hashes) VALUES ('k', 'bogus', zeroblob(16), zeroblob(0))",
            [],
        )?;
        Ok(())
    });
    let store = SqliteCheckpointStore::new(Arc::clone(&db));

    // Act
    let loaded = store.load("k");

    // Assert
    assert!(corrupt.is_ok(), "{corrupt:?}");
    assert!(loaded.is_err(), "{loaded:?}");
}

#[test]
fn fifty_thousand_filled_asset_rows_fit_in_ten_megabytes_once_checkpointed() {
    // Arrange
    let dir = TempDir::new().expect("temp dir");
    let db = db_in(&dir);
    db.write(|t| {
        let mut insert = t.prepare(
            "INSERT INTO asset (asset_id, change_token, size, date_taken, content_hash, upload_name, client_key, \
             node_id, revision_id, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'uploaded')",
        )?;
        for id in 1..=50_000i64 {
            let seed = (id % 251) as u8;
            insert.execute(rusqlite::params![
                id,
                format!("{}", 1_763_000_000_000i64 + id),
                (id * 7919) % 900_000 + 100_000,
                1_700_000_000_000i64 + id * 61_000,
                [seed; 32].to_vec(),
                format!("IMG_20261118_{:06}_{id:05}.jpg", id % 86_400),
                [seed.wrapping_add(1); 16].to_vec(),
                [seed.wrapping_add(2); 16].to_vec(),
                [seed.wrapping_add(3); 16].to_vec(),
            ])?;
        }
        Ok(())
    })
    .expect("rows");

    // Act
    db.read(|c| Ok(c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?))
        .expect("checkpoint");
    let bytes = fs::metadata(db.path()).expect("db file").len();

    // Assert
    assert!(bytes < 10 * 1024 * 1024, "{bytes} bytes");
}

const CHILD_ENV: &str = "MOBILE_BACKUP_KILL_TEST_DB";
/// Far more than ten kills can reach, so the writer is always mid-run when it is killed.
const CHILD_STEPS: i64 = 1_000_000;

/// The writer the kill test starts as its own process: one transaction per step that inserts asset `i`, a batch
/// record and the step counter together, then reports the step on stdout. Without the variable it does nothing.
#[test]
fn kill_child_writer() {
    let Ok(path) = std::env::var(CHILD_ENV) else {
        return;
    };
    let db = CheckpointDb::new(path);
    let start: i64 = db
        .read(|c| {
            Ok(
                c.query_row("SELECT coalesce(max(asset_id), 0) FROM asset", [], |row| {
                    row.get(0)
                })?,
            )
        })
        .expect("resume point");
    for step in start + 1..=CHILD_STEPS {
        db.write(|t| {
            t.execute(
                "INSERT INTO asset (asset_id, change_token, size, date_taken) VALUES (?1, 't', 1, 1)",
                [step],
            )?;
            t.execute(
                "INSERT INTO batch (commit_id, entries) VALUES (?1, '[]')",
                [Uuid::from_u64_pair(0, step as u64).as_bytes().to_vec()],
            )?;
            t.execute(
                "INSERT INTO cursor (key, value) VALUES ('steps', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                [step.to_string()],
            )?;
            Ok(())
        })
        .expect("step");
        println!("committed {step}");
    }
}

fn counts(db: &CheckpointDb) -> (i64, i64, i64) {
    db.read(|c| {
        Ok((
            c.query_row("SELECT count(*) FROM asset", [], |row| row.get(0))?,
            c.query_row("SELECT count(*) FROM batch", [], |row| row.get(0))?,
            c.query_row("SELECT coalesce((SELECT CAST(value AS INTEGER) FROM cursor WHERE key = 'steps'), 0)", [], |row| row.get(0))?,
        ))
    })
    .expect("counts")
}

#[test]
fn ten_sigkills_of_a_writing_process_leave_every_commit_and_no_partial_one() {
    // Arrange
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("checkpoint.db");
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .subsec_nanos() as u64
        | 1;
    let mut acknowledged = 0i64;

    for round in 1..=10 {
        // Act: a writer process, killed at a random moment after it has committed something.
        let mut child = Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "checkpoint::tests::kill_child_writer",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_ENV, &path)
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the writer");
        let mut lines = BufReader::new(child.stdout.take().expect("stdout"));
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                lines.read_line(&mut line).expect("read") > 0,
                "round {round}: the writer ended early"
            );
            if let Some(step) = line.trim().strip_prefix("committed ") {
                acknowledged = acknowledged.max(step.parse().expect("step number"));
                break;
            }
        }
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        std::thread::sleep(Duration::from_millis(seed >> 33 & 0xFF));
        child.kill().expect("SIGKILL");
        for rest in lines.lines().map_while(Result::ok) {
            if let Some(step) = rest.strip_prefix("committed ") {
                acknowledged = acknowledged.max(step.parse().expect("step number"));
            }
        }
        child.wait().expect("reap");

        // Assert: the database reopens, is intact, holds every acknowledged step and only whole steps.
        let db = CheckpointDb::new(&path);
        let (assets, batches, steps) = counts(&db);
        assert_eq!(integrity(&db), "ok", "round {round}");
        assert!(
            assets >= acknowledged,
            "round {round}: {assets} rows, {acknowledged} acknowledged"
        );
        assert_eq!(
            (assets, batches),
            (steps, steps),
            "round {round}: a partial transaction"
        );
    }
    assert!(acknowledged > 0);
}
