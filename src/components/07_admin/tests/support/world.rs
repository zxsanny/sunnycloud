//! The account services over the real components on a real PostgreSQL 18: 01's `IdentityAccounts` (account
//! admin and guard) and `IdentityAuthenticator`, 02's tree reader and commit (`PgTreeService` with 01's guard,
//! 07a's ledger, 04's GC queue and 05's stamp and notifier), and 07a's `PgQuotaAdmin`. The components connect
//! as the API role with row-level security on; fixtures and probes use a plain connection of the schema owner.

use std::sync::Arc;

use contracts::{AccountAdmin, Credential};
use sc_audit::AuditWriter;
use sc_clock::{Clock, ManualClock};
use sc_config::Environment;
use sc_db::Db;
use sc_ids::{RawToken, TokenKind};
use sc_testdb::{MIGRATOR, TestDb};
use sqlx::PgConnection;
use sunnycloud_admin::{AccountDeletionServiceImpl, AccountServiceImpl, AdminConfig};
use sunnycloud_blobstore::{BlobstoreConfig, GcQueue};
use sunnycloud_identity::{IdentityAccounts, IdentityAuthenticator, IdentityConfig};
use sunnycloud_quota::{PgQuotaAdmin, PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedStamp};
use sunnycloud_tree::{PgTreeReader, PgTreeService, TreeConfig};
use uuid::Uuid;

use super::settings::load_settings;

/// The harness value of `account_delete_deadline` (07 `tests.md`, harness H1).
pub const DELETE_DEADLINE: &str = "30s";

pub struct World {
    pub db: TestDb,
    pub clock: Arc<ManualClock>,
    pub accounts: AccountServiceImpl,
    pub deletion: AccountDeletionServiceImpl,
    pub quota: Arc<PgQuotaAdmin>,
    pub tree: Arc<PgTreeReader>,
    pub commit: PgTreeService,
    identity_config: IdentityConfig,
    api: Db,
}

impl World {
    pub async fn new() -> World {
        let settings = load_settings(
            Environment::Test,
            &[("SUNNYCLOUD_ACCOUNT_DELETE_DEADLINE", DELETE_DEADLINE)],
        )
        .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let clock = sc_testdb::clock_at_start();
        let dyn_clock: Arc<dyn Clock> = clock.clone();
        let identity = Arc::new(IdentityAccounts::new(api.clone(), dyn_clock.clone()));
        let quota_config = QuotaConfig::from_settings(&settings);
        let quota = Arc::new(PgQuotaAdmin::new(api.clone(), quota_config));
        let tree = Arc::new(PgTreeReader::new(
            api.clone(),
            &TreeConfig::from_settings(&settings),
        ));
        let queue = GcQueue::new(
            &BlobstoreConfig::from_settings(&settings),
            api.clone(),
            dyn_clock.clone(),
        )
        .expect("gc queue");
        let commit = PgTreeService::new(
            api.clone(),
            identity.clone(),
            Arc::new(PgQuotaLedger::new(quota_config)),
            Arc::new(queue),
            Arc::new(PgFeedStamp),
            InProcessNotifier::new(),
            dyn_clock.clone(),
        );
        let admin: Arc<dyn AccountAdmin> = identity;
        let accounts = AccountServiceImpl::new(admin.clone(), quota.clone(), tree.clone());
        let deletion = AccountDeletionServiceImpl::new(
            admin,
            AdminConfig::from_settings(&settings),
            AuditWriter::new(api.clone(), dyn_clock.clone()),
            api.clone(),
            dyn_clock,
        );
        World {
            db,
            clock,
            accounts,
            deletion,
            quota,
            tree,
            commit,
            identity_config: IdentityConfig::from_settings(&settings),
            api,
        }
    }

    pub async fn fixture(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// A new authenticator, with an empty principal cache, so it answers from the stored credential rows.
    pub fn authenticator(&self) -> IdentityAuthenticator {
        IdentityAuthenticator::new(self.api.clone(), self.clock.clone(), &self.identity_config)
    }

    /// Drops the database; the server ends the sessions of probes a test still holds.
    pub async fn finish(self) {
        let World {
            db,
            clock,
            accounts,
            deletion,
            quota,
            tree,
            commit,
            identity_config,
            api,
        } = self;
        drop((
            clock,
            accounts,
            deletion,
            quota,
            tree,
            commit,
            identity_config,
            api,
        ));
        db.drop_db().await;
    }
}

/// The access token of the session `id` seeded by `seed::session`.
pub fn session_credential(id: Uuid) -> Credential {
    Credential::Session(RawToken::from_presented(
        TokenKind::Session,
        format!("{id}a"),
    ))
}

/// The device token `id` seeded by `seed::device_token`.
pub fn device_credential(id: Uuid) -> Credential {
    Credential::Device(RawToken::from_presented(
        TokenKind::DeviceToken,
        format!("{id}d"),
    ))
}
