use serde_json::Value;
use sunnycloud_http_api::registry::{NamingViolation, document, lint_path};

use super::oas_validator::validate_document;
use super::support::sample_surface;

/// Every path of the operations table in `api_v1_surface.md`.
const CONTRACT_PATHS: &[&str] = &[
    "/api/v1/server-info",
    "/api/v1/openapi.json",
    "/api/v1/auth/providers",
    "/api/v1/auth/login-attempts",
    "/api/v1/auth/providers/{provider}/callback",
    "/api/v1/auth/mobile-code-exchanges",
    "/api/v1/auth/refresh-tokens",
    "/api/v1/sessions",
    "/api/v1/sessions/{id}",
    "/api/v1/sessions/current",
    "/api/v1/identity-link-attempts",
    "/api/v1/device-grants",
    "/api/v1/device-tokens",
    "/api/v1/device-grants/{id}",
    "/api/v1/devices",
    "/api/v1/devices/{id}",
    "/api/v1/devices/me",
    "/api/v1/accounts/me",
    "/api/v1/accounts/me/quota",
    "/api/v1/admin/users",
    "/api/v1/admin/users/{id}",
    "/api/v1/admin/users/{id}/quota",
    "/api/v1/admin/health",
    "/api/v1/admin/invites",
    "/api/v1/admin/invites/{id}",
    "/api/v1/nodes/root",
    "/api/v1/nodes",
    "/api/v1/nodes/{id}",
    "/api/v1/nodes/{id}/children",
    "/api/v1/revisions/{id}",
    "/api/v1/revisions",
    "/api/v1/revisions:lookup",
    "/api/v1/uploads",
    "/api/v1/uploads/{upload_id}",
    "/api/v1/uploads/{upload_id}/blocks/{index}",
    "/api/v1/staged-uploads/{staged_key}",
    "/api/v1/commits",
    "/api/v1/commits:batch",
    "/api/v1/nodes/{node_id}/content",
    "/api/v1/revisions/{revision_id}/content",
    "/api/v1/changes",
    "/api/v1/changes/head",
    "/api/v1/shares",
    "/api/v1/shares/{id}",
    "/api/v1/public-shares/{token}",
    "/api/v1/public-shares/{token}/password-verifications",
    "/api/v1/public-shares/{token}/content",
    "/api/v1/public-shares/{token}/thumbnail",
    "/api/v1/nodes/{node_id}/thumbnail",
    "/api/v1/nodes/{node_id}/transcode",
    "/api/v1/thumbnail-states",
    "/api/v1/worker-jobs/claims",
    "/api/v1/worker-jobs/{job_id}/lease",
    "/api/v1/worker-jobs/{job_id}/input",
    "/api/v1/worker-jobs/{job_id}/outputs/{kind}",
    "/api/v1/worker-jobs/{job_id}/result",
    "/api/v1/worker-jobs/{job_id}/failure",
    "/api/v1/worker-jobs/status",
    "/internal/v1/test/clock-offset",
    "/api/v1/vaults/{id}",
];

#[test]
fn every_path_of_the_contract_table_passes_the_lint() {
    // Arrange
    let paths = CONTRACT_PATHS;

    // Act
    let failures: Vec<(&&str, Vec<NamingViolation>)> = paths
        .iter()
        .map(|path| (path, lint_path(path)))
        .filter(|(_, found)| !found.is_empty())
        .collect();

    // Assert
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn each_naming_rule_rejects_its_violation() {
    // Arrange
    let cases: Vec<(&str, NamingViolation)> = vec![
        ("/api/v1/nodes/", NamingViolation::TrailingSlash),
        ("/nodes", NamingViolation::OutsideApi),
        ("/api/v2/nodes", NamingViolation::OutsideApi),
        ("/ops/v1/nodes", NamingViolation::OutsideApi),
        (
            "/api/v1/getNodes",
            NamingViolation::NotKebabCase("getNodes".into()),
        ),
        (
            "/api/v1/Nodes",
            NamingViolation::NotKebabCase("Nodes".into()),
        ),
        (
            "/api/v1/public_shares",
            NamingViolation::NotKebabCase("public_shares".into()),
        ),
        (
            "/api/v1/public--shares",
            NamingViolation::NotKebabCase("public--shares".into()),
        ),
        (
            "/api/v1/get-nodes",
            NamingViolation::VerbForm("get-nodes".into()),
        ),
        ("/api/v1/create", NamingViolation::VerbForm("create".into())),
        (
            "/api/v1/nodes:move",
            NamingViolation::ColonForm("nodes:move".into()),
        ),
        (
            "/api/v1/commits:batches",
            NamingViolation::ColonForm("commits:batches".into()),
        ),
        (
            "/api/v1/node/{id}",
            NamingViolation::SingularCollection("node".into()),
        ),
        (
            "/api/v1/nodes/{nodeId}",
            NamingViolation::BadParameter("{nodeId}".into()),
        ),
        ("/api/v1//nodes", NamingViolation::EmptySegment),
    ];

    for (path, expected) in cases {
        // Act
        let found = lint_path(path);

        // Assert
        assert!(found.contains(&expected), "{path}: {found:?}");
    }
}

#[test]
fn only_two_colon_forms_exist_in_the_document() {
    // Arrange
    let parsed: Value =
        serde_json::from_slice(document(&sample_surface()).as_bytes()).expect("document");

    // Act
    let colon: Vec<&String> = parsed["paths"]
        .as_object()
        .expect("paths")
        .keys()
        .filter(|path| path.contains(':'))
        .collect();

    // Assert
    assert_eq!(colon, ["/api/v1/commits:batch", "/api/v1/revisions:lookup"]);
}

#[test]
fn every_document_path_is_under_api_v1_or_the_test_clock_and_is_lint_clean() {
    // Arrange
    let parsed: Value =
        serde_json::from_slice(document(&sample_surface()).as_bytes()).expect("document");

    // Act
    let paths: Vec<&String> = parsed["paths"].as_object().expect("paths").keys().collect();

    // Assert
    assert!(paths.len() >= 15);
    for path in paths {
        assert!(path.starts_with("/api/v1/"), "{path}");
        assert_eq!(lint_path(path), vec![], "{path}");
    }
}

#[test]
fn the_generated_document_passes_the_openapi_validator() {
    // Arrange
    let parsed: Value =
        serde_json::from_slice(document(&sample_surface()).as_bytes()).expect("document");

    // Act
    let errors = validate_document(&parsed);

    // Assert
    assert_eq!(errors, Vec::<String>::new());
}
