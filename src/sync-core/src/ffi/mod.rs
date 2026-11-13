//! The `uniffi` feature (`uniffi_surface.md`): the converters for types other crates own, the client handle
//! as a UniFFI object with `cancel()` as its only cancellation, the foreign implementations of the host traits,
//! and on Android the JNI entry point that initialises the system trust store. Records, enums and errors carry
//! their derives next to their definitions.

#[cfg(target_os = "android")]
mod android;
mod foreign;

use sc_config::Secret;
use sync_core_proto::{FileHash, Hash32, JsonValue};
use uuid::Uuid;

use crate::client::{HttpSyncClient, SyncApi};
use crate::negotiate::Limits;

pub use foreign::{
    Foreign, ForeignCheckpointStore, ForeignCredentialProvider, ForeignDownloadSink,
    ForeignFileSource, HostError,
};

uniffi::use_remote_type!(sync_core_proto::Uuid);
uniffi::use_remote_type!(sync_core_proto::Hash32);
uniffi::use_remote_type!(sync_core_proto::FileHash);
uniffi::use_remote_type!(sync_core_proto::JsonValue);

/// A token value. It crosses the boundary only inside `Credential`, `CredentialProvider::on_rotated` and
/// `SessionTokens`; the core never logs its foreign string form.
pub type SecretString = Secret<String>;

uniffi::custom_type!(SecretString, String, {
    remote,
    lower: |secret| secret.expose().clone(),
    try_lift: |text| Ok(Secret::new(text)),
});

#[uniffi::export]
impl HttpSyncClient {
    /// Ends every operation in flight with `Cancelled` within 1 s; nothing is sent afterwards. UniFFI has no
    /// cancellation of its own, so this is the only one (`sync_api.md` `cancel()`).
    #[uniffi::method(name = "cancel")]
    pub fn cancel_all(&self) {
        SyncApi::cancel(self);
    }

    /// What the server allows, after the clamps of `connect`.
    #[uniffi::method(name = "limits")]
    pub fn negotiated_limits(&self) -> Limits {
        self.limits()
    }
}
