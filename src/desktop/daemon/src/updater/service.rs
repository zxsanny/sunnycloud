//! The update check: fetch from the fixed source, verify, download, stage, record. Under the owner's option A
//! (2026-11-12) a staged package is only shown as "update ready"; the user opens it and the OS installer
//! applies it. The daemon never installs, never restarts and never replaces its own binary.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ipc_schema::UpdateView;
use sc_clock::Clock;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::fetch::{Payload, ReleaseClient};
use super::manifest::{MANIFEST_NAME, SIGNATURE_NAME, Version, platform_key};
use super::redirect::RedirectRule;
use super::verify::{Rejection, Verified, VerifyContext, verify_manifest};
use crate::local_index::{IndexError, LocalIndexStore};

const SERIAL_KEY: &str = "updater.serial";
const STAGED_KEY: &str = "updater.staged";
pub const STAGING_DIR_NAME: &str = "updates";
const PARTIAL_PREFIX: &str = ".partial-";

/// Timing of the updater. The caller binds these from configuration (`desktop.update_check_interval`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdaterConfig {
    pub check_interval: Duration,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(24 * 3600),
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(300),
        }
    }
}

/// What the build is: the release source, the hosts a download may be redirected to and the installed
/// version. All are compile-time values in a release build; there is no run-time input for them, nor for
/// the pinned keys.
#[derive(Debug, Clone)]
pub struct Build {
    pub source: String,
    pub redirects: RedirectRule,
    pub installed: Version,
}

impl Build {
    #[cfg(not(any(test, feature = "test-update-trust")))]
    fn compiled() -> Result<Self, UpdaterError> {
        Ok(Self {
            source: super::trust::RELEASE_SOURCE.to_owned(),
            redirects: RedirectRule::release(),
            installed: Version::parse(env!("CARGO_PKG_VERSION"))
                .ok_or(UpdaterError::InstalledVersion)?,
        })
    }
}

/// A package ready for the user to open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedUpdate {
    pub version: String,
    pub serial: u64,
    /// The pinned key that verified the manifest: `active` or `recovery`.
    pub key: String,
    pub path: PathBuf,
}

#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("the installed version is not MAJOR.MINOR.PATCH")]
    InstalledVersion,
    #[error("http client setup failed: {0}")]
    Client(String),
    #[error("update rejected by the {check} check: {rejection}", check = .0.check(), rejection = .0)]
    Rejected(Rejection),
    #[error("staging failed: {0}")]
    Stage(#[from] io::Error),
    #[error("local index: {0}")]
    Index(#[from] IndexError),
}

impl UpdaterError {
    /// The check named in the WARN line.
    pub fn check(&self) -> &'static str {
        match self {
            Self::Rejected(rejection) => rejection.check(),
            Self::InstalledVersion | Self::Client(_) => "setup",
            Self::Stage(_) | Self::Index(_) => "stage",
        }
    }
}

pub struct Updater {
    build: Build,
    client: ReleaseClient,
    index: Arc<LocalIndexStore>,
    staging_dir: PathBuf,
    clock: Arc<dyn Clock>,
    config: UpdaterConfig,
}

impl Updater {
    /// The release updater: compiled-in source and keys, nothing else.
    #[cfg(not(any(test, feature = "test-update-trust")))]
    pub fn new(
        index: Arc<LocalIndexStore>,
        data_dir: &Path,
        clock: Arc<dyn Clock>,
        config: UpdaterConfig,
    ) -> Result<Self, UpdaterError> {
        Self::with_build(Build::compiled()?, index, data_dir, clock, config)
    }

    /// Test build only: local hosts stand in for the release source and its redirect hosts. The keys stay
    /// pinned.
    #[cfg(any(test, feature = "test-update-trust"))]
    pub fn for_test_build(
        build: Build,
        index: Arc<LocalIndexStore>,
        data_dir: &Path,
        clock: Arc<dyn Clock>,
        config: UpdaterConfig,
    ) -> Result<Self, UpdaterError> {
        Self::with_build(build, index, data_dir, clock, config)
    }

    fn with_build(
        build: Build,
        index: Arc<LocalIndexStore>,
        data_dir: &Path,
        clock: Arc<dyn Clock>,
        config: UpdaterConfig,
    ) -> Result<Self, UpdaterError> {
        let client = ReleaseClient::new(
            config.connect_timeout,
            config.request_timeout,
            build.redirects.clone(),
        )
        .map_err(UpdaterError::Client)?;
        Ok(Self {
            build,
            client,
            index,
            staging_dir: data_dir.join(STAGING_DIR_NAME),
            clock,
            config,
        })
    }

    /// Checks every `check_interval` until the clock's shutdown token is cancelled.
    pub async fn run(&self) {
        loop {
            let staged = self.check_once().await.is_ok();
            tracing::debug!(staged, "update check finished");
            let Some(next) = self
                .clock
                .monotonic()
                .checked_add(self.config.check_interval)
            else {
                return;
            };
            if self.clock.sleep_until_monotonic(next).await.is_err() {
                return;
            }
        }
    }

    /// One check. On success the payload is staged, the serial stored and INFO "update staged" logged; on any
    /// failure nothing is staged, the stored serial is unchanged and a WARN names the failed check.
    pub async fn check_once(&self) -> Result<StagedUpdate, UpdaterError> {
        let outcome = self.try_check().await;
        match &outcome {
            Ok(staged) => tracing::info!(
                version = %staged.version,
                serial = staged.serial,
                key = staged.key,
                "update staged"
            ),
            Err(error) => tracing::warn!(check = error.check(), %error, "update not staged"),
        }
        outcome
    }

    async fn try_check(&self) -> Result<StagedUpdate, UpdaterError> {
        let base = self.build.source.trim_end_matches('/');
        let latest = format!("{base}/latest/download");
        let manifest = self
            .client
            .get_small(&format!("{latest}/{MANIFEST_NAME}"))
            .await
            .map_err(UpdaterError::Rejected)?
            .ok_or_else(|| UpdaterError::Rejected(Rejection::Source("no manifest".to_owned())))?;
        let signature = self
            .client
            .get_small(&format!("{latest}/{SIGNATURE_NAME}"))
            .await
            .map_err(UpdaterError::Rejected)?;
        let platform = platform_key();
        let context = VerifyContext {
            now_unix_micros: self.clock.now_utc().as_unix_micros(),
            accepted_serial: self.accepted_serial()?,
            installed: self.build.installed,
            platform: &platform,
        };
        let verified = verify_manifest(&manifest, signature.as_deref(), &context)
            .map_err(UpdaterError::Rejected)?;
        let url = format!(
            "{base}/download/v{}/{}",
            verified.version, verified.entry.asset
        );
        let payload = self
            .client
            .download(&url, verified.entry.size, &verified.sha256)
            .await
            .map_err(UpdaterError::Rejected)?;
        self.stage(&verified, payload)
    }

    /// The staged package while it is newer than the installed build and still on disk.
    pub fn staged(&self) -> Result<Option<StagedUpdate>, UpdaterError> {
        let Some(text) = self.index.meta_get(STAGED_KEY)? else {
            return Ok(None);
        };
        let staged: StagedUpdate =
            serde_json::from_str(&text).map_err(|_| IndexError::IndexCorrupt)?;
        let newer = Version::parse(&staged.version).is_some_and(|v| v > self.build.installed);
        Ok((newer && staged.path.is_file()).then_some(staged))
    }

    /// `DaemonStatus.update_available`.
    pub fn update_view(&self) -> Result<Option<UpdateView>, UpdaterError> {
        Ok(self.staged()?.map(|staged| UpdateView {
            version: staged.version,
        }))
    }

    /// The highest accepted serial; a stored value this daemon could not have written is a damaged index.
    pub fn accepted_serial(&self) -> Result<Option<u64>, UpdaterError> {
        self.index
            .meta_get(SERIAL_KEY)?
            .map(|text| text.parse().map_err(|_| IndexError::IndexCorrupt.into()))
            .transpose()
    }

    fn stage(&self, verified: &Verified, payload: Payload) -> Result<StagedUpdate, UpdaterError> {
        let path = write_staged(&self.staging_dir, &verified.entry.asset, &payload.0)?;
        let staged = StagedUpdate {
            version: verified.version.to_string(),
            serial: verified.serial,
            key: verified.slot.as_str().to_owned(),
            path,
        };
        let record = serde_json::to_string(&staged).map_err(io::Error::other)?;
        self.index.meta_set(STAGED_KEY, &record)?;
        self.index
            .meta_set(SERIAL_KEY, &verified.serial.to_string())?;
        Ok(staged)
    }
}

/// Writes `name` in `dir` through a temporary file and a rename, then removes any older staged package.
fn write_staged(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let partial = dir.join(format!("{PARTIAL_PREFIX}{}", Uuid::new_v4()));
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        });
    let target = dir.join(name);
    if let Err(error) = written.and_then(|()| fs::rename(&partial, &target)) {
        if let Err(cleanup) = fs::remove_file(&partial)
            && cleanup.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(error = %cleanup, "cannot remove a partial update file");
        }
        return Err(error);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path() != target {
            remove_stale(&entry.path())?;
        }
    }
    Ok(target)
}

fn remove_stale(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}
