use sunnycloud_http_api::{HttpConfig, HttpServer, HttpServerError};

use crate::harness::{Script, default_config, new_metrics, stub_services};

fn build(public_url: &str) -> Result<HttpServer, HttpServerError> {
    let config = HttpConfig {
        public_url: public_url.to_owned(),
        ..default_config()
    };
    HttpServer::new(stub_services(&Script::default(), new_metrics()), config)
}

#[test]
fn an_origin_with_or_without_a_trailing_slash_or_port_builds() {
    // Arrange
    let urls = [
        "https://sunnycloud.test",
        "https://sunnycloud.test/",
        "http://localhost:8080",
    ];

    // Act
    let results = urls.map(build);

    // Assert
    assert!(results.iter().all(Result::is_ok));
}

#[test]
fn a_public_url_that_is_not_an_origin_is_refused_at_start() {
    // Arrange
    let urls = [
        "",
        "sunnycloud.test",
        "ftp://sunnycloud.test",
        "https://sunnycloud.test/app",
        "https://user@sunnycloud.test",
    ];

    // Act
    let results = urls.map(build);

    // Assert
    for (url, result) in urls.iter().zip(&results) {
        match result {
            Err(HttpServerError::InvalidConfig(reason)) => {
                assert!(reason.contains("public_url"), "{url}: {reason}");
            }
            other => panic!(
                "{url}: expected InvalidConfig, got {:?}",
                other.as_ref().err()
            ),
        }
    }
}
