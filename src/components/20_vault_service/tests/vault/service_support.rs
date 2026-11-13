//! `PgVaultService` wired to the real 02 `PgTreeReader` on the test database, with the vault section's default
//! header cap (1024 bytes). The service runs as `sunnycloud`; probes run as the schema owner.

use std::sync::Arc;
use std::time::Duration;

use contracts::{ETag, VaultService};
use sc_db::Db;
use sqlx::{Connection, PgConnection};
use sunnycloud_tree::{PgTreeReader, TreeConfig};
use sunnycloud_vault_service::{PgVaultService, VaultConfig};
use uuid::Uuid;

use crate::harness::{Harness, account_with_folder};

/// `max_vault_header_bytes` of these tests: the default of the vault section.
pub const MAX_HEADER: usize = 1024;

pub fn vault_config() -> VaultConfig {
    VaultConfig {
        max_vault_file_bytes: 100 * 1024 * 1024,
        max_vault_header_bytes: MAX_HEADER as u64,
    }
}

/// Short durations: a tombstone written at `START` is past the 2 s horizon at `START + 3 s`.
pub fn tree_config() -> TreeConfig {
    TreeConfig {
        tombstone_horizon: Duration::from_secs(2),
        listing_default_limit: 1000,
        listing_max_limit: 1000,
        cascade_batch_size: 1000,
        purge_batch_size: 1000,
        tree_maintenance_interval: Duration::from_secs(30),
        conflict_copy_time_zone: "UTC".to_owned(),
    }
}

/// A service over `db`, as the composition root builds it.
pub fn service_on(db: &Db) -> Arc<PgVaultService> {
    let reader = Arc::new(PgTreeReader::new(db.clone(), &tree_config()));
    Arc::new(PgVaultService::new(
        db.clone(),
        reader,
        sc_testdb::clock_at_start(),
        &vault_config(),
    ))
}

pub struct Env {
    pub harness: Harness,
    pub service: Arc<PgVaultService>,
    pub probe: PgConnection,
}

impl Env {
    pub async fn new() -> Env {
        let harness = Harness::new().await;
        let probe = harness.fixture_conn().await;
        let service = service_on(&harness.api);
        Env {
            harness,
            service,
            probe,
        }
    }

    /// A new account with an empty live folder; returns `(account, folder)`.
    pub async fn account(&mut self) -> (Uuid, Uuid) {
        let account = Uuid::new_v4();
        let folder = account_with_folder(&mut self.probe, account).await;
        (account, folder)
    }

    /// A new account with a vault holding `header`; returns `(account, vault, etag)`.
    pub async fn account_with_vault(&mut self, header: &[u8]) -> (Uuid, Uuid, ETag) {
        let (account, folder) = self.account().await;
        let created = self
            .service
            .create_vault(account, header, folder)
            .await
            .expect("create vault");
        (account, created.vault_id, created.etag)
    }

    /// The stored header bytes of `vault`, read past the service.
    pub async fn stored(&mut self, vault: Uuid) -> Option<Vec<u8>> {
        sqlx::query_scalar("SELECT header FROM vaults WHERE id = $1")
            .bind(vault)
            .fetch_optional(&mut self.probe)
            .await
            .expect("read stored header")
    }

    pub async fn vaults_of(&mut self, account: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM vaults WHERE account_id = $1")
            .bind(account)
            .fetch_one(&mut self.probe)
            .await
            .expect("count vaults")
    }

    /// The service holds a clone of the pool, so it goes first or its connections count as a leak.
    pub async fn finish(self) {
        let Env {
            harness,
            service,
            probe,
        } = self;
        drop(service);
        probe.close().await.expect("close probe connection");
        harness.finish().await;
    }
}
