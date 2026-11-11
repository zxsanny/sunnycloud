//! The JSON of the admin routes. A listing never carries an invite token: the plaintext is in the create answer
//! only (`InviteService` stores its hash).

use contracts::{
    AccountState, AccountStatus, AdminUserView, HealthReport, HealthStatus, InviteInfo,
    InviteToken, Role,
};
use serde_json::{Value, json};

use crate::json_body::{self, optional, required, text};
use crate::registry::SchemaRef;
use crate::registry::account::role;
use crate::timestamp::rfc3339;

fn parse_role(value: &Value) -> Option<Role> {
    match value.as_str()? {
        "user" => Some(Role::User),
        "admin" => Some(Role::Admin),
        _ => None,
    }
}

fn parse_state(value: &Value) -> Option<AccountState> {
    match value.as_str()? {
        "active" => Some(AccountState::Active),
        "disabled" => Some(AccountState::Disabled),
        _ => None,
    }
}

/// `{role?, state?}`; a `null` is the same as an absent member. 07 says what neither member means.
pub(super) fn update_request(body: &Value) -> Option<(Option<Role>, Option<AccountState>)> {
    let members = json_body::object(body, &["role", "state"])?;
    Some((
        optional(members, "role", parse_role)?,
        optional(members, "state", parse_state)?,
    ))
}

/// `{limit}`: a whole number of bytes, or `null` to restore the installation default.
pub(super) fn quota_request(body: &Value) -> Option<Option<i64>> {
    let [limit] = json_body::fields(body, ["limit"])?;
    if limit.is_null() {
        return Some(None);
    }
    limit.as_i64().map(Some)
}

/// `{note}` of an invite.
pub(super) fn invite_request(body: &Value) -> Option<String> {
    let members = json_body::object(body, &["note"])?;
    required(members, "note", text)
}

fn status(status: AccountStatus) -> &'static str {
    match status {
        AccountStatus::Active => "active",
        AccountStatus::Disabled => "disabled",
        AccountStatus::Deleting => "deleting",
    }
}

pub(super) fn user_list(users: &[AdminUserView]) -> Value {
    let items: Vec<Value> = users
        .iter()
        .map(|user| {
            json!({
                "id": user.account,
                "role": role(user.role),
                "state": status(user.state),
                "last_login": user.last_login.map(rfc3339),
                "used": user.used,
                "limit": user.limit,
            })
        })
        .collect();
    json!({"items": items})
}

pub(super) fn health(report: &HealthReport) -> Value {
    let items: Vec<Value> = report
        .items
        .iter()
        .map(|(name, item)| {
            let word = match item.status {
                HealthStatus::Ok => "ok",
                HealthStatus::Degraded => "degraded",
                HealthStatus::Down => "down",
            };
            json!({"name": name, "status": word, "detail": item.detail})
        })
        .collect();
    json!({"items": items})
}

/// The plaintext token, shown once.
pub(super) fn created_invite(token: &InviteToken) -> Value {
    json!({"token": token.0.expose()})
}

pub(super) fn invite_list(invites: &[InviteInfo]) -> Value {
    let items: Vec<Value> = invites
        .iter()
        .map(|invite| {
            json!({
                "id": invite.id,
                "note": invite.note,
                "created_at": rfc3339(invite.created_at),
                "expires_at": rfc3339(invite.expires_at),
                "redeemed_at": invite.redeemed_at.map(rfc3339),
            })
        })
        .collect();
    json!({"items": items})
}

pub(super) const USER_LIST: SchemaRef = SchemaRef {
    name: "AdminUserList",
    build: || {
        json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "role", "state", "used"],
                        "properties": {
                            "id": {"type": "string", "format": "uuid"},
                            "role": {"type": "string", "enum": ["user", "admin"]},
                            "state": {"type": "string", "enum": ["active", "disabled", "deleting"]},
                            "last_login": {"type": ["string", "null"]},
                            "used": {"type": "integer"},
                            "limit": {"type": ["integer", "null"], "description": "Null when the installation default applies."}
                        }
                    }
                }
            }
        })
    },
};

pub(super) const UPDATE_REQUEST: SchemaRef = SchemaRef {
    name: "AdminUserUpdate",
    build: || {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "role": {"type": ["string", "null"], "enum": ["user", "admin", null]},
                "state": {"type": ["string", "null"], "enum": ["active", "disabled", null]}
            }
        })
    },
};

pub(super) const QUOTA_REQUEST: SchemaRef = SchemaRef {
    name: "AdminQuotaUpdate",
    build: || {
        json!({
            "type": "object",
            "required": ["limit"],
            "additionalProperties": false,
            "properties": {"limit": {"type": ["integer", "null"], "description": "Bytes; null restores the installation default."}}
        })
    },
};

pub(super) const HEALTH: SchemaRef = SchemaRef {
    name: "AdminHealth",
    build: || {
        json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["name", "status", "detail"],
                        "properties": {
                            "name": {"type": "string"},
                            "status": {"type": "string", "enum": ["ok", "degraded", "down"]},
                            "detail": {"type": "string"}
                        }
                    }
                }
            }
        })
    },
};

pub(super) const INVITE_REQUEST: SchemaRef = SchemaRef {
    name: "InviteRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["note"],
            "additionalProperties": false,
            "properties": {"note": {"type": "string"}}
        })
    },
};

pub(super) const CREATED_INVITE: SchemaRef = SchemaRef {
    name: "CreatedInvite",
    build: || {
        json!({
            "type": "object",
            "required": ["token"],
            "properties": {"token": {"type": "string", "description": "Shown once; no other answer carries it."}}
        })
    },
};

pub(super) const INVITE_LIST: SchemaRef = SchemaRef {
    name: "InviteList",
    build: || {
        json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "note", "created_at", "expires_at"],
                        "properties": {
                            "id": {"type": "string", "format": "uuid"},
                            "note": {"type": "string"},
                            "created_at": {"type": "string"},
                            "expires_at": {"type": "string"},
                            "redeemed_at": {"type": ["string", "null"]}
                        }
                    }
                }
            }
        })
    },
};
