//! The wire of the vault routes (`vault_http_api.md`): JSON bodies whose `header` is the opaque header in
//! standard base64 with padding, and the `ETag` and `If-Match` headers as a quoted lowercase hex SHA-256.

use axum::http::header::IF_MATCH;
use axum::http::{HeaderMap, HeaderValue};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use contracts::{ETag, VaultHeader};
use sc_error::CorrelationId;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::json_body::{self, malformed};
use crate::problem::ApiProblem;
use crate::registry::SchemaRef;

const HEADER: &str = "header";
const ROOT: &str = "root_node_id";

/// `{root_node_id, header}` of the create route.
pub(super) struct CreateVault {
    pub(super) root_node_id: Uuid,
    pub(super) header: Vec<u8>,
}

/// `{root_node_id: uuid, header: base64}`; an unknown, missing or mistyped member is a 400 `body`, a `header`
/// that is not base64 or decodes to no byte a 400 `header`.
pub(super) fn create_request(
    body: &Value,
    request_id: CorrelationId,
) -> Result<CreateVault, ApiProblem> {
    let members = json_body::object(body, &[ROOT, HEADER]).ok_or_else(|| malformed(request_id))?;
    let root_node_id =
        json_body::required(members, ROOT, json_body::uuid).ok_or_else(|| malformed(request_id))?;
    let text = json_body::required(members, HEADER, json_body::text)
        .ok_or_else(|| malformed(request_id))?;
    Ok(CreateVault {
        root_node_id,
        header: decode(&text, request_id)?,
    })
}

/// `{header: base64}` of the replace route, read as [`create_request`] reads its `header`.
pub(super) fn replace_request(
    body: &Value,
    request_id: CorrelationId,
) -> Result<Vec<u8>, ApiProblem> {
    let members = json_body::object(body, &[HEADER]).ok_or_else(|| malformed(request_id))?;
    let text = json_body::required(members, HEADER, json_body::text)
        .ok_or_else(|| malformed(request_id))?;
    decode(&text, request_id)
}

/// The header bytes: 1 or more (the contract's lower bound); the upper bound is 20's `HeaderTooLarge`.
fn decode(text: &str, request_id: CorrelationId) -> Result<Vec<u8>, ApiProblem> {
    let bytes = STANDARD
        .decode(text)
        .map_err(|_| ApiProblem::validation(HEADER, "not_base64", request_id))?;
    if bytes.is_empty() {
        return Err(ApiProblem::validation(HEADER, "empty", request_id));
    }
    Ok(bytes)
}

/// The `If-Match` of a replace. `None` only when the request has no `If-Match` line (the 428 path). One line
/// holding exactly one quoted entity-tag gives the text between the quotes; every other present value (empty,
/// unquoted, weak `W/"…"`, `*`, a list, several lines, not visible ASCII) gives an empty tag. An empty tag
/// equals no stored tag (64 hex digits), so 20 answers `PreconditionFailed` (412) and nothing is written.
pub(super) fn if_match(headers: &HeaderMap) -> Option<ETag> {
    let mut lines = headers.get_all(IF_MATCH).iter();
    let first = lines.next()?;
    if lines.next().is_some() {
        return Some(ETag(String::new()));
    }
    Some(ETag(quoted(first).unwrap_or_default()))
}

fn quoted(value: &HeaderValue) -> Option<String> {
    let inner = value.to_str().ok()?.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains('"')).then(|| inner.to_owned())
}

/// The `ETag` response value: the port's hex digest, quoted (a strong tag).
pub(super) fn etag_value(etag: &ETag) -> String {
    format!("\"{}\"", etag.0)
}

pub(super) fn created(vault_id: Uuid) -> Value {
    json!({"id": vault_id})
}

pub(super) fn vault(vault_id: Uuid, found: &VaultHeader) -> Value {
    json!({
        "id": vault_id,
        "root_node_id": found.root_node_id,
        "header": STANDARD.encode(&found.header),
    })
}

fn header_schema() -> Value {
    json!({"type": "string", "format": "byte", "minLength": 4})
}

pub(super) const CREATE_REQUEST: SchemaRef = SchemaRef {
    name: "VaultCreateRequest",
    build: || {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": [ROOT, HEADER],
            "properties": {
                ROOT: {"type": "string", "format": "uuid"},
                HEADER: header_schema()
            }
        })
    },
};

pub(super) const REPLACE_REQUEST: SchemaRef = SchemaRef {
    name: "VaultHeaderReplaceRequest",
    build: || {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": [HEADER],
            "properties": {HEADER: header_schema()}
        })
    },
};

pub(super) const CREATED: SchemaRef = SchemaRef {
    name: "VaultCreated",
    build: || {
        json!({
            "type": "object",
            "required": ["id"],
            "properties": {"id": {"type": "string", "format": "uuid"}}
        })
    },
};

pub(super) const VAULT: SchemaRef = SchemaRef {
    name: "Vault",
    build: || {
        json!({
            "type": "object",
            "required": ["id", ROOT, HEADER],
            "properties": {
                "id": {"type": "string", "format": "uuid"},
                ROOT: {"type": "string", "format": "uuid"},
                HEADER: header_schema()
            }
        })
    },
};

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn with_if_match(values: &[&'static str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(IF_MATCH, HeaderValue::from_static(value));
        }
        headers
    }

    const HEX: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    #[test]
    fn if_match_absent_is_the_428_path() {
        // Arrange
        let headers = HeaderMap::new();

        // Act
        let tag = if_match(&headers);

        // Assert
        assert_eq!(tag, None);
    }

    #[test]
    fn if_match_quoted_tag_reaches_the_port_without_its_quotes() {
        // Arrange
        let headers = with_if_match(&[
            "\"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\"",
        ]);

        // Act
        let tag = if_match(&headers);

        // Assert
        assert_eq!(tag, Some(ETag(HEX.to_owned())));
    }

    #[test]
    fn if_match_empty_or_malformed_reaches_the_port_as_a_tag_that_matches_nothing() {
        // Arrange
        let cases: [&[&'static str]; 7] = [
            &[""],
            &["9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"],
            &["W/\"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\""],
            &["*"],
            &["\"a\", \"b\""],
            &["\""],
            &["\"a\"", "\"a\""],
        ];

        // Act
        let tags: Vec<Option<ETag>> = cases
            .iter()
            .map(|values| if_match(&with_if_match(values)))
            .collect();

        // Assert
        assert_eq!(tags, vec![Some(ETag(String::new())); cases.len()]);
    }

    #[test]
    fn etag_value_is_the_hex_digest_in_quotes() {
        // Arrange
        let etag = ETag(HEX.to_owned());

        // Act
        let value = etag_value(&etag);

        // Assert
        assert_eq!(value, format!("\"{HEX}\""));
    }

    #[test]
    fn create_request_decodes_standard_base64_and_refuses_other_text() {
        // Arrange
        let root = Uuid::from_u128(7);
        let id = CorrelationId::new(Uuid::from_u128(1));
        let good = json!({"root_node_id": root, "header": "AAEC/w=="});
        let not_base64 = json!({"root_node_id": root, "header": "AAEC_w"});
        let empty = json!({"root_node_id": root, "header": ""});
        let extra = json!({"root_node_id": root, "header": "AA==", "account_id": root});

        // Act
        let decoded =
            create_request(&good, id).map(|request| (request.root_node_id, request.header));
        let refused = [&not_base64, &empty, &extra].map(|body| {
            create_request(body, id)
                .err()
                .map(|problem| format!("{problem:?}"))
        });

        // Assert
        assert_eq!(decoded.ok(), Some((root, vec![0, 1, 2, 255])));
        let expected = [
            "field: \"header\", problem: \"not_base64\"",
            "field: \"header\", problem: \"empty\"",
            "field: \"body\", problem: \"malformed\"",
        ];
        for (found, expected) in refused.iter().zip(expected) {
            assert!(
                found.as_deref().is_some_and(|text| text.contains(expected)),
                "{found:?}"
            );
        }
    }
}
