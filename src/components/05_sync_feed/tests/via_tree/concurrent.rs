//! AC-69 "via 02" (IT-02-13 repeated from the feed side): 50 concurrent `publish` calls on distinct files of one
//! account, then 50 concurrent updates of those files, read back with the feed's own reader from `E.0` to head in
//! pages of 7. Each commit's `seq` is the one its own notify carried.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use contracts::{ChangeItem, Cursor, NodeKind, Seq};
use sc_clock::SystemClock;
use tokio::sync::Barrier;
use uuid::Uuid;

use crate::support::{Rig, commit, cursor, existing, new_file};

const WRITERS: usize = 50;
const PAGE: u64 = 7;

/// One commit as its task saw it: node, revision, notified `seq`.
type Commit = (Uuid, Uuid, Seq);

async fn rig() -> Rig {
    Rig::new(Arc::new(SystemClock::new()), PAGE).await
}

/// 50 tasks, each publishing one new file at a barrier.
async fn create_concurrently(rig: &Rig, blobs: &[Uuid]) -> Vec<Commit> {
    let barrier = Arc::new(Barrier::new(WRITERS));
    let tasks: Vec<_> = (0..WRITERS)
        .map(|index| {
            let (service, barrier) = (rig.service.clone(), barrier.clone());
            let (account, entry) = (
                rig.account,
                new_file(rig.root, &format!("f{index}.txt"), blobs[index]),
            );
            tokio::spawn(async move {
                barrier.wait().await;
                commit(&service, account, entry).await
            })
        })
        .collect();
    let mut commits = Vec::with_capacity(WRITERS);
    for task in tasks {
        commits.push(task.await.expect("create task"));
    }
    commits
}

/// 50 tasks, each updating a different file once at a barrier.
async fn update_concurrently(rig: &Rig, files: &[Commit], blobs: &[Uuid]) -> Vec<Commit> {
    let barrier = Arc::new(Barrier::new(WRITERS));
    let tasks: Vec<_> = files
        .iter()
        .zip(blobs)
        .map(|(&(node, revision, _), &blob)| {
            let (service, barrier, account) = (rig.service.clone(), barrier.clone(), rig.account);
            tokio::spawn(async move {
                barrier.wait().await;
                commit(&service, account, existing(node, revision, blob)).await
            })
        })
        .collect();
    let mut commits = Vec::with_capacity(WRITERS);
    for task in tasks {
        commits.push(task.await.expect("update task"));
    }
    commits
}

/// Pages from `E.0` until a page says there is no more; returns the pages and the final cursor.
async fn read_to_head(rig: &Rig) -> (Vec<Vec<ChangeItem>>, Cursor) {
    let (mut at, mut pages) = (cursor(0), Vec::new());
    loop {
        let page = rig
            .reader
            .read_page(rig.account, at, None)
            .await
            .expect("page");
        at = page.next_cursor;
        let more = page.has_more;
        if !page.items.is_empty() {
            pages.push(page.items);
        }
        if !more {
            return (pages, at);
        }
    }
}

/// Every page is non-decreasing in `seq` and ends on a `seq` boundary.
fn ordered(pages: &[Vec<ChangeItem>]) -> bool {
    let flat: Vec<Seq> = pages.iter().flatten().map(|item| item.seq).collect();
    let boundaries = pages
        .windows(2)
        .all(|pair| pair[0].last().map(|item| item.seq) < pair[1].first().map(|item| item.seq));
    flat.windows(2).all(|pair| pair[0] <= pair[1]) && boundaries
}

/// The files the reader saw: node -> (revision, seq), and how many items it saw in all.
fn seen_files(pages: &[Vec<ChangeItem>]) -> (BTreeMap<Uuid, (Option<Uuid>, Seq)>, usize) {
    let files: Vec<&ChangeItem> = pages
        .iter()
        .flatten()
        .filter(|item| item.kind == NodeKind::File)
        .collect();
    let seen = files
        .iter()
        .map(|item| (item.node_id, (item.revision_id, item.seq)))
        .collect();
    (seen, files.len())
}

fn expected(commits: &[Commit]) -> BTreeMap<Uuid, (Option<Uuid>, Seq)> {
    commits
        .iter()
        .map(|&(node, revision, seq)| (node, (Some(revision), seq)))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_concurrent_publishes_read_from_zero_show_each_file_once_at_seq_one_to_fifty_and_end_at_head()
 {
    // Arrange
    let rig = rig().await;
    let blobs = rig.blobs(WRITERS).await;

    // Act
    let commits = create_concurrently(&rig, &blobs).await;
    let (pages, last) = read_to_head(&rig).await;
    let head = rig.reader.head(rig.account).await.expect("head");

    // Assert
    let notified: BTreeSet<Seq> = commits.iter().map(|&(_, _, seq)| seq).collect();
    let (seen, items) = seen_files(&pages);
    let distinct: BTreeSet<Seq> = pages.iter().flatten().map(|item| item.seq).collect();
    assert_eq!(
        notified,
        (1..=50).collect(),
        "each publish took its own seq"
    );
    assert_eq!(items, WRITERS, "each file exactly once");
    assert_eq!(
        seen,
        expected(&commits),
        "each file in its committed state at its commit's seq"
    );
    assert_eq!(
        distinct,
        (1..=50).collect(),
        "seq 1..N, none skipped or repeated"
    );
    assert!(ordered(&pages), "pages in seq order on seq boundaries");
    assert!(pages.len() > 1, "the read paged");
    assert_eq!(last, head, "the final cursor is head");
    assert_eq!(head, cursor(50));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_concurrent_updates_read_from_zero_show_each_file_once_in_its_latest_state() {
    // Arrange
    let rig = rig().await;
    let blobs = rig.blobs(2 * WRITERS).await;
    let created = create_concurrently(&rig, &blobs[..WRITERS]).await;

    // Act
    let updated = update_concurrently(&rig, &created, &blobs[WRITERS..]).await;
    let (pages, last) = read_to_head(&rig).await;
    let head = rig.reader.head(rig.account).await.expect("head");

    // Assert
    let notified: BTreeSet<Seq> = updated.iter().map(|&(_, _, seq)| seq).collect();
    let (seen, items) = seen_files(&pages);
    let distinct: BTreeSet<Seq> = pages.iter().flatten().map(|item| item.seq).collect();
    assert_eq!(
        notified,
        (51..=100).collect(),
        "each update took its own seq"
    );
    assert_eq!(items, WRITERS, "each file once, not once per commit");
    assert_eq!(
        seen,
        expected(&updated),
        "each file at its latest revision and its last commit's seq"
    );
    assert_eq!(
        distinct,
        (51..=100).collect(),
        "only the latest seq of each file, none skipped or repeated"
    );
    assert!(ordered(&pages), "pages in seq order on seq boundaries");
    assert_eq!(last, head, "the final cursor is head");
    assert_eq!(head, cursor(100));
    rig.finish().await;
}
