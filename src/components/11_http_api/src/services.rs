use std::sync::Arc;

use contracts::{
    AccountDeletionService, AccountService, AdminService, Authenticator, ChangeFeed, CommitService,
    DerivedOpener, DeviceGrantService, DownloadService, InviteService, PrincipalRevalidator,
    PublicShareService, ReadinessCheck, SessionService, ShareService, SignInService,
    ThumbnailStateSource, TreeReader, TreeService, UploadService, VaultService,
    WorkerAuthenticator, WorkerJobs,
};
use sc_clock::Clock;
use sc_observability::Metrics;

/// Everything 11 calls, built by 12. 11 constructs none of it.
pub struct ApiServices {
    pub authenticator: Arc<dyn Authenticator>,
    pub principal_revalidator: Arc<dyn PrincipalRevalidator>,
    pub sign_in: Arc<dyn SignInService>,
    pub sessions: Arc<dyn SessionService>,
    pub device_grants: Arc<dyn DeviceGrantService>,
    pub invites: Arc<dyn InviteService>,
    pub tree: Arc<dyn TreeService>,
    pub tree_reader: Arc<dyn TreeReader>,
    pub uploads: Arc<dyn UploadService>,
    pub commits: Arc<dyn CommitService>,
    pub downloads: Arc<dyn DownloadService>,
    pub change_feed: Arc<dyn ChangeFeed>,
    pub shares: Arc<dyn ShareService>,
    pub public_shares: Arc<dyn PublicShareService>,
    pub admin: Arc<dyn AdminService>,
    pub accounts: Arc<dyn AccountService>,
    pub account_deletion: Arc<dyn AccountDeletionService>,
    pub thumbnail_states: Arc<dyn ThumbnailStateSource>,
    pub derived: Arc<dyn DerivedOpener>,
    pub worker_auth: Arc<dyn WorkerAuthenticator>,
    pub worker_jobs: Arc<dyn WorkerJobs>,
    /// Absent in v1.0 and v1.1 builds.
    pub vault: Option<Arc<dyn VaultService>>,
    /// The checks `/readyz` runs, registered by 12.
    pub readiness: Vec<Arc<dyn ReadinessCheck>>,
    pub metrics: Arc<Metrics>,
    pub clock: Arc<dyn Clock>,
    /// The same clock as `clock`, kept as the concrete type the test-clock routes advance.
    #[cfg(feature = "test-clock")]
    pub offset_clock: Arc<sc_clock::OffsetClock>,
}
