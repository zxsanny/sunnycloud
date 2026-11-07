use std::time::Duration;

use contracts::{
    AccountServiceError, AdminError, AuthenticatorError, ChangeFeedError, CommitServiceError,
    DeletionError, DerivedError, DeviceGrantError, DownloadError, EntryError, InviteError,
    PublicShareError, ResyncReason, RevalidatorError, SessionError, ShareError, SignInError,
    ThumbnailStateError, TreeReaderError, TreeServiceError, UploadError, VaultError,
    WorkerAuthError, WorkerJobError,
};
use sc_error::{CorrelationId, DomainError, ErrorCode};
use sunnycloud_http_api::{PortError, entry_error_code};
use uuid::Uuid;

use super::probe::fresh_id;

enum Expect {
    Code(ErrorCode),
    Field(&'static str),
    Fault,
    AwaitingCode,
}

struct Row {
    variant: &'static str,
    expect: Expect,
    actual: DomainError,
}

const NO_CODE: &str = "has no sc_error code";

fn fault() -> contracts::Fault {
    "disk gone".into()
}

fn delay() -> Duration {
    Duration::from_secs(3)
}

macro_rules! rows {
    ($id:expr; $( $expect:expr => $error:expr ),+ $(,)?) => {
        vec![ $( Row {
            variant: stringify!($error),
            expect: $expect,
            actual: PortError::into_domain($error, $id),
        } ),+ ]
    };
}

use Expect::{AwaitingCode, Code, Fault, Field};

fn identity_rows(id: CorrelationId) -> Vec<Row> {
    rows![id;
        Code(ErrorCode::Unauthenticated) => AuthenticatorError::Unauthenticated,
        Code(ErrorCode::Forbidden) => AuthenticatorError::Forbidden,
        Fault => AuthenticatorError::Unexpected(fault()),
        Fault => RevalidatorError::Unexpected(fault()),
        Code(ErrorCode::Unauthenticated) => SessionError::ReuseDetected,
        Code(ErrorCode::Unauthenticated) => SessionError::Unauthenticated,
        Fault => SessionError::Unexpected(fault()),
        Field("provider") => SignInError::UnknownProvider,
        Field("return_to") => SignInError::InvalidReturnPath,
        Field("host") => SignInError::HostMismatch,
        Code(ErrorCode::RateLimited) => SignInError::RateLimited { retry_after: delay() },
        Code(ErrorCode::InvalidRequest) => SignInError::UntrustedState,
        Code(ErrorCode::NotFound) => SignInError::TrustedFailure { code: ErrorCode::NotFound },
        AwaitingCode => SignInError::InvitationRequired,
        Code(ErrorCode::ServiceUnavailable) => SignInError::ProviderUnavailable,
        AwaitingCode => SignInError::IdentityTaken,
        AwaitingCode => SignInError::InvalidCode,
        Fault => SignInError::Unexpected(fault()),
        AwaitingCode => DeviceGrantError::AuthorizationPending,
        AwaitingCode => DeviceGrantError::SlowDown,
        AwaitingCode => DeviceGrantError::AccessDenied,
        AwaitingCode => DeviceGrantError::ExpiredToken,
        Code(ErrorCode::NotFound) => DeviceGrantError::UnknownCode,
        Code(ErrorCode::RateLimited) => DeviceGrantError::RateLimited { retry_after: delay() },
        Code(ErrorCode::NotFound) => DeviceGrantError::Expired,
        Code(ErrorCode::NotFound) => DeviceGrantError::AlreadyDecided,
        Code(ErrorCode::NotFound) => DeviceGrantError::NotFound,
        Code(ErrorCode::Forbidden) => DeviceGrantError::Forbidden,
        Fault => DeviceGrantError::Unexpected(fault()),
        Fault => InviteError::Unexpected(fault()),
    ]
}

fn tree_rows(id: CorrelationId) -> Vec<Row> {
    rows![id;
        Code(ErrorCode::InvalidName) => TreeServiceError::NameInvalid("a/b".to_owned()),
        Code(ErrorCode::NameConflict) => TreeServiceError::NameConflict,
        Code(ErrorCode::ParentDeleted) => TreeServiceError::ParentDeleted,
        Code(ErrorCode::NotFound) => TreeServiceError::NotFound,
        Code(ErrorCode::NotAFolder) => TreeServiceError::NotAFolder,
        Code(ErrorCode::Cycle) => TreeServiceError::Cycle,
        Code(ErrorCode::InvalidRequest) => TreeServiceError::IsRoot,
        Code(ErrorCode::EditedSince) => TreeServiceError::EditedSince { current_revision: Uuid::nil() },
        Code(ErrorCode::InvalidCursor) => TreeServiceError::InvalidCursor,
        Fault => TreeServiceError::Unexpected(fault()),
        Code(ErrorCode::NotFound) => TreeReaderError::NotFound,
        Code(ErrorCode::NotAFolder) => TreeReaderError::NotAFolder,
        Code(ErrorCode::InvalidCursor) => TreeReaderError::InvalidCursor,
        Code(ErrorCode::BatchTooLarge) => TreeReaderError::BatchTooLarge,
        Fault => TreeReaderError::Unexpected(fault()),
    ]
}

fn transfer_rows(id: CorrelationId) -> Vec<Row> {
    rows![id;
        Code(ErrorCode::InvalidName) => UploadError::InvalidName,
        Code(ErrorCode::ParentNotFound) => UploadError::ParentNotFound,
        Code(ErrorCode::NotAFolder) => UploadError::NotAFolder,
        Code(ErrorCode::SizeTooLarge) => UploadError::SizeTooLarge,
        Code(ErrorCode::QuotaExceeded) => UploadError::QuotaExceeded,
        Code(ErrorCode::SessionCapExceeded) => UploadError::SessionCapReached,
        Code(ErrorCode::StagedCapReached) => UploadError::StagedCapReached,
        Code(ErrorCode::FreeSpaceFloor) => UploadError::FreeSpaceFloor,
        AwaitingCode => UploadError::ClientKeyMismatch,
        Code(ErrorCode::AccountNotActive) => UploadError::AccountNotActive,
        Code(ErrorCode::UploadSessionGone) => UploadError::UploadNotFound,
        Field("index") => UploadError::IndexOutOfRange,
        Code(ErrorCode::BlockLengthMismatch) => UploadError::LengthMismatch,
        Code(ErrorCode::BlockHashMismatch) => UploadError::HashMismatch,
        Code(ErrorCode::BlockConflict) => UploadError::BlockConflict,
        Code(ErrorCode::Overloaded) => UploadError::Overloaded,
        Code(ErrorCode::FreeSpaceFloor) => UploadError::StorageFull,
        Fault => UploadError::Storage(fault()),
        Fault => UploadError::Unexpected(fault()),
        Code(ErrorCode::NeedBlocks) => CommitServiceError::NeedBlocks(vec![1]),
        Code(ErrorCode::BlocklistMismatch) => CommitServiceError::BlocklistMismatch,
        Code(ErrorCode::UploadSessionGone) => CommitServiceError::UploadNotFound,
        Code(ErrorCode::InvalidName) => CommitServiceError::InvalidName,
        Code(ErrorCode::EditedSince) => CommitServiceError::EditedSince,
        Code(ErrorCode::ParentDeleted) => CommitServiceError::ParentGone,
        Code(ErrorCode::ParentNotFound) => CommitServiceError::ParentNotFound,
        Code(ErrorCode::NotAFolder) => CommitServiceError::NotAFolder,
        Code(ErrorCode::AccountNotActive) => CommitServiceError::AccountNotActive,
        Code(ErrorCode::BatchTooLarge) => CommitServiceError::BatchTooLarge,
        Code(ErrorCode::CommitIdReused) => CommitServiceError::CommitIdReused,
        Fault => CommitServiceError::Unexpected(fault()),
        Code(ErrorCode::NotFound) => DownloadError::NotFound,
        Code(ErrorCode::RangeNotSatisfiable) => DownloadError::RangeNotSatisfiable,
        Code(ErrorCode::ContentUnavailable) => DownloadError::ContentUnavailable,
        Fault => DownloadError::Storage(fault()),
        Fault => DownloadError::Unexpected(fault()),
        Code(ErrorCode::ResyncRequired) => ChangeFeedError::ResyncRequired { reason: ResyncReason::CursorAhead },
        Code(ErrorCode::Unauthenticated) => ChangeFeedError::Unauthenticated,
        Code(ErrorCode::InvalidCursor) => ChangeFeedError::BadCursor,
        Code(ErrorCode::RateLimited) => ChangeFeedError::TooManyHolds { retry_after: delay() },
        Fault => ChangeFeedError::Unexpected(fault()),
    ]
}

fn service_rows(id: CorrelationId) -> Vec<Row> {
    rows![id;
        Code(ErrorCode::NotFound) => ShareError::TargetNotFound,
        Code(ErrorCode::TargetNotShareable) => ShareError::TargetNotShareable,
        Field("expires_in") => ShareError::ExpiryOutOfRange,
        Code(ErrorCode::NotFound) => ShareError::NotFound,
        Fault => ShareError::Unexpected(fault()),
        Code(ErrorCode::NotFound) => PublicShareError::NotFound,
        Code(ErrorCode::PasswordRequired) => PublicShareError::WrongPassword,
        Code(ErrorCode::PasswordRequired) => PublicShareError::PasswordRequired,
        Code(ErrorCode::RateLimited) => PublicShareError::RateLimited { retry_after: delay() },
        Fault => PublicShareError::Unexpected(fault()),
        Code(ErrorCode::LastAdmin) => AdminError::LastAdmin,
        Code(ErrorCode::NotFound) => AdminError::NotFound,
        Field("limit") => AdminError::InvalidLimit,
        Fault => AdminError::Unexpected(fault()),
        Fault => AccountServiceError::Unexpected(fault()),
        Code(ErrorCode::Forbidden) => DeletionError::Forbidden,
        Code(ErrorCode::LastAdmin) => DeletionError::LastAdmin,
        Field("confirmation") => DeletionError::ConfirmationMismatch,
        Fault => DeletionError::Unexpected(fault()),
        Fault => ThumbnailStateError::Unexpected(fault()),
        Code(ErrorCode::ThumbnailNotReady) => DerivedError::NotReady,
        Code(ErrorCode::ThumbnailUnavailable) => DerivedError::NotFound,
        Fault => DerivedError::Unexpected(fault()),
        Code(ErrorCode::Forbidden) => WorkerAuthError::Forbidden,
        Code(ErrorCode::LeaseLost) => WorkerJobError::LeaseLost,
        Code(ErrorCode::RevisionObsolete) => WorkerJobError::RevisionObsolete,
        Code(ErrorCode::OutputTooLarge) => WorkerJobError::OutputTooLarge,
        Code(ErrorCode::OutputTypeRejected) => WorkerJobError::OutputTypeRejected,
        Code(ErrorCode::OutputKindRejected) => WorkerJobError::OutputKindRejected,
        Fault => WorkerJobError::Storage(fault()),
        Fault => WorkerJobError::Unexpected(fault()),
        AwaitingCode => VaultError::VaultExists,
        Code(ErrorCode::SizeTooLarge) => VaultError::HeaderTooLarge,
        Code(ErrorCode::NotFound) => VaultError::NotFound,
        AwaitingCode => VaultError::PreconditionFailed,
        Fault => VaultError::Unexpected(fault()),
    ]
}

fn all_rows(id: CorrelationId) -> Vec<Row> {
    let mut all = identity_rows(id);
    all.extend(tree_rows(id));
    all.extend(transfer_rows(id));
    all.extend(service_rows(id));
    all
}

#[test]
fn every_port_variant_maps_to_exactly_its_code_and_only_tier_three_falls_to_internal() {
    // Arrange
    let id = fresh_id();

    // Act
    let rows = all_rows(id);

    // Assert
    assert!(rows.len() > 100, "{} rows", rows.len());
    for row in &rows {
        match (&row.expect, &row.actual) {
            (Code(code), DomainError::Business(error)) => {
                assert_ne!(*code, ErrorCode::Internal, "{}", row.variant);
                assert_eq!(error.code, *code, "{}", row.variant);
            }
            (Field(field), DomainError::Validation(problems)) => {
                assert_eq!(problems.len(), 1, "{}", row.variant);
                assert_eq!(problems[0].field, *field, "{}", row.variant);
            }
            (
                Fault,
                DomainError::Unexpected {
                    source,
                    correlation_id,
                },
            ) => {
                assert!(!source.to_string().contains(NO_CODE), "{}", row.variant);
                assert_eq!(*correlation_id, id);
            }
            (AwaitingCode, DomainError::Unexpected { source, .. }) => {
                assert!(source.to_string().contains(NO_CODE), "{}", row.variant);
            }
            _ => panic!("{} was mapped to {:?}", row.variant, row.actual),
        }
    }
}

#[test]
fn variants_without_an_sc_error_code_are_exactly_the_known_gaps() {
    // Arrange
    let rows = all_rows(fresh_id());

    // Act
    let gaps: Vec<&str> = rows
        .iter()
        .filter(|row| matches!(row.expect, AwaitingCode))
        .map(|row| row.variant)
        .collect();

    // Assert
    assert_eq!(
        gaps,
        [
            "SignInError::InvitationRequired",
            "SignInError::IdentityTaken",
            "SignInError::InvalidCode",
            "DeviceGrantError::AuthorizationPending",
            "DeviceGrantError::SlowDown",
            "DeviceGrantError::AccessDenied",
            "DeviceGrantError::ExpiredToken",
            "UploadError::ClientKeyMismatch",
            "VaultError::VaultExists",
            "VaultError::PreconditionFailed",
        ]
    );
}

#[test]
fn every_batch_entry_error_has_its_code() {
    // Arrange
    let entries = [
        (EntryError::InvalidName, ErrorCode::InvalidName),
        (EntryError::EditedSince, ErrorCode::EditedSince),
        (EntryError::ParentGone, ErrorCode::ParentDeleted),
        (EntryError::ParentNotFound, ErrorCode::ParentNotFound),
        (EntryError::NotAFolder, ErrorCode::NotAFolder),
        (EntryError::BlocklistMismatch, ErrorCode::BlocklistMismatch),
        (EntryError::UploadNotFound, ErrorCode::UploadSessionGone),
        (
            EntryError::EffectFailed(ErrorCode::QuotaExceeded),
            ErrorCode::QuotaExceeded,
        ),
    ];

    for (error, code) in entries {
        // Act
        let mapped = entry_error_code(&error);

        // Assert
        assert_eq!(mapped, code, "{error:?}");
    }
}

#[test]
fn a_retry_after_of_a_port_error_reaches_the_domain_error_unchanged() {
    // Arrange
    let error = ChangeFeedError::TooManyHolds {
        retry_after: delay(),
    };

    // Act
    let domain = error.into_domain(fresh_id());

    // Assert
    let DomainError::Business(business) = domain else {
        panic!("not a business error: {domain:?}");
    };
    assert_eq!(business.params.retry_after, Some(delay()));
}
