//! AZ-3757 AC-69 (IT-03-19, NFT-RES-35): 03's effects add nothing after 02's stamp. With the shared statement log
//! and the reader and writer decorators marking each call (`blob_reader`, `blob_writer`), a single commit and a
//! batch commit on the real 02 and 05 run every effect statement (the `on_begin` receipt lookup, the `on_entry`
//! session lock, blob flip and row change, the `on_batch` receipt insert) before the stamp, nothing between the
//! stamp and COMMIT, and no blob I/O between the transaction's start and COMMIT.

use sc_testdb::statements::StatementLog;
use uuid::Uuid;

use crate::batch_support::{batch, entry};
use crate::commit_support::commit_request;
use crate::put_support::content;
use crate::start_support::{MIB, Rig};

const STAMP: &str = "UPDATE account_feeds SET seq";

fn first_word_is(line: &str, word: &str) -> bool {
    line.split_whitespace()
        .next()
        .is_some_and(|first| first.eq_ignore_ascii_case(word))
}

/// The `publish` transaction: from its start (sqlx logs no BEGIN; `sc_db` opens the transaction with its scope
/// `set_config`) before the stamp to the COMMIT after it; returns the statements in it and the stamp's index among
/// them.
pub fn publish_transaction(log: &[String]) -> (&[String], usize) {
    let stamp = log
        .iter()
        .rposition(|line| line.contains(STAMP))
        .unwrap_or_else(|| panic!("no stamp in {log:#?}"));
    let begin = log[..stamp]
        .iter()
        .rposition(|line| line.contains("set_config("))
        .unwrap_or_else(|| panic!("no transaction start before the stamp in {log:#?}"));
    let commit = log[stamp..]
        .iter()
        .position(|line| first_word_is(line, "COMMIT"))
        .map(|index| index + stamp)
        .unwrap_or_else(|| panic!("no COMMIT after the stamp in {log:#?}"));
    (&log[begin..=commit], stamp - begin)
}

pub fn position(tx: &[String], what: &str, test: impl Fn(&str) -> bool) -> usize {
    tx.iter()
        .position(|line| test(line))
        .unwrap_or_else(|| panic!("no {what} in {tx:#?}"))
}

fn is_blob_io(line: &str) -> bool {
    line == "blob_reader" || line == "blob_writer"
}

pub fn is_effect(line: &str) -> bool {
    line.contains("upload_sessions") || line.contains("commit_receipts")
}

pub fn trimmed(log: &StatementLog) -> Vec<String> {
    log.take()
        .iter()
        .map(|line| line.trim().to_owned())
        .collect()
}

#[tokio::test]
async fn commit_upload_runs_the_blob_effect_before_the_stamp_and_no_blob_io_in_the_transaction() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig
        .upload_bytes("single.bin", "single", content(1101, 3 * MIB as usize))
        .await;
    rig.send_all(&upload).await;
    let log = StatementLog::install();

    // Act
    rig.commit(commit_request(&upload, rig.root, "single.bin", None))
        .await
        .expect("committed");
    let statements = trimmed(&log);

    // Assert
    let (tx, stamp) = publish_transaction(&statements);
    let lock = position(tx, "session lock", |line| {
        line.contains("FROM upload_sessions") && line.contains("FOR UPDATE")
    });
    let flip = position(tx, "blob flip", |line| line.starts_with("UPDATE blobs"));
    let record = position(tx, "committed-key record", |line| {
        line.starts_with("UPDATE upload_sessions")
    });
    assert!(lock < flip && flip < record && record < stamp, "{tx:#?}");
    assert!(
        (0..tx.len())
            .filter(|&i| is_effect(&tx[i]))
            .all(|i| i < stamp),
        "{tx:#?}"
    );
    assert_eq!(tx.len() - stamp, 2, "only COMMIT after the stamp: {tx:#?}");
    assert!(!tx.iter().any(|line| is_blob_io(line)), "{tx:#?}");
    assert!(
        statements.iter().any(|line| line == "blob_reader"),
        "the head read is marked, before the transaction"
    );
    rig.finish().await;
}

#[tokio::test]
async fn commit_batch_runs_the_receipt_and_staged_effects_before_the_stamp_and_no_blob_io_in_the_transaction()
 {
    // Arrange
    let rig = Rig::new().await;
    let mut entries = Vec::new();
    for index in 0..3 {
        let key = Uuid::new_v4();
        rig.stage(key, &content(1200 + index, 1000))
            .await
            .expect("staged");
        let file = crate::batch_support::StagedFile {
            key,
            name: format!("b{index}.bin"),
            data: Vec::new(),
        };
        entries.push(entry(&file, rig.root));
    }
    let log = StatementLog::install();

    // Act
    let result = rig
        .commit_batch(batch(Uuid::new_v4(), entries))
        .await
        .expect("batch committed");
    let statements = trimmed(&log);

    // Assert
    assert_eq!(result.per_entry.len(), 3);
    let (tx, stamp) = publish_transaction(&statements);
    let lookup = position(tx, "receipt lookup", |line| {
        line.starts_with("SELECT") && line.contains("FROM commit_receipts")
    });
    let lock = position(tx, "session lock", |line| {
        line.contains("FROM upload_sessions") && line.contains("FOR UPDATE")
    });
    let flip = position(tx, "blob flip", |line| line.starts_with("UPDATE blobs"));
    let delete = position(tx, "staged row delete", |line| {
        line.starts_with("DELETE FROM upload_sessions")
    });
    let insert = position(tx, "receipt insert", |line| {
        line.starts_with("INSERT INTO commit_receipts")
    });
    assert!(lookup < lock && lock < delete, "{tx:#?}");
    assert!(
        [lookup, lock, flip, delete, insert]
            .iter()
            .all(|&i| i < stamp),
        "{tx:#?}"
    );
    assert!(
        (0..tx.len())
            .filter(|&i| is_effect(&tx[i]))
            .all(|i| i < stamp),
        "{tx:#?}"
    );
    assert_eq!(tx.len() - stamp, 2, "only COMMIT after the stamp: {tx:#?}");
    assert!(!tx.iter().any(|line| is_blob_io(line)), "{tx:#?}");
    rig.finish().await;
}
