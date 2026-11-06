//! The sign-in service on the harness, with the in-process provider as both configured providers (`github`
//! and `oidc`), and row counts to show that a failed callback creates nothing.

use contracts::{BeginSignIn, BrowserBinding, CallbackParams, ClientKind};
use sc_config::Secret;
use sqlx::{Connection, PgConnection};
use sunnycloud_identity::{IdentityConfig, IdentitySignIn, ProviderConfig};
use uuid::Uuid;

use super::provider::FakeProvider;
use super::{Harness, START, fixtures};

pub const HOST: &str = "sunnycloud.test";
pub const GITHUB: &str = "github";
pub const OIDC: &str = "oidc";

pub struct World {
    pub harness: Harness,
    pub idp: FakeProvider,
    pub service: IdentitySignIn,
    pub conn: PgConnection,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Counts {
    pub sessions: i64,
    pub accounts: i64,
    pub identities: i64,
    pub login_codes: i64,
    pub oauth_states: i64,
}

pub async fn world() -> World {
    world_with(|_| {}).await
}

pub async fn world_with(change: impl FnOnce(&mut IdentityConfig)) -> World {
    let mut harness = Harness::new().await;
    let idp = FakeProvider::start(START.as_unix_micros() / 1_000_000).await;
    let provider = |key: &str, client_id: &str| ProviderConfig {
        key: key.to_owned(),
        kind: key.to_owned(),
        issuer: Some(idp.base_url()),
        client_id: client_id.to_owned(),
        client_secret: Secret::new("provider-secret".to_owned()),
    };
    harness.config.providers = vec![provider(GITHUB, "gh-client"), provider(OIDC, "oidc-client")];
    harness.config.allow_plain_http_dev = true;
    change(&mut harness.config);
    let service = IdentitySignIn::new(harness.api.clone(), harness.clock.clone(), &harness.config)
        .expect("sign-in service");
    let conn = harness.fixture_conn().await;
    World {
        harness,
        idp,
        service,
        conn,
    }
}

impl World {
    /// An active account with an identity at `provider`; returns the account id.
    pub async fn known_identity(&mut self, provider: &str, provider_user_id: &str) -> Uuid {
        let (at, account) = (START.as_unix_micros(), Uuid::new_v4());
        fixtures::account(&mut self.conn, account, at).await;
        fixtures::identity_at(&mut self.conn, account, provider, provider_user_id, at).await;
        account
    }

    pub async fn counts(&mut self) -> Counts {
        let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM sessions), (SELECT count(*) FROM accounts), \
             (SELECT count(*) FROM identities), (SELECT count(*) FROM login_codes), \
             (SELECT count(*) FROM oauth_states)",
        )
        .fetch_one(&mut self.conn)
        .await
        .expect("count rows");
        Counts {
            sessions: row.0,
            accounts: row.1,
            identities: row.2,
            login_codes: row.3,
            oauth_states: row.4,
        }
    }

    /// `begin` as a web browser would start it; returns the provider URL and the binding cookie.
    pub async fn begin(&self, provider: &str) -> (String, BrowserBinding) {
        let redirect = self
            .service
            .begin(begin_request(provider, "/files"))
            .await
            .expect("begin");
        (redirect.url, redirect.browser_binding)
    }

    pub async fn finish(self) {
        let World {
            harness,
            idp,
            service,
            conn,
        } = self;
        drop(service);
        drop(idp);
        conn.close().await.expect("close fixture connection");
        assert_eq!(harness.finish().await, 0, "leaked connections");
    }
}

pub fn begin_request(provider: &str, return_to: &str) -> BeginSignIn {
    BeginSignIn {
        provider: provider.to_owned(),
        return_to: return_to.to_owned(),
        client_kind: ClientKind::Web,
        invite_token: None,
        setup_token: None,
        code_challenge: None,
        host: HOST.to_owned(),
    }
}

pub fn params(pairs: Vec<(String, String)>) -> CallbackParams {
    CallbackParams(pairs)
}
