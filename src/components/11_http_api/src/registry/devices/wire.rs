//! The JSON of the device routes. The grant's names are RFC 8628's (`device_code`, `user_code`, `interval`,
//! `expires_in`, `access_token`, `token_type`); the plaintext codes and the token are in these answers only.

use contracts::{ApprovalView, Decision, DeviceAuthorization, DeviceInfo, DeviceToken};
use sc_ids::{RawToken, TokenKind};
use serde_json::{Value, json};

use crate::json_body;
use crate::registry::SchemaRef;
use crate::registry::request::seconds;
use crate::timestamp::rfc3339;

/// `{device_name, platform}`.
pub(super) fn grant_request(body: &Value) -> Option<(String, String)> {
    let [name, platform] = json_body::fields(body, ["device_name", "platform"])?;
    Some((json_body::text(name)?, json_body::text(platform)?))
}

/// `{device_code}`.
pub(super) fn poll_request(body: &Value) -> Option<RawToken> {
    let [code] = json_body::fields(body, ["device_code"])?;
    Some(RawToken::from_presented(
        TokenKind::DeviceCode,
        code.as_str()?.to_owned(),
    ))
}

/// `{decision: "approve" | "deny"}`.
pub(super) fn decision(body: &Value) -> Option<Decision> {
    let [decision] = json_body::fields(body, ["decision"])?;
    match decision.as_str()? {
        "approve" => Some(Decision::Approve),
        "deny" => Some(Decision::Deny),
        _ => None,
    }
}

pub(super) fn authorization(grant: &DeviceAuthorization) -> Value {
    json!({
        "device_code": grant.device_code.expose(),
        "user_code": grant.user_code.as_str(),
        "interval": seconds(grant.interval),
        "expires_in": seconds(grant.expires_in),
    })
}

/// What the approver sees: the grant id `decide` takes, the code's expiry, the device's name, both addresses and
/// their locations (`null` when unknown), and whether the two addresses disagree.
pub(super) fn approval(view: &ApprovalView) -> Value {
    json!({
        "grant_id": view.grant_id,
        "expires_at": rfc3339(view.expires_at),
        "device": view.device,
        "requester_ip": view.requester_ip.to_string(),
        "requester_location": view.requester_location,
        "approver_ip": view.approver_ip.to_string(),
        "approver_location": view.approver_location,
        "mismatch": view.mismatch,
    })
}

pub(super) fn device_token(token: &DeviceToken) -> Value {
    json!({"access_token": token.0.expose(), "token_type": "Bearer"})
}

/// `{items: [{id, name, platform, created_at, last_seen_at}]}`; `last_seen_at` is `null` until first use.
pub(super) fn device_list(devices: &[DeviceInfo]) -> Value {
    let items: Vec<Value> = devices
        .iter()
        .map(|device| {
            json!({
                "id": device.id,
                "name": device.name,
                "platform": device.platform,
                "created_at": rfc3339(device.created_at),
                "last_seen_at": device.last_seen_at.map(rfc3339),
            })
        })
        .collect();
    json!({"items": items})
}

pub(super) const GRANT_REQUEST: SchemaRef = SchemaRef {
    name: "DeviceGrantRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["device_name", "platform"],
            "additionalProperties": false,
            "properties": {"device_name": {"type": "string"}, "platform": {"type": "string"}}
        })
    },
};

pub(super) const GRANT: SchemaRef = SchemaRef {
    name: "DeviceGrant",
    build: || {
        json!({
            "type": "object",
            "required": ["device_code", "user_code", "interval", "expires_in"],
            "properties": {
                "device_code": {"type": "string"},
                "user_code": {"type": "string"},
                "interval": {"type": "integer", "description": "Seconds between polls."},
                "expires_in": {"type": "integer", "description": "Seconds."}
            }
        })
    },
};

pub(super) const POLL_REQUEST: SchemaRef = SchemaRef {
    name: "DeviceTokenRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["device_code"],
            "additionalProperties": false,
            "properties": {"device_code": {"type": "string"}}
        })
    },
};

pub(super) const APPROVAL: SchemaRef = SchemaRef {
    name: "DeviceGrantApproval",
    build: || {
        json!({
            "type": "object",
            "required": [
                "grant_id", "expires_at", "device", "requester_ip", "requester_location",
                "approver_ip", "approver_location", "mismatch"
            ],
            "properties": {
                "grant_id": {"type": "string", "format": "uuid"},
                "expires_at": {"type": "string"},
                "device": {"type": "string"},
                "requester_ip": {"type": "string"},
                "requester_location": {"type": ["string", "null"]},
                "approver_ip": {"type": "string"},
                "approver_location": {"type": ["string", "null"]},
                "mismatch": {"type": "boolean"}
            }
        })
    },
};

pub(super) const DEVICE_TOKEN: SchemaRef = SchemaRef {
    name: "DeviceToken",
    build: || {
        json!({
            "type": "object",
            "required": ["access_token", "token_type"],
            "properties": {
                "access_token": {"type": "string"},
                "token_type": {"type": "string", "enum": ["Bearer"]}
            }
        })
    },
};

pub(super) const DECISION_REQUEST: SchemaRef = SchemaRef {
    name: "DeviceGrantDecision",
    build: || {
        json!({
            "type": "object",
            "required": ["decision"],
            "additionalProperties": false,
            "properties": {"decision": {"type": "string", "enum": ["approve", "deny"]}}
        })
    },
};

pub(super) const DEVICE_LIST: SchemaRef = SchemaRef {
    name: "DeviceList",
    build: || {
        json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "name", "platform", "created_at"],
                        "properties": {
                            "id": {"type": "string", "format": "uuid"},
                            "name": {"type": "string"},
                            "platform": {"type": "string"},
                            "created_at": {"type": "string"},
                            "last_seen_at": {"type": ["string", "null"]}
                        }
                    }
                }
            }
        })
    },
};
