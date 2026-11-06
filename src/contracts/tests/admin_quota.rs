//! Recording doubles of the 07 admin and 07a quota ports (AC-2). Methods that take `&mut DbTx` are in
//! `tx_join.rs`.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use common::{Calls, NOW, id, principal, sorted};
use contracts::{
    AccountDeletionService, AccountService, AccountServiceError, AccountState, AccountView, Actor,
    AdminError, AdminService, AdminUserView, DeletionAccepted, DeletionError, DevSeed,
    DevSeedError, Drift, HealthItem, HealthReport, Page, Principal, QuotaAdmin, QuotaAdminError,
    QuotaLedger, QuotaLedgerError, QuotaView, Reserved, Role, async_trait,
};
use sc_db::DbTx;
use uuid::Uuid;

#[derive(Default)]
struct Double(Calls);

fn quota() -> QuotaView {
    QuotaView {
        limit_bytes: None,
        effective_limit: 1 << 30,
        used: 10,
        reserved: 5,
    }
}

#[async_trait]
impl AdminService for Double {
    async fn list_users(
        &self,
        _actor: &Actor,
        _page: Page,
    ) -> Result<Vec<AdminUserView>, AdminError> {
        self.0.record("list_users");
        Ok(Vec::new())
    }

    async fn update_user(
        &self,
        _actor: &Actor,
        _account: Uuid,
        _role: Option<Role>,
        _state: Option<AccountState>,
    ) -> Result<(), AdminError> {
        self.0.record("update_user");
        Ok(())
    }

    async fn set_quota(
        &self,
        _actor: &Actor,
        _account: Uuid,
        _limit: Option<i64>,
    ) -> Result<(), AdminError> {
        self.0.record("set_quota");
        Ok(())
    }

    async fn health(&self, _actor: &Actor) -> HealthReport {
        self.0.record("health");
        HealthReport {
            items: vec![("database".to_owned(), HealthItem::ok())],
        }
    }
}

#[async_trait]
impl AccountService for Double {
    async fn view(&self, principal: &Principal) -> Result<AccountView, AccountServiceError> {
        self.0.record("view");
        Ok(AccountView {
            id: principal.account_id,
            display_name: "Ada".to_owned(),
            role: Role::User,
            identities: Vec::new(),
            quota: quota(),
            root_node_id: id(),
        })
    }

    async fn quota_view(&self, _principal: &Principal) -> Result<QuotaView, AccountServiceError> {
        self.0.record("quota_view");
        Ok(quota())
    }
}

#[async_trait]
impl AccountDeletionService for Double {
    async fn request(
        &self,
        _principal: &Principal,
        _confirmation: &str,
    ) -> Result<DeletionAccepted, DeletionError> {
        self.0.record("request");
        Ok(DeletionAccepted { deadline: NOW })
    }
}

#[async_trait]
impl DevSeed for Double {
    async fn seed(&self) -> Result<(), DevSeedError> {
        self.0.record("seed");
        Ok(())
    }
}

#[async_trait]
impl QuotaLedger for Double {
    async fn reserve(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        bytes: i64,
    ) -> Result<Reserved, QuotaLedgerError> {
        self.0.record("reserve");
        Ok(Reserved {
            reserved_bytes: bytes,
        })
    }

    async fn release_reservation(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _bytes: i64,
    ) -> Result<(), QuotaLedgerError> {
        self.0.record("release_reservation");
        Ok(())
    }

    async fn adjust_used(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _delta: i64,
    ) -> Result<(), QuotaLedgerError> {
        self.0.record("adjust_used");
        Ok(())
    }
}

#[async_trait]
impl QuotaAdmin for Double {
    async fn view(&self, _account: Uuid) -> Result<QuotaView, QuotaAdminError> {
        self.0.record("quota_admin_view");
        Ok(quota())
    }

    async fn views(&self, accounts: &[Uuid]) -> Result<HashMap<Uuid, QuotaView>, QuotaAdminError> {
        self.0.record("views");
        Ok(accounts.iter().map(|account| (*account, quota())).collect())
    }

    async fn set_limit(&self, _account: Uuid, _limit: Option<i64>) -> Result<(), QuotaAdminError> {
        self.0.record("set_limit");
        Ok(())
    }

    async fn recompute(
        &self,
        _account: Uuid,
        live_bytes: i64,
        _reserved_bytes: i64,
        _repair: bool,
    ) -> Result<Drift, QuotaAdminError> {
        self.0.record("recompute");
        Ok(Drift {
            stored: 10,
            computed: live_bytes,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_double_records_each_method_across_two_threads() {
    let calls = Calls::default();
    let port: Arc<dyn AdminService> = Arc::new(Double(calls.clone()));
    let other = port.clone();
    let page = Page {
        offset: 0,
        limit: 50,
    };

    let a = tokio::spawn(async move {
        port.list_users(&Actor::Operator, page)
            .await
            .expect("list_users");
        port.update_user(
            &Actor::Admin(principal()),
            id(),
            Some(Role::Admin),
            Some(AccountState::Active),
        )
        .await
        .expect("update_user");
    });
    let b = tokio::spawn(async move {
        other
            .set_quota(&Actor::Operator, id(), Some(1))
            .await
            .expect("set_quota");
        other.health(&Actor::Operator).await
    });
    a.await.expect("first task");
    let report = b.await.expect("second task");

    assert_eq!(report.items.len(), 1);
    assert_eq!(
        calls.sorted(),
        sorted(vec!["health", "list_users", "set_quota", "update_user"])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_deletion_and_dev_seed_doubles_record_each_method_across_two_threads() {
    let calls = Calls::default();
    let accounts: Arc<dyn AccountService> = Arc::new(Double(calls.clone()));
    let deletion: Arc<dyn AccountDeletionService> = Arc::new(Double(calls.clone()));
    let seed: Arc<dyn DevSeed> = Arc::new(Double(calls.clone()));

    let a = tokio::spawn(async move {
        accounts.view(&principal()).await.expect("view");
        accounts.quota_view(&principal()).await.expect("quota_view");
    });
    let b = tokio::spawn(async move {
        let accepted = deletion
            .request(&principal(), "delete")
            .await
            .expect("request");
        seed.seed().await.expect("seed");
        accepted
    });
    a.await.expect("first task");
    let accepted = b.await.expect("second task");

    assert_eq!(accepted, DeletionAccepted { deadline: NOW });
    assert_eq!(
        calls.sorted(),
        sorted(vec!["quota_view", "request", "seed", "view"])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quota_admin_double_records_each_method_across_two_threads() {
    let calls = Calls::default();
    let port: Arc<dyn QuotaAdmin> = Arc::new(Double(calls.clone()));
    let other = port.clone();

    let a = tokio::spawn(async move {
        port.view(id()).await.expect("view");
        port.views(&[id()]).await.expect("views")
    });
    let b = tokio::spawn(async move {
        other.set_limit(id(), None).await.expect("set_limit");
        other.recompute(id(), 7, 0, false).await.expect("recompute")
    });
    let views = a.await.expect("first task");
    let drift = b.await.expect("second task");

    assert_eq!(views.len(), 1);
    assert_eq!(
        drift,
        Drift {
            stored: 10,
            computed: 7
        }
    );
    assert_eq!(
        calls.sorted(),
        sorted(vec!["quota_admin_view", "recompute", "set_limit", "views"])
    );
}

#[test]
fn every_admin_and_quota_port_is_object_safe_send_and_sync() {
    fn held<T: ?Sized + Send + Sync>(_: Option<Arc<T>>) {}
    held::<dyn AdminService>(None);
    held::<dyn AccountService>(None);
    held::<dyn AccountDeletionService>(None);
    held::<dyn DevSeed>(None);
    held::<dyn QuotaLedger>(None);
    held::<dyn QuotaAdmin>(None);
}
