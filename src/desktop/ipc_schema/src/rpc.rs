use std::fmt::Display;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

use crate::error::{ControlError, ControlResult};
use crate::methods::MethodCall;
use crate::types::StatusEvent;

pub const CONTROL_ERROR_CODE: i64 = -32000;
pub const PARSE_ERROR_CODE: i64 = -32700;
pub const INVALID_REQUEST_CODE: i64 = -32600;
pub const METHOD_NOT_FOUND_CODE: i64 = -32601;
pub const INVALID_PARAMS_CODE: i64 = -32602;
pub const INTERNAL_ERROR_CODE: i64 = -32603;

/// Method name of the notification that carries one `StatusEvent`.
pub const STATUS_EVENT_METHOD: &str = "status_event";

/// The `"jsonrpc": "2.0"` member; any other value does not decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version;

impl Serialize for Version {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text == "2.0" {
            Ok(Self)
        } else {
            Err(serde::de::Error::custom("jsonrpc must be \"2.0\""))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn parse_error(detail: impl Display) -> Self {
        Self::with_detail(PARSE_ERROR_CODE, "Parse error", detail)
    }

    pub fn invalid_request(detail: impl Display) -> Self {
        Self::with_detail(INVALID_REQUEST_CODE, "Invalid Request", detail)
    }

    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: METHOD_NOT_FOUND_CODE,
            message: "Method not found".to_owned(),
            data: Some(json!({ "method": method })),
        }
    }

    pub fn invalid_params(detail: impl Display) -> Self {
        Self::with_detail(INVALID_PARAMS_CODE, "Invalid params", detail)
    }

    pub fn internal(detail: impl Display) -> Self {
        Self::with_detail(INTERNAL_ERROR_CODE, "Internal error", detail)
    }

    /// Maps a `ControlError` to code -32000 with `data` = `{"kind": ..., fields}`.
    pub fn from_control(error: &ControlError) -> Result<Self, serde_json::Error> {
        Ok(Self {
            code: CONTROL_ERROR_CODE,
            message: error.variant_name().to_owned(),
            data: Some(serde_json::to_value(error)?),
        })
    }

    /// `None` for any error that is not a `ControlError` this build knows (for example a
    /// variant added by a newer daemon); clients show such an error generically.
    pub fn to_control(&self) -> Option<ControlError> {
        if self.code != CONTROL_ERROR_CODE {
            return None;
        }
        serde_json::from_value(self.data.clone()?).ok()
    }

    fn with_detail(code: i64, message: &str, detail: impl Display) -> Self {
        Self {
            code,
            message: message.to_owned(),
            data: Some(json!({ "detail": detail.to_string() })),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: Version,
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    pub fn new(id: RequestId, call: &MethodCall) -> Result<Self, serde_json::Error> {
        Ok(Self {
            jsonrpc: Version,
            id,
            method: call.name().to_owned(),
            params: Some(call.params_value()?),
        })
    }
}

/// A message without `id`, sent by the daemon after `subscribe`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: Version,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Notification {
    pub fn status_event(event: &StatusEvent) -> Result<Self, serde_json::Error> {
        Ok(Self {
            jsonrpc: Version,
            method: STATUS_EVENT_METHOD.to_owned(),
            params: Some(serde_json::to_value(event)?),
        })
    }

    /// `Ok(None)` when this notification is not a status event.
    pub fn as_status_event(&self) -> Result<Option<StatusEvent>, serde_json::Error> {
        if self.method != STATUS_EVENT_METHOD {
            return Ok(None);
        }
        let params = self.params.clone().unwrap_or(Value::Null);
        serde_json::from_value(params).map(Some)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Outcome {
    Success { result: Value },
    Failure { error: RpcError },
}

/// `id` is null when the request id could not be read (parse error, invalid request).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: Version,
    pub id: Option<RequestId>,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error("{0}")]
    Control(ControlError),
    #[error("JSON-RPC error {}: {}", .0.code, .0.message)]
    Rpc(RpcError),
    #[error("malformed result: {0}")]
    Malformed(serde_json::Error),
}

impl Response {
    pub fn success<T: Serialize>(id: RequestId, result: &T) -> Result<Self, serde_json::Error> {
        Ok(Self {
            jsonrpc: Version,
            id: Some(id),
            outcome: Outcome::Success {
                result: serde_json::to_value(result)?,
            },
        })
    }

    pub fn failure(id: Option<RequestId>, error: RpcError) -> Self {
        Self {
            jsonrpc: Version,
            id,
            outcome: Outcome::Failure { error },
        }
    }

    /// The response for one method outcome. A result or error that cannot be serialized
    /// becomes a -32603 Internal error instead of being dropped.
    pub fn from_outcome<T: Serialize>(id: RequestId, outcome: ControlResult<T>) -> Self {
        let built = match &outcome {
            Ok(value) => Self::success(id.clone(), value),
            Err(error) => {
                RpcError::from_control(error).map(|rpc| Self::failure(Some(id.clone()), rpc))
            }
        };
        built.unwrap_or_else(|e| Self::failure(Some(id), RpcError::internal(e)))
    }

    /// Typed reading of the result by the client.
    pub fn into_result<T: DeserializeOwned>(self) -> Result<T, CallError> {
        match self.outcome {
            Outcome::Success { result } => {
                serde_json::from_value(result).map_err(CallError::Malformed)
            }
            Outcome::Failure { error } => Err(match error.to_control() {
                Some(control) => CallError::Control(control),
                None => CallError::Rpc(error),
            }),
        }
    }
}

/// What a client reads from the daemon: a response or a notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerMessage {
    Notification(Notification),
    Response(Response),
}
