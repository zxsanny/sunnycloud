//! AC-7 (IT-05-2 reader part): a reader loops over the feed while 50 writers stamp one account, some rolling
//! back. Order comes from a barrier per round and a channel, never from a clock: the writers run their first
//! round, the reader proves it has read a non-empty page, and only then the remaining rounds start.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use contracts::Cursor;
use sqlx::Connection;
use sunnycloud_sync_feed::PgFeedReader;
use tokio::sync::{Barrier, oneshot};
use uuid::Uuid;

use crate::read_support::feed_config;
use crate::stamp_concurrency::{committed, one_account};

const WRITERS: usize = 50;
const ROUNDS: usize = 20;
const ROLLBACK_EVERY: usize = 5;
const PAGE: u32 = 25;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reader_sees_every_committed_node_once_in_non_decreasing_seq_while_fifty_writers_stamp() {
    // Arrange
    let (fixture, writer) = one_account(WRITERS as u32 + 2).await;
    let reader = PgFeedReader::new(fixture.api.clone(), &feed_config(500, None));
    let account = writer.account;
    let writers_done = Arc::new(AtomicBool::new(false));
    let (first_page, first_page_seen) = oneshot::channel();
    let reading = {
        let writers_done = writers_done.clone();
        tokio::spawn(async move {
            let mut first_page = Some(first_page);
            let mut at = Cursor { epoch: 1, seq: 0 };
            let mut seen: Vec<(Uuid, i64)> = Vec::new();
            loop {
                let finished = writers_done.load(Ordering::SeqCst);
                let page = reader
                    .read_page(account, at, Some(PAGE))
                    .await
                    .expect("page");
                if let Some(signal) = first_page.take_if(|_| !page.items.is_empty()) {
                    signal.send(()).expect("test is waiting");
                }
                seen.extend(page.items.iter().map(|item| (item.node_id, item.seq)));
                at = page.next_cursor;
                if finished && page.items.is_empty() {
                    return seen;
                }
                tokio::task::yield_now().await;
            }
        })
    };

    // Act
    let mut first_page_seen = Some(first_page_seen);
    for round in 0..ROUNDS {
        let barrier = Arc::new(Barrier::new(WRITERS));
        let tasks: Vec<_> = (0..WRITERS)
            .map(|index| {
                let (writer, barrier) = (writer.clone(), barrier.clone());
                let commit = !(round * WRITERS + index).is_multiple_of(ROLLBACK_EVERY);
                tokio::spawn(async move {
                    barrier.wait().await;
                    writer.transaction(commit).await
                })
            })
            .collect();
        for task in tasks {
            committed(task.await.expect("writer task"));
        }
        if let Some(signal) = first_page_seen.take() {
            signal.await.expect("reader read a page while writers ran");
        }
    }
    writers_done.store(true, Ordering::SeqCst);
    let seen = reading.await.expect("reader task");

    // Assert
    let seqs: Vec<i64> = seen.iter().map(|(_, seq)| *seq).collect();
    assert!(
        seqs.windows(2).all(|pair| pair[0] <= pair[1]),
        "seq decreased"
    );
    let mut by_node = HashMap::new();
    for (node, seq) in &seen {
        assert!(by_node.insert(*node, *seq).is_none(), "{node} listed twice");
    }
    let mut conn = fixture.owner_conn().await;
    let stored: Vec<(Uuid, i64)> =
        sqlx::query_as("SELECT id, seq FROM nodes WHERE account_id = $1 AND seq > 0")
            .bind(account)
            .fetch_all(&mut conn)
            .await
            .expect("committed nodes");
    conn.close().await.expect("close owner connection");
    assert_eq!(
        stored.len(),
        ROUNDS * WRITERS - ROUNDS * WRITERS / ROLLBACK_EVERY
    );
    assert_eq!(by_node, stored.into_iter().collect::<HashMap<_, _>>());
    drop(writer);
    fixture.finish().await;
}
