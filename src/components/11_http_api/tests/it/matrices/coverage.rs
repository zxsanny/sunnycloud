//! The generators in CI mode (AC-6): an operation or ID parameter with no case fails both, the matrix size is the
//! registry's, every `x-id-params` entry is found where it sits, and the one runner reaches a stack by base URL.

use std::collections::HashMap;

use axum::http::StatusCode;
use axum::routing::{MethodFilter, MethodRouter, on};
use serde_json::{Value, json};
use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, Param, ParamLocation, ParamType, RouteContext, RouteDef,
    RouteSpec, SchemaRef, production_routes,
};

use super::document::Document;
use super::plans::{csrf_plan, idor_plan};
use super::run::csrf_refusals;
use super::samples::has_case_data;
use super::target::{RemoteConfig, Target, kind_of, remote_config, remote_from_env};
use super::world::PER_ACCOUNT_FAKES;

fn no_content(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, || async { StatusCode::NO_CONTENT })
}

const PLANTED_PATH: &str = "/api/v1/planted-items/{planted_id}";
const PLANTED_PARAMS: &[Param] = &[Param::id(
    ParamLocation::Path,
    "planted_id",
    ParamType::Uuid,
)];

fn planted() -> RouteDef {
    let spec = RouteSpec::new(
        HttpMethod::Post,
        PLANTED_PATH,
        "PlantedService::mutate",
        AuthClass::SessionOrDevice,
        BodyClass::Small,
    )
    .params(PLANTED_PARAMS)
    .summary("A route registered with no case data");
    RouteDef::new(spec, no_content)
}

fn per_account(owner: &str) -> bool {
    PER_ACCOUNT_FAKES.contains(&owner)
}

#[test]
fn a_planted_route_without_case_data_fails_both_generators_naming_it() {
    // Arrange
    let mut defs = production_routes();
    defs.push(planted());
    let doc = Document::of(&defs);

    // Act
    let csrf = csrf_plan(&doc, &has_case_data).err();
    let idor = idor_plan(&doc, &per_account).err();

    // Assert
    assert_eq!(
        csrf,
        Some(vec![format!(
            "POST {PLANTED_PATH} has no case: no case data for PlantedService::mutate, so its passing \
             controls cannot run"
        )])
    );
    assert_eq!(
        idor,
        Some(vec![format!(
            "POST {PLANTED_PATH}: ID parameter planted_id (path) has no case: no per-account fake for \
             PlantedService::mutate"
        )])
    );
}

#[test]
fn the_release_registry_has_a_case_for_every_operation_and_every_id_parameter() {
    // Arrange
    let doc = Document::production();
    let pairs: usize = production_routes()
        .iter()
        .map(|def| def.spec.params.iter().filter(|param| param.id).count())
        .sum();

    // Act
    let csrf = csrf_plan(&doc, &has_case_data);
    let idor = idor_plan(&doc, &per_account);

    // Assert
    assert!(csrf.is_ok(), "{:?}", csrf.err());
    let cases = idor.expect("no IDOR gaps");
    println!(
        "IDOR matrix: {} (operation, ID parameter) pairs",
        cases.len()
    );
    assert_eq!(cases.len(), pairs);
}

#[test]
fn a_state_changing_get_without_a_named_exemption_fails_the_csrf_generator() {
    // Arrange
    let spec = RouteSpec::new(
        HttpMethod::Get,
        "/api/v1/planted-reads",
        "PlantedService::read_and_write",
        AuthClass::Session,
        BodyClass::None,
    )
    .mutating()
    .summary("A GET that writes");
    let mut defs = production_routes();
    defs.push(RouteDef::new(spec, no_content));

    // Act
    let problems = csrf_plan(&Document::of(&defs), &has_case_data).err();

    // Assert
    assert_eq!(
        problems,
        Some(vec![
            "GET /api/v1/planted-reads (PlantedService::read_and_write) is a state-changing read with \
             no named exemption"
                .to_owned()
        ])
    );
}

fn fixture_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["entries"],
        "properties": {
            "folder_id": {"type": "string", "format": "uuid"},
            "entries": {"type": "array", "items": {
                "type": "object",
                "required": ["item_id"],
                "properties": {"item_id": {"type": "string", "format": "uuid"}}
            }}
        }
    })
}

const FIXTURE_PARAMS: &[Param] = &[
    Param::id(ParamLocation::Path, "thing_id", ParamType::Uuid),
    Param::id(ParamLocation::Query, "other_id", ParamType::Uuid),
    Param::id(ParamLocation::Header, "x-thing-id", ParamType::Uuid),
    Param::id(ParamLocation::Body, "folder_id", ParamType::Uuid),
    Param::id(ParamLocation::Body, "item_id", ParamType::Uuid),
    Param::id(ParamLocation::Body, "ghost_id", ParamType::Uuid),
];

#[test]
fn id_parameters_are_found_in_path_query_header_and_body_and_a_missing_one_is_named() {
    // Arrange
    let spec = RouteSpec::new(
        HttpMethod::Post,
        "/api/v1/things/{thing_id}",
        "TreeService::create_folder",
        AuthClass::SessionOrDevice,
        BodyClass::Small,
    )
    .params(FIXTURE_PARAMS)
    .request(SchemaRef {
        name: "ThingRequest",
        build: fixture_schema,
    })
    .summary("A fixture operation");
    let doc = Document::of(&[RouteDef::new(spec, no_content)]);

    // Act
    let problems = idor_plan(&doc, &per_account).expect_err("ghost_id is nowhere");
    let mut found = Vec::new();
    for param in &doc.operations[0].id_params {
        let built = super::synth::build(&doc, &doc.operations[0], Some(param), &mut |p| {
            format!("id-of-{}", p.name)
        })
        .expect("the fixture builds");
        let text = format!("{} {:?} {:?}", built.target, built.headers, built.payload);
        found.push((
            param.name.clone(),
            text.contains(&format!("id-of-{}", param.name)),
        ));
    }

    // Assert
    assert_eq!(
        problems,
        [
            "POST /api/v1/things/{thing_id}: ID parameter ghost_id (body) is not in its request schema"
        ]
    );
    assert_eq!(
        found,
        [
            ("thing_id".to_owned(), true),
            ("other_id".to_owned(), true),
            ("x-thing-id".to_owned(), true),
            ("folder_id".to_owned(), true),
            ("item_id".to_owned(), true),
            ("ghost_id".to_owned(), false),
        ]
    );
}

#[test]
fn a_remote_stack_is_read_from_its_variables_and_refuses_https() {
    // Arrange
    let vars: HashMap<&str, &str> = HashMap::from([
        ("SUNNYCLOUD_MATRIX_PUBLIC_URL", "https://cloud.example"),
        ("SUNNYCLOUD_MATRIX_COOKIE", "__Host-session"),
        ("SUNNYCLOUD_MATRIX_MEMBER_TOKEN", "b-session"),
        ("SUNNYCLOUD_MATRIX_ADMIN_TOKEN", "admin-session"),
        (
            "SUNNYCLOUD_MATRIX_IDS",
            r#"{"a": {"nodes": "a-node"}, "b": {"nodes": "b-node"}}"#,
        ),
    ]);
    let var = |name: &str| vars.get(name).map(|value| (*value).to_owned());

    // Act
    let config = remote_config("http://127.0.0.1:8080/", &var).expect("a plain base URL");
    let tls = remote_config("https://127.0.0.1:8443", &var).err();

    // Assert
    assert_eq!(config.public.to_string(), "127.0.0.1:8080");
    assert_eq!(config.ids_a["nodes"], "a-node");
    assert_eq!(config.ids_b["nodes"], "b-node");
    assert_eq!(config.credentials.member, "b-session");
    assert!(tls.is_some_and(|error| error.contains("only http://")));
}

/// The runner's real-stack path, aimed at a Mode S server by address: ids come from the kind table, nothing is
/// scripted, and the refusals still hold on N of N.
#[tokio::test]
async fn the_runner_reaches_a_stack_by_base_url() {
    // Arrange
    let stub = Target::stub().await;
    let doc = Document::production();
    let mut ids = HashMap::new();
    for operation in &doc.operations {
        for param in &operation.id_params {
            ids.insert(kind_of(operation, param), stub.random_id());
        }
    }
    let remote = Target::remote(RemoteConfig {
        public: stub.public,
        internal: stub.internal,
        host: "sunnycloud.test".to_owned(),
        public_url: stub.public_url.clone(),
        credentials: stub.credentials.clone(),
        ids_a: ids.clone(),
        ids_b: ids,
    });
    let plan = csrf_plan(&doc, &|owner| remote.has_case_data(owner)).expect("no gaps");

    // Act
    let outcome = csrf_refusals(&doc, &remote, &plan).await;

    // Assert
    assert!(outcome.cases > 0);
    assert_eq!(outcome.failures, []);
    stub.stop().await;
}

/// E-21/c20 (ST-11-1, ST-11-3): the same matrices on a real stack named by `SUNNYCLOUD_MATRIX_*`.
#[tokio::test]
#[ignore = "E-21/c20: needs a real stack named by SUNNYCLOUD_MATRIX_BASE_URL"]
async fn the_matrices_hold_on_a_real_stack() {
    // Arrange
    let config = remote_from_env()
        .expect("SUNNYCLOUD_MATRIX_BASE_URL is set")
        .expect("the SUNNYCLOUD_MATRIX_* variables are complete");
    let target = Target::remote(config);
    let doc = Document::production();
    let plan = csrf_plan(&doc, &|owner| target.has_case_data(owner)).expect("no CSRF gaps");
    let cases = idor_plan(&doc, &|owner| target.has_per_account_fake(owner)).expect("no IDOR gaps");

    // Act
    let refusals = csrf_refusals(&doc, &target, &plan).await;
    let pairs = super::run_idor::two_accounts(&doc, &target, &cases).await;

    // Assert
    assert_eq!(refusals.failures, []);
    assert_eq!(pairs.failures, []);
}
