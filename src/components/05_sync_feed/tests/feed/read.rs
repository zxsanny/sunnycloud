//! AC-1 to AC-4 and AC-6: the page query, the paging rules, the cursor rules and head on a real PostgreSQL 18.
//! Stamps are real; node edits are fixture SQL as the schema owner.

use contracts::{
    ChangeFeedError, ChangeItem, ChangePage, Cursor, FeedStamp, NodeKind, NodeState, ResyncReason,
    TouchedNodes,
};
use sc_db::Scope;
use sc_ids::FileHash;
use sqlx::Connection;
use sunnycloud_sync_feed::{PgFeedReader, PgFeedStamp};
use uuid::Uuid;

use crate::read_support::{EPOCH, FILE_HASH_BYTE, FILE_SIZE, cursor, feed_config, scenario};
use crate::stamp::account_with_nodes;
use crate::support::{self, Fixture};

fn seqs(page: &ChangePage) -> Vec<i64> {
    page.items.iter().map(|item| item.seq).collect()
}

fn ids(page: &ChangePage) -> Vec<Uuid> {
    page.items.iter().map(|item| item.node_id).collect()
}

#[tokio::test]
async fn each_node_appears_once_with_its_latest_state_and_every_field() {
    // Arrange
    let s = scenario(0).await;
    let root = s.stamped.root;

    // Act
    let page = s.stamped.read_from(0).await.expect("page");

    // Assert
    assert_eq!(ids(&page), [s.b, s.file, s.c, s.a, s.f]);
    assert_eq!(seqs(&page), [2, 3, 4, 5, 6]);
    assert!(!page.has_more);
    assert_eq!(page.next_cursor, cursor(6));
    let by_id = |id: Uuid| page.items.iter().find(|item| item.node_id == id).unwrap();
    assert_eq!(
        by_id(s.a),
        &ChangeItem {
            node_id: s.a,
            parent_id: Some(root),
            kind: NodeKind::Folder,
            name: "renamed".to_owned(),
            state: NodeState::Live,
            revision_id: None,
            size: None,
            content_hash: None,
            seq: 5,
        }
    );
    let tombstone = by_id(s.f);
    assert_eq!(tombstone.state, NodeState::Tombstone);
    assert_eq!((tombstone.kind, tombstone.seq), (NodeKind::Folder, 6));
    assert_eq!(tombstone.parent_id, Some(root));
    assert_eq!(
        by_id(s.file),
        &ChangeItem {
            node_id: s.file,
            parent_id: Some(root),
            kind: NodeKind::File,
            name: "doc.bin".to_owned(),
            state: NodeState::Live,
            revision_id: Some(s.revision),
            size: Some(FILE_SIZE),
            content_hash: Some(FileHash::from_bytes([FILE_HASH_BYTE; 32])),
            seq: 3,
        }
    );
    s.stamped.fixture.finish().await;
}

#[tokio::test]
async fn a_batch_sharing_one_seq_is_never_split_and_an_empty_page_follows() {
    // Arrange
    let s = scenario(500).await;
    let reader = s.stamped.reader();

    // Act
    let page = reader
        .read_page(s.stamped.account, cursor(6), Some(100))
        .await
        .expect("boundary page");
    let after = reader
        .read_page(s.stamped.account, page.next_cursor, Some(100))
        .await
        .expect("page after the boundary");

    // Assert
    assert_eq!(page.items.len(), 500);
    assert!(seqs(&page).iter().all(|seq| *seq == 7));
    let mut batch_ids = ids(&page);
    batch_ids.sort();
    let mut expected = s.batch.clone();
    expected.sort();
    assert_eq!(batch_ids, expected);
    assert!(!page.has_more);
    assert_eq!(page.next_cursor, cursor(7));
    assert!(after.items.is_empty());
    assert!(!after.has_more);
    assert_eq!(after.next_cursor, cursor(7));
    drop(reader);
    s.stamped.fixture.finish().await;
}

#[tokio::test]
async fn small_pages_walk_the_feed_and_cross_a_batch_in_one_page() {
    // Arrange
    let s = scenario(500).await;
    let reader = s.stamped.reader();
    let mut at = cursor(0);
    let mut pages = Vec::new();

    // Act
    loop {
        let page = reader
            .read_page(s.stamped.account, at, Some(2))
            .await
            .expect("page");
        at = page.next_cursor;
        let more = page.has_more;
        pages.push(page);
        if !more {
            break;
        }
    }

    // Assert
    let lengths: Vec<_> = pages.iter().map(|page| page.items.len()).collect();
    assert_eq!(lengths, [2, 2, 501]);
    assert_eq!(
        pages.iter().map(|page| page.has_more).collect::<Vec<_>>(),
        [true, true, false]
    );
    assert_eq!(pages[0].next_cursor, cursor(3));
    assert_eq!(pages[1].next_cursor, cursor(5));
    assert_eq!(at, cursor(7));
    drop(reader);
    s.stamped.fixture.finish().await;
}

#[tokio::test]
async fn a_configured_ceiling_caps_the_page_but_not_the_batch() {
    // Arrange
    let s = scenario(0).await;
    let capped = PgFeedReader::new(s.stamped.fixture.api.clone(), &feed_config(500, Some(2)));

    // Act
    let page = capped
        .read_page(s.stamped.account, cursor(0), Some(1000))
        .await
        .expect("page");

    // Assert
    assert_eq!(seqs(&page), [2, 3]);
    assert!(page.has_more);
    assert_eq!(page.next_cursor, cursor(3));
    drop(capped);
    s.stamped.fixture.finish().await;
}

#[tokio::test]
async fn reading_from_seq_5_returns_seq_6_and_7_and_a_revived_folder_moves_to_its_new_seq() {
    // Arrange
    let s = scenario(500).await;

    // Act
    let before = s.stamped.read_from(5).await.expect("before revival");
    s.stamped.set_tombstone(s.f, false).await;
    let revived = s.stamped.stamp_committed(&[s.f]).await.expect("seq 8");
    let after = s
        .stamped
        .reader()
        .read_page(s.stamped.account, cursor(5), Some(1000))
        .await
        .expect("after revival");

    // Assert
    assert_eq!(before.items.len(), 501);
    assert_eq!(before.items[0].node_id, s.f);
    assert_eq!(before.items[0].state, NodeState::Tombstone);
    assert!(before.items[1..].iter().all(|item| item.seq == 7));
    assert_eq!(revived, 8);
    assert_eq!(after.items.len(), 501);
    assert_eq!(after.items.iter().filter(|i| i.node_id == s.f).count(), 1);
    let folder = after.items.last().expect("revived folder is last");
    assert_eq!(
        (folder.node_id, folder.state, folder.seq),
        (s.f, NodeState::Live, 8)
    );
    assert!(after.items[..500].iter().all(|item| item.seq == 7));
    assert_eq!(after.next_cursor, cursor(8));
    s.stamped.fixture.finish().await;
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Page(usize),
    Resync(ResyncReason),
}

fn outcome(result: Result<ChangePage, ChangeFeedError>) -> Outcome {
    match result {
        Ok(page) => Outcome::Page(page.items.len()),
        Err(ChangeFeedError::ResyncRequired { reason }) => Outcome::Resync(reason),
        Err(error) => panic!("unexpected {error:?}"),
    }
}

#[tokio::test]
async fn cursors_are_judged_against_epoch_horizon_and_counter() {
    // Arrange: epoch 7, seq 50, purge horizon 20, one node changed after the horizon.
    let (stamped, ids) = account_with_nodes(2).await;
    stamped.set_feed(50, 20).await;
    let mut conn = stamped.fixture.owner_conn().await;
    sqlx::query("UPDATE nodes SET seq = 30 WHERE id = $1")
        .bind(ids[0])
        .execute(&mut conn)
        .await
        .expect("stamp directly");
    Connection::close(conn).await.expect("close");
    let cases = [
        (
            Cursor {
                epoch: EPOCH + 1,
                seq: 10,
            },
            Outcome::Resync(ResyncReason::EpochChanged),
        ),
        (cursor(19), Outcome::Resync(ResyncReason::BelowPurgeHorizon)),
        (cursor(20), Outcome::Page(1)),
        (cursor(50), Outcome::Page(0)),
        (cursor(51), Outcome::Resync(ResyncReason::CursorAhead)),
        (
            cursor(9_999_999_999),
            Outcome::Resync(ResyncReason::CursorAhead),
        ),
        (
            Cursor {
                epoch: EPOCH + 1,
                seq: 9_999_999_999,
            },
            Outcome::Resync(ResyncReason::EpochChanged),
        ),
    ];

    // Act
    let mut outcomes = Vec::new();
    for (at, _) in &cases {
        let result = stamped.reader().read_page(stamped.account, *at, None).await;
        outcomes.push(outcome(result));
    }

    // Assert
    let expected: Vec<_> = cases.into_iter().map(|(_, expected)| expected).collect();
    assert_eq!(outcomes, expected);
    stamped.fixture.finish().await;
}

#[tokio::test]
async fn a_refused_cursor_never_reads_a_page() {
    // Arrange
    let s = scenario(0).await;
    s.stamped.set_feed(6, 4).await;

    // Act
    let below = s.stamped.read_from(3).await;

    // Assert
    assert!(matches!(
        below,
        Err(ChangeFeedError::ResyncRequired {
            reason: ResyncReason::BelowPurgeHorizon
        })
    ));
    s.stamped.fixture.finish().await;
}

#[tokio::test]
async fn head_is_the_epoch_and_stamp_count_per_account_and_a_new_device_reads_only_later_changes() {
    // Arrange
    let (first, first_ids) = account_with_nodes(6).await;
    let other = Uuid::new_v4();
    let mut conn = first.fixture.owner_conn().await;
    support::account_with_feed(&mut conn, other, 99).await;
    let other_root = support::root_node(&mut conn, other).await;
    let other_nodes = support::insert_folders(&mut conn, other, other_root, 2)
        .await
        .expect("other nodes");
    Connection::close(conn).await.expect("close");
    let reader = first.reader();
    for node in &first_ids[..3] {
        first.stamp_committed(&[*node]).await.expect("stamp");
    }
    first
        .fixture
        .api
        .run(Scope::Account(other), async |tx| {
            PgFeedStamp
                .stamp(tx, other, TouchedNodes(other_nodes.clone()))
                .await
        })
        .await
        .expect("stamp other account");

    // Act
    let head = reader.head(first.account).await.expect("head");
    let mut owner = first.fixture.owner_conn().await;
    let listed: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes WHERE account_id = $1")
        .bind(first.account)
        .fetch_one(&mut owner)
        .await
        .expect("list the tree");
    Connection::close(owner).await.expect("close");
    let nothing_yet = reader
        .read_page(first.account, head, None)
        .await
        .expect("page");
    first
        .stamp_committed(&first_ids[3..5])
        .await
        .expect("later stamp");
    let later = reader
        .read_page(first.account, head, None)
        .await
        .expect("page");
    let other_head = reader.head(other).await.expect("other head");

    // Assert
    assert_eq!(head, cursor(3));
    assert_eq!(listed, 7);
    assert!(nothing_yet.items.is_empty());
    assert_eq!(later.items.len(), 2);
    assert!(later.items.iter().all(|item| item.seq == 4));
    assert_eq!(other_head, Cursor { epoch: 99, seq: 1 });
    drop(reader);
    first.fixture.finish().await;
}

#[tokio::test]
async fn an_account_without_a_feed_row_is_the_unexpected_tier_for_read_and_head() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let account = Uuid::new_v4();
    support::account(&mut conn, account).await;
    Connection::close(conn).await.expect("close");
    let reader = PgFeedReader::new(fixture.api.clone(), &feed_config(500, None));

    // Act
    let read = reader.read_page(account, cursor(0), None).await;
    let head = reader.head(account).await;

    // Assert
    for result in [read.map(drop), head.map(drop)] {
        let Err(ChangeFeedError::Unexpected(source)) = result else {
            panic!("expected the unexpected tier, got {result:?}");
        };
        assert_eq!(source.to_string(), "account feed missing");
    }
    drop(reader);
    fixture.finish().await;
}

#[tokio::test]
async fn a_cursor_of_another_account_reads_nothing_of_it() {
    // Arrange
    let s = scenario(0).await;
    let stranger = Uuid::new_v4();
    let mut conn = s.stamped.fixture.owner_conn().await;
    support::account_with_feed(&mut conn, stranger, EPOCH).await;
    Connection::close(conn).await.expect("close");

    // Act
    let page = s
        .stamped
        .reader()
        .read_page(stranger, cursor(0), None)
        .await
        .expect("page");

    // Assert
    assert!(page.items.is_empty());
    assert_eq!(page.next_cursor, cursor(0));
    s.stamped.fixture.finish().await;
}
