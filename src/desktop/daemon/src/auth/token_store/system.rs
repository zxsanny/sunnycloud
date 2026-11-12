#[cfg(target_os = "linux")]
use std::path::Path;
use std::sync::OnceLock;

use uuid::Uuid;

#[cfg(target_os = "linux")]
use super::{FILE_FALLBACK_WARNING, UserFileTokenStore, default_state_dir};
use super::{KeyringTokenStore, StoreLimits, Token, TokenStore, TokenStoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Keyring,
    /// No Secret Service: the user-only file on Linux, nothing elsewhere.
    NoSecretService,
}

/// The daemon's token store: the detection rule of 14 §5, decided at first use and kept for the process.
/// A Secret Service that answers selects the keyring even when it is locked or unusable (those calls are
/// `StoreUnavailable`); only a missing or silent one selects the Linux file, with the one WARN record.
pub struct SystemTokenStore {
    limits: StoreLimits,
    keyring: KeyringTokenStore,
    #[cfg(target_os = "linux")]
    file: Option<UserFileTokenStore>,
    choice: OnceLock<Choice>,
}

impl SystemTokenStore {
    /// The file fallback lives under [`default_state_dir`] on Linux.
    pub fn new(limits: StoreLimits) -> Self {
        Self {
            limits,
            keyring: KeyringTokenStore::new(limits),
            #[cfg(target_os = "linux")]
            file: default_state_dir().map(|dir| UserFileTokenStore::new(&dir)),
            choice: OnceLock::new(),
        }
    }

    #[cfg(target_os = "linux")]
    pub fn with_state_dir(limits: StoreLimits, state_dir: &Path) -> Self {
        Self {
            file: Some(UserFileTokenStore::new(state_dir)),
            ..Self::new(limits)
        }
    }

    fn choice(&self, account: Uuid) -> Choice {
        *self.choice.get_or_init(|| {
            if KeyringTokenStore::secret_service_answers(self.limits) {
                Choice::Keyring
            } else {
                self.warn_fallback(account);
                Choice::NoSecretService
            }
        })
    }

    #[cfg(target_os = "linux")]
    fn warn_fallback(&self, account: Uuid) {
        match &self.file {
            Some(file) => tracing::warn!(
                component = "token_store",
                path = %file.path_for(account).display(),
                "{FILE_FALLBACK_WARNING}"
            ),
            None => tracing::error!(
                component = "token_store",
                "no Secret Service and no absolute $XDG_STATE_HOME or $HOME for the token file"
            ),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn warn_fallback(&self, _account: Uuid) {
        tracing::error!(
            component = "token_store",
            "the OS credential store did not answer"
        );
    }

    #[cfg(target_os = "linux")]
    fn file(&self) -> Result<&UserFileTokenStore, TokenStoreError> {
        self.file
            .as_ref()
            .ok_or(TokenStoreError::Io(std::io::ErrorKind::NotFound))
    }

    /// After a keyring save: the copy written while the keyring was down is removed (14 §5 rule 3).
    #[cfg(target_os = "linux")]
    fn drop_file_copy(&self, account: Uuid) {
        if let Some(file) = &self.file
            && let Err(error) = file.delete(account)
        {
            tracing::warn!(
                component = "token_store",
                path = %file.path_for(account).display(),
                %error,
                "the fallback token file could not be removed after the keyring save"
            );
        }
    }
}

#[cfg(target_os = "linux")]
impl TokenStore for SystemTokenStore {
    fn load(&self, account: Uuid) -> Result<Option<Token>, TokenStoreError> {
        match self.choice(account) {
            Choice::Keyring => match self.keyring.load(account)? {
                Some(token) => Ok(Some(token)),
                None => match &self.file {
                    Some(file) => file.load(account),
                    None => Ok(None),
                },
            },
            Choice::NoSecretService => self.file()?.load(account),
        }
    }

    fn save(&self, account: Uuid, token: &Token) -> Result<(), TokenStoreError> {
        match self.choice(account) {
            Choice::Keyring => {
                self.keyring.save(account, token)?;
                self.drop_file_copy(account);
                Ok(())
            }
            Choice::NoSecretService => self.file()?.save(account, token),
        }
    }

    fn delete(&self, account: Uuid) -> Result<(), TokenStoreError> {
        if self.choice(account) == Choice::Keyring {
            self.keyring.delete(account)?;
        }
        match &self.file {
            Some(file) => file.delete(account),
            None => Ok(()),
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl TokenStore for SystemTokenStore {
    fn load(&self, account: Uuid) -> Result<Option<Token>, TokenStoreError> {
        self.keyring_only(account)?.load(account)
    }

    fn save(&self, account: Uuid, token: &Token) -> Result<(), TokenStoreError> {
        self.keyring_only(account)?.save(account, token)
    }

    fn delete(&self, account: Uuid) -> Result<(), TokenStoreError> {
        self.keyring_only(account)?.delete(account)
    }
}

#[cfg(not(target_os = "linux"))]
impl SystemTokenStore {
    fn keyring_only(&self, account: Uuid) -> Result<&KeyringTokenStore, TokenStoreError> {
        match self.choice(account) {
            Choice::Keyring => Ok(&self.keyring),
            Choice::NoSecretService => Err(TokenStoreError::StoreUnavailable),
        }
    }
}
