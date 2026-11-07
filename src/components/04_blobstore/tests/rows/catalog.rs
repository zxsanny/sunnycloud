//! AC-2: the catalog is account scoped.

use contracts::{BlobCatalog, BlobRef};
use uuid::Uuid;

use crate::support::{Rig, WRAPPED_KEY, account, committed_blob};

#[tokio::test]
async fn own_blob_returns_its_ref_and_foreign_or_unknown_ids_return_none() {
    // Arrange
    let rig = Rig::new().await;
    let (a, b) = (account(&rig).await, account(&rig).await);
    let own = committed_blob(&rig, a).await;
    let foreign = committed_blob(&rig, b).await;

    // Act
    let mine = rig.catalog.get_ref(a, own).await.expect("own lookup");
    let theirs = rig
        .catalog
        .get_ref(a, foreign)
        .await
        .expect("foreign lookup");
    let unknown = rig
        .catalog
        .get_ref(a, Uuid::new_v4())
        .await
        .expect("unknown lookup");

    // Assert
    assert_eq!(
        mine,
        Some(BlobRef {
            blob_id: own,
            size: 10,
            wrapped_key: WRAPPED_KEY.to_vec(),
            master_key_id: 3,
        })
    );
    assert_eq!(theirs, None);
    assert_eq!(unknown, None);
    rig.finish().await;
}

#[tokio::test]
async fn an_unfinished_upload_has_no_ref_and_a_retained_blob_keeps_one() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let retained = committed_blob(&rig, a).await;
    let uploading = Uuid::new_v4();
    let mut conn = rig.owner().await;
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, 'uploading', $3, 3, 'epoch'::timestamptz)",
    )
    .bind(uploading)
    .bind(a)
    .bind(WRAPPED_KEY)
    .execute(&mut conn)
    .await
    .expect("uploading row");
    sqlx::query("UPDATE blobs SET state = 'gc_pending' WHERE id = $1")
        .bind(retained)
        .execute(&mut conn)
        .await
        .expect("retire blob");
    sqlx::Connection::close(conn).await.expect("close");

    // Act
    let unfinished = rig.catalog.get_ref(a, uploading).await.expect("lookup");
    let kept = rig.catalog.get_ref(a, retained).await.expect("lookup");

    // Assert
    assert_eq!(unfinished, None);
    assert_eq!(kept.map(|found| found.blob_id), Some(retained));
    rig.finish().await;
}
