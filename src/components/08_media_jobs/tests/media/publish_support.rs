//! The real 02 `TreeCommit::publish` (`PgTreeService`) over the media test database, wired as 02's own publish
//! tests wire it: the real 07a ledger, 04 `GcQueue`, 05 feed stamp and notifier, and the boundary-backed
//! `sunnycloud_testkit::StateGuard` (the identity guard is not a dependency of this crate). Minimal copies of 02's `mutation_support.rs` and
//! `publish_support.rs`; the tree crate is not edited.

use std::collections::HashSet;
use std::sync::Arc;

use contracts::{
    Begin, CommitActor, CommitEffect, EffectError, EntryOutcome, NewRevision, PublishEntry,
    PublishedEntry, Target, async_trait,
};
use sc_db::DbTx;
use sc_error::ErrorCode;
use sc_ids::{FileHash, Hash32};
use sqlx::Connection;
use sunnycloud_blobstore::{BlobstoreConfig, GcQueue};
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedStamp};
use sunnycloud_testkit::StateGuard;
use sunnycloud_tree::PgTreeService;
use uuid::Uuid;

use crate::queue_support::Queue;
use crate::support::{blob, load_settings};

/// Large enough that the ledger never refuses in these tests.
const DEFAULT_QUOTA: i64 = 1 << 40;

/// An effect run after the media effect: its `on_entry` fails the entries of the named revisions, and its
/// `on_batch` fails the whole commit when `batch` is set.
pub struct Failing {
    pub entries: HashSet<Uuid>,
    pub batch: bool,
}

#[async_trait]
impl CommitEffect for Failing {
    async fn on_begin(&self, _tx: &mut DbTx) -> Result<Begin, EffectError> {
        Ok(Begin::Proceed)
    }

    async fn on_entry(&self, _tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), EffectError> {
        if self.entries.contains(&entry.revision_id) {
            Err(EffectError::Entry(ErrorCode::UploadSessionGone))
        } else {
            Ok(())
        }
    }

    async fn on_batch(
        &self,
        _tx: &mut DbTx,
        _outcomes: &[EntryOutcome],
    ) -> Result<(), EffectError> {
        if self.batch {
            Err(EffectError::Batch(ErrorCode::Internal))
        } else {
            Ok(())
        }
    }
}

/// The tree service of the queue's database; the default account gains its feed and quota rows.
pub async fn tree_service(q: &Queue) -> (Arc<PgTreeService>, Uuid) {
    let mut conn = q.fixture.owner_conn().await;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM nodes WHERE account_id = $1 AND parent_id IS NULL")
            .bind(q.account)
            .fetch_one(&mut conn)
            .await
            .expect("the account's root");
    sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, 1)")
        .bind(q.account)
        .execute(&mut conn)
        .await
        .expect("insert feed row");
    sqlx::query("INSERT INTO quota (account_id) VALUES ($1)")
        .bind(q.account)
        .execute(&mut conn)
        .await
        .expect("insert quota row");
    conn.close().await.expect("close owner connection");
    let clock = sc_testdb::clock_at_start();
    let blobstore = BlobstoreConfig::from_settings(&load_settings(&[]).expect("settings"));
    let queue = GcQueue::new(&blobstore, q.fixture.api.clone(), clock.clone()).expect("gc queue");
    let service = PgTreeService::new(
        q.fixture.api.clone(),
        Arc::new(StateGuard::default()),
        Arc::new(PgQuotaLedger::new(QuotaConfig {
            default_quota_bytes: DEFAULT_QUOTA,
        })),
        Arc::new(queue),
        Arc::new(PgFeedStamp),
        InProcessNotifier::new(),
        clock,
    );
    (Arc::new(service), root)
}

pub fn actor(account: Uuid) -> CommitActor {
    CommitActor {
        account_id: account,
        device_label: "laptop".to_owned(),
    }
}

/// A new JPEG file under `parent` over a fresh committed blob of the account.
pub async fn new_photo(
    q: &Queue,
    parent: Uuid,
    name: &str,
    content_hash: FileHash,
) -> PublishEntry {
    new_file(q, parent, name, content_hash, Some("image/jpeg"), false).await
}

/// A new file of `media_type` under `parent` over a fresh committed blob of the account.
pub async fn new_file(
    q: &Queue,
    parent: Uuid,
    name: &str,
    content_hash: FileHash,
    media_type: Option<&str>,
    vault: bool,
) -> PublishEntry {
    let blob_id = Uuid::new_v4();
    let mut conn = q.fixture.owner_conn().await;
    blob(&mut conn, blob_id, q.account).await;
    conn.close().await.expect("close owner connection");
    PublishEntry {
        target: Target::New {
            parent_id: parent,
            name: name.to_owned(),
        },
        revision: NewRevision {
            revision_id: Uuid::new_v4(),
            blob_id,
            size: 100,
            content_hash,
            blocklist: vec![Hash32::from_bytes([3; 32])],
            media_type: media_type.map(str::to_owned),
            client_mtime: None,
            client_meta: None,
            reserved_bytes: 0,
            import_hint: false,
        },
        vault,
    }
}
