use std::sync::Arc;

use sc_config::Secret;
use sync_core::{Credential, CredentialProvider, CredentialUnavailable, PersistFailed};
use uuid::Uuid;

use super::token_store::TokenStore;

/// sync-core's credential source: reads the device token through the store for every request and keeps no copy.
///
/// A missing or unreadable token is `CredentialUnavailable`: sync-core sends nothing and the session stays signed
/// in, so a locked keyring pauses sync and never signs the user out (AZ-4231, owner decision 2026-11-12).
pub struct StoreCredentials {
    store: Arc<dyn TokenStore>,
    account: Uuid,
}

impl StoreCredentials {
    pub fn new(store: Arc<dyn TokenStore>, account: Uuid) -> Self {
        Self { store, account }
    }
}

impl CredentialProvider for StoreCredentials {
    fn bearer(&self) -> Result<Credential, CredentialUnavailable> {
        match self.store.load(self.account) {
            Ok(Some(access)) => Ok(Credential {
                access,
                refresh: None,
            }),
            Ok(None) => {
                tracing::warn!(component = "token_store", "no device token is stored");
                Err(CredentialUnavailable)
            }
            Err(error) => {
                tracing::error!(component = "token_store", %error, "the device token could not be read");
                Err(CredentialUnavailable)
            }
        }
    }

    /// Desktop device tokens never rotate (v1.0); a rotated pair has nowhere to go.
    fn on_rotated(
        &self,
        _access: Secret<String>,
        _refresh: Secret<String>,
    ) -> Result<(), PersistFailed> {
        tracing::error!(
            component = "token_store",
            "a token rotation reached the desktop daemon, which holds a device token only"
        );
        Err(PersistFailed)
    }
}
