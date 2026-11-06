use contracts::{AdminError, DeletionError};
use sc_error::ErrorCode;

/// The wire code of an `AdminService` failure, as in the shared status table. `None`: the table has no code
/// for it yet (`InvalidLimit`), and no substitute is chosen here.
pub fn admin_error_code(error: &AdminError) -> Option<ErrorCode> {
    match error {
        AdminError::LastAdmin => Some(ErrorCode::LastAdmin),
        AdminError::NotFound => Some(ErrorCode::NotFound),
        AdminError::InvalidLimit => None,
        AdminError::Unexpected(_) => Some(ErrorCode::Internal),
    }
}

/// The wire code of an `AccountDeletionService` failure. `None`: the table has no code for it yet
/// (`ConfirmationMismatch`).
pub fn deletion_error_code(error: &DeletionError) -> Option<ErrorCode> {
    match error {
        DeletionError::Forbidden => Some(ErrorCode::Forbidden),
        DeletionError::LastAdmin => Some(ErrorCode::LastAdmin),
        DeletionError::ConfirmationMismatch => None,
        DeletionError::Unexpected(_) => Some(ErrorCode::Internal),
    }
}
