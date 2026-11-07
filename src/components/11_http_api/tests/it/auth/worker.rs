use sunnycloud_http_api::registry::AuthClass;

use super::support::{
    ADMIN_TOKEN, DEVICE_TOKEN, MEMBER_TOKEN, Running, WORKER_SECRET, class_routes, path_of,
    without_request_id,
};
use crate::harness::SESSION_COOKIE;
use crate::harness::router_server::request_with;

#[tokio::test]
async fn every_credential_but_the_worker_secret_gets_one_403_and_01_is_never_asked() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    let path = path_of(AuthClass::Worker);
    let addr = server.internal.addr;
    let session = format!("Bearer {MEMBER_TOKEN}");
    let admin = format!("Bearer {ADMIN_TOKEN}");
    let device = format!("Bearer {DEVICE_TOKEN}");
    let cookie = format!("{SESSION_COOKIE}={MEMBER_TOKEN}");
    let wrong = "Bearer not-the-secret".to_owned();
    let worker = format!("Bearer {WORKER_SECRET}");
    let refused: [&[(&str, &str)]; 6] = [
        &[],
        &[("authorization", session.as_str())],
        &[("authorization", admin.as_str())],
        &[("cookie", cookie.as_str())],
        &[("authorization", device.as_str())],
        &[("authorization", wrong.as_str())],
    ];

    // Act
    let mut answers = Vec::new();
    for headers in refused {
        answers.push(request_with(addr, "GET", path, headers).await);
    }
    let accepted = request_with(addr, "GET", path, &[("authorization", worker.as_str())]).await;

    // Assert
    let first = without_request_id(answers[0].json());
    assert_eq!(first["code"], "forbidden");
    for answer in &answers {
        assert_eq!(answer.status, 403);
        assert_eq!(without_request_id(answer.json()), first);
    }
    assert_eq!(accepted.status, 200);
    assert_eq!(accepted.json()["worker"], true);
    assert!(server.authenticator.calls().is_empty());
    assert_eq!(server.authenticator.required(), 0);
    server.stop().await;
}

#[tokio::test]
async fn the_worker_port_gets_only_the_authorization_bearer_and_never_a_cookie() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    let path = path_of(AuthClass::Worker);
    let cookie = format!("{SESSION_COOKIE}={MEMBER_TOKEN}");
    let bearer = format!("Bearer {WORKER_SECRET}");

    // Act
    request_with(server.internal.addr, "GET", path, &[]).await;
    request_with(
        server.internal.addr,
        "GET",
        path,
        &[("cookie", cookie.as_str())],
    )
    .await;
    request_with(
        server.internal.addr,
        "GET",
        path,
        &[
            ("authorization", bearer.as_str()),
            ("cookie", cookie.as_str()),
        ],
    )
    .await;

    // Assert
    assert_eq!(server.worker_auth.calls(), ["", "", WORKER_SECRET]);
    server.stop().await;
}

#[tokio::test]
async fn a_worker_bearer_on_a_user_route_goes_to_01_only() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    let bearer = format!("Bearer {WORKER_SECRET}");

    // Act
    let answer = request_with(
        server.public.addr,
        "GET",
        path_of(AuthClass::SessionOrDevice),
        &[("authorization", bearer.as_str())],
    )
    .await;

    // Assert
    assert_eq!(answer.status, 401);
    assert!(server.worker_auth.calls().is_empty());
    assert_eq!(server.authenticator.calls().len(), 1);
    server.stop().await;
}
