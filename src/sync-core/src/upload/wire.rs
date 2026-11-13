//! The four routes of a large upload and what their answers mean to the planner (`sync_api.md` wire table).

use bytes::Bytes;
use serde::Deserialize;
use serde_json::{Value, json};
use sync_core_proto::{BlockResult, CommitOutcome, FileHash, Hash32, StartOutcome};
use uuid::Uuid;

use crate::error::{Mapped, PlannerInput, SyncError};
use crate::request::{Method, Request, RequestPath, WireResponse};
use crate::tree_ops::{decode, into_error, send};

/// What `start_upload` sends. The same values go out on every retry, resume and lost-commit recovery.
pub(super) struct StartBody<'a> {
    pub(super) parent: Uuid,
    pub(super) name: &'a str,
    pub(super) declared_size: u64,
    pub(super) client_key: Uuid,
    pub(super) content_hash: &'a FileHash,
    pub(super) base_revision: Option<Uuid>,
    pub(super) client_meta: Option<&'a Value>,
}

/// `POST /api/v1/uploads`; idempotent by `client_key`, so it uses the default number of sends.
pub(super) async fn start_upload(
    path: &RequestPath,
    body: &StartBody<'_>,
) -> Result<StartOutcome, SyncError> {
    let body = json!({
        "parent_id": body.parent,
        "name": body.name,
        "declared_size": body.declared_size,
        "client_key": body.client_key,
        "content_hash": body.content_hash.to_string(),
        "base_revision": body.base_revision,
        "client_meta": body.client_meta,
    });
    let request = json_request(Method::Post, "/api/v1/uploads".to_owned(), &body)?;
    let request = Request {
        max_attempts: path.default_attempts(),
        ..request
    };
    decode("start_upload", send(path, "start_upload", request).await?)
}

/// The part of the `GET /api/v1/uploads/{id}` answer the planner reads.
#[derive(Deserialize)]
struct Listing {
    need_blocks: Vec<u32>,
}

/// `GET /api/v1/uploads/{id}`: the blocks the server still lacks. `SessionGone` stays a planner input.
pub(super) async fn get_upload(path: &RequestPath, upload_id: Uuid) -> Result<Vec<u32>, Mapped> {
    let request = Request {
        method: Method::Get,
        target: format!("/api/v1/uploads/{upload_id}"),
        headers: Vec::new(),
        body: Bytes::new(),
        max_attempts: path.default_attempts(),
    };
    let response = path.send("get_upload", request).await?;
    let listing: Listing = decode("get_upload", response).map_err(Mapped::Error)?;
    Ok(listing.need_blocks)
}

/// What the commit request carries besides the session.
pub(super) struct CommitBody<'a> {
    pub(super) upload_id: Uuid,
    pub(super) parent: Uuid,
    pub(super) name: &'a str,
    pub(super) blocklist: &'a [Hash32],
    pub(super) base_revision: Option<Uuid>,
    pub(super) client_meta: Option<&'a Value>,
    pub(super) import_hint: bool,
}

/// `POST /api/v1/commits`, sent once: a lost answer is recovered through `start_upload` with the same key
/// (`sync_api.md` § Lost-commit recovery), never by sending the commit again.
pub(super) async fn commit(
    path: &RequestPath,
    body: &CommitBody<'_>,
) -> Result<CommitOutcome, Mapped> {
    let blocklist: Vec<String> = body.blocklist.iter().map(ToString::to_string).collect();
    let body = json!({
        "upload_id": body.upload_id,
        "parent_id": body.parent,
        "name": body.name,
        "blocklist": blocklist,
        "base_revision": body.base_revision,
        "client_meta": body.client_meta,
        "import_hint": body.import_hint,
    });
    let request =
        json_request(Method::Post, "/api/v1/commits".to_owned(), &body).map_err(Mapped::Error)?;
    let response = path.send("commit", request).await?;
    decode("commit", response).map_err(Mapped::Error)
}

fn json_request(method: Method, target: String, body: &Value) -> Result<Request, SyncError> {
    let body = serde_json::to_vec(body).map_err(|error| {
        tracing::error!(%error, "upload request does not serialise");
        SyncError::ProtocolViolation {
            detail: "upload request does not serialise",
        }
    })?;
    Ok(Request {
        method,
        target,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: Bytes::from(body),
        max_attempts: 1,
    })
}

/// One block `PUT` as the planner sees it, and the error to report if the block is given up.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Sent {
    pub(super) result: BlockResult,
    /// `None` exactly for `Acked`.
    pub(super) failure: Option<SyncError>,
}

/// `PUT /api/v1/uploads/{id}/blocks/{index}` with the block's BLAKE3, sent once: the planner owns the retries.
/// `Err` is a refusal that ends the upload.
pub(super) async fn put_block(
    path: &RequestPath,
    upload_id: Uuid,
    index: u32,
    bytes: Bytes,
    hash: &Hash32,
) -> Result<Sent, SyncError> {
    let request = Request {
        method: Method::Put,
        target: format!("/api/v1/uploads/{upload_id}/blocks/{index}"),
        headers: vec![
            (
                "content-type".to_owned(),
                "application/octet-stream".to_owned(),
            ),
            ("x-blake3".to_owned(), hash.to_string()),
        ],
        body: bytes,
        max_attempts: 1,
    };
    block_result(path.send_keeping_status("put_block", request).await)
}

/// A block is done only on 204 (AC-38). Other 2xx, 5xx, 429, a reset or a timeout leave it needed; 422
/// `block_hash_mismatch`, 409 `block_conflict` and 410 `upload_session_gone` are the planner's; any other
/// refusal (quota, account, credential, cancel) ends the upload.
pub(super) fn block_result(
    answer: Result<WireResponse, (Mapped, Option<u16>)>,
) -> Result<Sent, SyncError> {
    let transient = |retry_after, failure| Sent {
        result: BlockResult::Transient { retry_after },
        failure: Some(failure),
    };
    let planner = |result, failure| Sent {
        result,
        failure: Some(failure),
    };
    Ok(match answer {
        Ok(response) if response.status == 204 => Sent {
            result: BlockResult::Acked,
            failure: None,
        },
        Ok(_) => transient(
            None,
            SyncError::ProtocolViolation {
                detail: "block answered without 204",
            },
        ),
        Err((Mapped::Planner(PlannerInput::HashRejected), _)) => {
            planner(BlockResult::HashRejected, unexpected("block_hash_mismatch"))
        }
        Err((Mapped::Planner(PlannerInput::BlockConflict { retry_after }), _)) => planner(
            BlockResult::BlockConflict { retry_after },
            unexpected("block_conflict"),
        ),
        Err((Mapped::Planner(PlannerInput::SessionGone), _)) => {
            planner(BlockResult::SessionGone, unexpected("upload_session_gone"))
        }
        Err((Mapped::Error(error @ SyncError::Transport(_)), _)) => transient(None, error),
        Err((Mapped::Error(SyncError::RateLimited { retry_after }), _)) => {
            transient(retry_after, SyncError::RateLimited { retry_after })
        }
        Err((Mapped::Error(error @ SyncError::Unexpected { .. }), Some(status)))
            if status >= 500 =>
        {
            transient(None, error)
        }
        Err((mapped, _)) => return Err(into_error(mapped)),
    })
}

/// A planner answer reported as an error, after its block was given up or its session restarted too often.
pub(super) fn unexpected(code: &str) -> SyncError {
    SyncError::Unexpected {
        code: code.to_owned(),
        request_id: None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;
    use sc_error::TransportKind;
    use sync_core_proto::BlockResult;

    use super::{Sent, block_result};
    use crate::error::{Mapped, PlannerInput, SyncError};
    use crate::request::WireResponse;

    fn answered(status: u16) -> Result<WireResponse, (Mapped, Option<u16>)> {
        Ok(WireResponse {
            status,
            headers: Vec::new(),
            body: Bytes::from_static(b"{\"stored\":true}"),
        })
    }

    fn refused(
        error: SyncError,
        status: Option<u16>,
    ) -> Result<WireResponse, (Mapped, Option<u16>)> {
        Err((Mapped::Error(error), status))
    }

    fn internal() -> SyncError {
        SyncError::Unexpected {
            code: "internal".to_owned(),
            request_id: None,
        }
    }

    fn result_of(answer: Result<WireResponse, (Mapped, Option<u16>)>) -> Option<BlockResult> {
        block_result(answer).ok().map(|sent| sent.result)
    }

    #[test]
    fn only_204_acks_a_block() {
        // Arrange
        let answers = [200, 201, 202, 204, 206];

        // Act
        let results: Vec<Option<BlockResult>> = answers
            .iter()
            .map(|status| result_of(answered(*status)))
            .collect();

        // Assert
        let transient = Some(BlockResult::Transient { retry_after: None });
        assert_eq!(
            results,
            vec![
                transient,
                transient,
                transient,
                Some(BlockResult::Acked),
                transient
            ]
        );
        assert_eq!(
            block_result(answered(204)),
            Ok(Sent {
                result: BlockResult::Acked,
                failure: None
            })
        );
    }

    #[test]
    fn a_5xx_a_reset_and_a_429_leave_the_block_needed() {
        // Arrange
        let retry_after = Some(Duration::from_secs(3));
        let answers = [
            refused(internal(), Some(500)),
            refused(internal(), Some(503)),
            refused(SyncError::Transport(TransportKind::Other), None),
            refused(SyncError::Transport(TransportKind::Timeout), None),
            refused(SyncError::RateLimited { retry_after }, Some(429)),
        ];

        // Act
        let results: Vec<Option<BlockResult>> = answers.into_iter().map(result_of).collect();

        // Assert
        let transient = Some(BlockResult::Transient { retry_after: None });
        assert_eq!(
            results,
            vec![
                transient,
                transient,
                transient,
                transient,
                Some(BlockResult::Transient { retry_after })
            ]
        );
    }

    #[test]
    fn planner_answers_become_their_block_results() {
        // Arrange
        let retry_after = Some(Duration::from_secs(2));
        let planner = |input| Err((Mapped::Planner(input), Some(409)));

        // Act
        let rejected = result_of(planner(PlannerInput::HashRejected));
        let conflict = result_of(planner(PlannerInput::BlockConflict { retry_after }));
        let gone = result_of(planner(PlannerInput::SessionGone));

        // Assert
        assert_eq!(rejected, Some(BlockResult::HashRejected));
        assert_eq!(conflict, Some(BlockResult::BlockConflict { retry_after }));
        assert_eq!(gone, Some(BlockResult::SessionGone));
    }

    #[test]
    fn a_refusal_that_is_not_a_5xx_ends_the_upload() {
        // Arrange
        let answers = [
            (SyncError::QuotaExceeded, Some(507)),
            (SyncError::Unauthorized, Some(401)),
            (SyncError::Cancelled, None),
            (internal(), Some(422)),
        ];

        // Act
        let results: Vec<Result<Sent, SyncError>> = answers
            .into_iter()
            .map(|(error, status)| block_result(refused(error, status)))
            .collect();

        // Assert
        assert_eq!(
            results,
            vec![
                Err(SyncError::QuotaExceeded),
                Err(SyncError::Unauthorized),
                Err(SyncError::Cancelled),
                Err(internal()),
            ]
        );
    }
}
