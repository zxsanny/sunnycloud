//! `set_server`: the address check of `BackupCore` (AC-54). sync-core owns the rules: it refuses `http://` before it
//! opens a socket (unless the debug-only feature `allow-plain-http` is on), then reads and range-checks
//! `GET /api/v1/server-info`. This module only maps its answer to the contract's types.

use std::sync::Arc;

use sc_config::{ClientSettings, ConfigSources, load};
use sync_core::{
    BoxFuture, ConnectError, Credential, CredentialProvider, CredentialUnavailable, HttpSyncClient,
    PersistFailed, SyncConfig,
};
use tracing::{error, warn};

use crate::error::BackupError;

/// Who vouches for the server's certificate. `UserAdded` is filled by E-27/c12 (`TlsTrustBridge`); until then every
/// checked server reads `System`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ServerTrust {
    System,
    UserAdded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ServerCheck {
    pub trust: ServerTrust,
}

/// `HttpSyncClient::connect` is an associated function, so the one-shot check goes through this seam, as the run
/// loop's calls go through `BackupApi`. `HttpProbe` is the one production implementation.
pub trait ServerProbe: Send + Sync {
    fn check(&self, config: SyncConfig) -> BoxFuture<'_, Result<(), ConnectError>>;
}

/// `server-info` is public, so the client that checks it holds no credential.
struct NoCredential;

impl CredentialProvider for NoCredential {
    fn bearer(&self) -> Result<Credential, CredentialUnavailable> {
        Err(CredentialUnavailable)
    }

    fn on_rotated(
        &self,
        _access: sc_config::Secret<String>,
        _refresh: sc_config::Secret<String>,
    ) -> Result<(), PersistFailed> {
        Err(PersistFailed)
    }
}

pub struct HttpProbe;

impl ServerProbe for HttpProbe {
    fn check(&self, config: SyncConfig) -> BoxFuture<'_, Result<(), ConnectError>> {
        Box::pin(async move {
            HttpSyncClient::connect(config, Arc::new(NoCredential))
                .await
                .map(|_| ())
        })
    }
}

/// The `sync` section for `url`: the defaults of the shared key table with the three keys the build injects.
pub(crate) fn sync_config(url: &str, client_version: &str) -> Option<SyncConfig> {
    let mut sources = ConfigSources::default();
    for (name, value) in [
        ("SUNNYCLOUD_SYNC_SERVER_URL", url),
        ("SUNNYCLOUD_SYNC_CLIENT_FAMILY", "android"),
        ("SUNNYCLOUD_SYNC_CLIENT_VERSION", client_version),
    ] {
        sources.env.insert(name.to_owned(), value.to_owned());
    }
    let config =
        load::<ClientSettings>(&sources).and_then(|settings| SyncConfig::from_settings(&settings));
    match config {
        Ok(config) => Some(config),
        Err(failure) => {
            error!(error = %failure, "the sync settings of set_server did not load");
            None
        }
    }
}

/// Every refusal of `connect` and the typed error the host gets. Errors the host cannot act on differently (the
/// server's answer is outside what this client accepts, a local setting is wrong) read as unreachable; the cause
/// is logged by sync-core and here.
pub(crate) fn backup_error(error: ConnectError) -> BackupError {
    match error {
        ConnectError::InsecureScheme => BackupError::HttpsRequired,
        ConnectError::UpgradeRequired { message, .. } => BackupError::UpgradeRequired { message },
        ConnectError::InvalidConfig {
            field: "server_url",
        } => BackupError::InvalidServerUrl,
        ConnectError::Unreachable => BackupError::ServerUnreachable,
        ConnectError::ProtocolMismatch { .. }
        | ConnectError::Unauthorized
        | ConnectError::InvalidConfig { .. }
        | ConnectError::TrustStoreUninitialized => {
            warn!(%error, "the server check failed in a way the host cannot fix");
            BackupError::ServerUnreachable
        }
    }
}

#[cfg(test)]
mod tests;
