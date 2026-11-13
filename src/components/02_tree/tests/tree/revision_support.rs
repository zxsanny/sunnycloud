//! The revision data of the AZ-3716 tests (IT-02-18), written through the real `publish` and `delete_node`: 1,500
//! files with distinct hashes, the first 200 recorded by the device `pixel-7` and the next 50 by `tablet`, 100 of
//! the rest replaced by a new revision and 100 deleted; and the plans of the revision statements as the API role.

use contracts::{EntryResult, PublishEntry, PublishResult, TreeCommit};
use sc_db::Scope;
use sc_ids::FileHash;
use sqlx::AssertSqlSafe;
use sqlx::postgres::PgArguments;
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::publish_support::{actor, existing, new_file};

pub const FILES: usize = 1500;
pub const DEVICE: &str = "pixel-7";
pub const DEVICE_FILES: usize = 200;
pub const OTHER_DEVICE_FILES: usize = 50;
/// Files `REPLACED` are replaced by a new revision, files `DELETED` are tombstoned; none of them has a device.
pub const REPLACED: std::ops::Range<usize> = 1300..1400;
pub const DELETED: std::ops::Range<usize> = 1400..1500;
const BATCH: usize = 500;

/// One published file: its node, the revision published for it and that revision's content hash.
#[derive(Debug, Clone, Copy)]
pub struct File {
    pub node: Uuid,
    pub revision: Uuid,
    pub hash: FileHash,
}

pub struct Dataset {
    pub files: Vec<File>,
    /// The current revisions of the replaced files, by index into `REPLACED`.
    pub replacements: Vec<File>,
}

/// `client_meta` as stored and read back (`jsonb` text).
pub fn device_meta(device: &str) -> String {
    format!("{{\"device\": \"{device}\"}}")
}

/// The client metadata recorded for file `index`.
pub fn meta_of(index: usize) -> Option<String> {
    if index < DEVICE_FILES {
        Some(device_meta(DEVICE))
    } else if index < DEVICE_FILES + OTHER_DEVICE_FILES {
        Some(device_meta("tablet"))
    } else {
        None
    }
}

pub async fn dataset(tree: &Tree) -> Dataset {
    let blobs = tree.blobs(FILES + REPLACED.len()).await;
    let entries = (0..FILES)
        .map(|index| {
            let mut entry = new_file(tree.root, &format!("file {index:04}.bin"), blobs[index]);
            entry.revision.client_meta = meta_of(index);
            entry
        })
        .collect();
    let files = publish_all(tree, entries).await;
    let replaced = REPLACED
        .zip(&blobs[FILES..])
        .map(|(index, blob)| existing(files[index].node, Some(files[index].revision), *blob))
        .collect();
    let replacements = publish_all(tree, replaced).await;
    for file in &files[DELETED] {
        tree.service
            .delete_node(tree.account, file.node, None)
            .await
            .expect("delete a file");
    }
    tree.exec("ANALYZE nodes").await;
    tree.exec("ANALYZE revisions").await;
    Dataset {
        files,
        replacements,
    }
}

/// Publishes `entries` of the tree's account in batches; one file per entry, in entry order.
pub async fn publish_all(tree: &Tree, entries: Vec<PublishEntry>) -> Vec<File> {
    let mut files = Vec::with_capacity(entries.len());
    let mut entries = entries.into_iter().peekable();
    while entries.peek().is_some() {
        let batch: Vec<PublishEntry> = entries.by_ref().take(BATCH).collect();
        let hashes: Vec<FileHash> = batch.iter().map(|e| e.revision.content_hash).collect();
        let result = tree
            .service
            .publish(actor(tree.account), batch, Vec::new(), sc_testdb::START)
            .await
            .expect("publish a batch");
        let PublishResult::Published(results) = result else {
            panic!("unexpected replay");
        };
        for (result, hash) in results.into_iter().zip(hashes) {
            match result {
                EntryResult::Published {
                    node_id,
                    revision_id,
                    ..
                } => files.push(File {
                    node: node_id,
                    revision: revision_id,
                    hash,
                }),
                failed @ EntryResult::Failed(_) => panic!("entry failed: {failed:?}"),
            }
        }
    }
    files
}

/// The plan of `sql` as the provider runs it: the API role, the account scope, row-level security on.
pub async fn plan(tree: &Tree, sql: &str, args: PgArguments) -> String {
    let explain = format!("EXPLAIN (COSTS OFF) {sql}");
    tree.fixture
        .api
        .run(Scope::Account(tree.account), async |tx| {
            let role: String = sqlx::query_scalar("SELECT current_user::text")
                .fetch_one(tx.conn())
                .await?;
            assert_eq!(role, sc_testdb::API);
            let lines: Vec<String> = sqlx::query_scalar_with(AssertSqlSafe(explain), args)
                .fetch_all(tx.conn())
                .await?;
            Ok::<_, sqlx::Error>(lines.join("\n"))
        })
        .await
        .expect("explain")
}
