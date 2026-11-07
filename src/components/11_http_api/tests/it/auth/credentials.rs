use sunnycloud_http_api::registry::AuthClass;
use sunnycloud_http_api::{CredentialFormat, HttpServer, HttpServerError};

use super::support::{
    ADMIN_TOKEN, DEVICE_TOKEN, MEMBER_ACCOUNT, MEMBER_TOKEN, Presented, Running, class_routes,
    path_of,
};
use crate::harness::router_server::request_with;
use crate::harness::{SESSION_COOKIE, Script, default_config, new_metrics, stub_services};

fn presented(kind: &'static str, token: &str) -> Presented {
    Presented {
        kind,
        token: token.to_owned(),
    }
}

async fn get_with(server: &Running, headers: &[(&str, &str)]) -> (u16, Vec<Presented>) {
    let path = path_of(AuthClass::SessionOrDevice);
    let answer = request_with(server.public.addr, "GET", path, headers).await;
    (answer.status, server.authenticator.calls())
}

#[tokio::test]
async fn a_bearer_is_a_device_credential_only_when_it_starts_with_the_device_marker() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    let session = format!("Bearer {MEMBER_TOKEN}");
    let device = format!("Bearer {DEVICE_TOKEN}");

    // Act
    get_with(&server, &[("authorization", session.as_str())]).await;
    let (status, calls) = get_with(&server, &[("authorization", device.as_str())]).await;

    // Assert
    assert_eq!(status, 200);
    assert_eq!(
        calls,
        [
            presented("session", MEMBER_TOKEN),
            presented("device", DEVICE_TOKEN)
        ]
    );
    server.stop().await;
}

#[tokio::test]
async fn the_session_cookie_is_a_session_credential_and_a_bearer_wins_over_it() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    let cookie = format!("theme=dark; {SESSION_COOKIE}={MEMBER_TOKEN}; lang=en");
    let admin = format!("Bearer {ADMIN_TOKEN}");

    // Act
    let (cookie_status, after_cookie) = get_with(&server, &[("cookie", cookie.as_str())]).await;
    let (both_status, after_both) = get_with(
        &server,
        &[
            ("cookie", cookie.as_str()),
            ("authorization", admin.as_str()),
        ],
    )
    .await;

    // Assert
    assert_eq!(cookie_status, 200);
    assert_eq!(after_cookie, [presented("session", MEMBER_TOKEN)]);
    assert_eq!(both_status, 200);
    assert_eq!(after_both[1], presented("session", ADMIN_TOKEN));
    server.stop().await;
}

#[tokio::test]
async fn a_cookie_of_another_name_or_a_foreign_scheme_is_no_credential() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    let wrong_name = format!("session={MEMBER_TOKEN}");
    let without_prefix = format!(
        "{}={MEMBER_TOKEN}",
        SESSION_COOKIE.trim_start_matches("__Host-")
    );
    let basic = format!("Basic {MEMBER_TOKEN}");

    // Act
    let answers = [
        get_with(&server, &[("cookie", wrong_name.as_str())])
            .await
            .0,
        get_with(&server, &[("cookie", without_prefix.as_str())])
            .await
            .0,
        get_with(&server, &[("authorization", basic.as_str())])
            .await
            .0,
        get_with(&server, &[("authorization", "Bearer ")]).await.0,
    ];

    // Assert
    assert_eq!(answers, [401; 4]);
    assert!(server.authenticator.calls().is_empty());
    server.stop().await;
}

#[tokio::test]
async fn the_cookie_authenticates_to_the_account_of_its_session() {
    // Arrange
    let server = Running::start(&class_routes()).await;
    let cookie = format!("{SESSION_COOKIE}={MEMBER_TOKEN}");

    // Act
    let answer = request_with(
        server.public.addr,
        "GET",
        path_of(AuthClass::SessionOrDevice),
        &[("cookie", cookie.as_str())],
    )
    .await;

    // Assert
    assert_eq!(answer.json()["account_id"], MEMBER_ACCOUNT.to_string());
    assert_eq!(answer.json()["source"], "Cookie");
    server.stop().await;
}

#[test]
fn a_credential_format_the_documents_rule_out_is_refused_at_start() {
    // Arrange
    let formats = [
        ("session", "dev-"),
        ("__Host-", "dev-"),
        ("__Host-sid", ""),
        ("__Host-a b", "dev-"),
        ("__Host-a=b", "dev-"),
    ];

    // Act
    let outcomes: Vec<_> = formats
        .into_iter()
        .map(|(cookie, prefix)| {
            let mut config = default_config();
            config.credentials = CredentialFormat {
                session_cookie: cookie.to_owned(),
                device_token_prefix: prefix.to_owned(),
            };
            HttpServer::new(stub_services(&Script::default(), new_metrics()), config).err()
        })
        .collect();

    // Assert
    for outcome in outcomes {
        assert!(matches!(outcome, Some(HttpServerError::InvalidConfig(_))));
    }
}
