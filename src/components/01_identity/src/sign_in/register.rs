//! The registration branch of `complete`: an unknown identity that presented a token redeems it in one short
//! transaction (`registration_repo`). A member or operator admin invite, or the setup token while no account
//! exists, creates an account and runs every registered `AccountLifecycleHook::initialize` (01 §2; contract
//! `account_creation_hooks.md`); a re-bind invite binds the identity to its existing account and runs no hook
//! (contract `operator_identity_ops.md`). The provider exchange has finished before the transaction opens.

use std::sync::Arc;

use contracts::{AccountLifecycleHook, SignInError};
use sc_db::{DbError, RunError};
use sc_ids::new_id;

use super::{ACTIVE, IdentitySignIn, unexpected};
use crate::account_repo::{IDENTITY_KEY, NewIdentity};
use crate::auth_repo::{self, KnownIdentity};
use crate::error::{HooksAlreadyRegistered, IdentityError};
use crate::provider::ProviderIdentity;
use crate::registration_repo::{CreateError, NewRegistration, Redeemed};

/// The display name of a new account whose provider sent neither a name nor a username.
const DEFAULT_DISPLAY_NAME: &str = "User";

impl IdentitySignIn {
    /// Sets the account-creation hook list, once. Called by the composition root (12) only, before the API
    /// accepts sign-in requests; a second call is refused and the first list stays.
    pub fn register_hooks(
        &self,
        hooks: Vec<Arc<dyn AccountLifecycleHook>>,
    ) -> Result<(), HooksAlreadyRegistered> {
        self.hooks.set(hooks).map_err(|_| HooksAlreadyRegistered)
    }

    /// Redeems for `identity` the token that hashes to `invite_token_hash`: the created or re-bound account. No
    /// token, an unknown, expired or already redeemed one, a setup token while an account exists, a lost race,
    /// and (with `email_allowlist` set) an email that is missing or not on the list are all
    /// `InvitationRequired`, with nothing written.
    pub(super) async fn register(
        &self,
        provider: &str,
        identity: &ProviderIdentity,
        invite_token_hash: Option<&[u8]>,
    ) -> Result<KnownIdentity, SignInError> {
        let invite_token_hash = invite_token_hash.ok_or(SignInError::InvitationRequired)?;
        if !self.email_allowed(identity.email.as_deref()) {
            return Err(SignInError::InvitationRequired);
        }
        let hooks = self.hooks.get().map(Vec::as_slice);
        let now = self.clock.now_utc();
        let (account_id, identity_id) = (new_id(self.clock.as_ref()), new_id(self.clock.as_ref()));
        let registration = NewRegistration {
            invite_token_hash,
            account_id,
            display_name: identity
                .display_name
                .as_deref()
                .unwrap_or(DEFAULT_DISPLAY_NAME),
            identity: NewIdentity {
                id: identity_id,
                account_id,
                provider,
                provider_user_id: &identity.provider_user_id,
                email: identity.email.as_deref(),
                created_at: now,
            },
            now,
        };
        match auth_repo::redeem_invite(&self.db, &registration, hooks, &self.audit).await {
            Ok(Redeemed::Created) => Ok(KnownIdentity {
                identity_id,
                account_id,
                account_state: ACTIVE.to_owned(),
            }),
            Ok(Redeemed::Rebound { account, state }) => Ok(KnownIdentity {
                identity_id,
                account_id: account,
                account_state: state,
            }),
            Err(RunError::Closure(CreateError::HooksNotRegistered)) => {
                tracing::error!("account_hooks_not_registered");
                Err(unexpected(IdentityError::HooksNotRegistered))
            }
            Err(RunError::Closure(CreateError::InviteNotRedeemable)) => {
                Err(SignInError::InvitationRequired)
            }
            // The same provider identity redeemed concurrently (two tabs): this attempt lost the race.
            Err(RunError::Closure(CreateError::Db(DbError::UniqueViolation {
                constraint: Some(constraint),
            }))) if constraint == IDENTITY_KEY => Err(SignInError::InvitationRequired),
            Err(RunError::Closure(CreateError::Hook(error))) => {
                tracing::error!("account creation hook failed");
                Err(unexpected(error))
            }
            Err(other) => Err(unexpected(other)),
        }
    }

    /// The email rule at invite redemption (01 §5): with no allowlist every redemption passes and no email was
    /// read; with one, only a verified address on the list passes (compared without regard to ASCII case).
    fn email_allowed(&self, email: Option<&str>) -> bool {
        self.email_allowlist.is_empty()
            || email.is_some_and(|address| {
                self.email_allowlist
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(address))
            })
    }
}
