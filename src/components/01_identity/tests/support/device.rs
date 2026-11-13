//! Helpers of the device-grant tests: the service on the harness over a fixed-table `GeoLookup`, signed-in
//! approvers, the full request-approve-poll flow, and probes of the grant and token rows as the schema owner.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use contracts::{Decision, DeviceGrantService, DeviceToken, Principal, PrincipalKind};
use sc_ids::{RawToken, TokenKind, UserCode};
use sc_ratelimit::{AbuseLimiter, LimitSpec, LimitTable};
use sqlx::PgConnection;
use sunnycloud_identity::{GeoLookup, IdentityDeviceGrants};
use uuid::Uuid;

use super::sessions::principal;
use super::{Harness, START, fixtures};

/// The poll interval of these tests: short, and well inside the harness's `device_code_ttl`.
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The DS-32 stand-in of the component tests: an address-to-city table behind the `GeoLookup` port.
pub struct FixedGeo(HashMap<IpAddr, String>);

impl GeoLookup for FixedGeo {
    fn locate(&self, ip: IpAddr) -> Option<String> {
        self.0.get(&ip).cloned()
    }
}

pub fn ip(text: &str) -> IpAddr {
    text.parse().expect("test address")
}

pub fn geo(pairs: &[(&str, &str)]) -> Arc<FixedGeo> {
    Arc::new(FixedGeo(
        pairs
            .iter()
            .map(|(address, city)| (ip(address), (*city).to_owned()))
            .collect(),
    ))
}

/// The two limits AC-20 gives no number for are not exercised by these tests: a roomy spec.
const UNUSED_LIMIT: LimitSpec = LimitSpec::new(1_000, Duration::from_secs(60));

/// A limiter of the AC-20 numbers (hour windows; the manual clock does not refill it unless a test ticks it).
pub fn limiter(harness: &Harness) -> Arc<AbuseLimiter> {
    limiter_with(harness, LimitTable::new(UNUSED_LIMIT, UNUSED_LIMIT))
}

pub fn limiter_with(harness: &Harness, table: LimitTable) -> Arc<AbuseLimiter> {
    Arc::new(AbuseLimiter::new(harness.clock.clone(), table).expect("limit table"))
}

pub fn grants(harness: &mut Harness, geo: Arc<dyn GeoLookup>) -> IdentityDeviceGrants {
    let limiter = limiter(harness);
    grants_with(harness, geo, limiter)
}

pub fn grants_with(
    harness: &mut Harness,
    geo: Arc<dyn GeoLookup>,
    limiter: Arc<AbuseLimiter>,
) -> IdentityDeviceGrants {
    harness.config.device_poll_interval = POLL_INTERVAL;
    IdentityDeviceGrants::new(
        harness.api.clone(),
        harness.clock.clone(),
        geo,
        &harness.config,
        limiter,
        harness.audit(),
    )
}

pub fn shared_grants(harness: &mut Harness, geo: Arc<dyn GeoLookup>) -> Arc<IdentityDeviceGrants> {
    Arc::new(grants(harness, geo))
}

/// A new account with one live session; returns the session principal.
pub async fn member(conn: &mut PgConnection) -> Principal {
    let at = START.as_unix_micros();
    let account = Uuid::new_v4();
    fixtures::account(conn, account, at).await;
    let token = format!("session-{account}");
    let session = fixtures::live_session(
        conn,
        account,
        &token,
        "browser",
        super::auth::FAR_FUTURE,
        at,
    )
    .await;
    principal(account, session, PrincipalKind::Session)
}

/// The device principal of a minted token, for `revoke_self`.
pub fn device_principal(account: Uuid, device: Uuid) -> Principal {
    principal(account, device, PrincipalKind::Device)
}

pub fn code_again(code: &RawToken) -> RawToken {
    RawToken::from_presented(TokenKind::DeviceCode, code.expose().to_owned())
}

/// Looks the user code up as `approver` from `approver_ip` and approves the grant; returns the grant id.
pub async fn approve(
    service: &IdentityDeviceGrants,
    approver: &Principal,
    user_code: &UserCode,
    approver_ip: IpAddr,
) -> Uuid {
    let view = service
        .lookup_for_approval(approver, user_code, approver_ip)
        .await
        .expect("lookup of a pending grant");
    service
        .decide(approver, view.grant_id, Decision::Approve)
        .await
        .expect("approval of a pending grant");
    view.grant_id
}

/// Request from 10.0.0.5, approval by `approver`, one poll: the device token of a new device `lab-1`.
pub async fn mint(service: &IdentityDeviceGrants, approver: &Principal) -> DeviceToken {
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    approve(
        service,
        approver,
        &authorization.user_code,
        ip("198.51.100.9"),
    )
    .await;
    service
        .poll(&authorization.device_code)
        .await
        .expect("poll of an approved grant")
}

pub async fn token_count(conn: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM device_tokens")
        .fetch_one(conn)
        .await
        .expect("count device tokens")
}

pub async fn revoked_count(conn: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM device_tokens WHERE revoked_at IS NOT NULL")
        .fetch_one(conn)
        .await
        .expect("count revoked device tokens")
}

pub async fn grant_status(conn: &mut PgConnection, device_code: &RawToken) -> String {
    sqlx::query_scalar(
        "SELECT status FROM device_grants WHERE device_code_hash = sha256(convert_to($1, 'UTF8'))",
    )
    .bind(device_code.expose())
    .fetch_one(conn)
    .await
    .expect("read grant status")
}

/// The stored expiry of the grant behind `user_code`, in microseconds.
pub async fn grant_expiry(conn: &mut PgConnection, user_code: &UserCode) -> i64 {
    sqlx::query_scalar(
        "SELECT (extract(epoch FROM expires_at) * 1000000)::bigint FROM device_grants \
         WHERE user_code = $1",
    )
    .bind(user_code.as_str())
    .fetch_one(conn)
    .await
    .expect("read grant expiry")
}

pub async fn grant_id(conn: &mut PgConnection, user_code: &UserCode) -> Uuid {
    sqlx::query_scalar("SELECT id FROM device_grants WHERE user_code = $1")
        .bind(user_code.as_str())
        .fetch_one(conn)
        .await
        .expect("read grant id")
}

/// `(account_id, approved_by_session_id)` of the grant behind `user_code`.
pub async fn grant_approver(
    conn: &mut PgConnection,
    user_code: &UserCode,
) -> (Option<Uuid>, Option<Uuid>) {
    sqlx::query_as(
        "SELECT account_id, approved_by_session_id FROM device_grants WHERE user_code = $1",
    )
    .bind(user_code.as_str())
    .fetch_one(conn)
    .await
    .expect("read grant approver")
}
