//! AZ-3757 AC-1 and AC-2 (IT-03-15; AC-27, AC-30): the account erase hook on the real 04 GC queue. Each
//! `erase_batch` is one short account-scoped transaction without the account advisory lock that deletes rows and
//! queues their blobs `AccountDeleted`, `Due::Now` together; no reservation is released; the progress entries go
//! after COMMIT; `remaining` counts the session rows and receipts left. Starts are refused while the account is
//! `deleting` (the guard is the boundary-backed `StateGuard`; `real_guard` repeats it with the real 01 guard, AZ-3760).

use std::sync::Arc;

use contracts::{AccountLifecycleHook, StartUploadOutcome, UploadError};
use sc_observability::{BuildInfo, Metrics};
use sc_testdb::statements::StatementLog;
use sunnycloud_transfer::TransferConfig;
use uuid::Uuid;

use crate::put_support::content;
use crate::start_support::{MIB, Rig, request, transfer_config};

const SESSIONS: usize = 30;
const STAGED: usize = 200;
const RECEIPTS: i64 = 5000;
const LIMIT: u32 = 100;

fn metrics() -> Metrics {
    Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics")
}

fn first_word_is(line: &str, word: &str) -> bool {
    line.split_whitespace()
        .next()
        .is_some_and(|first| first.eq_ignore_ascii_case(word))
}

/// The statements of one `erase_batch` call: one transaction (sqlx logs no BEGIN; `sc_db` opens it with its scope
/// `set_config`) that ends in its only COMMIT, without an advisory lock.
fn one_short_transaction(log: &[String]) -> bool {
    let commits = log
        .iter()
        .filter(|line| first_word_is(line, "COMMIT"))
        .count();
    let opened = log.first().is_some_and(|line| line.contains("set_config("));
    let closed = log.last().is_some_and(|line| first_word_is(line, "COMMIT"));
    opened && closed && commits == 1 && !log.iter().any(|line| line.contains("pg_advisory"))
}

/// 30 open sessions with progress entries, 200 staged rows, one committed-key record and 5,000 receipts, served by
/// an instance whose gauge counts the progress entries.
async fn full_account(metrics: &Metrics) -> (Rig, Uuid) {
    let mut rig = Rig::with_config(TransferConfig {
        max_open_sessions: 32,
        ..transfer_config()
    })
    .await;
    rig.service = Arc::new(rig.instance().with_metrics(metrics).expect("upload series"));
    let revision = rig
        .commit_bytes("kept.bin", "kept", content(1, MIB as usize), None)
        .await
        .expect("committed")
        .revision_id;
    for seed in 0..SESSIONS {
        rig.upload(MIB as usize, 500 + seed as u64).await;
    }
    for index in 0..STAGED {
        let data = content(700 + index as u64, 100 + index);
        rig.stage(Uuid::new_v4(), &data).await.expect("staged");
    }
    rig.fill_receipts(RECEIPTS).await;
    let blob = rig.revision_row(revision).await.blob_id;
    (rig, blob)
}

#[tokio::test]
async fn erase_batch_deletes_rows_with_their_blobs_queued_in_short_transactions_until_nothing_remains()
 {
    // Arrange
    let metrics = metrics();
    let (rig, revision_blob) = full_account(&metrics).await;
    let open_blobs = rig.open_session_blobs().await;
    let before = metrics.render().await.expect("render");
    let (reserved, releases, files) = (rig.reserved().await, rig.ledger.releases(), rig.files());
    rig.begin_deletion().await;
    let hook = rig.lifecycle();
    let total = (SESSIONS + STAGED + 1) as u64 + RECEIPTS as u64;
    let log = StatementLog::install();

    // Act
    let mut remaining = Vec::new();
    let mut transactions_short = true;
    let mut unqueued = 0;
    loop {
        log.take();
        let progress = hook
            .erase_batch(rig.account, LIMIT)
            .await
            .expect("erase batch");
        transactions_short &= one_short_transaction(&log.take());
        unqueued += rig.unqueued_blobs().await;
        remaining.push(progress.remaining);
        if progress.remaining == 0 {
            break;
        }
    }
    let again = hook.erase_batch(rig.account, LIMIT).await.expect("again");
    let after = metrics.render().await.expect("render");

    // Assert
    let expected: Vec<u64> = (1..=total.div_ceil(u64::from(LIMIT)))
        .map(|batch| total.saturating_sub(batch * u64::from(LIMIT)))
        .collect();
    assert_eq!(remaining, expected);
    assert_eq!(again.remaining, 0, "an erased account reports 0 again");
    assert!(
        transactions_short,
        "one transaction per call, no advisory lock"
    );
    assert_eq!(unqueued, 0, "no deleted row without its blob_gc row");
    assert_eq!(open_blobs.len(), SESSIONS + STAGED);
    assert_eq!(rig.queued_now().await, open_blobs.len() as i64);
    for blob in &open_blobs {
        assert_eq!(rig.queued(*blob).await, (1, Some("gc_pending".to_owned())));
    }
    assert_eq!(
        rig.queued(revision_blob).await,
        (0, Some("committed".to_owned())),
        "a committed-key record's blob is its revision's"
    );
    assert_eq!((rig.sessions().await, rig.account_receipts().await), (0, 0));
    assert_eq!(
        (rig.ledger.releases(), rig.reserved().await),
        (releases, reserved),
        "no release_reservation: the reservation goes with 07a's row"
    );
    assert_eq!(rig.files(), files, "files are 04's sweeper's");
    assert!(
        before.contains(&format!("upload_sessions_active {SESSIONS}")),
        "{before}"
    );
    assert!(after.contains("upload_sessions_active 0"), "{after}");
    drop(hook);
    rig.finish().await;
}

#[tokio::test]
async fn start_upload_while_the_account_is_deleting_is_refused() {
    // Arrange
    let rig = Rig::new().await;
    rig.upload(MIB as usize, 801).await;
    rig.begin_deletion().await;

    // Act
    let result = rig
        .start(request(rig.root, "late.bin", 4 * MIB, "late"))
        .await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::AccountNotActive)),
        "{result:?}"
    );
    assert_eq!(rig.sessions().await, 1, "no new session row");
    rig.finish().await;
}

#[tokio::test]
async fn erase_batch_of_an_account_without_transfer_rows_reports_nothing_left() {
    // Arrange
    let rig = Rig::new().await;
    let hook = rig.lifecycle();
    let other = Uuid::new_v4();

    // Act
    let own = hook.erase_batch(rig.account, LIMIT).await.expect("own");
    let unknown = hook.erase_batch(other, LIMIT).await.expect("unknown");

    // Assert
    assert_eq!((own.remaining, unknown.remaining), (0, 0));
    drop(hook);
    rig.finish().await;
}

#[tokio::test]
async fn erase_batch_leaves_another_accounts_sessions_alone() {
    // Arrange
    let rig = Rig::new().await;
    let (other, other_root) = rig.other_account().await;
    let service = Arc::clone(&rig.service);
    let started = service
        .start_upload(other, request(other_root, "theirs.bin", 4 * MIB, "theirs"))
        .await;
    rig.upload(MIB as usize, 802).await;
    let hook = rig.lifecycle();

    // Act
    let own = hook.erase_batch(rig.account, LIMIT).await.expect("own");

    // Assert
    assert!(
        matches!(started, Ok(StartUploadOutcome::Started(_))),
        "{started:?}"
    );
    assert_eq!(own.remaining, 0);
    assert_eq!(
        rig.staged_rows(other).await,
        1,
        "the other account keeps its row"
    );
    drop((hook, service));
    rig.finish().await;
}
