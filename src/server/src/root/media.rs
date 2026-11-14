//! 08 media jobs. [`MediaPlanning`] holds what needs no other component (configuration, planner, metrics, queue,
//! thumbnail states), so 02's reader can take the thumbnail source without a cycle; [`Media`] holds the services
//! that read the tree and the blobs.

use std::sync::Arc;

use media_api::{
    BlobPorts, DerivedFiles, DerivedStore, JobMetrics, JobPlanner, JobQueue, MediaAdminService,
    MediaConfig, MediaHealth, MediaLifecycle, ThumbnailStates, WorkerAuth, WorkerJobsFacade,
};
use sc_clock::Clock;
use sc_db::Db;
use sc_observability::Metrics;

use super::domain::Tree;
use super::failed;
use super::storage::Blobstore;
use crate::command_config::ServeSettings;
use crate::context::{ContextBuilder, ServiceContext};
use crate::error::CliError;

pub struct MediaPlanning {
    pub config: MediaConfig,
    pub planner: JobPlanner,
    pub metrics: Arc<JobMetrics>,
    pub queue: Arc<JobQueue>,
    pub thumbnails: Arc<ThumbnailStates>,
}

pub struct Media {
    pub store: Arc<DerivedStore>,
    pub derived: Arc<DerivedFiles>,
    pub worker_auth: Arc<WorkerAuth>,
    pub worker_jobs: Arc<WorkerJobsFacade>,
    pub health: Arc<MediaHealth>,
    pub admin: Arc<MediaAdminService>,
    pub lifecycle: Arc<MediaLifecycle>,
}

pub(super) fn register(builder: ContextBuilder) -> ContextBuilder {
    builder
        .register::<MediaPlanning, _, _>(|services| async move {
            planning(&services).await.map(Arc::new)
        })
        .register::<Media, _, _>(|services| async move { media(&services).await.map(Arc::new) })
}

async fn planning(services: &ServiceContext) -> Result<MediaPlanning, CliError> {
    let settings = services.get::<ServeSettings>().await?;
    let db = Db::clone(&*services.get::<Db>().await?);
    let clock = services.get::<dyn Clock>().await?;
    let registry = services.get::<Metrics>().await?;
    let config = MediaConfig::from_settings(&settings);
    let planner = JobPlanner::from_config(&config);
    let metrics = Arc::new(
        JobMetrics::register(&registry).map_err(|error| failed("the media job metrics", error))?,
    );
    Ok(MediaPlanning {
        queue: Arc::new(JobQueue::new(
            db.clone(),
            clock,
            &config,
            Arc::clone(&metrics),
        )),
        thumbnails: Arc::new(ThumbnailStates::new(db, planner)),
        planner,
        metrics,
        config,
    })
}

async fn media(services: &ServiceContext) -> Result<Media, CliError> {
    let db = Db::clone(&*services.get::<Db>().await?);
    let clock = services.get::<dyn Clock>().await?;
    let registry = services.get::<Metrics>().await?;
    let planning = services.get::<MediaPlanning>().await?;
    let tree = services.get::<Tree>().await?;
    let blobs = services.get::<Blobstore>().await?;
    let ports = BlobPorts {
        writer: blobs.writer.clone(),
        reader: blobs.reader.clone(),
        catalog: blobs.catalog.clone(),
        registry: blobs.registry.clone(),
        gc: blobs.gc.clone(),
    };
    let store = Arc::new(DerivedStore::new(
        db.clone(),
        Arc::clone(&clock),
        tree.reader.clone(),
        ports,
        &planning.config,
        Arc::clone(&planning.metrics),
    ));
    let admin = Arc::new(MediaAdminService::new(
        db.clone(),
        Arc::clone(&planning.queue),
        Arc::clone(&planning.metrics),
    ));
    // The queue gauges are read from the table before each scrape (08's scrape probe).
    registry.register_probe(admin.clone());
    Ok(Media {
        derived: Arc::new(DerivedFiles::new(
            db.clone(),
            tree.reader.clone(),
            blobs.catalog.clone(),
            planning.planner,
        )),
        worker_auth: Arc::new(WorkerAuth::new(
            &planning.config,
            Arc::clone(&planning.metrics),
        )),
        worker_jobs: Arc::new(WorkerJobsFacade::new(
            db.clone(),
            Arc::clone(&clock),
            Arc::clone(&planning.queue),
            Arc::clone(&store),
        )),
        health: Arc::new(MediaHealth::new(db.clone(), clock, &planning.config)),
        lifecycle: Arc::new(MediaLifecycle::new(db, blobs.gc.clone())),
        store,
        admin,
    })
}
