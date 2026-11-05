//! Client-side parsing of any error response into a typed value (feature `client`).

use crate::business::{BusinessParams, ResyncReason};
use crate::code::ErrorCode;
use crate::correlation::CorrelationId;
use serde_json::Value;
use std::time::Duration;

/// Why no HTTP response arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind {
    Timeout,
    Connect,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    /// A response arrived. An unknown or unreadable code is `internal`; the request id is kept when present.
    Api {
        code: ErrorCode,
        params: BusinessParams,
        correlation_id: Option<CorrelationId>,
    },
    /// No response arrived.
    Transport(TransportKind),
}

impl ApiError {
    /// Parses a response; total, never panics. `status` below 100 (0) means no response arrived.
    /// `headers` are (name, value) pairs; `X-Request-Id` and `Retry-After` (seconds) are read when the body lacks them.
    pub fn parse(status: u16, headers: &[(&str, &str)], body: &[u8]) -> ApiError {
        if status < 100 {
            return ApiError::Transport(TransportKind::Other);
        }
        let parsed = match serde_json::from_slice::<Value>(body) {
            Ok(value) => value,
            Err(_) => Value::Null,
        };
        let field = |name: &str| parsed.get(name);
        let correlation_id = field("request_id")
            .and_then(Value::as_str)
            .and_then(CorrelationId::parse)
            .or_else(|| header(headers, "x-request-id").and_then(CorrelationId::parse));
        let code = field("code")
            .and_then(Value::as_str)
            .and_then(ErrorCode::from_wire);
        let Some(code) = code else {
            return ApiError::Api {
                code: ErrorCode::Internal,
                params: BusinessParams::default(),
                correlation_id,
            };
        };
        let retry_after = field("retry_after")
            .and_then(Value::as_u64)
            .or_else(|| header(headers, "retry-after").and_then(|v| v.trim().parse().ok()))
            .map(Duration::from_secs);
        let need_blocks = field("need_blocks")
            .and_then(Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .map(|item| item.as_u64().and_then(|n| u32::try_from(n).ok()))
                    .collect::<Option<Vec<u32>>>()
            })
            .unwrap_or_default();
        ApiError::Api {
            code,
            params: BusinessParams {
                need_blocks,
                retry_after,
                reason: field("reason")
                    .and_then(Value::as_str)
                    .and_then(ResyncReason::from_wire),
                min_version: field("min_version")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            },
            correlation_id,
        }
    }

    /// The server's retry delay, when the response carried one.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            ApiError::Api { params, .. } => params.retry_after,
            ApiError::Transport(_) => None,
        }
    }
}

fn header<'a>(headers: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value)
}
