//! The host side of the backup loop: `AssetSource`, implemented by Kotlin over MediaStore and by Swift over PhotoKit
//! (`backup_core.md` § `AssetSource`). Methods are synchronous host callbacks, like the traits of `sync-core`.

use std::sync::Arc;

use sync_core::FileSource;
use thiserror::Error;

/// Opaque bytes: the core stores and returns them, never interprets them. The empty token means "from the start".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeToken(pub Vec<u8>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetDescriptor {
    pub asset_id: i64,
    pub change_token: ChangeToken,
    pub display_name: String,
    pub mime: String,
    pub size: u64,
    pub date_taken: i64,
    pub relative_path: String,
}

/// The original of an asset is no longer there (deleted on the phone, or no longer granted).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("asset gone")]
pub struct AssetGone;

/// The readable original of an asset, as the file source sync-core hashes and uploads.
pub type ReadableFd = Arc<dyn FileSource>;

pub trait AssetSource: Send + Sync {
    /// Visible assets whose token is after `token`, oldest token first, at most `limit`; pending and trashed rows
    /// are left out until they are final. The empty token lists everything.
    fn assets_after(&self, token: &ChangeToken, limit: u32) -> Vec<AssetDescriptor>;

    fn current_token(&self) -> ChangeToken;

    /// Every asset visible now, with the same filters; page `0`, `1`, ... until an empty page.
    fn list_visible_ids(&self, page: u32) -> Vec<i64>;

    /// How many assets `list_visible_ids` lists. Read at the start of every run.
    fn visible_count(&self) -> u64;

    /// Descriptors of these ids, for the ids of a visible-id diff that no listing has returned; ids that are
    /// gone are left out.
    fn describe(&self, asset_ids: &[i64]) -> Vec<AssetDescriptor>;

    fn open_read(&self, asset_id: i64) -> Result<ReadableFd, AssetGone>;

    /// Read at the start of every run.
    fn access_level(&self) -> AccessLevel;
}
