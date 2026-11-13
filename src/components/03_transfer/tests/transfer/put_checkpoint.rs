//! `put_block` AC-7: progress reaches the session row every `checkpoint_bytes` (8 MiB) or `checkpoint_interval`
//! (1 s), each checkpoint rolling `expires_at`. The checkpoint transaction also checks the account (owner
//! decision 2026-11-13) and never writes a committed or vanished row.

use std::time::Duration;

use contracts::UploadError;
use sc_ids::FileHash;

use crate::put_support::{Upload, bit};
use crate::start_support::{Rig, TTL, file, now};

const SIXTEEN_MIB: usize = 16 << 20;
const QUICK: Duration = Duration::from_millis(100);

/// `expires_at` a checkpoint written now would set, in Unix microseconds.
fn expiry_now(rig: &Rig) -> i64 {
    now(&rig.clock)
        .checked_add(TTL)
        .expect("expiry")
        .as_unix_micros()
}

fn hashes_of(upload: &Upload, blocks: u32) -> Vec<u8> {
    let mut expected = vec![0u8; upload.block_count() as usize * 32];
    for index in 0..blocks {
        let slot = index as usize * 32;
        expected[slot..slot + 32].copy_from_slice(upload.hash(index).as_bytes());
    }
    expected
}

#[tokio::test]
async fn put_block_checkpoints_after_eight_mib_and_after_the_interval() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(SIXTEEN_MIB, 50).await;
    let (_, _, started_expiry) = rig.row(upload.id).await;

    // Act
    for index in 0..3 {
        rig.clock.tick(QUICK);
        rig.put(&upload, index).await.expect("quick block");
    }
    let after_three = rig.row(upload.id).await;
    rig.clock.tick(QUICK);
    rig.put(&upload, 3).await.expect("fourth block");
    let fourth_expiry = expiry_now(&rig);
    let after_four = rig.row(upload.id).await;
    rig.clock.tick(QUICK);
    rig.put(&upload, 4).await.expect("fifth block");
    let after_five = rig.row(upload.id).await;
    rig.clock.tick(Duration::from_secs(1));
    rig.put(&upload, 5).await.expect("block after the pause");
    let after_pause = rig.row(upload.id).await;

    // Assert
    assert_eq!(after_three.0, [0x00], "6 MiB: no checkpoint yet");
    assert_eq!(after_three.2, started_expiry);
    assert_eq!(after_four.0, [0x0F], "8 MiB: checkpoint");
    assert_eq!(after_four.1, hashes_of(&upload, 4));
    assert_eq!(after_four.2, fourth_expiry);
    assert!(after_four.2 > started_expiry, "expires_at moves forward");
    assert_eq!(
        after_five, after_four,
        "2 MiB and 100 ms later: no checkpoint"
    );
    assert_eq!(after_pause.0, [0x3F], "1 s later: checkpoint");
    assert_eq!(after_pause.1, hashes_of(&upload, 6));
    assert_eq!(after_pause.2, expiry_now(&rig));
    assert!(after_pause.2 > after_four.2);
    rig.finish().await;
}

#[tokio::test]
async fn put_block_a_deleting_account_is_refused_at_the_next_checkpoint() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(SIXTEEN_MIB, 51).await;
    for index in 0..3 {
        rig.put(&upload, index)
            .await
            .expect("block before the deletion");
    }
    rig.exec(&format!(
        "UPDATE accounts SET state = 'deleting' WHERE id = '{}'",
        rig.account
    ))
    .await;

    // Act
    let at_checkpoint = rig.put(&upload, 3).await;
    let next = rig.put(&upload, 4).await;
    let (received, _, _) = rig.row(upload.id).await;

    // Assert
    assert!(
        matches!(at_checkpoint, Err(UploadError::AccountNotActive)),
        "{at_checkpoint:?}"
    );
    assert!(
        matches!(next, Err(UploadError::AccountNotActive)),
        "{next:?}"
    );
    assert!(
        !bit(&received, 0),
        "nothing is checkpointed for a deleting account"
    );
    rig.finish().await;
}

#[tokio::test]
async fn put_block_a_vanished_session_is_not_found_at_the_checkpoint_and_after() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(SIXTEEN_MIB, 52).await;
    for index in 0..3 {
        rig.put(&upload, index)
            .await
            .expect("block before the abort");
    }
    rig.exec(&format!(
        "DELETE FROM upload_sessions WHERE id = '{}' AND account_id = '{}'",
        upload.id, rig.account
    ))
    .await;

    // Act
    let at_checkpoint = rig.put(&upload, 3).await;
    let writes = rig.writer.writes();
    let next = rig.put(&upload, 4).await;

    // Assert
    assert!(
        matches!(at_checkpoint, Err(UploadError::UploadNotFound)),
        "{at_checkpoint:?}"
    );
    assert!(matches!(next, Err(UploadError::UploadNotFound)), "{next:?}");
    assert_eq!(
        rig.writer.writes(),
        writes,
        "the dropped session writes nothing more"
    );
    rig.finish().await;
}

#[tokio::test]
async fn put_block_never_writes_a_committed_row() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(SIXTEEN_MIB, 53).await;
    let mut conn = rig.owner().await;
    let (_, revision) = file(
        &mut conn,
        rig.account,
        rig.root,
        "committed.bin",
        &FileHash::from_bytes([5; 32]),
    )
    .await;
    crate::start_support::close(conn).await;
    for index in 0..3 {
        rig.put(&upload, index)
            .await
            .expect("block before the commit");
    }
    rig.exec(&format!(
        "UPDATE upload_sessions SET committed_revision_id = '{revision}', reserved_bytes = 0 \
         WHERE id = '{}' AND account_id = '{}'",
        upload.id, rig.account
    ))
    .await;
    let before = rig.row(upload.id).await;

    // Act
    let at_checkpoint = rig.put(&upload, 3).await;

    // Assert
    assert!(
        matches!(at_checkpoint, Err(UploadError::UploadNotFound)),
        "{at_checkpoint:?}"
    );
    assert_eq!(
        rig.row(upload.id).await,
        before,
        "the committed row is unchanged"
    );
    rig.finish().await;
}
