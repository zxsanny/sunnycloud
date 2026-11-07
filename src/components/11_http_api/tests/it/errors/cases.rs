use std::error::Error;
use std::time::Duration;

use contracts::{
    AdminError, AuthenticatorError, ChangeFeedError, CommitServiceError, DownloadError,
    PublicShareError, ResyncReason, TreeReaderError, TreeServiceError, UploadError, WorkerJobError,
};
use sc_error::{
    BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode, FieldProblem,
};
use sunnycloud_http_api::ApiProblem;

pub struct Case {
    pub name: &'static str,
    pub status: u16,
    pub code: &'static str,
    build: fn(CorrelationId) -> ApiProblem,
}

#[derive(Debug, thiserror::Error)]
#[error("query failed: SELECT * FROM nodes WHERE account_id = 7")]
struct SqlFailure(#[source] FileFailure);

#[derive(Debug, thiserror::Error)]
#[error("cannot read /var/lib/sunnycloud/blobs/ab/cd/x")]
struct FileFailure(#[source] KeyFailure);

#[derive(Debug, thiserror::Error)]
#[error("key material K3Y-MARK rejected")]
struct KeyFailure;

pub const PLANTED: [&str; 3] = [
    "SELECT * FROM nodes WHERE",
    "/var/lib/sunnycloud/blobs/ab/cd/x",
    "K3Y-MARK",
];

pub fn planted_chain() -> Box<dyn Error + Send + Sync> {
    Box::new(SqlFailure(FileFailure(KeyFailure)))
}

fn business(code: ErrorCode, params: BusinessParams) -> DomainError {
    DomainError::Business(BusinessError::new(code, params))
}

pub fn build(name: &str, request_id: CorrelationId) -> Option<ApiProblem> {
    CASES
        .iter()
        .find(|case| case.name == name)
        .map(|case| (case.build)(request_id))
}

const fn case(
    name: &'static str,
    status: u16,
    code: &'static str,
    build: fn(CorrelationId) -> ApiProblem,
) -> Case {
    Case {
        name,
        status,
        code,
        build,
    }
}

pub const CASES: &[Case] = &[
    case("validation", 400, "invalid_request", |id| {
        let problems = vec![FieldProblem {
            field: "name".to_owned(),
            problem: "empty".to_owned(),
        }];
        ApiProblem::new(DomainError::Validation(problems), id)
    }),
    case("invalid_name", 422, "invalid_name", |id| {
        ApiProblem::from_port(TreeServiceError::NameInvalid("a/b".to_owned()), id)
    }),
    case("unauthenticated", 401, "unauthenticated", |id| {
        ApiProblem::from_port(AuthenticatorError::Unauthenticated, id)
    }),
    case("forbidden", 403, "forbidden", |id| {
        ApiProblem::from_port(AuthenticatorError::Forbidden, id)
    }),
    case("not_found", 404, "not_found", |id| {
        ApiProblem::from_port(TreeReaderError::NotFound, id)
    }),
    case("name_conflict", 409, "name_conflict", |id| {
        ApiProblem::from_port(TreeServiceError::NameConflict, id)
    }),
    case("block_conflict", 409, "block_conflict", |id| {
        ApiProblem::from_port(UploadError::BlockConflict, id)
    }),
    case("rate_limited", 429, "rate_limited", |id| {
        let error = PublicShareError::RateLimited {
            retry_after: Duration::from_secs(17),
        };
        ApiProblem::from_port(error, id)
    }),
    case("too_many_holds", 429, "rate_limited", |id| {
        let error = ChangeFeedError::TooManyHolds {
            retry_after: Duration::from_secs(5),
        };
        ApiProblem::from_port(error, id)
    }),
    case("resync_required", 410, "resync_required", |id| {
        let error = ChangeFeedError::ResyncRequired {
            reason: ResyncReason::EpochChanged,
        };
        ApiProblem::from_port(error, id)
    }),
    case("upload_session_gone", 410, "upload_session_gone", |id| {
        ApiProblem::from_port(UploadError::UploadNotFound, id)
    }),
    case("need_blocks", 409, "need_blocks", |id| {
        ApiProblem::from_port(CommitServiceError::NeedBlocks(vec![3, 7]), id)
    }),
    case("edited_since", 409, "edited_since", |id| {
        ApiProblem::from_port(CommitServiceError::EditedSince, id)
    }),
    case("batch_too_large", 422, "batch_too_large", |id| {
        ApiProblem::from_port(CommitServiceError::BatchTooLarge, id)
    }),
    case("size_too_large", 413, "size_too_large", |id| {
        ApiProblem::from_port(UploadError::SizeTooLarge, id)
    }),
    case(
        "range_not_satisfiable",
        416,
        "range_not_satisfiable",
        |id| ApiProblem::from_port(DownloadError::RangeNotSatisfiable, id),
    ),
    case("upgrade_required", 426, "upgrade_required", |id| {
        let params = BusinessParams {
            min_version: Some("1.2.0".to_owned()),
            ..BusinessParams::default()
        };
        ApiProblem::new(business(ErrorCode::UpgradeRequired, params), id)
    }),
    case("service_unavailable", 503, "service_unavailable", |id| {
        let params = BusinessParams {
            retry_after: Some(Duration::from_secs(9)),
            ..BusinessParams::default()
        };
        ApiProblem::new(business(ErrorCode::ServiceUnavailable, params), id)
    }),
    case("quota_exceeded", 507, "quota_exceeded", |id| {
        ApiProblem::from_port(UploadError::QuotaExceeded, id)
    }),
    case("last_admin", 409, "last_admin", |id| {
        ApiProblem::from_port(AdminError::LastAdmin, id)
    }),
    case("lease_lost", 409, "lease_lost", |id| {
        ApiProblem::from_port(WorkerJobError::LeaseLost, id)
    }),
    case("revision_obsolete", 404, "revision_obsolete", |id| {
        ApiProblem::from_port(WorkerJobError::RevisionObsolete, id)
    }),
    case("output_too_large", 413, "output_too_large", |id| {
        ApiProblem::from_port(WorkerJobError::OutputTooLarge, id)
    }),
    case("output_type_rejected", 422, "output_type_rejected", |id| {
        ApiProblem::from_port(WorkerJobError::OutputTypeRejected, id)
    }),
    case("output_kind_rejected", 422, "output_kind_rejected", |id| {
        ApiProblem::from_port(WorkerJobError::OutputKindRejected, id)
    }),
    case("internal", 500, "internal", |id| {
        ApiProblem::new(DomainError::unexpected(planted_chain(), id), id)
    }),
];
