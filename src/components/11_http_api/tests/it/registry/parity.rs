use std::collections::BTreeSet;

use serde_json::Value;
use sunnycloud_http_api::registry::{HttpMethod, document, production_routes};
use sunnycloud_http_api::{ApiServices, HttpServer, current_document};

use super::support::{concrete, sample_routers, sample_surface};
use crate::harness::router_server::{RouterServer, request};
use crate::harness::{Script, default_config, new_metrics, stub_services};

type Operation = (String, String);

fn operations_on(document: &Value, listener: &str) -> BTreeSet<Operation> {
    let mut found = BTreeSet::new();
    for (path, item) in document["paths"].as_object().expect("paths") {
        for (key, operation) in item.as_object().expect("path item") {
            if operation["x-listener"] == listener {
                found.insert((key.to_uppercase(), path.clone()));
            }
        }
    }
    found
}

/// Every (method, path) a live listener answers other than with 404 or 405, found by asking it for every
/// method of every path the document names.
async fn routed_on(server: &RouterServer, document: &Value) -> BTreeSet<Operation> {
    let mut found = BTreeSet::new();
    for path in document["paths"].as_object().expect("paths").keys() {
        for method in HttpMethod::ALL {
            let verb = method.key().to_uppercase();
            let answer = request(server.addr, &verb, &concrete(path)).await;
            if answer.status != 404 && answer.status != 405 {
                found.insert((verb, path.clone()));
            }
        }
    }
    found
}

#[tokio::test]
async fn the_routers_answer_exactly_the_operations_the_document_lists_per_listener() {
    // Arrange
    let (routers, openapi) = sample_routers(&sample_surface());
    let document: Value = serde_json::from_slice(openapi.as_bytes()).expect("document");
    let public = RouterServer::start(routers.public).await;
    let internal = RouterServer::start(routers.internal).await;

    // Act
    let routed_public = routed_on(&public, &document).await;
    let routed_internal = routed_on(&internal, &document).await;

    // Assert
    assert_eq!(routed_public, operations_on(&document, "public"));
    assert_eq!(routed_internal, operations_on(&document, "internal"));
    assert!(routed_public.len() >= 15 && routed_internal.len() == 3);
    public.stop().await;
    internal.stop().await;
}

#[tokio::test]
async fn the_served_document_is_byte_equal_to_openapi() {
    // Arrange
    let services = stub_services(&Script::default(), new_metrics());
    let server = HttpServer::new(services, default_config()).expect("server builds");
    let printed = server.openapi();
    let bound = server.bind().await.expect("listeners bind");
    let addr = bound.public_addr();
    let (stop, signal) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(bound.serve(async move {
        signal.await.ok();
    }));

    // Act
    let served = request(addr, "GET", "/api/v1/openapi.json").await;

    // Assert
    assert_eq!(served.status, 200);
    assert_eq!(served.content_type.as_deref(), Some("application/json"));
    assert_eq!(served.body, printed.as_bytes());
    stop.send(()).expect("server is running");
    task.await.expect("serve joins").expect("serve returns");
}

#[tokio::test]
async fn the_real_servers_routes_match_its_document() {
    // Arrange
    let server = HttpServer::new(
        stub_services(&Script::default(), new_metrics()),
        default_config(),
    )
    .expect("server builds");
    let document: Value = serde_json::from_slice(server.openapi().as_bytes()).expect("document");
    let bound = server.bind().await.expect("listeners bind");
    let (stop, signal) = tokio::sync::oneshot::channel::<()>();
    let public = bound.public_addr();
    let internal = bound.internal_addr();
    let task = tokio::spawn(bound.serve(async move {
        signal.await.ok();
    }));

    // Act
    let mut live = BTreeSet::new();
    for path in document["paths"].as_object().expect("paths").keys() {
        for method in HttpMethod::ALL {
            let verb = method.key().to_uppercase();
            let answer = request(public, &verb, &concrete(path)).await;
            if answer.status != 404 && answer.status != 405 {
                live.insert((verb, path.clone()));
            }
        }
    }
    let on_internal = request(internal, "GET", "/api/v1/server-info").await;

    // Assert
    assert_eq!(live, operations_on(&document, "public"));
    assert!(operations_on(&document, "internal").is_empty());
    assert_eq!(on_internal.status, 404);
    stop.send(()).expect("server is running");
    task.await.expect("serve joins").expect("serve returns");
}

#[test]
fn two_builds_of_the_document_are_byte_identical() {
    // Arrange
    let first = current_document();

    // Act
    let second = current_document();

    // Assert
    assert_eq!(first.as_bytes(), second.as_bytes());
    let server =
        |services: ApiServices| HttpServer::new(services, default_config()).expect("builds");
    let from_servers = [
        server(stub_services(&Script::default(), new_metrics())).openapi(),
        server(stub_services(&Script::default(), new_metrics())).openapi(),
    ];
    assert_eq!(from_servers[0].as_bytes(), first.as_bytes());
    assert_eq!(from_servers[1].as_bytes(), first.as_bytes());
}

#[test]
fn the_document_does_not_depend_on_registration_order() {
    // Arrange
    let defs = sample_surface();
    let mut reversed = defs.clone();
    reversed.reverse();

    // Act
    let (forward, backward) = (document(&defs), document(&reversed));

    // Assert
    assert_eq!(forward.as_bytes(), backward.as_bytes());
}

#[test]
fn the_document_lists_the_system_operations() {
    // Arrange
    let document: Value = serde_json::from_slice(current_document().as_bytes()).expect("document");

    // Act
    let public = operations_on(&document, "public");

    // Assert
    let expected: BTreeSet<Operation> = production_routes()
        .iter()
        .map(|route| {
            (
                route.spec.method.key().to_uppercase(),
                route.spec.path.to_owned(),
            )
        })
        .collect();
    assert_eq!(public, expected);
    assert!(expected.contains(&("GET".to_owned(), "/api/v1/server-info".to_owned())));
    assert!(expected.contains(&("GET".to_owned(), "/api/v1/openapi.json".to_owned())));
}
