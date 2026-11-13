//! Provider sign-in: `begin` stores a hashed one-time state and builds the provider redirect, `complete`
//! consumes the state, has the provider client validate the callback, creates the account of an unknown
//! identity that presented an invite (`register`) and signs the identity in (a web client gets a session, a
//! mobile one a one-time code that `exchange_mobile_code` turns into one).
//!
//! `begin_link` and `complete_link` attach another provider identity to a signed-in account (`link`), and
//! `list_providers` tells clients which providers are enabled and whether the install still needs its first
//! account. `register` also redeems the setup token and the re-bind invite of an admin reset. The public methods count failed steps and
//! invalid tokens per client address (`limits`).

mod limits;
mod link;
mod mobile;
mod register;
mod rules;
mod service;

use std::collections::HashMap;
use std::future::Future;
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use contracts::{
    AccountLifecycleHook, AuthorizationRedirect, BeginSignIn, BrowserBinding, CallbackParams,
    ClientKind, ProviderInfo, SessionTokens, SignInError, SignInOutcome,
};
use rules::{PublicUrl, is_relative_return_path, link_state_hash, state_hash};
use sc_audit::AuditWriter;
use sc_clock::{Clock, Timestamp};
use sc_config::{KeyProblem, Reason};
use sc_db::Db;
use sc_error::ErrorCode;
use sc_ids::{RawToken, TokenHash, TokenKind, new_id, new_token, sha256_token};
use sc_ratelimit::AbuseLimiter;
use uuid::Uuid;

use crate::abuse::AbuseCounter;
use crate::audit_events::{self, record_logged};
use crate::auth_repo::{self, KnownIdentity, NewState, StateRow};
use crate::config::IdentityConfig;
use crate::error::IdentityError;
use crate::outbound::OutboundClient;
use crate::provider::{
    self, AuthorizationRequest, CodeExchange, ProviderClient, ProviderError, ProviderIdentity,
};
use crate::session_repo::NewSession;

const ACTIVE: &str = "active";

/// How a provider exchange failed, after the deadline is applied.
enum ProviderFailure {
    Unavailable,
    Rejected,
}

/// What a new sign-in state stores besides its hash, nonce and verifier.
struct StateRequest<'a> {
    provider: &'a str,
    return_to: &'a str,
    client_kind: &'static str,
    invite: Option<RawToken>,
    code_challenge: Option<&'a str>,
    /// The account a link attempt is for: the state hash is bound to it as well as to the browser.
    link_account: Option<Uuid>,
}

pub struct IdentitySignIn {
    db: Db,
    clock: Arc<dyn Clock>,
    audit: AuditWriter,
    limits: AbuseCounter,
    providers: HashMap<String, Arc<dyn ProviderClient>>,
    /// The configured providers in configuration order, for `list_providers`.
    provider_list: Vec<ProviderInfo>,
    public_url: PublicUrl,
    oauth_state_ttl: Duration,
    login_code_ttl: Duration,
    access_token_ttl: Duration,
    refresh_token_ttl: Duration,
    provider_timeout: Duration,
    /// Empty when no allowlist is configured; then no email is read at all.
    email_allowlist: Vec<String>,
    /// Set once by the composition root; account creation fails closed while it is unset.
    hooks: OnceLock<Vec<Arc<dyn AccountLifecycleHook>>>,
}

impl IdentitySignIn {
    /// Validates the identity section and builds one client per configured provider. Opens no connection.
    /// `limiter` is the one instance of the server, built by the composition root.
    pub fn new(
        db: Db,
        clock: Arc<dyn Clock>,
        config: &IdentityConfig,
        audit: AuditWriter,
        limiter: Arc<AbuseLimiter>,
    ) -> Result<Self, IdentityError> {
        let problems = config.validate();
        if !problems.is_empty() {
            return Err(IdentityError::Config(problems));
        }
        let public_url = PublicUrl::parse(&config.public_url).ok_or_else(|| {
            IdentityError::Config(vec![KeyProblem::new("public_url", Reason::WrongType)])
        })?;
        let http = Arc::new(
            OutboundClient::new(config.allow_plain_http_dev)
                .map_err(|error| IdentityError::OutboundSetup(Box::new(error)))?,
        );
        let read_email = !config.email_allowlist.is_empty();
        let mut providers = HashMap::new();
        for (index, provider) in config.providers.iter().enumerate() {
            let client = provider::build(
                index,
                provider,
                Arc::clone(&http),
                Arc::clone(&clock),
                read_email,
            )?;
            providers.insert(provider.key.clone(), client);
        }
        Ok(IdentitySignIn {
            db,
            clock,
            limits: AbuseCounter::new(limiter, audit.clone()),
            audit,
            providers,
            provider_list: link::provider_list(&config.providers),
            public_url,
            oauth_state_ttl: config.oauth_state_ttl,
            login_code_ttl: config.login_code_ttl,
            access_token_ttl: config.access_token_ttl,
            refresh_token_ttl: config.refresh_token_ttl,
            provider_timeout: config.provider_timeout,
            email_allowlist: config.email_allowlist.clone(),
            hooks: OnceLock::new(),
        })
    }

    async fn begin_step(&self, request: BeginSignIn) -> Result<AuthorizationRedirect, SignInError> {
        let provider = self
            .providers
            .get(&request.provider)
            .ok_or(SignInError::UnknownProvider)?;
        self.check_host(&request.host)?;
        if !is_relative_return_path(&request.return_to) {
            return Err(SignInError::InvalidReturnPath);
        }
        let client_kind = client_kind_text(request.client_kind).ok_or_else(invalid_request)?;
        let mobile = request.client_kind.is_mobile();
        let code_challenge = request
            .code_challenge
            .filter(|challenge| mobile && !challenge.is_empty());
        if mobile && code_challenge.is_none() {
            return Err(invalid_request());
        }
        let invite = match (request.invite_token, request.setup_token) {
            (Some(_), Some(_)) => return Err(invalid_request()),
            (token, None) | (None, token) => token,
        };
        self.open_state(
            provider.as_ref(),
            StateRequest {
                provider: &request.provider,
                return_to: &request.return_to,
                client_kind,
                invite,
                code_challenge: code_challenge.as_deref(),
                link_account: None,
            },
        )
        .await
    }

    /// Stores a new hashed one-time state and builds the provider redirect.
    async fn open_state(
        &self,
        provider: &dyn ProviderClient,
        request: StateRequest<'_>,
    ) -> Result<AuthorizationRedirect, SignInError> {
        let (state, nonce, pkce_verifier, binding) = (random()?, random()?, random()?, random()?);
        let redirect_uri = self.public_url.callback_url(request.provider);
        let url = self
            .bounded(
                request.provider,
                provider.authorization_url(&AuthorizationRequest {
                    redirect_uri: &redirect_uri,
                    state: &state,
                    nonce: &nonce,
                    pkce_verifier: &pkce_verifier,
                }),
            )
            .await
            .map_err(|_| SignInError::ProviderUnavailable)?;
        let created_at = self.clock.now_utc();
        auth_repo::insert_state(
            &self.db,
            NewState {
                state_hash: &bound_state_hash(&state, &binding, request.link_account),
                provider: request.provider,
                nonce: &nonce,
                pkce_verifier: &pkce_verifier,
                return_to: request.return_to,
                client_kind: request.client_kind,
                invite_token_hash: request.invite.map(|token| sha256_token(token.expose())),
                code_challenge: request.code_challenge,
                created_at,
                expires_at: after(created_at, self.oauth_state_ttl)?,
            },
        )
        .await
        .map_err(unexpected)?;
        Ok(AuthorizationRedirect {
            url,
            browser_binding: BrowserBinding(binding),
        })
    }

    async fn complete_step(
        &self,
        provider: &str,
        callback_params: CallbackParams,
        browser_binding: &BrowserBinding,
        host: &str,
        client_ip: IpAddr,
    ) -> Result<SignInOutcome, SignInError> {
        let (stored, identity) = self
            .callback(provider, &callback_params, browser_binding, host, None)
            .await?;
        let client_kind = parse_client_kind(&stored.client_kind)?;
        let known = match self.known_identity(provider, &identity).await? {
            Some(known) => known,
            None => {
                let invite = stored.invite_token_hash.as_deref();
                self.register_counted(provider, &identity, invite, client_ip)
                    .await?
            }
        };
        if known.account_state != ACTIVE {
            return Err(SignInError::TrustedFailure {
                code: ErrorCode::AccountNotActive,
            });
        }
        if client_kind.is_mobile() {
            let challenge = stored.code_challenge.ok_or_else(invalid_request)?;
            let code = self
                .store_login_code(&known, &stored.client_kind, &challenge)
                .await?;
            self.audit_sign_in(&known, None).await;
            return Ok(SignInOutcome::MobileCode {
                code,
                return_to: stored.return_to,
            });
        }
        let (tokens, session) = self.start_session(&known, &stored.client_kind).await?;
        self.audit_sign_in(&known, Some(session)).await;
        Ok(SignInOutcome::Session {
            tokens,
            return_to: stored.return_to,
        })
    }

    /// The callback validation shared by `complete` and `complete_link`: host, consumed live state of this
    /// provider (bound to the browser and, for a link, to the account), code, and the provider exchange.
    async fn callback(
        &self,
        provider: &str,
        callback_params: &CallbackParams,
        browser_binding: &BrowserBinding,
        host: &str,
        link_account: Option<Uuid>,
    ) -> Result<(StateRow, ProviderIdentity), SignInError> {
        self.check_host(host)?;
        let client = self
            .providers
            .get(provider)
            .ok_or(SignInError::UntrustedState)?;
        let state = param(callback_params, "state").ok_or(SignInError::UntrustedState)?;
        let hash = bound_state_hash(state, &browser_binding.0, link_account);
        let stored = auth_repo::consume_state(&self.db, &hash)
            .await
            .map_err(unexpected)?
            .filter(|stored| self.is_live_state_of(stored, provider))
            .ok_or(SignInError::UntrustedState)?;
        let code = param(callback_params, "code")
            .filter(|code| !code.is_empty())
            .ok_or_else(invalid_request)?;
        let identity = self
            .bounded(
                provider,
                client.identity(&CodeExchange {
                    code,
                    redirect_uri: &self.public_url.callback_url(provider),
                    nonce: &stored.nonce,
                    pkce_verifier: &stored.pkce_verifier,
                }),
            )
            .await
            .map_err(|failure| match failure {
                ProviderFailure::Unavailable => SignInError::ProviderUnavailable,
                ProviderFailure::Rejected => SignInError::TrustedFailure {
                    code: ErrorCode::Unauthenticated,
                },
            })?;
        Ok((stored, identity))
    }

    /// The best-effort `sign_in` record (owner decision 2026-11-13): a failed write is logged, the sign-in stands.
    async fn audit_sign_in(&self, known: &KnownIdentity, session: Option<Uuid>) {
        let event = audit_events::signed_in(known.account_id, known.identity_id, session);
        record_logged(&self.audit, event).await;
    }

    fn check_host(&self, host: &str) -> Result<(), SignInError> {
        if self.public_url.matches_host(host) {
            Ok(())
        } else {
            Err(SignInError::HostMismatch)
        }
    }

    /// A state row is good for this callback when it belongs to this provider and has not expired.
    fn is_live_state_of(&self, stored: &StateRow, provider: &str) -> bool {
        stored.provider == provider
            && stored.expires_at_micros > self.clock.now_utc().as_unix_micros()
    }

    /// Runs one provider exchange under `provider_timeout`. Only the class of a failure is logged.
    async fn bounded<T>(
        &self,
        provider: &str,
        exchange: impl Future<Output = Result<T, ProviderError>>,
    ) -> Result<T, ProviderFailure> {
        let failure = match tokio::time::timeout(self.provider_timeout, exchange).await {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(ProviderError::Rejected(_))) => ProviderFailure::Rejected,
            Ok(Err(ProviderError::Unavailable(_))) | Err(_) => ProviderFailure::Unavailable,
        };
        let class = match failure {
            ProviderFailure::Unavailable => "unavailable",
            ProviderFailure::Rejected => "rejected",
        };
        tracing::warn!(provider, class, "provider exchange failed");
        Err(failure)
    }

    async fn known_identity(
        &self,
        provider: &str,
        identity: &ProviderIdentity,
    ) -> Result<Option<KnownIdentity>, SignInError> {
        auth_repo::identity_by_provider_id(&self.db, provider, &identity.provider_user_id)
            .await
            .map_err(unexpected)
    }

    /// The tokens of a new session and its id.
    async fn start_session(
        &self,
        identity: &KnownIdentity,
        client_kind: &str,
    ) -> Result<(SessionTokens, Uuid), SignInError> {
        let access = new_token(TokenKind::Session).map_err(unexpected)?;
        let refresh = new_token(TokenKind::Refresh).map_err(unexpected)?;
        let now = self.clock.now_utc();
        let id = new_id(self.clock.as_ref());
        auth_repo::start_session(
            &self.db,
            identity,
            NewSession {
                id,
                client_kind,
                label: client_kind,
                token_hash: sha256_token(access.expose()),
                access_expires_at: after(now, self.access_token_ttl)?,
                refresh_hash: sha256_token(refresh.expose()),
                refresh_expires_at: after(now, self.refresh_token_ttl)?,
                created_at: now,
            },
        )
        .await
        .map_err(unexpected)?;
        Ok((
            SessionTokens {
                access,
                refresh,
                expires_in: self.access_token_ttl,
            },
            id,
        ))
    }
}

/// The stored state hash: bound to the browser, and for a link attempt to the account as well, so a sign-in
/// callback cannot consume a link state, nor another account's session a link state.
fn bound_state_hash(state: &str, browser_binding: &str, link_account: Option<Uuid>) -> TokenHash {
    match link_account {
        Some(account) => link_state_hash(state, browser_binding, account),
        None => state_hash(state, browser_binding),
    }
}

/// The failure of a request that is malformed in a way the port has no variant for.
fn invalid_request() -> SignInError {
    SignInError::TrustedFailure {
        code: ErrorCode::InvalidRequest,
    }
}

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> SignInError {
    SignInError::Unexpected(Box::new(error))
}

fn param<'a>(params: &'a CallbackParams, name: &str) -> Option<&'a str> {
    params
        .0
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// A random 256-bit URL-safe text for the state, nonce, PKCE verifier and browser binding. The token kind
/// is only a label of the generator; these values are never presented as credentials.
fn random() -> Result<String, SignInError> {
    new_token(TokenKind::Session)
        .map(|token| token.expose().to_owned())
        .map_err(unexpected)
}

fn after(at: Timestamp, by: Duration) -> Result<Timestamp, SignInError> {
    at.checked_add(by)
        .ok_or_else(|| unexpected(IdentityError::TimeOutOfRange))
}

/// The `client_kind` text of the sign-in tables; a desktop client signs in through the device grant.
fn client_kind_text(kind: ClientKind) -> Option<&'static str> {
    match kind {
        ClientKind::Web => Some("web"),
        ClientKind::Android => Some("android"),
        ClientKind::Ios => Some("ios"),
        ClientKind::Desktop => None,
    }
}

fn parse_client_kind(text: &str) -> Result<ClientKind, SignInError> {
    match text {
        "web" => Ok(ClientKind::Web),
        "android" => Ok(ClientKind::Android),
        "ios" => Ok(ClientKind::Ios),
        other => Err(unexpected(IdentityError::UnexpectedColumnValue {
            column: "oauth_states.client_kind",
            value: other.to_owned(),
        })),
    }
}
