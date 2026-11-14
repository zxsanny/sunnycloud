//! Shared pieces of the `commit_upload` tests: uploads of chosen bytes sent in full, commit requests built from
//! the client's blocklist, and probes of the revision, node, session and quota rows.

use contracts::{CommitActor, CommitServiceError, CommitUpload, Committed};
use sc_clock::Timestamp;
use sc_ids::{FileHash, FileHasher, Hash32};
use uuid::Uuid;

use crate::put_support::Upload;
use crate::start_support::{Rig, close, request, started};

/// The device that names conflict copies in these tests.
pub const DEVICE: &str = "phone-2";

/// The revision columns a commit writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionRow {
    pub node_id: Uuid,
    pub blob_id: Uuid,
    pub size: i64,
    pub content_hash: Vec<u8>,
    pub media_type: Option<String>,
}

/// The committed-key columns of a session row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub committed_revision_id: Option<Uuid>,
    pub reserved_bytes: i64,
    pub expires_at: Timestamp,
}

/// The whole-file hash computed from the bytes, independently of the block hashes the service holds.
pub fn file_hash(data: &[u8]) -> FileHash {
    let mut hasher = FileHasher::new();
    for block in data.chunks(sc_ids::BLOCK_SIZE) {
        hasher.update(block).expect("one block");
        hasher.finish_block();
    }
    hasher.finish().1
}

/// The client's blocklist of `upload`.
pub fn blocklist(upload: &Upload) -> Vec<Hash32> {
    (0..upload.block_count())
        .map(|index| upload.hash(index))
        .collect()
}

/// A commit of `upload` as `name` under `parent`, on `base` when given.
pub fn commit_request(
    upload: &Upload,
    parent: Uuid,
    name: &str,
    base: Option<Uuid>,
) -> CommitUpload {
    CommitUpload {
        upload_id: upload.id,
        parent_id: parent,
        name: name.to_owned(),
        base_revision: base,
        blocklist: blocklist(upload),
        client_mtime: None,
        client_meta: None,
        import_hint: false,
    }
}

impl Rig {
    /// Starts a session for `data` named `name` under the root with the key `key`.
    pub async fn upload_bytes(&self, name: &str, key: &str, data: Vec<u8>) -> Upload {
        let declared = i64::try_from(data.len()).expect("size");
        let id = started(self.start(request(self.root, name, declared, key)).await).upload_id;
        let blob = self.blob_of(id).await;
        Upload { id, blob, data }
    }

    /// Sends every block of `upload`, one after the other.
    pub async fn send_all(&self, upload: &Upload) {
        for index in 0..upload.block_count() {
            self.put(upload, index).await.expect("block acknowledged");
        }
    }

    /// Starts, sends in full and commits `data` as a new file `name` under the root, on `base` when given.
    pub async fn commit_bytes(
        &self,
        name: &str,
        key: &str,
        data: Vec<u8>,
        base: Option<Uuid>,
    ) -> Result<Committed, CommitServiceError> {
        let upload = self.upload_bytes(name, key, data).await;
        self.send_all(&upload).await;
        self.commit(commit_request(&upload, self.root, name, base))
            .await
    }

    pub async fn commit(&self, request: CommitUpload) -> Result<Committed, CommitServiceError> {
        self.service.commit_upload(self.actor(), request).await
    }

    pub fn actor(&self) -> CommitActor {
        CommitActor {
            account_id: self.account,
            device_label: DEVICE.to_owned(),
        }
    }

    pub async fn revision_row(&self, revision: Uuid) -> RevisionRow {
        let mut conn = self.owner().await;
        let (node_id, blob_id, size, content_hash, media_type) = sqlx::query_as(
            "SELECT node_id, blob_id, size, content_hash, media_type FROM revisions \
             WHERE id = $1 AND account_id = $2",
        )
        .bind(revision)
        .bind(self.account)
        .fetch_one(&mut conn)
        .await
        .expect("revision row");
        close(conn).await;
        RevisionRow {
            node_id,
            blob_id,
            size,
            content_hash,
            media_type,
        }
    }

    /// The name and current revision of a node.
    pub async fn node_row(&self, node: Uuid) -> (String, Option<Uuid>) {
        let mut conn = self.owner().await;
        let row = sqlx::query_as(
            "SELECT name, current_revision_id FROM nodes WHERE id = $1 AND account_id = $2",
        )
        .bind(node)
        .bind(self.account)
        .fetch_one(&mut conn)
        .await
        .expect("node row");
        close(conn).await;
        row
    }

    pub async fn session_record(&self, upload_id: Uuid) -> Option<SessionRecord> {
        let mut conn = self.owner().await;
        let row: Option<(Option<Uuid>, i64, i64)> = sqlx::query_as(
            "SELECT committed_revision_id, reserved_bytes, \
             (extract(epoch FROM expires_at) * 1000000)::bigint \
             FROM upload_sessions WHERE id = $1 AND account_id = $2",
        )
        .bind(upload_id)
        .bind(self.account)
        .fetch_optional(&mut conn)
        .await
        .expect("session row");
        close(conn).await;
        row.map(
            |(committed_revision_id, reserved_bytes, expires)| SessionRecord {
                committed_revision_id,
                reserved_bytes,
                expires_at: Timestamp::from_unix_micros(expires),
            },
        )
    }

    pub async fn used(&self) -> i64 {
        self.count(&format!(
            "SELECT used_bytes FROM quota WHERE account_id = '{}'",
            self.account
        ))
        .await
    }

    pub async fn revisions(&self) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM revisions WHERE account_id = '{}'",
            self.account
        ))
        .await
    }

    pub async fn blob_state(&self, blob: Uuid) -> Option<String> {
        let mut conn = self.owner().await;
        let state = sqlx::query_scalar("SELECT state FROM blobs WHERE id = $1 AND account_id = $2")
            .bind(blob)
            .bind(self.account)
            .fetch_optional(&mut conn)
            .await
            .expect("blob state");
        close(conn).await;
        state
    }
}
