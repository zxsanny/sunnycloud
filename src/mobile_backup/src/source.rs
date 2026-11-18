//! The host side of the backup loop: `AssetSource`, implemented by Kotlin over MediaStore and by Swift over PhotoKit
//! (`backup_core.md` § `AssetSource`). Methods are synchronous host callbacks, like the traits of `sync-core`.

use std::sync::Arc;

use sync_core::FileSource;
use thiserror::Error;

/// Opaque bytes: the core stores and returns them, never interprets them. The empty token means "from the start".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeToken(pub Vec<u8>);

#[cfg(feature = "uniffi")]
uniffi::custom_type!(ChangeToken, Vec<u8>, {
    lower: |token| token.0,
    try_lift: |bytes| Ok(ChangeToken(bytes)),
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum AccessLevel {
    Full,
    Limited,
    Denied,
}

impl AccessLevel {
    pub(crate) fn as_text(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Limited => "limited",
            Self::Denied => "denied",
        }
    }

    pub(crate) fn from_text(text: &str) -> Option<Self> {
        [Self::Full, Self::Limited, Self::Denied]
            .into_iter()
            .find(|level| level.as_text() == text)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct AssetDescriptor {
    pub asset_id: i64,
    pub change_token: ChangeToken,
    pub display_name: String,
    pub mime: String,
    pub size: u64,
    pub date_taken: i64,
    pub relative_path: String,
}

/// Why the host could not open the original of an asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum OpenError {
    /// The original is no longer there (deleted on the phone, or no longer granted): the asset ends `skipped_gone`.
    #[error("asset gone")]
    Gone,
    /// The host failed for another reason: the asset ends `failed` and is tried again only after it changes.
    #[error("asset could not be opened")]
    Failed,
}

/// A host call that has no answer to give: the host threw, or its store is unusable. The run ends with this error
/// and every asset it stood on stays as it was; the cause is logged where the host error is mapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("asset source call failed")]
pub struct SourceError;

/// The readable original of an asset, as the file source sync-core hashes and uploads.
pub type ReadableFd = Arc<dyn FileSource>;

/// An opened original and whether the host could only get it with its location data removed (Android without
/// the media-location permission).
pub struct OpenedAsset {
    pub file: ReadableFd,
    pub location_redacted: bool,
}

pub trait AssetSource: Send + Sync {
    /// Visible assets whose token is after `token`, oldest token first, at most `limit`; pending and trashed rows
    /// are left out until they are final. The empty token lists everything.
    fn assets_after(
        &self,
        token: &ChangeToken,
        limit: u32,
    ) -> Result<Vec<AssetDescriptor>, SourceError>;

    fn current_token(&self) -> Result<ChangeToken, SourceError>;

    /// Every asset visible now, with the same filters; page `0`, `1`, ... until an empty page.
    fn list_visible_ids(&self, page: u32) -> Result<Vec<i64>, SourceError>;

    /// How many assets `list_visible_ids` lists. Read at the start of every run.
    fn visible_count(&self) -> Result<u64, SourceError>;

    /// Descriptors of these ids, for the ids of a visible-id diff that no listing has returned; ids that are
    /// gone are left out.
    fn describe(&self, asset_ids: &[i64]) -> Result<Vec<AssetDescriptor>, SourceError>;

    fn open_read(&self, asset_id: i64) -> Result<OpenedAsset, OpenError>;

    /// Read at the start of every run.
    fn access_level(&self) -> Result<AccessLevel, SourceError>;
}
