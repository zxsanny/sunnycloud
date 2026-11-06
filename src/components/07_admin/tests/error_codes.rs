//! AC-2: each 07 error converts to its wire code with the status of the shared table.

use contracts::{AdminError, DeletionError};
use sc_error::ErrorCode;
use sunnycloud_admin::{admin_error_code, deletion_error_code};

fn fault() -> Box<dyn std::error::Error + Send + Sync> {
    "disk on fire".into()
}

#[test]
fn last_admin_is_409_last_admin() {
    // Arrange
    let from_admin = AdminError::LastAdmin;
    let from_deletion = DeletionError::LastAdmin;

    // Act
    let codes = [
        admin_error_code(&from_admin),
        deletion_error_code(&from_deletion),
    ];

    // Assert
    for code in codes {
        let code = code.expect("last admin has a code");
        assert_eq!((code.as_str(), code.status()), ("last_admin", 409));
    }
}

#[test]
fn not_found_is_404_not_found() {
    // Arrange
    let error = AdminError::NotFound;

    // Act
    let code = admin_error_code(&error).expect("not found has a code");

    // Assert
    assert_eq!((code.as_str(), code.status()), ("not_found", 404));
}

#[test]
fn forbidden_is_403_forbidden() {
    // Arrange
    let error = DeletionError::Forbidden;

    // Act
    let code = deletion_error_code(&error).expect("forbidden has a code");

    // Assert
    assert_eq!((code.as_str(), code.status()), ("forbidden", 403));
}

#[test]
fn unexpected_failures_are_the_opaque_internal_code() {
    // Arrange
    let admin = AdminError::Unexpected(fault());
    let deletion = DeletionError::Unexpected(fault());

    // Act
    let codes = [admin_error_code(&admin), deletion_error_code(&deletion)];

    // Assert
    assert_eq!(
        codes,
        [Some(ErrorCode::Internal), Some(ErrorCode::Internal)]
    );
}

#[test]
fn errors_without_a_code_in_the_shared_table_map_to_none() {
    // Arrange
    let invalid_limit = AdminError::InvalidLimit;
    let mismatch = DeletionError::ConfirmationMismatch;

    // Act
    let codes = [
        admin_error_code(&invalid_limit),
        deletion_error_code(&mismatch),
    ];

    // Assert
    assert_eq!(codes, [None, None]);
}
