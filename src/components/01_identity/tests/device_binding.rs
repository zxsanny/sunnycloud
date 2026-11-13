//! AZ-4267: a pending device grant is bound at lookup to the account that looked its user code up, and only that
//! account can decide it. Another account gets the answer of an unknown code or id, even when both look the code
//! up at the same time. Real PostgreSQL, manual clock.

mod support;

use contracts::{Decision, DeviceGrantError, DeviceGrantService};
use sqlx::Connection;
use support::device::{geo, grant_id, grant_looked_up_by, grant_status, grants, ip, member};
use support::{Harness, audit};

#[tokio::test]
async fn the_first_lookup_binds_the_grant_and_a_second_account_gets_an_unknown_code() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let a = member(&mut conn).await;
    let b = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");

    // Act
    let first = service
        .lookup_for_approval(&a, &authorization.user_code, ip("10.0.0.6"))
        .await;
    let bound_after_a = grant_looked_up_by(&mut conn, &authorization.user_code).await;
    let second = service
        .lookup_for_approval(&b, &authorization.user_code, ip("10.0.0.7"))
        .await;
    let again = service
        .lookup_for_approval(&a, &authorization.user_code, ip("10.0.0.6"))
        .await;

    // Assert
    let view = first.expect("A looks the grant up");
    assert_eq!(bound_after_a, Some(a.account_id));
    assert!(
        matches!(second, Err(DeviceGrantError::UnknownCode)),
        "{second:?}"
    );
    assert_eq!(
        grant_looked_up_by(&mut conn, &authorization.user_code).await,
        Some(a.account_id)
    );
    assert_eq!(again.expect("A looks it up again").grant_id, view.grant_id);
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn of_two_concurrent_lookups_by_two_accounts_exactly_one_binds_the_grant() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let a = member(&mut conn).await;
    let b = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");

    // Act
    let (by_a, by_b) = tokio::join!(
        service.lookup_for_approval(&a, &authorization.user_code, ip("10.0.0.6")),
        service.lookup_for_approval(&b, &authorization.user_code, ip("10.0.0.7")),
    );

    // Assert
    let winner = match (&by_a, &by_b) {
        (Ok(_), Err(DeviceGrantError::UnknownCode)) => a.account_id,
        (Err(DeviceGrantError::UnknownCode), Ok(_)) => b.account_id,
        other => panic!("exactly one lookup binds the grant: {other:?}"),
    };
    assert_eq!(
        grant_looked_up_by(&mut conn, &authorization.user_code).await,
        Some(winner)
    );
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn a_grant_nobody_looked_up_cannot_be_decided_by_its_id() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let a = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    let grant = grant_id(&mut conn, &authorization.user_code).await;
    let audit_before = audit::rows(&mut conn).await;

    // Act
    let approved = service.decide(&a, grant, Decision::Approve).await;
    let denied = service.decide(&a, grant, Decision::Deny).await;

    // Assert
    assert!(
        matches!(approved, Err(DeviceGrantError::UnknownCode)),
        "{approved:?}"
    );
    assert!(
        matches!(denied, Err(DeviceGrantError::UnknownCode)),
        "{denied:?}"
    );
    assert_eq!(
        grant_status(&mut conn, &authorization.device_code).await,
        "pending"
    );
    assert_eq!(
        grant_looked_up_by(&mut conn, &authorization.user_code).await,
        None
    );
    assert_eq!(audit::rows(&mut conn).await, audit_before);
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}
