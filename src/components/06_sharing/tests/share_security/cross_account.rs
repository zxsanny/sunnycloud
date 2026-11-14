//! ST-06-1 (AZ-3703 AC-1, AC-59, NFT-SEC-13): account B holds the node id of A's file and the id of A's link. B's
//! `create` on the file, `list` (all and for the node) and `revoke` of the link all fail as if they did not exist;
//! A's row, the link count and the audit log are unchanged, and a visitor still opens A's link.

use contracts::{ShareError, ShareOpen, ShareService};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::harness::Harness;
use crate::owner_support::{account_with_root, link_count};
use crate::visitor_support::{HOUR, VISITOR, named_file};

async fn row_of(conn: &mut PgConnection, link: Uuid) -> String {
    sqlx::query_scalar("SELECT row_to_json(s)::text FROM share_links s WHERE id = $1")
        .bind(link)
        .fetch_one(conn)
        .await
        .expect("read the link row")
}

async fn audit_count(conn: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(conn)
        .await
        .expect("count audit rows")
}

#[tokio::test]
async fn account_b_cannot_create_list_or_revoke_on_the_link_of_a() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let root_of_a = account_with_root(&mut conn, a).await;
    account_with_root(&mut conn, b).await;
    let f = named_file(&mut conn, a, root_of_a, "a-only.jpg", 512, "image/jpeg").await;
    let shares = harness.shares();
    let link_of_a = shares
        .create(a, f, Some(24 * HOUR), None)
        .await
        .expect("A's link");
    let row_before = row_of(&mut conn, link_of_a.id).await;
    let (links_before, audit_before) = (link_count(&mut conn).await, audit_count(&mut conn).await);

    // Act
    let created = shares.create(b, f, None, None).await;
    let created_with_password = shares.create(b, f, None, Some("b's password")).await;
    let listed = shares.list(b, None).await;
    let listed_for_f = shares.list(b, Some(f)).await;
    let revoked = shares.revoke(b, link_of_a.id).await;
    let opened = shares.open(&link_of_a.token, VISITOR).await;

    // Assert
    for attempt in [&created, &created_with_password] {
        assert!(
            matches!(attempt, Err(ShareError::TargetNotFound)),
            "{attempt:?}"
        );
    }
    assert!(listed.expect("B's list").is_empty());
    assert!(listed_for_f.expect("B's list for f").is_empty());
    assert!(matches!(revoked, Err(ShareError::NotFound)), "{revoked:?}");
    assert_eq!(row_of(&mut conn, link_of_a.id).await, row_before);
    assert_eq!(link_count(&mut conn).await, links_before);
    assert_eq!(audit_count(&mut conn).await, audit_before);
    assert_eq!(
        opened.expect("open"),
        ShareOpen::Available {
            name: "a-only.jpg".to_owned(),
            size: 512,
            media_type: Some("image/jpeg".to_owned()),
            expires_at: link_of_a.expires_at,
        }
    );
    conn.close().await.expect("close fixture connection");
    drop(shares);
    harness.finish().await;
}
