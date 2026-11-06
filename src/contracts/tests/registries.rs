//! Recording doubles of the registry and lifecycle ports (AC-2), the re-exports and the error variants (AC-6).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Calls, NOW, id};
use contracts::{
    AccountLifecycleHook, BlobCleanupError, BlobOwnerCleanup, CheckFuture, CycleStats, DoctorCheck,
    DoctorContext, DoctorTag, EraseProgress, Finding, FindingLevel, HealthContributor, HealthItem,
    Janitor, JanitorError, LifecycleError, ReadinessCheck, ReadinessItem, async_trait,
};
use sc_clock::Timestamp;
use sc_db::DbTx;
use uuid::Uuid;

#[derive(Default)]
struct Double(Calls);

#[async_trait]
impl Janitor for Double {
    fn name(&self) -> &'static str {
        self.0.record("name");
        "double"
    }

    async fn run_cycle(&self, _now: Timestamp) -> Result<CycleStats, JanitorError> {
        self.0.record("run_cycle");
        Ok(CycleStats::default())
    }
}

#[async_trait]
impl AccountLifecycleHook for Double {
    async fn initialize(&self, _tx: &mut DbTx, _account: Uuid) -> Result<(), LifecycleError> {
        self.0.record("initialize");
        Ok(())
    }

    async fn erase_batch(
        &self,
        _account: Uuid,
        _limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        self.0.record("erase_batch");
        Ok(EraseProgress { remaining: 0 })
    }
}

#[async_trait]
impl BlobOwnerCleanup for Double {
    async fn remove_owner_rows(
        &self,
        _tx: &mut DbTx,
        _blob_id: Uuid,
    ) -> Result<(), BlobCleanupError> {
        self.0.record("remove_owner_rows");
        Ok(())
    }
}

#[async_trait]
impl DoctorCheck for Double {
    fn name(&self) -> &'static str {
        self.0.record("name");
        "double"
    }

    fn tags(&self) -> &[DoctorTag] {
        self.0.record("tags");
        &[DoctorTag::Restore, DoctorTag::PreFlight]
    }

    async fn run(&self, _ctx: &DoctorContext) -> Vec<Finding> {
        self.0.record("run");
        vec![Finding {
            level: FindingLevel::Ok,
            message: String::new(),
            remedy: String::new(),
        }]
    }
}

impl HealthContributor for Double {
    fn name(&self) -> &str {
        self.0.record("health_name");
        "double"
    }

    fn check(&self) -> CheckFuture<'_, HealthItem> {
        self.0.record("health_check");
        Box::pin(async { HealthItem::ok() })
    }
}

impl ReadinessCheck for Double {
    fn name(&self) -> &str {
        self.0.record("readiness_name");
        "double"
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        self.0.record("readiness_check");
        Box::pin(async { ReadinessItem::ok() })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn janitor_double_is_shared_across_two_threads_and_records_each_method_once() {
    let calls = Calls::default();
    let port: Arc<dyn Janitor> = Arc::new(Double(calls.clone()));
    let (a, b) = (port.clone(), port);

    let first = tokio::spawn(async move { a.name() });
    let second = tokio::spawn(async move { b.run_cycle(NOW).await });
    let name = first.await.expect("first task");
    let stats = second.await.expect("second task");

    assert_eq!(name, "double");
    assert_eq!(stats.expect("run_cycle"), CycleStats { processed: 0 });
    assert_eq!(calls.sorted(), ["name", "run_cycle"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doctor_double_is_shared_across_two_threads_and_records_each_method_once() {
    let calls = Calls::default();
    let port: Arc<dyn DoctorCheck> = Arc::new(Double(calls.clone()));
    let (a, b) = (port.clone(), port);
    let ctx = DoctorContext {
        pre_flight: true,
        backup_interval: Some(Duration::from_secs(3600)),
        attest_backup: false,
    };

    let first = tokio::spawn(async move { (a.name(), a.tags().len()) });
    let second = tokio::spawn(async move { b.run(&ctx).await });
    let (name, tags) = first.await.expect("first task");
    let findings = second.await.expect("second task");

    assert_eq!((name, tags), ("double", 2));
    assert_eq!(findings.len(), 1);
    assert_eq!(calls.sorted(), ["name", "run", "tags"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_double_is_shared_across_two_threads_and_records_erase_batch() {
    let calls = Calls::default();
    let hook: Arc<dyn AccountLifecycleHook> = Arc::new(Double(calls.clone()));
    let other = hook.clone();

    let first = tokio::spawn(async move { hook.erase_batch(id(), 100).await });
    let second = tokio::spawn(async move { other.erase_batch(id(), 100).await });
    let a = first.await.expect("first task").expect("erase_batch");
    let b = second.await.expect("second task").expect("erase_batch");

    assert_eq!(
        (a, b),
        (
            EraseProgress { remaining: 0 },
            EraseProgress { remaining: 0 }
        )
    );
    assert_eq!(calls.sorted(), ["erase_batch", "erase_batch"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_and_readiness_are_the_sc_observability_traits() {
    let calls = Calls::default();
    let health: Arc<dyn sc_observability::HealthContributor> = Arc::new(Double(calls.clone()));
    let ready: Arc<dyn sc_observability::ReadinessCheck> = Arc::new(Double(calls.clone()));
    let health: Arc<dyn HealthContributor> = health;
    let ready: Arc<dyn ReadinessCheck> = ready;

    let a = tokio::spawn(async move { (health.name().to_owned(), health.check().await) });
    let b = tokio::spawn(async move { (ready.name().to_owned(), ready.check().await) });
    let (health_name, health_item) = a.await.expect("health task");
    let (ready_name, ready_item) = b.await.expect("readiness task");

    assert_eq!(
        (health_name.as_str(), ready_name.as_str()),
        ("double", "double")
    );
    assert_eq!(health_item, HealthItem::ok());
    assert_eq!(ready_item, ReadinessItem::ok());
    assert_eq!(
        calls.sorted(),
        [
            "health_check",
            "health_name",
            "readiness_check",
            "readiness_name"
        ]
    );
}

fn janitor_variants(error: &JanitorError) -> &'static str {
    match error {
        JanitorError::Unexpected(_) => "Unexpected",
    }
}

fn lifecycle_variants(error: &LifecycleError) -> &'static str {
    match error {
        LifecycleError::Unexpected(_) => "Unexpected",
    }
}

fn cleanup_variants(error: &BlobCleanupError) -> &'static str {
    match error {
        BlobCleanupError::Unexpected(_) => "Unexpected",
    }
}

#[test]
fn registry_errors_carry_only_the_unexpected_variant_of_the_contract() {
    let names = [
        janitor_variants(&JanitorError::Unexpected(common::fault())),
        lifecycle_variants(&LifecycleError::Unexpected(common::fault())),
        cleanup_variants(&BlobCleanupError::Unexpected(common::fault())),
    ];

    assert_eq!(names, ["Unexpected"; 3]);
}
