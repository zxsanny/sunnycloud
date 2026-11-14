//! AZ-3760 AC-1 (IT-03-15 refusal lines, IT-03-1's `begin_deletion` line; NFT-SEC-27): an account moved to
//! `deleting` by the real 01 `AccountAdmin::begin_deletion` is refused by the real 01 guard on every transfer entry
//! point, `start_upload`, `put_block`, `stage_small`, `commit_upload` and `commit_batch`, with nothing written. The
//! same refusals against the boundary-backed guard stay in `start_open`, `put_checkpoint`, `stage_refusals` and
//! `erase`.

use contracts::{CommitServiceError, UploadError};
use uuid::Uuid;

use crate::batch_support::{StagedFile, batch, entry};
use crate::commit_support::commit_request;
use crate::real_support::{Real, jpeg};
use crate::start_support::{MIB, request};

#[tokio::test]
async fn a_deleting_account_is_refused_by_every_transfer_entry_point_with_nothing_written() {
    // Arrange
    let real = Real::new().await;
    let rig = &real.rig;
    // Five blocks of 2 MiB: the fourth reaches `checkpoint_bytes` (8 MiB), where `put_block` runs the guard.
    let open = rig.upload(10 * MIB as usize, 3760).await;
    for index in 0..3 {
        rig.put(&open, index)
            .await
            .expect("block before the deletion");
    }
    let sent = rig.upload_bytes("sent.jpg", "sent", jpeg("sent")).await;
    rig.send_all(&sent).await;
    let staged = StagedFile {
        key: Uuid::new_v4(),
        name: "staged.jpg".to_owned(),
        data: jpeg("staged"),
    };
    rig.stage(staged.key, &staged.data)
        .await
        .expect("staged before the deletion");
    real.begin_deletion().await;
    let (rows, seq) = (real.rows().await, real.seq().await);

    // Act
    let start = rig
        .start(request(rig.root, "late.bin", 4 * MIB, "late"))
        .await;
    let put = rig.put(&open, 3).await;
    let stage = rig.stage(Uuid::new_v4(), &jpeg("late")).await;
    let commit = rig
        .commit(commit_request(&sent, rig.root, "sent.jpg", None))
        .await;
    let committed_batch = rig
        .commit_batch(batch(Uuid::new_v4(), vec![entry(&staged, rig.root)]))
        .await;

    // Assert
    for (name, result) in [
        ("start_upload", start.map(drop)),
        ("put_block", put.map(drop)),
        ("stage_small", stage.map(drop)),
    ] {
        assert!(
            matches!(result, Err(UploadError::AccountNotActive)),
            "{name}: {result:?}"
        );
    }
    for (name, result) in [
        ("commit_upload", commit.map(drop)),
        ("commit_batch", committed_batch.map(drop)),
    ] {
        assert!(
            matches!(result, Err(CommitServiceError::AccountNotActive)),
            "{name}: {result:?}"
        );
    }
    assert_eq!(
        real.rows().await,
        rows,
        "no session, staged row, node, revision, job or receipt; quota and feed unchanged"
    );
    assert_eq!(real.seq().await, seq, "no seq consumed");
    real.finish().await;
}
