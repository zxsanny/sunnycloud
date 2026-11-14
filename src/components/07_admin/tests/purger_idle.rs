//! IT-07-13 (AZ-3820 AC-5): with 20 active accounts and none `deleting`, 100 cycles run no INSERT, UPDATE or
//! DELETE, finalize nothing and each return within 100 ms; the name is stable and non-empty. The statements are
//! read from sqlx's statement log of the test thread (the purger runs on it).

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{AccountLifecycleHook, Janitor};
use sc_audit::AuditWriter;
use sc_clock::{Clock, SystemClock};
use sc_config::Environment;
use sc_testdb::statements::StatementLog;
use sc_testdb::{MIGRATOR, START, TestDb};
use sqlx::Connection;
use sunnycloud_admin::{AdminConfig, DeletionPurger};
use sunnycloud_identity::IdentityAccounts;
use sunnycloud_quota::{PgQuotaAdmin, QuotaConfig};
use sunnycloud_sharing::PgShareLifecycle;
use support::seed;
use support::settings::load_settings;
use uuid::Uuid;

const CYCLES: usize = 100;
const CYCLE_LIMIT: Duration = Duration::from_millis(100);
const WRITES: [&str; 3] = ["INSERT", "UPDATE", "DELETE"];

#[tokio::test]
async fn it_07_13_cycles_with_no_deleting_account_write_nothing_and_return_quickly() {
    // Arrange
    let settings = load_settings(Environment::Test, &[])
        .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
    let db = TestDb::clone_template().await;
    let api = db.api().await;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let mut conn = db.conn_as(MIGRATOR).await;
    for index in 0..20 {
        let created = START.as_unix_micros() + index;
        seed::account(
            &mut conn,
            Uuid::new_v4(),
            "Idle Fixture",
            "user",
            "active",
            created,
        )
        .await;
    }
    let hooks: Vec<Arc<dyn AccountLifecycleHook>> = vec![
        Arc::new(PgShareLifecycle::new(api.clone())),
        Arc::new(PgQuotaAdmin::new(
            api.clone(),
            QuotaConfig::from_settings(&settings),
        )),
    ];
    let purger = DeletionPurger::new(
        Arc::new(IdentityAccounts::new(api.clone(), clock.clone())),
        hooks,
        AdminConfig::from_settings(&settings),
        AuditWriter::new(api.clone(), clock.clone()),
        api.clone(),
        clock.clone(),
    );
    let log = StatementLog::install();

    // Act
    let mut slowest = Duration::ZERO;
    let mut processed = 0;
    for _ in 0..CYCLES {
        let started = Instant::now();
        processed += purger
            .run_cycle(clock.now_utc())
            .await
            .expect("cycle")
            .processed;
        slowest = slowest.max(started.elapsed());
    }
    let statements = log.take_data_statements();

    // Assert
    assert_eq!(processed, 0);
    assert!(slowest < CYCLE_LIMIT, "a cycle took {slowest:?}");
    assert!(!statements.is_empty(), "the listing was not captured");
    let writes: Vec<&String> = statements
        .iter()
        .filter(|sql| {
            let upper = sql.to_ascii_uppercase();
            WRITES.iter().any(|word| upper.contains(word))
        })
        .collect();
    assert!(writes.is_empty(), "{writes:#?}");
    assert_eq!(purger.name(), purger.name());
    assert!(!purger.name().is_empty());
    conn.close().await.expect("close");
    drop((purger, api));
    db.drop_db().await;
}
