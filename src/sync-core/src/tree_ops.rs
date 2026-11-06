//! The tree calls of `SyncApi` (`sync_api.md` § Operations): one function per operation over the request path.
//! Names and page tokens from the server are data: they are decoded from JSON and handed back, never used as paths.

use bytes::Bytes;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sync_core_proto::{
    FileHash, LOOKUP_CHUNK, ListingPage, NodeView, PageToken, RevisionRef, RevisionView,
};
use uuid::Uuid;

use crate::error::{Mapped, SyncError};
use crate::request::{Method, Request, RequestPath, WireResponse};

const NODES: &str = "/api/v1/nodes";
const HEX: &[u8; 16] = b"0123456789ABCDEF";

pub(crate) async fn root_node(path: &RequestPath) -> Result<NodeView, SyncError> {
    let request = bodyless(path, Method::Get, format!("{NODES}/root"));
    decode("root_node", send(path, "root_node", request).await?)
}

pub(crate) async fn list_children(
    path: &RequestPath,
    node: Uuid,
    after: Option<&PageToken>,
) -> Result<ListingPage, SyncError> {
    let mut target = format!("{NODES}/{node}/children");
    if let Some(PageToken(token)) = after {
        target.push_str("?after=");
        percent_encode(token, &mut target);
    }
    let request = bodyless(path, Method::Get, target);
    decode("list_children", send(path, "list_children", request).await?)
}

pub(crate) async fn get_revision(
    path: &RequestPath,
    revision: Uuid,
) -> Result<RevisionView, SyncError> {
    let request = bodyless(path, Method::Get, format!("/api/v1/revisions/{revision}"));
    decode("get_revision", send(path, "get_revision", request).await?)
}

/// Sends the hashes in requests of at most `LOOKUP_CHUNK`, one after the other on the shared connection, and
/// joins the answers in request order. A hash the server does not know adds nothing to the result.
pub(crate) async fn lookup_revisions(
    path: &RequestPath,
    hashes: &[FileHash],
) -> Result<Vec<RevisionRef>, SyncError> {
    let mut found = Vec::new();
    for chunk in hashes.chunks(LOOKUP_CHUNK) {
        let listed: Vec<String> = chunk.iter().map(ToString::to_string).collect();
        let request = with_json(
            Method::Post,
            "/api/v1/revisions:lookup".to_owned(),
            &json!({ "content_hashes": listed }),
            path.default_attempts(),
        );
        let answer = send(path, "lookup_revisions", request).await?;
        found.extend(decode::<Vec<RevisionRef>>("lookup_revisions", answer)?);
    }
    Ok(found)
}

/// Sent once: a lost answer to a create that did happen would make a second send fail with `NameConflict`,
/// so the caller decides what to do after a transport failure.
pub(crate) async fn create_folder(
    path: &RequestPath,
    parent: Uuid,
    name: &str,
) -> Result<NodeView, SyncError> {
    let request = with_json(
        Method::Post,
        NODES.to_owned(),
        &json!({ "parent_id": parent, "name": name }),
        1,
    );
    decode("create_folder", send(path, "create_folder", request).await?)
}

/// Never carries a base revision: a move does not conflict with an edit (AC-70).
pub(crate) async fn move_node(
    path: &RequestPath,
    node: Uuid,
    new_parent: Uuid,
    name: &str,
) -> Result<NodeView, SyncError> {
    let request = with_json(
        Method::Patch,
        format!("{NODES}/{node}"),
        &json!({ "parent_id": new_parent, "name": name }),
        path.default_attempts(),
    );
    decode("move_node", send(path, "move_node", request).await?)
}

/// `base_revision` is the revision the caller last saw of a file; a folder has none.
pub(crate) async fn delete_node(
    path: &RequestPath,
    node: Uuid,
    base_revision: Option<Uuid>,
) -> Result<(), SyncError> {
    let mut target = format!("{NODES}/{node}");
    if let Some(revision) = base_revision {
        target.push_str("?base_revision=");
        target.push_str(&revision.to_string());
    }
    let request = bodyless(path, Method::Delete, target);
    send(path, "delete_node", request).await.map(|_| ())
}

fn bodyless(path: &RequestPath, method: Method, target: String) -> Request {
    Request {
        method,
        target,
        headers: Vec::new(),
        body: Bytes::new(),
        max_attempts: path.default_attempts(),
    }
}

fn with_json(method: Method, target: String, body: &Value, max_attempts: u32) -> Request {
    Request {
        method,
        target,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: Bytes::from(body.to_string()),
        max_attempts,
    }
}

pub(crate) async fn send(
    path: &RequestPath,
    op: &'static str,
    request: Request,
) -> Result<WireResponse, SyncError> {
    path.send(op, request).await.map_err(into_error)
}

/// A planner input outside the upload routes is a server bug, not an operation error.
pub(crate) fn into_error(mapped: Mapped) -> SyncError {
    match mapped {
        Mapped::Error(error) => error,
        Mapped::Planner(_) => SyncError::ProtocolViolation {
            detail: "upload answer on a tree route",
        },
    }
}

pub(crate) fn decode<T: DeserializeOwned>(
    op: &'static str,
    response: WireResponse,
) -> Result<T, SyncError> {
    serde_json::from_slice(&response.body).map_err(|error| {
        tracing::warn!(op, %error, "response body does not match its DTO");
        SyncError::ProtocolViolation {
            detail: "malformed response body",
        }
    })
}

/// Escapes everything outside the RFC 3986 unreserved set, so an opaque token cannot add a parameter.
fn percent_encode(text: &str, out: &mut String) {
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }
}
