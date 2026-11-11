//! FT-N-35: the test-clock endpoints exist only in a build with feature `test-clock`.

#[cfg(feature = "test-clock")]
mod test_build {
    use serde_json::json;

    use super::super::support::{Stack, as_worker, send};

    const OFFSET: &str = "/internal/v1/test/clock-offset";
    const EIGHT_DAYS: i64 = 691_200;

    #[tokio::test]
    async fn ac5_an_advance_on_9091_is_read_back_with_the_worker_bearer_and_refused_without_it() {
        // Arrange
        let stack = Stack::start().await;
        let before = as_worker(stack.internal, "GET", OFFSET, &[], b"").await;
        let body = json!({"advance_seconds": EIGHT_DAYS}).to_string();

        // Act
        let advanced = send(
            stack.test_clock,
            "POST",
            "/test/clock",
            &[],
            body.as_bytes(),
        )
        .await;
        let read = as_worker(stack.internal, "GET", OFFSET, &[], b"").await;
        let anonymous = send(stack.internal, "GET", OFFSET, &[], b"").await;

        // Assert
        assert_eq!(before.json(), json!({"offset_seconds": 0}));
        assert_eq!(
            (advanced.status, advanced.json()),
            (200, json!({"offset_seconds": EIGHT_DAYS}))
        );
        assert_eq!(
            (read.status, read.json()),
            (200, json!({"offset_seconds": EIGHT_DAYS}))
        );
        assert_eq!(
            (anonymous.status, anonymous.code().as_str()),
            (403, "forbidden")
        );
        stack.stop().await;
    }

    #[tokio::test]
    async fn advances_add_up_and_a_negative_or_malformed_advance_leaves_the_offset_alone() {
        // Arrange
        let stack = Stack::start().await;
        let advance = |body: &'static str| {
            send(
                stack.test_clock,
                "POST",
                "/test/clock",
                &[],
                body.as_bytes(),
            )
        };

        // Act
        let first = advance(r#"{"advance_seconds":60}"#).await;
        let second = advance(r#"{"advance_seconds":40}"#).await;
        let negative = advance(r#"{"advance_seconds":-1}"#).await;
        let malformed = advance(r#"{"advance":1}"#).await;
        let read = as_worker(stack.internal, "GET", OFFSET, &[], b"").await;

        // Assert
        assert_eq!(first.json(), json!({"offset_seconds": 60}));
        assert_eq!(second.json(), json!({"offset_seconds": 100}));
        assert_eq!(
            (negative.status, negative.code().as_str()),
            (400, "invalid_request")
        );
        assert_eq!(
            (malformed.status, malformed.code().as_str()),
            (400, "invalid_request")
        );
        assert_eq!(read.json(), json!({"offset_seconds": 100}));
        stack.stop().await;
    }

    #[tokio::test]
    async fn the_clock_route_is_not_on_the_public_listener_and_the_internal_listener_has_no_post() {
        // Arrange
        let stack = Stack::start().await;
        let body = br#"{"advance_seconds":1}"#;

        // Act
        let public = send(stack.public, "POST", "/test/clock", &[], body).await;
        let internal = as_worker(stack.internal, "POST", "/test/clock", &[], body).await;
        let public_read = as_worker(stack.public, "GET", OFFSET, &[], b"").await;

        // Assert
        for answer in [&public, &internal, &public_read] {
            assert_eq!(answer.status, 404);
        }
        stack.stop().await;
    }

    #[test]
    fn the_offset_read_is_a_registry_operation_of_the_internal_listener_in_the_document() {
        // Arrange
        let document: serde_json::Value =
            serde_json::from_slice(sunnycloud_http_api::current_document().as_bytes())
                .expect("document");

        // Act
        let operation = &document["paths"][OFFSET]["get"];

        // Assert
        assert_eq!(operation["x-listener"], "internal");
        assert_eq!(operation["x-auth-class"], "Worker");
        assert!(document["paths"].get("/test/clock").is_none());
    }
}

#[cfg(not(feature = "test-clock"))]
mod release_build {
    use serde_json::json;
    use sunnycloud_http_api::registry::production_routes;

    use super::super::support::{Stack, as_worker, send};
    use crate::auth::support::ADMIN_TOKEN;

    #[test]
    fn ac6_the_route_table_and_the_document_hold_no_clock_route() {
        // Arrange
        let routes = production_routes();
        let document =
            String::from_utf8(sunnycloud_http_api::current_document().as_bytes().to_vec())
                .expect("the document is text");

        // Act
        let clock_routes: Vec<_> = routes
            .iter()
            .filter(|route| route.spec.path.contains("clock") || route.spec.path.contains("/test/"))
            .map(|route| route.spec.path)
            .collect();

        // Assert
        assert!(clock_routes.is_empty(), "{clock_routes:?}");
        assert!(!document.contains("clock"));
    }

    #[tokio::test]
    async fn ac6_every_clock_request_answers_404_on_both_listeners_with_or_without_credentials() {
        // Arrange
        let stack = Stack::start().await;
        let admin = format!("Bearer {ADMIN_TOKEN}");
        let body = json!({"advance_seconds": 691_200}).to_string();
        let unknown = send(stack.internal, "GET", "/no-such-thing", &[], b"").await;

        // Act
        let read = as_worker(
            stack.internal,
            "GET",
            "/internal/v1/test/clock-offset",
            &[],
            b"",
        )
        .await;
        let post_anonymous = send(stack.public, "POST", "/test/clock", &[], body.as_bytes()).await;
        let post_admin = send(
            stack.public,
            "POST",
            "/test/clock",
            &[("authorization", admin.as_str())],
            body.as_bytes(),
        )
        .await;

        // Assert
        for answer in [&read, &post_anonymous, &post_admin] {
            assert_eq!(answer.status, 404);
        }
        assert_eq!(read.problem(), unknown.problem());
        stack.stop().await;
    }
}
