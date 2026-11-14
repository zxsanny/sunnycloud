//! Fixtures of the visitor tests (AZ-3698): named files whose current revision carries a size and a sniffed media
//! type, a save in place (a new current revision over a fresh blob), and the links of every visitor case made
//! through the real owner service. Rows are written as the schema owner; the service reads them through the real
//! tree.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use contracts::ShareService;
use sc_ids::{RawToken, TokenKind};
use sqlx::{Connection, PgConnection};
use sunnycloud_sharing::{PgShareService, SHARE_TOKEN_PREFIX};
use uuid::Uuid;

use crate::harness::Harness;
use crate::owner_support::{account_with_root, node, purge, tombstone};

pub const VISITOR: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
pub const HOUR: Duration = Duration::from_secs(3600);
pub const PASSWORD: &str = "correct horse";

/// A new current revision of `file` with `size` and `media_type`, over a fresh committed blob, as a publish or a
/// save in place leaves it.
pub async fn new_current_revision(
    conn: &mut PgConnection,
    account: Uuid,
    file: Uuid,
    size: i64,
    media_type: &str,
) {
    let (blob, revision) = (Uuid::new_v4(), Uuid::new_v4());
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, 'committed', $3, '\\x00', 1, 'epoch'::timestamptz)",
    )
    .bind(blob)
    .bind(account)
    .bind(size)
    .execute(&mut *conn)
    .await
    .expect("insert blob");
    sqlx::query(
        "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, media_type, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'epoch'::timestamptz)",
    )
    .bind(revision)
    .bind(account)
    .bind(file)
    .bind(blob)
    .bind(size)
    .bind(revision.as_bytes().repeat(2))
    .bind([9u8; 32].as_slice())
    .bind(media_type)
    .execute(&mut *conn)
    .await
    .expect("insert revision");
    sqlx::query("UPDATE nodes SET current_revision_id = $1 WHERE id = $2")
        .bind(revision)
        .bind(file)
        .execute(conn)
        .await
        .expect("point the file at its revision");
}

/// A live file `name` under `parent` whose current revision has `size` and `media_type`.
pub async fn named_file(
    conn: &mut PgConnection,
    account: Uuid,
    parent: Uuid,
    name: &str,
    size: i64,
    media_type: &str,
) -> Uuid {
    let file = node(conn, account, Some(parent), "file", name).await;
    new_current_revision(conn, account, file, size, media_type).await;
    file
}

/// A token as a visitor presents it.
pub fn presented(text: String) -> RawToken {
    RawToken::from_presented(TokenKind::Share, text)
}

/// A well-formed share token that no link has.
pub fn unknown_token() -> RawToken {
    presented(format!("{SHARE_TOKEN_PREFIX}{}", "A".repeat(43)))
}

/// The malformed tokens: empty, 10 kB, non-ASCII.
pub fn malformed_tokens() -> [RawToken; 3] {
    [
        presented(String::new()),
        presented("x".repeat(10 * 1024)),
        presented(format!("{SHARE_TOKEN_PREFIX}ключ-🔑-é")),
    ]
}

/// One account with a link in each server-side state; `expired_*` expire when the clock moves one hour.
pub struct Cases {
    pub shares: PgShareService,
    pub account: Uuid,
    pub live_file: Uuid,
    pub live: RawToken,
    pub password: RawToken,
    pub expired: RawToken,
    pub expired_password: RawToken,
    pub revoked: RawToken,
    pub tombstoned: RawToken,
    pub purged: RawToken,
}

/// Every link is made by the real `create`; the expired ones by a one-hour lifetime and a clock move of exactly
/// one hour (`expires_at == now` is gone), the others then still have days left.
pub async fn cases(harness: &Harness) -> Cases {
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    let root = account_with_root(&mut conn, account).await;
    let mut files = Vec::new();
    for name in [
        "live.jpg",
        "secret-plan.pdf",
        "old.txt",
        "gone.txt",
        "dead.txt",
    ] {
        files.push(named_file(&mut conn, account, root, name, 1234, "image/jpeg").await);
    }
    let shares = harness.shares();
    let create = async |file: Uuid, ttl: Option<Duration>, password: Option<&str>| {
        shares
            .create(account, file, ttl.or(Some(7 * 24 * HOUR)), password)
            .await
            .expect("create link")
    };
    let live = create(files[0], None, None).await;
    let password = create(files[1], None, Some(PASSWORD)).await;
    let expired = create(files[2], Some(HOUR), None).await;
    let expired_password = create(files[1], Some(HOUR), Some(PASSWORD)).await;
    let revoked = create(files[0], None, None).await;
    let tombstoned = create(files[3], None, None).await;
    let purged = create(files[4], None, None).await;
    shares.revoke(account, revoked.id).await.expect("revoke");
    tombstone(&mut conn, files[3]).await;
    purge(&mut conn, files[4]).await;
    conn.close().await.expect("close fixture connection");
    harness.clock.tick(HOUR);
    Cases {
        account,
        live_file: files[0],
        live: live.token,
        password: password.token,
        expired: expired.token,
        expired_password: expired_password.token,
        revoked: revoked.token,
        tombstoned: tombstoned.token,
        purged: purged.token,
        shares,
    }
}
