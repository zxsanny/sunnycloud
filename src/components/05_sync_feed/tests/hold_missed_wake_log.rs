//! A commit no notify announced is healed by the safety recheck with one WARN line. This binary holds this one
//! test: the capturing subscriber is installed on the test's thread, and a scoped default races with the callsite
//! interest cache when other tests run in parallel.

#[path = "feed/probe_clock.rs"]
mod probe_clock;

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::{
    ChangeFeed, Cursor, FeedStamp, Principal, PrincipalKind, PrincipalRevalidator,
    RevalidatorError, Role, TouchedNodes, async_trait,
};
use probe_clock::ProbeClock;
use sc_clock::CancellationToken;
use sc_db::Scope;
use sc_observability::{
    BuildInfo, Component, LineSink, LogConfig, LogFormat, Metrics, NoLabels, subscriber,
};
use sc_testdb::{MIGRATOR, START, TestDb};
use sqlx::Connection;
use sunnycloud_sync_feed::{
    FeedConfig, InProcessNotifier, PgChangeFeed, PgFeedReader, PgFeedStamp,
};
use tracing::Level;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl LineSink for Lines {
    fn write_line(&self, line: &str) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
        Ok(())
    }
}

struct Valid;

#[async_trait]
impl PrincipalRevalidator for Valid {
    async fn revalidate(&self, _principal: &Principal) -> Result<bool, RevalidatorError> {
        Ok(true)
    }
}

const EPOCH: i64 = 3;

#[tokio::test]
async fn a_missed_wake_logs_one_warn_line_with_the_account_id_and_counts_once() {
    // Arrange
    let db = TestDb::clone_template().await;
    let api = db.api().await;
    let account = Uuid::new_v4();
    let node = Uuid::new_v4();
    let mut conn = db.conn_as(MIGRATOR).await;
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
    )
    .bind(account)
    .bind(START.as_unix_micros())
    .execute(&mut conn)
    .await
    .expect("insert account");
    sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, $2)")
        .bind(account)
        .bind(EPOCH)
        .execute(&mut conn)
        .await
        .expect("insert feed row");
    sqlx::query(
        "INSERT INTO nodes (id, account_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ($1, $2, 'folder', '', '', 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(node)
    .bind(account)
    .execute(&mut conn)
    .await
    .expect("insert node");
    conn.close().await.expect("close owner connection");
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::WARN,
            component: Component::Api,
        },
        sc_testdb::clock_at_start(),
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);
    let metrics = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics");
    let missed = Arc::new(
        metrics
            .counter::<NoLabels>("feed_missed_wake_total")
            .expect("counter"),
    );
    let config = FeedConfig {
        longpoll_wait_max: Duration::from_secs(30),
        longpoll_auth_recheck: Duration::from_secs(2),
        feed_safety_recheck: Duration::from_millis(200),
        feed_page_limit_default: 500,
        feed_page_limit_max: None,
        max_holds_per_principal: 4,
    };
    let clock = ProbeClock::new(START);
    let feed = Arc::new(PgChangeFeed::new(
        PgFeedReader::new(api.clone(), &config),
        InProcessNotifier::new(),
        clock.clone(),
        CancellationToken::new(),
        missed.clone(),
        &config,
    ));
    let who = Principal {
        account_id: account,
        kind: PrincipalKind::Session,
        session_or_device_id: Uuid::new_v4(),
        role: Role::User,
        label: "test".to_owned(),
    };
    let held = tokio::spawn({
        let feed = feed.clone();
        async move {
            feed.read(
                &who,
                &Valid,
                Cursor {
                    epoch: EPOCH,
                    seq: 0,
                },
                Some(Duration::from_secs(30)),
                None,
            )
            .await
        }
    });
    clock.sleeps_reach(1).await;
    api.run(Scope::Account(account), async |tx| {
        PgFeedStamp
            .stamp(tx, account, TouchedNodes(vec![node]))
            .await
    })
    .await
    .expect("stamp");

    // Act
    clock.tick(Duration::from_millis(200));
    let page = held.await.expect("hold task").expect("page");

    // Assert
    assert_eq!(page.items.len(), 1);
    assert_eq!(missed.get(NoLabels), 1);
    let logged = lines
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(logged[0].contains("\"level\":\"warn\""), "{}", logged[0]);
    assert!(logged[0].contains(&account.to_string()), "{}", logged[0]);
    drop((feed, api));
    assert_eq!(db.finish().await, 0, "connections left open");
}
