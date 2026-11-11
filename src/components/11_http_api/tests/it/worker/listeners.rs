use serde_json::Value;
use sunnycloud_http_api::current_document;
use sunnycloud_http_api::registry::{AuthClass, ListenerKind, RouteSpec, production_routes};

use super::support::{Stack, operations, send};
use crate::auth::support::{DEVICE_TOKEN, MEMBER_TOKEN};
use crate::harness::SESSION_COOKIE;

#[test]
fn the_worker_routes_are_internal_and_nothing_internal_is_anything_else() {
    // Arrange
    let routes = production_routes();

    // Act
    let (internal, public): (Vec<&RouteSpec>, Vec<&RouteSpec>) = routes
        .iter()
        .map(|route| &route.spec)
        .partition(|spec| spec.listener == ListenerKind::Internal);

    // Assert
    let worker_ops = internal
        .iter()
        .filter(|spec| spec.path.starts_with("/api/v1/worker-jobs/"))
        .count();
    assert_eq!(worker_ops, 8, "the eight worker operations of 08");
    assert!(internal.iter().all(|spec| spec.auth == AuthClass::Worker
        && (spec.path.starts_with("/api/v1/worker-jobs/")
            || spec.path.starts_with("/internal/v1/test/"))));
    assert!(public.iter().all(|spec| spec.auth != AuthClass::Worker
        && !spec.path.starts_with("/api/v1/worker-jobs/")
        && !spec.path.starts_with("/internal/")
        && !spec.path.starts_with("/test/")));
}

#[test]
fn the_document_lists_the_worker_operations_as_internal_with_the_worker_scheme() {
    // Arrange
    let document: Value = serde_json::from_slice(current_document().as_bytes()).expect("document");

    // Act
    let mut worker = Vec::new();
    for (path, item) in document["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("path item") {
            if path.starts_with("/api/v1/worker-jobs/") {
                worker.push((method.clone(), path.clone(), operation.clone()));
            }
        }
    }

    // Assert
    assert_eq!(worker.len(), 8);
    for (method, path, operation) in worker {
        let label = format!("{method} {path}");
        assert_eq!(operation["x-listener"], "internal", "{label}");
        assert_eq!(operation["x-auth-class"], "Worker", "{label}");
        assert_eq!(
            operation["security"],
            serde_json::json!([{"bearerWorker": []}]),
            "{label}"
        );
    }
}

#[tokio::test]
async fn ac1_worker_routes_on_the_public_listener_answer_like_an_unknown_path() {
    // Arrange
    let stack = Stack::start().await;
    let bearer = super::support::worker_bearer();
    let unknown = send(stack.public, "POST", "/api/v1/no-such-thing", &[], b"").await;

    // Act
    let claims = send(stack.public, "POST", "/api/v1/worker-jobs/claims", &[], b"").await;
    let input = send(
        stack.public,
        "GET",
        &super::support::job_path("input"),
        &[],
        b"",
    )
    .await;
    let status = send(stack.public, "PUT", "/api/v1/worker-jobs/status", &[], b"").await;
    let with_secret = send(
        stack.public,
        "PUT",
        "/api/v1/worker-jobs/status",
        &[("authorization", bearer.as_str()), ("x-worker-id", "w")],
        b"{}",
    )
    .await;

    // Assert
    assert_eq!(unknown.status, 404);
    for answer in [&claims, &input, &status, &with_secret] {
        assert_eq!(answer.status, 404);
        assert_eq!(answer.problem(), unknown.problem());
    }
    assert!(stack.jobs.calls().is_empty());
    stack.stop().await;
}

#[tokio::test]
async fn ac2_every_credential_but_the_worker_secret_gets_one_403_on_each_operation() {
    // Arrange
    let stack = Stack::start().await;
    let session = format!("Bearer {MEMBER_TOKEN}");
    let device = format!("Bearer {DEVICE_TOKEN}");
    let cookie = format!("{SESSION_COOKIE}={MEMBER_TOKEN}");
    let wrong = "Bearer not-the-worker-secret".to_owned();
    let credentials: [&[(&str, &str)]; 5] = [
        &[],
        &[("authorization", session.as_str())],
        &[("cookie", cookie.as_str())],
        &[("authorization", device.as_str())],
        &[("authorization", wrong.as_str())],
    ];

    // Act
    let mut answers = Vec::new();
    for (method, path, body) in operations() {
        for headers in credentials {
            answers.push(send(stack.internal, method, &path, headers, body.as_bytes()).await);
        }
    }

    // Assert
    assert_eq!(answers.len(), 40);
    let first = answers[0].problem();
    assert_eq!(first["code"], "forbidden");
    for answer in &answers {
        assert_eq!(answer.status, 403);
        assert_eq!(answer.problem(), first);
    }
    assert!(stack.jobs.calls().is_empty(), "{:?}", stack.jobs.calls());
    assert!(stack.authenticator.calls().is_empty(), "01 is never asked");
    stack.stop().await;
}

#[tokio::test]
async fn ac4_the_internal_listener_answers_404_for_every_other_path_with_any_credential() {
    // Arrange
    let stack = Stack::start().await;
    let worker = super::support::worker_bearer();
    let session = format!("Bearer {MEMBER_TOKEN}");
    let cookie = format!("{SESSION_COOKIE}={MEMBER_TOKEN}");
    let credentials: [&[(&str, &str)]; 4] = [
        &[],
        &[("authorization", worker.as_str())],
        &[("authorization", session.as_str())],
        &[("cookie", cookie.as_str())],
    ];
    let unknown = send(stack.internal, "GET", "/no-such-thing", &[], b"").await;

    // Act
    let mut answers = Vec::new();
    for path in [
        "/api/v1/nodes/root",
        "/api/v1/server-info",
        "/api/v1/openapi.json",
        "/api/v1/worker-jobs",
        "/",
    ] {
        for headers in credentials {
            answers.push(send(stack.internal, "GET", path, headers, b"").await);
        }
    }

    // Assert
    assert_eq!(unknown.status, 404);
    for answer in &answers {
        assert_eq!(answer.status, 404);
        assert_eq!(answer.problem(), unknown.problem());
    }
    stack.stop().await;
}
