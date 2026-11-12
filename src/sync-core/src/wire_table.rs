//! The wire-code table of `sync_api.md`: where a problem body becomes a typed error or a planner input. The one
//! exception is a route's own `Refusal` (`request/path.rs`): the mobile code exchange reads its refused code.
//!
//! Rows are keyed by `sc_error::ErrorCode` with no wildcard arm, so a code added to or renamed in
//! `sc_error` stops this crate from building until its row is decided.

use sc_error::{ApiError, ErrorCode};
use serde_json::Value;
use sync_core_proto::{DeviceRefusal, ResyncReason};

use crate::error::{Mapped, PlannerInput, SyncError};

const MAX_TOKEN_LEN: usize = 64;
const MAX_MESSAGE_CHARS: usize = 256;
const MAX_URL_LEN: usize = 2048;

/// Maps one error response (`status`, `(name, value)` header pairs, raw body). Total: any input gives a value.
///
/// Rows depend on the code only, not on the status, so an open status question (`batch_too_large` 422 or 413)
/// cannot change the result. `status` below 100 means no response arrived.
pub fn map_problem(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Mapped {
    let (code, params, correlation) = match ApiError::parse(status, headers, body) {
        ApiError::Transport(kind) => return Mapped::Error(SyncError::Transport(kind)),
        ApiError::Api {
            code,
            params,
            correlation_id,
        } => (code, params, correlation_id),
    };
    // A 426 is `upgrade_required` whatever the body says, so a gateway's bare 426 still stops the client.
    // Only a body that names the code is trusted for its text, minimum version and URL.
    let declared_upgrade = code == ErrorCode::UpgradeRequired;
    let code = if status == 426 {
        ErrorCode::UpgradeRequired
    } else {
        code
    };
    let raw = RawProblem::read(headers, body);
    let error = |error| Mapped::Error(error);
    let planner = |input| Mapped::Planner(input);

    match code {
        ErrorCode::StagedCapReached => error(SyncError::StagedCapReached),
        ErrorCode::CommitIdReused => error(SyncError::CommitIdReused),
        ErrorCode::BatchTooLarge => error(SyncError::BatchTooLarge),
        ErrorCode::SizeTooLarge => error(SyncError::SizeTooLarge),
        ErrorCode::AccountNotActive => error(SyncError::AccountNotActive),
        ErrorCode::BlocklistMismatch => error(SyncError::BlocklistMismatch),
        ErrorCode::ParentNotFound => error(SyncError::ParentNotFound),
        ErrorCode::ParentDeleted => error(SyncError::ParentGone),
        ErrorCode::NotAFolder => error(SyncError::NotAFolder),
        ErrorCode::QuotaExceeded => error(SyncError::QuotaExceeded),
        ErrorCode::FreeSpaceFloor => error(SyncError::FreeSpaceFloor),
        ErrorCode::ResyncRequired => match params.reason {
            Some(reason) => error(SyncError::ResyncRequired {
                reason: reason.into(),
            }),
            None => unexpected(code, &raw, &correlation),
        },
        ErrorCode::InvalidCursor => error(SyncError::ResyncRequired {
            reason: ResyncReason::InvalidCursor,
        }),
        ErrorCode::BlockConflict => planner(PlannerInput::BlockConflict {
            retry_after: params.retry_after,
        }),
        ErrorCode::BlockHashMismatch => planner(PlannerInput::HashRejected),
        ErrorCode::UploadSessionGone => planner(PlannerInput::SessionGone),
        ErrorCode::NeedBlocks => planner(PlannerInput::NeedBlocks {
            list: params.need_blocks,
        }),
        ErrorCode::NameConflict => error(SyncError::NameConflict),
        ErrorCode::EditedSince => error(SyncError::EditedSince { current: None }),
        ErrorCode::Cycle => error(SyncError::Cycle),
        ErrorCode::InvalidName => error(SyncError::InvalidName),
        ErrorCode::SessionCapExceeded => error(SyncError::SessionCapReached),
        ErrorCode::NotFound => error(SyncError::NotFound),
        ErrorCode::ContentUnavailable => error(SyncError::BlobUnavailable),
        ErrorCode::Unauthenticated => error(SyncError::Unauthorized),
        ErrorCode::RateLimited => error(SyncError::RateLimited {
            retry_after: params.retry_after,
        }),
        ErrorCode::UpgradeRequired if !declared_upgrade => error(SyncError::UpgradeRequired {
            message: code.title().to_owned(),
            min_version: None,
            update_url: None,
        }),
        ErrorCode::UpgradeRequired => error(SyncError::UpgradeRequired {
            message: raw.message.unwrap_or_else(|| code.title().to_owned()),
            min_version: params.min_version.filter(|v| is_safe_version(v)),
            update_url: raw.update_url,
        }),
        // Known to `sc_error` but without a row in the contract's table: reported with their own code.
        ErrorCode::InvalidRequest
        | ErrorCode::PasswordRequired
        | ErrorCode::Forbidden
        | ErrorCode::LastAdmin
        | ErrorCode::RangeNotSatisfiable
        | ErrorCode::TargetNotShareable
        | ErrorCode::BlockLengthMismatch
        | ErrorCode::Internal
        | ErrorCode::ServiceUnavailable
        | ErrorCode::Overloaded
        | ErrorCode::LeaseLost
        | ErrorCode::RevisionObsolete
        | ErrorCode::ThumbnailNotReady
        | ErrorCode::ThumbnailUnavailable
        | ErrorCode::LengthRequired
        | ErrorCode::CsrfRejected
        | ErrorCode::OutputTooLarge
        | ErrorCode::OutputTypeRejected
        | ErrorCode::OutputKindRejected => unexpected(code, &raw, &correlation),
    }
}

fn unexpected(
    code: ErrorCode,
    raw: &RawProblem,
    correlation: &Option<sc_error::CorrelationId>,
) -> Mapped {
    // `ApiError::parse` folds an unknown code into `internal`; the raw code is what the caller reports. The
    // RFC 8628 poll refusals have no `sc_error` row (`problem_details.md` open point 3), so they arrive here.
    let code = match (code, &raw.code) {
        (ErrorCode::Internal, Some(raw_code)) => {
            if let Some(refusal) = DeviceRefusal::from_code(raw_code) {
                return Mapped::Error(SyncError::DeviceGrantRefused(refusal));
            }
            raw_code.clone()
        }
        _ => code.as_str().to_owned(),
    };
    let request_id = raw
        .request_id
        .clone()
        .or_else(|| correlation.as_ref().map(ToString::to_string));
    Mapped::Error(SyncError::Unexpected { code, request_id })
}

/// The fields `ApiError::parse` does not keep: the raw code and request id, the server text and the update URL.
/// Every value passed a charset or shape check, so none can carry free text into a `Display`.
struct RawProblem {
    code: Option<String>,
    request_id: Option<String>,
    message: Option<String>,
    update_url: Option<String>,
}

impl RawProblem {
    fn read(headers: &[(&str, &str)], body: &[u8]) -> Self {
        let value = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
        let text = |name: &str| value.get(name).and_then(Value::as_str);
        let token = |name: &str| {
            text(name)
                .filter(|t| is_safe_token(t, false))
                .map(str::to_owned)
        };
        RawProblem {
            code: token("code"),
            request_id: token("request_id").or_else(|| {
                headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("x-request-id"))
                    .map(|(_, v)| *v)
                    .filter(|v| is_safe_token(v, false))
                    .map(str::to_owned)
            }),
            message: text("detail")
                .or_else(|| text("title"))
                .map(safe_message)
                .filter(|m| !m.is_empty()),
            update_url: text("update_url")
                .filter(|url| is_safe_https_url(url))
                .map(str::to_owned),
        }
    }
}

/// Code and request id charset: ASCII letters, digits, `_`, `-` (and `.` for versions when `dotted`).
fn is_safe_token(text: &str, dotted: bool) -> bool {
    !text.is_empty()
        && text.len() <= MAX_TOKEN_LEN
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || (dotted && b == b'.'))
}

/// A version starts with a digit, so a file name cannot pass as one.
fn is_safe_version(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_digit()) && is_safe_token(text, true)
}

fn safe_message(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_MESSAGE_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn is_safe_https_url(text: &str) -> bool {
    text.len() <= MAX_URL_LEN
        && text.starts_with("https://")
        && text.len() > "https://".len()
        && text.chars().all(|c| c.is_ascii_graphic())
}
