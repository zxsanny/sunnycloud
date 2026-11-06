use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use contracts::TreeReader;
use sunnycloud_http_api::ApiServices;
use uuid::Uuid;

/// The account every probe call is made for.
pub const ACCOUNT: Uuid = Uuid::from_u128(1);

/// Two routes for the harness's own tests: one calls a port, one panics.
pub fn router(services: &ApiServices) -> Router {
    Router::new()
        .route("/probe/root-node", get(root_node))
        .route("/probe/panic", get(panic_handler))
        .with_state(Arc::clone(&services.tree_reader))
}

async fn root_node(State(tree): State<Arc<dyn TreeReader>>) -> (StatusCode, String) {
    match tree.root_node(ACCOUNT).await {
        Ok(node) => (StatusCode::OK, node.id.to_string()),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:?}")),
    }
}

async fn panic_handler() -> StatusCode {
    panic!("probe handler panic");
}
