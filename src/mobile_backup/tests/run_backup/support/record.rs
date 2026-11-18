//! What the fake sync API saw, for the assertions of the tests.

use serde_json::Value;
use sync_core::UploadCheckpoint;
use tokio::sync::Notify;
use uuid::Uuid;

/// A call that waits here until the test lets it go.
#[derive(Default)]
pub struct Gate {
    pub entered: Notify,
    pub release: Notify,
}

#[derive(Clone, Debug)]
pub struct LargeCall {
    pub local_key: String,
    pub key: Uuid,
    pub name: String,
    pub parent: Uuid,
    pub meta: Option<Value>,
    pub session: Option<UploadCheckpoint>,
    pub bytes_sent: u64,
}

#[derive(Clone, Debug)]
pub struct CommitSeen {
    pub commit_id: Uuid,
    pub entries: Vec<(Uuid, Uuid, Option<Value>)>,
    pub stored_before_staging: bool,
}

#[derive(Default)]
pub struct Record {
    pub calls: usize,
    /// `local_key` of every `hash_file` call.
    pub hashed: Vec<String>,
    /// Requests that carried the signed-in credential.
    pub bearer_uses: usize,
    /// The most rows holding a stored hash and not yet uploaded, counted when an upload starts.
    pub most_unfinished: usize,
    pub acked: usize,
    pub large: Vec<LargeCall>,
    /// Entries per `upload_small` call.
    pub groups: Vec<usize>,
    pub commits: Vec<CommitSeen>,
    pub resumed: Vec<Uuid>,
    pub creates: usize,
    pub lists: usize,
    /// Keys whose row had no stored hash when their upload started.
    pub unstored: Vec<Uuid>,
}
