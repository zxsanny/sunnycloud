//! The account data of the purger suite, written as the schema owner in bulk: the account with an identity, a
//! session and a device token; its quota row (with the reservations of its open uploads) and feed row; a tree of
//! folders and live files, each file with a current revision over a committed blob, some with retained older
//! revisions; share links; open upload sessions (large and staged) with reservations; commit receipts. Every
//! blob has a file on the volume. The media part (08's jobs and derived rows, with the derived blobs) is in
//! `media`. Statements are test code (the SQL gate exempts `tests/`).

use sc_testdb::START;
use sqlx::PgConnection;
use sunnycloud_blobstore::blob_path;
use uuid::Uuid;

use super::media;
use super::world::PurgeWorld;
use crate::support::seed;

/// The name of every seeded file and folder starts with it; no log line may carry it.
pub const NAME_MARKER: &str = "private-holiday";

/// Sizes of one seeded account.
#[derive(Debug, Clone, Copy)]
pub struct Plan {
    pub files: usize,
    pub folders: usize,
    pub retained: usize,
    pub shares: usize,
    pub feed_rows: usize,
    pub receipts: usize,
    /// Photo and video jobs over the account's revisions.
    pub jobs: usize,
    /// Derived rows (thumbnails, previews, transcodes), each over a committed blob with a file.
    pub derived: usize,
}

/// `tree-small` with the media subset (07 `tests.md`, IT-07-9): 10,000 files in 200 folders, 400 retained
/// revisions, 50 shares, 120 stamped feed rows, 30 photo and video jobs, 60 derived rows. Every plan also has 3
/// upload sessions (two large, one staged).
pub const TREE_SMALL: Plan = Plan {
    files: 10_000,
    folders: 200,
    retained: 400,
    shares: 50,
    feed_rows: 120,
    receipts: 5,
    jobs: 30,
    derived: 60,
};

/// A plan of `files` files with the same proportions as `tree-small`.
pub fn files(files: usize) -> Plan {
    Plan {
        files,
        folders: (files / 50).max(1),
        retained: files / 25,
        shares: files.min(50),
        feed_rows: files.min(120),
        receipts: 5,
        jobs: files.min(30),
        derived: 2 * files.min(30),
    }
}

/// `(declared_size, block_count, received)` of the two large sessions and the staged row.
const SESSIONS: [(i64, i32, &[u8]); 3] = [
    (3 << 20, 2, b"\x00"),
    (5 << 20, 3, b"\x00"),
    (10, 1, b"\x01"),
];

pub struct Seeded {
    pub account: Uuid,
    /// Every blob of the account: revisions', sessions' and derived.
    pub blobs: Vec<Uuid>,
    /// The derived blobs only.
    pub derived: Vec<Uuid>,
}

fn new_ids(count: usize) -> Vec<Uuid> {
    (0..count).map(|_| Uuid::new_v4()).collect()
}

/// An active user with the data of `plan`; the deletion is not requested yet.
pub async fn account(world: &PurgeWorld, plan: Plan) -> Seeded {
    let account = Uuid::new_v4();
    let mut conn = world.fixture().await;
    seed::account(
        &mut conn,
        account,
        "Purge Fixture",
        "user",
        "active",
        START.as_unix_micros(),
    )
    .await;
    seed::identity(&mut conn, account, "purge@example.test", None).await;
    seed::session(&mut conn, account).await;
    seed::device_token(&mut conn, account).await;
    let reserved: i64 = SESSIONS.iter().map(|(size, _, _)| size).sum();
    seed::quota(
        &mut conn,
        account,
        None,
        i64::try_from(plan.files).expect("fits"),
        reserved,
    )
    .await;
    exec(
        &mut conn,
        "INSERT INTO account_feeds (account_id, epoch, seq) VALUES ($1, 1, $2)",
        account,
        i64::try_from(plan.feed_rows).expect("fits"),
    )
    .await;
    let blobs = tree(&mut conn, account, plan).await;
    let mut all = blobs;
    all.extend(sessions(&mut conn, account).await);
    media::jobs(&mut conn, account, plan.jobs).await;
    let derived = media::derived(&mut conn, account, plan.derived).await;
    all.extend(&derived);
    exec(
        &mut conn,
        "INSERT INTO commit_receipts (account_id, commit_id, request_hash, response, created_at) \
         SELECT $1, gen_random_uuid(), decode(repeat('07', 32), 'hex'), '[]'::jsonb, 'epoch'::timestamptz \
         FROM generate_series(1, $2)",
        account,
        i64::try_from(plan.receipts).expect("fits"),
    )
    .await;
    for &blob in &all {
        put_file(world, blob);
    }
    Seeded {
        account,
        blobs: all,
        derived,
    }
}

async fn exec(conn: &mut PgConnection, sql: &'static str, account: Uuid, count: i64) {
    sqlx::query(sql)
        .bind(account)
        .bind(count)
        .execute(conn)
        .await
        .unwrap_or_else(|error| panic!("{error}\n{sql}"));
}

fn put_file(world: &PurgeWorld, blob: Uuid) {
    let path = blob_path(&world.volume(), blob);
    std::fs::create_dir_all(path.parent().expect("blob folder")).expect("folders");
    std::fs::write(path, b"x").expect("blob file");
}

/// Root, folders, files with current revisions and retained ones, stamped feed rows and share links; returns the
/// revisions' blobs.
async fn tree(conn: &mut PgConnection, account: Uuid, plan: Plan) -> Vec<Uuid> {
    let root = Uuid::new_v4();
    let folders = new_ids(plan.folders);
    let files = new_ids(plan.files);
    let blobs = new_ids(plan.files + plan.retained);
    let revisions = new_ids(plan.files + plan.retained);
    nodes(conn, account, "folder", &[root], &[None]).await;
    let parents: Vec<Option<Uuid>> = vec![Some(root); folders.len()];
    nodes(conn, account, "folder", &folders, &parents).await;
    let parents: Vec<Option<Uuid>> = (0..files.len())
        .map(|i| Some(folders[i % folders.len()]))
        .collect();
    nodes(conn, account, "file", &files, &parents).await;
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         SELECT b, $2, 'committed', 1, '\\x00', 1, 'epoch'::timestamptz FROM unnest($1::uuid[]) AS t(b)",
    )
    .bind(&blobs)
    .bind(account)
    .execute(&mut *conn)
    .await
    .expect("insert blobs");
    // Revision i < files is file i's current one; the rest are older revisions of the first files.
    let owners: Vec<Uuid> = (0..revisions.len())
        .map(|i| files[i % files.len()])
        .collect();
    let ages: Vec<i64> = (0..revisions.len())
        .map(|i| if i < files.len() { 1 } else { 0 })
        .collect();
    sqlx::query(
        "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, created_at) \
         SELECT r, $4, n, b, 1, decode(repeat('07', 32), 'hex'), ''::bytea, \
         'epoch'::timestamptz + a * interval '1 second' \
         FROM unnest($1::uuid[], $2::uuid[], $3::uuid[], $5::bigint[]) AS t(r, n, b, a)",
    )
    .bind(&revisions)
    .bind(&owners)
    .bind(&blobs)
    .bind(account)
    .bind(&ages)
    .execute(&mut *conn)
    .await
    .expect("insert revisions");
    sqlx::query(
        "UPDATE nodes n SET current_revision_id = t.r FROM unnest($1::uuid[], $2::uuid[]) AS t(n, r) \
         WHERE n.id = t.n",
    )
    .bind(&files)
    .bind(&revisions[..files.len()])
    .execute(&mut *conn)
    .await
    .expect("point files at their revisions");
    sqlx::query(
        "UPDATE nodes SET seq = t.ord FROM unnest($1::uuid[]) WITH ORDINALITY AS t(id, ord) \
         WHERE nodes.id = t.id",
    )
    .bind(&files[..plan.feed_rows])
    .execute(&mut *conn)
    .await
    .expect("stamp feed rows");
    sqlx::query(
        "INSERT INTO share_links (id, account_id, node_id, token_hash, expires_at, created_at) \
         SELECT gen_random_uuid(), $2, n, sha256(gen_random_uuid()::text::bytea), \
         'epoch'::timestamptz + interval '100 years', 'epoch'::timestamptz FROM unnest($1::uuid[]) AS t(n)",
    )
    .bind(&files[..plan.shares])
    .bind(account)
    .execute(&mut *conn)
    .await
    .expect("insert share links");
    blobs
}

async fn nodes(
    conn: &mut PgConnection,
    account: Uuid,
    kind: &str,
    ids: &[Uuid],
    parents: &[Option<Uuid>],
) {
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         SELECT id, $3, parent, $4, \
         CASE WHEN parent IS NULL THEN '' ELSE $5 || '-' || ord END, \
         CASE WHEN parent IS NULL THEN '' ELSE $5 || '-' || ord END, \
         1, 'epoch'::timestamptz, 'epoch'::timestamptz \
         FROM unnest($1::uuid[], $2::uuid[]) WITH ORDINALITY AS t(id, parent, ord)",
    )
    .bind(ids)
    .bind(parents)
    .bind(account)
    .bind(kind)
    .bind(NAME_MARKER)
    .execute(conn)
    .await
    .expect("insert nodes");
}

/// Two open large sessions and one staged row, each over an `uploading` blob; returns their blobs.
async fn sessions(conn: &mut PgConnection, account: Uuid) -> Vec<Uuid> {
    let mut blobs = Vec::new();
    for (index, (size, blocks, received)) in SESSIONS.into_iter().enumerate() {
        let blob = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO blobs (id, account_id, state, wrapped_key, master_key_id, created_at) \
             VALUES ($1, $2, 'uploading', '\\x00', 1, 'epoch'::timestamptz)",
        )
        .bind(blob)
        .bind(account)
        .execute(&mut *conn)
        .await
        .expect("insert session blob");
        sqlx::query(
            "INSERT INTO upload_sessions (id, account_id, blob_id, client_key, declared_size, block_count, \
             reserved_bytes, received, block_hashes, created_at, last_checkpoint_at, expires_at) \
             VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $4, $6, \
             decode(repeat('07', 32 * $5), 'hex'), 'epoch'::timestamptz, 'epoch'::timestamptz, \
             'epoch'::timestamptz + interval '100 years')",
        )
        .bind(account)
        .bind(blob)
        .bind(format!("upload-{index}"))
        .bind(size)
        .bind(blocks)
        .bind(received)
        .execute(&mut *conn)
        .await
        .expect("insert upload session");
        blobs.push(blob);
    }
    blobs
}
