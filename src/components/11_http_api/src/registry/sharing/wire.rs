//! The JSON of the share routes. 06 fixes the ports, not the bodies; the field names here are 11's choice
//! (`api_v1_surface.md`, open point 22).

use std::time::Duration;

use contracts::{CreatedShare, ShareInfo};
use sc_clock::Timestamp;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::registry::SchemaRef;
use crate::timestamp::rfc3339;

/// The body of `POST /shares`.
pub(super) struct CreateShare {
    pub(super) node_id: Uuid,
    pub(super) expires_in: Option<Duration>,
    pub(super) password: Option<String>,
}

/// `{node_id, expires_in?, password?}` with `expires_in` in whole seconds; an unknown field, a wrong type or a
/// node id that is not a UUID is `None`. A `null` is the same as an absent optional field.
pub(super) fn create_request(body: &Value) -> Option<CreateShare> {
    let object = body.as_object()?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "node_id" | "expires_in" | "password"))
    {
        return None;
    }
    let optional = |name: &str| object.get(name).filter(|value| !value.is_null());
    let expires_in = match optional("expires_in") {
        Some(seconds) => Some(Duration::from_secs(seconds.as_u64()?)),
        None => None,
    };
    let password = match optional("password") {
        Some(password) => Some(password.as_str()?.to_owned()),
        None => None,
    };
    Some(CreateShare {
        node_id: Uuid::parse_str(object.get("node_id")?.as_str()?).ok()?,
        expires_in,
        password,
    })
}

/// The one place the plaintext token leaves 06 for a client.
pub(super) fn created_share(created: &CreatedShare) -> Value {
    json!({
        "id": created.id,
        "token": created.token.expose(),
        "url_path": created.url_path,
        "expires_at": rfc3339(created.expires_at),
        "has_password": created.has_password,
    })
}

/// A listing never carries a token.
pub(super) fn share_list(shares: &[ShareInfo]) -> Value {
    let items: Vec<Value> = shares
        .iter()
        .map(|share| {
            json!({
                "id": share.id,
                "node_id": share.node_id,
                "expires_at": rfc3339(share.expires_at),
                "has_password": share.has_password,
                "created_at": rfc3339(share.created_at),
            })
        })
        .collect();
    json!({"items": items})
}

/// What a visitor may learn of an available link: the name, size and type of the file, and when the link ends.
pub(super) fn share_open(
    name: &str,
    size: i64,
    media_type: Option<&str>,
    expires_at: Timestamp,
) -> Value {
    let mut object = Map::new();
    object.insert("name".to_owned(), json!(name));
    object.insert("size".to_owned(), json!(size));
    object.insert("media_type".to_owned(), json!(media_type));
    object.insert("expires_at".to_owned(), json!(rfc3339(expires_at)));
    Value::Object(object)
}

/// `{password}`: the password to check.
pub(super) fn password(body: &Value) -> Option<&str> {
    let [password] = crate::json_body::fields(body, ["password"])?;
    password.as_str()
}

pub(super) const CREATE_REQUEST: SchemaRef = SchemaRef {
    name: "ShareCreateRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["node_id"],
            "additionalProperties": false,
            "properties": {
                "node_id": {"type": "string", "format": "uuid"},
                "expires_in": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Seconds the link lives; the default and the longest are 06's."
                },
                "password": {"type": "string"}
            }
        })
    },
};

pub(super) const CREATED_SHARE: SchemaRef = SchemaRef {
    name: "CreatedShare",
    build: || {
        json!({
            "type": "object",
            "required": ["id", "token", "url_path", "expires_at", "has_password"],
            "properties": {
                "id": {"type": "string", "format": "uuid"},
                "token": {"type": "string", "description": "Shown once; no other answer carries it."},
                "url_path": {"type": "string"},
                "expires_at": {"type": "string", "format": "date-time"},
                "has_password": {"type": "boolean"}
            }
        })
    },
};

pub(super) const SHARE_LIST: SchemaRef = SchemaRef {
    name: "ShareList",
    build: || {
        json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "node_id", "expires_at", "has_password", "created_at"],
                        "properties": {
                            "id": {"type": "string", "format": "uuid"},
                            "node_id": {"type": "string", "format": "uuid"},
                            "expires_at": {"type": "string", "format": "date-time"},
                            "has_password": {"type": "boolean"},
                            "created_at": {"type": "string", "format": "date-time"}
                        }
                    }
                }
            }
        })
    },
};

pub(super) const SHARE_OPEN: SchemaRef = SchemaRef {
    name: "ShareOpen",
    build: || {
        json!({
            "type": "object",
            "required": ["name", "size", "media_type", "expires_at"],
            "properties": {
                "name": {"type": "string"},
                "size": {"type": "integer", "minimum": 0},
                "media_type": {"type": ["string", "null"]},
                "expires_at": {"type": "string", "format": "date-time"}
            }
        })
    },
};

pub(super) const PASSWORD_REQUEST: SchemaRef = SchemaRef {
    name: "SharePasswordRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["password"],
            "additionalProperties": false,
            "properties": {"password": {"type": "string"}}
        })
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    const NODE: &str = "00000000-0000-0000-0000-0000000000a1";

    #[test]
    fn a_create_body_with_only_the_node_has_the_defaults_of_06() {
        // Arrange
        let body = json!({"node_id": NODE});

        // Act
        let request = create_request(&body).expect("a request");

        // Assert
        assert_eq!(request.node_id, Uuid::from_u128(0xA1));
        assert_eq!(request.expires_in, None);
        assert_eq!(request.password, None);
    }

    #[test]
    fn a_create_body_carries_the_expiry_in_seconds_and_the_password() {
        // Arrange
        let body = json!({"node_id": NODE, "expires_in": 3600, "password": "pw", "extra": null});
        let clean = json!({"node_id": NODE, "expires_in": 3600, "password": "pw"});

        // Act
        let request = create_request(&clean).expect("a request");
        let with_unknown_field = create_request(&body);

        // Assert
        assert_eq!(request.expires_in, Some(Duration::from_secs(3600)));
        assert_eq!(request.password.as_deref(), Some("pw"));
        assert!(with_unknown_field.is_none());
    }

    #[test]
    fn a_create_body_of_the_wrong_shape_is_refused() {
        // Arrange
        let bodies = [
            json!({}),
            json!({"node_id": "7"}),
            json!({"node_id": NODE, "expires_in": -1}),
            json!({"node_id": NODE, "expires_in": "soon"}),
            json!({"node_id": NODE, "password": 5}),
            json!([NODE]),
        ];

        // Act
        let found: Vec<bool> = bodies
            .iter()
            .map(|body| create_request(body).is_some())
            .collect();

        // Assert
        assert_eq!(found, [false; 6]);
    }
}
