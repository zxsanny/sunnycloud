//! AZ-3709 AC-2 to AC-6 (IT-02-10): statement order with the stamp last, the account lock held while `on_begin`
//! runs, replay, a batch-effect failure and an all-`NoChange` call.

use std::sync::Arc;

use contracts::{
    EffectError, EntryFailure, EntryResult, PublishOutcome, PublishResult, StoredResponse,
    TreeCommit, TreeCommitError,
};
use sc_error::ErrorCode;
use sqlx::Connection;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::mutation_support::{Tree, close};
use crate::publish_support::{
    Call, OnBatch, OnBegin, TestEffect, actor, effects, existing, new_file,
};
use crate::statements::StatementLog;

const TRY_ACCOUNT_LOCK: &str =
    "SELECT pg_try_advisory_xact_lock(hashtextextended('sunnycloud.account:' || $1::text, 0))";

fn position(log: &[String], what: &str, needle: &str) -> usize {
    log.iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no {what} in {log:#?}"))
}

#[tokio::test]
async fn a_batch_runs_guard_lock_begin_savepoints_batch_and_the_stamp_last_then_commit_then_notify()
{
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(2).await;
    let entries = vec![
        new_file(tree.root, "a.txt", blobs[0]),
        new_file(tree.root, "bad/name", blobs[1]),
        new_file(tree.root, "b.txt", blobs[1]),
    ];
    let effect = TestEffect::recording();
    let log = StatementLog::install();

    // Act
    let result = tree
        .service
        .publish(
            actor(tree.account),
            entries,
            effects(&effect),
            sc_testdb::START,
        )
        .await
        .expect("publish");
    let statements: Vec<String> = log
        .take()
        .iter()
        .map(|line| line.trim().to_owned())
        .collect();

    // Assert
    let guard = position(&statements, "guard", "pg_current_xact_id_if_assigned");
    let lock = position(&statements, "account lock", "pg_advisory_xact_lock");
    let begin = position(&statements, "on_begin", "effect:on_begin");
    let batch = position(&statements, "on_batch", "effect:on_batch");
    let stamp = position(&statements, "stamp", "UPDATE account_feeds");
    let savepoints: Vec<usize> = (0..statements.len())
        .filter(|&index| statements[index].contains("SAVEPOINT"))
        .collect();
    let opened = statements
        .iter()
        .filter(|line| line.starts_with("SAVEPOINT"))
        .count();
    assert_eq!(opened, 3, "one savepoint per entry: {statements:#?}");
    assert!(
        statements[0].contains("set_config(") && guard == 1,
        "the guard follows the scope of sc_db: {statements:#?}"
    );
    assert!(guard < lock && lock < begin, "{statements:#?}");
    assert!(
        begin < savepoints[0] && savepoints[savepoints.len() - 1] < batch,
        "{statements:#?}"
    );
    assert!(batch < stamp, "{statements:#?}");
    assert_eq!(
        &statements[stamp + 1..],
        ["COMMIT", "notify"],
        "0 statements between the stamp and COMMIT; notify after COMMIT"
    );
    assert_eq!(
        tree.guard.calls(),
        [true],
        "the guard is the first statement"
    );
    let Call::Batch(outcomes) = &effect.calls()[3] else {
        panic!("on_batch is the fourth call: {:?}", effect.calls());
    };
    assert_eq!(outcomes.len(), 3, "every outcome, the failed one included");
    assert!(matches!(
        outcomes[1].result,
        EntryResult::Failed(EntryFailure::NameInvalid(_))
    ));
    assert!(matches!(result, PublishResult::Published(results) if results.len() == 3));
    tree.finish().await;
}

#[tokio::test]
async fn the_account_lock_is_held_while_on_begin_runs() {
    // Arrange
    let tree = Tree::new().await;
    let blob = tree.blobs(1).await[0];
    let (reached, resume) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let effect = TestEffect::new(
        OnBegin::Pause {
            reached: reached.clone(),
            resume: resume.clone(),
        },
        OnBatch::Proceed,
    );
    let mut other = tree.fixture.owner_conn().await;

    // Act
    let (published, locked) = tokio::join!(
        tree.service.publish(
            actor(tree.account),
            vec![new_file(tree.root, "a.txt", blob)],
            effects(&effect),
            sc_testdb::START,
        ),
        async {
            reached.notified().await;
            let mut tx = other.begin().await.expect("begin the second connection");
            let acquired: bool = sqlx::query_scalar(TRY_ACCOUNT_LOCK)
                .bind(tree.account)
                .fetch_one(&mut *tx)
                .await
                .expect("try the account lock");
            tx.rollback().await.expect("rollback the second connection");
            resume.notify_one();
            acquired
        }
    );

    // Assert
    assert!(!locked, "the try returns false while on_begin runs");
    assert!(published.is_ok());
    close(other).await;
    tree.finish().await;
}

#[tokio::test]
async fn a_replay_from_on_begin_returns_the_stored_response_and_applies_nothing() {
    // Arrange
    let tree = Tree::new().await;
    let blob = tree.blobs(1).await[0];
    let mut entry = new_file(tree.root, "a.txt", blob);
    entry.revision.reserved_bytes = 10;
    tree.set_reserved(10).await;
    let stored = StoredResponse(b"stored batch result".to_vec());
    let effect = TestEffect::new(OnBegin::Replay(stored.clone()), OnBatch::Proceed);
    let nodes = tree.nodes().await;

    // Act
    let result = tree
        .service
        .publish(
            actor(tree.account),
            vec![entry],
            effects(&effect),
            sc_testdb::START,
        )
        .await
        .expect("publish");

    // Assert
    assert_eq!(result, PublishResult::Replayed(stored));
    assert_eq!(tree.nodes().await, nodes, "no node write");
    assert_eq!(tree.revisions().await, 0);
    assert_eq!(tree.seq().await, 0, "no stamp");
    assert_eq!(tree.quota().await, (0, 10), "no quota call");
    assert_eq!(effect.calls(), [Call::Begin], "nothing else runs");
    tree.finish().await;
}

#[tokio::test]
async fn a_batch_effect_failure_rolls_everything_back_and_consumes_no_seq() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(3).await;
    let mut entry = new_file(tree.root, "a.txt", blobs[0]);
    entry.revision.reserved_bytes = 10;
    tree.set_reserved(10).await;
    let failing = TestEffect::new(
        OnBegin::Proceed,
        OnBatch::Fail(EffectError::Batch(ErrorCode::Internal)),
    );
    let nodes = tree.nodes().await;

    // Act
    let failed = tree
        .service
        .publish(
            actor(tree.account),
            vec![entry, new_file(tree.root, "b.txt", blobs[1])],
            effects(&failing),
            sc_testdb::START,
        )
        .await;
    let next = tree
        .service
        .publish(
            actor(tree.account),
            vec![new_file(tree.root, "c.txt", blobs[2])],
            effects(&TestEffect::recording()),
            sc_testdb::START,
        )
        .await;

    // Assert
    assert!(matches!(
        failed,
        Err(TreeCommitError::EffectFailed(ErrorCode::Internal))
    ));
    assert!(next.is_ok());
    assert_eq!(tree.nodes().await, nodes + 1, "only the next call's file");
    assert_eq!(tree.revisions().await, 1);
    assert_eq!(tree.seq().await, 1, "the failed call consumed no seq");
    assert_eq!(tree.quota().await.1, 10, "the release rolled back");
    tree.finish().await;
}

#[tokio::test]
async fn an_all_no_change_call_stamps_and_notifies_nothing_and_commits_the_effect_row() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(4).await;
    let firsts = vec![
        new_file(tree.root, "a.txt", blobs[0]),
        new_file(tree.root, "b.txt", blobs[1]),
    ];
    let created = tree
        .service
        .publish(
            actor(tree.account),
            firsts.clone(),
            effects(&TestEffect::recording()),
            sc_testdb::START,
        )
        .await
        .expect("create");
    let PublishResult::Published(created) = created else {
        panic!("unexpected replay");
    };
    let again: Vec<_> = created
        .iter()
        .zip(&firsts)
        .zip(&blobs[2..])
        .map(|((result, first), blob)| {
            let EntryResult::Published { node_id, .. } = result else {
                panic!("created: {result:?}");
            };
            let mut entry = existing(*node_id, None, *blob);
            entry.revision.content_hash = first.revision.content_hash;
            entry
        })
        .collect();
    tree.exec(
        "CREATE TABLE effect_rows (id uuid NOT NULL); GRANT SELECT, INSERT ON effect_rows TO sunnycloud",
    )
    .await;
    let row = Uuid::new_v4();
    let effect = TestEffect::new(OnBegin::Proceed, OnBatch::WriteRow(row));
    let wakes = tree.notifier.subscribe(tree.account);
    let woken = wakes.notified_seq();

    // Act
    let result = tree
        .service
        .publish(
            actor(tree.account),
            again,
            effects(&effect),
            sc_testdb::START,
        )
        .await
        .expect("publish");

    // Assert
    let PublishResult::Published(results) = result else {
        panic!("unexpected replay");
    };
    assert!(results.iter().all(|result| matches!(
        result,
        EntryResult::Published {
            outcome: PublishOutcome::NoChange { .. },
            ..
        }
    )));
    assert_eq!(tree.seq().await, 1, "no stamp");
    assert_eq!(wakes.notified_seq(), woken, "no notify");
    assert_eq!(
        tree.count(&format!(
            "SELECT count(*) FROM effect_rows WHERE id = '{row}'"
        ))
        .await,
        1,
        "the effect's row is committed"
    );
    drop(wakes);
    tree.finish().await;
}
