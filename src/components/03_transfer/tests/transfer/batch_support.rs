//! Shared pieces of the `commit_batch` tests: staged files of deterministic content, batch requests built from
//! them, a second account in the same rig, and probes of the receipt, node and quota rows.

use contracts::{
    BatchCommit, BatchEntry, BatchEntryResult, BatchResult, CommitActor, CommitService,
    CommitServiceError, Committed, EntryError, UploadService,
};
use sc_ids::hash_block;
use uuid::Uuid;

use crate::commit_support::DEVICE;
use crate::put_support::{body, content};
use crate::start_support::{Rig, close, node};

/// One staged file: its key, the name it is committed under and its bytes.
#[derive(Debug, Clone)]
pub struct StagedFile {
    pub key: Uuid,
    pub name: String,
    pub data: Vec<u8>,
}

/// A small-file mix like DS-01: every 50th file is empty, the rest 1 byte to about 48 KiB.
pub fn small_file(index: usize) -> Vec<u8> {
    let len = if index.is_multiple_of(50) {
        0
    } else {
        1 + (index * 977) % (48 * 1024)
    };
    content(10_000 + index as u64, len)
}

/// The entry that commits `file` as a new file under `parent`.
pub fn entry(file: &StagedFile, parent: Uuid) -> BatchEntry {
    BatchEntry {
        staged_key: file.key,
        parent_id: parent,
        name: file.name.clone(),
        base_revision: None,
        client_mtime: None,
        client_meta: None,
        import_hint: false,
    }
}

pub fn batch(commit_id: Uuid, entries: Vec<BatchEntry>) -> BatchCommit {
    BatchCommit { commit_id, entries }
}

/// The committed entry, or a panic naming what came instead.
pub fn committed(result: &BatchEntryResult) -> Committed {
    match result {
        BatchEntryResult::Committed(committed) => *committed,
        other => panic!("expected Committed, got {other:?}"),
    }
}

/// The per-entry errors with their positions.
pub fn failures(result: &BatchResult) -> Vec<(usize, EntryError)> {
    result
        .per_entry
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match entry {
            BatchEntryResult::Failed(error) => Some((index, error.clone())),
            BatchEntryResult::Committed(_) => None,
        })
        .collect()
}

impl Rig {
    /// Stages `count` small files for `account` (see [`small_file`]), named `<prefix>-<index>.bin`.
    pub async fn stage_files(&self, account: Uuid, prefix: &str, count: usize) -> Vec<StagedFile> {
        let mut files = Vec::with_capacity(count);
        for index in 0..count {
            let file = StagedFile {
                key: Uuid::new_v4(),
                name: format!("{prefix}-{index}.bin"),
                data: small_file(index),
            };
            self.stage_for(account, &file).await;
            files.push(file);
        }
        files
    }

    pub async fn stage_for(&self, account: Uuid, file: &StagedFile) {
        let hash = (!file.data.is_empty()).then(|| hash_block(&file.data));
        let (stream, _) = body(&file.data, 64 * 1024);
        let service: &dyn UploadService = self.service.as_ref();
        service
            .stage_small(account, file.key, hash, stream, file.data.len() as u64)
            .await
            .expect("staged");
    }

    /// `commit_batch` for the rig's account, through the `CommitService` port.
    pub async fn commit_batch(
        &self,
        request: BatchCommit,
    ) -> Result<BatchResult, CommitServiceError> {
        self.commit_batch_as(self.account, request).await
    }

    pub async fn commit_batch_as(
        &self,
        account: Uuid,
        request: BatchCommit,
    ) -> Result<BatchResult, CommitServiceError> {
        let service: &dyn CommitService = self.service.as_ref();
        let actor = CommitActor {
            account_id: account,
            device_label: DEVICE.to_owned(),
        };
        service.commit_batch(actor, request).await
    }

    /// A second active account with its root, feed row and quota row; returns the account and its root.
    pub async fn other_account(&self) -> (Uuid, Uuid) {
        let account = Uuid::new_v4();
        let mut conn = self.owner().await;
        crate::support::account(&mut conn, account).await;
        let root = node(&mut conn, account, None, "folder", "").await;
        sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, 1)")
            .bind(account)
            .execute(&mut conn)
            .await
            .expect("insert feed row");
        sqlx::query("INSERT INTO quota (account_id) VALUES ($1)")
            .bind(account)
            .execute(&mut conn)
            .await
            .expect("insert quota row");
        close(conn).await;
        (account, root)
    }

    /// A folder under the root, tombstoned when `deleted`.
    pub async fn folder(&self, name: &str, deleted: bool) -> Uuid {
        let mut conn = self.owner().await;
        let folder = node(&mut conn, self.account, Some(self.root), "folder", name).await;
        close(conn).await;
        if deleted {
            self.exec(&format!(
                "UPDATE nodes SET deleted_at = 'epoch'::timestamptz WHERE id = '{folder}'"
            ))
            .await;
        }
        folder
    }

    /// The outcomes stored under `commit_id` for `account`; `None` without a receipt.
    pub async fn receipt_len(&self, account: Uuid, commit_id: Uuid) -> Option<i64> {
        let rows = self
            .count(&format!(
                "SELECT count(*) FROM commit_receipts WHERE account_id = '{account}' \
                 AND commit_id = '{commit_id}'"
            ))
            .await;
        if rows == 0 {
            return None;
        }
        Some(
            self.count(&format!(
                "SELECT jsonb_array_length(response)::bigint FROM commit_receipts \
                 WHERE account_id = '{account}' AND commit_id = '{commit_id}'"
            ))
            .await,
        )
    }

    pub async fn receipts(&self) -> i64 {
        self.count("SELECT count(*) FROM commit_receipts").await
    }

    pub async fn nodes_of(&self, account: Uuid) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM nodes WHERE account_id = '{account}'"
        ))
        .await
    }

    pub async fn revisions_of(&self, account: Uuid) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM revisions WHERE account_id = '{account}'"
        ))
        .await
    }

    pub async fn staged_rows(&self, account: Uuid) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM upload_sessions WHERE account_id = '{account}'"
        ))
        .await
    }

    /// The distinct feed sequence numbers of `nodes`.
    pub async fn distinct_seqs(&self, nodes: &[Uuid]) -> i64 {
        let list = nodes
            .iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(",");
        self.count(&format!(
            "SELECT count(DISTINCT seq) FROM nodes WHERE id IN ({list})"
        ))
        .await
    }

    /// `(used, reserved)` of the rig's account.
    pub async fn usage(&self) -> (i64, i64) {
        (self.used().await, self.reserved().await)
    }
}
