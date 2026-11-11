//! Stubs of the ports the share and media routes call that also write each call, with its arguments, to the
//! ordered log of the [`Script`]. A port method returns the result scripted for `"Port::method"`, or fails as
//! `Unexpected` when none is queued.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use contracts::*;
use sc_ids::{FileHash, RawToken};
use uuid::Uuid;

use super::script::{Script, unscripted};

/// As `stub_port!`, plus a log line `Port::method (arguments as Debug)`.
macro_rules! recording_port {
    ($stub:ident: $trait:ident, $error:ident, {
        $( async fn $method:ident(&self $(, $arg:ident: $ty:ty)* $(,)?) -> $ret:ty; )*
    }) => {
        pub struct $stub(Script);

        impl $stub {
            pub fn new(script: &Script) -> Arc<Self> {
                Arc::new(Self(script.clone()))
            }
        }

        #[contracts::async_trait]
        impl $trait for $stub {
            $(
                async fn $method(&self $(, $arg: $ty)*) -> $ret {
                    let key = concat!(stringify!($trait), "::", stringify!($method));
                    self.0.record(format!("{key} {:?}", ($(&$arg,)*)));
                    self.0
                        .take::<$ret>(key)
                        .unwrap_or_else(|| Err($error::Unexpected(unscripted(key))))
                }
            )*
        }
    };
}

recording_port!(RecordingTree: TreeService, TreeServiceError, {
    async fn create_folder(&self, account: Uuid, parent_id: Uuid, name: &str)
        -> Result<NodeView, TreeServiceError>;
    async fn update_node(&self, account: Uuid, node_id: Uuid, new_name: Option<&str>,
        new_parent: Option<Uuid>) -> Result<NodeView, TreeServiceError>;
    async fn delete_node(&self, account: Uuid, node_id: Uuid, base_revision: Option<Uuid>)
        -> Result<(), TreeServiceError>;
    async fn resolve_path(&self, account: Uuid, path: &str) -> Result<Uuid, TreeServiceError>;
    async fn find_revisions(&self, account: Uuid, client_device: Option<&str>, page: Page)
        -> Result<Vec<RevisionRef>, TreeServiceError>;
});

recording_port!(RecordingTreeReader: TreeReader, TreeReaderError, {
    async fn get_node(&self, account: Uuid, selector: NodeSelector)
        -> Result<NodeView, TreeReaderError>;
    async fn list_children(&self, account: Uuid, folder_id: Uuid, after_name_key: Option<&str>,
        limit: u32) -> Result<ListingPage, TreeReaderError>;
    async fn find_live_node(&self, account: Uuid, parent_id: Uuid, name: &str)
        -> Result<Option<NodeRef>, TreeReaderError>;
    async fn resolve_live_file(&self, account: Uuid, node_id: Uuid)
        -> Result<LiveFileLookup, TreeReaderError>;
    async fn root_node(&self, account: Uuid) -> Result<NodeRef, TreeReaderError>;
    async fn get_revision(&self, account: Uuid, revision_id: Uuid)
        -> Result<RevisionView, TreeReaderError>;
    async fn is_current_or_retained(&self, account: Uuid, revision_id: Uuid)
        -> Result<bool, TreeReaderError>;
    async fn describe_blobs(&self, blob_ids: &[Uuid])
        -> Result<HashMap<Uuid, Vec<String>>, TreeReaderError>;
    async fn lookup_revisions(&self, account: Uuid, hashes: &[FileHash])
        -> Result<Vec<RevisionMatch>, TreeReaderError>;
});

recording_port!(RecordingCommits: CommitService, CommitServiceError, {
    async fn commit_upload(&self, actor: CommitActor, request: CommitUpload)
        -> Result<Committed, CommitServiceError>;
    async fn commit_batch(&self, actor: CommitActor, request: BatchCommit)
        -> Result<BatchResult, CommitServiceError>;
});

recording_port!(RecordingShares: ShareService, ShareError, {
    async fn create(&self, account: Uuid, node_id: Uuid, expires_in: Option<Duration>,
        password: Option<&str>) -> Result<CreatedShare, ShareError>;
    async fn list(&self, account: Uuid, node_id: Option<Uuid>)
        -> Result<Vec<ShareInfo>, ShareError>;
    async fn revoke(&self, account: Uuid, share_id: Uuid) -> Result<(), ShareError>;
});

recording_port!(RecordingDownloads: DownloadService, DownloadError, {
    async fn open_revision_content(&self, account: Uuid, target: RevisionOrCurrent,
        options: DownloadOptions) -> Result<Download, DownloadError>;
    async fn open_blob_content(&self, account: Uuid, blob: BlobRef, media_type: Option<String>,
        options: DownloadOptions) -> Result<Download, DownloadError>;
});

recording_port!(RecordingDerived: DerivedOpener, DerivedError, {
    async fn open_derived(&self, account: Uuid, node_id: Uuid, kind: DerivedKind)
        -> Result<BlobRef, DerivedError>;
    async fn open_derived_for(&self, file: &LiveFile, kind: DerivedKind)
        -> Result<BlobRef, DerivedError>;
});

recording_port!(RecordingThumbnailStates: ThumbnailStateSource, ThumbnailStateError, {
    async fn states(&self, account: Uuid, hashes: &[FileHash])
        -> Result<HashMap<FileHash, ThumbState>, ThumbnailStateError>;
});

/// 06's visitor port. A grant's value is redacted by its `Debug`, so the log writes it out.
pub struct RecordingPublicShares(Script);

impl RecordingPublicShares {
    pub fn new(script: &Script) -> Arc<Self> {
        Arc::new(Self(script.clone()))
    }

    fn next<T: 'static>(&self, key: &'static str) -> Result<T, PublicShareError> {
        self.0
            .take::<Result<T, PublicShareError>>(key)
            .unwrap_or_else(|| Err(PublicShareError::Unexpected(unscripted(key))))
    }
}

fn grant_line(grant: Option<&AccessGrant>) -> String {
    match grant {
        Some(grant) => format!(
            "grant={} expires_at={}",
            grant.value,
            grant.expires_at.as_unix_micros()
        ),
        None => "no grant".to_owned(),
    }
}

#[contracts::async_trait]
impl PublicShareService for RecordingPublicShares {
    async fn open(
        &self,
        token: &RawToken,
        client_ip: IpAddr,
    ) -> Result<ShareOpen, PublicShareError> {
        self.0.record(format!(
            "PublicShareService::open {} {client_ip}",
            token.expose()
        ));
        self.next("PublicShareService::open")
    }

    async fn verify_password(
        &self,
        token: &RawToken,
        password: &str,
        client_ip: IpAddr,
    ) -> Result<AccessGrant, PublicShareError> {
        self.0.record(format!(
            "PublicShareService::verify_password {} {password} {client_ip}",
            token.expose()
        ));
        self.next("PublicShareService::verify_password")
    }

    async fn authorize_content(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
        starts_download: bool,
    ) -> Result<ShareContent, PublicShareError> {
        self.0.record(format!(
            "PublicShareService::authorize_content {} {} {client_ip} starts_download={starts_download}",
            token.expose(),
            grant_line(grant)
        ));
        self.next("PublicShareService::authorize_content")
    }

    async fn authorize_derived(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
    ) -> Result<ShareDerived, PublicShareError> {
        self.0.record(format!(
            "PublicShareService::authorize_derived {} {} {client_ip}",
            token.expose(),
            grant_line(grant)
        ));
        self.next("PublicShareService::authorize_derived")
    }
}
