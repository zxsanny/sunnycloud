//! The sign-in and token calls of `SyncApi` (`sync_api.md` § Operations): device grant, mobile code exchange,
//! refresh and sign-out. Bodies here carry secrets (codes, tokens), so a body that does not match its DTO is
//! reported without the decoder's message, which can quote the value.

use bytes::Bytes;
use sc_config::Secret;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sync_core_proto::{DeviceAuthorization, DevicePoll, SessionTokens};

use crate::error::SyncError;
use crate::request::{Method, Request, RequestPath, WireResponse};
use crate::tree_ops::{into_error, send};

const DEVICE_GRANTS: &str = "/api/v1/device-grants";
const DEVICE_TOKENS: &str = "/api/v1/device-tokens";
const MOBILE_CODE_EXCHANGES: &str = "/api/v1/auth/mobile-code-exchanges";
const REFRESH_TOKENS: &str = "/api/v1/auth/refresh-tokens";
const DEVICE_SELF: &str = "/api/v1/devices/me";
const SESSION_CURRENT: &str = "/api/v1/sessions/current";

/// The wire code of a reused, expired or wrong-verifier mobile code; it has no `sc_error` row yet (`sync_api.md` open point 3).
const INVALID_CODE: &str = "invalid_code";

pub(crate) async fn device_grant_request(
    path: &RequestPath,
    device_name: &str,
    platform: &str,
) -> Result<DeviceAuthorization, SyncError> {
    let body = json!({ "device_name": device_name, "platform": platform });
    let request = json_post(path, DEVICE_GRANTS, &body);
    decode(
        "device_grant_request",
        anonymous(path, "device_grant_request", request).await?,
    )
}

/// A new token ends a stop of work: the host stores it next.
pub(crate) async fn device_grant_poll(
    path: &RequestPath,
    device_code: &str,
) -> Result<DevicePoll, SyncError> {
    let request = json_post(path, DEVICE_TOKENS, &json!({ "device_code": device_code }));
    let poll: DevicePoll = decode(
        "device_grant_poll",
        anonymous(path, "device_grant_poll", request).await?,
    )?;
    if matches!(poll, DevicePoll::Token(_)) {
        path.resume_auth();
    }
    Ok(poll)
}

pub(crate) async fn exchange_mobile_code(
    path: &RequestPath,
    code: &str,
    code_verifier: &str,
) -> Result<SessionTokens, SyncError> {
    let body = json!({ "code": code, "code_verifier": code_verifier });
    let request = json_post(path, MOBILE_CODE_EXCHANGES, &body);
    let answer = anonymous(path, "exchange_mobile_code", request)
        .await
        .map_err(|error| match error {
            SyncError::Unexpected { code, .. } if code == INVALID_CODE => SyncError::InvalidCode,
            other => other,
        })?;
    let tokens = decode("exchange_mobile_code", answer)?;
    path.resume_auth();
    Ok(tokens)
}

/// One rotation request. The refresh token travels in the body only, and the request carries no access token.
pub(crate) async fn request_refresh(
    path: &RequestPath,
    refresh: &Secret<String>,
) -> Result<SessionTokens, SyncError> {
    let request = json_post(
        path,
        REFRESH_TOKENS,
        &json!({ "refresh_token": refresh.expose() }),
    );
    decode("refresh", anonymous(path, "refresh", request).await?)
}

/// Revokes the caller's own credential: a device token through the device route, a session through its own.
pub(crate) async fn sign_out(path: &RequestPath) -> Result<(), SyncError> {
    let target = if path.is_device_credential() {
        DEVICE_SELF
    } else {
        SESSION_CURRENT
    };
    let request = Request {
        method: Method::Delete,
        target: target.to_owned(),
        headers: Vec::new(),
        body: Bytes::new(),
        max_attempts: path.default_attempts(),
    };
    send(path, "sign_out", request).await.map(|_| ())
}

fn json_post(path: &RequestPath, target: &str, body: &Value) -> Request {
    Request {
        method: Method::Post,
        target: target.to_owned(),
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: Bytes::from(body.to_string()),
        max_attempts: path.default_attempts(),
    }
}

async fn anonymous(
    path: &RequestPath,
    op: &'static str,
    request: Request,
) -> Result<WireResponse, SyncError> {
    path.send_anonymous(op, request).await.map_err(into_error)
}

fn decode<T: DeserializeOwned>(op: &'static str, response: WireResponse) -> Result<T, SyncError> {
    serde_json::from_slice(&response.body).map_err(|error| {
        tracing::warn!(op, kind = ?error.classify(), "response body does not match its DTO");
        SyncError::ProtocolViolation {
            detail: "malformed response body",
        }
    })
}
