//! The domain components behind their ports: 01 identity, 07a quota, 05 sync feed, 02 tree, 03 transfer,
//! 06 sharing and 07 admin. Each builder asks the context for the ports it consumes, so a component built by
//! several consumers is still built once.

use std::sync::Arc;

use contracts::{
    AccountDeletionService, AdminService, EpochAdmin, InviteService, OperatorIdentityOps,
    ThumbNotifier, TreeRestore,
};
use sc_audit::AuditWriter;
use sc_clock::Clock;
use sc_config::ConfigError;
use sc_db::Db;
use sc_observability::{Metrics, NoLabels};
use sc_ratelimit::AbuseLimiter;
use sunnycloud_admin::{
    AccountDeletionServiceImpl, AccountServiceImpl, AdminConfig, AdminServiceImpl,
};
use sunnycloud_blobstore::FileEpochMarker;
use sunnycloud_identity::{
    IdentityAccounts, IdentityAuthenticator, IdentityConfig, IdentityDeviceGrants, IdentityInvites,
    IdentityMaintenance, IdentitySessions, IdentitySignIn, MaxmindGeoLookup,
};
use sunnycloud_quota::{PgQuotaAdmin, PgQuotaLedger, QuotaConfig};
use sunnycloud_sharing::{PgShareLifecycle, PgShareService, ShareConfig};
use sunnycloud_sync_feed::{
    FeedConfig, InProcessNotifier, InProcessThumbNotifier, PgChangeFeed, PgEpochAdmin,
    PgFeedHorizon, PgFeedLifecycle, PgFeedReader, PgFeedStamp,
};
use sunnycloud_transfer::{PgTransferLifecycle, PgUploadService, TransferConfig, UploadPorts};
use sunnycloud_tree::{
    PgTreeLifecycle, PgTreeMaintenance, PgTreeReader, PgTreeRestore, PgTreeService, TreeConfig,
};

use super::base::Shutdown;
use super::failed;
use super::lists::{CommitEffects, HealthContributors, items};
use super::media::MediaPlanning;
use super::storage::Blobstore;
use crate::command_config::ServeSettings;
use crate::context::{ContextBuilder, ServiceContext};
use crate::error::CliError;

pub struct Identity {
    pub config: IdentityConfig,
    pub accounts: Arc<IdentityAccounts>,
    pub authenticator: Arc<IdentityAuthenticator>,
    pub sign_in: Arc<IdentitySignIn>,
    pub sessions: Arc<IdentitySessions>,
    pub device_grants: Arc<IdentityDeviceGrants>,
    pub invites: Arc<IdentityInvites>,
    pub maintenance: Arc<IdentityMaintenance>,
}

pub struct Quota {
    pub ledger: Arc<PgQuotaLedger>,
    pub admin: Arc<PgQuotaAdmin>,
}

pub struct Feed {
    pub notifier: Arc<InProcessNotifier>,
    pub stamp: Arc<PgFeedStamp>,
    pub horizon: Arc<PgFeedHorizon>,
    pub change_feed: Arc<PgChangeFeed>,
    pub lifecycle: Arc<PgFeedLifecycle>,
    pub epoch_admin: Arc<PgEpochAdmin>,
}

pub struct Tree {
    pub config: TreeConfig,
    pub service: Arc<PgTreeService>,
    pub reader: Arc<PgTreeReader>,
    pub maintenance: Arc<PgTreeMaintenance>,
    pub restore: Arc<PgTreeRestore>,
    pub lifecycle: Arc<PgTreeLifecycle>,
}

pub struct Transfer {
    pub config: TransferConfig,
    pub service: Arc<PgUploadService>,
    pub lifecycle: Arc<PgTransferLifecycle>,
}

pub struct Sharing {
    pub service: Arc<PgShareService>,
    pub lifecycle: Arc<PgShareLifecycle>,
}

pub struct Admin {
    pub config: AdminConfig,
    pub accounts: Arc<AccountServiceImpl>,
    pub deletion: Arc<AccountDeletionServiceImpl>,
    pub admin: Arc<AdminServiceImpl>,
}

/// The pieces every builder below reads.
struct Base {
    settings: Arc<ServeSettings>,
    db: Db,
    audit: AuditWriter,
    clock: Arc<dyn Clock>,
}

impl Base {
    async fn get(services: &ServiceContext) -> Result<Base, CliError> {
        Ok(Base {
            settings: services.get::<ServeSettings>().await?,
            db: Db::clone(&*services.get::<Db>().await?),
            audit: AuditWriter::clone(&*services.get::<AuditWriter>().await?),
            clock: services.get::<dyn Clock>().await?,
        })
    }
}

pub(super) fn register(builder: ContextBuilder) -> ContextBuilder {
    builder
        .register::<Identity, _, _>(
            |services| async move { identity(&services).await.map(Arc::new) },
        )
        .register::<Quota, _, _>(|services| async move {
            let base = Base::get(&services).await?;
            let config = QuotaConfig::from_settings(&base.settings);
            Ok(Arc::new(Quota {
                ledger: Arc::new(PgQuotaLedger::new(config)),
                admin: Arc::new(PgQuotaAdmin::new(base.db, config)),
            }))
        })
        // The one wake of previews: 08 announces into it, 11's realtime sockets subscribe to it.
        .register::<dyn ThumbNotifier, _, _>(|_services| async move {
            let notifier: Arc<dyn ThumbNotifier> = InProcessThumbNotifier::new();
            Ok(notifier)
        })
        .register::<PgEpochAdmin, _, _>(|services| async move {
            let db = Db::clone(&*services.get::<Db>().await?);
            let marker = services.get::<FileEpochMarker>().await?;
            Ok(Arc::new(PgEpochAdmin::new(db, marker)))
        })
        .register::<dyn AdminService, _, _>(|services| async move {
            let admin: Arc<dyn AdminService> = services.get::<Admin>().await?.admin.clone();
            Ok(admin)
        })
        .register::<dyn AccountDeletionService, _, _>(|services| async move {
            let deletion: Arc<dyn AccountDeletionService> =
                services.get::<Admin>().await?.deletion.clone();
            Ok(deletion)
        })
        .register::<dyn OperatorIdentityOps, _, _>(|services| async move {
            let ops: Arc<dyn OperatorIdentityOps> =
                services.get::<Identity>().await?.invites.clone();
            Ok(ops)
        })
        .register::<dyn InviteService, _, _>(|services| async move {
            let invites: Arc<dyn InviteService> = services.get::<Identity>().await?.invites.clone();
            Ok(invites)
        })
        .register::<dyn EpochAdmin, _, _>(|services| async move {
            let admin: Arc<dyn EpochAdmin> = services.get::<PgEpochAdmin>().await?;
            Ok(admin)
        })
        .register::<Feed, _, _>(|services| async move { feed(&services).await.map(Arc::new) })
        .register::<Tree, _, _>(|services| async move { tree(&services).await.map(Arc::new) })
        .register::<dyn TreeRestore, _, _>(|services| async move {
            let restore: Arc<dyn TreeRestore> = services.get::<Tree>().await?.restore.clone();
            Ok(restore)
        })
        .register::<Transfer, _, _>(
            |services| async move { transfer(&services).await.map(Arc::new) },
        )
        .register::<Sharing, _, _>(|services| async move {
            let base = Base::get(&services).await?;
            let tree = services.get::<Tree>().await?;
            let limiter = services.get::<AbuseLimiter>().await?;
            let config = ShareConfig::from_settings(&base.settings);
            let service = PgShareService::new(
                base.db.clone(),
                base.clock,
                tree.reader.clone(),
                base.audit,
                limiter,
                &config,
            )
            .map_err(|problem| CliError::Config(ConfigError::Invalid(vec![problem])))?;
            Ok(Arc::new(Sharing {
                service: Arc::new(service),
                lifecycle: Arc::new(PgShareLifecycle::new(base.db)),
            }))
        })
        .register::<Admin, _, _>(|services| async move { admin(&services).await.map(Arc::new) })
}

async fn identity(services: &ServiceContext) -> Result<Identity, CliError> {
    let Base {
        settings,
        db,
        audit,
        clock,
    } = Base::get(services).await?;
    let limiter = services.get::<AbuseLimiter>().await?;
    let config = IdentityConfig::from_settings(&settings);
    let geo = Arc::new(MaxmindGeoLookup::open(config.geoip_db_path.as_deref()));
    let sign_in = IdentitySignIn::new(
        db.clone(),
        Arc::clone(&clock),
        &config,
        audit.clone(),
        Arc::clone(&limiter),
    )
    .map_err(|error| failed("the identity sign-in service", error))?;
    Ok(Identity {
        accounts: Arc::new(IdentityAccounts::new(db.clone(), Arc::clone(&clock))),
        authenticator: Arc::new(IdentityAuthenticator::new(
            db.clone(),
            Arc::clone(&clock),
            &config,
        )),
        sign_in: Arc::new(sign_in),
        sessions: Arc::new(IdentitySessions::new(
            db.clone(),
            Arc::clone(&clock),
            &config,
            audit.clone(),
        )),
        device_grants: Arc::new(IdentityDeviceGrants::new(
            db.clone(),
            Arc::clone(&clock),
            geo,
            &config,
            limiter,
            audit.clone(),
        )),
        invites: Arc::new(IdentityInvites::new(db.clone(), clock, &config, audit)),
        maintenance: Arc::new(IdentityMaintenance::new(db)),
        config,
    })
}

async fn feed(services: &ServiceContext) -> Result<Feed, CliError> {
    let base = Base::get(services).await?;
    let blobs = services.get::<Blobstore>().await?;
    let metrics = services.get::<Metrics>().await?;
    let shutdown = services.get::<Shutdown>().await?;
    let config = FeedConfig::from_settings(&base.settings);
    let counter = |name| {
        metrics
            .counter::<NoLabels>(name)
            .map(Arc::new)
            .map_err(|error| failed("the feed metrics", error))
    };
    let notifier = InProcessNotifier::new();
    let change_feed = PgChangeFeed::new(
        PgFeedReader::new(base.db.clone(), &config),
        Arc::clone(&notifier),
        Arc::clone(&base.clock),
        shutdown.feed.clone(),
        counter("feed_missed_wake_total")?,
        counter("feed_resync_required_total")?,
        &config,
    );
    Ok(Feed {
        notifier,
        stamp: Arc::new(PgFeedStamp),
        horizon: Arc::new(PgFeedHorizon),
        change_feed: Arc::new(change_feed),
        lifecycle: Arc::new(PgFeedLifecycle::new(
            base.db,
            base.clock,
            blobs.epoch_marker.clone(),
        )),
        epoch_admin: services.get::<PgEpochAdmin>().await?,
    })
}

async fn tree(services: &ServiceContext) -> Result<Tree, CliError> {
    let base = Base::get(services).await?;
    let identity = services.get::<Identity>().await?;
    let quota = services.get::<Quota>().await?;
    let blobs = services.get::<Blobstore>().await?;
    let feed = services.get::<Feed>().await?;
    let media = services.get::<MediaPlanning>().await?;
    let config = TreeConfig::from_settings(&base.settings);
    let service = Arc::new(PgTreeService::new(
        base.db.clone(),
        identity.accounts.clone(),
        quota.ledger.clone(),
        blobs.gc.clone(),
        feed.stamp.clone(),
        feed.notifier.clone(),
        Arc::clone(&base.clock),
    ));
    let reader =
        PgTreeReader::new(base.db.clone(), &config).with_thumbnails(media.thumbnails.clone());
    Ok(Tree {
        reader: Arc::new(reader),
        maintenance: Arc::new(PgTreeMaintenance::new(
            base.db.clone(),
            blobs.gc.clone(),
            quota.ledger.clone(),
            feed.horizon.clone(),
            &config,
        )),
        restore: Arc::new(PgTreeRestore::new(
            Arc::clone(&service),
            blobs.registry.clone(),
            blobs.config.retention,
        )),
        lifecycle: Arc::new(PgTreeLifecycle::new(base.db, blobs.gc.clone(), base.clock)),
        service,
        config,
    })
}

async fn transfer(services: &ServiceContext) -> Result<Transfer, CliError> {
    let base = Base::get(services).await?;
    let identity = services.get::<Identity>().await?;
    let quota = services.get::<Quota>().await?;
    let blobs = services.get::<Blobstore>().await?;
    let tree = services.get::<Tree>().await?;
    let metrics = services.get::<Metrics>().await?;
    let effects = services.get::<CommitEffects>().await?;
    let config = TransferConfig::from_settings(&base.settings);
    let ports = UploadPorts {
        guard: identity.accounts.clone(),
        tree: tree.reader.clone(),
        commit: tree.service.clone(),
        reader: blobs.reader.clone(),
        commit_effects: items(&effects.0),
        quota: quota.ledger.clone(),
        quota_admin: quota.admin.clone(),
        writer: blobs.writer.clone(),
        registry: blobs.registry.clone(),
        catalog: blobs.catalog.clone(),
    };
    let service = PgUploadService::new(base.db, config.clone(), ports, base.clock)
        .with_metrics(&metrics)
        .map_err(|error| failed("the upload metrics", error))?;
    let service = Arc::new(service);
    Ok(Transfer {
        lifecycle: Arc::new(PgTransferLifecycle::new(
            Arc::clone(&service),
            blobs.gc.clone(),
        )),
        service,
        config,
    })
}

async fn admin(services: &ServiceContext) -> Result<Admin, CliError> {
    let base = Base::get(services).await?;
    let identity = services.get::<Identity>().await?;
    let quota = services.get::<Quota>().await?;
    let tree = services.get::<Tree>().await?;
    let health = services.get::<HealthContributors>().await?;
    let config = AdminConfig::from_settings(&base.settings);
    Ok(Admin {
        accounts: Arc::new(AccountServiceImpl::new(
            identity.accounts.clone(),
            quota.admin.clone(),
            tree.reader.clone(),
        )),
        deletion: Arc::new(AccountDeletionServiceImpl::new(
            identity.accounts.clone(),
            config.clone(),
            base.audit.clone(),
            base.db.clone(),
            base.clock,
        )),
        admin: Arc::new(AdminServiceImpl::new(
            identity.accounts.clone(),
            quota.admin.clone(),
            health.0.clone(),
            config.clone(),
            base.audit,
            base.db,
        )),
        config,
    })
}
