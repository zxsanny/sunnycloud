//! Helpers of the account-administration tests: the component on the harness, accounts with credentials, and
//! probes of the rows of one account as the schema owner.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::{Connection, PgConnection};
use sunnycloud_identity::IdentityAccounts;
use uuid::Uuid;

use super::auth::FAR_FUTURE;
use super::{Harness, START, fixtures};

const WAIT_POLL: Duration = Duration::from_millis(20);
const WAIT_LIMIT: Duration = Duration::from_secs(10);

pub fn accounts(harness: &Harness) -> Arc<IdentityAccounts> {
    Arc::new(IdentityAccounts::new(
        harness.api.clone(),
        harness.clock.clone(),
    ))
}

/// Access token of session `n` (1 or 2) of the account seeded under `tag`.
pub fn session_token(tag: &str, n: u8) -> String {
    format!("{tag}-s{n}")
}

/// Device token of device `n` (1 or 2) of the account seeded under `tag`.
pub fn device_token(tag: &str, n: u8) -> String {
    format!("{tag}-d{n}")
}

/// A new active member account with 2 live sessions and 2 device tokens named after `tag`.
pub async fn seed(conn: &mut PgConnection, tag: &str) -> Uuid {
    let (account, at) = (Uuid::new_v4(), START.as_unix_micros());
    fixtures::account(conn, account, at).await;
    for n in 1..=2 {
        fixtures::live_session(
            conn,
            account,
            &session_token(tag, n),
            "browser",
            FAR_FUTURE,
            at,
        )
        .await;
        fixtures::named_device(conn, account, &device_token(tag, n), "laptop", at).await;
    }
    account
}

/// `(revoked sessions, revoked device tokens)` of the account.
pub async fn revoked(conn: &mut PgConnection, account: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM sessions WHERE account_id = $1 AND revoked_at IS NOT NULL), \
         (SELECT count(*) FROM device_tokens WHERE account_id = $1 AND revoked_at IS NOT NULL)",
    )
    .bind(account)
    .fetch_one(conn)
    .await
    .expect("count revoked credentials")
}

/// The distinct revoke reasons of the account's sessions and device tokens, sorted.
pub async fn revoke_reasons(conn: &mut PgConnection, account: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT DISTINCT reason FROM (SELECT revoke_reason AS reason FROM sessions WHERE account_id = $1 \
         UNION ALL SELECT revoke_reason FROM device_tokens WHERE account_id = $1) r \
         WHERE reason IS NOT NULL ORDER BY reason",
    )
    .bind(account)
    .fetch_all(conn)
    .await
    .expect("read revoke reasons")
}

pub async fn state_of(conn: &mut PgConnection, account: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM accounts WHERE id = $1")
        .bind(account)
        .fetch_one(conn)
        .await
        .expect("read account state")
}

/// Rows of the account in `identities`, `sessions`, `device_tokens`, `device_grants`, `login_codes`, `accounts`.
pub async fn owned_rows(conn: &mut PgConnection, account: Uuid) -> [i64; 6] {
    let row: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM identities WHERE account_id = $1), \
         (SELECT count(*) FROM sessions WHERE account_id = $1), \
         (SELECT count(*) FROM device_tokens WHERE account_id = $1), \
         (SELECT count(*) FROM device_grants WHERE account_id = $1), \
         (SELECT count(*) FROM login_codes WHERE account_id = $1), \
         (SELECT count(*) FROM accounts WHERE id = $1)",
    )
    .bind(account)
    .fetch_one(conn)
    .await
    .expect("count owned rows");
    [row.0, row.1, row.2, row.3, row.4, row.5]
}

/// Lock requests that are waiting in this database (`pg_locks`, not granted).
pub async fn waiting_locks(conn: &mut PgConnection) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid = l.pid \
         WHERE NOT l.granted AND a.datname = current_database()",
    )
    .fetch_one(conn)
    .await
    .expect("count waiting locks")
}

/// Polls until a lock request is waiting; panics after ten seconds so a missing wait fails instead of hanging.
pub async fn until_a_lock_waits(conn: &mut PgConnection) {
    let started = Instant::now();
    while waiting_locks(conn).await == 0 {
        assert!(started.elapsed() < WAIT_LIMIT, "no lock request is waiting");
        tokio::time::sleep(WAIT_POLL).await;
    }
}

/// The harness and a connection as the schema owner, for tests that seed and probe rows.
pub struct Fixture {
    pub harness: Harness,
    pub conn: PgConnection,
}

pub async fn fixture() -> Fixture {
    let harness = Harness::new().await;
    let conn = harness.fixture_conn().await;
    Fixture { harness, conn }
}

impl Fixture {
    pub async fn finish(self) {
        let Fixture { harness, conn } = self;
        conn.close().await.expect("close fixture connection");
        assert_eq!(harness.finish().await, 0, "leaked connections");
    }
}
