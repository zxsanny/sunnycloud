//! AZ-3709 AC-9 (IT-02-13, AC-69): 50 concurrent commits on one account feed exactly once, in order. Round A: 50
//! tasks each publish one new file at a barrier; round B: 50 tasks each update a different file twice. A reader
//! pages the change feed from cursor 0 with limit 7 during and after the writes. Each repetition uses a fresh
//! account on the same database; the `seq` of each commit is the one its own task's notify carried.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use contracts::{Cursor, EntryResult, PublishEntry, PublishResult, Seq, TreeCommit};
use sunnycloud_sync_feed::{FeedConfig, PgFeedReader};
use sunnycloud_tree::PgTreeService;
use tokio::sync::Barrier;
use uuid::Uuid;

use crate::mutation_support::{NOTIFIED, Tree};
use crate::publish_support::{TestEffect, actor, effects, existing, new_file};

const WRITERS: usize = 50;
const REPETITIONS: usize = 100;
const PAGE: u32 = 7;
const START: Cursor = Cursor { epoch: 1, seq: 0 };

type Pages = Vec<Vec<(Uuid, Seq)>>;

/// One commit: its node, its revision and the `seq` its notify carried.
async fn commit(service: &PgTreeService, account: Uuid, entry: PublishEntry) -> (Uuid, Uuid, Seq) {
    NOTIFIED
        .scope(Cell::new(None), async {
            let result = service
                .publish(
                    actor(account),
                    vec![entry],
                    effects(&TestEffect::recording()),
                    sc_testdb::START,
                )
                .await
                .expect("publish");
            let PublishResult::Published(results) = result else {
                panic!("unexpected replay");
            };
            let [
                EntryResult::Published {
                    node_id,
                    revision_id,
                    ..
                },
            ] = results[..]
            else {
                panic!("not published: {results:?}");
            };
            let seq = NOTIFIED.with(Cell::get).expect("notified after COMMIT");
            (node_id, revision_id, seq)
        })
        .await
}

fn reader(tree: &Tree) -> PgFeedReader {
    let config = FeedConfig {
        longpoll_wait_max: Duration::from_secs(30),
        longpoll_auth_recheck: Duration::from_secs(2),
        feed_safety_recheck: Duration::from_secs(10),
        feed_page_limit_default: u64::from(PAGE),
        feed_page_limit_max: None,
        max_holds_per_principal: 4,
    };
    PgFeedReader::new(tree.fixture.api.clone(), &config)
}

/// Pages from `START` until the writers are done and a page comes back empty.
async fn read_until(reader: PgFeedReader, account: Uuid, done: Arc<AtomicBool>) -> Pages {
    let (mut at, mut pages) = (START, Pages::new());
    loop {
        let finished = done.load(Ordering::SeqCst);
        let page = reader
            .read_page(account, at, Some(PAGE))
            .await
            .expect("page");
        if page.items.is_empty() {
            if finished {
                return pages;
            }
            tokio::task::yield_now().await;
            continue;
        }
        at = page.next_cursor;
        pages.push(
            page.items
                .iter()
                .map(|item| (item.node_id, item.seq))
                .collect(),
        );
    }
}

/// Every page is non-decreasing in `seq` and ends on a `seq` boundary: the next page starts at a higher `seq`.
fn pages_are_ordered(pages: &Pages) -> bool {
    let flat: Vec<Seq> = pages.iter().flatten().map(|(_, seq)| *seq).collect();
    let sorted = flat.windows(2).all(|pair| pair[0] <= pair[1]);
    let boundaries = pages
        .windows(2)
        .all(|pair| pair[0].last().map(|item| item.1) < pair[1].first().map(|item| item.1));
    sorted && boundaries
}

struct Repetition {
    round_a: Vec<Seq>,
    round_a_on_nodes: bool,
    round_b: Vec<Seq>,
    head: i64,
    node_seq_is_last_commit: bool,
    live_pages: Pages,
    final_pages: Pages,
    last_commits: BTreeMap<Uuid, Seq>,
}

async fn repetition(tree: &Tree) -> Repetition {
    let (account, root) = tree.add_account().await;
    let blobs = tree.blobs_of(account, 3 * WRITERS, "committed").await;
    let done = Arc::new(AtomicBool::new(false));
    let live = tokio::spawn(read_until(reader(tree), account, done.clone()));
    let barrier = Arc::new(Barrier::new(WRITERS));
    let round_a: Vec<_> = (0..WRITERS)
        .map(|index| {
            let (service, barrier, blob) = (tree.service.clone(), barrier.clone(), blobs[index]);
            tokio::spawn(async move {
                barrier.wait().await;
                let entry = new_file(root, &format!("f{index}.txt"), blob);
                commit(&service, account, entry).await
            })
        })
        .collect();
    let mut created = Vec::with_capacity(WRITERS);
    for task in round_a {
        created.push(task.await.expect("round A task"));
    }
    let after_a: BTreeMap<Uuid, Seq> = created.iter().map(|&(node, _, seq)| (node, seq)).collect();
    let round_a_on_nodes = node_seqs(tree, account).await == after_a;
    let barrier = Arc::new(Barrier::new(WRITERS));
    let round_b: Vec<_> = created
        .iter()
        .enumerate()
        .map(|(index, &(node, revision, _))| {
            let (service, barrier) = (tree.service.clone(), barrier.clone());
            let (first, second) = (blobs[WRITERS + index], blobs[2 * WRITERS + index]);
            tokio::spawn(async move {
                barrier.wait().await;
                let (_, updated, one) =
                    commit(&service, account, existing(node, Some(revision), first)).await;
                let (_, _, two) =
                    commit(&service, account, existing(node, Some(updated), second)).await;
                (node, one, two)
            })
        })
        .collect();
    let mut round_b_seqs = Vec::with_capacity(2 * WRITERS);
    let mut last_commits = BTreeMap::new();
    for task in round_b {
        let (node, one, two) = task.await.expect("round B task");
        round_b_seqs.extend([one, two]);
        last_commits.insert(node, two);
    }
    done.store(true, Ordering::SeqCst);
    let live_pages = live.await.expect("live reader");
    let final_pages = read_until(reader(tree), account, Arc::new(AtomicBool::new(true))).await;
    let node_seqs = node_seqs(tree, account).await;
    Repetition {
        round_a: created.iter().map(|&(_, _, seq)| seq).collect(),
        round_a_on_nodes,
        round_b: round_b_seqs,
        head: tree
            .count(&format!(
                "SELECT seq FROM account_feeds WHERE account_id = '{account}'"
            ))
            .await,
        node_seq_is_last_commit: node_seqs == last_commits,
        live_pages,
        final_pages,
        last_commits,
    }
}

async fn node_seqs(tree: &Tree, account: Uuid) -> BTreeMap<Uuid, Seq> {
    let mut admin = tree.fixture.db.admin().await;
    let rows: Vec<(Uuid, Seq)> =
        sqlx::query_as("SELECT id, seq FROM nodes WHERE account_id = $1 AND kind = 'file'")
            .bind(account)
            .fetch_all(&mut admin)
            .await
            .expect("node seqs");
    crate::mutation_support::close(admin).await;
    rows.into_iter().collect()
}

fn sorted(mut seqs: Vec<Seq>) -> Vec<Seq> {
    seqs.sort_unstable();
    seqs
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_concurrent_commits_feed_each_seq_once_in_order_one_hundred_times() {
    // Arrange
    let tree = Tree::new().await;
    let round_a: Vec<Seq> = (1..=50).collect();
    let round_b: Vec<Seq> = (51..=150).collect();

    // Act
    let mut runs = Vec::with_capacity(REPETITIONS);
    for _ in 0..REPETITIONS {
        runs.push(repetition(&tree).await);
    }

    // Assert
    for (index, run) in runs.iter().enumerate() {
        assert_eq!(
            sorted(run.round_a.clone()),
            round_a,
            "round A of run {index}"
        );
        assert!(
            run.round_a_on_nodes,
            "each round A node carries its commit's seq, run {index}"
        );
        assert_eq!(
            sorted(run.round_b.clone()),
            round_b,
            "round B of run {index}"
        );
        assert_eq!(run.head, 150, "run {index}");
        assert!(run.node_seq_is_last_commit, "run {index}");
        assert!(
            !run.live_pages.is_empty(),
            "the live reader read, run {index}"
        );
        assert!(
            pages_are_ordered(&run.live_pages),
            "live pages of run {index}"
        );
        assert!(
            pages_are_ordered(&run.final_pages),
            "final pages of run {index}"
        );
        let once: HashMap<Uuid, Seq> = run.final_pages.iter().flatten().copied().collect();
        let items = run.final_pages.iter().flatten().count();
        assert_eq!(items, WRITERS, "each file once, run {index}");
        assert!(
            run.last_commits
                .iter()
                .all(|(node, seq)| once.get(node) == Some(seq)),
            "the reader sees each file at its last commit, run {index}"
        );
    }
    tree.finish().await;
}
