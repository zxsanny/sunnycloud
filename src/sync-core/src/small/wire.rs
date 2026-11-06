//! The two routes of the small-file path. Both are idempotent (by staged key and by `commit_id`), so both use the
//! default number of sends.

use bytes::Bytes;
use sync_core_proto::{BatchCommit, BatchResult, Hash32, Staged};
use uuid::Uuid;

use crate::error::SyncError;
use crate::request::{Method, Request, RequestPath};
use crate::tree_ops::{decode, send};

/// `PUT /api/v1/staged-uploads/{staged_key}`; the 2xx body is the staged row.
pub(crate) async fn stage_small(
    path: &RequestPath,
    staged_key: Uuid,
    bytes: Bytes,
    hash: &Hash32,
) -> Result<Staged, SyncError> {
    let request = Request {
        method: Method::Put,
        target: format!("/api/v1/staged-uploads/{staged_key}"),
        headers: vec![
            (
                "content-type".to_owned(),
                "application/octet-stream".to_owned(),
            ),
            ("x-blake3".to_owned(), hash.to_string()),
        ],
        body: bytes,
        max_attempts: path.default_attempts(),
    };
    decode("stage_small", send(path, "stage_small", request).await?)
}

/// `POST /api/v1/commits:batch`. A repeated `commit_id` with the same entries returns the stored response.
pub(crate) async fn batch_commit(
    path: &RequestPath,
    commit: &BatchCommit,
) -> Result<BatchResult, SyncError> {
    let body = serde_json::to_vec(commit).map_err(|error| {
        tracing::error!(%error, "batch commit does not serialise");
        SyncError::ProtocolViolation {
            detail: "batch commit does not serialise",
        }
    })?;
    let request = Request {
        method: Method::Post,
        target: "/api/v1/commits:batch".to_owned(),
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: Bytes::from(body),
        max_attempts: path.default_attempts(),
    };
    decode("batch_commit", send(path, "batch_commit", request).await?)
}
