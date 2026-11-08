use std::time::Duration;

use axum::body::Bytes;
use sc_ids::BLOCK_SIZE;
use serde_json::{Value, json};

use crate::registry::BodyClass;

/// The `/api/vN` families this build serves.
const API_VERSIONS: [&str; 1] = ["v1"];

/// The `ServerInfo` value built by 12 from the typed config and served at `GET /api/v1/server-info`
/// (`server_info.md`). `block_size`, `small_file_max` and `api_versions` are protocol constants of this
/// build, not inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// Config key `max_lanes`.
    pub max_lanes: u64,
    /// Config key `batch_max_entries`.
    pub batch_max_entries: u64,
    /// Config key `max_staged`.
    pub max_staged_per_account: u64,
    /// Config key `longpoll_wait_max`; served in whole seconds, rounded down.
    pub longpoll_max_wait: Duration,
    /// The configured minimum as written (one semver, or `family=semver,...`).
    pub min_client_version: String,
}

impl ServerInfo {
    fn block_size() -> u64 {
        u64::try_from(BLOCK_SIZE).unwrap_or(u64::MAX)
    }

    fn small_file_max() -> u64 {
        BodyClass::Staged.limit_bytes()
    }

    /// The JSON body of the route: exactly the eight fields of the contract.
    pub(crate) fn to_body(&self) -> Bytes {
        let value = json!({
            "block_size": Self::block_size(),
            "max_lanes": self.max_lanes,
            "small_file_max": Self::small_file_max(),
            "batch_max_entries": self.batch_max_entries,
            "max_staged_per_account": self.max_staged_per_account,
            "longpoll_max_wait": self.longpoll_max_wait.as_secs(),
            "min_client_version": self.min_client_version,
            "api_versions": API_VERSIONS,
        });
        Bytes::from(value.to_string().into_bytes())
    }

    /// The `ServerInfo` schema of the OpenAPI components.
    pub(crate) fn schema() -> Value {
        json!({
            "type": "object",
            "required": [
                "block_size", "max_lanes", "small_file_max", "batch_max_entries",
                "max_staged_per_account", "longpoll_max_wait", "min_client_version", "api_versions"
            ],
            "properties": {
                "block_size": {
                    "type": "integer",
                    "const": Self::block_size(),
                    "description": "Bytes; a protocol constant of /api/v1."
                },
                "max_lanes": {"type": "integer", "minimum": 0},
                "small_file_max": {
                    "type": "integer",
                    "const": Self::small_file_max(),
                    "description": "Bytes; the staged-upload cap."
                },
                "batch_max_entries": {"type": "integer", "minimum": 0},
                "max_staged_per_account": {"type": "integer", "minimum": 0},
                "longpoll_max_wait": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Seconds."
                },
                "min_client_version": {
                    "type": "string",
                    "description": "One MAJOR.MINOR.PATCH for every client family, or family=version,... per family."
                },
                "api_versions": {"type": "array", "items": {"type": "string"}}
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ServerInfo {
        ServerInfo {
            max_lanes: 3,
            batch_max_entries: 250,
            max_staged_per_account: 7,
            longpoll_max_wait: Duration::from_secs(20),
            min_client_version: "1.2.0".to_owned(),
        }
    }

    #[test]
    fn the_body_has_exactly_the_eight_fields_with_the_configured_values() {
        // Arrange
        let info = info();

        // Act
        let body: Value = serde_json::from_slice(&info.to_body()).expect("json body");

        // Assert
        assert_eq!(
            body,
            json!({
                "block_size": 2_097_152,
                "max_lanes": 3,
                "small_file_max": 2_097_152,
                "batch_max_entries": 250,
                "max_staged_per_account": 7,
                "longpoll_max_wait": 20,
                "min_client_version": "1.2.0",
                "api_versions": ["v1"],
            })
        );
    }

    #[test]
    fn a_sub_second_remainder_of_the_wait_is_not_advertised() {
        // Arrange
        let info = ServerInfo {
            longpoll_max_wait: Duration::from_millis(20_900),
            ..info()
        };

        // Act
        let body: Value = serde_json::from_slice(&info.to_body()).expect("json body");

        // Assert
        assert_eq!(body["longpoll_max_wait"], 20);
    }

    #[test]
    fn the_schema_requires_every_field_it_describes() {
        // Arrange
        let schema = ServerInfo::schema();

        // Act
        let required = schema["required"].as_array().map(Vec::len);
        let described = schema["properties"].as_object().map(|map| map.len());

        // Assert
        assert_eq!(required, Some(8));
        assert_eq!(described, Some(8));
    }
}
