//! AZ-3823 AC-2 (ST-07-4, AC-27): the IT-07-2 changes on a user and an IT-07-9 deletion run on another, both named
//! `Zed Unique-7Q` with email `zed.unique7q@example.test`, through the real ports. Neither string appears in the
//! audit rows, the JSON log lines of the binary or the metrics output; the audit details hold action codes only.

use std::sync::Arc;

use contracts::{AccountDeletionService, AccountState, Actor, Role};
use sc_clock::{Clock, SystemClock};
use sc_observability::{BuildInfo, Metrics};
use serde_json::Value;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::capture;
use crate::ports::{Ports, live_session, principal};
use crate::purge_support::probes::{deleted_audits, drain, rows_of, total};
use crate::purge_support::seed;
use crate::purge_support::world::PurgeWorld;
use crate::support::seed::{account, admin, identity, quota};
use crate::support::world::session_credential;

const ZED_NAME: &str = "Zed Unique-7Q";
const ZED_EMAIL: &str = "zed.unique7q@example.test";
/// Fragments of both strings, so a partial copy (a lowercased name, the local part) is caught too.
const NEEDLES: [&str; 4] = [ZED_NAME, ZED_EMAIL, "Unique-7Q", "unique7q"];
const GIB: i64 = 1 << 30;

/// Every audit row as JSON text, and the details of the `admin_action` rows.
async fn audit_texts(conn: &mut PgConnection) -> (Vec<String>, Vec<Value>) {
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT row_to_json(audit_log)::text FROM audit_log")
            .fetch_all(&mut *conn)
            .await
            .expect("read audit rows");
    let details: Vec<String> = sqlx::query_scalar(
        "SELECT detail::text FROM audit_log WHERE event = 'admin_action' ORDER BY id",
    )
    .fetch_all(conn)
    .await
    .expect("read audit details");
    let details = details
        .iter()
        .map(|text| serde_json::from_str(text).expect("detail is JSON"))
        .collect();
    (rows, details)
}

/// `{"action": "<lowercase code>"}` and nothing else.
fn action_code_only(detail: &Value) -> bool {
    let Some(object) = detail.as_object() else {
        return false;
    };
    object.len() == 1
        && object
            .get("action")
            .and_then(Value::as_str)
            .is_some_and(|code| {
                !code.is_empty() && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            })
}

async fn name_as_zed(conn: &mut PgConnection, account_id: Uuid) {
    sqlx::query("UPDATE accounts SET display_name = $2 WHERE id = $1")
        .bind(account_id)
        .bind(ZED_NAME)
        .execute(&mut *conn)
        .await
        .expect("rename the account");
    sqlx::query("UPDATE identities SET email = $2 WHERE account_id = $1")
        .bind(account_id)
        .bind(ZED_EMAIL)
        .execute(conn)
        .await
        .expect("set the email");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn st_07_4_no_name_or_email_reaches_audit_logs_or_metrics() {
    // Arrange
    capture::install();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let world = PurgeWorld::new(clock).await;
    let ports = Ports::new(&world);
    // The keyring check runs before any blob row exists, as at a first start.
    let wiring = world.wiring(world.api.clone()).await;
    let metrics = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: false,
    })
    .expect("metrics");
    let mut conn = world.fixture().await;
    let (a, changed) = (Uuid::new_v4(), Uuid::new_v4());
    admin(&mut conn, a).await;
    let a_session = live_session(&mut conn, a).await;
    let start = sc_testdb::START.as_unix_micros();
    account(&mut conn, changed, ZED_NAME, "user", "active", start).await;
    quota(&mut conn, changed, None, 0, 0).await;
    identity(&mut conn, changed, ZED_EMAIL, Some(start)).await;
    let deleted = seed::account(&world, seed::files(50)).await.account;
    name_as_zed(&mut conn, deleted).await;
    let caller =
        Actor::Admin(principal(&ports.authenticator(&world), session_credential(a_session)).await);
    let purger = world
        .purger(
            wiring.accounts.clone(),
            wiring.hooks.clone(),
            world.api.clone(),
        )
        .with_metrics(&metrics)
        .expect("metric registered");

    // Act
    let admin_service = &ports.admin;
    let changes = [
        admin_service
            .update_user(&caller, changed, Some(Role::Admin), None)
            .await,
        admin_service
            .update_user(&caller, changed, Some(Role::User), None)
            .await,
        admin_service
            .update_user(&caller, changed, None, Some(AccountState::Disabled))
            .await,
        admin_service
            .set_quota(&caller, changed, Some(5 * GIB))
            .await,
        admin_service.set_quota(&caller, changed, None).await,
    ];
    ports
        .deletion
        .request_as_operator(deleted)
        .await
        .expect("deletion accepted");
    drain(&world, &purger, &world.sweeper(&wiring), deleted).await;
    let scrape = metrics.render().await.expect("metrics render");

    // Assert
    assert!(changes.iter().all(Result::is_ok), "{changes:?}");
    assert_eq!(total(&rows_of(&mut conn, deleted).await), 0);
    assert_eq!(deleted_audits(&mut conn, deleted).await, 1);
    let (rows, details) = audit_texts(&mut conn).await;
    assert_eq!(details.len(), 6, "5 changes and the deletion request");
    assert!(details.iter().all(action_code_only), "{details:?}");
    assert!(
        scrape.contains("sunnycloud_account_deletion_seconds_count 1"),
        "{scrape}"
    );
    assert!(
        capture::of_account(deleted)
            .iter()
            .any(|line| line.contains("account deleted")),
        "the purger's lines were not captured"
    );
    for needle in NEEDLES {
        let in_audit: Vec<&String> = rows.iter().filter(|row| row.contains(needle)).collect();
        assert!(in_audit.is_empty(), "{needle} in audit: {in_audit:#?}");
        let in_logs = capture::containing(needle);
        assert!(in_logs.is_empty(), "{needle} in logs: {in_logs:#?}");
        assert!(!scrape.contains(needle), "{needle} in metrics:\n{scrape}");
    }
    conn.close().await.expect("close");
    world.finish().await;
}
