use sc_config::KeyProblem;
use thiserror::Error;

/// Failures of the identity component that are not port outcomes.
#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("sealing the session successor failed")]
    SuccessorSeal,
    /// The identity section failed its start validation; every problem is listed.
    #[error("invalid identity configuration: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Config(Vec<KeyProblem>),
    /// A stored value outside the set its CHECK constraint allows; a bug or a damaged database.
    #[error("unexpected value {value:?} in column {column}")]
    UnexpectedColumnValue { column: &'static str, value: String },
    /// The outbound HTTP client (TLS roots, client builder) could not be built.
    #[error("outbound HTTP client setup failed")]
    OutboundSetup(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// A configured duration added to the clock leaves the representable range.
    #[error("a configured duration is out of the clock's range")]
    TimeOutOfRange,
    /// A mobile sign-in reached the callback; the one-time code outcome belongs to a later task.
    #[error("the mobile sign-in outcome is not implemented")]
    MobileSignInUnsupported,
    /// The device grant could not draw an unused user code; the random source or the table is damaged.
    #[error("no unused user code after repeated draws")]
    UserCodeExhausted,
}
