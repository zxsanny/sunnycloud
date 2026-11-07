//! A structural OpenAPI 3.1 check of the subset the generator emits. The workspace has no OpenAPI validator
//! crate, so this checks the rules a client generator relies on; it is tested against broken documents below.

use std::collections::BTreeSet;

use serde_json::Value;

const METHODS: [&str; 6] = ["get", "head", "post", "put", "patch", "delete"];

pub fn validate_document(document: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    match document["openapi"].as_str() {
        Some(version) if version.starts_with("3.") => {}
        other => errors.push(format!("openapi version {other:?} is not 3.x")),
    }
    for field in ["title", "version"] {
        if document["info"][field].as_str().is_none_or(str::is_empty) {
            errors.push(format!("info.{field} is missing"));
        }
    }
    let schemes: BTreeSet<&str> = document["components"]["securitySchemes"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name.as_str())
        .collect();
    let mut operation_ids = BTreeSet::new();
    let Some(paths) = document["paths"].as_object() else {
        errors.push("paths is missing".to_owned());
        return errors;
    };
    for (path, item) in paths {
        if !path.starts_with('/') {
            errors.push(format!("{path}: does not start with a slash"));
        }
        for (key, operation) in item.as_object().into_iter().flatten() {
            let label = format!("{key} {path}");
            if !METHODS.contains(&key.as_str()) {
                errors.push(format!("{label}: not an HTTP method"));
                continue;
            }
            check_operation(path, operation, &label, &schemes, &mut errors);
            match operation["operationId"].as_str() {
                Some(id) if operation_ids.insert(id) => {}
                other => errors.push(format!(
                    "{label}: operationId {other:?} missing or repeated"
                )),
            }
        }
    }
    check_refs(document, document, "#", &mut errors);
    errors
}

fn check_operation(
    path: &str,
    operation: &Value,
    label: &str,
    schemes: &BTreeSet<&str>,
    errors: &mut Vec<String>,
) {
    match operation["responses"].as_object() {
        Some(responses) if !responses.is_empty() => {
            for (status, response) in responses {
                let known = status == "default"
                    || status.parse::<u16>().is_ok_and(|s| (100..600).contains(&s));
                if !known {
                    errors.push(format!("{label}: response key {status} is not a status"));
                }
                if response["description"].as_str().is_none() {
                    errors.push(format!("{label}: response {status} has no description"));
                }
            }
        }
        _ => errors.push(format!("{label}: no responses")),
    }
    let mut declared = BTreeSet::new();
    for parameter in operation["parameters"].as_array().into_iter().flatten() {
        let (name, place) = (parameter["name"].as_str(), parameter["in"].as_str());
        let (Some(name), Some(place)) = (name, place) else {
            errors.push(format!("{label}: parameter without name or location"));
            continue;
        };
        if !["path", "query", "header", "cookie"].contains(&place) {
            errors.push(format!("{label}: parameter {name} is in {place}"));
        }
        if !declared.insert((name, place)) {
            errors.push(format!("{label}: parameter {name} repeated"));
        }
        if place == "path" && parameter["required"] != true {
            errors.push(format!("{label}: path parameter {name} is not required"));
        }
        if parameter["schema"].is_null() {
            errors.push(format!("{label}: parameter {name} has no schema"));
        }
    }
    for segment in path.split('/') {
        if let Some(name) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}'))
            && !declared.contains(&(name, "path"))
        {
            errors.push(format!("{label}: path variable {name} has no parameter"));
        }
    }
    for requirement in operation["security"].as_array().into_iter().flatten() {
        for scheme in requirement
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, _)| name)
        {
            if !schemes.contains(scheme.as_str()) {
                errors.push(format!("{label}: security scheme {scheme} is not defined"));
            }
        }
    }
    if let Some(body) = operation.get("requestBody") {
        let content = body["content"].as_object();
        if content.is_none_or(|media| media.is_empty()) {
            errors.push(format!("{label}: requestBody has no content"));
        }
    }
}

fn check_refs(root: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(target) = map.get("$ref").and_then(Value::as_str) {
                let resolved = target
                    .strip_prefix('#')
                    .and_then(|pointer| root.pointer(pointer));
                if resolved.is_none() {
                    errors.push(format!("{at}: $ref {target} does not resolve"));
                }
            }
            for (key, child) in map {
                check_refs(root, child, &format!("{at}/{key}"), errors);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                check_refs(root, child, &format!("{at}/{index}"), errors);
            }
        }
        _ => {}
    }
}

#[test]
fn the_validator_rejects_each_kind_of_broken_document() {
    // Arrange
    let good = serde_json::json!({
        "openapi": "3.1.0",
        "info": {"title": "t", "version": "1"},
        "paths": {"/a/{id}": {"get": {
            "operationId": "get_a",
            "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
            "responses": {"200": {"description": "ok"}},
            "security": [{"bearerWorker": []}]
        }}},
        "components": {"securitySchemes": {"bearerWorker": {"type": "http", "scheme": "bearer"}}}
    });
    let broken = |edit: fn(&mut Value)| {
        let mut document = good.clone();
        edit(&mut document);
        validate_document(&document)
    };

    // Act
    let outcomes = [
        ("good", validate_document(&good)),
        ("version", broken(|d| d["openapi"] = "2.0".into())),
        (
            "no variable parameter",
            broken(|d| d["paths"]["/a/{id}"]["get"]["parameters"] = serde_json::json!([])),
        ),
        (
            "no responses",
            broken(|d| d["paths"]["/a/{id}"]["get"]["responses"] = serde_json::json!({})),
        ),
        (
            "unknown scheme",
            broken(|d| {
                d["paths"]["/a/{id}"]["get"]["security"] = serde_json::json!([{"nope": []}])
            }),
        ),
        (
            "dangling ref",
            broken(|d| {
                d["paths"]["/a/{id}"]["get"]["responses"]["200"]["x"] =
                    serde_json::json!({"$ref": "#/components/schemas/Gone"})
            }),
        ),
        (
            "repeated id",
            broken(|d| {
                let operation = d["paths"]["/a/{id}"]["get"].clone();
                d["paths"]["/b"] = serde_json::json!({"get": operation});
            }),
        ),
    ];

    // Assert
    assert!(outcomes[0].1.is_empty());
    for (case, errors) in &outcomes[1..] {
        assert!(!errors.is_empty(), "{case} was accepted");
    }
}
