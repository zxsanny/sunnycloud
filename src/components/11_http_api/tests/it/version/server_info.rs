use serde_json::{Value, json};
use sunnycloud_http_api::current_document;

use super::support::{Gated, SERVER_INFO, gated_config, get};
use crate::harness::Server;

/// The subset of JSON Schema the `ServerInfo` component uses: object, required, properties, type, const
/// and array items.
fn schema_errors(schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
    if let Some(expected) = schema.get("const")
        && expected != value
    {
        errors.push(format!("{at}: {value} is not {expected}"));
    }
    let type_ok = match schema["type"].as_str() {
        Some("object") => value.is_object(),
        Some("integer") => value.is_u64() || value.is_i64(),
        Some("string") => value.is_string(),
        Some("array") => value.is_array(),
        _ => true,
    };
    if !type_ok {
        errors.push(format!("{at}: {value} is not a {}", schema["type"]));
    }
    for name in schema["required"].as_array().into_iter().flatten() {
        let name = name.as_str().unwrap_or_default();
        if value.get(name).is_none() {
            errors.push(format!("{at}.{name}: missing"));
        }
    }
    for (name, property) in schema["properties"].as_object().into_iter().flatten() {
        if let Some(inner) = value.get(name) {
            schema_errors(property, inner, &format!("{at}.{name}"), errors);
        }
    }
    for (index, item) in value.as_array().into_iter().flatten().enumerate() {
        schema_errors(&schema["items"], item, &format!("{at}[{index}]"), errors);
    }
}

fn server_info_schema() -> Value {
    let document: Value =
        serde_json::from_slice(current_document().as_bytes()).expect("the document is JSON");
    document["components"]["schemas"]["ServerInfo"].clone()
}

#[tokio::test]
async fn server_info_has_exactly_the_eight_fields_for_any_client_and_no_credentials() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;
    let schema = server_info_schema();

    // Act
    let anonymous = get(server.public.addr, SERVER_INFO, &[]).await;
    let web = get(
        server.public.addr,
        SERVER_INFO,
        &[("x-sunnycloud-client", "web/0.1.0")],
    )
    .await;

    // Assert
    let expected = json!({
        "block_size": 2_097_152,
        "max_lanes": 3,
        "small_file_max": 2_097_152,
        "batch_max_entries": 250,
        "max_staged_per_account": 7,
        "longpoll_max_wait": 20,
        "min_client_version": "1.2.0",
        "api_versions": ["v1"],
    });
    for reply in [&anonymous, &web] {
        assert_eq!(reply.status, 200);
        assert_eq!(reply.json(), expected);
        assert_eq!(reply.header("cache-control"), Some("no-store"));
        assert_eq!(reply.header("content-type"), Some("application/json"));
        let mut errors = Vec::new();
        schema_errors(&schema, &reply.json(), "$", &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
    }
    assert!(server.authenticator.calls().is_empty());
    server.stop().await;
}

#[test]
fn the_schema_check_rejects_a_body_that_breaks_the_schema() {
    // Arrange
    let schema = server_info_schema();
    let broken = json!({"block_size": 1, "max_lanes": "3", "api_versions": [1]});

    // Act
    let mut errors = Vec::new();
    schema_errors(&schema, &broken, "$", &mut errors);

    // Assert
    assert!(errors.iter().any(|e| e.contains("block_size")));
    assert!(errors.iter().any(|e| e.contains("max_lanes")));
    assert!(errors.iter().any(|e| e.contains("api_versions[0]")));
    assert!(
        errors
            .iter()
            .any(|e| e.contains("longpoll_max_wait: missing"))
    );
}

#[tokio::test]
async fn server_info_names_no_provider_and_no_setup_flag() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;

    // Act
    let body = get(server.public.addr, SERVER_INFO, &[]).await.json();

    // Assert
    let keys: Vec<&String> = body.as_object().expect("object").keys().collect();
    assert_eq!(keys.len(), 8);
    assert!(
        keys.iter()
            .all(|k| !k.contains("provider") && !k.contains("setup"))
    );
    server.stop().await;
}

#[tokio::test]
async fn the_running_server_serves_server_info_to_an_old_client_with_the_configured_values() {
    // Arrange
    let server = Server::start(gated_config("1.2.0")).await;

    // Act
    let reply = get(
        server.public,
        SERVER_INFO,
        &[("x-sunnycloud-client", "ios/0.9.0")],
    )
    .await;

    // Assert
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["max_lanes"], 3);
    assert_eq!(reply.json()["min_client_version"], "1.2.0");
    assert_eq!(reply.header("cache-control"), Some("no-store"));
    server.stop().await.expect("run returns");
}
