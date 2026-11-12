//! AC-1 "via 02": the stamp-last and notify-after-COMMIT obligations of `feed-writer-ports.md` against the real
//! `publish`. The holds run on a manual clock that is never ticked, so a hold can end only by a notify (or the
//! rig's shutdown). `#[tokio::test]` keeps the publish and the hold on one thread, so the hold's wake read lands
//! in the same statement log as the publish, in the order it ran.
//!
//! A publish is rolled back after its stamp at COMMIT: a deferred constraint trigger, installed on `nodes` by the
//! superuser for this test's database, refuses a file named `doomed.txt` when the transaction commits. No
//! component is replaced; the database refuses the commit.

use std::sync::Arc;
use std::time::Duration;

use contracts::TreeCommitError;
use sc_clock::ManualClock;
use sc_testdb::START;
use sunnycloud_sync_feed::READ_PAGE_SQL;

use crate::statements::StatementLog;
use crate::support::{Rig, cursor, new_file, published, settle};

const WAIT: Duration = Duration::from_secs(30);
const STAMP: &str = "UPDATE account_feeds";

const REFUSE_AT_COMMIT: &str = "\
CREATE FUNCTION refuse_at_commit() RETURNS trigger LANGUAGE plpgsql AS $$ \
BEGIN RAISE EXCEPTION 'probe: commit refused for %', NEW.name; END $$; \
CREATE CONSTRAINT TRIGGER refuse_doomed AFTER INSERT ON nodes DEFERRABLE INITIALLY DEFERRED \
FOR EACH ROW WHEN (NEW.name = 'doomed.txt') EXECUTE FUNCTION refuse_at_commit()";

async fn rig() -> Rig {
    Rig::new(Arc::new(ManualClock::new(START)), 500).await
}

fn position(log: &[String], what: &str, found: impl Fn(&str) -> bool) -> usize {
    log.iter()
        .position(|line| found(line))
        .unwrap_or_else(|| panic!("no {what} in {log:#?}"))
}

fn is_feed_read(line: &str) -> bool {
    line == READ_PAGE_SQL.trim()
}

fn trimmed(log: Vec<String>) -> Vec<String> {
    log.into_iter().map(|line| line.trim().to_owned()).collect()
}

#[tokio::test]
async fn a_publish_stamps_last_then_commits_then_notifies_and_the_held_reader_wakes_only_after_that()
 {
    // Arrange
    let rig = rig().await;
    let blob = rig.blobs(1).await[0];
    let held = rig.hold(cursor(0), WAIT);
    rig.clock.sleeps_reach(1).await;
    let log = StatementLog::install();

    // Act
    let result = rig
        .publish("device-a", new_file(rig.root, "a.txt", blob))
        .await
        .expect("publish");
    let parked_until_publish_returned = !held.is_finished();
    let (page, _) = held.await.expect("hold task");
    let statements = trimmed(log.take());

    // Assert
    let (node, revision) = published(&result);
    let stamp = position(&statements, "stamp", |line| line.contains(STAMP));
    let notify = position(&statements, "notify", |line| line == "notify");
    let wake_read = position(&statements, "wake read", is_feed_read);
    assert_eq!(
        statements
            .iter()
            .filter(|line| line.contains(STAMP))
            .count(),
        1,
        "one stamp: {statements:#?}"
    );
    assert_eq!(
        &statements[stamp + 1..=notify],
        ["COMMIT", "notify"],
        "0 statements between the stamp and COMMIT; notify after COMMIT"
    );
    assert!(
        !statements[..notify].iter().any(|line| is_feed_read(line)),
        "the held reader read nothing before the notify: {statements:#?}"
    );
    assert!(notify < wake_read, "the wake read follows the notify");
    assert!(parked_until_publish_returned);
    let page = page.expect("page");
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        (
            page.items[0].node_id,
            page.items[0].revision_id,
            page.items[0].seq
        ),
        (node, Some(revision), 1)
    );
    assert_eq!(page.next_cursor, cursor(1));
    assert_eq!(rig.notifies().len(), 1);
    rig.finish().await;
}

#[tokio::test]
async fn a_publish_rolled_back_at_commit_after_its_stamp_leaves_no_gap_and_wakes_nobody() {
    // Arrange
    let rig = rig().await;
    rig.db.admin_exec(REFUSE_AT_COMMIT).await;
    let blobs = rig.blobs(3).await;
    rig.publish("device-a", new_file(rig.root, "a.txt", blobs[0]))
        .await
        .expect("the first publish commits");
    let held = rig.hold(cursor(1), WAIT);
    rig.clock.sleeps_reach(1).await;
    let log = StatementLog::install();

    // Act
    let doomed = rig
        .publish("device-a", new_file(rig.root, "doomed.txt", blobs[1]))
        .await;
    let doomed_statements = trimmed(log.take());
    settle().await;
    let parked_after_rollback = !held.is_finished();
    let counter_after_rollback = rig.counter().await;
    let notifies_after_rollback = rig.notifies().len();
    let next = rig
        .publish("device-a", new_file(rig.root, "c.txt", blobs[2]))
        .await
        .expect("the next publish commits");
    let (page, _) = held.await.expect("hold task");

    // Assert
    assert!(
        matches!(doomed, Err(TreeCommitError::Unexpected(_))),
        "{doomed:?}"
    );
    let stamp = position(&doomed_statements, "stamp", |line| line.contains(STAMP));
    assert_eq!(
        doomed_statements[stamp + 1],
        "COMMIT",
        "the refused COMMIT follows the stamp: {doomed_statements:#?}"
    );
    assert!(
        !doomed_statements.iter().any(|line| line == "notify"),
        "no notify for a rolled-back publish: {doomed_statements:#?}"
    );
    assert!(parked_after_rollback, "the rolled-back publish woke nobody");
    assert_eq!(counter_after_rollback, 1, "the stamp rolled back");
    assert_eq!(
        notifies_after_rollback, 1,
        "only the first publish notified"
    );
    let (node, _) = published(&next);
    let page = page.expect("page");
    assert_eq!(
        page.items
            .iter()
            .map(|item| (item.node_id, item.seq))
            .collect::<Vec<_>>(),
        [(node, 2)],
        "the next commit takes seq 2: no gap, and the doomed file is not in the feed"
    );
    assert_eq!(
        rig.db
            .admin_count("SELECT count(*) FROM nodes WHERE name = 'doomed.txt'")
            .await,
        0
    );
    rig.finish().await;
}
