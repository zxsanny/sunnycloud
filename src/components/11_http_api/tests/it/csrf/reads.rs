use std::sync::atomic::Ordering;

use sunnycloud_http_api::registry::{HttpMethod, ListenerKind, production_routes};

use super::support::{GROUPS, STATE_CHANGES, read_routes, with_cookie};
use crate::auth::support::Running;

const CROSS_SITE: [(&str, &str); 2] = [
    ("origin", "https://evil.example"),
    ("sec-fetch-site", "cross-site"),
];

#[tokio::test]
async fn gets_and_heads_with_a_cookie_and_cross_site_headers_change_no_state() {
    // Arrange
    let server = Running::start(&read_routes()).await;
    let before = STATE_CHANGES.load(Ordering::SeqCst);
    let mut answers = Vec::new();

    // Act
    for method in ["GET", "HEAD"] {
        for (_, path, token) in GROUPS {
            answers.push((
                method,
                path,
                with_cookie(server.public.addr, method, path, token, &CROSS_SITE).await,
            ));
        }
    }

    // Assert
    for (method, path, answer) in &answers {
        assert_eq!(answer.status, 200, "{method} {path}");
    }
    assert_eq!(STATE_CHANGES.load(Ordering::SeqCst), before);
    server.stop().await;
}

#[tokio::test]
async fn the_cross_site_post_on_the_same_path_is_still_refused() {
    // Arrange
    let server = Running::start(&read_routes()).await;
    let before = STATE_CHANGES.load(Ordering::SeqCst);
    let (_, path, token) = GROUPS[0];

    // Act
    let refused = with_cookie(server.public.addr, "POST", path, token, &CROSS_SITE).await;

    // Assert
    assert_eq!(refused.status, 403);
    assert_eq!(STATE_CHANGES.load(Ordering::SeqCst), before);
    server.stop().await;
}

#[test]
fn no_get_or_head_of_the_registry_is_declared_state_changing_except_the_named_callback() {
    // Arrange
    let routes = production_routes();

    // Act
    let offenders: Vec<_> = routes
        .iter()
        .map(|def| &def.spec)
        .filter(|spec| matches!(spec.method, HttpMethod::Get | HttpMethod::Head))
        .filter(|spec| spec.state_changing && spec.owner != "SignInService::complete")
        .map(|spec| (spec.method.key(), spec.path))
        .collect();

    // Assert
    assert!(offenders.is_empty(), "{offenders:?}");
}

#[tokio::test]
async fn the_reads_of_the_production_registry_answer_to_a_cross_site_cookie_request() {
    // Arrange
    let routes: Vec<_> = production_routes()
        .into_iter()
        .filter(|def| matches!(def.spec.method, HttpMethod::Get))
        .filter(|def| def.spec.listener == ListenerKind::Public)
        .collect();
    let server = Running::start(&routes).await;

    // Act
    let mut answers = Vec::new();
    for def in &routes {
        answers.push((
            def.spec.path,
            with_cookie(
                server.public.addr,
                "GET",
                def.spec.path,
                "anything",
                &CROSS_SITE,
            )
            .await,
        ));
    }

    // Assert
    for (path, answer) in &answers {
        assert_eq!(answer.status, 200, "{path}");
    }
    server.stop().await;
}
