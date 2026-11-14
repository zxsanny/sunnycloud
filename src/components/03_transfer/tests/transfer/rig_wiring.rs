//! What a rig is wired with (AZ-3760): the account guard 02's `publish` and 03 share, the effects in 03's
//! composition-root slot (`UploadPorts::commit_effects`) and the account with its root. The boundary wiring writes
//! the account rows directly, checks the state through `sunnycloud_testkit::StateGuard` and leaves the slot empty;
//! `real_support` (transfer binary only) wires the real 01 guard and 08 media effect.

use std::sync::Arc;

use contracts::{AccountGuard, CommitEffect};
use sc_clock::ManualClock;
use sc_db::Db;
use sqlx::PgConnection;
use sunnycloud_blobstore::BlobstoreConfig;
use sunnycloud_testkit::StateGuard;
use uuid::Uuid;

use crate::start_support::{close, node};
use crate::support::{Fixture, account};

/// What a wiring may use: the component pool, an owner connection it closes, the volume's 04 settings and the
/// rig's clock.
pub struct Site {
    pub api: Db,
    pub owner: PgConnection,
    pub blob_config: BlobstoreConfig,
    pub clock: Arc<ManualClock>,
}

impl Site {
    pub async fn new(
        fixture: &Fixture,
        blob_config: &BlobstoreConfig,
        clock: &Arc<ManualClock>,
    ) -> Site {
        Site {
            api: fixture.api.clone(),
            owner: fixture.owner_conn().await,
            blob_config: blob_config.clone(),
            clock: clock.clone(),
        }
    }
}

pub struct Wiring {
    pub guard: Arc<dyn AccountGuard>,
    pub effects: Vec<Arc<dyn CommitEffect>>,
    pub account: Uuid,
    pub root: Uuid,
}

/// The guard backed at its boundary, an empty effect slot, and an active account written row by row.
pub async fn boundary(site: Site) -> Wiring {
    let Site { mut owner, .. } = site;
    let id = Uuid::new_v4();
    let root = account_rows(&mut owner, id).await;
    close(owner).await;
    Wiring {
        guard: Arc::new(StateGuard::default()),
        effects: Vec::new(),
        account: id,
        root,
    }
}

/// The account, its root, its feed row and its quota row (the installation default of 10 GiB applies); returns
/// the root.
async fn account_rows(conn: &mut PgConnection, id: Uuid) -> Uuid {
    account(conn, id).await;
    let root = node(conn, id, None, "folder", "").await;
    sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, 1)")
        .bind(id)
        .execute(&mut *conn)
        .await
        .expect("insert feed row");
    sqlx::query("INSERT INTO quota (account_id) VALUES ($1)")
        .bind(id)
        .execute(conn)
        .await
        .expect("insert quota row");
    root
}
