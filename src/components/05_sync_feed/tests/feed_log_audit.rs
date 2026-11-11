//! AC-8, log half: one run through every feed event that logs, captured at DEBUG. A stamp for an account without a
//! feed row logs one ERROR, a safety recheck that finds a change no notify announced logs one WARN, a bump logs one
//! INFO, and 50 `ResyncRequired` answers log nothing. Every line of the feed carries ids only, and the node name
//! the run reads never appears in any line. This binary holds this one test: the capturing subscriber is
//! installed on the test's thread, and a scoped default races with the callsite interest cache when other tests
//! run in parallel.

#[path = "feed/probe_clock.rs"]
mod probe_clock;

use std::collections::BTreeSet;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::{
    ChangeFeed, ChangeFeedError, Cursor, EpochAdmin, EpochMarker, EpochMarkerError, FeedStamp,
    Principal, PrincipalKind, PrincipalRevalidator, RevalidatorError, Role, TouchedNodes,
    async_trait,
};
use probe_clock::ProbeClock;
use sc_clock::{CancellationToken, Timestamp};
use sc_db::Scope;
use sc_observability::{
    BuildInfo, Component, LineSink, LogConfig, LogFormat, Metrics, NoLabels, subscriber,
};
use sc_testdb::{MIGRATOR, START, TestDb};
use sqlx::Connection;
use sunnycloud_sync_feed::{
    FeedConfig, InProcessNotifier, PgChangeFeed, PgEpochAdmin, PgFeedReader, PgFeedStamp,
};
use tracing::Level;
use uuid::Uuid;

const EPOCH: i64 = 3;
const SECRET_NAME: &str = "Quarterly-Salaries-SECRET";
const REFUSED_READS: u64 = 50;
const FEED_TARGET: &str = "sunnycloud_sync_feed";
const IDS_ONLY: [&str; 8] = [
    "ts",
    "level",
    "target",
    "msg",
    "request_id",
    "component",
    "account_id",
    "node_id",
];

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

struct AcceptingMarker;

#[async_trait]
impl EpochMarker for AcceptingMarker {
    async fn write(&self, _epoch: i64) -> Result<(), EpochMarkerError> {
        Ok(())
    }

    async fn read(&self) -> Result<Option<i64>, EpochMarkerError> {
        Ok(None)
    }
}

async fn account(conn: &mut sqlx::PgConnection, with_feed: bool) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
    )
    .bind(id)
    .bind(START.as_unix_micros())
    .execute(&mut *conn)
    .await
    .expect("insert account");
    if with_feed {
        sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, $2)")
            .bind(id)
            .bind(EPOCH)
            .execute(&mut *conn)
            .await
            .expect("insert feed row");
    }
    id
}

/// The keys of one flat JSON line: a quoted token followed by a colon. Values of these lines hold no quote.
fn keys_of(line: &str) -> BTreeSet<&str> {
    let tokens: Vec<&str> = line.split('"').collect();
    tokens
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 2 == 1)
        .filter(|(index, _)| {
            tokens
                .get(index + 1)
                .is_some_and(|next| next.starts_with(':'))
        })
        .map(|(_, token)| *token)
        .collect()
}

fn who(account: Uuid) -> Principal {
    Principal {
        account_id: account,
        kind: PrincipalKind::Session,
        session_or_device_id: Uuid::new_v4(),
        role: Role::User,
        label: "test".to_owned(),
    }
}

#[tokio::test]
async fn feed_log_lines_carry_ids_only_at_the_listed_levels_and_resync_answers_log_nothing() {
    // Arrange
    let db = TestDb::clone_template().await;
    let api = db.api().await;
    let mut conn = db.conn_as(MIGRATOR).await;
    let account = account(&mut conn, true).await;
    let without_feed = self::account(&mut conn, false).await;
    let (root, node) = (Uuid::new_v4(), Uuid::new_v4());
    sqlx::query(
        "INSERT INTO nodes (id, account_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ($1, $2, 'folder', '', '', 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(root)
    .bind(account)
    .execute(&mut conn)
    .await
    .expect("insert root");
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ($1, $2, $3, 'folder', $4, $4, 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(node)
    .bind(account)
    .bind(root)
    .bind(SECRET_NAME)
    .execute(&mut conn)
    .await
    .expect("insert node");
    conn.close().await.expect("close owner connection");
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::DEBUG,
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
    let resyncs = Arc::new(
        metrics
            .counter::<NoLabels>("feed_resync_required_total")
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
        resyncs.clone(),
        &config,
    ));
    let me = who(account);
    let at_start = Cursor {
        epoch: EPOCH,
        seq: 0,
    };
    let held = tokio::spawn({
        let (feed, me) = (feed.clone(), me.clone());
        async move {
            feed.read(&me, &Valid, at_start, Some(Duration::from_secs(30)), None)
                .await
        }
    });
    clock.sleeps_reach(1).await;

    // Act
    api.run(Scope::Account(account), async |tx| {
        PgFeedStamp
            .stamp(tx, account, TouchedNodes(vec![node]))
            .await
    })
    .await
    .expect("stamp");
    clock.tick(Duration::from_millis(200));
    let page = held
        .await
        .expect("hold task")
        .expect("page after the recheck");
    let mut refused = Vec::new();
    for index in 0..REFUSED_READS {
        let stale = Cursor {
            epoch: EPOCH + 1 + i64::try_from(index).expect("small"),
            seq: 0,
        };
        refused.push(feed.read(&me, &Valid, stale, None, None).await);
    }
    let missing = api
        .run(Scope::Account(without_feed), async |tx| {
            PgFeedStamp
                .stamp(tx, without_feed, TouchedNodes(vec![Uuid::new_v4()]))
                .await
        })
        .await;
    let admin = PgEpochAdmin::new(api.clone(), Arc::new(AcceptingMarker));
    admin
        .bump_epoch(Timestamp::from_unix_micros(START.as_unix_micros()))
        .await
        .expect("bump");

    // Assert
    assert_eq!(page.items[0].name, SECRET_NAME, "the name is in the data");
    assert!(
        refused
            .iter()
            .all(|answer| matches!(answer, Err(ChangeFeedError::ResyncRequired { .. })))
    );
    assert!(missing.is_err());
    assert_eq!(resyncs.get(NoLabels), REFUSED_READS);
    assert_eq!(missed.get(NoLabels), 1);
    let logged = lines
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert!(
        logged.iter().all(|line| !line.contains(SECRET_NAME)),
        "{logged:?}"
    );
    let feed_lines: Vec<&String> = logged
        .iter()
        .filter(|line| line.contains(&format!("\"target\":\"{FEED_TARGET}")))
        .collect();
    let expected = [
        ("warn", "safety recheck found changes a notify missed"),
        ("error", "account feed row missing"),
        ("info", "epoch bumped"),
    ];
    assert_eq!(feed_lines.len(), expected.len(), "{feed_lines:?}");
    for (line, (level, msg)) in feed_lines.iter().zip(expected) {
        assert!(line.contains(&format!("\"level\":\"{level}\"")), "{line}");
        assert!(line.contains(&format!("\"msg\":\"{msg}\"")), "{line}");
        assert!(
            keys_of(line).is_subset(&IDS_ONLY.into_iter().collect()),
            "{line}"
        );
    }
    assert!(feed_lines[0].contains(&account.to_string()));
    assert!(feed_lines[1].contains(&without_feed.to_string()));
    drop((feed, admin, api));
    assert_eq!(db.finish().await, 0, "connections left open");
}
