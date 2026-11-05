//! AC-9: `Db::health` and `Db::stats` with a pending migration and with a stopped database.

use std::time::{Duration, Instant};

use super::proxy::Proxy;
use super::support::{TestDb, admin_options};
use crate::{Db, DbConfig, DbError, DbRole, Migrator};

/// The connection that verified the role goes back to the pool in a task of its own; wait for it.
async fn settled_idle_health(api: &Db) -> crate::DbHealth {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let health = api.health().await;
        if health.pool_in_use == 0 || Instant::now() > deadline {
            return health;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn settled_idle_stats(api: &Db) -> crate::DbStats {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let stats = api.stats();
        if stats.pool_in_use == 0 || Instant::now() > deadline {
            return stats;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn health_reports_version_pending_and_pool_in_use() {
    let db = TestDb::empty().await;
    let api = db.api().await;

    let idle = settled_idle_health(&api).await;
    let busy = api
        .run_system("test_health_busy", async |_tx| {
            Ok::<_, DbError>(api.health().await)
        })
        .await
        .expect("run");

    assert!(idle.reachable);
    assert!(
        idle.server_version
            .as_deref()
            .is_some_and(|version| version.starts_with("18")),
        "{idle:?}"
    );
    assert_eq!(idle.migrations_pending, Some(1));
    assert_eq!(idle.pool_in_use, 0);
    assert!(busy.reachable);
    assert_eq!(
        busy.pool_in_use, 1,
        "the connection of the open transaction"
    );
    let stats = settled_idle_stats(&api).await;
    assert_eq!(stats.migrations_pending, Some(1));
    assert_eq!(stats.pool_max, 5);
    assert_eq!(stats.pool_in_use, 0);
    let busy_op = stats
        .queries
        .iter()
        .find(|q| q.op == "test_health_busy")
        .expect("op recorded");
    assert_eq!(busy_op.count, 1);
    assert!(busy_op.max >= Duration::ZERO && busy_op.total >= busy_op.max);
    db.drop_db().await;
}

#[tokio::test]
async fn health_counts_zero_pending_after_the_migration_ran() {
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    let api = db.api().await;
    Migrator::run_pending(&migrator).await.expect("migrate");

    let health = api.health().await;

    assert_eq!(health.migrations_pending, Some(0));
    assert_eq!(api.stats().migrations_pending, Some(0));
    db.drop_db().await;
}

#[tokio::test]
async fn health_of_a_stopped_database_is_unreachable_within_the_acquire_timeout() {
    let db = TestDb::empty().await;
    let target = admin_options(&db.name);
    let proxy = Proxy::start(target.get_host(), target.get_port()).await;
    let cfg = DbConfig {
        db_acquire_timeout: Duration::from_secs(1),
        ..db.config_via("127.0.0.1", proxy.port())
    };
    let api = Db::connect(&cfg, DbRole::Api).await.expect("connect");
    assert!(api.health().await.reachable, "healthy before the outage");

    proxy.cut();
    let started = Instant::now();
    let health = api.health().await;
    let took = started.elapsed();

    assert!(!health.reachable, "{health:?}");
    assert_eq!(health.server_version, None);
    assert_eq!(health.migrations_pending, None);
    assert!(
        took < cfg.db_acquire_timeout + Duration::from_millis(500),
        "health blocked for {took:?}"
    );
    assert_eq!(
        api.stats().migrations_pending,
        Some(1),
        "the last reading is kept"
    );
    drop(proxy);
    db.drop_db().await;
}
