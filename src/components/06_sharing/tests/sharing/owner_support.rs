//! Fixtures of the owner tests (AZ-3697), written as the schema owner: accounts with a root folder, live files
//! with a current revision over a committed blob (what 02's `resolve_live_file` reads), tombstones and purges,
//! and probes of `share_links` and `audit_log`.

use sqlx::PgConnection;
use uuid::Uuid;

use crate::harness::account;

/// The name every fixture file has; the audit test checks it never appears in an event.
pub const FILE_NAME_PREFIX: &str = "holiday-photo";

/// One `audit_log` row as text: event, actor, subject type, subject id, detail.
pub type AuditRow = (
    String,
    Option<Uuid>,
    Option<String>,
    Option<Uuid>,
    Option<String>,
);

/// An account with its root folder; returns the root id.
pub async fn account_with_root(conn: &mut PgConnection, id: Uuid) -> Uuid {
    account(conn, id).await;
    node(conn, id, None, "folder", "").await
}

pub async fn node(
    conn: &mut PgConnection,
    account: Uuid,
    parent: Option<Uuid>,
    kind: &str,
    name: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, lower($5), 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(account)
    .bind(parent)
    .bind(kind)
    .bind(name)
    .execute(conn)
    .await
    .expect("insert node");
    id
}

/// A live file under `parent` with one current revision over a fresh committed blob; returns the file id.
pub async fn live_file(conn: &mut PgConnection, account: Uuid, parent: Uuid) -> Uuid {
    let (blob, revision) = (Uuid::new_v4(), Uuid::new_v4());
    let name = format!("{FILE_NAME_PREFIX}-{}.jpg", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, 'committed', 1, '\\x00', 1, 'epoch'::timestamptz)",
    )
    .bind(blob)
    .bind(account)
    .execute(&mut *conn)
    .await
    .expect("insert blob");
    let file = node(conn, account, Some(parent), "file", &name).await;
    sqlx::query(
        "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, created_at) \
         VALUES ($1, $2, $3, $4, 1, $5, $6, 'epoch'::timestamptz)",
    )
    .bind(revision)
    .bind(account)
    .bind(file)
    .bind(blob)
    .bind([7u8; 32].as_slice())
    .bind([9u8; 32].as_slice())
    .execute(&mut *conn)
    .await
    .expect("insert revision");
    sqlx::query("UPDATE nodes SET current_revision_id = $1 WHERE id = $2")
        .bind(revision)
        .bind(file)
        .execute(conn)
        .await
        .expect("point the file at its revision");
    file
}

pub async fn tombstone(conn: &mut PgConnection, node: Uuid) {
    sqlx::query("UPDATE nodes SET deleted_at = 'epoch'::timestamptz WHERE id = $1")
        .bind(node)
        .execute(conn)
        .await
        .expect("tombstone the node");
}

/// Removes the file row with its revisions, as the purge does.
pub async fn purge(conn: &mut PgConnection, file: Uuid) {
    for statement in [
        "UPDATE nodes SET current_revision_id = NULL WHERE id = $1",
        "DELETE FROM revisions WHERE node_id = $1",
        "DELETE FROM nodes WHERE id = $1",
    ] {
        sqlx::query(statement)
            .bind(file)
            .execute(&mut *conn)
            .await
            .expect("purge the file");
    }
}

/// Every column of every link, as JSON text.
pub async fn link_rows_as_text(conn: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar("SELECT row_to_json(s)::text FROM share_links s ORDER BY created_at, id")
        .fetch_all(conn)
        .await
        .expect("read share_links")
}

pub async fn token_hash_of(conn: &mut PgConnection, link: Uuid) -> Vec<u8> {
    sqlx::query_scalar("SELECT token_hash FROM share_links WHERE id = $1")
        .bind(link)
        .fetch_one(conn)
        .await
        .expect("read the token hash")
}

pub async fn password_hash_of(conn: &mut PgConnection, link: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT password_hash FROM share_links WHERE id = $1")
        .bind(link)
        .fetch_one(conn)
        .await
        .expect("read the password hash")
}

/// `revoked_at` in unix microseconds.
pub async fn revoked_at_of(conn: &mut PgConnection, link: Uuid) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT (extract(epoch FROM revoked_at) * 1000000)::bigint FROM share_links WHERE id = $1",
    )
    .bind(link)
    .fetch_one(conn)
    .await
    .expect("read revoked_at")
}

pub async fn link_count(conn: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM share_links")
        .fetch_one(conn)
        .await
        .expect("count links")
}

pub async fn audit_rows(conn: &mut PgConnection) -> Vec<AuditRow> {
    sqlx::query_as(
        "SELECT event, actor_account_id, subject_type, subject_id, detail::text FROM audit_log ORDER BY id",
    )
    .fetch_all(conn)
    .await
    .expect("read audit_log")
}
