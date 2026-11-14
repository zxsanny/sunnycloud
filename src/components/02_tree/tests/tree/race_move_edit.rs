//! AZ-3713 AC-70 (IT-02-14, interaction risk R3): a folder move and an edit of a file inside it serialise on the
//! account advisory lock in either order, and the edit, addressed by node id, always lands in the moved folder.
//! AZ-3713 AC-1: a move's cycle check runs under the account lock and before the stamp, the stamp last.
//! The real 05 stamp, notifier and page reader and the real 07a ledger on one PostgreSQL; every operation runs on
//! a pool connection of its own.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use contracts::{
    ChangeItem, Cursor, EntryResult, NodeState, NodeView, PublishOutcome, PublishResult,
    TreeCommit, TreeCommitError, TreeServiceError,
};
use sqlx::{Connection, PgConnection};
use sunnycloud_sync_feed::PgFeedReader;
use tokio::sync::Barrier;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::mutation_support::{Tree, close};
use crate::publish_concurrency::reader;
use crate::publish_support::{SIZE, actor, existing};
use crate::support::{file_with_revision, node};
use sc_testdb::statements::StatementLog;

const ITERATIONS: usize = 500;
/// Iterations `index % FORCE_EVERY == 0` force the move first, `== FORCE_EVERY / 2` the edit first: 50 each.
const FORCE_EVERY: usize = 10;
const SKEW_MAX_MICROS: u64 = 5_000;
/// The size of the fixture's first revision of each file (`file_with_revision`).
const FIXTURE_SIZE: i64 = 1;
const SUITE_LIMIT: Duration = Duration::from_secs(60);

/// The key `sc_db` locks for an account (`DbTx::advisory_lock_account`), taken here from a plain connection.
const HOLD_ACCOUNT_LOCK: &str =
    "SELECT pg_advisory_xact_lock(hashtextextended('sunnycloud.account:' || $1::text, 0))";

const WAITING_ADVISORY: &str = "SELECT count(*) FROM pg_locks \
    WHERE locktype = 'advisory' AND NOT granted \
    AND database = (SELECT oid FROM pg_database WHERE datname = current_database())";

/// `(used_bytes, node rows of the account, children of the folder)`.
const STATE: &str = "SELECT (SELECT used_bytes FROM quota WHERE account_id = $1), \
    (SELECT count(*) FROM nodes WHERE account_id = $1), \
    (SELECT count(*) FROM nodes WHERE account_id = $1 AND parent_id = $2)";

const POLL: Duration = Duration::from_millis(1);
const POLL_LIMIT: u32 = 5_000;

/// One iteration's rows: folder F holding file f at revision r1, the move target G, and the blob of the edit.
#[derive(Clone, Copy)]
struct Case {
    folder: Uuid,
    target: Uuid,
    file: Uuid,
    first_revision: Uuid,
    blob: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    MoveFirst,
    EditFirst,
}

/// How the two calls start.
enum Start {
    /// At a barrier; the delayed side sleeps `skew` after it.
    Random { skew: Duration, delay_move: bool },
    /// Both queue on the account lock held by a third connection, `Order` first.
    Forced(Order),
}

/// splitmix64: the skew and the delayed side of the random iterations, reproducible from the seed in a failure.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

fn start_of(index: usize, rng: &mut Rng) -> Start {
    match index % FORCE_EVERY {
        0 => Start::Forced(Order::MoveFirst),
        half if half == FORCE_EVERY / 2 => Start::Forced(Order::EditFirst),
        _ => Start::Random {
            skew: Duration::from_micros(rng.next() % (SKEW_MAX_MICROS + 1)),
            delay_move: rng.next().is_multiple_of(2),
        },
    }
}

/// `ITERATIONS` cases under the root, seeded by the fixture on one connection.
async fn seed(tree: &Tree) -> Vec<Case> {
    let blobs = tree.blobs(ITERATIONS).await;
    let mut owner = tree.fixture.owner_conn().await;
    let mut cases = Vec::with_capacity(ITERATIONS);
    for (index, blob) in blobs.into_iter().enumerate() {
        let account = tree.account;
        let folder = node(
            &mut owner,
            account,
            Some(tree.root),
            "folder",
            &format!("F{index}"),
        )
        .await
        .expect("insert F");
        let target = node(
            &mut owner,
            account,
            Some(tree.root),
            "folder",
            &format!("G{index}"),
        )
        .await
        .expect("insert G");
        let (file, first_revision, _) =
            file_with_revision(&mut owner, account, folder, "f.txt").await;
        cases.push(Case {
            folder,
            target,
            file,
            first_revision,
            blob,
        });
    }
    close(owner).await;
    cases
}

type MoveHandle = JoinHandle<Result<NodeView, TreeServiceError>>;
type EditHandle = JoinHandle<Result<PublishResult, TreeCommitError>>;

/// `update_node(F -> G)`, started after `barrier` and `delay` when given.
fn spawn_move(
    tree: &Tree,
    case: Case,
    barrier: Option<Arc<Barrier>>,
    delay: Duration,
) -> MoveHandle {
    let (service, account) = (tree.service.clone(), tree.account);
    tokio::spawn(async move {
        released(barrier, delay).await;
        service
            .update_node(account, case.folder, None, Some(case.target))
            .await
    })
}

/// `publish(Existing{f, base = r1})`, started after `barrier` and `delay` when given.
fn spawn_edit(
    tree: &Tree,
    case: Case,
    barrier: Option<Arc<Barrier>>,
    delay: Duration,
) -> EditHandle {
    let (service, account) = (tree.service.clone(), tree.account);
    tokio::spawn(async move {
        released(barrier, delay).await;
        let entry = existing(case.file, Some(case.first_revision), case.blob);
        service
            .publish(actor(account), vec![entry], Vec::new(), sc_testdb::START)
            .await
    })
}

async fn released(barrier: Option<Arc<Barrier>>, delay: Duration) {
    if let Some(barrier) = barrier {
        barrier.wait().await;
    }
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
}

async fn wait_for_waiters(probe: &mut PgConnection, waiters: i64) {
    for _ in 0..POLL_LIMIT {
        let waiting: i64 = sqlx::query_scalar(WAITING_ADVISORY)
            .fetch_one(&mut *probe)
            .await
            .expect("read pg_locks");
        if waiting == waiters {
            return;
        }
        tokio::time::sleep(POLL).await;
    }
    panic!("{waiters} calls never queued on the account lock");
}

/// Runs both calls of one iteration; `holder` and `probe` are the third and fourth connections of a forced order.
async fn race(
    tree: &Tree,
    case: Case,
    start: &Start,
    holder: &mut PgConnection,
    probe: &mut PgConnection,
) -> (NodeView, EntryResult) {
    let (moved, edited) = match *start {
        Start::Random { skew, delay_move } => {
            let barrier = Arc::new(Barrier::new(2));
            let (move_delay, edit_delay) = if delay_move {
                (skew, Duration::ZERO)
            } else {
                (Duration::ZERO, skew)
            };
            let moved = spawn_move(tree, case, Some(barrier.clone()), move_delay);
            let edited = spawn_edit(tree, case, Some(barrier), edit_delay);
            (moved, edited)
        }
        Start::Forced(first) => {
            let mut held = holder.begin().await.expect("begin the holder");
            sqlx::query(HOLD_ACCOUNT_LOCK)
                .bind(tree.account)
                .execute(&mut *held)
                .await
                .expect("hold the account lock");
            let (moved, edited) = if first == Order::MoveFirst {
                let moved = spawn_move(tree, case, None, Duration::ZERO);
                wait_for_waiters(probe, 1).await;
                (moved, spawn_edit(tree, case, None, Duration::ZERO))
            } else {
                let edited = spawn_edit(tree, case, None, Duration::ZERO);
                wait_for_waiters(probe, 1).await;
                (spawn_move(tree, case, None, Duration::ZERO), edited)
            };
            wait_for_waiters(probe, 2).await;
            held.commit().await.expect("release the account lock");
            (moved, edited)
        }
    };
    let moved = moved.await.expect("move task").expect("the move succeeds");
    let edited = match edited.await.expect("edit task").expect("the edit succeeds") {
        PublishResult::Published(mut results) if results.len() == 1 => results.remove(0),
        other => panic!("one published entry expected, got {other:?}"),
    };
    (moved, edited)
}

/// The two feed items of the iteration that started at `seq`: F's and f's, in `seq` order.
async fn feed_items(reader: &PgFeedReader, account: Uuid, seq: i64) -> Vec<ChangeItem> {
    let page = reader
        .read_page(account, Cursor { epoch: 1, seq }, Some(10))
        .await
        .expect("read the feed");
    assert!(!page.has_more, "nothing after the iteration's two changes");
    page.items
}

/// What one iteration produced: both results, the feed page after it and the `STATE` probe.
struct Observed {
    moved: NodeView,
    edited: EntryResult,
    items: Vec<ChangeItem>,
    state: (i64, i64, i64),
}

/// What the iteration must leave: the feed `seq` it started at, the usage and the node rows after it.
struct Expected {
    seq: i64,
    used: i64,
    nodes: i64,
}

/// Checks one iteration's outcome and returns the order the two calls committed in.
fn check(case: &Case, context: &str, seen: &Observed, expected: &Expected) -> Order {
    let (moved, items) = (&seen.moved, &seen.items);
    let EntryResult::Published {
        outcome,
        node_id,
        revision_id,
        conflict_copy,
    } = &seen.edited
    else {
        panic!("{context}: the edit failed: {:?}", seen.edited);
    };
    assert_eq!(*node_id, case.file, "{context}: f keeps its node id");
    assert!(
        matches!(outcome, PublishOutcome::Updated),
        "{context}: {outcome:?}"
    );
    assert_eq!(*conflict_copy, None, "{context}: no conflict copy");
    assert_eq!(
        (moved.id, moved.parent_id),
        (case.folder, Some(case.target)),
        "{context}: F moved under G"
    );
    assert_eq!(
        items.len(),
        2,
        "{context}: both changes and nothing else: {items:#?}"
    );
    let folder_item = items
        .iter()
        .find(|item| item.node_id == case.folder)
        .unwrap_or_else(|| panic!("{context}: the move is in the feed: {items:#?}"));
    let file_item = items
        .iter()
        .find(|item| item.node_id == case.file)
        .unwrap_or_else(|| panic!("{context}: the edit is in the feed: {items:#?}"));
    assert_eq!(
        folder_item.parent_id,
        Some(case.target),
        "{context}: F.parent = G"
    );
    assert_eq!(
        file_item.parent_id,
        Some(case.folder),
        "{context}: f.parent = F"
    );
    assert_eq!(file_item.state, NodeState::Live, "{context}");
    assert_eq!(
        (file_item.revision_id, file_item.size),
        (Some(*revision_id), Some(SIZE)),
        "{context}: the new revision is current"
    );
    let mut seqs = [folder_item.seq, file_item.seq];
    seqs.sort_unstable();
    assert_eq!(
        seqs,
        [expected.seq + 1, expected.seq + 2],
        "{context}: one stamp per call"
    );
    assert_eq!(
        moved.seq, folder_item.seq,
        "{context}: the move's view carries its seq"
    );
    let (used, nodes, children) = seen.state;
    assert_eq!(
        used, expected.used,
        "{context}: the usage delta is applied once"
    );
    assert_eq!(
        nodes, expected.nodes,
        "{context}: no node row added (0 conflict copies)"
    );
    assert_eq!(children, 1, "{context}: F holds f alone");
    if folder_item.seq < file_item.seq {
        Order::MoveFirst
    } else {
        Order::EditFirst
    }
}

async fn state(probe: &mut PgConnection, account: Uuid, folder: Uuid) -> (i64, i64, i64) {
    sqlx::query_as(STATE)
        .bind(account)
        .bind(folder)
        .fetch_one(probe)
        .await
        .expect("read usage and node counts")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_move_racing_an_edit_inside_always_leaves_the_edit_in_the_moved_folder() {
    // Arrange
    let started = Instant::now();
    let tree = Tree::new().await;
    let cases = seed(&tree).await;
    let reader = reader(&tree);
    let mut holder = tree.fixture.owner_conn().await;
    let mut probe = tree.fixture.db.admin().await;
    let (used_before, nodes, _) = state(&mut probe, tree.account, cases[0].folder).await;
    assert_eq!(
        used_before,
        FIXTURE_SIZE * i64::try_from(ITERATIONS).expect("small")
    );
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after the epoch");
    let seed_value = since_epoch.as_secs() ^ u64::from(since_epoch.subsec_nanos());
    let mut rng = Rng(seed_value);
    let (mut seq, mut used) = (0i64, used_before);
    let mut forced = [0usize; 2];
    let mut random = [0usize; 2];

    // Act + Assert, per iteration: the two calls of one iteration share nothing with the next one's rows.
    for (index, case) in cases.iter().enumerate() {
        let start = start_of(index, &mut rng);
        let context = format!("iteration {index} (seed {seed_value})");
        let (moved, edited) = race(&tree, *case, &start, &mut holder, &mut probe).await;
        let seen = Observed {
            moved,
            edited,
            items: feed_items(&reader, tree.account, seq).await,
            state: state(&mut probe, tree.account, case.folder).await,
        };
        used += SIZE - FIXTURE_SIZE;
        let order = check(case, &context, &seen, &Expected { seq, used, nodes });
        let slot = usize::from(order == Order::EditFirst);
        match start {
            Start::Forced(first) => {
                assert_eq!(order, first, "{context}: the forced order held");
                forced[slot] += 1;
            }
            Start::Random { .. } => random[slot] += 1,
        }
        seq += 2;
    }

    // Assert
    assert_eq!(forced, [50, 50], "each order forced in 50 iterations");
    assert_eq!(random.iter().sum::<usize>(), 400);
    assert!(
        random.iter().all(|count| *count > 0),
        "random skew produced both orders: {random:?} (seed {seed_value})"
    );
    assert_eq!(
        tree.seq().await,
        2 * i64::try_from(ITERATIONS).expect("small")
    );
    assert_eq!(tree.live_bytes().await, used, "stored usage is live usage");
    let elapsed = started.elapsed();
    assert!(elapsed < SUITE_LIMIT, "the suite took {elapsed:?}");
    close(holder).await;
    close(probe).await;
    drop(reader);
    tree.finish().await;
}

fn position(log: &[String], what: &str, needle: &str) -> usize {
    log.iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no {what} in {log:#?}"))
}

#[tokio::test]
async fn a_move_with_a_30_level_cycle_check_runs_it_under_the_lock_before_the_stamp_and_stamps_last()
 {
    // Arrange: the target sits 31 levels below the root, so the walk up from it covers 30 folders and the root.
    let tree = Tree::new().await;
    let mover = tree.folder(tree.root, "mover").await;
    let mut deepest = tree.folder(tree.root, "top").await;
    for level in 1..=30 {
        deepest = tree.folder(deepest, &format!("level{level}")).await;
    }
    let log = StatementLog::install();

    // Act
    let moved = tree
        .service
        .update_node(tree.account, mover, None, Some(deepest))
        .await;
    let statements: Vec<String> = log
        .take()
        .iter()
        .map(|line| line.trim().to_owned())
        .collect();

    // Assert
    let moved = moved.expect("the move succeeds");
    assert_eq!(moved.parent_id, Some(deepest));
    let lock = position(&statements, "account lock", "pg_advisory_xact_lock");
    let stamp = position(&statements, "stamp", "UPDATE account_feeds");
    let cycle_check: Vec<usize> = (0..statements.len())
        .filter(|&index| {
            let line = &statements[index];
            line.contains("FOR SHARE OF n") || line.contains("WITH RECURSIVE chain")
        })
        .collect();
    assert_eq!(
        cycle_check.len(),
        2,
        "the target lock and the walk: {statements:#?}"
    );
    assert!(
        cycle_check
            .iter()
            .all(|&index| lock < index && index < stamp),
        "every cycle-check statement runs under the lock and before the stamp: {statements:#?}"
    );
    assert_eq!(
        &statements[stamp + 1..],
        ["COMMIT", "notify"],
        "0 statements between the stamp and COMMIT; notify after COMMIT"
    );
    tree.finish().await;
}
