//! AC-1 to AC-5 and AC-8 of AZ-3966: a 401 on a mobile session rotates the tokens once (single-flight), the host
//! stores the new pair before any request uses it, a lost pair or a revoked session stops all work.

mod auth_support;
mod connect_support;
mod request_support;
mod support;

use std::sync::Arc;
use std::time::Duration;

use auth_support::{FakeAuth, SpyCredentials, connected_as, requests_to, root_view};
use request_support::{clock, eventually};
use support::{Answer, Route, Script, seeded_bytes};
use sync_core::{HttpSyncClient, Method, Request, SyncApi, SyncError};

async fn four_root_nodes(
    api: &Arc<HttpSyncClient>,
) -> Vec<Result<sync_core_proto::NodeView, SyncError>> {
    let calls: Vec<_> = (0..4)
        .map(|_| {
            let api = Arc::clone(api);
            tokio::spawn(async move { api.root_node().await })
        })
        .collect();
    let mut results = Vec::new();
    for call in calls {
        results.push(call.await.expect("the call task ends"));
    }
    results
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_parallel_401s_cause_one_refresh_and_four_retries() {
    // Arrange
    let auth = FakeAuth::new(clock(), Duration::from_millis(150));
    let credentials = SpyCredentials::session(Duration::ZERO);
    let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
    let (api, fixture) = connected_as(script, credentials.clone()).await;
    auth.expire_access();

    // Act
    let results = four_root_nodes(&api).await;

    // Assert
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert_eq!(auth.refresh_presented(), vec![FakeAuth::initial_refresh()]);
    assert_eq!(requests_to(&fixture, Route::RootNode).len(), 8);
    assert_eq!(
        credentials.persisted(),
        vec![("ACC-SENTINEL-1".to_owned(), "REF-SENTINEL-1".to_owned())]
    );
    let retried = auth
        .carried()
        .iter()
        .filter(|(_, token)| token == "ACC-SENTINEL-1")
        .count();
    assert_eq!(retried, 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_refresh_token_travels_in_the_body_only_and_the_refresh_request_has_no_access_token() {
    // Arrange
    let auth = FakeAuth::new(clock(), Duration::ZERO);
    let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
    let (api, fixture) = connected_as(script, SpyCredentials::session(Duration::ZERO)).await;
    auth.expire_access();

    // Act
    let result = api.root_node().await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    let refreshes = requests_to(&fixture, Route::RefreshTokens);
    assert_eq!(refreshes.len(), 1);
    assert_eq!(refreshes[0].target, "/api/v1/auth/refresh-tokens");
    assert_eq!(refreshes[0].header("authorization"), None);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&refreshes[0].body).expect("a JSON body")["refresh_token"],
        "REF-SENTINEL-0"
    );
    assert!(
        fixture
            .requests()
            .iter()
            .all(|request| !request.target.contains("SENTINEL"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_request_carries_the_new_access_token_before_the_host_stored_it() {
    // Arrange
    let auth = FakeAuth::new(clock(), Duration::ZERO);
    let credentials = SpyCredentials::session(Duration::from_millis(300));
    let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
    let (api, _fixture) = connected_as(script, credentials.clone()).await;
    auth.expire_access();

    // Act
    let first = {
        let api = Arc::clone(&api);
        tokio::spawn(async move { api.root_node().await })
    };
    eventually(5_000, "the host to start storing", || {
        !credentials.persist_started().is_empty()
    })
    .await;
    let second = {
        let api = Arc::clone(&api);
        tokio::spawn(async move { api.root_node().await })
    };
    let (first, second) = (first.await.expect("task"), second.await.expect("task"));

    // Assert
    assert!(first.is_ok() && second.is_ok(), "{first:?} {second:?}");
    assert_eq!(credentials.persisted().len(), 1);
    let returned = credentials.persist_returned()[0];
    let with_new_token: Vec<_> = auth
        .carried()
        .into_iter()
        .filter(|(_, token)| token == "ACC-SENTINEL-1")
        .collect();
    assert_eq!(with_new_token.len(), 2);
    assert!(
        with_new_token
            .iter()
            .all(|(arrived, _)| *arrived >= returned),
        "a request used the new access token before on_rotated returned"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pair_the_host_cannot_store_after_one_retry_is_unauthorized_and_stops_all_work() {
    // Arrange
    let auth = FakeAuth::new(clock(), Duration::ZERO);
    let credentials = SpyCredentials::session(Duration::ZERO);
    credentials.fail_persists(2);
    let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
    let (api, fixture) = connected_as(script, credentials.clone()).await;
    auth.expire_access();

    // Act
    let first = api.root_node().await;
    let sent = fixture.requests().len();
    let second = api.root_node().await;

    // Assert
    assert_eq!(first, Err(SyncError::Unauthorized));
    assert_eq!(second, Err(SyncError::Unauthorized));
    assert_eq!(credentials.persisted().len(), 2);
    assert_eq!(auth.refresh_presented().len(), 1);
    assert_eq!(
        fixture.requests().len(),
        sent,
        "work continued after the stop"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refresh_lost_in_transit_is_retried_with_the_same_refresh_token_and_accepted() {
    // Arrange
    let auth = FakeAuth::new(clock(), Duration::ZERO);
    let credentials = SpyCredentials::session(Duration::ZERO);
    auth.lose_next_refresh_response();
    let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
    let (api, _fixture) = connected_as(script, credentials.clone()).await;
    auth.expire_access();

    // Act
    let result = api.root_node().await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        auth.refresh_presented(),
        vec![FakeAuth::initial_refresh(), FakeAuth::initial_refresh()]
    );
    assert_eq!(
        credentials.persisted(),
        vec![("ACC-SENTINEL-1".to_owned(), "REF-SENTINEL-1".to_owned())]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_revoked_session_ends_every_waiting_call_with_unauthorized_and_then_the_client_is_silent()
{
    // Arrange
    let auth = FakeAuth::new(clock(), Duration::from_millis(100));
    let credentials = SpyCredentials::session(Duration::ZERO);
    auth.revoke();
    let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
    let (api, fixture) = connected_as(script, credentials.clone()).await;
    auth.expire_access();

    // Act
    let results = four_root_nodes(&api).await;
    let sent = fixture.requests().len();
    let later = api.root_node().await;
    let refreshed = api.refresh().await;

    // Assert
    assert!(
        results
            .iter()
            .all(|result| *result == Err(SyncError::Unauthorized)),
        "{results:?}"
    );
    assert_eq!(auth.refresh_presented().len(), 1);
    assert!(credentials.persisted().is_empty());
    assert_eq!(later, Err(SyncError::Unauthorized));
    assert_eq!(refreshed, Err(SyncError::Unauthorized));
    assert_eq!(
        fixture.requests().len(),
        sent,
        "work continued after the stop"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_explicit_refresh_rotates_once_and_hands_the_pair_to_the_host() {
    // Arrange
    let auth = FakeAuth::new(clock(), Duration::ZERO);
    let credentials = SpyCredentials::session(Duration::ZERO);
    let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
    let (api, _fixture) = connected_as(script, credentials.clone()).await;

    // Act
    let refreshed = api.refresh().await;
    let after = api.root_node().await;

    // Assert
    assert_eq!(refreshed, Ok(()));
    assert!(after.is_ok(), "{after:?}");
    assert_eq!(auth.refresh_presented().len(), 1);
    assert_eq!(credentials.persisted().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_twenty_minute_block_transfer_refreshes_at_the_expiry_and_repeats_the_block() {
    // Arrange
    let test_clock = clock();
    let auth = FakeAuth::new(test_clock.clone(), Duration::ZERO);
    let credentials = SpyCredentials::session(Duration::ZERO);
    let script = auth.wire(Script::new(), &[(Route::PutBlock, Answer::no_content())]);
    let (api, fixture) = connected_as(script, credentials.clone()).await;
    let upload = uuid::Uuid::from_u128(7);

    // Act
    let mut statuses = Vec::new();
    for block in 0..20_u32 {
        test_clock.tick(Duration::from_secs(60));
        let request = Request {
            method: Method::Put,
            target: format!("/api/v1/uploads/{upload}/blocks/{block}"),
            headers: Vec::new(),
            body: seeded_bytes(u64::from(block), 1024),
            max_attempts: 3,
        };
        let sent = api.request_path().send("put_block", request).await;
        statuses.push(sent.map(|response| response.status));
    }

    // Assert
    assert!(
        statuses.iter().all(|status| matches!(status, Ok(204))),
        "{statuses:?}"
    );
    let indexes: Vec<u32> = requests_to(&fixture, Route::PutBlock)
        .iter()
        .filter_map(support::LoggedRequest::block_index)
        .collect();
    let mut expected: Vec<u32> = (0..20).collect();
    expected.insert(15, 14);
    assert_eq!(indexes, expected, "one block repeated, none lost");
    assert_eq!(auth.refresh_presented(), vec![FakeAuth::initial_refresh()]);
    assert_eq!(credentials.persisted().len(), 1);
}
