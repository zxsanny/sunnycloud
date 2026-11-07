use serde_json::Value;
use sunnycloud_http_api::registry::{ListenerKind, document, mount};

use super::support::{def, sample_routers, sample_surface};
use crate::harness::router_server::{RouterServer, request, route_context};

fn without_request_id(mut body: Value) -> Value {
    body.as_object_mut()
        .expect("object")
        .remove("request_id")
        .expect("request_id");
    body
}

#[tokio::test]
async fn an_internal_route_is_absent_from_the_public_router_and_answered_by_the_internal_one() {
    // Arrange
    let (routers, _) = sample_routers(&sample_surface());
    let public = RouterServer::start(routers.public).await;
    let internal = RouterServer::start(routers.internal).await;

    // Act
    let on_public = request(public.addr, "POST", "/api/v1/worker-jobs/claims").await;
    let lease_on_public = request(public.addr, "PUT", "/api/v1/worker-jobs/7/lease").await;
    let unknown_on_public = request(public.addr, "POST", "/api/v1/worker-jobs/nothing").await;
    let on_internal = request(internal.addr, "POST", "/api/v1/worker-jobs/claims").await;

    // Assert
    assert_eq!(on_public.status, 404);
    assert_eq!(lease_on_public.status, 404);
    assert_eq!(on_internal.status, 204);
    assert_eq!(
        without_request_id(on_public.json()),
        without_request_id(unknown_on_public.json()),
        "a worker route on the public listener is indistinguishable from an unknown path"
    );
    public.stop().await;
    internal.stop().await;
}

#[tokio::test]
async fn the_internal_router_serves_only_its_own_routes() {
    // Arrange
    let (routers, _) = sample_routers(&sample_surface());
    let internal = RouterServer::start(routers.internal).await;

    // Act
    let answers = [
        request(internal.addr, "GET", "/api/v1/server-info").await,
        request(internal.addr, "GET", "/api/v1/nodes/root").await,
        request(internal.addr, "GET", "/files/a").await,
        request(internal.addr, "GET", "/api/v1/openapi.json").await,
    ];

    // Assert
    for answer in &answers {
        assert_eq!(answer.status, 404);
        assert_eq!(answer.json()["code"], "not_found");
    }
    internal.stop().await;
}

#[test]
fn the_document_marks_internal_operations_with_the_worker_scheme_and_keeps_workers_off_public_ones()
{
    // Arrange
    let parsed: Value =
        serde_json::from_slice(document(&sample_surface()).as_bytes()).expect("document");

    // Act
    let mut internal = 0;
    for item in parsed["paths"].as_object().expect("paths").values() {
        for operation in item.as_object().expect("item").values() {
            let worker = operation["x-auth-class"] == "Worker";
            let is_internal = operation["x-listener"] == "internal";

            // Assert
            assert_eq!(worker, is_internal);
            if is_internal {
                internal += 1;
                assert_eq!(
                    operation["security"],
                    serde_json::json!([{"bearerWorker": []}])
                );
            }
        }
    }
    assert_eq!(internal, 3);
}

#[tokio::test]
async fn a_route_declared_internal_is_mounted_only_on_the_internal_router() {
    // Arrange
    let defs = vec![def(sunnycloud_http_api::registry::RouteSpec::new(
        sunnycloud_http_api::registry::HttpMethod::Get,
        "/api/v1/worker-jobs/status",
        "WorkerJobs::record_status",
        sunnycloud_http_api::registry::AuthClass::Worker,
        sunnycloud_http_api::registry::BodyClass::None,
    )
    .summary("A test operation")
    .internal())];
    assert_eq!(defs[0].spec.listener, ListenerKind::Internal);

    // Act
    let routers = mount(&defs, &route_context(document(&defs)), None);
    let (public, internal) = (
        RouterServer::start(routers.public).await,
        RouterServer::start(routers.internal).await,
    );

    // Assert
    assert_eq!(
        request(public.addr, "GET", "/api/v1/worker-jobs/status")
            .await
            .status,
        404
    );
    assert_eq!(
        request(internal.addr, "GET", "/api/v1/worker-jobs/status")
            .await
            .status,
        204
    );
    public.stop().await;
    internal.stop().await;
}
