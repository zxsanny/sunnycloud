//! Linking another provider identity to the signed-in account (E-15/c14, AC-55, AC-56) and the provider list
//! with the setup state (AC-19).
//!
//! A link attempt stores a sign-in state like `begin`, its hash bound to the browser and to the account of the
//! session that began it. The callback is validated as in `complete`; the identity row is then added to that
//! account by `(provider, provider_user_id)` only, never by email, and no email is stored on it. An identity
//! that already belongs to the account is an idempotent success (no row, no event); one that belongs to another
//! account is `IdentityTaken`. The mandatory `provider_linked` event is written in the link transaction.

use contracts::{
    AuthorizationRedirect, BrowserBinding, CallbackParams, Linked, Principal, ProviderInfo,
    ProviderList, SignInError,
};
use sc_db::{DbError, RunError, Scope};
use sc_ids::new_id;

use super::{IdentitySignIn, StateRequest, unexpected};
use crate::account_repo::{self, IDENTITY_KEY, NewIdentity};
use crate::audit_events::{self, TxError};
use crate::auth_repo;
use crate::config::ProviderConfig;

/// `begin_link` takes no return path (`contracts::SignInService`); the stored one is never used, since
/// `complete_link` answers `Linked` and the route decides where the browser goes.
const LINK_RETURN_TO: &str = "/";
/// Linking is a browser flow on a session.
const LINK_CLIENT_KIND: &str = "web";

/// The configured providers in configuration order, with the name a sign-in page shows.
pub(super) fn provider_list(providers: &[ProviderConfig]) -> Vec<ProviderInfo> {
    providers
        .iter()
        .map(|provider| ProviderInfo {
            key: provider.key.clone(),
            display_name: display_name(provider).to_owned(),
        })
        .collect()
}

fn display_name(provider: &ProviderConfig) -> &str {
    match provider.kind.as_str() {
        "github" => "GitHub",
        "oidc" => "OpenID Connect",
        _ => provider.key.as_str(),
    }
}

/// What the link transaction did.
enum LinkStep {
    Added,
    AlreadyLinked,
}

impl IdentitySignIn {
    pub async fn begin_link(
        &self,
        principal: &Principal,
        provider: &str,
    ) -> Result<AuthorizationRedirect, SignInError> {
        let client = self
            .providers
            .get(provider)
            .ok_or(SignInError::UnknownProvider)?;
        self.open_state(
            client.as_ref(),
            StateRequest {
                provider,
                return_to: LINK_RETURN_TO,
                client_kind: LINK_CLIENT_KIND,
                invite: None,
                code_challenge: None,
                link_account: Some(principal.account_id),
            },
        )
        .await
    }

    pub(super) async fn complete_link_step(
        &self,
        principal: &Principal,
        provider: &str,
        callback_params: CallbackParams,
        browser_binding: &BrowserBinding,
        host: &str,
    ) -> Result<Linked, SignInError> {
        let account = principal.account_id;
        let (_, identity) = self
            .callback(
                provider,
                &callback_params,
                browser_binding,
                host,
                Some(account),
            )
            .await?;
        let row = NewIdentity {
            id: new_id(self.clock.as_ref()),
            account_id: account,
            provider,
            provider_user_id: &identity.provider_user_id,
            email: None,
            created_at: self.clock.now_utc(),
        };
        let audit = &self.audit;
        let step = self
            .db
            .run(Scope::Account(account), async |tx| {
                if account_repo::owns_identity(tx, account, provider, row.provider_user_id).await? {
                    return Ok(LinkStep::AlreadyLinked);
                }
                account_repo::insert_identity(tx, &row).await?;
                let event = audit_events::provider_linked(principal, row.id);
                audit.record_in(tx, event).await?;
                Ok::<_, TxError>(LinkStep::Added)
            })
            .await;
        match step {
            Ok(LinkStep::Added | LinkStep::AlreadyLinked) => Ok(Linked),
            Err(RunError::Closure(TxError::Db(DbError::UniqueViolation {
                constraint: Some(constraint),
            }))) if constraint == IDENTITY_KEY => Err(SignInError::IdentityTaken),
            Err(other) => Err(unexpected(other)),
        }
    }

    /// The enabled providers in configured order, and `setup_required` exactly while no account exists.
    pub async fn list_providers(&self) -> Result<ProviderList, SignInError> {
        let any_account = auth_repo::any_account(&self.db).await.map_err(unexpected)?;
        Ok(ProviderList {
            providers: self.provider_list.clone(),
            setup_required: !any_account,
        })
    }
}

#[cfg(test)]
mod tests {
    use sc_config::Secret;

    use super::*;

    fn provider(key: &str, kind: &str) -> ProviderConfig {
        ProviderConfig {
            key: key.to_owned(),
            kind: kind.to_owned(),
            issuer: None,
            client_id: "client".to_owned(),
            client_secret: Secret::new("secret".to_owned()),
        }
    }

    #[test]
    fn the_list_is_the_configured_providers_in_configured_order() {
        // Arrange
        let configured = [provider("oidc", "oidc"), provider("github", "github")];

        // Act
        let list = provider_list(&configured);

        // Assert
        let keys: Vec<(&str, &str)> = list
            .iter()
            .map(|info| (info.key.as_str(), info.display_name.as_str()))
            .collect();
        assert_eq!(keys, vec![("oidc", "OpenID Connect"), ("github", "GitHub")]);
    }

    #[test]
    fn no_configured_provider_gives_an_empty_list() {
        // Arrange
        let configured: [ProviderConfig; 0] = [];

        // Act
        let list = provider_list(&configured);

        // Assert
        assert!(list.is_empty());
    }
}
