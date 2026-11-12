//! AC-3 of AZ-4236: sync-core's real client runs a mobile code exchange and then a refresh against the real sign-in
//! and session routes, with 01's ports as recording stubs; both answers decode and the refresh rotates the pair.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::{SessionError, SessionTokens as IssuedTokens, SignInError};
use sc_config::Secret;
use sc_ids::TokenKind;
use sync_core::{
    Credential, CredentialProvider, CredentialUnavailable, HttpSyncClient, PersistFailed, SyncApi,
};
use sync_core_proto::SessionTokens;

use super::support::{Stack, token};
use super::tls_terminator::{Terminator, client_config};

const EXCHANGE: &str = "SignInService::exchange_mobile_code";
const REFRESH: &str = "SessionService::refresh";

fn issued(access: &str, refresh: &str) -> IssuedTokens {
    IssuedTokens {
        access: token(TokenKind::Session, access),
        refresh: token(TokenKind::Refresh, refresh),
        expires_in: Duration::from_secs(900),
    }
}

/// A mobile host's secure store: empty before the sign-in, then the pair it was handed or rotated to.
#[derive(Default)]
struct MobileHost {
    pair: Mutex<Option<(String, String)>>,
    rotated: Mutex<Vec<(String, String)>>,
}

impl MobileHost {
    fn store(&self, tokens: &SessionTokens) {
        let pair = (tokens.access_token.clone(), tokens.refresh_token.clone());
        *self.pair.lock().unwrap_or_else(PoisonError::into_inner) = Some(pair);
    }

    fn rotated(&self) -> Vec<(String, String)> {
        self.rotated
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl CredentialProvider for MobileHost {
    fn bearer(&self) -> Result<Credential, CredentialUnavailable> {
        let pair = self.pair.lock().unwrap_or_else(PoisonError::into_inner);
        let (access, refresh) = pair.clone().ok_or(CredentialUnavailable)?;
        Ok(Credential {
            access: Secret::new(access),
            refresh: Some(Secret::new(refresh)),
        })
    }

    fn on_rotated(
        &self,
        access: Secret<String>,
        refresh: Secret<String>,
    ) -> Result<(), PersistFailed> {
        let pair = (access.expose().clone(), refresh.expose().clone());
        self.rotated
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(pair.clone());
        *self.pair.lock().unwrap_or_else(PoisonError::into_inner) = Some(pair);
        Ok(())
    }
}

#[tokio::test]
async fn sync_core_exchanges_a_mobile_code_and_then_refreshes_against_the_real_routes() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        EXCHANGE,
        Ok::<_, SignInError>(issued("acc-FIRST_1", "ref-FIRST_1")),
    );
    stack.script.push(
        REFRESH,
        Ok::<_, SessionError>(issued("acc-SECOND_2", "ref-SECOND_2")),
    );
    let terminator = Terminator::start(stack.public).await;
    let host = Arc::new(MobileHost::default());
    let (api, _) = HttpSyncClient::connect_with_roots(
        client_config(format!("https://{}", terminator.addr), "android"),
        host.clone(),
        vec![terminator.ca.clone()],
    )
    .await
    .expect("the client connects through the terminator");

    // Act
    let exchanged = api.exchange_mobile_code("mob-CODE_1", "ver-IFIER").await;
    if let Ok(tokens) = &exchanged {
        host.store(tokens);
    }
    let refreshed = api.refresh().await;

    // Assert
    assert_eq!(
        exchanged,
        Ok(SessionTokens {
            access_token: "acc-FIRST_1".to_owned(),
            refresh_token: "ref-FIRST_1".to_owned(),
            expires_in: 900,
        })
    );
    assert_eq!(refreshed, Ok(()));
    assert_eq!(
        host.rotated(),
        [("acc-SECOND_2".to_owned(), "ref-SECOND_2".to_owned())]
    );
    assert_eq!(
        stack.log(EXCHANGE),
        ["SignInService::exchange_mobile_code mob-CODE_1 ver-IFIER"]
    );
    assert_eq!(stack.log(REFRESH), ["SessionService::refresh ref-FIRST_1"]);
    drop(terminator);
    stack.stop().await;
}
