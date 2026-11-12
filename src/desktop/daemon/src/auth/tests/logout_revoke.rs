//! AC-5 (IT-14-21 at the unit tier) and AC-6 of AZ-3908: revocation signs out and stops the loops; logout
//! deletes the local token even when the server is down.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use ipc_schema::LoginState;
use sc_config::Secret;
use sc_error::TransportKind;
use sync_core::{SyncApi, SyncError};
use sync_core_proto::{Cursor, Hash32};
use uuid::Uuid;

use super::fake_api::FakeApi;
use super::{ACCOUNT, DEVICE_TOKEN, Harness, harness};
use crate::auth::token_store::TokenStore;
use crate::local_index::{LocalIndexStore, NewPair};

async fn signed_in() -> Harness {
    let h = harness();
    h.store
        .save(ACCOUNT, &Secret::new(DEVICE_TOKEN.to_owned()))
        .expect("seed the token");
    let restored = h.session.restore().await.expect("restore");
    assert_eq!(restored, LoginState::Approved);
    h
}

fn api_for(h: &Harness) -> Arc<FakeApi> {
    Arc::new(FakeApi::new(Arc::clone(&h.clock), 5))
}

#[tokio::test]
async fn a_revoked_token_signs_out_within_five_seconds_stops_uploads_and_keeps_pairs() {
    // Arrange
    let h = Arc::new(signed_in().await);
    let api = api_for(&h);
    let index = LocalIndexStore::open(h.dir.path()).expect("index");
    let pair = index
        .add_pair(&NewPair {
            server_node_id: Uuid::from_u128(7),
            local_root: "/home/u/Sync".to_owned(),
            rules: String::new(),
            root_sentinel: "sentinel".to_owned(),
            case_insensitive: false,
        })
        .expect("pair");
    let upload = {
        let (h, api) = (Arc::clone(&h), Arc::clone(&api));
        tokio::spawn(async move {
            let staging = api.stage_small(
                Uuid::from_u128(1),
                Bytes::from_static(b"x"),
                Hash32::from_bytes([0; 32]),
            );
            h.session.while_signed_in(staging).await.is_none()
        })
    };
    let feed = {
        let (h, api) = (Arc::clone(&h), Arc::clone(&api));
        tokio::spawn(async move {
            let polled = h
                .session
                .while_signed_in(api.poll_changes(Cursor::new(0, 0), Duration::from_secs(30), None))
                .await;
            if let Some(Err(error)) = &polled {
                h.session.note_sync_error(error);
            }
            polled
        })
    };
    while api.requests() < 2 {
        tokio::task::yield_now().await;
    }

    // Act
    api.release_feed();
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        (
            feed.await.expect("feed task"),
            upload.await.expect("upload task"),
        )
    })
    .await;

    // Assert
    let (polled, upload_stopped) = ended.expect("both loops ended within 5 s");
    assert!(matches!(polled, Some(Err(SyncError::Unauthorized))));
    assert!(upload_stopped);
    assert_eq!(h.session.login_status(), LoginState::SignedOut);
    assert!(index.get_pair(pair).expect("read pair").is_some());
}

#[tokio::test]
async fn an_unreachable_server_or_identity_provider_does_not_sign_out() {
    // Arrange
    let h = signed_in().await;
    let outages = [
        SyncError::Transport(TransportKind::Connect),
        SyncError::Unexpected {
            code: "service_unavailable".to_owned(),
            request_id: None,
        },
        SyncError::RateLimited { retry_after: None },
    ];

    // Act
    for outage in &outages {
        h.session.note_sync_error(outage);
    }
    let work = h.session.while_signed_in(async { 42 }).await;

    // Assert
    assert_eq!(h.session.login_status(), LoginState::Approved);
    assert_eq!(work, Some(42));
}

#[tokio::test]
async fn logout_with_the_server_down_deletes_the_token_and_reports_the_server_copy() {
    // Arrange
    let h = signed_in().await;
    let api = Arc::new(
        FakeApi::new(Arc::clone(&h.clock), 5)
            .sign_out_failing(SyncError::Transport(TransportKind::Connect)),
    );

    // Act
    let outcome = h
        .session
        .logout(Some(Arc::clone(&api) as Arc<dyn SyncApi>))
        .await
        .expect("logout");

    // Assert
    assert!(!outcome.server_revoked);
    assert_eq!(api.requests(), 1);
    assert_eq!(h.stored_token(), None);
    assert_eq!(h.session.login_status(), LoginState::SignedOut);
}

#[tokio::test]
async fn logout_with_the_server_up_revokes_and_deletes_the_token() {
    // Arrange
    let h = signed_in().await;
    let api = api_for(&h);

    // Act
    let outcome = h
        .session
        .logout(Some(Arc::clone(&api) as Arc<dyn SyncApi>))
        .await
        .expect("logout");

    // Assert
    assert!(outcome.server_revoked);
    assert_eq!(h.stored_token(), None);
    assert_eq!(h.session.login_status(), LoginState::SignedOut);
}

#[tokio::test]
async fn logout_without_a_server_still_deletes_the_token() {
    // Arrange
    let h = signed_in().await;

    // Act
    let outcome = h.session.logout(None).await.expect("logout");

    // Assert
    assert!(!outcome.server_revoked);
    assert_eq!(h.stored_token(), None);
}

#[tokio::test]
async fn restore_without_a_stored_token_is_signed_out() {
    // Arrange
    let h = harness();

    // Act
    let restored = h.session.restore().await.expect("restore");

    // Assert
    assert_eq!(restored, LoginState::SignedOut);
}
