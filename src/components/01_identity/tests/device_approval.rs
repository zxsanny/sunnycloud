//! AC-3 and AC-4 (IT-01-11 and IT-01-12, AC-57): expiry on lookup, decision and poll, and the approval view
//! of requester against approver. Real PostgreSQL, manual clock, a fixed-table `GeoLookup` as input.

mod support;

use std::time::Duration;

use contracts::{ApprovalView, Decision, DeviceGrantError, DeviceGrantService, PrincipalKind};
use sc_clock::Timestamp;
use sc_ids::new_user_code;
use sqlx::Connection;
use support::Harness;
use support::device::{
    approve, device_principal, geo, grant_approver, grant_expiry, grant_id, grant_status, grants,
    ip, member, token_count,
};
use uuid::Uuid;

const NEAR_EXPIRY: Duration = Duration::from_millis(100);

#[tokio::test]
async fn approval_before_expiry_succeeds_and_after_expiry_is_closed() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let ttl = harness.config.device_code_ttl;
    let early = service
        .request("early", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    let late = service
        .request("late", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    let late_view = service
        .lookup_for_approval(&approver, &late.user_code, ip("10.0.0.6"))
        .await
        .expect("lookup of the late grant before expiry");

    // Act
    harness.clock.tick(ttl - NEAR_EXPIRY);
    let early_id = approve(&service, &approver, &early.user_code, ip("10.0.0.6")).await;
    harness.clock.tick(NEAR_EXPIRY);
    let late_lookup = service
        .lookup_for_approval(&approver, &late.user_code, ip("10.0.0.6"))
        .await;
    let first_decision = service
        .decide(&approver, late_view.grant_id, Decision::Approve)
        .await;
    let second_decision = service
        .decide(&approver, late_view.grant_id, Decision::Approve)
        .await;
    let early_poll = service.poll(&early.device_code).await;
    let late_poll = service.poll(&late.device_code).await;

    // Assert
    assert_eq!(early_id, grant_id(&mut conn, &early.user_code).await);
    assert!(matches!(late_lookup, Err(DeviceGrantError::UnknownCode)));
    assert!(matches!(first_decision, Err(DeviceGrantError::Expired)));
    assert!(matches!(
        second_decision,
        Err(DeviceGrantError::AlreadyDecided)
    ));
    assert!(matches!(early_poll, Err(DeviceGrantError::ExpiredToken)));
    assert!(matches!(late_poll, Err(DeviceGrantError::ExpiredToken)));
    assert_eq!(token_count(&mut conn).await, 0);
    assert_eq!(
        grant_status(&mut conn, &early.device_code).await,
        "approved"
    );
    assert_eq!(
        grant_approver(&mut conn, &late.user_code).await,
        (None, None),
        "the expired grant was never approved"
    );
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn approval_records_the_approving_account_and_session() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");

    // Act
    approve(
        &service,
        &approver,
        &authorization.user_code,
        ip("10.0.0.6"),
    )
    .await;

    // Assert
    assert_eq!(
        grant_approver(&mut conn, &authorization.user_code).await,
        (
            Some(approver.account_id),
            Some(approver.session_or_device_id)
        )
    );
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn an_unknown_user_code_or_grant_id_is_unknown() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let never_issued = new_user_code().expect("user code");

    // Act
    let lookup = service
        .lookup_for_approval(&approver, &never_issued, ip("10.0.0.6"))
        .await;
    let decision = service
        .decide(&approver, Uuid::new_v4(), Decision::Approve)
        .await;

    // Assert
    assert!(matches!(lookup, Err(DeviceGrantError::UnknownCode)));
    assert!(matches!(decision, Err(DeviceGrantError::UnknownCode)));
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn a_device_principal_can_neither_look_up_nor_decide() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let first = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    let view = service
        .lookup_for_approval(&approver, &first.user_code, ip("10.0.0.6"))
        .await
        .expect("lookup");
    let device = device_principal(approver.account_id, Uuid::new_v4());
    assert_eq!(device.kind, PrincipalKind::Device);

    // Act
    let lookup = service
        .lookup_for_approval(&device, &first.user_code, ip("10.0.0.6"))
        .await;
    let decision = service
        .decide(&device, view.grant_id, Decision::Approve)
        .await;

    // Assert
    assert!(
        matches!(lookup, Err(DeviceGrantError::Forbidden)),
        "{lookup:?}"
    );
    assert!(
        matches!(decision, Err(DeviceGrantError::Forbidden)),
        "{decision:?}"
    );
    assert_eq!(grant_status(&mut conn, &first.device_code).await, "pending");
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

async fn view_for(
    service: &impl DeviceGrantService,
    approver: &contracts::Principal,
    requester_ip: &str,
    approver_ip: &str,
) -> (ApprovalView, sc_ids::UserCode) {
    let authorization = service
        .request("lab-1", "linux", ip(requester_ip))
        .await
        .expect("request");
    let view = service
        .lookup_for_approval(approver, &authorization.user_code, ip(approver_ip))
        .await
        .expect("lookup");
    (view, authorization.user_code)
}

#[tokio::test]
async fn the_view_shows_both_sides_and_flags_a_mismatch() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let cities = geo(&[("203.0.113.7", "Kyiv"), ("198.51.100.9", "Lviv")]);
    let service = grants(&mut harness, cities);
    let nowhere = grants(&mut harness, geo(&[]));

    // Act
    let (cities_apart, apart_code) =
        view_for(&service, &approver, "203.0.113.7", "198.51.100.9").await;
    let (same_ip, same_code) = view_for(&service, &approver, "203.0.113.7", "203.0.113.7").await;
    let (unknown, unknown_code) =
        view_for(&nowhere, &approver, "203.0.113.7", "198.51.100.9").await;

    // Assert
    assert!(cities_apart.mismatch);
    assert_eq!(cities_apart.requester_location.as_deref(), Some("Kyiv"));
    assert_eq!(cities_apart.approver_location.as_deref(), Some("Lviv"));
    assert_eq!(
        (cities_apart.requester_ip, cities_apart.approver_ip),
        (ip("203.0.113.7"), ip("198.51.100.9"))
    );
    assert_eq!(cities_apart.device, "lab-1");
    assert!(!same_ip.mismatch);
    assert_eq!(same_ip.requester_location.as_deref(), Some("Kyiv"));
    assert_eq!(same_ip.approver_location.as_deref(), Some("Kyiv"));
    assert!(
        unknown.mismatch,
        "different addresses warn even without locations"
    );
    assert_eq!(unknown.requester_location, None);
    assert_eq!(unknown.approver_location, None);
    for (view, code) in [
        (&cities_apart, &apart_code),
        (&same_ip, &same_code),
        (&unknown, &unknown_code),
    ] {
        assert_eq!(view.grant_id, grant_id(&mut conn, code).await);
        assert_eq!(
            view.expires_at,
            Timestamp::from_unix_micros(grant_expiry(&mut conn, code).await)
        );
    }
    conn.close().await.expect("close fixture connection");
    drop((service, nowhere));
    assert_eq!(harness.finish().await, 0, "leaked connections");
}
