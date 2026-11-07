use std::sync::Arc;

use sc_clock::SystemClock;
use sc_observability::__private::tracing;
use sc_observability::logging::Level;
use sc_observability::{Component, LogConfig, LogFormat, subscriber};
use sunnycloud_http_api::registry::AuthClass;

use super::support::{
    FAULT_TOKEN, Running, call, class_routes, path_of, unknown_device_token, without_request_id,
};
use crate::errors::Capture;
use crate::harness::router_server::request_with;
use crate::harness::{DEVICE_PREFIX, SESSION_COOKIE};

const USER_CLASSES: [AuthClass; 5] = [
    AuthClass::Session,
    AuthClass::SessionOrDevice,
    AuthClass::DeviceOnly,
    AuthClass::SessionOnly,
    AuthClass::Admin,
];

#[tokio::test]
async fn missing_revoked_expired_and_unknown_credentials_all_get_one_401_body() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    let cookie = format!("{SESSION_COOKIE}=expired-session");
    let revoked_device = format!("Bearer {DEVICE_PREFIX}revoked");
    let unknown_device = format!("Bearer {}", unknown_device_token());
    let credentials: [Vec<(&str, &str)>; 6] = [
        vec![],
        vec![("authorization", "Bearer revoked-session")],
        vec![("authorization", revoked_device.as_str())],
        vec![("cookie", cookie.as_str())],
        vec![("authorization", "Bearer never-issued")],
        vec![("authorization", unknown_device.as_str())],
    ];

    // Act
    let mut answers = Vec::new();
    for class in USER_CLASSES {
        for headers in &credentials {
            answers.push(request_with(server.public.addr, "GET", path_of(class), headers).await);
        }
    }

    // Assert
    let first = without_request_id(answers[0].json());
    assert_eq!(first["code"], "unauthenticated");
    for answer in &answers {
        assert_eq!(answer.status, 401);
        assert_eq!(without_request_id(answer.json()), first);
    }
    server.stop().await;
}

#[tokio::test]
async fn the_401_body_never_names_why_the_credential_failed() {
    // Arrange
    let server = Running::start(&class_routes()).await;

    // Act
    let answer = request_with(
        server.public.addr,
        "GET",
        path_of(AuthClass::SessionOrDevice),
        &[("authorization", "Bearer revoked-session")],
    )
    .await;

    // Assert
    let text = String::from_utf8(answer.body)
        .expect("utf-8 body")
        .to_lowercase();
    for reason in ["revoked", "expired", "unknown", "disabled", "deleting"] {
        assert!(!text.contains(reason), "the body names `{reason}`");
    }
    assert!(!text.contains("revoked-session"), "the token is echoed");
    server.stop().await;
}

#[tokio::test]
async fn an_unexpected_authenticator_failure_is_an_opaque_500_and_its_log_line_holds_no_token() {
    // Arrange
    let capture = Capture::default();
    let (logger, _stats) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::INFO,
            component: Component::Api,
        },
        Arc::new(SystemClock::new()),
        Arc::new(capture.clone()),
    );
    let _installed = tracing::subscriber::set_default(logger);
    let server = Running::start(&class_routes()).await;

    // Act
    let answer = call(
        server.public.addr,
        "GET",
        path_of(AuthClass::SessionOrDevice),
        Some(FAULT_TOKEN),
    )
    .await;

    // Assert
    assert_eq!(answer.status, 500);
    assert_eq!(answer.json()["code"], "internal");
    let text = String::from_utf8(answer.body.clone()).expect("utf-8 body");
    assert!(!text.contains("/var/lib/sunnycloud") && !text.contains("database"));
    let lines = capture.lines();
    let errors: Vec<_> = lines
        .iter()
        .filter(|line| line["level"] == "error")
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["request_id"], answer.json()["request_id"]);
    assert!(
        errors[0]["err"]
            .as_str()
            .expect("the chain")
            .contains("database unreachable")
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.to_string().contains(FAULT_TOKEN))
    );
    server.stop().await;
}
