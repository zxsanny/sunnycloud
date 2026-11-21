//! `DevSeed` of 07 (`data_model.md` §13.2): a small demo installation, created only through the service ports,
//! so every invariant of the real paths holds. The accounts come from the normal sign-up path (owner decision
//! 2026-11-14): the setup token for the admin and admin invites for the users, each redeemed by a sign-in at
//! the development identity provider. Production is refused before any port is called.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use contracts::{
    AccountFilter, Authenticator, BeginSignIn, CallbackParams, ClientKind, CommitService,
    Credential, DevSeed, DevSeedError, Fault, InviteService, OperatorIdentityOps, Page,
    ShareService, SignInOutcome, SignInService, TreeReader, TreeService, UploadService,
    async_trait,
};
use sc_clock::Clock;
use sc_config::Environment;
use sc_ids::RawToken;
use thiserror::Error;
use uuid::Uuid;

use crate::AdminServiceImpl;

/// The seed's sign-ins come from the machine itself.
const SEED_ADDRESS: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const RETURN_TO: &str = "/";
const INVITE_NOTE: &str = "dev-seed";
/// The provider subjects of the seeded accounts; the identity provider signs each in under its subject.
pub const ADMIN_SUBJECT: &str = "dev-seed-admin";
pub const USER_SUBJECTS: [&str; 2] = ["dev-seed-user-1", "dev-seed-user-2"];

/// The user's step at the identity provider: signs in as `subject` at `authorization_url` and returns the
/// parameters the provider redirects back with. In development this is the mock identity provider.
#[async_trait]
pub trait ProviderLogin: Send + Sync {
    async fn authorize(
        &self,
        authorization_url: &str,
        subject: &str,
    ) -> Result<CallbackParams, Fault>;
}

/// Where the seed signs in: the configured provider key and the public host the sign-in checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevSeedTarget {
    pub provider: String,
    pub host: String,
}

/// The ports beyond 07's own, passed by the composition root to `DevSeed` only.
pub struct DevSeedPorts {
    pub operator: Arc<dyn OperatorIdentityOps>,
    pub invites: Arc<dyn InviteService>,
    pub sign_in: Arc<dyn SignInService>,
    pub provider_login: Arc<dyn ProviderLogin>,
    pub authenticator: Arc<dyn Authenticator>,
    pub tree: Arc<dyn TreeService>,
    pub tree_reader: Arc<dyn TreeReader>,
    pub upload: Arc<dyn UploadService>,
    pub commit: Arc<dyn CommitService>,
    pub shares: Arc<dyn ShareService>,
}

pub struct DevSeedImpl {
    pub(crate) environment: Environment,
    pub(crate) target: DevSeedTarget,
    pub(crate) admin: Arc<AdminServiceImpl>,
    pub(crate) ports: DevSeedPorts,
    pub(crate) clock: Arc<dyn Clock>,
}

/// A seed step whose port answered with a shape the seed cannot continue from.
#[derive(Debug, Error)]
#[error("dev seed step failed: {0}")]
pub(crate) struct SeedStepFailed(pub(crate) &'static str);

pub(crate) fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> DevSeedError {
    DevSeedError::Unexpected(Box::new(error))
}

/// The token a sign-up presents.
enum Redeem {
    Setup(RawToken),
    Invite(RawToken),
}

/// The refusal comes before any port is called, so production stays untouched.
fn refuses(environment: Environment) -> bool {
    environment == Environment::Production
}

impl DevSeedImpl {
    pub fn new(
        environment: Environment,
        target: DevSeedTarget,
        admin: Arc<AdminServiceImpl>,
        ports: DevSeedPorts,
        clock: Arc<dyn Clock>,
    ) -> Self {
        DevSeedImpl {
            environment,
            target,
            admin,
            ports,
            clock,
        }
    }

    /// Any account means the installation is seeded or in real use; the seed then changes nothing.
    async fn has_accounts(&self) -> Result<bool, DevSeedError> {
        let first = self
            .admin
            .accounts()
            .list_accounts(
                AccountFilter::default(),
                Page {
                    offset: 0,
                    limit: 1,
                },
            )
            .await
            .map_err(unexpected)?;
        Ok(!first.is_empty())
    }

    /// Begin, the provider step and complete, as a browser would run them; returns the new account's id.
    async fn sign_up(&self, subject: &str, token: Redeem) -> Result<Uuid, DevSeedError> {
        let (invite_token, setup_token) = match token {
            Redeem::Setup(token) => (None, Some(token)),
            Redeem::Invite(token) => (Some(token), None),
        };
        let provider = self.target.provider.as_str();
        let redirect = self
            .ports
            .sign_in
            .begin(BeginSignIn {
                provider: provider.to_owned(),
                return_to: RETURN_TO.to_owned(),
                client_kind: ClientKind::Web,
                invite_token,
                setup_token,
                code_challenge: None,
                host: self.target.host.clone(),
                client_ip: SEED_ADDRESS,
            })
            .await
            .map_err(unexpected)?;
        let callback = self
            .ports
            .provider_login
            .authorize(&redirect.url, subject)
            .await
            .map_err(DevSeedError::Unexpected)?;
        let outcome = self
            .ports
            .sign_in
            .complete(
                provider,
                callback,
                &redirect.browser_binding,
                &self.target.host,
                SEED_ADDRESS,
            )
            .await
            .map_err(unexpected)?;
        let SignInOutcome::Session { tokens, .. } = outcome else {
            return Err(unexpected(SeedStepFailed(
                "a web sign-in returned no session",
            )));
        };
        let principal = self
            .ports
            .authenticator
            .authenticate(Credential::Session(tokens.access))
            .await
            .map_err(unexpected)?;
        Ok(principal.account_id)
    }

    /// The admin from the setup token, then one admin invite per user. `None`: an account appeared since the
    /// check, so the installation is not empty and nothing is created.
    async fn accounts(&self) -> Result<Option<(Uuid, Vec<Uuid>)>, DevSeedError> {
        let Some(setup) = self
            .ports
            .operator
            .ensure_setup_token()
            .await
            .map_err(unexpected)?
        else {
            return Ok(None);
        };
        let admin = self.sign_up(ADMIN_SUBJECT, Redeem::Setup(setup)).await?;
        let mut users = Vec::with_capacity(USER_SUBJECTS.len());
        for subject in USER_SUBJECTS {
            let invite = self
                .ports
                .invites
                .create_invite(admin, INVITE_NOTE)
                .await
                .map_err(unexpected)?;
            users.push(
                self.sign_up(subject, Redeem::Invite(invite.token.0))
                    .await?,
            );
        }
        Ok(Some((admin, users)))
    }
}

#[async_trait]
impl DevSeed for DevSeedImpl {
    /// Refused in production before any call. An installation that already has an account is left as it is,
    /// so a second run adds nothing.
    async fn seed(&self) -> Result<(), DevSeedError> {
        if refuses(self.environment) {
            return Err(DevSeedError::RefusedInProduction);
        }
        if self.has_accounts().await? {
            return Ok(());
        }
        let Some((_admin, users)) = self.accounts().await? else {
            return Ok(());
        };
        for (index, user) in users.iter().enumerate() {
            self.content(*user, index).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_production_is_refused() {
        // Arrange
        let environments = [
            Environment::Production,
            Environment::Development,
            Environment::Test,
        ];

        // Act
        let refused = environments.map(refuses);

        // Assert
        assert_eq!(refused, [true, false, false]);
    }
}
