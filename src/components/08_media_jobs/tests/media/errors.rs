//! AC-6: each 08 error converts to its wire code and status, and every code has exactly one status in the
//! shared table.

use media_api::MediaError;
use sc_error::{BusinessError, DomainError, ErrorCode};

const CASES: [(MediaError, &str, u16); 8] = [
    (MediaError::LeaseLost, "lease_lost", 409),
    (MediaError::RevisionObsolete, "revision_obsolete", 404),
    (MediaError::NotReady, "thumbnail_not_ready", 404),
    (MediaError::NotFound, "thumbnail_unavailable", 404),
    (MediaError::Forbidden, "forbidden", 403),
    (MediaError::OutputTooLarge, "output_too_large", 413),
    (MediaError::OutputTypeRejected, "output_type_rejected", 422),
    (MediaError::OutputKindRejected, "output_kind_rejected", 422),
];

#[test]
fn every_error_has_its_code_and_status() {
    for (error, wire, status) in CASES {
        // Arrange
        let expected = (wire, status);

        // Act
        let code = error.code();

        // Assert
        assert_eq!((code.as_str(), code.status()), expected, "{error:?}");
    }
}

#[test]
fn every_error_converts_to_a_business_value_with_no_parameters() {
    for (error, wire, _) in CASES {
        // Arrange
        let expected = ErrorCode::from_wire(wire).expect("code in the table");

        // Act
        let business = BusinessError::from(error);
        let domain = DomainError::from(error);

        // Assert
        assert_eq!(business.code, expected);
        assert_eq!(business.params, Default::default());
        assert!(matches!(domain, DomainError::Business(inner) if inner == business));
    }
}

#[test]
fn each_code_appears_once_in_the_shared_table_with_one_status() {
    for (_, wire, status) in CASES {
        // Arrange
        let matching = |code: &&ErrorCode| code.as_str() == wire;

        // Act
        let found: Vec<&ErrorCode> = ErrorCode::ALL.iter().filter(matching).collect();

        // Assert
        assert_eq!(found.len(), 1, "{wire}");
        assert_eq!(found[0].status(), status, "{wire}");
    }
}

#[test]
fn the_eight_errors_use_eight_different_codes() {
    // Arrange
    let mut wires: Vec<&str> = CASES
        .iter()
        .map(|(error, ..)| error.code().as_str())
        .collect();

    // Act
    wires.sort_unstable();
    wires.dedup();

    // Assert
    assert_eq!(wires.len(), CASES.len());
}
