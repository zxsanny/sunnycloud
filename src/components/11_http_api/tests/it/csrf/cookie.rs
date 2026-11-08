use super::support::{GROUPS, MUTATIONS, handler_ran, mutation_routes, with_cookie};
use crate::auth::support::Running;

/// Header sets that prove no same origin, for AC-1 and AC-4.
const REFUSED: [&[(&str, &str)]; 17] = [
    &[("origin", "https://evil.example")],
    &[("origin", "https://app.sunnycloud.test")],
    &[("origin", "null")],
    &[("origin", "http://sunnycloud.test")],
    &[("origin", "https://sunnycloud.test:8443")],
    &[("origin", "https://sunnycloud.test.evil.example")],
    &[("origin", "https://sunnycloud.test/")],
    &[("origin", "")],
    &[("sec-fetch-site", "cross-site")],
    &[("sec-fetch-site", "same-site")],
    &[("sec-fetch-site", "none")],
    &[("sec-fetch-site", "")],
    &[("sec-fetch-site", "Same-Origin")],
    &[
        ("sec-fetch-site", "cross-site"),
        ("origin", "https://evil.example"),
    ],
    &[("sec-fetch-site", "same-site"), ("origin", "null")],
    &[
        ("sec-fetch-site", "none"),
        ("origin", "https://sunnycloud.test.evil.example"),
    ],
    &[],
];

#[tokio::test]
async fn a_cookie_mutation_without_a_same_origin_proof_is_403_csrf_rejected() {
    // Arrange
    let server = Running::start(&mutation_routes()).await;
    let mut answers = Vec::new();

    // Act
    for (_, method) in MUTATIONS {
        for (_, path, token) in GROUPS {
            for headers in REFUSED {
                let answer = with_cookie(server.public.addr, method, path, token, headers).await;
                answers.push((method, path, headers, answer));
            }
        }
    }

    // Assert
    for (method, path, headers, answer) in &answers {
        let label = format!("{method} {path} {headers:?}");
        assert_eq!(answer.status, 403, "{label}");
        assert_eq!(answer.json()["code"], "csrf_rejected", "{label}");
        assert!(!handler_ran(answer), "{label}");
    }
    server.stop().await;
}

#[tokio::test]
async fn a_cookie_mutation_with_a_same_origin_proof_reaches_the_handler() {
    // Arrange
    let server = Running::start(&mutation_routes()).await;
    let proofs: [&[(&str, &str)]; 4] = [
        &[("origin", "https://sunnycloud.test")],
        &[("sec-fetch-site", "same-origin")],
        &[("origin", "https://sunnycloud.test:443")],
        &[
            ("sec-fetch-site", "same-origin"),
            ("origin", "https://evil.example"),
        ],
    ];
    let mut answers = Vec::new();

    // Act
    for (_, method) in MUTATIONS {
        for (_, path, token) in GROUPS {
            for headers in proofs {
                let answer = with_cookie(server.public.addr, method, path, token, headers).await;
                answers.push((method, path, headers, answer));
            }
        }
    }

    // Assert
    for (method, path, headers, answer) in &answers {
        assert!(handler_ran(answer), "{method} {path} {headers:?}");
    }
    server.stop().await;
}

#[tokio::test]
async fn auth_refuses_before_the_origin_is_looked_at() {
    // Arrange
    let server = Running::start(&mutation_routes()).await;
    let (_, path, _) = GROUPS[0];

    // Act
    let unknown = with_cookie(
        server.public.addr,
        "POST",
        path,
        "no-such-session",
        &[("origin", "https://evil.example")],
    )
    .await;

    // Assert
    assert_eq!(unknown.status, 401);
    assert_eq!(unknown.json()["code"], "unauthenticated");
    server.stop().await;
}

#[tokio::test]
async fn the_origin_comes_from_public_url_and_not_from_the_host_header() {
    // Arrange
    let server = Running::start(&mutation_routes()).await;
    let (_, path, token) = GROUPS[0];

    // Act
    let forged_host = with_cookie(
        server.public.addr,
        "POST",
        path,
        token,
        &[("origin", "https://evil.example"), ("host", "evil.example")],
    )
    .await;

    // Assert
    assert_eq!(forged_host.status, 403);
    assert_eq!(forged_host.json()["code"], "csrf_rejected");
    server.stop().await;
}
