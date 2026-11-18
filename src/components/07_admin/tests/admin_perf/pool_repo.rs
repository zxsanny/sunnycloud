//! The server pids of a pool's connections, read in system scope: `pg_backend_pid()` touches no account row, and
//! the pool has no account to scope to. PT-07-2 uses them to find the purger's own transactions in
//! `pg_stat_activity`.

use std::sync::Arc;

use sc_db::Db;
use tokio::sync::Barrier;

/// The server pids of `db`'s pool of `connections` connections: all are held at once, so the pool opens each.
pub async fn pool_pids(db: &Db, connections: u32) -> Vec<i32> {
    let together = Arc::new(Barrier::new(connections as usize));
    let units: Vec<_> = (0..connections)
        .map(|_| {
            let (db, together) = (db.clone(), together.clone());
            tokio::spawn(async move {
                db.run_system("pt_07_2_pool_pids", async |tx| {
                    let pid: Result<i32, sqlx::Error> =
                        sqlx::query_scalar("SELECT pg_backend_pid()")
                            .fetch_one(tx.conn())
                            .await;
                    together.wait().await;
                    pid
                })
                .await
                .expect("read the pid of a pool connection")
            })
        })
        .collect();
    let mut pids = Vec::new();
    for unit in units {
        pids.push(unit.await.expect("pid unit"));
    }
    pids
}
