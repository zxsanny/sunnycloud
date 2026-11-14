//! AZ-3720 AC-1 (IT-02-10 with the real counterparts, NFT-SEC-27): an account moved to `deleting` by the real 01
//! `AccountAdmin::begin_deletion` is refused by the real 01 guard on `publish` (with the real 08 effect),
//! `create_folder`, `update_node`, `delete_node` and 03's `commit_batch`, with nothing written.

use std::sync::Arc;

use contracts::{
    BatchEntry, CommitEffect, CommitServiceError, PublishEntry, Target, TreeCommit,
    TreeCommitError, TreeServiceError,
};
use media_api::MediaEnqueueEffect;

use crate::publish_support::revision;
use crate::real_ports::jpeg;
use crate::real_support::Real;

#[tokio::test]
async fn a_deleting_account_is_refused_by_every_entry_point_with_nothing_written() {
    // Arrange
    let real = Real::new().await;
    let folder = real
        .service
        .create_folder(real.account, real.root, "F")
        .await
        .expect("create folder")
        .id;
    let data = jpeg("refused");
    let key = real.stage(&data).await;
    let mut entry = PublishEntry {
        target: Target::New {
            parent_id: real.root,
            name: "photo.jpg".to_owned(),
        },
        revision: revision(uuid::Uuid::new_v4()),
        vault: false,
    };
    entry.revision.media_type = Some("image/jpeg".to_owned());
    entry.revision.reserved_bytes = data.len() as i64;
    real.begin_deletion().await;
    let (rows, seq) = (real.rows().await, real.seq().await);
    let media: Arc<dyn CommitEffect> =
        Arc::new(MediaEnqueueEffect::new(real.queue.clone(), real.planner));

    // Act
    let published = real
        .service
        .publish(real.actor(), vec![entry], vec![media], sc_testdb::START)
        .await;
    let create = real
        .service
        .create_folder(real.account, real.root, "x")
        .await;
    let update = real
        .service
        .update_node(real.account, folder, Some("y"), Some(real.root))
        .await;
    let delete = real.service.delete_node(real.account, folder, None).await;
    let batch = real
        .commit_batch(
            uuid::Uuid::new_v4(),
            vec![BatchEntry {
                staged_key: key,
                parent_id: real.root,
                name: "photo.jpg".to_owned(),
                base_revision: None,
                client_mtime: None,
                client_meta: None,
                import_hint: false,
            }],
        )
        .await;

    // Assert
    assert!(
        matches!(published, Err(TreeCommitError::AccountNotActive)),
        "{published:?}"
    );
    for (name, result) in [("create", create.map(drop)), ("update", update.map(drop))] {
        assert!(
            matches!(result, Err(TreeServiceError::AccountNotActive)),
            "{name}: {result:?}"
        );
    }
    assert!(
        matches!(delete, Err(TreeServiceError::AccountNotActive)),
        "{delete:?}"
    );
    assert!(
        matches!(batch, Err(CommitServiceError::AccountNotActive)),
        "{batch:?}"
    );
    assert_eq!(
        real.rows().await,
        rows,
        "0 rows written, no quota change, no seq"
    );
    assert_eq!(real.seq().await, seq);
    real.finish().await;
}
