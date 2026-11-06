//! The one request `connect` sends before it has a request path: `GET /api/v1/server-info`.

use bytes::Bytes;
use sync_core_proto::ServerInfo;

use crate::client::ConnectError;
use crate::error::{Mapped, SyncError};
use crate::http_transport::{HttpTransport, SERVER_INFO_PATH};
use crate::request::{Method, Transport, WireRequest, WireResponse};
use crate::wire_table::map_problem;

/// `GET /api/v1/server-info`: public, so it carries no credential and no version header.
pub(crate) async fn fetch_server_info(
    transport: &HttpTransport,
) -> Result<ServerInfo, ConnectError> {
    let response = transport
        .send(WireRequest {
            method: Method::Get,
            target: SERVER_INFO_PATH.to_owned(),
            headers: Vec::new(),
            bearer: None,
            body: Bytes::new(),
        })
        .await
        .map_err(|kind| {
            tracing::warn!(?kind, "server-info request failed");
            ConnectError::Unreachable
        })?;
    if !(200..300).contains(&response.status) {
        return Err(refused(&response));
    }
    serde_json::from_slice(&response.body).map_err(|_| ConnectError::ProtocolMismatch {
        what: "server_info",
    })
}

fn refused(response: &WireResponse) -> ConnectError {
    let headers: Vec<(&str, &str)> = response
        .headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    tracing::warn!(status = response.status, "server-info refused");
    match map_problem(response.status, &headers, &response.body) {
        Mapped::Error(SyncError::Unauthorized) => ConnectError::Unauthorized,
        Mapped::Error(SyncError::UpgradeRequired {
            message,
            update_url,
            ..
        }) => ConnectError::UpgradeRequired {
            message,
            update_url,
        },
        Mapped::Error(SyncError::Transport(_) | SyncError::RateLimited { .. }) => {
            ConnectError::Unreachable
        }
        _ => ConnectError::ProtocolMismatch {
            what: "server_info",
        },
    }
}
