//! Sign-in of the desktop: the device token store, the `CredentialProvider` sync-core reads it through, the
//! RFC 8628 device login, logout and the move to signed-out on revocation (14 §2, §5; task AZ-3908).

mod browser;
mod credentials;
mod login;
mod session;
pub mod token_store;

#[cfg(test)]
mod tests;

pub use browser::{BrowserLauncher, SystemBrowser};
pub use credentials::StoreCredentials;
pub use session::{LoginError, LoginStarted, LoginTiming, Session, SessionDeps, SignInServer};
