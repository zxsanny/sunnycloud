use serde::{Deserialize, Serialize};

pub type ControlResult<T> = Result<T, ControlError>;

/// On the wire: JSON-RPC code -32000, `message` = variant name, `data` = this value
/// (`{"kind": "<Variant>", ...fields}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind")]
pub enum ControlError {
    #[error("the server address must use https")]
    HttpsRequired,
    #[error("the server is unreachable")]
    ServerUnreachable,
    #[error("the server requires a newer client: {message}")]
    UpgradeRequired { message: String },
    #[error("the server cannot be changed while signed in")]
    SignedIn,
    #[error("not signed in")]
    NotSignedIn,
    #[error("no server address is set")]
    ServerNotSet,
    #[error("unknown server folder")]
    UnknownFolder,
    #[error("the local root is missing, not a folder or not writable")]
    RootInvalid,
    #[error("the local root is nested with another pair's root")]
    RootNested,
    #[error("rules text invalid at line {line}")]
    RulesInvalid { line: u32 },
    #[error("the local root is inside a cloud-managed folder")]
    CloudManagedRoot,
    #[error("the server folder is already paired")]
    AlreadyPaired,
    #[error("unknown pair")]
    UnknownPair,
    #[error("invalid setting: {key}")]
    SettingInvalid { key: String },
    #[error("the pair is not in a hold")]
    NoSuchHold,
    #[error("internal error, correlation id {correlation_id}")]
    Internal { correlation_id: String },
}

impl ControlError {
    /// The variant name, as carried in the JSON-RPC `message` and in `data.kind`.
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::HttpsRequired => "HttpsRequired",
            Self::ServerUnreachable => "ServerUnreachable",
            Self::UpgradeRequired { .. } => "UpgradeRequired",
            Self::SignedIn => "SignedIn",
            Self::NotSignedIn => "NotSignedIn",
            Self::ServerNotSet => "ServerNotSet",
            Self::UnknownFolder => "UnknownFolder",
            Self::RootInvalid => "RootInvalid",
            Self::RootNested => "RootNested",
            Self::RulesInvalid { .. } => "RulesInvalid",
            Self::CloudManagedRoot => "CloudManagedRoot",
            Self::AlreadyPaired => "AlreadyPaired",
            Self::UnknownPair => "UnknownPair",
            Self::SettingInvalid { .. } => "SettingInvalid",
            Self::NoSuchHold => "NoSuchHold",
            Self::Internal { .. } => "Internal",
        }
    }
}
