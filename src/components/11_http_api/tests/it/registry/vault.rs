//! AZ-3926 AC-1 and the 11 side of AC-7: a server whose vault slot is filled documents and routes the four vault
//! operations (declarations, statuses, problem schema, router parity, naming lint, a valid document); a server
//! with an empty slot (v1.0, v1.1) has neither the routes nor their document entries.

use std::collections::BTreeSet;
use std::net::SocketAddr;

use serde_json::{Value, json};
use sunnycloud_http_api::registry::{HttpMethod, lint_path, validate, vault_routes};
use sunnycloud_http_api::{ApiServices, HttpServer, current_document};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::oas_validator::validate_document;
use super::support::concrete;
use crate::harness::router_server::request;
use crate::harness::{Script, default_config, new_metrics, stub_services, vault_stub};

type Operation = (String, String);

struct Running {
    public: SocketAddr,
    document: Value,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Running {
    async fn start(services: ApiServices) -> Running {
        let server = HttpServer::new(services, default_config()).expect("server builds");
        let document = serde_json::from_slice(server.openapi().as_bytes()).expect("document");
        let bound = server.bind().await.expect("listeners bind");
        let public = bound.public_addr();
        let (stop, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            bound
                .serve(async move {
                    signal.await.ok();
                })
                .await
                .expect("serve returns");
        });
        Running {
            public,
            document,
            stop,
            task,
        }
    }

    async fn stop(self) {
        self.stop.send(()).expect("server is running");
        self.task.await.expect("serve joins");
    }
}

fn with_vault() -> ApiServices {
    let script = Script::default();
    ApiServices {
        vault: Some(vault_stub(&script)),
        ..stub_services(&script, new_metrics())
    }
}

fn vault_operations(document: &Value) -> BTreeSet<Operation> {
    let mut found = BTreeSet::new();
    for (path, item) in document["paths"].as_object().expect("paths") {
        if path.starts_with("/api/v1/vaults") {
            for method in item.as_object().expect("path item").keys() {
                found.insert((method.to_uppercase(), path.clone()));
            }
        }
    }
    found
}

/// The (method, path) pairs of the document's vault paths that the live public listener answers other than with
/// 404 or 405.
async fn routed_vault_operations(public: SocketAddr, paths: &[&str]) -> BTreeSet<Operation> {
    let mut found = BTreeSet::new();
    for path in paths {
        for method in HttpMethod::ALL {
            let verb = method.key().to_uppercase();
            let answer = request(public, &verb, &concrete(path)).await;
            if answer.status != 404 && answer.status != 405 {
                found.insert((verb, (*path).to_owned()));
            }
        }
    }
    found
}

const VAULT_PATHS: [&str; 4] = [
    "/api/v1/vaults",
    "/api/v1/vaults/{id}",
    "/api/v1/vaults/{id}/header",
    "/api/v1/vaults/{id}/unlock-failures",
];

fn expected_operations() -> BTreeSet<Operation> {
    [
        ("POST", "/api/v1/vaults"),
        ("GET", "/api/v1/vaults/{id}"),
        ("PUT", "/api/v1/vaults/{id}/header"),
        ("POST", "/api/v1/vaults/{id}/unlock-failures"),
    ]
    .into_iter()
    .map(|(method, path)| (method.to_owned(), path.to_owned()))
    .collect()
}

#[tokio::test]
async fn a_vault_build_documents_and_routes_exactly_the_four_vault_operations() {
    // Arrange
    let server = Running::start(with_vault()).await;

    // Act
    let documented = vault_operations(&server.document);
    let routed = routed_vault_operations(server.public, &VAULT_PATHS).await;

    // Assert
    assert_eq!(documented, expected_operations());
    assert_eq!(routed, documented);
    assert_eq!(validate_document(&server.document), Vec::<String>::new());
    server.stop().await;
}

#[test]
fn each_vault_operation_declares_its_classes_owner_id_params_and_statuses() {
    // Arrange
    let routes = vault_routes();
    let document: Value = {
        let doc = sunnycloud_http_api::registry::document(&routes);
        serde_json::from_slice(doc.as_bytes()).expect("document")
    };
    let declared = |method: &str, path: &str| {
        let operation = &document["paths"][path][method];
        let mut statuses: Vec<&String> = operation["responses"]
            .as_object()
            .expect("responses")
            .keys()
            .collect();
        statuses.sort();
        json!({
            "auth": operation["x-auth-class"],
            "body": operation["x-body-class"],
            "mutates": operation["x-state-changing"],
            "owner": operation["x-owner-method"],
            "ids": operation["x-id-params"],
            "statuses": statuses,
        })
    };

    // Act
    let found = [
        declared("post", "/api/v1/vaults"),
        declared("get", "/api/v1/vaults/{id}"),
        declared("put", "/api/v1/vaults/{id}/header"),
        declared("post", "/api/v1/vaults/{id}/unlock-failures"),
    ];

    // Assert
    let path_id = json!([{"name": "id", "in": "path"}]);
    assert_eq!(validate(&routes), Ok(()));
    assert_eq!(
        found,
        [
            json!({"auth": "SessionOnly", "body": "Small", "mutates": true,
                "owner": "VaultService::create_vault", "ids": [{"name": "root_node_id", "in": "body"}],
                "statuses": ["201", "400", "401", "403", "404", "409", "413", "426", "500"]}),
            json!({"auth": "Session", "body": "None", "mutates": false,
                "owner": "VaultService::get_header", "ids": path_id,
                "statuses": ["200", "400", "401", "403", "404", "426", "500"]}),
            json!({"auth": "SessionOnly", "body": "Small", "mutates": true,
                "owner": "VaultService::replace_header", "ids": path_id,
                "statuses": ["200", "400", "401", "403", "404", "412", "413", "426", "428", "500"]}),
            json!({"auth": "Session", "body": "Small", "mutates": true,
                "owner": "VaultService::report_unlock_failure", "ids": path_id,
                "statuses": ["204", "400", "401", "403", "404", "413", "426", "500"]}),
        ]
    );
    let problem = &document["paths"]["/api/v1/vaults/{id}/header"]["put"]["responses"]["412"];
    assert_eq!(
        problem["content"]["application/problem+json"]["schema"]["$ref"],
        "#/components/schemas/Problem"
    );
    assert_eq!(problem["x-error-codes"], json!(["precondition_failed"]));
    let if_match = &document["paths"]["/api/v1/vaults/{id}/header"]["put"]["parameters"][1];
    assert_eq!(if_match["name"], "If-Match");
    assert_eq!(if_match["in"], "header");
    assert_eq!(if_match["required"], true);
}

#[test]
fn every_vault_path_passes_the_naming_lint() {
    // Arrange
    let paths: Vec<&str> = vault_routes().iter().map(|def| def.spec.path).collect();

    // Act
    let violations: Vec<_> = paths.iter().flat_map(|path| lint_path(path)).collect();

    // Assert
    assert_eq!(paths.len(), 4);
    assert_eq!(violations, []);
}

#[tokio::test]
async fn a_build_without_the_vault_slot_has_no_vault_route_or_document_entry() {
    // Arrange
    let server = Running::start(stub_services(&Script::default(), new_metrics())).await;

    // Act
    let documented = vault_operations(&server.document);
    let routed = routed_vault_operations(server.public, &VAULT_PATHS).await;
    let from_release =
        vault_operations(&serde_json::from_slice(current_document().as_bytes()).expect("document"));

    // Assert
    assert_eq!(documented, BTreeSet::new());
    assert_eq!(routed, BTreeSet::new());
    assert_eq!(from_release, BTreeSet::new());
    server.stop().await;
}
