//! AC-5: an asset is `uploaded` only after the commit acknowledgement was stored; a lost write is healed by the
//! idempotent upload.

use mobile_backup::{BackupOutcome, RunError, Trigger};

use crate::support::Rig;

const BLOCK: usize = 2 * 1024 * 1024;

#[tokio::test]
async fn a_block_path_upload_whose_row_write_failed_after_the_ack_ends_with_nothing_to_send_on_the_next_run()
 {
    // Arrange
    let mut rig = Rig::new(8, 50);
    rig.source.add(1, BLOCK + 1);
    let db = rig.db.clone();
    rig.api.on_ack(move |_| db.fail_asset_writes());

    // Act
    let failed = rig.try_run(Trigger::MediaChange).await;
    let held = rig.db.asset(1).expect("row");
    let key = held.client_key.expect("a stored key");
    rig.db.heal_asset_writes();
    rig.restart();
    let outcome = rig.run(Trigger::AppOpen).await;

    // Assert
    assert!(matches!(failed, Err(RunError::Checkpoint(_))), "{failed:?}");
    assert_eq!(
        held.state, "uploading",
        "not uploaded before its row was written"
    );
    assert_eq!(outcome, BackupOutcome::Completed);
    let record = rig.api.record();
    assert_eq!(record.large.len(), 1);
    assert_eq!(record.large[0].key, key, "the same client_key");
    assert_eq!(
        record.large[0].bytes_sent, 0,
        "the server answered NothingToSend"
    );
    let row = rig.db.asset(1).expect("row");
    assert_eq!(row.state, "uploaded");
    assert!(row.revision_id.is_some() && row.node_id.is_none());
    assert_eq!(rig.server.file_count(), 1);
    assert_eq!(rig.server.state().nothing_to_send, 1);
}

#[tokio::test]
async fn a_batch_whose_results_were_not_stored_is_replayed_with_its_commit_id_and_makes_no_second_file()
 {
    // Arrange
    let mut rig = Rig::new(8, 50);
    rig.source.add(1, 4096);
    let db = rig.db.clone();
    rig.api.on_ack(move |_| db.fail_asset_writes());

    // Act
    let failed = rig.try_run(Trigger::MediaChange).await;
    let commit_id = rig.api.record().commits[0].commit_id;
    let held = (rig.db.asset(1).expect("row").state, rig.db.batch_ids());
    rig.db.heal_asset_writes();
    rig.restart();
    let outcome = rig.run(Trigger::AppOpen).await;

    // Assert
    assert!(matches!(failed, Err(RunError::Store(_))), "{failed:?}");
    assert_eq!(
        held,
        ("uploading".to_owned(), vec![commit_id]),
        "the batch record is kept"
    );
    assert_eq!(outcome, BackupOutcome::Completed);
    let record = rig.api.record();
    assert_eq!(record.resumed, vec![commit_id]);
    assert!(record.commits.is_empty(), "no new batch was staged");
    assert_eq!(rig.db.asset(1).expect("row").state, "uploaded");
    assert!(rig.db.batch_ids().is_empty());
    assert_eq!(rig.server.file_count(), 1);
}
