//! The service under test over a real PostgreSQL 18 database: the API role runs the transactions and writes
//! the audit rows, the port doubles read through the schema owner, and fixtures and probes use a plain
//! connection of the schema owner.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use contracts::{Actor, HealthContributor, Principal, PrincipalKind, Role};
use sc_audit::AuditWriter;
use sc_config::Environment;
use sc_db::Db;
pub use sc_testdb::{MIGRATOR, START, TestDb};
use sqlx::PgConnection;
use sunnycloud_admin::{AdminConfig, AdminServiceImpl};
use uuid::Uuid;

use super::accounts_double::AccountsDouble;
use super::quota_double::QuotaDouble;
use super::settings::load_settings;

pub struct Harness {
    pub db: TestDb,
    pub service: AdminServiceImpl,
    pub accounts: Arc<AccountsDouble>,
    pub quota: Arc<QuotaDouble>,
    owner: Db,
}

impl Harness {
    pub async fn new() -> Harness {
        Harness::with_settings(&[]).await
    }

    pub async fn with_settings(extra: &[(&str, &str)]) -> Harness {
        Harness::with_health(extra, Vec::new()).await
    }

    pub async fn with_health(
        extra: &[(&str, &str)],
        contributors: Vec<Arc<dyn HealthContributor>>,
    ) -> Harness {
        let settings = load_settings(Environment::Test, extra)
            .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let owner = db.migrator().await;
        let accounts = Arc::new(AccountsDouble::new(owner.clone()));
        let quota = Arc::new(QuotaDouble::new(owner.clone()));
        let audit = AuditWriter::new(api.clone(), sc_testdb::clock_at_start());
        let service = AdminServiceImpl::new(
            accounts.clone(),
            quota.clone(),
            contributors,
            AdminConfig::from_settings(&settings),
            audit,
            api,
        );
        Harness {
            db,
            service,
            accounts,
            quota,
            owner,
        }
    }

    pub async fn fixture(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// Port calls made so far: accounts listings, usage lookups (`view` and `views`), account mutations and
    /// limit changes.
    pub fn port_calls(&self) -> [usize; 5] {
        [
            self.accounts.list_calls.load(Ordering::SeqCst),
            self.quota.view_calls.load(Ordering::SeqCst),
            self.quota.views_calls.load(Ordering::SeqCst),
            self.accounts.mutations.load(Ordering::SeqCst),
            self.quota.set_limit_calls.load(Ordering::SeqCst),
        ]
    }

    /// Drops the database; the server ends the sessions of probes a test still holds.
    pub async fn finish(self) {
        let Harness {
            db,
            service,
            accounts,
            quota,
            owner,
        } = self;
        drop((service, accounts, quota, owner));
        db.drop_db().await;
    }
}

pub fn principal(account_id: Uuid, kind: PrincipalKind, role: Role) -> Principal {
    Principal {
        account_id,
        kind,
        session_or_device_id: Uuid::new_v4(),
        role,
        label: String::new(),
    }
}

pub fn admin_actor(account_id: Uuid) -> Actor {
    Actor::Admin(principal(account_id, PrincipalKind::Session, Role::Admin))
}
