//! The `AssetSource` of a foreign host (`uniffi` feature), built the way sync-core builds its host traits: a foreign
//! twin whose methods answer `Result<_, HostError>`, so an exception of Kotlin or Swift arrives as a value and
//! never as a panic, and an adapter that makes it the Rust [`AssetSource`] of the loop.

use std::sync::Arc;

use sync_core::{Foreign, ForeignFileSource, HostError};
use tracing::warn;

use crate::source::{
    AccessLevel, AssetDescriptor, AssetSource, ChangeToken, OpenError, OpenedAsset, SourceError,
};

/// An opened original and whether the host could only read it with its location data removed.
#[derive(Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ForeignOpened {
    pub file: Arc<dyn ForeignFileSource>,
    pub location_redacted: bool,
}

/// `AssetSource` for a foreign host. `open_read` answers `NotFound` or `PermissionDenied` when the original is gone
/// (deleted, or no longer granted); any other failure is `Failed`.
#[uniffi::export(with_foreign)]
pub trait ForeignAssetSource: Send + Sync {
    fn assets_after(
        &self,
        token: ChangeToken,
        limit: u32,
    ) -> Result<Vec<AssetDescriptor>, HostError>;

    fn current_token(&self) -> Result<ChangeToken, HostError>;

    fn list_visible_ids(&self, page: u32) -> Result<Vec<i64>, HostError>;

    fn visible_count(&self) -> Result<u64, HostError>;

    fn describe(&self, asset_ids: Vec<i64>) -> Result<Vec<AssetDescriptor>, HostError>;

    fn open_read(&self, asset_id: i64) -> Result<ForeignOpened, HostError>;

    fn access_level(&self) -> Result<AccessLevel, HostError>;
}

/// The host error carries no text (it is an enum), so the cause is logged by its variant.
fn unanswered(call: &'static str, error: HostError) -> SourceError {
    warn!(call, ?error, "the asset source failed");
    SourceError
}

impl AssetSource for Foreign<dyn ForeignAssetSource> {
    fn assets_after(
        &self,
        token: &ChangeToken,
        limit: u32,
    ) -> Result<Vec<AssetDescriptor>, SourceError> {
        self.0
            .assets_after(token.clone(), limit)
            .map_err(|error| unanswered("assets_after", error))
    }

    fn current_token(&self) -> Result<ChangeToken, SourceError> {
        self.0
            .current_token()
            .map_err(|error| unanswered("current_token", error))
    }

    fn list_visible_ids(&self, page: u32) -> Result<Vec<i64>, SourceError> {
        self.0
            .list_visible_ids(page)
            .map_err(|error| unanswered("list_visible_ids", error))
    }

    fn visible_count(&self) -> Result<u64, SourceError> {
        self.0
            .visible_count()
            .map_err(|error| unanswered("visible_count", error))
    }

    fn describe(&self, asset_ids: &[i64]) -> Result<Vec<AssetDescriptor>, SourceError> {
        self.0
            .describe(asset_ids.to_vec())
            .map_err(|error| unanswered("describe", error))
    }

    fn open_read(&self, asset_id: i64) -> Result<OpenedAsset, OpenError> {
        match self.0.open_read(asset_id) {
            Ok(opened) => Ok(OpenedAsset {
                file: Arc::new(Foreign(opened.file)),
                location_redacted: opened.location_redacted,
            }),
            Err(HostError::NotFound | HostError::PermissionDenied) => Err(OpenError::Gone),
            Err(error @ HostError::Failed) => {
                warn!(?error, "the asset source could not open an original");
                Err(OpenError::Failed)
            }
        }
    }

    fn access_level(&self) -> Result<AccessLevel, SourceError> {
        self.0
            .access_level()
            .map_err(|error| unanswered("access_level", error))
    }
}
