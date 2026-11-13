use std::net::SocketAddr;
use std::sync::Arc;

use contracts::{NodeKind, NodeState, NodeView, RevisionMatch, RevisionView, ThumbState};
use sc_clock::Timestamp;
use sc_ids::{FileHash, Hash32};
use sunnycloud_http_api::{ApiServices, HttpServer};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::auth::support::{DEVICE_TOKEN, MEMBER_TOKEN, ScriptedAuthenticator, ScriptedWorkerAuth};
use crate::harness::recording::{
    RecordingCommits, RecordingDownloads, RecordingTree, RecordingTreeReader,
};
use crate::harness::recording_stream::{RecordingFeed, RecordingUploads};
use crate::harness::{EVENT_BOUND, Script, default_config, new_metrics, stub_services};
use crate::worker::support::{Reply, send};

pub const FOLDER: Uuid = Uuid::from_u128(0xF0);
pub const NODE: Uuid = Uuid::from_u128(0xF1);
pub const REVISION: Uuid = Uuid::from_u128(0xF2);
pub const BASE: Uuid = Uuid::from_u128(0xF3);
pub const UPLOAD: Uuid = Uuid::from_u128(0xF4);
pub const STAGED: Uuid = Uuid::from_u128(0xF5);
pub const COMMIT_ID: Uuid = Uuid::from_u128(0xF6);
pub const COPY: Uuid = Uuid::from_u128(0xF7);

pub const CREATED: Timestamp = Timestamp::from_unix_micros(1_793_956_272_123_456);
pub const CREATED_TEXT: &str = "2026-11-06T09:11:12.123456Z";

/// The 64 hex digits of a block or file hash made of `byte`.
pub fn hex(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

pub fn hash32(byte: u8) -> Hash32 {
    Hash32::from_bytes([byte; 32])
}

pub fn file_hash(byte: u8) -> FileHash {
    FileHash::from_bytes([byte; 32])
}

pub fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

pub fn file_node(id: Uuid) -> NodeView {
    NodeView {
        id,
        parent_id: Some(FOLDER),
        kind: NodeKind::File,
        name: "report.pdf".to_owned(),
        state: NodeState::Live,
        current_revision_id: Some(REVISION),
        size: Some(2048),
        content_hash: Some(file_hash(0xAB)),
        media_type: Some("application/pdf".to_owned()),
        client_mtime: Some(CREATED),
        created_at: CREATED,
        updated_at: CREATED,
        seq: 9,
        thumb_state: ThumbState::None,
    }
}

pub fn folder_node(id: Uuid, parent: Option<Uuid>) -> NodeView {
    NodeView {
        id,
        parent_id: parent,
        kind: NodeKind::Folder,
        name: "Documents".to_owned(),
        state: NodeState::Live,
        current_revision_id: None,
        size: None,
        content_hash: None,
        media_type: None,
        client_mtime: None,
        created_at: CREATED,
        updated_at: CREATED,
        seq: 3,
        thumb_state: ThumbState::None,
    }
}

pub fn revision_view(meta: Option<&str>) -> RevisionView {
    RevisionView {
        id: REVISION,
        node_id: NODE,
        size: 4_194_304,
        content_hash: file_hash(0xAB),
        blocklist: Some(vec![hash32(1), hash32(2)]),
        media_type: None,
        client_mtime: None,
        client_meta: meta.map(str::to_owned),
        created_at: CREATED,
    }
}

pub fn revision_match(meta: Option<&str>) -> RevisionMatch {
    RevisionMatch {
        hash: file_hash(0xAB),
        node_id: NODE,
        revision_id: REVISION,
        client_meta: meta.map(str::to_owned),
    }
}

/// The whole server over the recording stubs; the authenticator knows the member, the admin and the device.
pub struct Stack {
    pub public: SocketAddr,
    pub script: Script,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Stack {
    pub async fn start() -> Stack {
        Stack::start_with(|_, services| services).await
    }

    /// As [`Stack::start`], with `replace` free to swap any port of the recording set for another double.
    pub async fn start_with(replace: impl FnOnce(&Script, ApiServices) -> ApiServices) -> Stack {
        let script = Script::default();
        let metrics = new_metrics();
        let services = ApiServices {
            authenticator: Arc::new(ScriptedAuthenticator::default()),
            worker_auth: Arc::new(ScriptedWorkerAuth::default()),
            tree: RecordingTree::new(&script),
            tree_reader: RecordingTreeReader::new(&script),
            uploads: RecordingUploads::new(&script),
            commits: RecordingCommits::new(&script),
            downloads: RecordingDownloads::new(&script),
            change_feed: RecordingFeed::new(&script),
            ..stub_services(&script, metrics.clone())
        };
        let services = replace(&script, services);
        let bound = HttpServer::new(services, default_config())
            .expect("server builds")
            .bind()
            .await
            .expect("listeners bind");
        let public = bound.public_addr();
        let (stop, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            bound
                .serve(async move {
                    signal.await.ok();
                })
                .await
                .expect("serve returns");
        });
        Stack {
            public,
            script,
            stop,
            task,
        }
    }

    pub async fn stop(self) {
        self.stop.send(()).expect("server is running");
        tokio::time::timeout(EVENT_BOUND, self.task)
            .await
            .expect("serve returns after the signal")
            .expect("serve does not panic");
    }

    pub async fn call(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Reply {
        send(self.public, method, path, headers, body).await
    }

    /// A call by the member's session as a native client sends it.
    pub async fn as_member(&self, method: &str, path: &str, body: &str) -> Reply {
        self.with_token(MEMBER_TOKEN, method, path, &[], body.as_bytes())
            .await
    }

    /// A call by the desktop device token, whose label is `laptop`.
    pub async fn as_device(&self, method: &str, path: &str, body: &str) -> Reply {
        self.with_token(DEVICE_TOKEN, method, path, &[], body.as_bytes())
            .await
    }

    pub async fn with_token(
        &self,
        token: &str,
        method: &str,
        path: &str,
        extra: &[(&str, &str)],
        body: &[u8],
    ) -> Reply {
        let auth = bearer(token);
        let mut headers = vec![("authorization", auth.as_str())];
        headers.extend_from_slice(extra);
        self.call(method, path, &headers, body).await
    }

    /// The log lines of the calls whose port method starts with `key`, in order.
    pub fn log(&self, key: &str) -> Vec<String> {
        self.script
            .log()
            .into_iter()
            .filter(|line| line.starts_with(key))
            .collect()
    }
}
