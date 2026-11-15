//! The composition root (E-22/c03): one lazy builder per component service on the [`ServiceContext`], each built
//! once on first use, and the late lists (account hooks, 03's commit effects, blob-owner cleanups, janitors, doctor
//! checks, health contributors, readiness checks) in their documented order.
//!
//! Wiring only: every builder calls owner constructors and hands over ports; no SQL, blob I/O, crypto or business
//! rule lives here. The vault service (20) is compiled only with feature `vault`. [`api_services`] assembles what
//! 11 receives from the same builders.

mod api;
mod base;
mod domain;
mod lists;
mod media;
mod storage;
#[cfg(feature = "vault")]
mod vault;

use std::sync::Arc;

use sc_clock::Clock;

use crate::context::ContextBuilder;
use crate::error::CliError;

pub use api::api_services;
pub use base::{DatabaseReady, Shutdown};
pub use domain::{Admin, Feed, Identity, Quota, Sharing, Transfer, Tree};
pub use lists::{
    AccountHooks, BlobCleanups, CommitEffects, DoctorChecks, HealthContributors, Janitors,
    ReadinessChecks, Registered,
};
pub use media::{Media, MediaPlanning};
pub use storage::Blobstore;
#[cfg(feature = "vault")]
pub use vault::Vault;

/// Registers every builder of the server. `clock` is the process clock, the same one the command log uses.
pub fn register(builder: ContextBuilder, clock: Arc<dyn Clock>) -> ContextBuilder {
    let builder = base::register(builder, clock);
    let builder = storage::register(builder);
    let builder = domain::register(builder);
    let builder = media::register(builder);
    #[cfg(feature = "vault")]
    let builder = vault::register(builder);
    lists::register(builder)
}

/// The builder of `service` failed with `source`.
pub(crate) fn failed(
    service: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> CliError {
    CliError::Build {
        service,
        source: Box::new(source),
    }
}
