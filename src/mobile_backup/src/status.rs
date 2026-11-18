//! `BackupStatus`: what the checkpoint says about the backup now (`backup_core.md` § Types). It holds counts and
//! error texts only, so no token can reach it.

use crate::checkpoint::{ACCESS_LEVEL_KEY, AssetStore, DbError};
use crate::outcome::PauseReason;
use crate::source::AccessLevel;

/// Assets that ended without an upload, by reason. `failed` is the end state of an unexpected error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SkippedCounts {
    pub gone: u64,
    pub name: u64,
    pub size: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct LastError {
    pub message: String,
    /// The server's request id, or an id the log line of the same failure carries.
    pub correlation_id: String,
}

/// `visible = uploaded + pending + skipped.gone + skipped.name + skipped.size + skipped.failed` at every read.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BackupStatus {
    pub visible: u64,
    pub uploaded: u64,
    pub pending: u64,
    pub paused: Option<PauseReason>,
    pub skipped: SkippedCounts,
    pub last_error: Option<LastError>,
    /// `Full` until a run has read the host's access level.
    pub access_level: AccessLevel,
    /// The last original a run opened could only be read with its location data removed.
    pub location_redacted: bool,
}

impl BackupStatus {
    pub(crate) fn read(assets: &AssetStore) -> Result<Self, DbError> {
        let counts = assets.counts()?;
        let paused = assets
            .paused()?
            .map(|text| PauseReason::from_text(&text).ok_or(DbError::Malformed("pause reason")))
            .transpose()?;
        let access_level = assets
            .cursor_text(ACCESS_LEVEL_KEY)?
            .map(|text| AccessLevel::from_text(&text).ok_or(DbError::Malformed("access level")))
            .transpose()?
            .unwrap_or(AccessLevel::Full);
        Ok(Self {
            visible: counts.visible,
            uploaded: counts.uploaded,
            pending: counts.pending,
            paused,
            skipped: SkippedCounts {
                gone: counts.gone,
                name: counts.name,
                size: counts.size,
                failed: counts.failed,
            },
            last_error: assets.last_error()?.map(|stored| LastError {
                message: stored.message,
                correlation_id: stored.correlation_id,
            }),
            access_level,
            location_redacted: assets.location_redacted()?.unwrap_or(false),
        })
    }
}
