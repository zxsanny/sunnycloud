//! AZ-3760 AC-4 (IT-03-19): the E-14 rows of the commit contract-test plan (E-13/c01,
//! `contracts/02_tree/tree_commit.md`) against 02's real `publish`, 03's real effects, 08's real media effect and
//! 01's real guard: order-and-tail, replay-under-lock, entry-failure-rollback with media-job-rollback, and
//! reaper-vs-commit. The double-wired runs of the same rows stay in `stamp_order`, `batch_replay`, `commit_target`
//! and `reaper_race`.

use std::sync::Arc;

use contracts::{BatchEntryResult, CommitServiceError, EntryError, Janitor};
use sc_testdb::statements::StatementLog;
use sunnycloud_transfer::SessionReaper;
use uuid::Uuid;

use crate::batch_support::{StagedFile, batch, entry};
use crate::commit_support::{commit_request, file_hash};
use crate::real_support::{Real, jpeg};
use crate::stamp_order::{is_effect, position, publish_transaction, trimmed};

/// Kept on the account through the race: a second release would show as reserved below it.
const WITNESS: i64 = 1_000_000;
const ROUNDS: usize = 100;

fn guard_line(line: &str) -> bool {
    line.contains("FROM accounts") && line.contains("FOR SHARE")
}

/// Every effect statement of 03 and 08 in the `publish` transaction comes before the stamp.
fn effects_before_stamp(tx: &[String], stamp: usize) -> bool {
    (0..tx.len())
        .filter(|&i| is_effect(&tx[i]) || tx[i].contains("jobs"))
        .all(|i| i < stamp)
}

async fn staged_jpegs(real: &Real, names: &[&str]) -> Vec<StagedFile> {
    let mut files = Vec::new();
    for name in names {
        let file = StagedFile {
            key: Uuid::new_v4(),
            name: format!("{name}.jpg"),
            data: jpeg(name),
        };
        real.rig.stage(file.key, &file.data).await.expect("staged");
        files.push(file);
    }
    files
}

#[tokio::test]
async fn order_and_tail_the_guard_effects_and_media_job_run_before_the_stamp_and_only_commit_after_it()
 {
    // Arrange
    let real = Real::new().await;
    let rig = &real.rig;
    let upload = rig
        .upload_bytes("single.jpg", "single", jpeg("single"))
        .await;
    rig.send_all(&upload).await;
    let files = staged_jpegs(&real, &["b0", "b1"]).await;
    let entries = files.iter().map(|file| entry(file, rig.root)).collect();
    let log = StatementLog::install();

    // Act
    rig.commit(commit_request(&upload, rig.root, "single.jpg", None))
        .await
        .expect("committed");
    let single = trimmed(&log);
    rig.commit_batch(batch(Uuid::new_v4(), entries))
        .await
        .expect("batch committed");
    let batched = trimmed(&log);

    // Assert
    let (tx, stamp) = publish_transaction(&single);
    let guard = position(tx, "guard", guard_line);
    let lock = position(tx, "account lock", |line| {
        line.contains("pg_advisory_xact_lock")
    });
    let session = position(tx, "session lock", |line| {
        line.contains("FROM upload_sessions") && line.contains("FOR UPDATE")
    });
    let job = position(tx, "job insert", |line| {
        line.starts_with("INSERT INTO jobs")
    });
    assert!(
        guard < lock && lock < session && session < job && job < stamp,
        "{tx:#?}"
    );
    assert!(effects_before_stamp(tx, stamp), "{tx:#?}");
    assert_eq!(tx.len() - stamp, 2, "only COMMIT after the stamp: {tx:#?}");
    let (tx, stamp) = publish_transaction(&batched);
    let guard = position(tx, "guard", guard_line);
    let lookup = position(tx, "receipt lookup", |line| {
        line.starts_with("SELECT") && line.contains("FROM commit_receipts")
    });
    let job = position(tx, "job insert", |line| {
        line.starts_with("INSERT INTO jobs")
    });
    let receipt = position(tx, "receipt insert", |line| {
        line.starts_with("INSERT INTO commit_receipts")
    });
    assert!(
        guard < lookup && lookup < job && job < receipt && receipt < stamp,
        "{tx:#?}"
    );
    assert!(effects_before_stamp(tx, stamp), "{tx:#?}");
    assert_eq!(tx.len() - stamp, 2, "only COMMIT after the stamp: {tx:#?}");
    assert_eq!(real.jobs().await, 3);
    real.finish().await;
}

#[tokio::test]
async fn replay_under_lock_two_identical_batches_at_once_apply_and_enqueue_exactly_once() {
    // Arrange
    let real = Real::new().await;
    let rig = &real.rig;
    let files = staged_jpegs(&real, &["c0", "c1", "c2", "c3"]).await;
    let request = batch(
        Uuid::new_v4(),
        files.iter().map(|file| entry(file, rig.root)).collect(),
    );
    let seq = real.seq().await;

    // Act
    let (one, two) = tokio::join!(rig.commit_batch(request.clone()), rig.commit_batch(request));

    // Assert
    let (one, two) = (one.expect("first answer"), two.expect("second answer"));
    assert_eq!(one, two);
    assert!(
        one.per_entry
            .iter()
            .all(|entry| matches!(entry, BatchEntryResult::Committed(_))),
        "{one:?}"
    );
    assert_eq!(rig.revisions().await, 4);
    assert_eq!(real.jobs().await, 4, "one job per image, enqueued once");
    assert_eq!(rig.receipts().await, 1);
    assert_eq!(real.seq().await, seq + 1, "one application");
    real.finish().await;
}

#[tokio::test]
async fn entry_failure_rollback_a_staged_row_gone_before_publish_fails_alone_with_no_job() {
    // Arrange
    let real = Real::new().await;
    let rig = &real.rig;
    let files = staged_jpegs(&real, &["d0", "d1", "d2"]).await;
    let request = batch(
        Uuid::new_v4(),
        files.iter().map(|file| entry(file, rig.root)).collect(),
    );
    let seq = real.seq().await;
    let (reached, resume) = rig.commit.pause_next();
    let service = Arc::clone(&rig.service);
    let actor = rig.actor();
    let held = tokio::spawn(async move {
        let service: &dyn contracts::CommitService = service.as_ref();
        service.commit_batch(actor, request).await
    });
    reached.await.expect("the batch passed its checks");
    rig.exec(&format!(
        "DELETE FROM upload_sessions WHERE account_id = '{}' AND client_key = '{}'",
        rig.account, files[1].key
    ))
    .await;

    // Act
    resume.send(()).expect("the batch waits");
    let result = held.await.expect("task").expect("the batch commits");

    // Assert
    assert!(
        matches!(result.per_entry[0], BatchEntryResult::Committed(_)),
        "{result:?}"
    );
    assert_eq!(
        result.per_entry[1],
        BatchEntryResult::Failed(EntryError::UploadNotFound)
    );
    assert!(
        matches!(result.per_entry[2], BatchEntryResult::Committed(_)),
        "{result:?}"
    );
    assert_eq!(rig.revisions().await, 2);
    assert_eq!(
        rig.reserved().await,
        files[1].data.len() as i64,
        "the failed entry's release rolled back with its savepoint"
    );
    assert_eq!(real.jobs_of(&file_hash(&files[0].data)).await, 1);
    assert_eq!(
        real.jobs_of(&file_hash(&files[1].data)).await,
        0,
        "media-job-rollback"
    );
    assert_eq!(real.jobs_of(&file_hash(&files[2].data)).await, 1);
    assert_eq!(real.seq().await, seq + 1, "one seq");
    real.finish().await;
}

#[tokio::test]
async fn reaper_vs_commit_has_one_winner_each_round_and_a_job_only_when_the_commit_wins() {
    // Arrange
    let real = Real::new().await;
    let rig = &real.rig;
    let reaper = Arc::new(SessionReaper::new(Arc::clone(&rig.service)));
    rig.exec(&format!(
        "UPDATE quota SET reserved_bytes = reserved_bytes + {WITNESS} WHERE account_id = '{}'",
        rig.account
    ))
    .await;
    let mut wins = (0usize, 0usize);

    for round in 0..ROUNDS {
        let name = format!("race-{round}.jpg");
        let data = jpeg(&name);
        let upload = rig.upload_bytes(&name, &name, data.clone()).await;
        rig.send_all(&upload).await;
        let expiry = rig
            .session_record(upload.id)
            .await
            .expect("open session")
            .expires_at;
        let (service, actor) = (Arc::clone(&rig.service), rig.actor());
        let request = commit_request(&upload, rig.root, &name, None);

        // Act
        let commit = tokio::spawn(async move { service.commit_upload(actor, request).await });
        let reap = {
            let reaper = Arc::clone(&reaper);
            tokio::spawn(async move { reaper.run_cycle(expiry).await })
        };
        let committed = commit.await.expect("commit task");
        reap.await.expect("reap task").expect("cycle");

        // Assert
        let jobs = real.jobs_of(&file_hash(&data)).await;
        match committed {
            Ok(_) => {
                wins.0 += 1;
                assert_eq!(jobs, 1, "round {round}");
            }
            Err(CommitServiceError::UploadNotFound) => {
                wins.1 += 1;
                assert_eq!(jobs, 0, "round {round}");
            }
            other => panic!("round {round}: {other:?}"),
        }
        assert_eq!(
            rig.reserved().await,
            WITNESS,
            "round {round}: released once"
        );
    }
    assert_eq!(rig.revisions().await, wins.0 as i64);
    assert_eq!(real.jobs().await, wins.0 as i64);
    eprintln!("commit won {} rounds, reaper won {}", wins.0, wins.1);
    drop(reaper);
    real.finish().await;
}
