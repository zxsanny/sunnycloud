//! AZ-3757 AC-68 (IT-03-17, R1 component part): a crash between the blob write and the commit leaves no revision
//! that points at a missing blob. (a) A large upload whose blocks are all acknowledged loses its service before
//! `commit_upload` (the restart is a new instance over the same database and volume): nothing is published, the
//! session resumes from its checkpoint and the commit succeeds. (b) A staged upload whose writer fails after
//! `write_complete` wrote the file and before the row transaction leaves no row, no reservation and one orphan
//! file for 04's sweep (IT-04-14). The reaper's TTL release after a crash is E-14/c11's.

use contracts::{CommitServiceError, Committed, UploadError, UploadState};
use uuid::Uuid;

use crate::commit_support::commit_request;
use crate::put_support::{Upload, content};
use crate::start_support::{MIB, Rig};

const LARGE: usize = 10 * MIB as usize;

impl Rig {
    async fn file_nodes(&self) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM nodes WHERE account_id = '{}' AND kind = 'file'",
            self.account
        ))
        .await
    }

    async fn blob_rows(&self) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM blobs WHERE account_id = '{}'",
            self.account
        ))
        .await
    }

    /// Every revision of the account whose blob file reads back, decrypted, as `expected`.
    async fn revisions_reading(&self, expected: &[u8]) -> usize {
        let mut conn = self.owner().await;
        let blobs: Vec<(Uuid, i64)> =
            sqlx::query_as("SELECT blob_id, size FROM revisions WHERE account_id = $1")
                .bind(self.account)
                .fetch_all(&mut conn)
                .await
                .expect("revision blobs");
        crate::start_support::close(conn).await;
        let mut matching = 0;
        for (blob, size) in blobs {
            let len = usize::try_from(size).expect("size");
            if self.file_exists(blob) && self.read_blob(blob, len).await == expected {
                matching += 1;
            }
        }
        matching
    }

    /// Resends the blocks `state` still needs, then commits `upload` under the root.
    async fn finish_after_restart(
        &self,
        upload: &Upload,
        state: &UploadState,
    ) -> Result<Committed, CommitServiceError> {
        for &index in &state.need_blocks {
            self.put(upload, index).await.expect("resent block");
        }
        self.commit(commit_request(upload, self.root, "crash.bin", None))
            .await
    }
}

#[tokio::test]
async fn a_large_upload_dropped_before_commit_publishes_nothing_and_commits_after_the_restart() {
    // Arrange
    let mut rig = Rig::new().await;
    let upload = rig.upload(LARGE, 901).await;
    rig.send_all(&upload).await;
    rig.restart();

    // Act
    let before = (rig.revisions().await, rig.file_nodes().await);
    let state = rig
        .service
        .get_upload(rig.account, upload.id)
        .await
        .expect("session after the restart");
    let committed = rig.finish_after_restart(&upload, &state).await;

    // Assert
    assert_eq!(before, (0, 0), "no revision and no node before the commit");
    let blocks = upload.block_count();
    assert_eq!(
        state.received_count + state.need_blocks.len() as u32,
        blocks,
        "every block is either received or listed as missing: {state:?}"
    );
    assert!(state.need_blocks.iter().all(|&index| index < blocks));
    let committed = committed.expect("committed after the restart");
    assert_eq!(
        rig.revision_row(committed.revision_id).await.blob_id,
        upload.blob
    );
    assert_eq!(rig.revisions().await, 1);
    assert_eq!(
        rig.revisions_reading(&upload.data).await,
        1,
        "every revision's blob file exists and decrypts"
    );
    rig.finish().await;
}

#[tokio::test]
async fn a_staged_upload_failing_after_its_file_write_leaves_no_row_no_reservation_and_one_orphan_file()
 {
    // Arrange
    let rig = Rig::new().await;
    let data = content(902, 4096);
    rig.writer.fail_after_complete(true);

    // Act
    let result = rig.stage(Uuid::new_v4(), &data).await;

    // Assert
    assert!(matches!(result, Err(UploadError::Storage(_))), "{result:?}");
    assert_eq!(rig.writer.completes(), 1, "the file was written");
    assert_eq!(
        (
            rig.sessions().await,
            rig.blob_rows().await,
            rig.reserved().await
        ),
        (0, 0, 0)
    );
    assert_eq!(rig.files(), 1, "the orphan file is left for 04's sweep");
    assert_eq!((rig.revisions().await, rig.file_nodes().await), (0, 0));
    rig.finish().await;
}
