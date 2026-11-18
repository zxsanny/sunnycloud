//! AZ-3926 against the real `sunnycloud serve` binary on PostgreSQL 18. With feature `vault` (v1.2): AC-2 (`ETag`
//! and `If-Match`: 428, 412, 200, the stored header kept on refusals), AC-3 (device token 403, anonymous 401,
//! cross-origin cookie 403 `csrf_rejected`), the cross-account half of AC-4 with A's current `If-Match`, and AC-7
//! (the routes are served). Without it (v1.0, v1.1): no vault route answers (AC-7).

mod root_support;
mod serve_support;
mod vault_support;

#[cfg(feature = "vault")]
mod v12 {
    use crate::vault_support::{
        PUBLIC_URL, Running, SESSION_COOKIE, bearer, create_body, json_type, quoted, replace_body,
    };

    const H0: &[u8] = b"vault header H0 \x00\x01\xfe\xff";
    const H1: &[u8] = b"vault header H1 \x10\x11\xee\xef";

    #[tokio::test(flavor = "multi_thread")]
    async fn put_without_if_match_is_428_stale_is_412_current_is_200_with_the_sha256_etag() {
        // Arrange
        let running = Running::start().await;
        let account = running.stack.account().await;
        let (vault, created) = running.create_vault(&account, H0);
        let h0_tag = quoted(&running.sha256_hex(H0).await);
        let path = format!("/api/v1/vaults/{vault}/header");
        let body = replace_body(H1);
        let stale = quoted(&running.sha256_hex(b"an older header").await);

        // Act
        let missing = running.as_account(&account, "PUT", &path, &[json_type()], body.as_bytes());
        let after_missing = running.stored_header(vault).await;
        let mismatched = running.as_account(
            &account,
            "PUT",
            &path,
            &[json_type(), ("If-Match", stale)],
            body.as_bytes(),
        );
        let after_mismatch = running.stored_header(vault).await;
        let current = running.as_account(
            &account,
            "PUT",
            &path,
            &[json_type(), ("If-Match", h0_tag.clone())],
            body.as_bytes(),
        );
        let read = running.as_account(
            &account,
            "GET",
            &format!("/api/v1/vaults/{vault}"),
            &[],
            &[],
        );

        // Assert
        assert_eq!(created.header("etag"), Some(h0_tag.as_str()));
        assert_eq!(
            (missing.status, missing.code()),
            (428, "precondition_required".to_owned())
        );
        assert_eq!(after_missing, H0);
        assert_eq!(
            (mismatched.status, mismatched.code()),
            (412, "precondition_failed".to_owned())
        );
        assert_eq!(after_mismatch, H0);
        let h1_tag = quoted(&running.sha256_hex(H1).await);
        assert_eq!(current.status, 200, "{current:?}");
        assert_eq!(current.header("etag"), Some(h1_tag.as_str()));
        assert_eq!(running.stored_header(vault).await, H1);
        assert_eq!(read.status, 200);
        assert_eq!(read.header("etag"), Some(h1_tag.as_str()));
        assert_eq!(read.json()["header"], create_body_header(H1));
        running.finish().await;
    }

    fn create_body_header(header: &[u8]) -> serde_json::Value {
        use base64::Engine;
        serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(header))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_device_token_gets_403_anonymous_gets_401_and_a_cross_origin_cookie_put_gets_csrf_rejected()
     {
        // Arrange
        let running = Running::start().await;
        let account = running.stack.account().await;
        let (vault, created) = running.create_vault(&account, H0);
        let tag = created.header("etag").expect("an ETag").to_owned();
        let device = running.device_token(&account).await;
        let root = running.folder(&account, "Second");
        let header_path = format!("/api/v1/vaults/{vault}/header");
        let body = replace_body(H1);
        let cookie = ("Cookie", format!("{SESSION_COOKIE}={}", account.token));

        // Act
        let device_create = running.call(
            "POST",
            "/api/v1/vaults",
            &[bearer(&device), json_type()],
            create_body(root, H1).as_bytes(),
        );
        let device_replace = running.call(
            "PUT",
            &header_path,
            &[bearer(&device), json_type(), ("If-Match", tag.clone())],
            body.as_bytes(),
        );
        let anonymous: Vec<u16> = [
            ("POST", "/api/v1/vaults".to_owned()),
            ("GET", format!("/api/v1/vaults/{vault}")),
            ("PUT", header_path.clone()),
            ("POST", format!("/api/v1/vaults/{vault}/unlock-failures")),
        ]
        .iter()
        .map(|(method, path)| running.call(method, path, &[json_type()], b"{}").status)
        .collect();
        let cross_origin = running.call(
            "PUT",
            &header_path,
            &[
                cookie.clone(),
                json_type(),
                ("If-Match", tag.clone()),
                ("Origin", "https://evil.example".to_owned()),
            ],
            body.as_bytes(),
        );
        let same_origin = running.call(
            "PUT",
            &header_path,
            &[
                cookie,
                json_type(),
                ("If-Match", tag),
                ("Origin", PUBLIC_URL.to_owned()),
            ],
            body.as_bytes(),
        );

        // Assert
        assert_eq!(
            (device_create.status, device_create.code()),
            (403, "forbidden".to_owned())
        );
        assert_eq!(
            (device_replace.status, device_replace.code()),
            (403, "forbidden".to_owned())
        );
        assert_eq!(anonymous, [401, 401, 401, 401]);
        assert_eq!(
            (cross_origin.status, cross_origin.code()),
            (403, "csrf_rejected".to_owned())
        );
        assert_eq!(same_origin.status, 200, "{same_origin:?}");
        running.finish().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn account_b_gets_404_with_as_vault_id_with_and_without_as_current_if_match() {
        // Arrange
        let running = Running::start().await;
        let a = running.stack.account().await;
        let b = running.stack.account().await;
        let (vault, created) = running.create_vault(&a, H0);
        let a_tag = created.header("etag").expect("an ETag").to_owned();
        let header_path = format!("/api/v1/vaults/{vault}/header");
        let body = replace_body(H1);

        // Act
        let answers = [
            running.as_account(&b, "GET", &format!("/api/v1/vaults/{vault}"), &[], &[]),
            running.as_account(
                &b,
                "PUT",
                &header_path,
                &[json_type(), ("If-Match", a_tag)],
                body.as_bytes(),
            ),
            running.as_account(
                &b,
                "PUT",
                &header_path,
                &[json_type(), ("If-Match", quoted("0"))],
                body.as_bytes(),
            ),
            running.as_account(
                &b,
                "POST",
                &format!("/api/v1/vaults/{vault}/unlock-failures"),
                &[],
                &[],
            ),
        ];

        // Assert
        let found: Vec<(u16, String)> = answers
            .iter()
            .map(|reply| (reply.status, reply.code()))
            .collect();
        assert_eq!(found, vec![(404, "not_found".to_owned()); 4]);
        assert_eq!(running.stored_header(vault).await, H0);
        running.finish().await;
    }

    /// AC-59 for the create operation's `root_node_id`: A's vault root, A's plain folder and an unknown id must
    /// answer B alike. Known failure (AZ-3926 discovery): 20's insert hits the `root_node_id` unique key before
    /// the composite foreign key, so A's vault root answers 409 `vault_exists`; the fix belongs to 20.
    #[tokio::test(flavor = "multi_thread")]
    async fn account_b_creating_on_as_vault_root_answers_like_an_unknown_root() {
        // Arrange
        let running = Running::start().await;
        let a = running.stack.account().await;
        let b = running.stack.account().await;
        let (vault, _) = running.create_vault(&a, H0);
        let a_root = running
            .as_account(&a, "GET", &format!("/api/v1/vaults/{vault}"), &[], &[])
            .json()["root_node_id"]
            .as_str()
            .and_then(|text| uuid::Uuid::parse_str(text).ok())
            .expect("a root id");
        let a_folder = running.folder(&a, "Plain");
        let create = |root| {
            let reply = running.as_account(
                &b,
                "POST",
                "/api/v1/vaults",
                &[json_type()],
                create_body(root, H1).as_bytes(),
            );
            (reply.status, reply.code())
        };

        // Act
        let found = [
            create(a_root),
            create(a_folder),
            create(uuid::Uuid::new_v4()),
        ];

        // Assert
        assert_eq!(
            found,
            [
                (404, "not_found".to_owned()),
                (404, "not_found".to_owned()),
                (404, "not_found".to_owned())
            ]
        );
        running.finish().await;
    }
}

#[cfg(not(feature = "vault"))]
mod v10 {
    use crate::vault_support::{Running, json_type};

    #[tokio::test(flavor = "multi_thread")]
    async fn a_default_build_serves_no_vault_route() {
        // Arrange
        let running = Running::start().await;
        let account = running.stack.account().await;
        let id = uuid::Uuid::new_v4();

        // Act
        let statuses: Vec<u16> = [
            ("POST", "/api/v1/vaults".to_owned()),
            ("GET", format!("/api/v1/vaults/{id}")),
            ("PUT", format!("/api/v1/vaults/{id}/header")),
            ("POST", format!("/api/v1/vaults/{id}/unlock-failures")),
        ]
        .iter()
        .map(|(method, path)| {
            running
                .as_account(&account, method, path, &[json_type()], b"{}")
                .status
        })
        .collect();

        // Assert
        assert_eq!(statuses, [404, 404, 404, 404]);
        running.finish().await;
    }
}
