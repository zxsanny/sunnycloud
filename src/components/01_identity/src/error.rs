use sc_config::KeyProblem;
use thiserror::Error;

/// Failures of the identity component that are not port outcomes.
#[derive(Debug, Error)]
pub enum IdentityError {
    /// The identity section failed its start validation; every problem is listed.
    #[error("invalid identity configuration: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Config(Vec<KeyProblem>),
    /// A stored value outside the set its CHECK constraint allows; a bug or a damaged database.
    #[error("unexpected value {value:?} in column {column}")]
    UnexpectedColumnValue { column: &'static str, value: String },
}
