use std::net::SocketAddr;
use std::sync::Arc;

use contracts::{
    AccessGrant, BlobRef, ByteRange, DerivedBlob, Disposition, Download, DownloadError,
    DownloadStatus, LiveFile, NodeKind, PublicShareError, ShareContent, ShareDerived,
};
use sc_clock::{Clock, SystemClock};
use sc_observability::Metrics;
use sunnycloud_http_api::{ApiServices, HttpServer};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::auth::support::{DEVICE_TOKEN, MEMBER_TOKEN, ScriptedAuthenticator, ScriptedWorkerAuth};
use crate::content::support::chunked;
use crate::harness::recording::{
    RecordingDerived, RecordingDownloads, RecordingPublicShares, RecordingShares,
    RecordingThumbnailStates,
};
use crate::harness::{
    EVENT_BOUND, SESSION_COOKIE, Script, default_config, new_metrics, stub_services,
};
use crate::worker::support::{Reply, send};

pub const TOKEN: &str = "sc_shr_Abc-123_xyz";
pub const NODE: Uuid = Uuid::from_u128(0xD1);
pub const REVISION: Uuid = Uuid::from_u128(0xD2);
pub const SHARE_ACCOUNT: Uuid = Uuid::from_u128(0xE1);

pub const CREATE: &str = "ShareService::create";
pub const LIST: &str = "ShareService::list";
pub const REVOKE: &str = "ShareService::revoke";
pub const OPEN: &str = "PublicShareService::open";
pub const VERIFY: &str = "PublicShareService::verify_password";
pub const AUTHORIZE_CONTENT: &str = "PublicShareService::authorize_content";
pub const AUTHORIZE_DERIVED: &str = "PublicShareService::authorize_derived";
pub const OPEN_DERIVED_FOR: &str = "DerivedOpener::open_derived_for";
pub const OPEN_REVISION: &str = "DownloadService::open_revision_content";
pub const OPEN_BLOB: &str = "DownloadService::open_blob_content";
pub const OPEN_DERIVED: &str = "DerivedOpener::open_derived";
pub const STATES: &str = "ThumbnailStateSource::states";

pub fn link(tail: &str) -> String {
    format!("/api/v1/public-shares/{TOKEN}{tail}")
}

pub fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// The whole server over the recording stubs; the authenticator knows the member, the admin and the device.
pub struct Stack {
    pub public: SocketAddr,
    pub script: Script,
    pub metrics: Arc<Metrics>,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Stack {
    pub async fn start() -> Stack {
        let script = Script::default();
        let metrics = new_metrics();
        let services = ApiServices {
            authenticator: Arc::new(ScriptedAuthenticator::default()),
            worker_auth: Arc::new(ScriptedWorkerAuth::default()),
            shares: RecordingShares::new(&script),
            public_shares: RecordingPublicShares::new(&script),
            downloads: RecordingDownloads::new(&script),
            derived: RecordingDerived::new(&script),
            thumbnail_states: RecordingThumbnailStates::new(&script),
            ..stub_services(&script, metrics.clone())
        };
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
            metrics,
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
        body: &str,
    ) -> Reply {
        send(self.public, method, path, headers, body.as_bytes()).await
    }

    /// A call by the member's session as a native client sends it.
    pub async fn as_member(&self, method: &str, path: &str, body: &str) -> Reply {
        let auth = bearer(MEMBER_TOKEN);
        self.call(method, path, &[("authorization", auth.as_str())], body)
            .await
    }

    /// A call by the desktop device token.
    pub async fn as_device(&self, method: &str, path: &str, body: &str) -> Reply {
        let auth = bearer(DEVICE_TOKEN);
        self.call(method, path, &[("authorization", auth.as_str())], body)
            .await
    }

    /// A call by a visitor (no credential), with `extra` headers.
    pub async fn as_visitor(&self, method: &str, path: &str, extra: &[(&str, &str)]) -> Reply {
        self.call(method, path, extra, "").await
    }

    /// The log lines of the calls whose port method starts with `key`, in order.
    pub fn log(&self, key: &str) -> Vec<String> {
        self.script
            .log()
            .into_iter()
            .filter(|line| line.starts_with(key))
            .collect()
    }

    /// Every port method called, in order, by name only.
    pub fn order(&self) -> Vec<String> {
        self.script
            .log()
            .iter()
            .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
            .collect()
    }
}

pub fn cookie_header(token: &str) -> String {
    format!("{SESSION_COOKIE}={token}")
}

/// `n` bytes served as a whole, inline image.
pub fn full(media_type: &str, n: usize) -> Download {
    Download {
        status: DownloadStatus::Full,
        total_size: i64::try_from(n).expect("size fits"),
        range: None,
        etag: "blob-etag".to_owned(),
        media_type: Some(media_type.to_owned()),
        disposition: Disposition::Inline,
        suggested_name: "file".to_owned(),
        body: chunked(n, None),
    }
}

pub fn partial(start: u64, end: u64, total: usize) -> Download {
    let length = usize::try_from(end - start + 1).expect("length fits");
    Download {
        status: DownloadStatus::Partial,
        total_size: i64::try_from(total).expect("size fits"),
        range: Some(ByteRange { start, end }),
        body: chunked(length, None),
        ..full("image/png", 0)
    }
}

pub fn served(download: Download) -> Result<Download, DownloadError> {
    Ok(download)
}

pub fn allowed() -> Result<ShareContent, PublicShareError> {
    Ok(ShareContent {
        account_id: SHARE_ACCOUNT,
        node_id: NODE,
        revision_id: REVISION,
    })
}

/// What 06 returns for a share thumbnail: the file of the link with the account that owns it.
pub fn shared_file() -> Result<ShareDerived, PublicShareError> {
    Ok(ShareDerived {
        account_id: SHARE_ACCOUNT,
        file: LiveFile {
            kind: NodeKind::File,
            revision: None,
            blob_ref: Some(BLOB_ID),
        },
    })
}

pub const BLOB_ID: Uuid = Uuid::from_u128(0xB10B);

pub fn blob() -> BlobRef {
    BlobRef {
        blob_id: BLOB_ID,
        size: 2048,
        wrapped_key: vec![1, 2, 3],
        master_key_id: 1,
    }
}

/// The derived blob 08 answers, with the media type stored with it.
pub fn derived(media_type: &str) -> DerivedBlob {
    DerivedBlob {
        blob: blob(),
        media_type: media_type.to_owned(),
    }
}

/// A grant that ends `seconds` after the server's clock.
pub fn grant(seconds: u64) -> AccessGrant {
    let clock = SystemClock::new();
    AccessGrant {
        value: "g-value_9".to_owned(),
        expires_at: clock
            .now_utc()
            .checked_add(std::time::Duration::from_secs(seconds))
            .expect("a grant a few minutes ahead does not overflow"),
    }
}

/// The problem body without its per-request id, and the headers without the request id, to compare answers.
pub fn shape(reply: &Reply) -> (serde_json::Value, Vec<(String, String)>) {
    let mut headers: Vec<(String, String)> = reply
        .headers
        .iter()
        .filter(|(name, _)| name.as_str() != "x-request-id")
        .map(|(name, value)| {
            (
                name.to_string(),
                value.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    headers.sort();
    (reply.problem(), headers)
}
