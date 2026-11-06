//! `HttpSyncClient`, the one production [`SyncApi`], and `connect` (`sync_api.md` construction and negotiation).

use std::sync::Arc;

use bytes::Bytes;
use sc_clock::SystemClock;
use sync_core_proto::{
    FileHash, ListingPage, NodeView, PageToken, RevisionRef, RevisionView, ServerInfo, ServerUrl,
    UrlError,
};
use thiserror::Error;
use uuid::Uuid;

use crate::config::SyncConfig;
use crate::error::{Mapped, SyncError};
use crate::host::CredentialProvider;
use crate::http_transport::{HttpTransport, SERVER_INFO_PATH};
use crate::negotiate::{Limits, identity, negotiate};
use crate::request::{BoxFuture, Method, RequestPath, Transport, WireRequest};
use crate::tree_ops;
use crate::wire_table::map_problem;

/// Why `connect` gave no client. Messages are fixed strings: nothing from the server reaches `Display`.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ConnectError {
    /// `sync.server_url` is `http://` and the build does not have `allow-plain-http`.
    #[error("plain http is not allowed")]
    InsecureScheme,
    /// No usable answer from the server: no connection, a failed TLS 1.3 / `h2` handshake, a timeout, or a refusal to serve.
    #[error("server unreachable")]
    Unreachable,
    /// `message` is meant for the user. `update_url` is `None` when the client decided itself: `server-info` carries none.
    #[error("client upgrade required")]
    UpgradeRequired {
        message: String,
        update_url: Option<String>,
    },
    /// The server's `server-info` is outside what this client accepts; `what` names the field.
    #[error("protocol mismatch ({what})")]
    ProtocolMismatch { what: &'static str },
    #[error("unauthorized")]
    Unauthorized,
    /// A local setting the client cannot use (a malformed server URL, client family or client version).
    #[error("invalid configuration ({field})")]
    InvalidConfig { field: &'static str },
}

/// The typed API hosts code against. Operations are added to this trait by their own tasks (`sync_api.md` § Operations).
pub trait SyncApi: Send + Sync {
    /// Ends every operation in flight with `Cancelled`; nothing is sent afterwards.
    fn cancel(&self);

    /// `GET /api/v1/nodes/root`.
    fn root_node(&self) -> BoxFuture<'_, Result<NodeView, SyncError>>;

    /// One page of a folder, files with their `content_hash`; `after` is the previous page's `next`,
    /// `None` for the first page. The last page has `next = None`. Errors: `NotFound`, `NotAFolder`.
    fn list_children<'a>(
        &'a self,
        node: Uuid,
        after: Option<&'a PageToken>,
    ) -> BoxFuture<'a, Result<ListingPage, SyncError>>;

    /// Error: `NotFound`.
    fn get_revision(&self, revision: Uuid) -> BoxFuture<'_, Result<RevisionView, SyncError>>;

    /// Current revisions of live files with these content hashes, in requests of at most `LOOKUP_CHUNK`;
    /// unknown hashes return nothing. For the reinstall rebuild only, never to skip upload bytes.
    fn lookup_revisions<'a>(
        &'a self,
        hashes: &'a [FileHash],
    ) -> BoxFuture<'a, Result<Vec<RevisionRef>, SyncError>>;

    /// Sent once, never retried. Errors: `NameConflict`, `InvalidName`, `ParentNotFound`, `ParentGone`, `NotAFolder`.
    fn create_folder<'a>(
        &'a self,
        parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>>;

    /// Moves and/or renames; carries no base revision, so it never conflicts with an edit.
    /// Errors: `NameConflict`, `InvalidName`, `Cycle`, `ParentNotFound`, `ParentGone`, `NotAFolder`, `NotFound`.
    fn move_node<'a>(
        &'a self,
        node: Uuid,
        new_parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>>;

    /// `base_revision` is the file's revision the caller last saw; `None` for a folder.
    /// Errors: `EditedSince` (a file changed on the server meanwhile), `NotFound`.
    fn delete_node(
        &self,
        node: Uuid,
        base_revision: Option<Uuid>,
    ) -> BoxFuture<'_, Result<(), SyncError>>;
}

pub struct HttpSyncClient {
    path: Arc<RequestPath>,
    limits: Limits,
}

impl HttpSyncClient {
    /// Checks the server address, reads and range-checks `server-info`, and returns the client with the
    /// answer as sent. The negotiated limits are [`limits`](Self::limits).
    pub async fn connect(
        config: SyncConfig,
        credentials: Arc<dyn CredentialProvider>,
    ) -> Result<(Self, ServerInfo), ConnectError> {
        Self::establish(config, credentials, Vec::new()).await
    }

    /// `connect` that also trusts `roots`: the scripted fake server signs with a CA generated per test run.
    #[cfg(feature = "test-support")]
    pub async fn connect_with_roots(
        config: SyncConfig,
        credentials: Arc<dyn CredentialProvider>,
        roots: Vec<rustls::pki_types::CertificateDer<'static>>,
    ) -> Result<(Self, ServerInfo), ConnectError> {
        Self::establish(config, credentials, roots).await
    }

    /// The request path every operation sends through.
    #[cfg(feature = "test-support")]
    pub fn request_path(&self) -> &RequestPath {
        &self.path
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    async fn establish(
        config: SyncConfig,
        credentials: Arc<dyn CredentialProvider>,
        extra_roots: Vec<rustls::pki_types::CertificateDer<'static>>,
    ) -> Result<(Self, ServerInfo), ConnectError> {
        let url = ServerUrl::parse(&config.server_url, cfg!(feature = "allow-plain-http"))
            .map_err(|error| match error {
                UrlError::InsecureScheme => ConnectError::InsecureScheme,
                UrlError::Invalid => ConnectError::InvalidConfig {
                    field: "server_url",
                },
            })?;
        let (header, own) = identity(&config)?;
        let transport =
            HttpTransport::new(&config, &url, &header, extra_roots).map_err(|error| {
                tracing::error!(%error, "http client setup failed");
                ConnectError::Unreachable
            })?;
        let info = fetch_server_info(&transport).await?;
        let limits = negotiate(&info, &config, own)?;
        let path = RequestPath::new(
            Arc::new(transport),
            credentials,
            Arc::new(SystemClock::new()),
            config.retry_policy(),
        );
        Ok((
            Self {
                path: Arc::new(path),
                limits,
            },
            info,
        ))
    }
}

impl SyncApi for HttpSyncClient {
    fn cancel(&self) {
        self.path.cancel();
    }

    fn root_node(&self) -> BoxFuture<'_, Result<NodeView, SyncError>> {
        Box::pin(tree_ops::root_node(&self.path))
    }

    fn list_children<'a>(
        &'a self,
        node: Uuid,
        after: Option<&'a PageToken>,
    ) -> BoxFuture<'a, Result<ListingPage, SyncError>> {
        Box::pin(tree_ops::list_children(&self.path, node, after))
    }

    fn get_revision(&self, revision: Uuid) -> BoxFuture<'_, Result<RevisionView, SyncError>> {
        Box::pin(tree_ops::get_revision(&self.path, revision))
    }

    fn lookup_revisions<'a>(
        &'a self,
        hashes: &'a [FileHash],
    ) -> BoxFuture<'a, Result<Vec<RevisionRef>, SyncError>> {
        Box::pin(tree_ops::lookup_revisions(&self.path, hashes))
    }

    fn create_folder<'a>(
        &'a self,
        parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>> {
        Box::pin(tree_ops::create_folder(&self.path, parent, name))
    }

    fn move_node<'a>(
        &'a self,
        node: Uuid,
        new_parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>> {
        Box::pin(tree_ops::move_node(&self.path, node, new_parent, name))
    }

    fn delete_node(
        &self,
        node: Uuid,
        base_revision: Option<Uuid>,
    ) -> BoxFuture<'_, Result<(), SyncError>> {
        Box::pin(tree_ops::delete_node(&self.path, node, base_revision))
    }
}

/// `GET /api/v1/server-info`: public, so it carries no credential and no version header.
async fn fetch_server_info(transport: &HttpTransport) -> Result<ServerInfo, ConnectError> {
    let response = transport
        .send(WireRequest {
            method: Method::Get,
            target: SERVER_INFO_PATH.to_owned(),
            headers: Vec::new(),
            bearer: None,
            body: Bytes::new(),
        })
        .await
        .map_err(|kind| {
            tracing::warn!(?kind, "server-info request failed");
            ConnectError::Unreachable
        })?;
    if !(200..300).contains(&response.status) {
        return Err(refused(&response));
    }
    serde_json::from_slice(&response.body).map_err(|_| ConnectError::ProtocolMismatch {
        what: "server_info",
    })
}

fn refused(response: &crate::request::WireResponse) -> ConnectError {
    let headers: Vec<(&str, &str)> = response
        .headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    tracing::warn!(status = response.status, "server-info refused");
    match map_problem(response.status, &headers, &response.body) {
        Mapped::Error(SyncError::Unauthorized) => ConnectError::Unauthorized,
        Mapped::Error(SyncError::UpgradeRequired {
            message,
            update_url,
            ..
        }) => ConnectError::UpgradeRequired {
            message,
            update_url,
        },
        Mapped::Error(SyncError::Transport(_) | SyncError::RateLimited { .. }) => {
            ConnectError::Unreachable
        }
        _ => ConnectError::ProtocolMismatch {
            what: "server_info",
        },
    }
}
