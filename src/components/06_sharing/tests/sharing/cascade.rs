//! AC-2: deleting a node row removes its links through `ON DELETE CASCADE`. The delete is a fixture delete as the
//! migrator role; the real purge path is tested with the purge (E-16/c06).

use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::harness::{Harness, account_with_file, link};

async fn link_ids(conn: &mut PgConnection) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT id FROM share_links ORDER BY id")
        .fetch_all(conn)
        .await
        .expect("list links")
}

#[tokio::test]
async fn deleting_a_node_removes_its_links_and_only_those() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let file_of_a = account_with_file(&mut conn, a).await;
    let file_of_b = account_with_file(&mut conn, b).await;
    link(&mut conn, a, file_of_a, 1).await.expect("link 1");
    link(&mut conn, a, file_of_a, 2).await.expect("link 2");
    let survivor = link(&mut conn, b, file_of_b, 3).await.expect("link 3");

    // Act
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(file_of_a)
        .execute(&mut conn)
        .await
        .expect("delete the node");
    let remaining = link_ids(&mut conn).await;

    // Assert
    assert_eq!(remaining, [survivor]);
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}
