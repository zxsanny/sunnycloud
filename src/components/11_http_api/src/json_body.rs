//! Reading a small JSON request body into a [`Value`] and checking its shape, for the handlers whose bodies are a
//! few fixed fields.

use axum::body::{Body, to_bytes};
use sc_clock::Timestamp;
use sc_error::CorrelationId;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::problem::ApiProblem;
use crate::timestamp::parse_rfc3339;

const FIELD: &str = "body";

/// The body as JSON. `limit` bounds the read; a failed read or non-JSON text is a 400.
pub(crate) async fn read(
    body: Body,
    limit: usize,
    request_id: CorrelationId,
) -> Result<Value, ApiProblem> {
    let bytes = to_bytes(body, limit)
        .await
        .map_err(|_| ApiProblem::validation(FIELD, "unreadable", request_id))?;
    serde_json::from_slice(&bytes).map_err(|_| malformed(request_id))
}

pub(crate) fn malformed(request_id: CorrelationId) -> ApiProblem {
    ApiProblem::validation(FIELD, "malformed", request_id)
}

/// The values of exactly the fields `names`, in order; `None` when `value` is not an object with just those fields.
pub(crate) fn fields<'a, const N: usize>(
    value: &'a Value,
    names: [&str; N],
) -> Option<[&'a Value; N]> {
    let object = value.as_object()?;
    let found: Vec<&Value> = names.iter().filter_map(|name| object.get(*name)).collect();
    if object.len() != N {
        return None;
    }
    found.try_into().ok()
}

/// The members of `value` when it is an object whose every key is one of `allowed`.
pub(crate) fn object<'a>(value: &'a Value, allowed: &[&str]) -> Option<&'a Map<String, Value>> {
    let members = value.as_object()?;
    members
        .keys()
        .all(|key| allowed.contains(&key.as_str()))
        .then_some(members)
}

/// A required member, read with `parse`; `None` when absent, null or of the wrong shape.
pub(crate) fn required<T>(
    members: &Map<String, Value>,
    name: &str,
    parse: impl FnOnce(&Value) -> Option<T>,
) -> Option<T> {
    members
        .get(name)
        .filter(|value| !value.is_null())
        .and_then(parse)
}

/// An optional member, read with `parse`: absent and null are `Some(None)`, a member of the wrong shape is
/// `None` (a malformed body).
pub(crate) fn optional<T>(
    members: &Map<String, Value>,
    name: &str,
    parse: impl FnOnce(&Value) -> Option<T>,
) -> Option<Option<T>> {
    match members.get(name).filter(|value| !value.is_null()) {
        Some(value) => parse(value).map(Some),
        None => Some(None),
    }
}

pub(crate) fn uuid(value: &Value) -> Option<Uuid> {
    Uuid::parse_str(value.as_str()?).ok()
}

pub(crate) fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// A `client_mtime` member: `YYYY-MM-DDTHH:MM:SS[.ffffff]Z`.
pub(crate) fn timestamp(value: &Value) -> Option<Timestamp> {
    parse_rfc3339(value.as_str()?)
}

/// A `client_meta` member: any JSON value, stored by the ports as its text.
pub(crate) fn meta_text(value: &Value) -> Option<String> {
    Some(value.to_string())
}

/// The JSON of the `client_meta` text a port returns; text that is not JSON is the port's own fault.
pub(crate) fn meta_value(text: Option<&str>) -> Result<Value, serde_json::Error> {
    text.map_or(Ok(Value::Null), serde_json::from_str)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn fields_returns_the_values_of_exactly_the_named_fields_in_order() {
        // Arrange
        let body = json!({"b": 2, "a": 1});

        // Act
        let found = fields(&body, ["a", "b"]);

        // Assert
        assert_eq!(found, Some([&json!(1), &json!(2)]));
    }

    #[test]
    fn fields_refuses_a_missing_an_extra_or_a_non_object_body() {
        // Arrange
        let missing = json!({"a": 1});
        let extra = json!({"a": 1, "b": 2, "c": 3});
        let wrong_name = json!({"a": 1, "c": 3});
        let array = json!([1, 2]);

        // Act
        let found = [&missing, &extra, &wrong_name, &array].map(|body| fields(body, ["a", "b"]));

        // Assert
        assert_eq!(found, [None, None, None, None]);
    }
}
