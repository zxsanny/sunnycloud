//! The reader of the tests (AZ-3707) on the API role, files written through the real `publish`, and the DS-02
//! folder: 10,000 entries (9,500 files, 500 folders) with mixed case and Unicode names, the folders made by
//! `create_folder` and the files by 500-entry `publish` batches.

use contracts::{EntryResult, PublishResult, TreeCommit};
use sunnycloud_tree::{PgTreeReader, TreeConfig};
use uuid::Uuid;

use crate::maintenance_support;
use crate::mutation_support::Tree;
use crate::publish_support::{actor, new_file};

pub const DS02_ENTRIES: usize = 10_000;
const DS02_BATCH: usize = 500;
/// Every 20th entry is a folder: 500 of 10,000.
const DS02_FOLDER_EVERY: usize = 20;
const DS02_STEMS: [&str; 8] = [
    "alpha",
    "Beta",
    "ÇELIK",
    "straße",
    "Ωmega",
    "日本語",
    "ZETA",
    "Émile",
];

/// The reader with the default limit 1000 and `max_limit` as the configured `listing_max_limit`.
pub fn reader(tree: &Tree, max_limit: u64) -> PgTreeReader {
    let config = TreeConfig {
        listing_default_limit: 1000,
        listing_max_limit: max_limit,
        ..maintenance_support::config(5000, 5000)
    };
    PgTreeReader::new(tree.fixture.api.clone(), &config)
}

/// A file of `account` under `parent` over a fresh committed blob; returns its node, revision and blob.
pub async fn publish_file(
    tree: &Tree,
    account: Uuid,
    parent: Uuid,
    name: &str,
) -> (Uuid, Uuid, Uuid) {
    let blob = tree.blobs_of(account, 1, "committed").await[0];
    let result = tree
        .service
        .publish(
            actor(account),
            vec![new_file(parent, name, blob)],
            Vec::new(),
            sc_testdb::START,
        )
        .await
        .expect("publish");
    match result {
        PublishResult::Published(results) => match results.as_slice() {
            [
                EntryResult::Published {
                    node_id,
                    revision_id,
                    ..
                },
            ] => (*node_id, *revision_id, blob),
            other => panic!("expected one published file, got {other:?}"),
        },
        PublishResult::Replayed(_) => panic!("unexpected replay"),
    }
}

pub fn ds02_name(index: usize) -> String {
    format!("{} {index:05}", DS02_STEMS[index % DS02_STEMS.len()])
}

/// The DS-02 folder under the root of the tree's account, with fresh planner statistics; returns the folder and
/// the id of one of its files.
pub async fn ds02(tree: &Tree) -> (Uuid, Uuid) {
    let folder = tree
        .service
        .create_folder(tree.account, tree.root, "DS-02")
        .await
        .expect("create the DS-02 folder")
        .id;
    let mut files = Vec::new();
    for index in 0..DS02_ENTRIES {
        if index % DS02_FOLDER_EVERY == 0 {
            tree.service
                .create_folder(tree.account, folder, &ds02_name(index))
                .await
                .expect("create a DS-02 folder");
        } else {
            files.push(ds02_name(index));
        }
    }
    let blobs = tree.blobs(files.len()).await;
    let mut file = None;
    for (names, blobs) in files.chunks(DS02_BATCH).zip(blobs.chunks(DS02_BATCH)) {
        let entries = names
            .iter()
            .zip(blobs)
            .map(|(name, blob)| new_file(folder, name, *blob))
            .collect();
        let result = tree
            .service
            .publish(actor(tree.account), entries, Vec::new(), sc_testdb::START)
            .await
            .expect("publish a DS-02 batch");
        let PublishResult::Published(results) = result else {
            panic!("unexpected replay");
        };
        for result in results {
            match result {
                EntryResult::Published { node_id, .. } => file = Some(node_id),
                failed @ EntryResult::Failed(_) => panic!("DS-02 entry failed: {failed:?}"),
            }
        }
    }
    tree.exec("ANALYZE nodes").await;
    tree.exec("ANALYZE revisions").await;
    (folder, file.expect("DS-02 has files"))
}
