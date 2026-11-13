//! AZ-3691 AC-1, AC-2 and AC-4 (IT-01-13, NFT-SEC-05, AC-20): wrong user codes at device approval count on the
//! real `AbuseLimiter` per approving session (5), per account (20) and per approver address (50), with windows
//! scaled to milliseconds; a limited caller cannot use even a correct code; each exhaustion writes one
//! `abuse_trip` event and one WARN line with ids only. Real PostgreSQL, manual clock.

mod support;

use std::net::IpAddr;
use std::time::Duration;

use contracts::{ApprovalView, DeviceGrantError, DeviceGrantService, Principal, PrincipalKind};
use sc_ids::{UserCode, new_user_code};
use sc_ratelimit::{LimitId, LimitSpec, LimitTable};
use sqlx::{Connection, PgConnection};
use sunnycloud_identity::IdentityDeviceGrants;
use support::auth::FAR_FUTURE;
use support::device::{geo, grants_with, ip, limiter_with, member};
use support::logs::LogCapture;
use support::sessions::principal;
use support::{Harness, START, audit, fixtures};
use tracing::Level;
use uuid::Uuid;

const WINDOW: Duration = Duration::from_millis(500);
const UNUSED: LimitSpec = LimitSpec::new(1_000, Duration::from_secs(60));
const APPROVER_IP: &str = "198.51.100.9";

struct Rig {
    harness: Harness,
    conn: PgConnection,
    service: IdentityDeviceGrants,
}

/// The device-approval limits at the AC-20 numbers, each over a 500 ms window.
async fn rig() -> Rig {
    let mut harness = Harness::new().await;
    let table = LimitTable::new(UNUSED, UNUSED)
        .with_spec(LimitId::DeviceApprovalSession, LimitSpec::new(5, WINDOW))
        .with_spec(LimitId::DeviceApprovalAccount, LimitSpec::new(20, WINDOW))
        .with_spec(LimitId::DeviceApprovalIp, LimitSpec::new(50, WINDOW));
    let limiter = limiter_with(&harness, table);
    let service = grants_with(&mut harness, geo(&[]), limiter);
    let conn = harness.fixture_conn().await;
    Rig {
        harness,
        conn,
        service,
    }
}

impl Rig {
    /// Another live session of `account`.
    async fn session_of(&mut self, account: Uuid) -> Principal {
        let token = format!("session-{}", Uuid::new_v4());
        let at = START.as_unix_micros();
        let session =
            fixtures::live_session(&mut self.conn, account, &token, "browser", FAR_FUTURE, at)
                .await;
        principal(account, session, PrincipalKind::Session)
    }

    async fn lookup(
        &self,
        approver: &Principal,
        code: &UserCode,
        from: IpAddr,
    ) -> Result<ApprovalView, DeviceGrantError> {
        self.service.lookup_for_approval(approver, code, from).await
    }

    /// One code nobody requested.
    async fn wrong(
        &self,
        approver: &Principal,
        from: IpAddr,
    ) -> Result<ApprovalView, DeviceGrantError> {
        let code = new_user_code().expect("user code");
        self.lookup(approver, &code, from).await
    }

    async fn pending_code(&self) -> UserCode {
        self.service
            .request("lab-1", "linux", ip("10.0.0.5"))
            .await
            .expect("request")
            .user_code
    }

    async fn finish(self) {
        let Rig {
            harness,
            conn,
            service,
        } = self;
        drop(service);
        conn.close().await.expect("close fixture connection");
        assert_eq!(harness.finish().await, 0, "leaked connections");
    }
}

fn unknown(result: &Result<ApprovalView, DeviceGrantError>) -> bool {
    matches!(result, Err(DeviceGrantError::UnknownCode))
}

fn limited(result: &Result<ApprovalView, DeviceGrantError>) -> bool {
    matches!(result, Err(DeviceGrantError::RateLimited { retry_after }) if !retry_after.is_zero())
}

/// `n` results unknown, then every later one limited.
fn trips_after(results: &[Result<ApprovalView, DeviceGrantError>], n: usize) -> bool {
    results[..n].iter().all(unknown) && results[n..].iter().all(limited)
}

/// The `limit` codes of the stored trips, in order.
async fn trips(conn: &mut PgConnection) -> Vec<String> {
    audit::of(conn, "abuse_trip")
        .await
        .into_iter()
        .map(|row| row.detail.unwrap_or_default())
        .collect()
}

#[tokio::test]
async fn one_session_gets_five_wrong_codes_then_rate_limited_and_the_trip_is_audited_and_logged_once()
 {
    // Arrange
    let mut rig = rig().await;
    let approver = member(&mut rig.conn).await;
    let logs = LogCapture::install();

    // Act
    let mut results = Vec::new();
    for _ in 0..7 {
        results.push(rig.wrong(&approver, ip(APPROVER_IP)).await);
    }

    // Assert
    assert!(trips_after(&results, 5), "{results:?}");
    assert_eq!(
        trips(&mut rig.conn).await,
        vec![r#"{"limit": "device_approval_session"}"#]
    );
    let warns = logs.at(Level::WARN, "abuse limit tripped");
    assert_eq!(warns.len(), 1, "{:?}", logs.lines());
    let expected = format!(
        " message=abuse limit tripped limit=\"device_approval_session\" account_id={} session_id={}",
        approver.account_id, approver.session_or_device_id
    );
    assert_eq!(warns[0].text, expected, "ids and the limit only");
    rig.finish().await;
}

#[tokio::test]
async fn sessions_of_one_account_get_twenty_wrong_codes_then_rate_limited() {
    // Arrange
    let mut rig = rig().await;
    let first = member(&mut rig.conn).await;
    let mut sessions = vec![first.clone()];
    for _ in 0..5 {
        sessions.push(rig.session_of(first.account_id).await);
    }

    // Act
    let mut results = Vec::new();
    for attempt in 0..21 {
        let session = &sessions[attempt / 4];
        results.push(rig.wrong(session, ip(APPROVER_IP)).await);
    }

    // Assert
    assert!(trips_after(&results, 20), "{results:?}");
    assert_eq!(
        trips(&mut rig.conn).await,
        vec![r#"{"limit": "device_approval_account"}"#]
    );
    rig.finish().await;
}

#[tokio::test]
async fn sessions_of_several_accounts_from_one_address_get_fifty_wrong_codes_then_rate_limited() {
    // Arrange
    let mut rig = rig().await;
    let mut approvers = Vec::new();
    for _ in 0..51 {
        approvers.push(member(&mut rig.conn).await);
    }

    // Act
    let mut results = Vec::new();
    for approver in &approvers {
        results.push(rig.wrong(approver, ip(APPROVER_IP)).await);
    }

    // Assert
    assert!(trips_after(&results, 50), "{results:?}");
    let stored = audit::of(&mut rig.conn, "abuse_trip").await;
    assert_eq!(stored.len(), 1, "{stored:?}");
    assert_eq!(
        stored[0].detail.as_deref(),
        Some(r#"{"limit": "device_approval_ip"}"#)
    );
    assert_eq!(stored[0].ip.as_deref(), Some(APPROVER_IP));
    rig.finish().await;
}

#[tokio::test]
async fn a_limited_session_cannot_use_a_correct_code_while_another_caller_can_and_the_window_frees_it()
 {
    // Arrange
    let mut rig = rig().await;
    let limited_session = member(&mut rig.conn).await;
    let other = member(&mut rig.conn).await;
    for _ in 0..5 {
        rig.wrong(&limited_session, ip(APPROVER_IP))
            .await
            .expect_err("wrong code");
    }
    let code = rig.pending_code().await;

    // Act
    let refused = rig.lookup(&limited_session, &code, ip(APPROVER_IP)).await;
    let allowed = rig.lookup(&other, &code, ip("203.0.113.40")).await;
    rig.harness.clock.tick(WINDOW);
    let after_window = rig.lookup(&limited_session, &code, ip(APPROVER_IP)).await;

    // Assert
    assert!(limited(&refused), "{refused:?}");
    assert!(allowed.is_ok(), "{allowed:?}");
    assert!(after_window.is_ok(), "{after_window:?}");
    rig.finish().await;
}
