//! `ApiServices` of 11: one implementation of every port 11 calls, each the instance the builders made for every
//! other consumer, plus the readiness list, the feed shutdown token, the metrics registry and the clock.

use sc_clock::Clock;
use sc_observability::Metrics;
use sunnycloud_http_api::ApiServices;

use super::base::{DbMetrics, Shutdown};
use super::domain::{Admin, Feed, Identity, Sharing, Transfer, Tree};
use super::lists::ReadinessChecks;
use super::media::{Media, MediaPlanning};
use crate::context::ServiceContext;
use crate::error::CliError;

/// Builds what 11 receives. The account hook list is not part of it: `serve` registers it with 01 after binding
/// the listeners and before serving, so no sign-in is handled before 01 holds it.
pub async fn api_services(services: &ServiceContext) -> Result<ApiServices, CliError> {
    let identity = services.get::<Identity>().await?;
    let tree = services.get::<Tree>().await?;
    let transfer = services.get::<Transfer>().await?;
    let feed = services.get::<Feed>().await?;
    let sharing = services.get::<Sharing>().await?;
    let admin = services.get::<Admin>().await?;
    let planning = services.get::<MediaPlanning>().await?;
    let media = services.get::<Media>().await?;
    let readiness = services.get::<ReadinessChecks>().await?;
    let shutdown = services.get::<Shutdown>().await?;
    // The pool families reach `/metrics` with the services 11 serves (observability.md, ops listener).
    services.get::<DbMetrics>().await?;
    #[cfg(feature = "vault")]
    let vault: Option<std::sync::Arc<dyn contracts::VaultService>> =
        Some(services.get::<super::vault::Vault>().await?.service.clone());
    // v1.0 and v1.1 builds hold no vault service (AC-6).
    #[cfg(not(feature = "vault"))]
    let vault = None;
    Ok(ApiServices {
        authenticator: identity.authenticator.clone(),
        principal_revalidator: identity.authenticator.clone(),
        sign_in: identity.sign_in.clone(),
        sessions: identity.sessions.clone(),
        device_grants: identity.device_grants.clone(),
        invites: identity.invites.clone(),
        tree: tree.service.clone(),
        tree_reader: tree.reader.clone(),
        uploads: transfer.service.clone(),
        commits: transfer.service.clone(),
        downloads: transfer.service.clone(),
        change_feed: feed.change_feed.clone(),
        shares: sharing.service.clone(),
        public_shares: sharing.service.clone(),
        admin: admin.admin.clone(),
        accounts: admin.accounts.clone(),
        account_deletion: admin.deletion.clone(),
        thumbnail_states: planning.thumbnails.clone(),
        derived: media.derived.clone(),
        worker_auth: media.worker_auth.clone(),
        worker_jobs: media.worker_jobs.clone(),
        vault,
        readiness: readiness.0.clone(),
        feed_shutdown: shutdown.feed.clone(),
        metrics: services.get::<Metrics>().await?,
        clock: services.get::<dyn Clock>().await?,
    })
}
