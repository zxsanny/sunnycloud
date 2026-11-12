use keyring::{Entry, Error as KeyringError};
use uuid::Uuid;

use super::{KEYRING_SERVICE, StoreLimits, Token, TokenStore, TokenStoreError, run_bounded};

/// The OS keyring through keyring 4.2.0 v1 mode: one entry `sunnycloud/<account id>`. Every call runs on its
/// own worker thread within `StoreLimits::call_bound`; any failure other than a missing entry is
/// `StoreUnavailable`, and the next call tries the keyring again.
#[derive(Debug, Clone, Copy)]
pub struct KeyringTokenStore {
    limits: StoreLimits,
}

impl KeyringTokenStore {
    pub fn new(limits: StoreLimits) -> Self {
        Self { limits }
    }

    /// The detection probe (14 §5): `Entry::store_status()` within `detect_bound`. `false` means no Secret
    /// Service answered (no bus, no service, or no answer in time). v1 mode keeps the first answer for the life
    /// of the process.
    pub fn secret_service_answers(limits: StoreLimits) -> bool {
        run_bounded("detect", limits.detect_bound, || {
            Entry::store_status().is_ok()
        })
        .unwrap_or(false)
    }

    fn call<T, F>(&self, name: &'static str, account: Uuid, op: F) -> Result<T, TokenStoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Entry) -> keyring::Result<T> + Send + 'static,
    {
        let answer = run_bounded(name, self.limits.call_bound, move || {
            Entry::new(KEYRING_SERVICE, &account.to_string()).and_then(|entry| op(&entry))
        })
        .map_err(|_| TokenStoreError::StoreUnavailable)?;
        answer.map_err(|error| {
            tracing::warn!(
                component = "token_store",
                call = name,
                error = kind_name(&error),
                "keyring call failed"
            );
            TokenStoreError::StoreUnavailable
        })
    }
}

impl TokenStore for KeyringTokenStore {
    fn load(&self, account: Uuid) -> Result<Option<Token>, TokenStoreError> {
        self.call("load", account, |entry| match entry.get_password() {
            Ok(token) => Ok(Some(Token::new(token))),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(error),
        })
    }

    fn save(&self, account: Uuid, token: &Token) -> Result<(), TokenStoreError> {
        let token = token.expose().clone();
        self.call("save", account, move |entry| entry.set_password(&token))
    }

    fn delete(&self, account: Uuid) -> Result<(), TokenStoreError> {
        self.call("delete", account, |entry| match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(error),
        })
    }
}

/// The variant only: some keyring errors carry the stored bytes, so their text is never logged.
fn kind_name(error: &KeyringError) -> &'static str {
    match error {
        KeyringError::PlatformFailure(_) => "PlatformFailure",
        KeyringError::NoStorageAccess(_) => "NoStorageAccess",
        KeyringError::NoEntry => "NoEntry",
        KeyringError::BadEncoding(_) => "BadEncoding",
        KeyringError::BadDataFormat(..) => "BadDataFormat",
        KeyringError::BadStoreFormat(_) => "BadStoreFormat",
        KeyringError::TooLong(..) => "TooLong",
        KeyringError::Invalid(..) => "Invalid",
        KeyringError::Ambiguous(_) => "Ambiguous",
        KeyringError::NoDefaultStore => "NoDefaultStore",
        KeyringError::NotSupportedByStore(_) => "NotSupportedByStore",
        _ => "Other",
    }
}
