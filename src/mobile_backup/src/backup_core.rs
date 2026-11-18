//! `BackupCore`, the one object Kotlin and Swift hold (`backup_core.md` § `BackupCore` methods). This part has the
//! server check, the status read and `cancel`; the run and the sign-in come with E-27/c08, which owns the credential
//! store a connected client needs.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use tracing::error;

use crate::backup::Backup;
use crate::checkpoint::{AssetStore, CheckpointDb};
use crate::error::BackupError;
use crate::server::{HttpProbe, ServerCheck, ServerProbe, ServerTrust, backup_error, sync_config};
use crate::status::BackupStatus;

const CHECKPOINT_FILE: &str = "checkpoint.db";

#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct BackupCore {
    db: Arc<CheckpointDb>,
    assets: AssetStore,
    client_version: String,
    probe: Arc<dyn ServerProbe>,
    /// The run loop of the signed-in session, once there is one.
    backup: Mutex<Option<Arc<Backup>>>,
}

impl BackupCore {
    /// `new` with the server check and the checkpoint given, for the host-free tests and for the code that builds the
    /// `Backup` of a session over the same checkpoint.
    pub fn with_probe(
        db: Arc<CheckpointDb>,
        client_version: String,
        probe: Arc<dyn ServerProbe>,
    ) -> Arc<Self> {
        Arc::new(Self {
            assets: AssetStore::new(Arc::clone(&db)),
            db,
            client_version,
            probe,
            backup: Mutex::new(None),
        })
    }

    /// The one checkpoint database of this process: the `Backup` built for a session shares it, so `status` and the
    /// loop read and write through the same connection.
    pub fn checkpoint(&self) -> Arc<CheckpointDb> {
        Arc::clone(&self.db)
    }

    /// Makes `backup` the run `cancel` reaches.
    pub fn attach(&self, backup: Arc<Backup>) {
        *self.backup.lock().unwrap_or_else(PoisonError::into_inner) = Some(backup);
    }
}

#[cfg_attr(feature = "uniffi", uniffi::export(async_runtime = "tokio"))]
impl BackupCore {
    /// `directory` is the app-private directory; `client_version` is `MAJOR.MINOR.PATCH`, as the server's minimum
    /// client version is compared with it.
    #[cfg_attr(feature = "uniffi", uniffi::constructor)]
    pub fn new(directory: String, client_version: String) -> Arc<Self> {
        let db = Arc::new(CheckpointDb::new(
            Path::new(&directory).join(CHECKPOINT_FILE),
        ));
        Self::with_probe(db, client_version, Arc::new(HttpProbe))
    }

    /// Checks `url` before it is used: `https` only (refused before any socket is opened), reachable, and a
    /// `server-info` this client accepts.
    pub async fn set_server(&self, url: String) -> Result<ServerCheck, BackupError> {
        let config =
            sync_config(&url, &self.client_version).ok_or(BackupError::ServerUnreachable)?;
        self.probe.check(config).await.map_err(backup_error)?;
        Ok(ServerCheck {
            trust: ServerTrust::System,
        })
    }

    pub fn status(&self) -> Result<BackupStatus, BackupError> {
        BackupStatus::read(&self.assets).map_err(|cause| {
            error!(error = %cause, "the status could not be read from the checkpoint");
            BackupError::CheckpointUnavailable
        })
    }

    /// Ends the run in progress `Partial` and its sync operations `Cancelled`; with no run it does nothing.
    pub fn cancel(&self) {
        let backup = self
            .backup
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(backup) = backup {
            backup.cancel();
        }
    }
}
