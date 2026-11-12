//! Requests built from the document's schemas: the required parameters, headers and body members only, plus the
//! ID parameter a case is about. Every ID parameter takes the value the caller gives it; every other value is the
//! smallest one its schema allows.

use serde_json::{Map, Value, json};

use super::document::{Document, IdParam, Operation};

/// A UUID-formatted member that is not an ID parameter (an idempotency key such as `commit_id`).
const PLAIN_UUID: &str = "00000000-0000-4000-8000-0000000000c1";
const DATE_TIME: &str = "2026-11-12T08:00:00.000000Z";

#[derive(Debug, Clone)]
pub enum Payload {
    Empty,
    Json(Value),
    Binary(Vec<u8>),
}

/// One request, ready to send to the listener its operation is on.
#[derive(Debug, Clone)]
pub struct Built {
    pub internal: bool,
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub payload: Payload,
}

impl Built {
    /// The same request with one more top-level body member; `None` when the body is no JSON object.
    pub fn with_member(&self, name: &str, value: Value) -> Option<Built> {
        let Payload::Json(Value::Object(members)) = &self.payload else {
            return None;
        };
        let mut members = members.clone();
        members.insert(name.to_owned(), value);
        Some(Built {
            payload: Payload::Json(Value::Object(members)),
            ..self.clone()
        })
    }

    pub fn body(&self) -> (Option<&'static str>, Vec<u8>) {
        match &self.payload {
            Payload::Empty => (None, Vec::new()),
            Payload::Json(value) => (Some("application/json"), value.to_string().into_bytes()),
            Payload::Binary(bytes) => (Some("application/octet-stream"), bytes.clone()),
        }
    }
}

/// Where each ID parameter of `operation` sits; a problem names the operation and the parameter that cannot be
/// placed.
pub fn check_placement(doc: &Document, operation: &Operation) -> Vec<String> {
    operation
        .id_params
        .iter()
        .filter(|param| !placeable(doc, operation, param))
        .map(|param| {
            format!(
                "{}: ID parameter {} ({}) is not in its {}",
                operation.label(),
                param.name,
                param.location,
                if param.location == "body" {
                    "request schema"
                } else {
                    "parameters"
                }
            )
        })
        .collect()
}

fn placeable(doc: &Document, operation: &Operation, param: &IdParam) -> bool {
    match param.location.as_str() {
        "path" => {
            operation.path.contains(&format!("{{{}}}", param.name))
                && operation.param(&param.name, "path").is_some()
        }
        "query" | "header" => operation.param(&param.name, &param.location).is_some(),
        "body" => operation
            .body
            .as_ref()
            .is_some_and(|body| doc.has_property(&body.schema, &param.name)),
        _ => false,
    }
}

/// The request for `operation`: `focus`, when given, is placed even where it is optional; `id` gives the value
/// of every ID parameter placed.
pub fn build(
    doc: &Document,
    operation: &Operation,
    focus: Option<&IdParam>,
    id: &mut dyn FnMut(&IdParam) -> String,
) -> Result<Built, String> {
    let label = operation.label();
    let mut target = String::new();
    for segment in operation.path.split('/').skip(1) {
        target.push('/');
        match segment
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
        {
            Some(name) => target.push_str(&parameter_value(operation, name, "path", id, &label)?),
            None => target.push_str(segment),
        }
    }
    let mut query = Vec::new();
    let mut headers = Vec::new();
    for param in &operation.params {
        let is_focus = focus.is_some_and(|f| f.name == param.name && f.location == param.location);
        if param.location == "path" || !(param.required || is_focus) {
            continue;
        }
        let value = parameter_value(operation, &param.name, &param.location, id, &label)?;
        match param.location.as_str() {
            "query" => query.push(format!("{}={value}", param.name)),
            "header" => headers.push((param.name.clone(), value)),
            other => return Err(format!("{label}: parameter in {other}")),
        }
    }
    if !query.is_empty() {
        target.push('?');
        target.push_str(&query.join("&"));
    }
    let payload = match &operation.body {
        None => Payload::Empty,
        Some(body) if body.media_type == "application/json" => {
            let body_ids: Vec<&IdParam> = operation
                .id_params
                .iter()
                .filter(|param| param.location == "body")
                .collect();
            let mut filler = Filler {
                doc,
                body_ids: &body_ids,
                focus: focus.filter(|f| f.location == "body"),
                id,
            };
            Payload::Json(filler.instance(&body.schema))
        }
        Some(_) => Payload::Binary(Vec::new()),
    };
    Ok(Built {
        internal: operation.is_internal(),
        method: operation.method.clone(),
        target,
        headers,
        payload,
    })
}

fn parameter_value(
    operation: &Operation,
    name: &str,
    location: &str,
    id: &mut dyn FnMut(&IdParam) -> String,
    label: &str,
) -> Result<String, String> {
    if let Some(param) = operation
        .id_params
        .iter()
        .find(|param| param.name == name && param.location == location)
    {
        return Ok(id(param));
    }
    let decl = operation
        .param(name, location)
        .ok_or(format!("{label}: {name} ({location}) is not declared"))?;
    Ok(match scalar(&decl.schema) {
        Value::String(text) => text,
        other => other.to_string(),
    })
}

/// The first type of `schema` that is not `null`.
fn type_of(schema: &Value) -> Option<&str> {
    match &schema["type"] {
        Value::String(name) => Some(name.as_str()),
        Value::Array(names) => names
            .iter()
            .filter_map(Value::as_str)
            .find(|n| *n != "null"),
        _ => None,
    }
}

fn scalar(schema: &Value) -> Value {
    if let Some(first) = schema["enum"]
        .as_array()
        .and_then(|values| values.iter().find(|value| !value.is_null()))
    {
        return first.clone();
    }
    match type_of(schema) {
        Some("string") => match schema["format"].as_str() {
            Some("uuid") => json!(PLAIN_UUID),
            Some("date-time") => json!(DATE_TIME),
            _ => json!("x"),
        },
        Some("integer" | "number") => schema.get("minimum").cloned().unwrap_or(json!(0)),
        Some("boolean") => json!(false),
        _ => Value::Null,
    }
}

struct Filler<'a> {
    doc: &'a Document,
    body_ids: &'a [&'a IdParam],
    focus: Option<&'a IdParam>,
    id: &'a mut dyn FnMut(&IdParam) -> String,
}

impl Filler<'_> {
    fn instance(&mut self, schema: &Value) -> Value {
        let schema = self.doc.resolve(schema).clone();
        match type_of(&schema) {
            Some("object") => self.object(&schema),
            Some("array") => match schema.get("items") {
                Some(items) if type_of(self.doc.resolve(items)) == Some("object") => {
                    Value::Array(vec![self.instance(items)])
                }
                _ => Value::Array(Vec::new()),
            },
            _ => scalar(&schema),
        }
    }

    fn object(&mut self, schema: &Value) -> Value {
        let required: Vec<&str> = schema["required"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let mut members = Map::new();
        let Some(properties) = schema["properties"].as_object() else {
            return Value::Object(members);
        };
        for (name, child) in properties {
            let is_focus = self.focus.is_some_and(|focus| focus.name == *name);
            let leads_to_focus = self
                .focus
                .is_some_and(|focus| self.doc.has_property(child, &focus.name));
            if !(required.contains(&name.as_str()) || is_focus || leads_to_focus) {
                continue;
            }
            let value = match self.body_ids.iter().find(|param| param.name == *name) {
                Some(param) => Value::String((self.id)(param)),
                None => self.instance(child),
            };
            members.insert(name.clone(), value);
        }
        Value::Object(members)
    }
}
