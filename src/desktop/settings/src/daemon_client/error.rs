use ipc_schema::{ControlError, Outcome, RpcError};
use serde_json::Value;

/// Everything a call can end with besides a result. `Clone` so one cause can be handed to
/// every call that was waiting when a connection broke.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    #[error("the daemon is not running")]
    DaemonNotRunning,
    /// The endpoint exists but does not belong to the current user; nothing was sent.
    #[error("endpoint untrusted: {0}")]
    EndpointUntrusted(String),
    /// A typed error of the daemon, with its fields.
    #[error("{0}")]
    Control(ControlError),
    /// A JSON-RPC error that is not a `ControlError` this build knows (for example a
    /// variant of a newer daemon). The text is data from the daemon, to be shown as plain
    /// text.
    #[error("daemon error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("no answer within the request timeout")]
    Timeout,
    #[error("the connection to the daemon was lost")]
    ConnectionLost,
    #[error("protocol violation: {0}")]
    Protocol(String),
    #[error("i/o: {0}")]
    Io(String),
    /// The client was shut down.
    #[error("the daemon client is closed")]
    Closed,
}

/// Why no endpoint path can be named for the current user.
#[derive(Debug, thiserror::Error)]
pub enum LocateError {
    #[cfg(unix)]
    #[error("XDG_RUNTIME_DIR is not set to an absolute path")]
    NoRuntimeDir,
    #[cfg(windows)]
    #[error("the current user SID cannot be read: {0}")]
    UserSid(String),
}

impl ClientError {
    pub(super) fn from_rpc(error: RpcError) -> Self {
        match error.to_control() {
            Some(control) => Self::Control(control),
            None => Self::Rpc {
                code: error.code,
                message: error.message,
            },
        }
    }

    pub(super) fn from_outcome(outcome: Outcome) -> Result<Value, Self> {
        match outcome {
            Outcome::Success { result } => Ok(result),
            Outcome::Failure { error } => Err(Self::from_rpc(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use ipc_schema::{CONTROL_ERROR_CODE, INTERNAL_ERROR_CODE};
    use serde_json::json;

    use super::*;

    fn all_variants() -> Vec<ControlError> {
        vec![
            ControlError::HttpsRequired,
            ControlError::ServerUnreachable,
            ControlError::UpgradeRequired {
                message: "update to 1.2".to_owned(),
            },
            ControlError::SignedIn,
            ControlError::NotSignedIn,
            ControlError::ServerNotSet,
            ControlError::UnknownFolder,
            ControlError::RootInvalid,
            ControlError::RootNested,
            ControlError::RulesInvalid { line: 3 },
            ControlError::CloudManagedRoot,
            ControlError::AlreadyPaired,
            ControlError::UnknownPair,
            ControlError::SettingInvalid {
                key: "bandwidth_limit".to_owned(),
            },
            ControlError::NoSuchHold,
            ControlError::Internal {
                correlation_id: "c-77".to_owned(),
            },
        ]
    }

    #[test]
    fn every_control_error_variant_is_decoded_with_its_fields() {
        for expected in all_variants() {
            let rpc = RpcError::from_control(&expected).unwrap();

            let decoded = ClientError::from_rpc(rpc);

            assert_eq!(decoded, ClientError::Control(expected));
        }
    }

    #[test]
    fn the_wire_form_of_rules_invalid_is_the_documented_one() {
        let rpc = RpcError {
            code: CONTROL_ERROR_CODE,
            message: "RulesInvalid".to_owned(),
            data: Some(json!({"kind": "RulesInvalid", "line": 3})),
        };

        let decoded = ClientError::from_rpc(rpc);

        assert_eq!(
            decoded,
            ClientError::Control(ControlError::RulesInvalid { line: 3 })
        );
    }

    #[test]
    fn an_unknown_kind_becomes_a_generic_error_with_the_daemons_text_unchanged() {
        let rpc = RpcError {
            code: CONTROL_ERROR_CODE,
            message: "<b>x</b>{@html}<script>".to_owned(),
            data: Some(json!({"kind": "FromTheFuture", "detail": 1})),
        };

        let decoded = ClientError::from_rpc(rpc);

        assert_eq!(
            decoded,
            ClientError::Rpc {
                code: CONTROL_ERROR_CODE,
                message: "<b>x</b>{@html}<script>".to_owned(),
            }
        );
    }

    #[test]
    fn a_known_kind_with_a_missing_field_is_generic_not_a_guess() {
        let rpc = RpcError {
            code: CONTROL_ERROR_CODE,
            message: "RulesInvalid".to_owned(),
            data: Some(json!({"kind": "RulesInvalid"})),
        };

        let decoded = ClientError::from_rpc(rpc);

        assert!(matches!(decoded, ClientError::Rpc { .. }));
    }

    #[test]
    fn a_json_rpc_internal_error_is_generic() {
        let rpc = RpcError::internal("boom");

        let decoded = ClientError::from_rpc(rpc);

        assert!(matches!(
            decoded,
            ClientError::Rpc {
                code: INTERNAL_ERROR_CODE,
                ..
            }
        ));
    }
}
