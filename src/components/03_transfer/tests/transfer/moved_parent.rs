//! AZ-3757 AC-70 (IT-03-18, FT-N-42): a commit names its node by id and `parent_id` is only a hint. An upload for
//! the existing file f (base r1) whose folder F is moved to G by the real 02 between `start_upload` and
//! `commit_upload` lands on f under G with no conflict copy; a new-file commit whose given parent was tombstoned
//! meanwhile is `ParentGone` (02's `ParentDeleted`, `commit_service.md`).

use contracts::CommitServiceError;
use uuid::Uuid;

use crate::commit_support::commit_request;
use crate::put_support::{Upload, content};
use crate::start_support::{MIB, Rig, request, started};

const NAME: &str = "f.txt";

impl Rig {
    /// A folder created through 02 under `parent`.
    async fn create_folder(&self, parent: Uuid, name: &str) -> Uuid {
        self.commit
            .tree()
            .create_folder(self.account, parent, name)
            .await
            .expect("folder created")
            .id
    }

    /// Starts and sends in full an upload of `data` named `NAME` under `parent`, on `base` when given.
    async fn sent_upload(&self, parent: Uuid, key: &str, base: Option<Uuid>, seed: u64) -> Upload {
        let data = content(seed, MIB as usize);
        let declared = i64::try_from(data.len()).expect("size");
        let start = contracts::StartUpload {
            base_revision: base,
            ..request(parent, NAME, declared, key)
        };
        let id = started(self.start(start).await).upload_id;
        let blob = self.blob_of(id).await;
        let upload = Upload { id, blob, data };
        self.send_all(&upload).await;
        upload
    }

    async fn parent_of(&self, node: Uuid) -> Option<Uuid> {
        let mut conn = self.owner().await;
        let parent =
            sqlx::query_scalar("SELECT parent_id FROM nodes WHERE id = $1 AND account_id = $2")
                .bind(node)
                .bind(self.account)
                .fetch_one(&mut conn)
                .await
                .expect("node parent");
        crate::start_support::close(conn).await;
        parent
    }
}

#[tokio::test]
async fn commit_upload_after_the_folder_moved_lands_on_the_same_file_under_the_new_parent() {
    // Arrange
    let rig = Rig::new().await;
    let folder = rig.create_folder(rig.root, "F").await;
    let target = rig.create_folder(rig.root, "G").await;
    let first = rig.sent_upload(folder, "f-v1", None, 1001).await;
    let r1 = rig
        .commit(commit_request(&first, folder, NAME, None))
        .await
        .expect("first version");
    let second = rig
        .sent_upload(folder, "f-v2", Some(r1.revision_id), 1002)
        .await;
    rig.commit
        .tree()
        .update_node(rig.account, folder, None, Some(target))
        .await
        .expect("F moved under G");

    // Act
    let committed = rig
        .commit(commit_request(&second, folder, NAME, Some(r1.revision_id)))
        .await;

    // Assert
    let committed = committed.expect("committed after the move");
    assert_eq!(committed.node_id, r1.node_id, "same node id");
    assert_eq!(committed.conflict_copy, None);
    assert_eq!(
        rig.node_row(r1.node_id).await,
        (NAME.to_owned(), Some(committed.revision_id))
    );
    assert_eq!(rig.parent_of(r1.node_id).await, Some(folder));
    assert_eq!(
        rig.parent_of(folder).await,
        Some(target),
        "f is now under G"
    );
    assert_eq!(
        rig.count(&format!(
            "SELECT count(*) FROM nodes WHERE account_id = '{}' AND kind = 'file'",
            rig.account
        ))
        .await,
        1,
        "0 conflict copies"
    );
    rig.finish().await;
}

#[tokio::test]
async fn commit_upload_of_a_new_file_whose_parent_was_tombstoned_is_parent_gone() {
    // Arrange
    let rig = Rig::new().await;
    let folder = rig.create_folder(rig.root, "P").await;
    let upload = rig.sent_upload(folder, "orphaned", None, 1003).await;
    rig.commit
        .tree()
        .delete_node(rig.account, folder, None)
        .await
        .expect("P tombstoned");

    // Act
    let result = rig
        .commit(commit_request(&upload, folder, NAME, None))
        .await;

    // Assert
    assert!(
        matches!(result, Err(CommitServiceError::ParentGone)),
        "{result:?}"
    );
    assert_eq!(rig.revisions().await, 0, "no revision");
    rig.finish().await;
}
