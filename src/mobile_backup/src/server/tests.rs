use sync_core::ConnectError;

use super::{backup_error, sync_config};
use crate::error::BackupError;

#[test]
fn every_refusal_of_connect_has_a_typed_error() {
    // Arrange
    let refusals = [
        (ConnectError::InsecureScheme, BackupError::HttpsRequired),
        (ConnectError::Unreachable, BackupError::ServerUnreachable),
        (
            ConnectError::UpgradeRequired {
                message: "update".to_owned(),
                update_url: Some("https://example.test/update".to_owned()),
            },
            BackupError::UpgradeRequired {
                message: "update".to_owned(),
            },
        ),
        (
            ConnectError::InvalidConfig {
                field: "server_url",
            },
            BackupError::InvalidServerUrl,
        ),
        (
            ConnectError::InvalidConfig {
                field: "client_version",
            },
            BackupError::ServerUnreachable,
        ),
        (
            ConnectError::ProtocolMismatch { what: "block_size" },
            BackupError::ServerUnreachable,
        ),
        (ConnectError::Unauthorized, BackupError::ServerUnreachable),
        (
            ConnectError::TrustStoreUninitialized,
            BackupError::ServerUnreachable,
        ),
    ];

    // Act
    let mapped: Vec<_> = refusals
        .iter()
        .map(|(error, _)| backup_error(error.clone()))
        .collect();

    // Assert
    let expected: Vec<_> = refusals.into_iter().map(|(_, typed)| typed).collect();
    assert_eq!(mapped, expected);
}

#[test]
fn the_sync_section_of_a_check_is_the_android_client_with_the_given_address_and_version() {
    // Arrange
    let url = "https://sunnycloud.test:8443";

    // Act
    let config = sync_config(url, "1.4.2").expect("the defaults load");

    // Assert
    assert_eq!(config.server_url, url);
    assert_eq!(config.client_family, "android");
    assert_eq!(config.client_version, "1.4.2");
}
