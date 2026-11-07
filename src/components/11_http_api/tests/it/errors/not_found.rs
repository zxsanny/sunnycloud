use axum::response::IntoResponse;
use contracts::{
    AdminError, DerivedError, DeviceGrantError, DownloadError, PublicShareError, ShareError,
    TreeReaderError, TreeServiceError, VaultError,
};
use http_body_util::BodyExt;
use sc_error::CorrelationId;
use sunnycloud_http_api::{ApiProblem, PortError};
use uuid::Uuid;

use super::probe::{ACCOUNT, ErrorProbe, fresh_id};

const GET_NODE: &str = "TreeReader::get_node";

fn without_request_id(mut body: serde_json::Value) -> serde_json::Value {
    body.as_object_mut()
        .expect("object")
        .remove("request_id")
        .expect("request_id");
    body
}

#[tokio::test]
async fn a_foreign_id_and_a_random_id_get_404_bodies_equal_apart_from_request_id() {
    // Arrange
    let probe = ErrorProbe::start().await;
    for _ in 0..2 {
        probe.script.push(
            GET_NODE,
            Err::<contracts::NodeView, _>(TreeReaderError::NotFound),
        );
    }
    let foreign = Uuid::from_u128(ACCOUNT.as_u128() + 1_000);
    let random = Uuid::from_u128(0x5eed_0001);

    // Act
    let foreign_reply = probe
        .get(&format!("/probe/nodes/{foreign}"), fresh_id())
        .await;
    let random_reply = probe
        .get(&format!("/probe/nodes/{random}"), fresh_id())
        .await;

    // Assert
    assert_eq!(foreign_reply.status.as_u16(), 404);
    assert_eq!(random_reply.status.as_u16(), 404);
    assert_ne!(
        foreign_reply.json()["request_id"],
        random_reply.json()["request_id"]
    );
    assert_eq!(
        without_request_id(foreign_reply.json()),
        without_request_id(random_reply.json())
    );
    for name in ["content-type", "cache-control", "x-content-type-options"] {
        assert_eq!(foreign_reply.header(name), random_reply.header(name));
    }
    assert_eq!(probe.script.calls(GET_NODE), 2);
    probe.stop().await;
}

async fn bytes_of(error: impl PortError, id: CorrelationId) -> Vec<u8> {
    ApiProblem::from_port(error, id)
        .into_response()
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes()
        .to_vec()
}

#[tokio::test]
async fn every_port_variant_meaning_missing_or_foreign_gives_byte_identical_bodies() {
    // Arrange
    let id = fresh_id();

    // Act
    let answers = [
        bytes_of(TreeReaderError::NotFound, id).await,
        bytes_of(TreeServiceError::NotFound, id).await,
        bytes_of(DownloadError::NotFound, id).await,
        bytes_of(ShareError::TargetNotFound, id).await,
        bytes_of(ShareError::NotFound, id).await,
        bytes_of(PublicShareError::NotFound, id).await,
        bytes_of(AdminError::NotFound, id).await,
        bytes_of(DeviceGrantError::NotFound, id).await,
        bytes_of(VaultError::NotFound, id).await,
    ];

    // Assert
    for answer in &answers[1..] {
        assert_eq!(answer, &answers[0]);
    }
    let derived = bytes_of(DerivedError::NotFound, id).await;
    assert_ne!(derived, answers[0], "a thumbnail answer keeps its own code");
}
