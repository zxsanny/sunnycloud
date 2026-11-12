//! Where the device token lives (14 §2 `TokenStore`, §5 "Token store"): the OS keyring, or on Linux
//! without a Secret Service a user-only file. Nothing else ever holds the token at rest.

mod bounded;
#[cfg(target_os = "linux")]
mod file_store;
mod keyring_store;
mod system;

use std::time::Duration;

use sc_config::Secret;
use thiserror::Error;
use uuid::Uuid;

pub(crate) use bounded::run_bounded;
#[cfg(target_os = "linux")]
pub use file_store::{UserFileTokenStore, default_state_dir};
pub use keyring_store::KeyringTokenStore;
pub use system::SystemTokenStore;

/// The device token of one account.
pub type Token = Secret<String>;

/// Keyring service name; the keyring user is the account id.
pub const KEYRING_SERVICE: &str = "sunnycloud";

/// The fixed text of the one fallback WARN record (14 §9, G-12).
pub const FILE_FALLBACK_WARNING: &str = "No Secret Service keyring is available; the device token is stored in a user-only file instead";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TokenStoreError {
    /// The keyring is present but locked, has no default collection, or a call outlived its bound.
    #[error("the keyring is unavailable; unlock the keyring and retry")]
    StoreUnavailable,
    /// The token file or its directory is readable or accessible by other users, or owned by someone else.
    #[error("the token file or its directory is accessible by other users")]
    PermissionTooOpen,
    /// The token file could not be read or written.
    #[error("token file i/o failed ({0:?})")]
    Io(std::io::ErrorKind),
}

/// The time bounds of the keyring calls (14 §5 detection rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreLimits {
    /// Bound of the first `store_status` probe; past it there is no Secret Service.
    pub detect_bound: Duration,
    /// Bound of every keyring call; past it an unlock prompt is waiting and the call is abandoned.
    pub call_bound: Duration,
}

impl Default for StoreLimits {
    fn default() -> Self {
        Self {
            detect_bound: Duration::from_secs(5),
            call_bound: Duration::from_secs(30),
        }
    }
}

/// Blocking calls: each is bounded by [`StoreLimits`], so an async caller runs them on a blocking thread.
pub trait TokenStore: Send + Sync {
    fn load(&self, account: Uuid) -> Result<Option<Token>, TokenStoreError>;

    fn save(&self, account: Uuid, token: &Token) -> Result<(), TokenStoreError>;

    /// Removes every local copy; deleting a missing token is `Ok`.
    fn delete(&self, account: Uuid) -> Result<(), TokenStoreError>;
}
