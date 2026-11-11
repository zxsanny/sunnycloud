//! AC-1 (ST-05-1): account B never sees account A's data through a cursor, a head or a read. A stands at seq 80,
//! B at seq 3, both in one installation epoch, with real stamps. The 1,000 cursor pairs come from a fixed-seed
//! generator, so a failure repeats.

use std::collections::HashSet;

use contracts::{ChangeFeed, ChangeFeedError, Cursor, ResyncReason};
use sc_observability::NoLabels;
use sqlx::Connection;
use uuid::Uuid;

use crate::hold_support::{Acct, Rig, config, cursor, principal};
use crate::read_support::EPOCH;

const A_STAMPS: i64 = 80;
const B_STAMPS: i64 = 3;
const PAIRS: usize = 1_000;

struct Pair {
    rig: Rig,
    b: Acct,
    a_nodes: HashSet<Uuid>,
    b_nodes: HashSet<Uuid>,
}

async fn node_ids(rig: &Rig, account: &Acct) -> HashSet<Uuid> {
    let mut conn = rig.fixture.owner_conn().await;
    let ids = sqlx::query_scalar("SELECT id FROM nodes WHERE account_id = $1 AND seq > 0")
        .bind(account.id)
        .fetch_all(&mut conn)
        .await
        .expect("read the stamped nodes");
    conn.close().await.expect("close owner connection");
    ids.into_iter().collect()
}

async fn a_at_80_b_at_3() -> Pair {
    let rig = Rig::new(&config(2_000, 10_000), 4).await;
    let a = rig.account().await;
    let b = rig.account().await;
    for _ in 0..A_STAMPS {
        rig.commit(&a).await;
    }
    for _ in 0..B_STAMPS {
        rig.commit(&b).await;
    }
    let a_nodes = node_ids(&rig, &a).await;
    let b_nodes = node_ids(&rig, &b).await;
    Pair {
        rig,
        b,
        a_nodes,
        b_nodes,
    }
}

/// xorshift64: enough spread for cursor fuzzing, no dependency.
struct Fuzz(u64);

impl Fuzz {
    fn below(&mut self, bound: u64) -> i64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        i64::try_from(self.0 % bound).expect("bound fits i64")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn b_reading_with_the_cursors_of_a_gets_cursor_ahead_or_only_its_own_items_and_its_own_head()
{
    // Arrange
    let pair = a_at_80_b_at_3().await;
    let me = principal(pair.b.id);

    // Act
    let with_a_cursor = pair.rig.feed_read_now(&me, cursor(50)).await;
    let from_zero = pair.rig.feed_read_now(&me, cursor(0)).await;
    let head = pair.rig.feed.head(pair.b.id).await;

    // Assert
    assert!(matches!(
        with_a_cursor,
        Err(ChangeFeedError::ResyncRequired {
            reason: ResyncReason::CursorAhead
        })
    ));
    let page = from_zero.expect("B's own page");
    let ids: HashSet<Uuid> = page.items.iter().map(|item| item.node_id).collect();
    assert_eq!(ids, pair.b_nodes);
    assert!(ids.is_disjoint(&pair.a_nodes));
    assert_eq!(page.next_cursor, cursor(B_STAMPS));
    assert_eq!(head.expect("head"), cursor(B_STAMPS));
    pair.rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_thousand_random_cursor_pairs_never_return_a_foreign_item_or_counter() {
    // Arrange
    let pair = a_at_80_b_at_3().await;
    let me = principal(pair.b.id);
    let mut fuzz = Fuzz(0x9E37_79B9_7F4A_7C15);
    let (mut answered, mut resyncs) = (0, 0_u64);

    // Act
    let mut outcomes = Vec::new();
    for _ in 0..PAIRS {
        let epoch = [EPOCH, EPOCH, fuzz.below(10), 0][usize::try_from(fuzz.below(4)).unwrap_or(0)];
        let at = Cursor {
            epoch,
            seq: fuzz.below(100),
        };
        outcomes.push((at, pair.rig.feed_read_now(&me, at).await));
    }

    // Assert
    for (at, outcome) in outcomes {
        match outcome {
            Ok(page) => {
                answered += 1;
                assert!(
                    page.items
                        .iter()
                        .all(|item| pair.b_nodes.contains(&item.node_id) && item.seq <= B_STAMPS),
                    "foreign item for {at:?}"
                );
                assert!(
                    page.next_cursor.seq <= B_STAMPS,
                    "foreign counter for {at:?}"
                );
            }
            Err(ChangeFeedError::ResyncRequired { reason }) => {
                resyncs += 1;
                let expected = if at.epoch != EPOCH {
                    ResyncReason::EpochChanged
                } else {
                    ResyncReason::CursorAhead
                };
                assert_eq!(reason, expected, "{at:?}");
            }
            Err(other) => panic!("{at:?} gave {other:?}"),
        }
    }
    assert_eq!(answered + resyncs, PAIRS as u64);
    assert!(
        answered > 0 && resyncs > 0,
        "the corpus reaches both outcomes"
    );
    assert_eq!(pair.rig.resyncs.get(NoLabels), resyncs);
    assert!(pair.a_nodes.is_disjoint(&pair.b_nodes));
    pair.rig.finish().await;
}
