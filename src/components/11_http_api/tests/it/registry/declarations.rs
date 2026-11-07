use serde_json::Value;
use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, Param, ParamLocation, ParamType, RouteSpec, document,
    production_routes, validate,
};

use super::support::{def, sample_surface};

const REQUIRED_EXTENSIONS: [&str; 6] = [
    "x-auth-class",
    "x-body-class",
    "x-state-changing",
    "x-owner-method",
    "x-id-params",
    "x-listener",
];

const ID: &[Param] = &[Param::id(ParamLocation::Path, "id", ParamType::Uuid)];
const ID_UNMARKED: &[Param] = &[Param::path("id", ParamType::Uuid)];
const QUERY_ID_UNMARKED: &[Param] = &[
    Param::id(ParamLocation::Path, "id", ParamType::Uuid),
    Param::query("parent_id", ParamType::Uuid),
];
const EXTRA_PATH_ID: &[Param] = &[
    Param::id(ParamLocation::Path, "id", ParamType::Uuid),
    Param::id(ParamLocation::Path, "other_id", ParamType::Uuid),
];
const PROVIDER: &[Param] = &[Param::path("provider", ParamType::String)];

fn valid() -> RouteSpec {
    RouteSpec::new(
        HttpMethod::Get,
        "/api/v1/nodes/{id}",
        "TreeReader::get_node",
        AuthClass::SessionOrDevice,
        BodyClass::None,
    )
    .summary("A test operation")
    .params(ID)
}

fn problems_of(spec: RouteSpec) -> Vec<String> {
    validate(&[def(spec)])
        .expect_err("the registry is refused")
        .0
}

fn mentions(problems: &[String], text: &str) -> bool {
    problems.iter().any(|problem| problem.contains(text))
}

#[test]
fn a_fully_declared_operation_is_accepted() {
    // Arrange
    let defs = [def(valid())];

    // Act
    let outcome = validate(&defs);

    // Assert
    assert_eq!(outcome, Ok(()));
}

#[test]
fn the_production_registry_and_the_sample_surface_validate() {
    // Arrange
    let production = production_routes();
    let sample = sample_surface();

    // Act
    let outcomes = [validate(&production), validate(&sample)];

    // Assert
    assert_eq!(outcomes, [Ok(()), Ok(())]);
}

#[test]
fn every_operation_of_the_document_carries_all_declarations() {
    // Arrange
    let defs = sample_surface();
    let parsed: Value = serde_json::from_slice(document(&defs).as_bytes()).expect("document");

    // Act
    let mut operations = 0;
    for (path, item) in parsed["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("path item") {
            operations += 1;

            // Assert
            for name in REQUIRED_EXTENSIONS {
                assert!(!operation[name].is_null(), "{method} {path} lacks {name}");
            }
            assert!(
                !operation["x-owner-method"]
                    .as_str()
                    .expect("owner")
                    .is_empty()
            );
        }
    }
    assert_eq!(operations, defs.len());
}

#[test]
fn x_id_params_name_every_declared_identifier_in_path_query_and_header() {
    // Arrange
    const PARAMS: &[Param] = &[
        Param::id(ParamLocation::Path, "id", ParamType::Uuid),
        Param::id(ParamLocation::Query, "after", ParamType::Uuid),
        Param::id(ParamLocation::Header, "x-parent-id", ParamType::Uuid),
        Param::id(ParamLocation::Body, "parent_id", ParamType::Uuid),
        Param::query("limit", ParamType::Integer),
    ];
    let spec = valid().params(PARAMS);

    // Act
    let parsed: Value =
        serde_json::from_slice(document(&[def(spec)]).as_bytes()).expect("document");

    // Assert
    let operation = &parsed["paths"]["/api/v1/nodes/{id}"]["get"];
    assert_eq!(
        operation["x-id-params"],
        serde_json::json!([
            {"name": "id", "in": "path"},
            {"name": "after", "in": "query"},
            {"name": "x-parent-id", "in": "header"},
            {"name": "parent_id", "in": "body"},
        ])
    );
    let listed: Vec<&str> = operation["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert_eq!(listed, ["id", "after", "x-parent-id", "limit"]);
}

#[test]
fn a_route_registered_without_a_declaration_fails_validation() {
    // Arrange
    let cases: Vec<(&str, RouteSpec, &str)> = vec![
        (
            "no owner",
            RouteSpec {
                owner: "",
                ..valid()
            },
            "owner port method is missing",
        ),
        (
            "no summary",
            RouteSpec {
                summary: "",
                ..valid()
            },
            "summary is missing",
        ),
        (
            "undeclared id",
            valid().params(&[]),
            "path variable id is not declared",
        ),
        (
            "id not marked",
            valid().params(ID_UNMARKED),
            "looks like an id",
        ),
        (
            "query id not marked",
            valid().params(QUERY_ID_UNMARKED),
            "looks like an id",
        ),
        (
            "declared but not in the path",
            valid().params(EXTRA_PATH_ID),
            "not in the path",
        ),
    ];

    for (case, spec, expected) in cases {
        // Act
        let problems = problems_of(spec);

        // Assert
        assert!(mentions(&problems, expected), "{case}: {problems:?}");
    }
}

#[test]
fn contradictory_declarations_are_refused() {
    // Arrange
    let cases: Vec<(&str, RouteSpec, &str)> = vec![
        (
            "worker on the public listener",
            RouteSpec {
                auth: AuthClass::Worker,
                ..valid()
            },
            "internal listener only",
        ),
        (
            "internal route with a user credential",
            RouteSpec {
                path: "/api/v1/worker-jobs/{id}",
                ..valid().internal()
            },
            "Worker routes only",
        ),
        (
            "internal route outside the worker paths",
            RouteSpec {
                auth: AuthClass::Worker,
                ..valid().internal()
            },
            "not a worker or test-clock path",
        ),
        (
            "worker path on the public listener",
            RouteSpec {
                path: "/api/v1/worker-jobs/{id}",
                ..valid()
            },
            "belongs to the internal listener",
        ),
        (
            "GET with a body class",
            RouteSpec {
                body: BodyClass::Small,
                ..valid()
            },
            "has no body",
        ),
        (
            "GET that mutates",
            valid().mutating(),
            "declared state-changing",
        ),
        (
            "POST without a body class",
            RouteSpec {
                method: HttpMethod::Post,
                state_changing: true,
                ..valid()
            },
            "needs GET, HEAD or DELETE",
        ),
        (
            "body schema on a bodiless operation",
            valid().request(sunnycloud_http_api::registry::SchemaRef {
                name: "X",
                build: || serde_json::json!({}),
            }),
            "request schema",
        ),
        (
            "unknown success status",
            valid().success(302),
            "success status 302",
        ),
        (
            "vault path with another owner",
            RouteSpec {
                path: "/api/v1/vaults/{id}",
                ..valid()
            },
            "reserved for VaultService",
        ),
    ];

    for (case, spec, expected) in cases {
        // Act
        let problems = problems_of(spec);

        // Assert
        assert!(mentions(&problems, expected), "{case}: {problems:?}");
    }
}

#[test]
fn the_named_get_mutation_is_the_only_exemption() {
    // Arrange
    let callback = RouteSpec {
        owner: "SignInService::complete",
        path: "/api/v1/auth/providers/{provider}/callback",
        ..valid()
    }
    .params(PROVIDER)
    .mutating();
    let other = RouteSpec {
        owner: "SignInService::begin",
        ..callback
    };

    // Act
    let outcomes = (validate(&[def(callback)]), validate(&[def(other)]));

    // Assert
    assert_eq!(outcomes.0, Ok(()));
    assert!(mentions(
        &outcomes.1.expect_err("refused").0,
        "declared state-changing"
    ));
}

#[test]
fn the_same_operation_twice_and_conflicting_schemas_are_refused() {
    // Arrange
    let one = sunnycloud_http_api::registry::SchemaRef {
        name: "Shape",
        build: || serde_json::json!({"type": "object"}),
    };
    let other = sunnycloud_http_api::registry::SchemaRef {
        name: "Shape",
        build: || serde_json::json!({"type": "string"}),
    };
    let post = |path: &'static str, schema| {
        RouteSpec::new(
            HttpMethod::Post,
            path,
            "TreeService::create_folder",
            AuthClass::SessionOnly,
            BodyClass::Small,
        )
        .summary("A test operation")
        .request(schema)
    };

    // Act
    let twice = validate(&[def(valid()), def(valid())]);
    let conflict = validate(&[
        def(post("/api/v1/nodes", one)),
        def(post("/api/v1/shares", other)),
    ]);

    // Assert
    assert!(mentions(&twice.expect_err("refused").0, "declared twice"));
    assert!(mentions(
        &conflict.expect_err("refused").0,
        "defined twice differently"
    ));
}

#[test]
fn security_follows_the_auth_class_and_the_worker_scheme_marks_internal_operations() {
    // Arrange
    let parsed: Value =
        serde_json::from_slice(document(&sample_surface()).as_bytes()).expect("document");
    let security = |path: &str, method: &str| parsed["paths"][path][method]["security"].clone();

    // Act
    let (public, device_or_session, worker, admin) = (
        security("/api/v1/server-info", "get"),
        security("/api/v1/nodes/{id}", "get"),
        security("/api/v1/worker-jobs/claims", "post"),
        security("/api/v1/admin/invites/{id}", "delete"),
    );

    // Assert
    assert_eq!(public, serde_json::json!([]));
    assert_eq!(
        device_or_session,
        serde_json::json!([{"cookieSession": []}, {"bearerSession": []}, {"bearerDevice": []}])
    );
    assert_eq!(worker, serde_json::json!([{"bearerWorker": []}]));
    assert_eq!(
        admin,
        serde_json::json!([{"cookieSession": []}, {"bearerSession": []}])
    );
    let schemes: Vec<&String> = parsed["components"]["securitySchemes"]
        .as_object()
        .expect("schemes")
        .keys()
        .collect();
    assert_eq!(
        schemes,
        [
            "bearerDevice",
            "bearerSession",
            "bearerWorker",
            "cookieSession"
        ]
    );
}

#[test]
fn error_responses_come_from_the_registry_declarations() {
    // Arrange
    let spec = valid().errors(&[
        sc_error::ErrorCode::NotFound,
        sc_error::ErrorCode::RateLimited,
    ]);
    let parsed: Value =
        serde_json::from_slice(document(&[def(spec)]).as_bytes()).expect("document");

    // Act
    let responses = &parsed["paths"]["/api/v1/nodes/{id}"]["get"]["responses"];

    // Assert
    assert_eq!(
        responses["404"]["x-error-codes"],
        serde_json::json!(["not_found"])
    );
    assert_eq!(
        responses["429"]["x-error-codes"],
        serde_json::json!(["rate_limited"])
    );
    assert_eq!(
        responses["401"]["x-error-codes"],
        serde_json::json!(["unauthenticated"])
    );
    assert_eq!(
        responses["403"]["x-error-codes"],
        serde_json::json!(["forbidden"])
    );
    assert_eq!(
        responses["426"]["x-error-codes"],
        serde_json::json!(["upgrade_required"])
    );
    assert_eq!(
        responses["500"]["x-error-codes"],
        serde_json::json!(["internal"])
    );
    assert_eq!(
        responses["404"]["content"]["application/problem+json"]["schema"]["$ref"],
        "#/components/schemas/Problem"
    );
    assert!(responses.get("200").is_some());
}
