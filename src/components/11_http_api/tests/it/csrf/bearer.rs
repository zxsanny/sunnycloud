use super::support::{GROUPS, MUTATIONS, PUBLIC_PATH, handler_ran, mutation_routes, with_cookie};
use crate::auth::support::{DEVICE_TOKEN, Running, call};
use crate::harness::router_server::request_with;

#[tokio::test]
async fn bearer_mutations_pass_with_a_foreign_origin_and_with_none() {
    // Arrange
    let server = Running::start(&mutation_routes()).await;
    let mut answers = Vec::new();

    // Act
    for (_, method) in MUTATIONS {
        for (_, path, token) in GROUPS {
            answers.push((
                method,
                path,
                call(server.public.addr, method, path, Some(token)).await,
            ));
            let bearer = format!("Bearer {token}");
            let foreign = request_with(
                server.public.addr,
                method,
                path,
                &[
                    ("authorization", bearer.as_str()),
                    ("origin", "https://evil.example"),
                    ("sec-fetch-site", "cross-site"),
                ],
            )
            .await;
            answers.push((method, path, foreign));
        }
    }

    // Assert
    for (method, path, answer) in &answers {
        assert!(handler_ran(answer), "{method} {path}: {}", answer.status);
    }
    server.stop().await;
}

#[tokio::test]
async fn a_device_bearer_passes_the_layer_on_a_route_that_accepts_it() {
    // Arrange
    let server = Running::start(&mutation_routes()).await;
    let (_, path, _) = GROUPS[1];
    let mut answers = Vec::new();

    // Act
    for (_, method) in MUTATIONS {
        answers.push(call(server.public.addr, method, path, Some(DEVICE_TOKEN)).await);
    }

    // Assert
    assert!(answers.iter().all(handler_ran));
    server.stop().await;
}

#[tokio::test]
async fn a_bearer_wins_over_a_cookie_so_the_request_is_exempt() {
    // Arrange
    let server = Running::start(&mutation_routes()).await;
    let (_, path, token) = GROUPS[0];
    let bearer = format!("Bearer {token}");

    // Act
    let answer = with_cookie(
        server.public.addr,
        "POST",
        path,
        token,
        &[
            ("authorization", bearer.as_str()),
            ("origin", "https://evil.example"),
        ],
    )
    .await;

    // Assert
    assert!(handler_ran(&answer));
    server.stop().await;
}

#[tokio::test]
async fn a_public_mutation_has_no_cookie_credential_to_protect() {
    // Arrange
    let server = Running::start(&mutation_routes()).await;
    let mut answers = Vec::new();

    // Act
    for (_, method) in MUTATIONS {
        answers.push(
            with_cookie(
                server.public.addr,
                method,
                PUBLIC_PATH,
                "anything",
                &[("origin", "https://evil.example")],
            )
            .await,
        );
    }

    // Assert
    assert!(answers.iter().all(handler_ran));
    server.stop().await;
}
