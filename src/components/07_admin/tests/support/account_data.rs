//! Fixtures of the account services as the schema owner: linked identities, the root folder, a live file over
//! a committed blob and a share link, and probes of what the deletion request must leave in place. The
//! statements are test code (the SQL gate exempts `tests/`).

use sqlx::PgConnection;
use uuid::Uuid;

use super::harness::START;

/// The name of every fixture file; the audit check proves it never reaches an audit row.
pub const FILE_NAME: &str = "holiday-photo.jpg";

/// An identity of `account` at `provider`; `email` is stored as verified when present.
pub async fn linked_identity(
    conn: &mut PgConnection,
    account: Uuid,
    provider: &str,
    email: Option<&str>,
    created_micros: i64,
) {
    sqlx::query(
        "INSERT INTO identities (id, account_id, provider, provider_user_id, email, email_verified, created_at) \
         VALUES (gen_random_uuid(), $1, $2, gen_random_uuid()::text, $3, $3 IS NOT NULL, \
         'epoch'::timestamptz + $4::bigint * interval '1 microsecond')",
    )
    .bind(account)
    .bind(provider)
    .bind(email)
    .bind(created_micros)
    .execute(conn)
    .await
    .expect("insert identity");
}

async fn node(
    conn: &mut PgConnection,
    account: Uuid,
    parent: Option<Uuid>,
    kind: &str,
    name: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, lower($5), 1, \
         'epoch'::timestamptz + $6::bigint * interval '1 microsecond', \
         'epoch'::timestamptz + $6::bigint * interval '1 microsecond')",
    )
    .bind(id)
    .bind(account)
    .bind(parent)
    .bind(kind)
    .bind(name)
    .bind(START.as_unix_micros())
    .execute(conn)
    .await
    .expect("insert node");
    id
}

/// The account's root folder; returns its id.
pub async fn root(conn: &mut PgConnection, account: Uuid) -> Uuid {
    node(conn, account, None, "folder", "").await
}

/// A live file under `parent` with one current revision over a committed blob; returns the file id.
pub async fn live_file(conn: &mut PgConnection, account: Uuid, parent: Uuid) -> Uuid {
    let (blob, revision) = (Uuid::new_v4(), Uuid::new_v4());
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, 'committed', 1, '\\x00', 1, 'epoch'::timestamptz)",
    )
    .bind(blob)
    .bind(account)
    .execute(&mut *conn)
    .await
    .expect("insert blob");
    let file = node(conn, account, Some(parent), "file", FILE_NAME).await;
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
        .expect("set current revision");
    file
}

/// A share link on `node`, valid for a week.
pub async fn share_link(conn: &mut PgConnection, account: Uuid, node: Uuid) {
    sqlx::query(
        "INSERT INTO share_links (id, account_id, node_id, token_hash, expires_at, created_at) \
         VALUES (gen_random_uuid(), $1, $2, sha256(gen_random_uuid()::text::bytea), \
         'epoch'::timestamptz + $3::bigint * interval '1 microsecond' + interval '7 days', \
         'epoch'::timestamptz + $3::bigint * interval '1 microsecond')",
    )
    .bind(account)
    .bind(node)
    .bind(START.as_unix_micros())
    .execute(conn)
    .await
    .expect("insert share link");
}

/// `(nodes, blobs, revisions, share links)` of the account.
pub async fn data_rows(conn: &mut PgConnection, account: Uuid) -> [i64; 4] {
    let row: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM nodes WHERE account_id = $1), \
         (SELECT count(*) FROM blobs WHERE account_id = $1), \
         (SELECT count(*) FROM revisions WHERE account_id = $1), \
         (SELECT count(*) FROM share_links WHERE account_id = $1)",
    )
    .bind(account)
    .fetch_one(conn)
    .await
    .expect("count data rows");
    [row.0, row.1, row.2, row.3]
}

/// `deletion_requested_at` of the account in microseconds.
pub async fn deletion_requested_at(conn: &mut PgConnection, account: Uuid) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT (extract(epoch FROM deletion_requested_at) * 1000000)::bigint FROM accounts WHERE id = $1",
    )
    .bind(account)
    .fetch_one(conn)
    .await
    .expect("read deletion_requested_at")
}
