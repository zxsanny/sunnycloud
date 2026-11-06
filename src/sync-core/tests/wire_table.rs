//! AC-1..AC-4 of AZ-3958: every row of the `sync_api` wire-code table, unknown codes, 426 and both resync statuses.

use std::time::Duration;

use sc_error::{
    BusinessParams, CorrelationId, ErrorCode, ProblemBody, ResyncReason as WireReason,
    TransportKind,
};
use sync_core::{Mapped, PlannerInput, SyncError, map_problem};
use sync_core_proto::{BlockResult, CommitResult, ResyncReason};
use uuid::Uuid;

fn request_id() -> CorrelationId {
    CorrelationId::new(Uuid::from_u128(0x0192_8f3a_7b00_7abc_8def_0123_4567_89ab))
}

fn body(code: ErrorCode, params: BusinessParams) -> Vec<u8> {
    let problem = ProblemBody {
        code,
        request_id: request_id(),
        params,
        problems: Vec::new(),
    };
    serde_json::to_vec(&problem).expect("problem body serialises")
}

fn map(code: ErrorCode, params: BusinessParams) -> Mapped {
    map_problem(code.status(), &[], &body(code, params))
}

fn error(code: ErrorCode) -> Mapped {
    map(code, BusinessParams::default())
}

fn seconds(n: u64) -> Option<Duration> {
    Some(Duration::from_secs(n))
}

#[test]
fn plain_rows_map_to_their_typed_error() {
    let rows = [
        (ErrorCode::StagedCapReached, SyncError::StagedCapReached),
        (ErrorCode::CommitIdReused, SyncError::CommitIdReused),
        (ErrorCode::BatchTooLarge, SyncError::BatchTooLarge),
        (ErrorCode::SizeTooLarge, SyncError::SizeTooLarge),
        (ErrorCode::AccountNotActive, SyncError::AccountNotActive),
        (ErrorCode::BlocklistMismatch, SyncError::BlocklistMismatch),
        (ErrorCode::ParentNotFound, SyncError::ParentNotFound),
        (ErrorCode::ParentDeleted, SyncError::ParentGone),
        (ErrorCode::NotAFolder, SyncError::NotAFolder),
        (ErrorCode::QuotaExceeded, SyncError::QuotaExceeded),
        (ErrorCode::FreeSpaceFloor, SyncError::FreeSpaceFloor),
        (ErrorCode::NameConflict, SyncError::NameConflict),
        (
            ErrorCode::EditedSince,
            SyncError::EditedSince { current: None },
        ),
        (ErrorCode::Cycle, SyncError::Cycle),
        (ErrorCode::InvalidName, SyncError::InvalidName),
        (ErrorCode::SessionCapExceeded, SyncError::SessionCapReached),
        (ErrorCode::NotFound, SyncError::NotFound),
        (ErrorCode::ContentUnavailable, SyncError::BlobUnavailable),
        (ErrorCode::Unauthenticated, SyncError::Unauthorized),
    ];

    for (code, expected) in rows {
        assert_eq!(error(code), Mapped::Error(expected), "row {code:?}");
    }
}

#[test]
fn rate_limited_carries_retry_after() {
    let params = BusinessParams {
        retry_after: seconds(3),
        ..BusinessParams::default()
    };

    assert_eq!(
        map(ErrorCode::RateLimited, params),
        Mapped::Error(SyncError::RateLimited {
            retry_after: seconds(3)
        })
    );
}

#[test]
fn rate_limited_reads_the_header_when_the_body_has_no_delay() {
    let body = body(ErrorCode::RateLimited, BusinessParams::default());

    let mapped = map_problem(429, &[("Retry-After", "7")], &body);

    assert_eq!(
        mapped,
        Mapped::Error(SyncError::RateLimited {
            retry_after: seconds(7)
        })
    );
}

#[test]
fn planner_rows_map_to_planner_inputs() {
    let conflict = BusinessParams {
        retry_after: seconds(2),
        ..BusinessParams::default()
    };
    let need = BusinessParams {
        need_blocks: vec![3, 9, 4_000_000_000],
        ..BusinessParams::default()
    };

    assert_eq!(
        map(ErrorCode::BlockConflict, conflict),
        Mapped::Planner(PlannerInput::BlockConflict {
            retry_after: seconds(2)
        })
    );
    assert_eq!(
        error(ErrorCode::BlockConflict),
        Mapped::Planner(PlannerInput::BlockConflict { retry_after: None })
    );
    assert_eq!(
        error(ErrorCode::BlockHashMismatch),
        Mapped::Planner(PlannerInput::HashRejected)
    );
    assert_eq!(
        error(ErrorCode::UploadSessionGone),
        Mapped::Planner(PlannerInput::SessionGone)
    );
    assert_eq!(
        map(ErrorCode::NeedBlocks, need),
        Mapped::Planner(PlannerInput::NeedBlocks {
            list: vec![3, 9, 4_000_000_000]
        })
    );
}

#[test]
fn planner_inputs_convert_to_the_planner_results_of_their_call_site() {
    let conflict = PlannerInput::BlockConflict {
        retry_after: seconds(1),
    };
    let need = PlannerInput::NeedBlocks { list: vec![2, 5] };

    assert_eq!(
        conflict.as_block_result(),
        Some(BlockResult::BlockConflict {
            retry_after: seconds(1)
        })
    );
    assert_eq!(conflict.as_commit_result(), None);
    assert_eq!(
        PlannerInput::HashRejected.as_block_result(),
        Some(BlockResult::HashRejected)
    );
    assert_eq!(PlannerInput::HashRejected.as_commit_result(), None);
    assert_eq!(
        PlannerInput::SessionGone.as_block_result(),
        Some(BlockResult::SessionGone)
    );
    assert_eq!(
        PlannerInput::SessionGone.as_commit_result(),
        Some(CommitResult::SessionGone)
    );
    assert_eq!(
        need.as_commit_result(),
        Some(CommitResult::NeedBlocks { list: vec![2, 5] })
    );
    assert_eq!(need.as_block_result(), None);
}

#[test]
fn unknown_code_keeps_its_identity_and_request_id() {
    let body = br#"{"type":"urn:sunnycloud:error:weird_code","title":"x","status":418,"code":"weird_code","request_id":"r-1"}"#;

    let mapped = map_problem(418, &[], body);

    assert_eq!(
        mapped,
        Mapped::Error(SyncError::Unexpected {
            code: "weird_code".to_owned(),
            request_id: Some("r-1".to_owned()),
        })
    );
}

#[test]
fn known_codes_without_a_table_row_are_unexpected_with_their_own_code() {
    let unmapped = [
        ErrorCode::InvalidRequest,
        ErrorCode::PasswordRequired,
        ErrorCode::Forbidden,
        ErrorCode::LastAdmin,
        ErrorCode::RangeNotSatisfiable,
        ErrorCode::TargetNotShareable,
        ErrorCode::BlockLengthMismatch,
        ErrorCode::Internal,
        ErrorCode::ServiceUnavailable,
        ErrorCode::Overloaded,
    ];

    for code in unmapped {
        assert_eq!(
            error(code),
            Mapped::Error(SyncError::Unexpected {
                code: code.as_str().to_owned(),
                request_id: Some(request_id().to_string()),
            }),
            "code {code:?}"
        );
    }
}

#[test]
fn unreadable_bodies_are_unexpected_internal_and_keep_the_header_request_id() {
    for raw in [&b""[..], b"<html>502</html>", b"[1]", b"\xff\xfe"] {
        let mapped = map_problem(502, &[("X-Request-Id", "req-9")], raw);

        assert_eq!(
            mapped,
            Mapped::Error(SyncError::Unexpected {
                code: "internal".to_owned(),
                request_id: Some("req-9".to_owned()),
            })
        );
    }
    let id = request_id().to_string();
    let mapped = map_problem(502, &[("X-Request-Id", id.as_str())], b"oops");
    assert_eq!(
        mapped,
        Mapped::Error(SyncError::Unexpected {
            code: "internal".to_owned(),
            request_id: Some(id),
        })
    );
}

#[test]
fn status_zero_is_a_transport_failure() {
    assert_eq!(
        map_problem(0, &[], b""),
        Mapped::Error(SyncError::Transport(TransportKind::Other))
    );
}

#[test]
fn upgrade_required_carries_text_and_url_and_is_not_retryable() {
    let raw = br#"{"code":"upgrade_required","status":426,"title":"Upgrade required","detail":"Please upgrade to 2.1.0","min_version":"2.1.0","update_url":"https://sunnycloud.test/download","request_id":"r-2"}"#;

    let mapped = map_problem(426, &[], raw);

    let Mapped::Error(error) = mapped else {
        panic!("expected an error, got {mapped:?}");
    };
    assert_eq!(
        error,
        SyncError::UpgradeRequired {
            message: "Please upgrade to 2.1.0".to_owned(),
            min_version: Some("2.1.0".to_owned()),
            update_url: Some("https://sunnycloud.test/download".to_owned()),
        }
    );
    assert!(!error.is_retryable());
}

#[test]
fn upgrade_required_falls_back_to_the_title_and_drops_an_unsafe_url() {
    let raw = br#"{"code":"upgrade_required","title":"Upgrade required","update_url":"javascript:alert(1)"}"#;

    let mapped = map_problem(426, &[], raw);

    assert_eq!(
        mapped,
        Mapped::Error(SyncError::UpgradeRequired {
            message: "Upgrade required".to_owned(),
            min_version: None,
            update_url: None,
        })
    );
}

#[test]
fn only_transport_and_rate_limit_are_retryable() {
    assert!(SyncError::Transport(TransportKind::Timeout).is_retryable());
    assert!(SyncError::RateLimited { retry_after: None }.is_retryable());
    for code in ErrorCode::ALL {
        if let Mapped::Error(error) = error(*code) {
            assert_eq!(
                error.is_retryable(),
                matches!(error, SyncError::RateLimited { .. }),
                "code {code:?}"
            );
        }
    }
}

#[test]
fn resync_from_each_410_reason_and_from_422_invalid_cursor() {
    let cases = [
        (WireReason::EpochChanged, ResyncReason::EpochChanged),
        (
            WireReason::BelowPurgeHorizon,
            ResyncReason::BelowPurgeHorizon,
        ),
        (WireReason::CursorAhead, ResyncReason::CursorAhead),
    ];

    for (wire, expected) in cases {
        let params = BusinessParams {
            reason: Some(wire),
            ..BusinessParams::default()
        };
        assert_eq!(
            map_problem(410, &[], &body(ErrorCode::ResyncRequired, params.clone())),
            Mapped::Error(SyncError::ResyncRequired { reason: expected }),
            "reason {wire:?}"
        );
    }
    assert_eq!(
        map_problem(
            422,
            &[],
            &body(ErrorCode::InvalidCursor, BusinessParams::default())
        ),
        Mapped::Error(SyncError::ResyncRequired {
            reason: ResyncReason::InvalidCursor
        })
    );
}

#[test]
fn resync_without_a_known_reason_is_reported_not_guessed() {
    let raw = br#"{"code":"resync_required","reason":"moon_phase","request_id":"r-3"}"#;

    assert_eq!(
        map_problem(410, &[], raw),
        Mapped::Error(SyncError::Unexpected {
            code: "resync_required".to_owned(),
            request_id: Some("r-3".to_owned()),
        })
    );
}

#[test]
fn the_row_is_chosen_by_code_not_by_status() {
    for status in [413, 422] {
        assert_eq!(
            map_problem(
                status,
                &[],
                &body(ErrorCode::BatchTooLarge, BusinessParams::default())
            ),
            Mapped::Error(SyncError::BatchTooLarge)
        );
    }
}

#[test]
fn every_sc_error_code_has_a_documented_outcome() {
    for code in ErrorCode::ALL {
        let mapped = error(*code);

        let documented = match mapped {
            Mapped::Planner(_) => true,
            Mapped::Error(SyncError::Unexpected { code: ref raw, .. }) => raw == code.as_str(),
            Mapped::Error(_) => true,
        };
        assert!(documented, "code {code:?} mapped to {mapped:?}");
    }
}
