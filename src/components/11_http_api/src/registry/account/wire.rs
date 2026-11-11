//! The JSON of the account routes. `QuotaView` is shared with the admin routes' wording: the limit is `null` when
//! the installation default applies.

use contracts::{AccountView, DeletionAccepted, QuotaView, Role};
use serde_json::{Value, json};

use crate::json_body;
use crate::registry::SchemaRef;
use crate::timestamp::rfc3339;

/// `{confirmation}` of an account deletion request.
pub(super) fn deletion_request(body: &Value) -> Option<String> {
    let [confirmation] = json_body::fields(body, ["confirmation"])?;
    json_body::text(confirmation)
}

pub(in crate::registry) fn role(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Admin => "admin",
    }
}

pub(super) fn quota(quota: &QuotaView) -> Value {
    json!({
        "limit_bytes": quota.limit_bytes,
        "effective_limit": quota.effective_limit,
        "used": quota.used,
        "reserved": quota.reserved,
    })
}

pub(super) fn account(view: &AccountView) -> Value {
    let identities: Vec<Value> = view
        .identities
        .iter()
        .map(|identity| json!({"provider": identity.provider, "email": identity.email}))
        .collect();
    json!({
        "id": view.id,
        "display_name": view.display_name,
        "role": role(view.role),
        "identities": identities,
        "quota": quota(&view.quota),
        "root_node_id": view.root_node_id,
    })
}

pub(super) fn deletion(accepted: &DeletionAccepted) -> Value {
    json!({"deadline": rfc3339(accepted.deadline)})
}

fn quota_schema() -> Value {
    json!({
        "type": "object",
        "required": ["limit_bytes", "effective_limit", "used", "reserved"],
        "properties": {
            "limit_bytes": {"type": ["integer", "null"], "description": "The account's own limit; null when the installation default applies."},
            "effective_limit": {"type": "integer"},
            "used": {"type": "integer"},
            "reserved": {"type": "integer"}
        }
    })
}

pub(super) const QUOTA: SchemaRef = SchemaRef {
    name: "Quota",
    build: quota_schema,
};

pub(super) const ACCOUNT: SchemaRef = SchemaRef {
    name: "Account",
    build: || {
        json!({
            "type": "object",
            "required": ["id", "display_name", "role", "identities", "quota", "root_node_id"],
            "properties": {
                "id": {"type": "string", "format": "uuid"},
                "display_name": {"type": "string"},
                "role": {"type": "string", "enum": ["user", "admin"]},
                "identities": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["provider"],
                        "properties": {"provider": {"type": "string"}, "email": {"type": ["string", "null"]}}
                    }
                },
                "quota": quota_schema(),
                "root_node_id": {"type": "string", "format": "uuid"}
            }
        })
    },
};

pub(super) const DELETION_REQUEST: SchemaRef = SchemaRef {
    name: "AccountDeletionRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["confirmation"],
            "additionalProperties": false,
            "properties": {"confirmation": {"type": "string"}}
        })
    },
};

pub(super) const DELETION: SchemaRef = SchemaRef {
    name: "AccountDeletionAccepted",
    build: || {
        json!({
            "type": "object",
            "required": ["deadline"],
            "properties": {"deadline": {"type": "string", "description": "RFC 3339 UTC; the account is erased by then."}}
        })
    },
};
