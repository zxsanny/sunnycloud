//! The additive-only gate between two OpenAPI documents (`api_v1_surface.md`, versioning rules): a removed
//! operation, a tightened auth or body class, a removed field or enum value, a retyped field and a newly
//! required request field or parameter are breaking; a new operation and a new optional field are not.
//! The comparison is structural, never a text diff.

use std::collections::BTreeSet;
use std::fmt;

use serde_json::Value;

use crate::registry::{AuthClass, BodyClass, HttpMethod};

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

/// One incompatible change, named by the operation it hits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breaking {
    pub operation: String,
    pub reason: String,
}

impl fmt::Display for Breaking {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.operation, self.reason)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Request,
    Response,
}

/// Every breaking change from `old` (the last release) to `new`; empty when `new` only adds.
pub fn diff(old: &Value, new: &Value) -> Vec<Breaking> {
    let mut diff = Diff {
        old,
        new,
        found: Vec::new(),
        label: String::new(),
        visited: BTreeSet::new(),
    };
    diff.operations();
    diff.found
}

struct Diff<'a> {
    old: &'a Value,
    new: &'a Value,
    found: Vec<Breaking>,
    label: String,
    visited: BTreeSet<(String, String, bool)>,
}

fn entries(value: &Value) -> impl Iterator<Item = (&String, &Value)> {
    value.as_object().into_iter().flatten()
}

fn text(value: &Value) -> Option<&str> {
    value.as_str()
}

impl Diff<'_> {
    fn breaking(&mut self, reason: String) {
        self.found.push(Breaking {
            operation: self.label.clone(),
            reason,
        });
    }

    fn operations(&mut self) {
        for (path, item) in entries(&self.old["paths"]) {
            for (key, old_operation) in entries(item) {
                let Some(method) = HttpMethod::from_key(key) else {
                    continue;
                };
                self.label = format!("{} {path}", method.key().to_uppercase());
                match self.new["paths"][path.as_str()].get(key.as_str()) {
                    None => self.breaking("operation removed".to_owned()),
                    Some(new_operation) => self.operation(old_operation, new_operation),
                }
            }
        }
    }

    fn operation(&mut self, old: &Value, new: &Value) {
        self.visited.clear();
        self.classes(old, new);
        self.parameters(old, new);
        self.request_body(old, new);
        self.responses(old, new);
    }

    fn classes(&mut self, old: &Value, new: &Value) {
        let (old_auth, new_auth) = (text(&old["x-auth-class"]), text(&new["x-auth-class"]));
        if old_auth != new_auth {
            let loosened = matches!(
                (old_auth.and_then(AuthClass::from_name), new_auth.and_then(AuthClass::from_name)),
                (Some(before), Some(after)) if after.loosens(before)
            );
            if !loosened {
                self.breaking(format!(
                    "auth class changed from {old_auth:?} to {new_auth:?}"
                ));
            }
        }
        let (old_body, new_body) = (text(&old["x-body-class"]), text(&new["x-body-class"]));
        if old_body != new_body {
            let loosened = matches!(
                (old_body.and_then(BodyClass::from_name), new_body.and_then(BodyClass::from_name)),
                (Some(before), Some(after)) if after.limit_bytes() >= before.limit_bytes()
            );
            if !loosened {
                self.breaking(format!(
                    "body class tightened from {old_body:?} to {new_body:?}"
                ));
            }
        }
    }

    fn parameters(&mut self, old: &Value, new: &Value) {
        let find = |operation: &'_ Value, name: &str, place: &str| {
            operation["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|p| text(&p["name"]) == Some(name) && text(&p["in"]) == Some(place))
                .cloned()
        };
        for parameter in new["parameters"].as_array().into_iter().flatten() {
            let (Some(name), Some(place)) = (text(&parameter["name"]), text(&parameter["in"]))
            else {
                continue;
            };
            let required = parameter["required"].as_bool().unwrap_or(false);
            match find(old, name, place) {
                None if required => {
                    self.breaking(format!("new required {place} parameter {name}"));
                }
                Some(before) => {
                    if required && !before["required"].as_bool().unwrap_or(false) {
                        self.breaking(format!("{place} parameter {name} became required"));
                    }
                    self.schema(
                        &before["schema"],
                        &parameter["schema"],
                        Direction::Request,
                        name,
                    );
                }
                None => {}
            }
        }
    }

    fn request_body(&mut self, old: &Value, new: &Value) {
        let (before, after) = (&old["requestBody"], &new["requestBody"]);
        if after.is_null() {
            return;
        }
        if before.is_null() {
            if after["required"].as_bool().unwrap_or(false) {
                self.breaking("a required request body was added".to_owned());
            }
            return;
        }
        for (media, old_media) in entries(&before["content"]) {
            if let Some(new_media) = after["content"].get(media.as_str()) {
                self.schema(
                    &old_media["schema"],
                    &new_media["schema"],
                    Direction::Request,
                    media,
                );
            }
        }
    }

    fn responses(&mut self, old: &Value, new: &Value) {
        for (status, old_response) in entries(&old["responses"]) {
            if !status.starts_with('2') {
                continue;
            }
            let Some(new_response) = new["responses"].get(status.as_str()) else {
                self.breaking(format!("response {status} removed"));
                continue;
            };
            for (media, old_media) in entries(&old_response["content"]) {
                match new_response["content"].get(media.as_str()) {
                    None => self.breaking(format!("response {status} no longer offers {media}")),
                    Some(new_media) => self.schema(
                        &old_media["schema"],
                        &new_media["schema"],
                        Direction::Response,
                        &format!("{status} {media}"),
                    ),
                }
            }
        }
    }

    fn resolve<'v>(&self, document: &'v Value, schema: &'v Value) -> (&'v Value, String) {
        match text(&schema["$ref"]).and_then(|r| r.strip_prefix(SCHEMA_REF_PREFIX)) {
            Some(name) => (&document["components"]["schemas"][name], name.to_owned()),
            None => (schema, String::new()),
        }
    }

    fn schema(&mut self, old: &Value, new: &Value, direction: Direction, at: &str) {
        let (old_schema, old_name) = self.resolve(self.old, old);
        let (new_schema, new_name) = self.resolve(self.new, new);
        let request = direction == Direction::Request;
        if !old_name.is_empty() && !self.visited.insert((old_name, new_name, request)) {
            return;
        }
        if old_schema.is_null() || new_schema.is_null() {
            return;
        }
        if let (Some(before), Some(after)) = (text(&old_schema["type"]), text(&new_schema["type"]))
            && before != after
        {
            self.breaking(format!("{at}: type changed from {before} to {after}"));
        }
        self.enum_values(old_schema, new_schema, at);
        self.properties(old_schema, new_schema, direction, at);
        if direction == Direction::Request {
            self.required(old_schema, new_schema, at);
        }
        if old_schema["items"].is_object() && new_schema["items"].is_object() {
            self.schema(
                &old_schema["items"],
                &new_schema["items"],
                direction,
                &format!("{at}[]"),
            );
        }
        for combinator in ["allOf", "anyOf", "oneOf"] {
            let pairs = old_schema[combinator]
                .as_array()
                .into_iter()
                .flatten()
                .zip(new_schema[combinator].as_array().into_iter().flatten());
            for (before, after) in pairs {
                self.schema(before, after, direction, at);
            }
        }
    }

    fn enum_values(&mut self, old: &Value, new: &Value, at: &str) {
        let (Some(before), Some(after)) = (old["enum"].as_array(), new["enum"].as_array()) else {
            return;
        };
        for value in before.iter().filter(|value| !after.contains(value)) {
            self.breaking(format!("{at}: enum value {value} removed"));
        }
    }

    fn properties(&mut self, old: &Value, new: &Value, direction: Direction, at: &str) {
        for (name, before) in entries(&old["properties"]) {
            match new["properties"].get(name.as_str()) {
                None => self.breaking(format!("{at}: field {name} removed")),
                Some(after) => self.schema(before, after, direction, &format!("{at}.{name}")),
            }
        }
    }

    fn required(&mut self, old: &Value, new: &Value, at: &str) {
        let before: BTreeSet<&str> = old["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(text)
            .collect();
        for name in new["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(text)
        {
            if !before.contains(name) {
                self.breaking(format!("{at}: request field {name} became required"));
            }
        }
    }
}
