//! AC-4 of the mobile-code task (IT-01-14, janitor part): `IdentityMaintenance` removes only the rows past their
//! boundary, in every account, reports counts per table, and a second run deletes nothing. Rows sit on both
//! sides of each boundary; time comes from the manual clock. Real PostgreSQL.

mod support;

use std::time::Duration;

use contracts::Janitor;
use sc_clock::Clock;
use sqlx::PgConnection;
use sunnycloud_identity::{IdentityMaintenance, Swept};
use support::auth::FAR_FUTURE;
use support::{Harness, START, fixtures};
use uuid::Uuid;

const MINUTE: i64 = 60_000_000;
const DAY: i64 = 24 * 60 * MINUTE;
const MILLI: i64 = 1_000;

struct Fixture {
    harness: Harness,
    conn: PgConnection,
    now: i64,
    accounts: [Uuid; 2],
}

async fn fixture() -> Fixture {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let now = harness.now_micros();
    let accounts = [Uuid::new_v4(), Uuid::new_v4()];
    for account in accounts {
        fixtures::account(&mut conn, account, now).await;
    }
    Fixture {
        harness,
        conn,
        now,
        accounts,
    }
}

impl Fixture {
    fn maintenance(&self) -> IdentityMaintenance {
        IdentityMaintenance::new(self.harness.api.clone())
    }

    async fn finish(self) {
        let Fixture { harness, conn, .. } = self;
        sqlx::Connection::close(conn)
            .await
            .expect("close fixture connection");
        assert_eq!(harness.finish().await, 0, "leaked connections");
    }

    /// Rows on both sides of each boundary, spread over the two accounts; returns the labels that must remain.
    async fn seed(&mut self) -> Vec<&'static str> {
        let (now, [first, second]) = (self.now, self.accounts);
        let conn = &mut self.conn;
        fixtures::oauth_state(conn, "state-expired", "/x", now - MILLI)
            .await
            .expect("state");
        fixtures::oauth_state(conn, "state-live", "/x", now + MILLI)
            .await
            .expect("state");
        fixtures::login_code(conn, first, "code-expired", now - MILLI).await;
        fixtures::login_code(conn, second, "code-live", now + MILLI).await;
        fixtures::device_grant(
            conn,
            Some(first),
            "grant-pending-61",
            "U1",
            "approved",
            now - 61 * MINUTE,
        )
        .await
        .expect("grant");
        fixtures::device_grant(
            conn,
            Some(first),
            "grant-pending-59",
            "U2",
            "approved",
            now - 59 * MINUTE,
        )
        .await
        .expect("grant");
        fixtures::device_grant(
            conn,
            Some(second),
            "grant-consumed-61",
            "U3",
            "consumed",
            now - 61 * MINUTE,
        )
        .await
        .expect("grant");
        fixtures::device_grant(
            conn,
            Some(second),
            "grant-consumed-59",
            "U4",
            "consumed",
            now - 59 * MINUTE,
        )
        .await
        .expect("grant");
        fixtures::device_grant(
            conn,
            None,
            "grant-open-61",
            "U5",
            "pending",
            now - 61 * MINUTE,
        )
        .await
        .expect("grant");
        let revoked_old =
            fixtures::live_session(conn, first, "s-revoked-31", "web", FAR_FUTURE, now).await;
        fixtures::revoke_session(conn, revoked_old, now - 31 * DAY).await;
        let revoked_recent =
            fixtures::live_session(conn, second, "s-revoked-29", "web", FAR_FUTURE, now).await;
        fixtures::revoke_session(conn, revoked_recent, now - 29 * DAY).await;
        fixtures::live_session(
            conn,
            first,
            "s-expired-31",
            "web",
            now - 31 * DAY,
            now - 40 * DAY,
        )
        .await;
        fixtures::live_session(
            conn,
            second,
            "s-expired-29",
            "web",
            now - 29 * DAY,
            now - 40 * DAY,
        )
        .await;
        fixtures::live_session(conn, second, "s-live", "web", FAR_FUTURE, now).await;
        vec![
            "state-live",
            "code-live",
            "grant-pending-59",
            "grant-consumed-59",
            "s-revoked-29",
            "s-expired-29",
            "s-live",
        ]
    }

    /// The labels of the rows still present, sorted: token hashes of sessions are matched to the labels.
    async fn remaining(&mut self) -> Vec<String> {
        let mut labels: Vec<String> = Vec::new();
        for (table, column) in [
            ("oauth_states", "state_hash"),
            ("login_codes", "code_hash"),
            ("device_grants", "device_code_hash"),
            ("sessions", "token_hash"),
        ] {
            labels.extend(self.labels(table, column).await);
        }
        labels.sort();
        labels
    }

    async fn labels(&mut self, table: &str, column: &str) -> Vec<String> {
        const KNOWN: [&str; 14] = [
            "state-expired",
            "state-live",
            "code-expired",
            "code-live",
            "grant-pending-61",
            "grant-pending-59",
            "grant-consumed-61",
            "grant-consumed-59",
            "grant-open-61",
            "s-revoked-31",
            "s-revoked-29",
            "s-expired-31",
            "s-expired-29",
            "s-live",
        ];
        let mut found = Vec::new();
        for label in KNOWN {
            let sql = format!(
                "SELECT count(*) FROM {table} WHERE {column} = sha256(convert_to($1, 'UTF8'))"
            );
            let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                .bind(label)
                .fetch_one(&mut self.conn)
                .await
                .expect("count by label");
            if count > 0 {
                found.push(label.to_owned());
            }
        }
        found
    }
}

fn sorted(mut labels: Vec<&'static str>) -> Vec<String> {
    labels.sort_unstable();
    labels.into_iter().map(str::to_owned).collect()
}

#[tokio::test]
async fn sweep_deletes_exactly_the_rows_past_their_boundary_in_every_account_and_counts_them() {
    // Arrange
    let mut fixture = fixture().await;
    let expected_remaining = fixture.seed().await;
    let janitor = fixture.maintenance();
    let now = fixture.harness.clock.now_utc();

    // Act
    let first = janitor.sweep(now).await.expect("first sweep");
    let second = janitor.sweep(now).await.expect("second sweep");

    // Assert
    assert_eq!(
        first,
        Swept {
            oauth_states: 1,
            login_codes: 1,
            device_grants: 3,
            sessions: 2,
        }
    );
    assert_eq!(second, Swept::default());
    assert_eq!(fixture.remaining().await, sorted(expected_remaining));
    drop(janitor);
    fixture.finish().await;
}

#[tokio::test]
async fn run_cycle_reports_the_total_and_the_second_cycle_processes_nothing() {
    // Arrange
    let mut fixture = fixture().await;
    fixture.seed().await;
    let janitor = fixture.maintenance();
    let now = fixture.harness.clock.now_utc();

    // Act
    let first = janitor.run_cycle(now).await.expect("first cycle");
    let second = janitor.run_cycle(now).await.expect("second cycle");

    // Assert
    assert_eq!(janitor.name(), "identity");
    assert_eq!((first.processed, second.processed), (7, 0));
    drop(janitor);
    fixture.finish().await;
}

#[tokio::test]
async fn a_state_and_a_code_are_removed_once_the_clock_passes_their_expiry_and_not_before() {
    // Arrange
    let mut fixture = fixture().await;
    let (now, account) = (fixture.now, fixture.accounts[0]);
    let expiry = now + 1_000 * MILLI;
    fixtures::oauth_state(&mut fixture.conn, "state-expired", "/x", expiry)
        .await
        .expect("state");
    fixtures::login_code(&mut fixture.conn, account, "code-expired", expiry).await;
    let janitor = fixture.maintenance();
    fixture.harness.clock.tick(Duration::from_millis(999));
    let before = janitor
        .sweep(fixture.harness.clock.now_utc())
        .await
        .expect("sweep before");
    fixture.harness.clock.tick(Duration::from_millis(1));

    // Act
    let at_expiry = janitor
        .sweep(fixture.harness.clock.now_utc())
        .await
        .expect("sweep at expiry");

    // Assert
    assert_eq!(before, Swept::default());
    assert_eq!((at_expiry.oauth_states, at_expiry.login_codes), (1, 1));
    drop(janitor);
    fixture.finish().await;
}

#[tokio::test]
async fn an_empty_database_sweeps_to_zero() {
    // Arrange
    let fixture = fixture().await;
    let janitor = fixture.maintenance();

    // Act
    let swept = janitor.sweep(START).await.expect("sweep");

    // Assert
    assert_eq!(swept, Swept::default());
    drop(janitor);
    fixture.finish().await;
}

#[tokio::test]
async fn a_backlog_larger_than_one_batch_is_removed_in_full() {
    // Arrange
    let mut fixture = fixture().await;
    let now = fixture.now;
    sqlx::query(
        "INSERT INTO oauth_states (state_hash, provider, nonce, pkce_verifier, return_to, client_kind, \
         created_at, expires_at) \
         SELECT sha256(convert_to(n::text, 'UTF8')), 'github', 'n', 'v', '/x', 'web', \
         'epoch'::timestamptz + $1::bigint * interval '1 microsecond', \
         'epoch'::timestamptz + ($1::bigint - 1000) * interval '1 microsecond' \
         FROM generate_series(1, 2100) AS n",
    )
    .bind(now)
    .execute(&mut fixture.conn)
    .await
    .expect("insert backlog");
    fixtures::oauth_state(&mut fixture.conn, "state-live", "/x", now + MILLI)
        .await
        .expect("state");
    let janitor = fixture.maintenance();

    // Act
    let swept = janitor
        .sweep(fixture.harness.clock.now_utc())
        .await
        .expect("sweep");

    // Assert
    assert_eq!(swept.oauth_states, 2100);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_states")
        .fetch_one(&mut fixture.conn)
        .await
        .expect("count");
    assert_eq!(left, 1);
    drop(janitor);
    fixture.finish().await;
}
