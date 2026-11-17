//! The late lists (ADR-015): the cycles a constructor graph cannot express, registered once in their documented
//! order and handed to their consumers.

use std::sync::Arc;
use std::time::Duration;

use contracts::{AccountLifecycleHook, BlobOwnerCleanup, CommitEffect, DoctorCheck, Janitor};
use media_api::{DerivedJanitor, JobReaper, MediaEnqueueEffect};
use sc_audit::AuditWriter;
use sc_clock::Clock;
use sc_db::{Db, DbConfig};
use sc_observability::{HealthContributor, Metrics, ReadinessCheck};
use sunnycloud_admin::DeletionPurger;
use sunnycloud_blobstore::{
    BlobHealthContributor, BlobstoreConfig, GcSweeper, KeyDoctorCheck, OrphanSweeper,
    RetentionCheck, ScrubJanitor, VolumeCheck,
};
use sunnycloud_identity::{IdentityConfig, IdentityProvidersCheck, IdentitySetupCheck};
use sunnycloud_transfer::{IdleFlush, ReceiptJanitor, SessionReaper};
use sunnycloud_tree::TreeJanitor;

use super::base::{DatabaseReady, Shutdown};
use super::domain::{Admin, Identity, Quota, Sharing, Transfer, Tree};
use super::failed;
use super::media::{Media, MediaPlanning};
use super::storage::Blobstore;
use crate::command_config::ServeSettings;
use crate::context::{ContextBuilder, ServiceContext};
use crate::database_check::DatabaseCheck;
use crate::error::CliError;
use crate::janitor::{AuditRetentionJanitor, JanitorMetrics, Scheduled, Scheduler};
use crate::missing_blobs_check::MissingBlobsReport;
use crate::quota_drift_check::QuotaDriftCheck;

/// A list entry with the component that registered it (`"06"`, `"07a"`); the hook and cleanup ports carry no
/// name of their own.
pub struct Registered<T: ?Sized> {
    pub owner: &'static str,
    pub item: Arc<T>,
}

impl<T: ?Sized> Registered<T> {
    fn new(owner: &'static str, item: Arc<T>) -> Self {
        Registered { owner, item }
    }
}

pub(super) fn items<T: ?Sized>(list: &[Registered<T>]) -> Vec<Arc<T>> {
    list.iter().map(|entry| Arc::clone(&entry.item)).collect()
}

/// 06 first so links stop at once, 20 (v1.2 builds) before 02 because vault roots reference 02's nodes, then 02,
/// 03, 08, and 05, 07a last because the tree's batches still use the feed and the quota row (07 §4). Given to
/// 07's `DeletionPurger` and to 01's account creation.
pub struct AccountHooks(pub Vec<Registered<dyn AccountLifecycleHook>>);

/// The commit effects 03 adds to its own on every commit: 08's media effect, one instance for every account (it
/// reads the account from each published entry).
pub struct CommitEffects(pub Vec<Registered<dyn CommitEffect>>);

/// 02, then 08; given to 04's GC sweeper.
pub struct BlobCleanups(pub Vec<Registered<dyn BlobOwnerCleanup>>);

/// The janitors with their owners' intervals, in the order 03, 04, 02, 07, 01, 08, then 12's audit retention.
pub struct Janitors(pub Vec<Scheduled>);

/// What 07's health aggregation runs: 04, then 08.
pub struct HealthContributors(pub Vec<Arc<dyn HealthContributor>>);

/// The owners' doctor checks: 04, then 08.
pub struct DoctorChecks(pub Vec<Arc<dyn DoctorCheck>>);

/// The `pre-flight` checks only, built from settings with no database: `doctor --pre-flight` runs before the
/// stack is up. Every check tagged `pre-flight` is listed here as well as in [`DoctorChecks`].
pub struct PreFlightChecks(pub Vec<Arc<dyn DoctorCheck>>);

/// 01's providers check; it holds no database, so `doctor --pre-flight` runs it too.
fn providers_check(
    settings: &ServeSettings,
    clock: Arc<dyn Clock>,
) -> Result<Arc<dyn DoctorCheck>, CliError> {
    let check = IdentityProvidersCheck::new(clock, &IdentityConfig::from_settings(settings))
        .map_err(|error| failed("the providers check", error))?;
    Ok(Arc::new(check))
}

/// What `/readyz` runs: the database, then `scheduler_alive` (failing until [`start_scheduler`]).
pub struct ReadinessChecks(pub Vec<Arc<dyn ReadinessCheck>>);

pub(super) fn register(builder: ContextBuilder) -> ContextBuilder {
    builder
        .register::<AccountHooks, _, _>(|services| async move {
            let hooks = account_hooks(&services).await?;
            let identity = services.get::<Identity>().await?;
            identity
                .sign_in
                .register_hooks(items(&hooks))
                .map_err(|error| failed("the account hook list", error))?;
            Ok(Arc::new(AccountHooks(hooks)))
        })
        .register::<CommitEffects, _, _>(|services| async move {
            let planning = services.get::<MediaPlanning>().await?;
            let media = MediaEnqueueEffect::new(planning.queue.clone(), planning.planner);
            Ok(Arc::new(CommitEffects(vec![Registered::new(
                "08",
                Arc::new(media),
            )])))
        })
        .register::<BlobCleanups, _, _>(|services| async move {
            let tree = services.get::<Tree>().await?;
            let media = services.get::<Media>().await?;
            Ok(Arc::new(BlobCleanups(vec![
                Registered::new("02", tree.lifecycle.clone()),
                Registered::new("08", media.lifecycle.clone()),
            ])))
        })
        .register::<HealthContributors, _, _>(|services| async move {
            let blobs = services.get::<Blobstore>().await?;
            let media = services.get::<Media>().await?;
            Ok(Arc::new(HealthContributors(vec![
                Arc::new(BlobHealthContributor::new(blobs.health.clone())),
                media.health.clone(),
            ])))
        })
        .register::<PreFlightChecks, _, _>(|services| async move {
            let settings = services.get::<ServeSettings>().await?;
            let clock = services.get::<dyn Clock>().await?;
            let config = BlobstoreConfig::from_settings(&settings);
            Ok(Arc::new(PreFlightChecks(vec![
                Arc::new(VolumeCheck::new(&config)),
                Arc::new(KeyDoctorCheck::without_database(config)),
                providers_check(&settings, clock)?,
            ])))
        })
        .register::<DatabaseCheck, _, _>(|services| async move {
            let settings = services.get::<ServeSettings>().await?;
            let clock = services.get::<dyn Clock>().await?;
            Ok(Arc::new(DatabaseCheck::new(
                DbConfig::from_settings(&settings),
                clock,
            )))
        })
        .register::<DoctorChecks, _, _>(|services| async move {
            let database = services.get::<DatabaseCheck>().await?;
            let identity = services.get::<Identity>().await?;
            let tree = services.get::<Tree>().await?;
            let transfer = services.get::<Transfer>().await?;
            let quota = services.get::<Quota>().await?;
            let settings = services.get::<ServeSettings>().await?;
            let clock = services.get::<dyn Clock>().await?;
            let db = Db::clone(&*services.get::<Db>().await?);
            let blobs = services.get::<Blobstore>().await?;
            let media = services.get::<Media>().await?;
            Ok(Arc::new(DoctorChecks(vec![
                database,
                Arc::new(KeyDoctorCheck::new(blobs.config.clone(), db.clone())),
                Arc::new(VolumeCheck::new(&blobs.config)),
                Arc::new(RetentionCheck::new(&blobs.config)),
                Arc::new(MissingBlobsReport::new(
                    blobs.inventory.clone(),
                    tree.reader.clone(),
                    blobs.batch(),
                )),
                Arc::new(QuotaDriftCheck::new(
                    identity.accounts.clone(),
                    tree.reader.clone(),
                    transfer.service.clone(),
                    quota.admin.clone(),
                )),
                media.health.clone(),
                providers_check(&settings, Arc::clone(&clock))?,
                Arc::new(IdentitySetupCheck::new(db, clock)),
            ])))
        })
        .register::<GcSweeper, _, _>(|services| async move {
            let db = Db::clone(&*services.get::<Db>().await?);
            let clock = services.get::<dyn Clock>().await?;
            let shutdown = services.get::<Shutdown>().await?;
            let blobs = services.get::<Blobstore>().await?;
            let cleanups = services.get::<BlobCleanups>().await?;
            GcSweeper::new(
                &blobs.config,
                db,
                clock,
                blobs.registry.clone(),
                items(&cleanups.0),
                shutdown.janitors.clone(),
            )
            .map(Arc::new)
            .map_err(|error| failed("the blob GC sweeper", error))
        })
        .register::<Janitors, _, _>(
            |services| async move { janitors(&services).await.map(Arc::new) },
        )
        .register::<Scheduler, _, _>(|services| async move {
            let clock = services.get::<dyn Clock>().await?;
            let metrics = services.get::<Metrics>().await?;
            let metrics = JanitorMetrics::new(&metrics)
                .map_err(|error| failed("the janitor metrics", error))?;
            Ok(Arc::new(Scheduler::new(clock, Arc::new(metrics))))
        })
        .register::<ReadinessChecks, _, _>(|services| async move {
            let database = services.get::<DatabaseReady>().await?;
            let scheduler = services.get::<Scheduler>().await?;
            Ok(Arc::new(ReadinessChecks(vec![
                database,
                scheduler.readiness(),
            ])))
        })
}

/// Starts the one scheduler with the janitor list; `serve` calls it after the late lists are registered.
pub async fn start_scheduler(services: &ServiceContext) -> Result<Arc<Scheduler>, CliError> {
    let janitors = services.get::<Janitors>().await?;
    let scheduler = services.get::<Scheduler>().await?;
    let jobs = janitors
        .0
        .iter()
        .map(|job| Scheduled {
            janitor: Arc::clone(&job.janitor),
            interval: job.interval,
        })
        .collect();
    scheduler
        .start(jobs)
        .map_err(|error| failed("the janitor scheduler", error))?;
    Ok(scheduler)
}

async fn account_hooks(
    services: &ServiceContext,
) -> Result<Vec<Registered<dyn AccountLifecycleHook>>, CliError> {
    let sharing = services.get::<Sharing>().await?;
    let tree = services.get::<Tree>().await?;
    let transfer = services.get::<Transfer>().await?;
    let media = services.get::<Media>().await?;
    let feed = services.get::<super::domain::Feed>().await?;
    let quota = services.get::<Quota>().await?;
    let mut hooks: Vec<Registered<dyn AccountLifecycleHook>> =
        vec![Registered::new("06", sharing.lifecycle.clone())];
    #[cfg(feature = "vault")]
    hooks.push(Registered::new(
        "20",
        services
            .get::<super::vault::Vault>()
            .await?
            .lifecycle
            .clone(),
    ));
    hooks.extend([
        Registered::new(
            "02",
            tree.lifecycle.clone() as Arc<dyn AccountLifecycleHook>,
        ),
        Registered::new("03", transfer.lifecycle.clone()),
        Registered::new("08", media.lifecycle.clone()),
        Registered::new("05", feed.lifecycle.clone()),
        Registered::new("07a", quota.admin.clone()),
    ]);
    Ok(hooks)
}

async fn janitors(services: &ServiceContext) -> Result<Janitors, CliError> {
    let settings = services.get::<ServeSettings>().await?;
    let db = Db::clone(&*services.get::<Db>().await?);
    let audit = AuditWriter::clone(&*services.get::<AuditWriter>().await?);
    let clock = services.get::<dyn Clock>().await?;
    let metrics = services.get::<Metrics>().await?;
    let transfer = services.get::<Transfer>().await?;
    let blobs = services.get::<Blobstore>().await?;
    let tree = services.get::<Tree>().await?;
    let identity = services.get::<Identity>().await?;
    let admin = services.get::<Admin>().await?;
    let hooks = services.get::<AccountHooks>().await?;
    let planning = services.get::<MediaPlanning>().await?;
    let blob = |error: contracts::BlobError| failed("a blobstore janitor", error);

    let gc = services.get::<GcSweeper>().await?;
    let orphans =
        OrphanSweeper::new(&blobs.config, db.clone(), Arc::clone(&clock)).map_err(blob)?;
    let purger = DeletionPurger::new(
        identity.accounts.clone(),
        items(&hooks.0),
        admin.config.clone(),
        audit,
        db.clone(),
        Arc::clone(&clock),
    )
    .with_metrics(&metrics)
    .map_err(|error| failed("the deletion purger metrics", error))?;
    let media = &planning.config;
    let every = |janitor: Arc<dyn Janitor>, interval: Duration| Scheduled { janitor, interval };
    Ok(Janitors(vec![
        every(
            Arc::new(SessionReaper::new(transfer.service.clone())),
            transfer.config.session_reap_interval,
        ),
        every(
            Arc::new(ReceiptJanitor::new(transfer.service.clone())),
            transfer.config.receipt_janitor_interval,
        ),
        every(
            Arc::new(IdleFlush::new(transfer.service.clone())),
            transfer.config.checkpoint_interval,
        ),
        every(gc, blobs.config.gc_interval),
        every(Arc::new(orphans), blobs.config.orphan_sweep_interval),
        every(
            Arc::new(ScrubJanitor::new(&blobs.config, blobs.scrubber.clone())),
            blobs.config.scrub_interval,
        ),
        every(
            Arc::new(TreeJanitor::purge(
                tree.maintenance.clone(),
                &tree.config,
                Arc::clone(&clock),
            )),
            tree.config.tree_maintenance_interval,
        ),
        every(
            Arc::new(TreeJanitor::cascade(
                tree.maintenance.clone(),
                &tree.config,
                Arc::clone(&clock),
            )),
            tree.config.tree_maintenance_interval,
        ),
        every(Arc::new(purger), admin.config.account_delete_interval),
        every(
            identity.maintenance.clone(),
            settings.identity_janitor_interval.get(),
        ),
        every(
            Arc::new(JobReaper::new(
                db.clone(),
                Arc::clone(&clock),
                planning.queue.clone(),
                media,
            )),
            media.job_reap_interval,
        ),
        every(
            Arc::new(DerivedJanitor::new(db.clone(), clock, blobs.gc.clone())),
            media.derived_janitor_interval,
        ),
        every(
            Arc::new(AuditRetentionJanitor::new(
                db,
                settings.audit_retention.get(),
            )),
            settings.audit_retention_interval.get(),
        ),
    ]))
}
