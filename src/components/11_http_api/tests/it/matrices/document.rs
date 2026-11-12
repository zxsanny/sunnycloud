//! What the two generators read from the generated OpenAPI document (`api_v1_surface.md`): every operation with
//! its auth class, state-changing flag, owner port method, `x-id-params`, parameters and request body schema.
//! Nothing here lists an operation: the list is the document's.

use serde_json::Value;
use sunnycloud_http_api::registry::{RouteDef, document, production_routes};

/// The cookie scheme of the document's `securitySchemes`: an operation that lists it accepts a browser session.
pub const COOKIE_SCHEME: &str = "cookieSession";

const UNSAFE_METHODS: [&str; 4] = ["POST", "PUT", "PATCH", "DELETE"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdParam {
    pub name: String,
    pub location: String,
}

#[derive(Debug, Clone)]
pub struct ParamDecl {
    pub name: String,
    pub location: String,
    pub required: bool,
    pub schema: Value,
}

#[derive(Debug, Clone)]
pub struct BodyDecl {
    pub media_type: String,
    pub schema: Value,
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub method: String,
    pub path: String,
    pub auth: String,
    pub state_changing: bool,
    pub owner: String,
    pub listener: String,
    pub security: Vec<String>,
    pub id_params: Vec<IdParam>,
    pub params: Vec<ParamDecl>,
    pub body: Option<BodyDecl>,
}

impl Operation {
    pub fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }

    pub fn accepts_cookie(&self) -> bool {
        self.security.iter().any(|scheme| scheme == COOKIE_SCHEME)
    }

    /// The CSRF layer decides by method, not by the state-changing flag (`api_v1_surface.md`, CSRF row).
    pub fn unsafe_method(&self) -> bool {
        UNSAFE_METHODS.contains(&self.method.as_str())
    }

    pub fn is_read(&self) -> bool {
        self.method == "GET" || self.method == "HEAD"
    }

    pub fn is_internal(&self) -> bool {
        self.listener == "internal"
    }

    pub fn param(&self, name: &str, location: &str) -> Option<&ParamDecl> {
        self.params
            .iter()
            .find(|param| param.name == name && param.location == location)
    }
}

/// The parsed document: its operations in document order, and its schemas for `$ref` resolution.
pub struct Document {
    pub operations: Vec<Operation>,
    schemas: Value,
}

impl Document {
    /// The document `HttpServer::openapi()` serves for the release registry.
    pub fn production() -> Document {
        Document::of(&production_routes())
    }

    /// The document generated from `defs`.
    pub fn of(defs: &[RouteDef]) -> Document {
        Document::parse(document(defs).as_bytes()).expect("the generated document parses")
    }

    pub fn parse(bytes: &[u8]) -> Result<Document, String> {
        let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        let paths = value["paths"]
            .as_object()
            .ok_or("the document has no paths")?;
        let mut operations = Vec::new();
        for (path, item) in paths {
            let item = item.as_object().ok_or(format!("{path} is no path item"))?;
            for (method, operation) in item {
                operations.push(operation_of(method, path, operation)?);
            }
        }
        Ok(Document {
            operations,
            schemas: value["components"]["schemas"].clone(),
        })
    }

    /// `schema` with a top-level `$ref` into `components.schemas` followed.
    pub fn resolve<'a>(&'a self, schema: &'a Value) -> &'a Value {
        match schema["$ref"].as_str() {
            Some(reference) => {
                let name = reference.trim_start_matches("#/components/schemas/");
                &self.schemas[name]
            }
            None => schema,
        }
    }

    /// Whether a property `name` exists anywhere in `schema` (nested objects and array items included).
    pub fn has_property(&self, schema: &Value, name: &str) -> bool {
        let schema = self.resolve(schema);
        if let Some(properties) = schema["properties"].as_object() {
            if properties.contains_key(name) {
                return true;
            }
            if properties
                .values()
                .any(|child| self.has_property(child, name))
            {
                return true;
            }
        }
        schema
            .get("items")
            .is_some_and(|items| self.has_property(items, name))
    }
}

fn text(value: &Value, what: &str, label: &str) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or(format!("{label} declares no {what}"))
}

fn operation_of(method: &str, path: &str, operation: &Value) -> Result<Operation, String> {
    let label = format!("{} {path}", method.to_uppercase());
    let id_params = operation["x-id-params"]
        .as_array()
        .ok_or(format!("{label} declares no x-id-params"))?
        .iter()
        .map(|entry| {
            Ok(IdParam {
                name: text(&entry["name"], "ID parameter name", &label)?,
                location: text(&entry["in"], "ID parameter location", &label)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let params = operation["parameters"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            Ok(ParamDecl {
                name: text(&entry["name"], "parameter name", &label)?,
                location: text(&entry["in"], "parameter location", &label)?,
                required: entry["required"].as_bool().unwrap_or(false),
                schema: entry["schema"].clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let body = operation["requestBody"]["content"]
        .as_object()
        .and_then(|content| content.iter().next())
        .map(|(media_type, entry)| BodyDecl {
            media_type: media_type.clone(),
            schema: entry["schema"].clone(),
        });
    let security = operation["security"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|requirement| requirement.keys().cloned())
        .collect();
    Ok(Operation {
        method: method.to_uppercase(),
        path: path.to_owned(),
        auth: text(&operation["x-auth-class"], "x-auth-class", &label)?,
        state_changing: operation["x-state-changing"]
            .as_bool()
            .ok_or(format!("{label} declares no x-state-changing"))?,
        owner: text(&operation["x-owner-method"], "x-owner-method", &label)?,
        listener: text(&operation["x-listener"], "x-listener", &label)?,
        security,
        id_params,
        params,
        body,
    })
}
