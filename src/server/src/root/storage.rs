//! 04 blobstore: the master keyring and the epoch marker (each its own builder, so `key-check` and `serve`'s start
//! checks build nothing else) and the blob services.
//! The GC sweeper, orphan sweeper and scrub janitor are built with the janitor list, because the sweeper takes the
//! blob-owner cleanups of 02 and 08.

use std::sync::Arc;

use sc_audit::AuditWriter;
use sc_clock::Clock;
use sc_db::Db;
use sc_observability::Metrics;
use sunnycloud_blobstore::{
    ActivityGauge, BlobCatalog, BlobHealth, BlobInventory, BlobMetrics, BlobRegistry, BlobScrubber,
    BlobstoreConfig, FileBlobReader, FileBlobWriter, FileEpochMarker, GcQueue, MasterKeyring,
    ScrubMetrics, SyncBarrier,
};

use super::failed;
use crate::command_config::ServeSettings;
use crate::context::{ContextBuilder, ServiceContext};
use crate::error::CliError;

/// The blob services of 04 over one keyring, one activity gauge and one metrics set.
pub struct Blobstore {
    pub config: BlobstoreConfig,
    pub keyring: Arc<MasterKeyring>,
    pub reader: Arc<FileBlobReader>,
    pub writer: Arc<FileBlobWriter>,
    pub registry: Arc<BlobRegistry>,
    pub catalog: Arc<BlobCatalog>,
    pub gc: Arc<GcQueue>,
    pub inventory: Arc<BlobInventory>,
    pub health: Arc<BlobHealth>,
    pub epoch_marker: Arc<FileEpochMarker>,
    /// The writer's group-commit barrier, for the final group sync at shutdown.
    pub barrier: SyncBarrier,
    pub scrubber: Arc<BlobScrubber>,
    pub activity: ActivityGauge,
}

impl Blobstore {
    /// The step size of a scrub run and the page of the missing-blobs check: the table has no key for either
    /// (04 leaves the choice to the composition root), so both follow 04's own batch key, `gc_batch`.
    pub fn batch(&self) -> u32 {
        batch(&self.config)
    }
}

fn batch(config: &BlobstoreConfig) -> u32 {
    u32::try_from(config.gc_batch).unwrap_or(u32::MAX)
}

pub(super) fn register(builder: ContextBuilder) -> ContextBuilder {
    builder
        .register::<MasterKeyring, _, _>(|services| async move {
            let settings = services.get::<ServeSettings>().await?;
            let db = services.get::<Db>().await?;
            let audit = services.get::<AuditWriter>().await?;
            Ok(Arc::new(MasterKeyring::new(
                BlobstoreConfig::from_settings(&settings),
                Db::clone(&db),
                AuditWriter::clone(&audit),
            )))
        })
        .register::<FileEpochMarker, _, _>(|services| async move {
            let settings = services.get::<ServeSettings>().await?;
            let marker = FileEpochMarker::new(&BlobstoreConfig::from_settings(&settings))
                .map_err(|error| failed("the epoch marker", error))?;
            Ok(Arc::new(marker))
        })
        .register::<Blobstore, _, _>(
            |services| async move { blobstore(&services).await.map(Arc::new) },
        )
}

async fn blobstore(services: &ServiceContext) -> Result<Blobstore, CliError> {
    let settings = services.get::<ServeSettings>().await?;
    let config = BlobstoreConfig::from_settings(&settings);
    let db = Db::clone(&*services.get::<Db>().await?);
    let audit = AuditWriter::clone(&*services.get::<AuditWriter>().await?);
    let clock = services.get::<dyn Clock>().await?;
    let metrics = services.get::<Metrics>().await?;
    let keyring = services.get::<MasterKeyring>().await?;
    let blob = |error: contracts::BlobError| failed("the blobstore", error);

    let blob_metrics = BlobMetrics::register(&metrics, Arc::clone(&clock))
        .map_err(|error| failed("the blobstore metrics", error))?;
    let scrub_metrics =
        ScrubMetrics::register(&metrics).map_err(|error| failed("the scrub metrics", error))?;
    let activity = ActivityGauge::new();
    let reader = FileBlobReader::new(
        &config,
        Arc::clone(&keyring),
        db.clone(),
        audit.clone(),
        Arc::clone(&clock),
    )
    .map_err(blob)?
    .with_activity(activity.clone())
    .with_metrics(Arc::clone(&blob_metrics));
    let barrier =
        SyncBarrier::new(config.blob_sync_coalesce_delay).with_metrics(Arc::clone(&blob_metrics));
    let writer = FileBlobWriter::new(&config, Arc::clone(&keyring), barrier.clone())
        .map_err(blob)?
        .with_activity(activity.clone());
    let health = BlobHealth::new(&config, db.clone())
        .map_err(blob)?
        .with_metrics(blob_metrics);
    let store = Blobstore {
        reader: Arc::new(reader),
        writer: Arc::new(writer),
        registry: Arc::new(BlobRegistry::new(&config, Arc::clone(&clock)).map_err(blob)?),
        catalog: Arc::new(BlobCatalog::new(db.clone())),
        gc: Arc::new(GcQueue::new(&config, db.clone(), Arc::clone(&clock)).map_err(blob)?),
        inventory: Arc::new(BlobInventory::new(&config, db.clone()).map_err(blob)?),
        health: Arc::new(health),
        epoch_marker: services.get::<FileEpochMarker>().await?,
        barrier,
        scrubber: Arc::new(scrubber(
            &config,
            &keyring,
            db,
            audit,
            clock,
            scrub_metrics,
            &activity,
        )?),
        keyring,
        activity,
        config,
    };
    Ok(store)
}

fn scrubber(
    config: &BlobstoreConfig,
    keyring: &Arc<MasterKeyring>,
    db: Db,
    audit: AuditWriter,
    clock: Arc<dyn Clock>,
    metrics: ScrubMetrics,
    activity: &ActivityGauge,
) -> Result<BlobScrubber, CliError> {
    BlobScrubber::new(
        config,
        Arc::clone(keyring),
        db,
        audit,
        clock,
        metrics,
        batch(config),
    )
    .map(|scrubber| scrubber.with_activity(activity.clone()))
    .map_err(|error| failed("the blob scrubber", error))
}
