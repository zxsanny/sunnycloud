use serde_json::Value;
use sunnycloud_http_api::registry::{AuthClass, HttpMethod};
use uuid::Uuid;

use super::support::{
    ADMIN_ACCOUNT, ADMIN_SESSION, ADMIN_TOKEN, DEVICE_ACCOUNT, DEVICE_ID, DEVICE_TOKEN,
    MEMBER_ACCOUNT, MEMBER_SESSION, MEMBER_TOKEN, Running, call, class_routes, path_of, route,
};

#[tokio::test]
async fn every_class_answers_as_the_contract_states_for_each_kind_of_caller() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    // none, member session, admin session, device token
    let callers = [
        None,
        Some(MEMBER_TOKEN),
        Some(ADMIN_TOKEN),
        Some(DEVICE_TOKEN),
    ];
    let expected: [(AuthClass, [u16; 4]); 7] = [
        (AuthClass::Public, [200, 200, 200, 200]),
        (AuthClass::ShareGrant, [200, 200, 200, 200]),
        (AuthClass::Session, [401, 200, 200, 403]),
        (AuthClass::SessionOrDevice, [401, 200, 200, 200]),
        (AuthClass::DeviceOnly, [401, 403, 403, 200]),
        (AuthClass::SessionOnly, [401, 200, 200, 403]),
        (AuthClass::Admin, [401, 403, 200, 403]),
    ];

    // Act
    let mut answered = Vec::new();
    for (class, _) in &expected {
        let mut row = [0u16; 4];
        for (slot, caller) in row.iter_mut().zip(callers) {
            *slot = call(server.public.addr, "GET", path_of(*class), caller)
                .await
                .status;
        }
        answered.push((*class, row));
    }

    // Assert
    assert_eq!(answered, expected);
    server.stop().await;
}

#[tokio::test]
async fn a_wrong_kind_is_403_on_each_operation_the_contract_lists() {
    // Arrange
    let defs = [
        route(
            HttpMethod::Patch,
            "/api/v1/device-grants/{id}",
            AuthClass::SessionOnly,
        ),
        route(HttpMethod::Post, "/api/v1/shares", AuthClass::SessionOnly),
        route(
            HttpMethod::Delete,
            "/api/v1/accounts/me",
            AuthClass::SessionOnly,
        ),
        route(
            HttpMethod::Post,
            "/api/v1/identity-link-attempts",
            AuthClass::SessionOnly,
        ),
        route(
            HttpMethod::Delete,
            "/api/v1/devices/{id}",
            AuthClass::SessionOnly,
        ),
        route(
            HttpMethod::Delete,
            "/api/v1/devices/me",
            AuthClass::DeviceOnly,
        ),
        route(HttpMethod::Get, "/api/v1/admin/users", AuthClass::Admin),
    ];
    let server = Running::start(&defs).await;
    let addr = server.public.addr;
    let id = Uuid::from_u128(7);

    // Act
    let mut refused = Vec::new();
    for (method, path) in [
        ("PATCH", format!("/api/v1/device-grants/{id}")),
        ("POST", "/api/v1/shares".to_owned()),
        ("DELETE", "/api/v1/accounts/me".to_owned()),
        ("POST", "/api/v1/identity-link-attempts".to_owned()),
        ("DELETE", format!("/api/v1/devices/{id}")),
    ] {
        refused.push(call(addr, method, &path, Some(DEVICE_TOKEN)).await);
    }
    refused.push(call(addr, "DELETE", "/api/v1/devices/me", Some(MEMBER_TOKEN)).await);
    refused.push(call(addr, "GET", "/api/v1/admin/users", Some(MEMBER_TOKEN)).await);
    let admin = call(addr, "GET", "/api/v1/admin/users", Some(ADMIN_TOKEN)).await;
    let device_self = call(addr, "DELETE", "/api/v1/devices/me", Some(DEVICE_TOKEN)).await;

    // Assert
    for answer in &refused {
        assert_eq!(answer.status, 403);
        assert_eq!(answer.json()["code"], "forbidden");
    }
    assert_eq!(admin.status, 200);
    assert_eq!(device_self.status, 200);
    server.stop().await;
}

fn id(value: &Value, key: &str) -> Uuid {
    value[key]
        .as_str()
        .and_then(|text| text.parse().ok())
        .expect("a uuid field")
}

#[tokio::test]
async fn the_handler_gets_the_principal_of_the_credential_and_retained_ok_only_for_the_admin() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    let cases = [
        (
            MEMBER_TOKEN,
            MEMBER_ACCOUNT,
            "Session",
            MEMBER_SESSION,
            false,
        ),
        (DEVICE_TOKEN, DEVICE_ACCOUNT, "Device", DEVICE_ID, false),
        (ADMIN_TOKEN, ADMIN_ACCOUNT, "Session", ADMIN_SESSION, true),
    ];
    let path = path_of(AuthClass::SessionOrDevice);

    // Act
    let mut seen = Vec::new();
    for (token, ..) in cases {
        seen.push(
            call(server.public.addr, "GET", path, Some(token))
                .await
                .json(),
        );
    }

    // Assert
    for (body, (_, account, kind, session_or_device, retained_ok)) in seen.iter().zip(cases) {
        assert_eq!(id(body, "account_id"), account);
        assert_eq!(body["kind"], kind);
        assert_eq!(id(body, "session_or_device_id"), session_or_device);
        assert_eq!(body["retained_ok"], retained_ok);
    }
    server.stop().await;
}

#[tokio::test]
async fn a_public_route_reaches_its_handler_for_every_caller_without_asking_01() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    let path = path_of(AuthClass::Public);

    // Act
    let mut answers = Vec::new();
    for token in [
        None,
        Some(MEMBER_TOKEN),
        Some(DEVICE_TOKEN),
        Some("revoked-session"),
    ] {
        answers.push(call(server.public.addr, "GET", path, token).await);
    }

    // Assert
    for answer in &answers {
        assert_eq!(answer.status, 200);
        assert_eq!(answer.json()["anonymous"], true);
    }
    assert!(server.authenticator.calls().is_empty());
    assert_eq!(server.authenticator.required(), 0);
    server.stop().await;
}

#[tokio::test]
async fn a_wrong_method_on_a_protected_path_is_405_not_a_credential_answer() {
    // Arrange
    let server = Running::start(&class_routes()).await;

    // Act
    let answer = call(
        server.public.addr,
        "DELETE",
        path_of(AuthClass::Admin),
        None,
    )
    .await;

    // Assert
    assert_eq!(answer.status, 405);
    assert!(server.authenticator.calls().is_empty());
    server.stop().await;
}

#[tokio::test]
async fn a_request_costs_one_authenticate_and_one_require() {
    // Arrange
    let server = Running::start(&class_routes()).await;

    // Act
    call(
        server.public.addr,
        "GET",
        path_of(AuthClass::Admin),
        Some(ADMIN_TOKEN),
    )
    .await;

    // Assert
    assert_eq!(server.authenticator.calls().len(), 1);
    assert_eq!(server.authenticator.required(), 1);
    server.stop().await;
}
