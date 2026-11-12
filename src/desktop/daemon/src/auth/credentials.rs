use std::sync::Arc;

use sc_config::Secret;
use sync_core::{Credential, CredentialProvider, PersistFailed};
use uuid::Uuid;

use super::token_store::TokenStore;

/// sync-core's credential source: reads the device token through the store for every request and keeps no copy.
///
/// `bearer` cannot fail (13 host traits), so a missing or unreadable token is sent as an empty bearer: the server
/// answers 401, sync-core reports `Unauthorized`, and the session moves to signed-out.
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
    fn bearer(&self) -> Credential {
        let access = match self.store.load(self.account) {
            Ok(Some(token)) => token,
            Ok(None) => {
                tracing::warn!(component = "token_store", "no device token is stored");
                Secret::new(String::new())
            }
            Err(error) => {
                tracing::error!(component = "token_store", %error, "the device token could not be read");
                Secret::new(String::new())
            }
        };
        Credential {
            access,
            refresh: None,
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
