//! AZ-3927 AC-2 (IT-20-5, FT-N-14, NFT-SEC-43): `max_vault_file_bytes` on a real PostgreSQL 18 with the real 02
//! reader and `publish`. At upload start a vault upload of the limit is accepted and one byte more is refused with
//! `VaultFileTooLarge` before anything is reserved, stored or sent; a raised limit admits 101 MiB; outside a vault
//! the limit does not apply. At commit the same limit holds on the session path and the batch path, for a target
//! in a vault the start did not name.

use contracts::{CommitServiceError, EntryError, UploadError};
use sc_error::ErrorCode;
use sunnycloud_transfer::TransferConfig;
use uuid::Uuid;

use crate::batch_support::{StagedFile, batch, committed, entry, failures};
use crate::commit_support::commit_request;
use crate::put_support::content;
use crate::start_support::{MIB, Rig, request, started, transfer_config};

const LIMIT: i64 = 104_857_600;

fn with_vault_limit(bytes: i64) -> TransferConfig {
    TransferConfig {
        max_vault_file_bytes: u64::try_from(bytes).expect("positive limit"),
        ..transfer_config()
    }
}

#[tokio::test]
async fn a_vault_start_at_the_limit_is_accepted_and_one_byte_more_is_refused_before_anything_is_kept()
 {
    // Arrange
    let rig = Rig::new().await;
    let vault = rig.vault_folder("Vault").await;
    let at_limit = started(rig.start(request(vault, "at.bin", LIMIT, "at")).await);
    let before = (rig.sessions().await, rig.reserved().await, rig.files());

    // Act
    let above = rig
        .start(request(vault, "above.bin", LIMIT + 1, "above"))
        .await;

    // Assert
    assert!(!at_limit.need_blocks.is_empty());
    assert!(
        matches!(above, Err(UploadError::VaultFileTooLarge)),
        "{above:?}"
    );
    assert_eq!(
        (rig.sessions().await, rig.reserved().await, rig.files()),
        before,
        "no session, reservation or file: no block can be sent"
    );
    rig.finish().await;
}

#[tokio::test]
async fn a_raised_vault_limit_admits_101_mib_and_outside_a_vault_the_limit_does_not_apply() {
    // Arrange
    let raised = Rig::with_config(with_vault_limit(200 * MIB)).await;
    let raised_vault = raised.vault_folder("Vault").await;
    let default = Rig::new().await;
    default.vault_folder("Vault").await;

    // Act
    let in_raised = raised
        .start(request(raised_vault, "big.bin", 101 * MIB, "raised"))
        .await;
    let outside = default
        .start(request(default.root, "big.bin", 101 * MIB, "outside"))
        .await;

    // Assert
    started(in_raised);
    started(outside);
    raised.finish().await;
    default.finish().await;
}

#[tokio::test]
async fn a_session_committed_into_a_vault_above_the_limit_is_refused_and_stays_open() {
    // Arrange
    let rig = Rig::with_config(with_vault_limit(MIB)).await;
    let vault = rig.vault_folder("Vault").await;
    let data = content(7, usize::try_from(MIB + 1).expect("size"));
    let upload = rig.upload_bytes("photo.bin", "over", data).await;
    rig.send_all(&upload).await;
    let revisions = rig.revisions().await;

    // Act
    let into_vault = rig
        .commit(commit_request(&upload, vault, "photo.bin", None))
        .await;
    let session = rig.session_record(upload.id).await;
    let outside = rig
        .commit(commit_request(&upload, rig.root, "photo.bin", None))
        .await;

    // Assert
    assert!(
        matches!(into_vault, Err(CommitServiceError::VaultFileTooLarge)),
        "{into_vault:?}"
    );
    let session = session.expect("the session stays open");
    assert_eq!(session.committed_revision_id, None);
    outside.expect("the same upload outside the vault");
    assert_eq!(rig.revisions().await, revisions + 1);
    rig.finish().await;
}

#[tokio::test]
async fn a_batch_entry_into_a_vault_above_the_limit_fails_alone_and_one_at_the_limit_commits() {
    // Arrange
    let rig = Rig::with_config(with_vault_limit(MIB)).await;
    let vault = rig.vault_folder("Vault").await;
    let files = [
        StagedFile {
            key: Uuid::new_v4(),
            name: "over.bin".to_owned(),
            data: content(8, usize::try_from(MIB + 1).expect("size")),
        },
        StagedFile {
            key: Uuid::new_v4(),
            name: "at.bin".to_owned(),
            data: content(9, usize::try_from(MIB).expect("size")),
        },
    ];
    for file in &files {
        rig.stage_for(rig.account, file).await;
    }

    // Act
    let result = rig
        .commit_batch(batch(
            Uuid::new_v4(),
            files.iter().map(|file| entry(file, vault)).collect(),
        ))
        .await
        .expect("commit batch");

    // Assert
    assert_eq!(
        failures(&result),
        [(0, EntryError::EffectFailed(ErrorCode::VaultFileTooLarge))]
    );
    committed(&result.per_entry[1]);
    assert_eq!(
        rig.staged_rows(rig.account).await,
        1,
        "the refused file stays staged"
    );
    rig.finish().await;
}
