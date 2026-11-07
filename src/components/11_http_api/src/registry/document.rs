use std::collections::{BTreeMap, BTreeSet};

use axum::body::Bytes;
use sc_error::ErrorCode;
use serde_json::{Map, Value, json};

use super::classes::AuthClass;
use super::spec::{Param, ParamLocation, RouteDef, RouteSpec, SchemaRef};
use crate::problem_schema::problem_schema;

const PROBLEM_JSON: &str = "application/problem+json";
const PROBLEM_REF: &str = "#/components/schemas/Problem";
const JSON: &str = "application/json";

/// The OpenAPI document as served by `GET /api/v1/openapi.json`: the same bytes every time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApiDocument(Bytes);

impl OpenApiDocument {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn to_body(&self) -> Bytes {
        self.0.clone()
    }
}

/// The document of `defs`, in canonical order: paths sorted, operations sorted by method, keys sorted.
pub fn document(defs: &[RouteDef]) -> OpenApiDocument {
    let mut paths: BTreeMap<&str, Map<String, Value>> = BTreeMap::new();
    let mut schemas: BTreeMap<&str, Value> = BTreeMap::new();
    schemas.insert("Problem", problem_schema());
    for def in defs {
        let spec = &def.spec;
        for schema in [spec.request, spec.response].into_iter().flatten() {
            schemas.insert(schema.name, (schema.build)());
        }
        paths
            .entry(spec.path)
            .or_default()
            .insert(spec.method.key().to_owned(), operation(spec));
    }
    let paths: Map<String, Value> = paths
        .into_iter()
        .map(|(path, item)| (path.to_owned(), Value::Object(item)))
        .collect();
    let schemas: Map<String, Value> = schemas
        .into_iter()
        .map(|(name, schema)| (name.to_owned(), schema))
        .collect();
    let value = json!({
        "openapi": "3.1.0",
        "info": {"title": "sunnycloud", "version": env!("CARGO_PKG_VERSION")},
        "paths": paths,
        "components": {"schemas": schemas, "securitySchemes": security_schemes()},
    });
    OpenApiDocument(Bytes::from(value.to_string().into_bytes()))
}

fn security_schemes() -> Value {
    json!({
        "bearerDevice": {
            "type": "http",
            "scheme": "bearer",
            "description": "Device token of the desktop client."
        },
        "bearerSession": {
            "type": "http",
            "scheme": "bearer",
            "description": "Session access token of a native client."
        },
        "bearerWorker": {
            "type": "http",
            "scheme": "bearer",
            "description": "Worker API token, internal listener only; sent with X-Worker-Id."
        },
        "cookieSession": {
            "type": "apiKey",
            "in": "cookie",
            "name": "__Host-session",
            "description": "Browser session cookie."
        }
    })
}

fn operation(spec: &RouteSpec) -> Value {
    let id_params: Vec<Value> = spec
        .params
        .iter()
        .filter(|param| param.id)
        .map(|param| json!({"name": param.name, "in": param.location.key()}))
        .collect();
    let mut operation = json!({
        "operationId": operation_id(spec),
        "summary": spec.summary,
        "parameters": spec.params.iter().filter(|p| p.location != ParamLocation::Body).map(parameter).collect::<Vec<_>>(),
        "responses": responses(spec),
        "security": security(spec.auth),
        "x-auth-class": spec.auth.name(),
        "x-body-class": spec.body.name(),
        "x-state-changing": spec.state_changing,
        "x-owner-method": spec.owner,
        "x-id-params": id_params,
        "x-listener": spec.listener.name(),
    });
    if let Some(body) = request_body(spec)
        && let Some(object) = operation.as_object_mut()
    {
        object.insert("requestBody".to_owned(), body);
    }
    operation
}

fn operation_id(spec: &RouteSpec) -> String {
    let mut id = spec.method.key().to_owned();
    for segment in spec.path.trim_start_matches("/api/v1/").split('/') {
        id.push('_');
        id.extend(
            segment
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }),
        );
    }
    id
}

fn parameter(param: &Param) -> Value {
    json!({
        "name": param.name,
        "in": param.location.key(),
        "required": param.required,
        "schema": param.ty.schema(),
    })
}

fn schema_ref(schema: SchemaRef) -> Value {
    json!({"$ref": format!("#/components/schemas/{}", schema.name)})
}

fn request_body(spec: &RouteSpec) -> Option<Value> {
    if spec.body == super::classes::BodyClass::None {
        return None;
    }
    let media = spec.body.media_type();
    let schema = match (spec.request, media) {
        (Some(schema), _) => schema_ref(schema),
        (None, JSON) => json!({}),
        (None, _) => json!({"type": "string", "format": "binary"}),
    };
    Some(json!({"required": true, "content": {media: {"schema": schema}}}))
}

fn security(auth: AuthClass) -> Value {
    Value::Array(
        auth.security_schemes()
            .iter()
            .map(|scheme| json!({*scheme: []}))
            .collect(),
    )
}

fn responses(spec: &RouteSpec) -> Value {
    let mut responses = Map::new();
    let mut success = json!({"description": "Success"});
    if let (Some(schema), Some(object)) = (spec.response, success.as_object_mut()) {
        object.insert(
            "content".to_owned(),
            json!({JSON: {"schema": schema_ref(schema)}}),
        );
    }
    responses.insert(spec.success.to_string(), success);

    let mut codes: BTreeSet<&str> = BTreeSet::new();
    let mut by_status: BTreeMap<u16, Vec<ErrorCode>> = BTreeMap::new();
    let implied = spec
        .auth
        .implied_errors()
        .iter()
        .chain(spec.body.implied_errors());
    for code in spec
        .errors
        .iter()
        .chain(implied)
        .chain([&ErrorCode::Internal])
    {
        if codes.insert(code.as_str()) {
            by_status.entry(code.status()).or_default().push(*code);
        }
    }
    for (status, mut group) in by_status {
        group.sort_by_key(|code| code.as_str());
        let titles: Vec<&str> = group.iter().map(|code| code.title()).collect();
        let wire: Vec<&str> = group.iter().map(|code| code.as_str()).collect();
        responses.insert(
            status.to_string(),
            json!({
                "description": titles.join(", "),
                "content": {PROBLEM_JSON: {"schema": {"$ref": PROBLEM_REF}}},
                "x-error-codes": wire,
            }),
        );
    }
    Value::Object(responses)
}
