use sunnycloud_http_api::{HttpConfig, HttpServer, HttpServerError};

use super::support::gated_config;
use crate::harness::{Script, new_metrics, stub_services};

fn build(config: HttpConfig) -> Result<HttpServer, HttpServerError> {
    HttpServer::new(stub_services(&Script::default(), new_metrics()), config)
}

fn invalid_reason(config: HttpConfig) -> String {
    match build(config) {
        Err(HttpServerError::InvalidConfig(reason)) => reason,
        Err(other) => panic!("expected InvalidConfig, got {other}"),
        Ok(_) => panic!("the server built from an unusable gate configuration"),
    }
}

#[test]
fn a_usable_gate_configuration_builds() {
    // Arrange
    let config = gated_config("web=1.0.0,android=1.5.0");

    // Act
    let server = build(config);

    // Assert
    assert!(server.is_ok());
}

#[test]
fn a_minimum_that_is_not_a_semver_or_a_family_list_is_refused_at_start() {
    // Arrange
    let config = HttpConfig {
        min_client_version: Some("1.2".to_owned()),
        ..gated_config("1.2.0")
    };

    // Act
    let reason = invalid_reason(config);

    // Assert
    assert!(reason.contains("min_client_version"), "{reason}");
}

#[test]
fn a_minimum_without_an_update_url_is_refused_at_start() {
    // Arrange
    let config = HttpConfig {
        update_url: None,
        ..gated_config("1.2.0")
    };

    // Act
    let reason = invalid_reason(config);

    // Assert
    assert!(reason.contains("update_url"), "{reason}");
}

#[test]
fn a_non_http_update_url_is_refused_at_start() {
    // Arrange
    let config = HttpConfig {
        update_url: Some("javascript:alert(1)".to_owned()),
        ..gated_config("1.2.0")
    };

    // Act
    let reason = invalid_reason(config);

    // Assert
    assert!(reason.contains("update_url"), "{reason}");
}

#[test]
fn server_info_advertising_another_minimum_than_the_gate_is_refused_at_start() {
    // Arrange
    let config = HttpConfig {
        min_client_version: Some("1.3.0".to_owned()),
        ..gated_config("1.2.0")
    };

    // Act
    let reason = invalid_reason(config);

    // Assert
    assert!(reason.contains("server_info"), "{reason}");
}
